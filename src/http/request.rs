use anyhow::Result;

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
    let host = host.unwrap_or_default();
    let url = if path.starts_with("http://") || path.starts_with("https://") {
        path
    } else {
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

fn normalize_url(url: &str, raw_uri: bool) -> String {
    if raw_uri {
        return url.to_string();
    }
    url.chars()
        .flat_map(|ch| match ch {
            ' ' => "%20".chars().collect::<Vec<_>>(),
            _ => vec![ch],
        })
        .collect()
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
}
