use std::collections::BTreeSet;
use std::fs;

use anyhow::{anyhow, Context, Result};
use reqwest::Method;

use crate::template::Template;

#[derive(Debug, Clone)]
pub struct RawRequestTemplate {
    pub method: Method,
    pub path: Template,
    pub headers: Vec<(String, Template)>,
    pub body: Option<Template>,
    pub scheme: String,
}

impl RawRequestTemplate {
    pub fn from_burp_file(path: &str, scheme: &str, keywords: &BTreeSet<String>) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read raw request file {}", path))?;
        parse_burp_request(&content, scheme, keywords)
    }

    pub fn placeholders(&self) -> BTreeSet<String> {
        let mut placeholders = self.path.placeholders();
        for (_, template) in &self.headers {
            placeholders.extend(template.placeholders());
        }
        if let Some(body) = &self.body {
            placeholders.extend(body.placeholders());
        }
        placeholders
    }
}

fn parse_burp_request(
    content: &str,
    scheme: &str,
    keywords: &BTreeSet<String>,
) -> Result<RawRequestTemplate> {
    let normalized = content.replace("\r\n", "\n");
    let (head, body) = normalized
        .split_once("\n\n")
        .map_or((normalized.as_str(), ""), |(head, body)| (head, body));
    let mut lines = head.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| anyhow!("raw request file is empty"))?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts
        .next()
        .ok_or_else(|| anyhow!("raw request missing method"))?;
    let path = request_parts
        .next()
        .ok_or_else(|| anyhow!("raw request missing path"))?;

    let mut headers = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| anyhow!("invalid raw request header: {}", line))?;
        headers.push((
            name.trim().to_string(),
            Template::compile(value.trim(), keywords)?,
        ));
    }

    Ok(RawRequestTemplate {
        method: Method::from_bytes(method.as_bytes())?,
        path: Template::compile(path, keywords)?,
        headers,
        body: (!body.is_empty())
            .then(|| Template::compile(body, keywords))
            .transpose()?,
        scheme: scheme.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::template::render::InputMap;

    #[test]
    fn parses_burp_style_request_with_template_body() {
        let keywords = BTreeSet::from(["PASS".to_string()]);
        let raw = "POST /login HTTP/1.1\r\nHost: example.com\r\nContent-Type: x\r\n\r\npassword=${{PASS}}$";
        let parsed = parse_burp_request(raw, "https", &keywords).unwrap();
        let input = InputMap::from([("PASS".to_string(), "secret".to_string())]);
        assert_eq!(parsed.method, Method::POST);
        assert_eq!(parsed.path.render(&input).unwrap(), "/login");
        assert_eq!(
            parsed.body.unwrap().render(&input).unwrap(),
            "password=secret"
        );
    }
}
