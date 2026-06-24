use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use anyhow::Result;
use reqwest::{Client, StatusCode};
use tokio::task::JoinSet;

use crate::config::Config;
use crate::engine::rate_limiter::RateLimiter;
use crate::http::request::normalize_url;
use crate::input::modes::InputCase;
use crate::input::wordlist::WordlistData;
use crate::template::render::InputMap;

type PrecheckCaseResult = Result<Option<(String, Vec<String>)>>;

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
    let wordlists = Arc::new(wordlists.to_vec());
    let attempts = config.precheck.attempts.max(1);
    let mut pending_values = wordlist.values.clone();

    for attempt in 1..=attempts {
        if pending_values.is_empty() {
            break;
        }

        let mut join_set = JoinSet::new();
        let round_values = std::mem::take(&mut pending_values);
        let mut values = round_values.iter().cloned();
        let mut round_failures: HashMap<String, Vec<String>> = HashMap::new();

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
            if let Some((value, failures)) = result?? {
                round_failures.insert(value, failures);
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

        if attempt == attempts {
            for value in round_values {
                if let Some(failures) = round_failures.remove(&value) {
                    eprintln!("PRECHECK ERROR {}={} {}", key, value, failures.join("; "));
                    failed_values.insert(value);
                }
            }
        } else {
            pending_values = round_values
                .into_iter()
                .filter(|value| round_failures.contains_key(value))
                .collect();
        }
    }

    Ok(failed_values)
}

fn spawn_precheck_case(
    join_set: &mut JoinSet<PrecheckCaseResult>,
    config: &Config,
    wordlists: Arc<Vec<WordlistData>>,
    key: &str,
    value: String,
    client: Client,
    limiter: RateLimiter,
) {
    let Some(template) = config.request.url.clone() else {
        return;
    };
    let encoders = config.input.encoders.clone();
    let raw_uri = config.request.raw_uri;
    let key = key.to_string();
    join_set.spawn(async move {
        limiter.wait().await;
        let rendered =
            render_precheck_url(&encoders, raw_uri, &wordlists, &key, &value, &template)?;
        let candidates = candidate_urls(&rendered);
        let mut failures = Vec::new();
        for url in &candidates {
            match probe_precheck_url(&client, url).await {
                Ok(_) => return Ok(None),
                Err(error) => {
                    failures.push(format!("{}: {}", url, brief_error(&error)));
                }
            }
        }
        Ok(Some((value, failures)))
    });
}

async fn probe_precheck_url(client: &Client, url: &str) -> Result<()> {
    match client.head(url).send().await {
        Ok(response)
            if response.status() != StatusCode::METHOD_NOT_ALLOWED
                && response.status() != StatusCode::NOT_IMPLEMENTED =>
        {
            Ok(())
        }
        Ok(_) | Err(_) => {
            client.get(url).send().await?;
            Ok(())
        }
    }
}

fn render_precheck_url(
    encoders: &crate::input::encoder::EncoderSet,
    raw_uri: bool,
    wordlists: &[WordlistData],
    key: &str,
    value: &str,
    template: &crate::template::Template,
) -> Result<String> {
    let input = precheck_input_values(wordlists, key, value);
    let input = encoders.apply_to_map(&input);
    let rendered = template.render(&input)?;
    Ok(normalize_url(&rendered, raw_uri))
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
        truncate_to_char_boundary(&mut reason, 117);
        reason.push_str("...");
    }
    reason
}

fn truncate_to_char_boundary(text: &mut String, max_len: usize) {
    if text.len() <= max_len {
        return;
    }
    let end = text
        .char_indices()
        .map(|(index, _)| index)
        .take_while(|index| *index <= max_len)
        .last()
        .unwrap_or(0);
    text.truncate(end);
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
    use std::sync::Arc;

    use clap::Parser;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;
    use tokio::sync::Mutex;

    use crate::cli::Cli;
    use crate::config::Config;
    use crate::http::client::build_client;
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
    fn precheck_rendering_applies_encoders() {
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            "https://example.com/${{TARGET}}$",
            "-w",
            "targets.txt:TARGET",
            "--enc",
            "TARGET:urlencode",
            "--precheck-key",
            "TARGET",
        ]);
        let config = Config::try_from(cli).unwrap();
        let wordlists = [WordlistData {
            keyword: "TARGET".to_string(),
            values: vec!["a b".to_string()],
        }];
        let template = config.request.url.as_ref().unwrap();

        let rendered = render_precheck_url(
            &config.input.encoders,
            config.request.raw_uri,
            &wordlists,
            "TARGET",
            "a b",
            template,
        )
        .unwrap();

        assert_eq!(rendered, "https://example.com/a+b");
    }

    #[test]
    fn precheck_rendering_normalizes_url_like_main_request() {
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            "https://example.com/${{TARGET}}$",
            "-w",
            "targets.txt:TARGET",
            "--precheck-key",
            "TARGET",
        ]);
        let config = Config::try_from(cli).unwrap();
        let wordlists = [WordlistData {
            keyword: "TARGET".to_string(),
            values: vec!["a b".to_string()],
        }];
        let template = config.request.url.as_ref().unwrap();

        let rendered = render_precheck_url(
            &config.input.encoders,
            config.request.raw_uri,
            &wordlists,
            "TARGET",
            "a b",
            template,
        )
        .unwrap();

        assert_eq!(rendered, "https://example.com/a%20b");
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

    #[tokio::test]
    async fn precheck_report_only_retries_targets_by_round_without_skipper() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let seen_paths = Arc::new(Mutex::new(Vec::new()));
        let server_paths = seen_paths.clone();

        let server = tokio::spawn(async move {
            for _ in 0..12 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = [0u8; 1024];
                let count = stream.read(&mut buffer).await.unwrap();
                let request = String::from_utf8_lossy(&buffer[..count]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("")
                    .to_string();
                server_paths.lock().await.push(path);
            }
        });

        let url = format!("http://{}/URLFUZZ", addr);
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            &url,
            "-w",
            "urls.txt:URLFUZZ",
            "--precheck-key",
            "URLFUZZ",
            "--precheck-report-only",
            "--precheck-attempts",
            "2",
            "-t",
            "1",
        ]);
        let config = Config::try_from(cli).unwrap();
        let wordlists = vec![WordlistData {
            keyword: "URLFUZZ".to_string(),
            values: vec!["one".to_string(), "two".to_string(), "three".to_string()],
        }];
        let client = build_client(&config.request).unwrap();

        let skipper = run(&config, &wordlists, client).await.unwrap();
        server.await.unwrap();

        assert!(skipper.is_none());
        assert_eq!(
            *seen_paths.lock().await,
            vec![
                "/one", "/one", "/two", "/two", "/three", "/three", "/one", "/one", "/two", "/two",
                "/three", "/three"
            ]
        );
    }

    #[tokio::test]
    async fn precheck_probe_falls_back_to_get_when_head_is_not_allowed() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let methods = Arc::new(Mutex::new(Vec::new()));
        let server_methods = methods.clone();

        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buffer = [0u8; 1024];
                let count = stream.read(&mut buffer).await.unwrap();
                let request = String::from_utf8_lossy(&buffer[..count]);
                let method = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().next())
                    .unwrap_or("")
                    .to_string();
                let response = if method == "HEAD" {
                    "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n"
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n"
                };
                server_methods.lock().await.push(method);
                use tokio::io::AsyncWriteExt;
                stream.write_all(response.as_bytes()).await.unwrap();
            }
        });

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        probe_precheck_url(&client, &format!("http://{}/target", addr))
            .await
            .unwrap();
        server.await.unwrap();

        assert_eq!(*methods.lock().await, vec!["HEAD", "GET"]);
    }
}
