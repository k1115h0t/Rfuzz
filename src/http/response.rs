use std::sync::OnceLock;

use bytes::Bytes;
use regex::Regex;
use reqwest::header::LOCATION;

use crate::matcher::signature::ResponseSignature;

#[derive(Debug, Clone)]
pub struct ResponseSummary {
    pub signature: ResponseSignature,
    version: reqwest::Version,
    headers: reqwest::header::HeaderMap,
    body_text: String,
    body_truncated: bool,
    body_preview_bytes: usize,
    raw: Option<String>,
}

pub fn summarize(
    status: u16,
    version: reqwest::Version,
    headers: &reqwest::header::HeaderMap,
    body: Bytes,
    body_truncated: bool,
    body_preview_bytes: usize,
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
        version,
        headers: headers.clone(),
        body_text,
        body_truncated,
        body_preview_bytes,
        raw: None,
    }
}

impl ResponseSummary {
    #[cfg(test)]
    pub fn from_parts(signature: ResponseSignature, raw: String) -> Self {
        Self {
            signature,
            version: reqwest::Version::HTTP_11,
            headers: reqwest::header::HeaderMap::new(),
            body_text: String::new(),
            body_truncated: false,
            body_preview_bytes: usize::MAX,
            raw: Some(raw),
        }
    }

    pub fn raw_response(&mut self) -> &str {
        if self.raw.is_none() {
            self.raw = Some(build_raw_response(
                self.version,
                self.signature.status,
                &self.headers,
                &self.body_text,
                self.body_truncated,
                self.body_preview_bytes,
            ));
        }
        self.raw.as_deref().unwrap_or_default()
    }

    pub fn header_text(&self) -> String {
        let mut raw = build_response_head(self.version, self.signature.status, &self.headers);
        raw.push_str("\r\n");
        raw
    }

    pub fn ensure_title(&mut self) {
        if self.signature.title.is_none() {
            self.signature.title = extract_title(&self.body_text);
        }
    }
}

fn build_raw_response(
    version: reqwest::Version,
    status: u16,
    headers: &reqwest::header::HeaderMap,
    body: &str,
    body_truncated: bool,
    body_preview_bytes: usize,
) -> String {
    let mut raw = build_response_head(version, status, headers);
    raw.push_str("\r\n");
    let (preview, preview_truncated) = body_preview(body, body_preview_bytes);
    raw.push_str(preview);
    if preview_truncated {
        raw.push_str(&format!(
            "\r\n[rfuzz: response body preview truncated at {} bytes]",
            body_preview_bytes
        ));
    }
    if body_truncated {
        raw.push_str("\r\n[rfuzz: response body read stopped at max-body limit]");
    }
    raw
}

fn build_response_head(
    version: reqwest::Version,
    status: u16,
    headers: &reqwest::header::HeaderMap,
) -> String {
    let mut raw = format!("{:?} {}\r\n", version, status);
    for (name, value) in headers {
        raw.push_str(&canonical_header_name(name.as_str()));
        raw.push_str(": ");
        raw.push_str(value.to_str().unwrap_or("<binary>"));
        raw.push_str("\r\n");
    }
    raw
}

fn body_preview(body: &str, limit: usize) -> (&str, bool) {
    if body.len() <= limit {
        return (body, false);
    }
    let mut end = limit.min(body.len());
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    (&body[..end], true)
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

        let mut response = summarize(
            200,
            reqwest::Version::HTTP_11,
            &headers,
            Bytes::from_static(b"ok"),
            false,
            4096,
            10,
        );

        assert!(response
            .raw_response()
            .contains("Set-Cookie: session_id=abc"));
    }

    #[test]
    fn raw_response_uses_response_version_and_preview_limit() {
        let headers = HeaderMap::new();
        let mut response = summarize(
            200,
            reqwest::Version::HTTP_2,
            &headers,
            Bytes::from_static(b"abcdef"),
            true,
            3,
            10,
        );

        let raw = response.raw_response();

        assert!(raw.starts_with("HTTP/2.0 200"));
        assert!(raw.contains("abc"));
        assert!(raw.contains("preview truncated"));
        assert!(raw.contains("max-body"));
    }

    #[test]
    fn header_text_contains_status_and_headers_without_body() {
        let mut headers = HeaderMap::new();
        headers.insert(SET_COOKIE, HeaderValue::from_static("session_id=abc"));

        let response = summarize(
            200,
            reqwest::Version::HTTP_11,
            &headers,
            Bytes::from_static(b"body"),
            false,
            4096,
            10,
        );

        let head = response.header_text();

        assert!(head.starts_with("HTTP/1.1 200\r\n"));
        assert!(head.contains("Set-Cookie: session_id=abc\r\n"));
        assert!(!head.contains("body"));
    }
}
