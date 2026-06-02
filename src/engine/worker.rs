use std::time::Instant;

use reqwest::Client;

use crate::config::RequestConfig;
use crate::engine::progress::ErrorKind;
use crate::engine::rate_limiter::DelayConfig;
use crate::engine::rate_limiter::RateLimiter;
use crate::http::client;
use crate::http::request::RenderedRequest;
use crate::http::response::ResponseSummary;
use crate::input::encoder::EncoderSet;
use crate::input::modes::InputCase;

#[derive(Debug)]
pub struct WorkerResult {
    pub input: InputCase,
    pub url: String,
    pub request_raw: String,
    pub rendered_request: RenderedRequest,
    pub response: ResponseSummary,
}

#[derive(Debug)]
pub struct WorkerError {
    pub input: InputCase,
    pub url: Option<String>,
    pub error: anyhow::Error,
    pub elapsed_ms: u128,
}

pub async fn execute_case(
    client: Client,
    request_config: RequestConfig,
    limiter: RateLimiter,
    delay: DelayConfig,
    encoders: EncoderSet,
    input: InputCase,
) -> std::result::Result<WorkerResult, WorkerError> {
    delay.wait().await;
    limiter.wait().await;
    let render_values = encoders.apply_to_map(&input.values);
    let rendered =
        RenderedRequest::from_config(&request_config, &render_values).map_err(|error| {
            WorkerError {
                input: input.clone(),
                url: None,
                error,
                elapsed_ms: 0,
            }
        })?;
    let url = rendered.url.clone();
    let request_raw = rendered.raw.clone();
    let started = Instant::now();
    let response = client::execute(&client, &rendered)
        .await
        .map_err(|error| WorkerError {
            input: input.clone(),
            url: Some(url.clone()),
            error,
            elapsed_ms: started.elapsed().as_millis(),
        })?;
    Ok(WorkerResult {
        input,
        url,
        request_raw,
        rendered_request: rendered,
        response,
    })
}

pub fn classify_error(error: &anyhow::Error) -> ErrorKind {
    classify_error_text(&format!("{:#}", error))
}

fn classify_error_text(text: &str) -> ErrorKind {
    let lower = text.to_ascii_lowercase();
    if lower.contains("no file descriptors")
        || lower.contains("too many open files")
        || lower.contains("emfile")
    {
        ErrorKind::FileDescriptor
    } else if lower.contains("timed out") || lower.contains("timeout") {
        ErrorKind::Timeout
    } else if lower.contains("dns") || lower.contains("lookup") || lower.contains("resolve") {
        ErrorKind::Dns
    } else if lower.contains("connect")
        || lower.contains("connection refused")
        || lower.contains("connection reset")
        || lower.contains("tcp open")
    {
        ErrorKind::Connect
    } else if lower.contains("certificate") || lower.contains("tls") || lower.contains("ssl") {
        ErrorKind::Tls
    } else if lower.contains("redirect") {
        ErrorKind::Redirect
    } else if lower.contains("builder error")
        || lower.contains("invalid url")
        || lower.contains("relative url")
    {
        ErrorKind::Request
    } else {
        ErrorKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_request_errors() {
        assert_eq!(
            classify_error_text("operation timed out while connecting"),
            ErrorKind::Timeout
        );
        assert_eq!(
            classify_error_text("dns error: failed to lookup address information"),
            ErrorKind::Dns
        );
        assert_eq!(
            classify_error_text("client error (Connect): connection refused"),
            ErrorKind::Connect
        );
        assert_eq!(
            classify_error_text("tls error: invalid certificate"),
            ErrorKind::Tls
        );
        assert_eq!(
            classify_error_text("too many open files"),
            ErrorKind::FileDescriptor
        );
    }
}
