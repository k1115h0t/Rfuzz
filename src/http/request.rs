use anyhow::{anyhow, Result};

use crate::config::RequestConfig;
use crate::http::raw_request::RawRequestTemplate;
use crate::template::render::InputMap;

#[derive(Debug, Clone)]
pub struct RenderedRequest {
    pub method: reqwest::Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<String>,
    pub raw: String,
}

impl RenderedRequest {
    pub fn from_config(config: &RequestConfig, input: &InputMap) -> Result<Self> {
        let mut rendered = if let Some(raw_request) = &config.raw_request {
            render_raw_request(raw_request, input)?
        } else {
            let url = config
                .url
                .as_ref()
                .expect("config validation requires URL or raw request")
                .render(input)?;
            Self {
                method: config.method.clone(),
                url: normalize_url(&url, config.raw_uri),
                headers: Vec::new(),
                body: None,
                raw: String::new(),
            }
        };

        let mut headers = Vec::new();
        headers.append(&mut rendered.headers);
        for (name, template) in &config.headers {
            headers.push((name.clone(), template.render(input)?));
        }
        if !config.cookies.is_empty() {
            let cookie = config
                .cookies
                .iter()
                .map(|cookie| cookie.render(input))
                .collect::<Result<Vec<_>>>()?
                .join("; ");
            headers.push(("Cookie".to_string(), cookie));
        }
        let body = config
            .body
            .as_ref()
            .map(|template| template.render(input))
            .transpose()?
            .or(rendered.body);
        let headers = normalize_content_length_headers(headers, body.as_deref());

        let raw = build_raw_request(&rendered.method, &rendered.url, &headers, body.as_deref());

        rendered.headers = headers;
        rendered.body = body;
        rendered.raw = raw;
        Ok(rendered)
    }
}

fn render_raw_request(raw: &RawRequestTemplate, input: &InputMap) -> Result<RenderedRequest> {
    let path = raw.path.render(input)?;
    let mut headers = Vec::new();
    let mut host = None;
    for (name, template) in &raw.headers {
        let value = template.render(input)?;
        if name.eq_ignore_ascii_case("host") {
            host = Some(value.clone());
        }
        headers.push((name.clone(), value));
    }
    let url = if path.starts_with("http://") || path.starts_with("https://") {
        path
    } else {
        let host = host
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| {
                anyhow!(
                    "raw request missing Host header; add a Host header or use an absolute URL in the request line"
                )
            })?;
        format!("{}://{}{}", raw.scheme, host, path)
    };
    let body = raw
        .body
        .as_ref()
        .map(|template| template.render(input))
        .transpose()?;

    Ok(RenderedRequest {
        method: raw.method.clone(),
        url,
        headers,
        body,
        raw: String::new(),
    })
}

fn normalize_content_length_headers(
    headers: Vec<(String, String)>,
    body: Option<&str>,
) -> Vec<(String, String)> {
    let mut normalized = headers
        .into_iter()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("content-length"))
        .collect::<Vec<_>>();
    if let Some(body) = body {
        normalized.push(("Content-Length".to_string(), body.len().to_string()));
    }
    normalized
}

fn normalize_url(url: &str, raw_uri: bool) -> String {
    if raw_uri {
        return url.to_string();
    }
    if !url.contains(' ') {
        return url.to_string();
    }
    url.replace(' ', "%20")
}

fn build_raw_request(
    method: &reqwest::Method,
    url: &str,
    headers: &[(String, String)],
    body: Option<&str>,
) -> String {
    let path = reqwest::Url::parse(url)
        .map(|url| {
            let mut path = url.path().to_string();
            if let Some(query) = url.query() {
                path.push('?');
                path.push_str(query);
            }
            path
        })
        .unwrap_or_else(|_| url.to_string());
    let mut raw = format!("{} {} HTTP/1.1\r\n", method, path);
    for (name, value) in headers {
        raw.push_str(name);
        raw.push_str(": ");
        raw.push_str(value);
        raw.push_str("\r\n");
    }
    raw.push_str("\r\n");
    if let Some(body) = body {
        raw.push_str(body);
    }
    raw
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use reqwest::Method;

    use super::*;
    use crate::http::raw_request::RawRequestTemplate;
    use crate::template::Template;

    #[test]
    fn renders_burp_raw_request_to_full_url() {
        let keywords = BTreeSet::from(["DIR".to_string()]);
        let raw = RawRequestTemplate {
            method: Method::GET,
            path: Template::compile("/${{DIR}}$", &keywords).unwrap(),
            headers: vec![(
                "Host".to_string(),
                Template::compile("example.com", &keywords).unwrap(),
            )],
            body: None,
            scheme: "https".to_string(),
        };
        let request = render_raw_request(
            &raw,
            &InputMap::from([("DIR".to_string(), "admin".to_string())]),
        )
        .unwrap();
        assert_eq!(request.url, "https://example.com/admin");
    }

    #[test]
    fn raw_request_requires_host_for_relative_paths() {
        let keywords = BTreeSet::from(["DIR".to_string()]);
        let raw = RawRequestTemplate {
            method: Method::GET,
            path: Template::compile("/${{DIR}}$", &keywords).unwrap(),
            headers: Vec::new(),
            body: None,
            scheme: "https".to_string(),
        };

        let error = render_raw_request(
            &raw,
            &InputMap::from([("DIR".to_string(), "admin".to_string())]),
        )
        .unwrap_err();

        assert!(error.to_string().contains("missing Host header"));
    }

    #[test]
    fn recalculates_content_length_after_template_rendering() {
        let keywords = BTreeSet::from(["PASS".to_string()]);
        let raw = RawRequestTemplate {
            method: Method::POST,
            path: Template::compile("/login", &keywords).unwrap(),
            headers: vec![
                (
                    "Host".to_string(),
                    Template::compile("example.com", &keywords).unwrap(),
                ),
                (
                    "Content-Length".to_string(),
                    Template::compile("999", &keywords).unwrap(),
                ),
            ],
            body: Some(Template::compile("password=${{PASS}}$", &keywords).unwrap()),
            scheme: "https".to_string(),
        };
        let request_config = RequestConfig {
            method: Method::GET,
            url: None,
            raw_request: Some(raw),
            headers: Vec::new(),
            cookies: Vec::new(),
            body: None,
            proxy: None,
            replay_proxy: None,
            timeout: std::time::Duration::from_secs(1),
            follow_redirects: false,
            raw_uri: false,
            sni: None,
            http2: false,
            ssl_verify: false,
            keepalive: true,
            dns_cache: false,
            dns_cache_ttl: std::time::Duration::from_secs(1),
            dns_negative_cache_ttl: std::time::Duration::from_secs(0),
            dns_max_concurrent: 1,
            client_cert: None,
            client_key: None,
            response_body: crate::config::ResponseBodyConfig {
                ignore: false,
                max_bytes: 1024,
                preview_bytes: 1024,
            },
        };

        let rendered = RenderedRequest::from_config(
            &request_config,
            &InputMap::from([("PASS".to_string(), "secret".to_string())]),
        )
        .unwrap();

        assert!(rendered.raw.contains("Content-Length: 15\r\n"));
        assert!(!rendered.raw.contains("Content-Length: 999"));
    }
}
