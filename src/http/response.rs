use bytes::Bytes;
use regex::Regex;
use reqwest::header::LOCATION;

use crate::matcher::signature::ResponseSignature;

#[derive(Debug, Clone)]
pub struct ResponseSummary {
    pub signature: ResponseSignature,
    pub raw: String,
}

pub fn summarize(
    status: u16,
    headers: &reqwest::header::HeaderMap,
    body: Bytes,
    elapsed_ms: u128,
) -> ResponseSummary {
    let size = body.len();
    let body_hash = fnv1a64(&body);
    let body_text = String::from_utf8_lossy(&body).to_string();
    let words = body_text.split_whitespace().count();
    let lines = if body_text.is_empty() {
        0
    } else {
        body_text.lines().count()
    };
    let location = headers
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let title = extract_title(&body_text);

    let raw = build_raw_response(status, headers, &body_text);
    ResponseSummary {
        signature: ResponseSignature {
            status,
            size,
            words,
            lines,
            elapsed_ms,
            location,
            title,
            body_hash,
        },
        raw,
    }
}

fn build_raw_response(status: u16, headers: &reqwest::header::HeaderMap, body: &str) -> String {
    let mut raw = format!("HTTP/1.1 {}\r\n", status);
    for (name, value) in headers {
        raw.push_str(name.as_str());
        raw.push_str(": ");
        raw.push_str(value.to_str().unwrap_or("<binary>"));
        raw.push_str("\r\n");
    }
    raw.push_str("\r\n");
    raw.push_str(body);
    raw
}

fn extract_title(body: &str) -> Option<String> {
    let regex = Regex::new(r"(?is)<title[^>]*>\s*(.*?)\s*</title>").ok()?;
    regex
        .captures(body)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().trim().to_string())
        .filter(|value| !value.is_empty())
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
