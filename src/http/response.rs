use std::sync::OnceLock;

use bytes::Bytes;
use regex::Regex;
use reqwest::header::LOCATION;

use crate::matcher::signature::ResponseSignature;

#[derive(Debug, Clone)]
pub struct ResponseSummary {
    pub signature: ResponseSignature,
    headers: reqwest::header::HeaderMap,
    body_text: String,
    raw: Option<String>,
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

    ResponseSummary {
        signature: ResponseSignature {
            status,
            size,
            words,
            lines,
            elapsed_ms,
            location,
            title: None,
            body_hash,
        },
        headers: headers.clone(),
        body_text,
        raw: None,
    }
}

impl ResponseSummary {
    #[cfg(test)]
    pub fn from_parts(signature: ResponseSignature, raw: String) -> Self {
        Self {
            signature,
            headers: reqwest::header::HeaderMap::new(),
            body_text: String::new(),
            raw: Some(raw),
        }
    }

    pub fn raw_response(&mut self) -> &str {
        if self.raw.is_none() {
            self.raw = Some(build_raw_response(
                self.signature.status,
                &self.headers,
                &self.body_text,
            ));
        }
        self.raw.as_deref().unwrap_or_default()
    }

    pub fn signature_and_raw_response(&mut self) -> (&ResponseSignature, &str) {
        if self.raw.is_none() {
            self.raw = Some(build_raw_response(
                self.signature.status,
                &self.headers,
                &self.body_text,
            ));
        }
        (&self.signature, self.raw.as_deref().unwrap_or_default())
    }

    pub fn ensure_title(&mut self) {
        if self.signature.title.is_none() {
            self.signature.title = extract_title(&self.body_text);
        }
    }
}

fn build_raw_response(status: u16, headers: &reqwest::header::HeaderMap, body: &str) -> String {
    let mut raw = format!("HTTP/1.1 {}\r\n", status);
    for (name, value) in headers {
        raw.push_str(&canonical_header_name(name.as_str()));
        raw.push_str(": ");
        raw.push_str(value.to_str().unwrap_or("<binary>"));
        raw.push_str("\r\n");
    }
    raw.push_str("\r\n");
    raw.push_str(body);
    raw
}

fn canonical_header_name(name: &str) -> String {
    name.split('-')
        .map(|part| {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            let mut canonical = String::new();
            canonical.push(first.to_ascii_uppercase());
            canonical.extend(chars.map(|ch| ch.to_ascii_lowercase()));
            canonical
        })
        .collect::<Vec<_>>()
        .join("-")
}

fn extract_title(body: &str) -> Option<String> {
    static TITLE_REGEX: OnceLock<Regex> = OnceLock::new();

    TITLE_REGEX
        .get_or_init(|| Regex::new(r"(?is)<title[^>]*>\s*(.*?)\s*</title>").unwrap())
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

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use reqwest::header::{HeaderMap, HeaderValue, SET_COOKIE};

    use super::summarize;

    #[test]
    fn raw_response_uses_canonical_header_names() {
        let mut headers = HeaderMap::new();
        headers.insert(SET_COOKIE, HeaderValue::from_static("session_id=abc"));

        let mut response = summarize(200, &headers, Bytes::from_static(b"ok"), 10);

        assert!(response
            .raw_response()
            .contains("Set-Cookie: session_id=abc"));
    }
}
