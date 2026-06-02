use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use reqwest::Client;
use tokio::task::JoinSet;

use crate::config::Config;
use crate::engine::rate_limiter::RateLimiter;
use crate::input::modes::InputCase;
use crate::input::wordlist::WordlistData;
use crate::template::render::InputMap;

const PRECHECK_ATTEMPTS: usize = 3;
const PRECHECK_RETRY_DELAY_MS: u64 = 150;

#[derive(Debug)]
pub struct PrecheckSkipper {
    key: String,
    failed_values: HashSet<String>,
}

impl PrecheckSkipper {
    pub fn new(key: String, failed_values: HashSet<String>) -> Self {
        Self { key, failed_values }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn failed_values(&self) -> &HashSet<String> {
        &self.failed_values
    }

    pub fn can_fast_skip_scope(&self, scope_keywords: &[String]) -> bool {
        scope_keywords.iter().any(|keyword| keyword == &self.key)
    }

    pub fn should_skip(&self, input: &InputCase) -> bool {
        input
            .values
            .get(&self.key)
            .is_some_and(|value| self.failed_values.contains(value))
    }
}

pub async fn run(
    config: &Config,
    wordlists: &[WordlistData],
    client: Client,
) -> Result<Option<Arc<PrecheckSkipper>>> {
    if !config.precheck.enabled {
        return Ok(None);
    }
    let Some(key) = resolve_precheck_key(config) else {
        return Ok(None);
    };
    let Some(wordlist) = wordlists.iter().find(|wordlist| wordlist.keyword == key) else {
        return Ok(None);
    };
    if config.request.url.is_none() {
        return Ok(None);
    }

    let failed_values = run_url_precheck(config, wordlists, wordlist, &client).await?;
    if failed_values.is_empty() || config.precheck.report_only {
        Ok(None)
    } else {
        Ok(Some(Arc::new(PrecheckSkipper::new(key, failed_values))))
    }
}

async fn run_url_precheck(
    config: &Config,
    wordlists: &[WordlistData],
    wordlist: &WordlistData,
    client: &Client,
) -> Result<HashSet<String>> {
    let key = &wordlist.keyword;
    let limiter = RateLimiter::new(config.execution.rate_per_sec);
    let mut failed_values = HashSet::new();
    let mut join_set = JoinSet::new();
    let mut values = wordlist.values.iter();
    let wordlists = Arc::new(wordlists.to_vec());

    while join_set.len() < config.execution.concurrency {
        let Some(value) = values.next() else {
            break;
        };
        spawn_precheck_case(
            &mut join_set,
            config,
            wordlists.clone(),
            key,
            value,
            client.clone(),
            limiter.clone(),
        );
    }

    while let Some(result) = join_set.join_next().await {
        if let Some((value, message)) = result?? {
            eprintln!("{}", message);
            failed_values.insert(value);
        }
        if let Some(value) = values.next() {
            spawn_precheck_case(
                &mut join_set,
                config,
                wordlists.clone(),
                key,
                value,
                client.clone(),
                limiter.clone(),
            );
        }
    }

    Ok(failed_values)
}

fn spawn_precheck_case(
    join_set: &mut JoinSet<Result<Option<(String, String)>>>,
    config: &Config,
    wordlists: Arc<Vec<WordlistData>>,
    key: &str,
    value: &str,
    client: Client,
    limiter: RateLimiter,
) {
    let Some(template) = config.request.url.clone() else {
        return;
    };
    let key = key.to_string();
    let value = value.to_string();
    join_set.spawn(async move {
        limiter.wait().await;
        let input = precheck_input_values(&wordlists, &key, &value);
        let rendered = template.render(&input)?;
        let candidates = candidate_urls(&rendered);
        let mut failures = Vec::new();
        for url in &candidates {
            match check_url(&client, url).await {
                Ok(()) => return Ok(None),
                Err(error) => failures.push(format!("{}: {}", url, brief_error(&error))),
            }
        }
        Ok(Some((
            value.clone(),
            format!("PRECHECK ERROR {}={} {}", key, value, failures.join("; ")),
        )))
    });
}

async fn check_url(client: &Client, url: &str) -> Result<()> {
    for attempt in 1..=PRECHECK_ATTEMPTS {
        match client.get(url).send().await {
            Ok(_) => return Ok(()),
            Err(error) => {
                let error = anyhow::Error::new(error);
                if attempt == PRECHECK_ATTEMPTS || !is_transient_precheck_error(&error) {
                    return Err(error);
                }
                tokio::time::sleep(Duration::from_millis(
                    PRECHECK_RETRY_DELAY_MS * attempt as u64,
                ))
                .await;
            }
        }
    }
    unreachable!("precheck attempts loop always returns")
}

fn resolve_precheck_key(config: &Config) -> Option<String> {
    config.precheck.key.clone()
}

pub(crate) fn candidate_urls(rendered: &str) -> Vec<String> {
    if rendered.starts_with("http://") || rendered.starts_with("https://") {
        vec![rendered.to_string()]
    } else {
        vec![
            format!("https://{}", rendered),
            format!("http://{}", rendered),
        ]
    }
}

pub(crate) fn precheck_input_values(
    wordlists: &[WordlistData],
    key: &str,
    value: &str,
) -> InputMap {
    wordlists
        .iter()
        .map(|wordlist| {
            (
                wordlist.keyword.clone(),
                if wordlist.keyword == key {
                    value.to_string()
                } else {
                    String::new()
                },
            )
        })
        .collect::<BTreeMap<_, _>>()
}

fn brief_error(error: &anyhow::Error) -> String {
    let text = format!("{:#}", error);
    brief_error_text(&text)
}

fn brief_error_text(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let reason = if is_fd_exhaustion_text(&lower) {
        "file descriptor exhausted"
    } else if is_temporary_dns_text(&lower) {
        "temporary dns failure"
    } else if lower.contains("timed out") || lower.contains("timeout") {
        "timeout"
    } else if lower.contains("dns") || lower.contains("lookup") || lower.contains("resolve") {
        "dns lookup failed"
    } else if lower.contains("connection refused") {
        "connection refused"
    } else if lower.contains("certificate") || lower.contains("tls") {
        "tls error"
    } else if lower.contains("redirect") {
        "redirect error"
    } else {
        text.lines().next().unwrap_or("request failed")
    };

    let mut reason = reason.to_string();
    if reason.len() > 120 {
        reason.truncate(117);
        reason.push_str("...");
    }
    reason
}

fn is_transient_precheck_error(error: &anyhow::Error) -> bool {
    is_transient_precheck_error_text(&format!("{:#}", error))
}

fn is_transient_precheck_error_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    is_fd_exhaustion_text(&lower) || is_temporary_dns_text(&lower)
}

fn is_fd_exhaustion_text(lower: &str) -> bool {
    lower.contains("no file descriptors")
        || lower.contains("too many open files")
        || lower.contains("emfile")
}

fn is_temporary_dns_text(lower: &str) -> bool {
    lower.contains("temporary failure")
        || lower.contains("resource temporarily unavailable")
        || lower.contains("try again")
        || lower.contains("eai_again")
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};

    use crate::input::modes::InputCase;
    use crate::input::wordlist::WordlistData;

    use super::*;

    #[test]
    fn candidate_urls_try_https_then_http_when_scheme_is_missing() {
        assert_eq!(
            candidate_urls("URLFUZZ/login"),
            vec![
                "https://URLFUZZ/login".to_string(),
                "http://URLFUZZ/login".to_string()
            ]
        );
    }

    #[test]
    fn candidate_urls_keep_explicit_http_scheme() {
        assert_eq!(
            candidate_urls("http://asdsadasd.example.com/login"),
            vec!["http://asdsadasd.example.com/login".to_string()]
        );
    }

    #[test]
    fn precheck_values_only_put_real_payload_on_precheck_key() {
        let values = precheck_input_values(
            &[
                WordlistData {
                    keyword: "URLFUZZ".to_string(),
                    values: vec!["example.com".to_string()],
                },
                WordlistData {
                    keyword: "UFUZZ".to_string(),
                    values: vec!["admin".to_string()],
                },
                WordlistData {
                    keyword: "PFUZZ".to_string(),
                    values: vec!["password".to_string()],
                },
            ],
            "URLFUZZ",
            "example.com",
        );

        assert_eq!(values["URLFUZZ"], "example.com");
        assert_eq!(values["UFUZZ"], "");
        assert_eq!(values["PFUZZ"], "");
    }

    #[test]
    fn skipper_skips_cases_by_failed_payload_value() {
        let skipper = PrecheckSkipper::new(
            "URLFUZZ".to_string(),
            HashSet::from(["bad-target".to_string()]),
        );
        let input = InputCase {
            values: BTreeMap::from([
                ("URLFUZZ".to_string(), "bad-target".to_string()),
                ("UFUZZ".to_string(), "admin".to_string()),
            ]),
            display: "URLFUZZ=bad-target,UFUZZ=admin".to_string(),
        };

        assert!(skipper.should_skip(&input));
    }

    #[test]
    fn skipper_can_fast_skip_scopes_that_include_its_key() {
        let skipper = PrecheckSkipper::new("URLFUZZ".to_string(), HashSet::new());

        assert!(skipper.can_fast_skip_scope(&["URLFUZZ".to_string()]));
        assert!(skipper.can_fast_skip_scope(&["URLFUZZ".to_string(), "UFUZZ".to_string()]));
        assert!(!skipper.can_fast_skip_scope(&["UFUZZ".to_string()]));
    }

    #[test]
    fn brief_error_reports_fd_exhaustion_before_dns_lookup() {
        let reason = brief_error_text(
            "error sending request for url: dns error: failed to lookup address information: No file descriptors available",
        );

        assert_eq!(reason, "file descriptor exhausted");
    }

    #[test]
    fn transient_precheck_error_detects_temporary_dns_and_fd_errors() {
        assert!(is_transient_precheck_error_text(
            "dns error: failed to lookup address information: Temporary failure in name resolution"
        ));
        assert!(is_transient_precheck_error_text(
            "dns error: failed to lookup address information: No file descriptors available"
        ));
        assert!(!is_transient_precheck_error_text(
            "dns error: failed to lookup address information: Name does not resolve"
        ));
    }
}
