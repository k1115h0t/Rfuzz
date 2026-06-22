use std::fs;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use bytes::Bytes;
use reqwest::Client;

use crate::config::{RequestConfig, ResponseBodyConfig};
use crate::http::dns_cache::CachedResolver;
use crate::http::request::RenderedRequest;
use crate::http::response::{summarize, ResponseSummary};

pub fn build_client(config: &RequestConfig) -> Result<Client> {
    let mut builder =
        Client::builder()
            .timeout(config.timeout)
            .redirect(if config.follow_redirects {
                reqwest::redirect::Policy::limited(10)
            } else {
                reqwest::redirect::Policy::none()
            });
    if !config.keepalive {
        builder = builder.pool_max_idle_per_host(0);
    }
    if config.dns_cache {
        builder = builder.dns_resolver(Arc::new(CachedResolver::new(
            config.dns_cache_ttl,
            config.dns_negative_cache_ttl,
            config.dns_max_concurrent,
        )));
    }
    if !config.ssl_verify {
        builder = builder
            .danger_accept_invalid_certs(true)
            .danger_accept_invalid_hostnames(true);
    }
    if config.http2 {
        builder = builder.http2_prior_knowledge();
    }
    if let Some(sni) = &config.sni {
        tracing::warn!(
            "-sni={} is accepted for compatibility, but arbitrary SNI override requires a lower-level connector than reqwest exposes",
            sni
        );
    }
    if let (Some(cert), Some(key)) = (&config.client_cert, &config.client_key) {
        let mut pem = fs::read(cert)?;
        pem.extend_from_slice(&fs::read(key)?);
        builder = builder.identity(reqwest::Identity::from_pem(&pem)?);
    }
    if let Some(proxy) = &config.proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy)?);
    }
    builder.build().context("failed to build HTTP client")
}

pub async fn execute(
    client: &Client,
    request: &RenderedRequest,
    body_config: ResponseBodyConfig,
) -> Result<ResponseSummary> {
    let started = Instant::now();
    let mut builder = client.request(request.method.clone(), &request.url);
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value);
    }
    if let Some(body) = &request.body {
        builder = builder.body(body.clone());
    }

    let response = builder.send().await?;
    let status = response.status().as_u16();
    let version = response.version();
    let headers = response.headers().clone();
    let (body, body_truncated) = read_body_with_limit(response, body_config).await?;
    Ok(summarize(
        status,
        version,
        &headers,
        body,
        body_truncated,
        body_config.preview_bytes,
        started.elapsed().as_millis(),
    ))
}

async fn read_body_with_limit(
    mut response: reqwest::Response,
    body_config: ResponseBodyConfig,
) -> Result<(Bytes, bool)> {
    let content_length = response.content_length();
    if body_config.ignore || body_config.max_bytes == 0 {
        return Ok((
            Bytes::new(),
            content_length.is_some_and(|length| length > 0),
        ));
    }

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        let remaining = body_config.max_bytes.saturating_sub(body.len());
        if remaining == 0 {
            return Ok((Bytes::from(body), true));
        }
        if chunk.len() > remaining {
            body.extend_from_slice(&chunk[..remaining]);
            return Ok((Bytes::from(body), true));
        }
        body.extend_from_slice(&chunk);
    }

    Ok((Bytes::from(body), false))
}
