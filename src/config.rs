use std::collections::BTreeSet;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use reqwest::Method;

use crate::cli::{Cli, ModeArg, OutputFormatArg, ScheduleArg};
use crate::engine::rate_limiter::DelayConfig;
use crate::http::raw_request::RawRequestTemplate;
use crate::input::encoder::EncoderSet;
use crate::input::wordlist::WordlistSpec;
use crate::matcher::legacy::MatcherConfig;
use crate::template::Template;

#[derive(Debug, Clone)]
pub struct Config {
    pub request: RequestConfig,
    pub input: InputConfig,
    pub matcher: MatcherConfig,
    pub output: OutputConfig,
    pub execution: ExecutionConfig,
    pub precheck: PrecheckConfig,
    pub future: FutureConfig,
}

#[derive(Debug, Clone)]
pub struct RequestConfig {
    pub method: Method,
    pub url: Option<Template>,
    pub raw_request: Option<RawRequestTemplate>,
    pub headers: Vec<(String, Template)>,
    pub cookies: Vec<Template>,
    pub body: Option<Template>,
    pub proxy: Option<String>,
    pub replay_proxy: Option<String>,
    pub timeout: Duration,
    pub follow_redirects: bool,
    pub raw_uri: bool,
    pub sni: Option<String>,
    pub http2: bool,
    pub ssl_verify: bool,
    pub keepalive: bool,
    pub dns_cache: bool,
    pub dns_cache_ttl: Duration,
    pub dns_negative_cache_ttl: Duration,
    pub dns_max_concurrent: usize,
    pub client_cert: Option<String>,
    pub client_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct InputConfig {
    pub wordlists: Vec<WordlistSpec>,
    pub mode: FuzzMode,
    pub order: Vec<String>,
    pub budget_requests: Option<usize>,
    pub extensions: Vec<String>,
    pub ignore_wordlist_comments: bool,
    pub encoders: EncoderSet,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuzzMode {
    Sniper,
    Pitchfork,
    Clusterbomb,
}

#[derive(Debug, Clone)]
pub struct OutputConfig {
    pub path: Option<String>,
    pub format: OutputFormat,
    pub silent: bool,
    pub output_directory: Option<String>,
    pub error_log: Option<String>,
    pub progress: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Console,
    Jsonl,
    Csv,
}

#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    pub concurrency: usize,
    pub rate_per_sec: Option<u64>,
    pub delay: DelayConfig,
    pub schedule: ScheduleConfig,
}

#[derive(Debug, Clone)]
pub struct ScheduleConfig {
    pub mode: ScheduleMode,
    pub target_key: Option<String>,
    pub target_window: usize,
    pub target_burst: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleMode {
    Default,
    RotateWindow,
}

#[derive(Debug, Clone)]
pub struct PrecheckConfig {
    pub enabled: bool,
    pub key: Option<String>,
    pub report_only: bool,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FutureConfig {
    // TODO(v0.2): wire these reserved fields into calibration, expression
    // matching, recursion, state persistence, and scoped stop controls.
    pub auto_calibration: AutoCalibrationConfig,
    pub dynamic_auto_filter: bool,
    pub filter_expr: Option<String>,
    pub match_expr: Option<String>,
    pub recursion: RecursionConfig,
    pub state: StateConfig,
    pub stop: StopConfig,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AutoCalibrationConfig {
    pub enabled: bool,
    pub scope: AutoCalibrationScope,
    pub ignore_keywords: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoCalibrationScope {
    Host,
    Job,
    Global,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct RecursionConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Default)]
#[allow(dead_code)]
pub struct StateConfig {
    pub save_state: Option<String>,
    pub resume: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct StopConfig {
    pub stop_on_match: Option<usize>,
    pub stop_scope: Vec<String>,
}

impl TryFrom<Cli> for Config {
    type Error = anyhow::Error;

    fn try_from(cli: Cli) -> Result<Self> {
        if cli.wordlists.is_empty() {
            return Err(anyhow!("at least one -w wordlist[:KEYWORD] is required"));
        }

        let wordlists = cli
            .wordlists
            .iter()
            .map(|spec| WordlistSpec::parse(spec))
            .collect::<Result<Vec<_>>>()?;

        validate_unique_keywords(&wordlists)?;
        let keywords = wordlists
            .iter()
            .map(|wordlist| wordlist.keyword.clone())
            .collect::<BTreeSet<_>>();
        let order = parse_keyword_order(cli.order.as_deref(), &wordlists)?;
        if !order.is_empty() && cli.mode != ModeArg::Clusterbomb {
            return Err(anyhow!("-order can only be used with -mode clusterbomb"));
        }

        if cli.url.is_none() && cli.request.is_none() {
            return Err(anyhow!(
                "either -u URL or -request raw request file is required"
            ));
        }

        let matcher = MatcherConfig::from_cli(&cli)?;

        let url = cli
            .url
            .as_ref()
            .map(|url| Template::compile(url, &keywords).context("invalid URL template"))
            .transpose()?;
        let raw_request = cli
            .request
            .as_ref()
            .map(|path| {
                RawRequestTemplate::from_burp_file(path, &cli.request_proto, &keywords)
                    .context("invalid raw request file")
            })
            .transpose()?;
        let headers = cli
            .headers
            .iter()
            .map(|header| compile_header(header, &keywords))
            .collect::<Result<Vec<_>>>()?;
        let cookies = cli
            .cookies
            .iter()
            .map(|cookie| Template::compile(cookie, &keywords).context("invalid cookie template"))
            .collect::<Result<Vec<_>>>()?;
        let body = cli
            .data
            .as_ref()
            .map(|body| Template::compile(body, &keywords).context("invalid body template"))
            .transpose()?;
        let stop_scope = parse_keyword_list(&cli.stop_scope);
        validate_keywords_exist("-stop-scope", &stop_scope, &keywords)?;
        let precheck = PrecheckConfig {
            enabled: parse_precheck_switch(&cli.precheck)?,
            key: cli.precheck_key.clone(),
            report_only: cli.precheck_report_only,
        };
        if let Some(key) = &precheck.key {
            validate_keywords_exist("-precheck-key", std::slice::from_ref(key), &keywords)?;
        }
        let schedule = parse_schedule(&cli, &wordlists, &keywords, &stop_scope, &precheck)?;

        let request = RequestConfig {
            method: Method::from_bytes(cli.method.as_bytes())
                .with_context(|| format!("invalid HTTP method: {}", cli.method))?,
            url,
            raw_request,
            headers,
            cookies,
            body,
            proxy: cli.proxy,
            replay_proxy: cli.replay_proxy,
            timeout: Duration::from_secs(cli.timeout_secs),
            follow_redirects: cli.follow_redirects,
            raw_uri: cli.raw_uri,
            sni: cli.sni,
            http2: cli.http2,
            ssl_verify: parse_on_off_switch("-ssl-verify", &cli.ssl_verify)?,
            keepalive: parse_on_off_switch("-keepalive", &cli.keepalive)?,
            dns_cache: parse_on_off_switch("-dns-cache", &cli.dns_cache)?,
            dns_cache_ttl: Duration::from_secs(cli.dns_cache_ttl_secs.max(1)),
            dns_negative_cache_ttl: Duration::from_secs(cli.dns_negative_cache_ttl_secs),
            dns_max_concurrent: cli.dns_max_concurrent.max(1),
            client_cert: cli.client_cert,
            client_key: cli.client_key,
        };

        let future = FutureConfig {
            auto_calibration: AutoCalibrationConfig {
                enabled: cli.auto_calibration,
                scope: parse_ac_scope(&cli.auto_calibration_scope)?,
                ignore_keywords: cli.auto_calibration_ignore,
            },
            dynamic_auto_filter: false,
            filter_expr: None,
            match_expr: None,
            recursion: RecursionConfig::default(),
            state: StateConfig::default(),
            stop: StopConfig {
                stop_on_match: cli.stop_on_match,
                stop_scope,
            },
        };

        Ok(Self {
            request,
            input: InputConfig {
                wordlists,
                mode: match cli.mode {
                    ModeArg::Sniper => FuzzMode::Sniper,
                    ModeArg::Pitchfork => FuzzMode::Pitchfork,
                    ModeArg::Clusterbomb => FuzzMode::Clusterbomb,
                },
                order,
                budget_requests: cli.budget_requests,
                extensions: parse_extensions(cli.extensions.as_deref()),
                ignore_wordlist_comments: cli.ignore_wordlist_comments,
                encoders: EncoderSet::parse(&cli.encoders)?,
            },
            matcher,
            output: OutputConfig {
                path: cli.output,
                format: match cli.output_format {
                    OutputFormatArg::Console => OutputFormat::Console,
                    OutputFormatArg::Jsonl => OutputFormat::Jsonl,
                    OutputFormatArg::Csv => OutputFormat::Csv,
                },
                silent: cli.silent,
                output_directory: cli.output_directory,
                error_log: cli.error_log,
                progress: !cli.no_progress,
            },
            execution: ExecutionConfig {
                concurrency: cli.concurrency.max(1),
                rate_per_sec: (cli.rate > 0).then_some(cli.rate),
                delay: DelayConfig::parse(cli.delay.as_deref())?,
                schedule,
            },
            precheck,
            future,
        })
    }
}

fn parse_extensions(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            if value.starts_with('.') {
                value.to_string()
            } else {
                format!(".{}", value)
            }
        })
        .collect()
}

fn parse_keyword_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_keyword_order(raw: Option<&str>, wordlists: &[WordlistSpec]) -> Result<Vec<String>> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };
    let order = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if order.is_empty() {
        return Err(anyhow!("-order must include at least one keyword"));
    }

    let expected = wordlists
        .iter()
        .map(|wordlist| wordlist.keyword.clone())
        .collect::<BTreeSet<_>>();
    validate_keywords_exist("-order", &order, &expected)?;

    let mut seen = BTreeSet::new();
    for keyword in &order {
        if !seen.insert(keyword.clone()) {
            return Err(anyhow!("-order contains duplicate keyword: {}", keyword));
        }
    }
    if seen != expected {
        let missing = expected
            .difference(&seen)
            .cloned()
            .collect::<Vec<_>>()
            .join(",");
        return Err(anyhow!(
            "-order must include every wordlist keyword; missing: {missing}"
        ));
    }

    Ok(order)
}

fn parse_schedule(
    cli: &Cli,
    wordlists: &[WordlistSpec],
    keywords: &BTreeSet<String>,
    stop_scope: &[String],
    precheck: &PrecheckConfig,
) -> Result<ScheduleConfig> {
    let mode = match cli.schedule {
        ScheduleArg::Default => ScheduleMode::Default,
        ScheduleArg::RotateWindow => ScheduleMode::RotateWindow,
    };
    if mode == ScheduleMode::RotateWindow && cli.mode != ModeArg::Clusterbomb {
        return Err(anyhow!(
            "-schedule rotate-window can only be used with -mode clusterbomb"
        ));
    }
    let target_key = if mode == ScheduleMode::RotateWindow {
        let inferred = cli
            .target_key
            .clone()
            .or_else(|| precheck.key.clone())
            .or_else(|| stop_scope.first().cloned())
            .or_else(|| wordlists.first().map(|wordlist| wordlist.keyword.clone()));
        if let Some(key) = &inferred {
            validate_keywords_exist("-target-key", std::slice::from_ref(key), keywords)?;
        }
        inferred
    } else {
        if let Some(key) = &cli.target_key {
            validate_keywords_exist("-target-key", std::slice::from_ref(key), keywords)?;
        }
        cli.target_key.clone()
    };

    Ok(ScheduleConfig {
        mode,
        target_key,
        target_window: cli.target_window.max(1),
        target_burst: cli.target_burst.max(1),
    })
}

fn validate_keywords_exist(
    option: &str,
    values: &[String],
    keywords: &BTreeSet<String>,
) -> Result<()> {
    for value in values {
        if !keywords.contains(value) {
            return Err(anyhow!("{} references unknown keyword: {}", option, value));
        }
    }
    Ok(())
}

fn parse_precheck_switch(raw: &str) -> Result<bool> {
    parse_on_off_switch("-precheck", raw)
}

fn parse_on_off_switch(option: &str, raw: &str) -> Result<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "1" => Ok(true),
        "off" | "false" | "0" => Ok(false),
        other => Err(anyhow!("{} only accepts on/off, got: {}", option, other)),
    }
}

fn validate_unique_keywords(wordlists: &[WordlistSpec]) -> Result<()> {
    let mut seen = BTreeSet::new();
    for wordlist in wordlists {
        if !seen.insert(wordlist.keyword.clone()) {
            return Err(anyhow!("duplicate wordlist keyword: {}", wordlist.keyword));
        }
    }
    Ok(())
}

fn compile_header(raw: &str, keywords: &BTreeSet<String>) -> Result<(String, Template)> {
    let (name, value) = raw
        .split_once(':')
        .ok_or_else(|| anyhow!("header must use 'Name: value' syntax: {}", raw))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("header name cannot be empty"));
    }
    Ok((
        name.to_string(),
        Template::compile(value.trim(), keywords).context("invalid header template")?,
    ))
}

fn parse_ac_scope(raw: &str) -> Result<AutoCalibrationScope> {
    match raw {
        "host" => Ok(AutoCalibrationScope::Host),
        "job" => Ok(AutoCalibrationScope::Job),
        "global" => Ok(AutoCalibrationScope::Global),
        other => Err(anyhow!("invalid -ac-scope value: {}", other)),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::cli::Cli;

    #[test]
    fn parses_complete_keyword_order() {
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            "https://URLFUZZ/login",
            "-w",
            "urls.txt:URLFUZZ",
            "-w",
            "users.txt:UFUZZ",
            "-w",
            "passes.txt:PFUZZ",
            "--order",
            "UFUZZ,PFUZZ,URLFUZZ",
        ]);

        let config = Config::try_from(cli).unwrap();

        assert_eq!(config.input.order, vec!["UFUZZ", "PFUZZ", "URLFUZZ"]);
    }

    #[test]
    fn rejects_keyword_order_missing_wordlist_keyword() {
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            "https://URLFUZZ/login",
            "-w",
            "urls.txt:URLFUZZ",
            "-w",
            "users.txt:UFUZZ",
            "-w",
            "passes.txt:PFUZZ",
            "--order",
            "UFUZZ,URLFUZZ",
        ]);

        let error = Config::try_from(cli).unwrap_err().to_string();

        assert!(error.contains("missing: PFUZZ"));
    }

    #[test]
    fn parses_precheck_options() {
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            "https://URLFUZZ/login",
            "-w",
            "urls.txt:URLFUZZ",
            "--precheck",
            "off",
            "--precheck-key",
            "URLFUZZ",
            "--precheck-report-only",
        ]);

        let config = Config::try_from(cli).unwrap();

        assert!(!config.precheck.enabled);
        assert_eq!(config.precheck.key.as_deref(), Some("URLFUZZ"));
        assert!(config.precheck.report_only);
    }

    #[test]
    fn rejects_unknown_precheck_key() {
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            "https://URLFUZZ/login",
            "-w",
            "urls.txt:URLFUZZ",
            "--precheck-key",
            "MISSING",
        ]);

        let error = Config::try_from(cli).unwrap_err().to_string();

        assert!(error.contains("-precheck-key references unknown keyword: MISSING"));
    }

    #[test]
    fn disables_ssl_verification_by_default() {
        let cli = Cli::parse_from(["rfuzz", "-u", "https://FUZZ/login", "-w", "urls.txt:FUZZ"]);

        let config = Config::try_from(cli).unwrap();

        assert!(!config.request.ssl_verify);
    }

    #[test]
    fn parses_ssl_verification_on() {
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            "https://FUZZ/login",
            "-w",
            "urls.txt:FUZZ",
            "--ssl-verify",
            "on",
        ]);

        let config = Config::try_from(cli).unwrap();

        assert!(config.request.ssl_verify);
    }

    #[test]
    fn rejects_invalid_ssl_verify_value() {
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            "https://FUZZ/login",
            "-w",
            "urls.txt:FUZZ",
            "--ssl-verify",
            "maybe",
        ]);

        let error = Config::try_from(cli).unwrap_err().to_string();

        assert!(error.contains("-ssl-verify only accepts on/off"));
    }

    #[test]
    fn enables_keepalive_by_default() {
        let cli = Cli::parse_from(["rfuzz", "-u", "https://FUZZ/login", "-w", "urls.txt:FUZZ"]);

        let config = Config::try_from(cli).unwrap();

        assert!(config.request.keepalive);
    }

    #[test]
    fn parses_keepalive_off() {
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            "https://FUZZ/login",
            "-w",
            "urls.txt:FUZZ",
            "--keepalive",
            "off",
        ]);

        let config = Config::try_from(cli).unwrap();

        assert!(!config.request.keepalive);
    }

    #[test]
    fn rejects_invalid_keepalive_value() {
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            "https://FUZZ/login",
            "-w",
            "urls.txt:FUZZ",
            "--keepalive",
            "maybe",
        ]);

        let error = Config::try_from(cli).unwrap_err().to_string();

        assert!(error.contains("-keepalive only accepts on/off"));
    }

    #[test]
    fn parses_rotate_window_schedule_and_infers_target_key_from_precheck() {
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            "URLFUZZ/login",
            "-w",
            "urls.txt:URLFUZZ",
            "-w",
            "users.txt:UFUZZ",
            "-w",
            "passes.txt:PFUZZ",
            "--precheck-key",
            "URLFUZZ",
            "--schedule",
            "rotate-window",
            "--target-window",
            "50",
            "--target-burst",
            "4",
        ]);

        let config = Config::try_from(cli).unwrap();

        assert_eq!(config.execution.schedule.mode, ScheduleMode::RotateWindow);
        assert_eq!(
            config.execution.schedule.target_key.as_deref(),
            Some("URLFUZZ")
        );
        assert_eq!(config.execution.schedule.target_window, 50);
        assert_eq!(config.execution.schedule.target_burst, 4);
    }

    #[test]
    fn allows_rotate_window_schedule_with_order() {
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            "URLFUZZ/login",
            "-w",
            "urls.txt:URLFUZZ",
            "-w",
            "users.txt:UFUZZ",
            "-w",
            "passes.txt:PFUZZ",
            "--order",
            "UFUZZ,PFUZZ,URLFUZZ",
            "--precheck-key",
            "URLFUZZ",
            "--schedule",
            "rotate-window",
        ]);

        let config = Config::try_from(cli).unwrap();

        assert_eq!(config.execution.schedule.mode, ScheduleMode::RotateWindow);
        assert_eq!(config.input.order, vec!["UFUZZ", "PFUZZ", "URLFUZZ"]);
        assert_eq!(
            config.execution.schedule.target_key.as_deref(),
            Some("URLFUZZ")
        );
    }

    #[test]
    fn rejects_rotate_window_unknown_target_key() {
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            "URLFUZZ/login",
            "-w",
            "urls.txt:URLFUZZ",
            "--schedule",
            "rotate-window",
            "--target-key",
            "MISSING",
        ]);

        let error = Config::try_from(cli).unwrap_err().to_string();

        assert!(error.contains("-target-key references unknown keyword: MISSING"));
    }

    #[test]
    fn enables_dns_cache_by_default() {
        let cli = Cli::parse_from(["rfuzz", "-u", "https://FUZZ/login", "-w", "urls.txt:FUZZ"]);

        let config = Config::try_from(cli).unwrap();

        assert!(config.request.dns_cache);
        assert_eq!(config.request.dns_cache_ttl, Duration::from_secs(300));
        assert_eq!(
            config.request.dns_negative_cache_ttl,
            Duration::from_secs(30)
        );
        assert_eq!(config.request.dns_max_concurrent, 64);
    }

    #[test]
    fn parses_dns_cache_options() {
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            "https://FUZZ/login",
            "-w",
            "urls.txt:FUZZ",
            "--dns-cache",
            "off",
            "--dns-cache-ttl",
            "60",
            "--dns-negative-cache-ttl",
            "5",
            "--dns-max-concurrent",
            "8",
        ]);

        let config = Config::try_from(cli).unwrap();

        assert!(!config.request.dns_cache);
        assert_eq!(config.request.dns_cache_ttl, Duration::from_secs(60));
        assert_eq!(
            config.request.dns_negative_cache_ttl,
            Duration::from_secs(5)
        );
        assert_eq!(config.request.dns_max_concurrent, 8);
    }

    #[test]
    fn fd_limit_plan_caps_concurrency_when_soft_limit_is_low() {
        let plan = crate::runtime_limits::FdLimitPlan::from_soft_limit(100, 256);

        assert_eq!(plan.requested_concurrency, 100);
        assert_eq!(plan.effective_concurrency, 32);
        assert_eq!(plan.required_soft_limit, 528);
        assert!(plan.adjusted());
    }

    #[test]
    fn fd_limit_plan_keeps_concurrency_when_soft_limit_is_enough() {
        let plan = crate::runtime_limits::FdLimitPlan::from_soft_limit(100, 1024);

        assert_eq!(plan.effective_concurrency, 100);
        assert_eq!(plan.required_soft_limit, 528);
        assert!(!plan.adjusted());
    }

    #[test]
    fn fd_limit_plan_keeps_at_least_one_worker_for_tiny_limits() {
        let plan = crate::runtime_limits::FdLimitPlan::from_soft_limit(100, 64);

        assert_eq!(plan.effective_concurrency, 1);
        assert!(plan.adjusted());
    }
}
