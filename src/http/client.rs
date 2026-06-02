use std::fs;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use reqwest::Client;

use crate::config::RequestConfig;
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
        builder = builder.dns_resolver(Arc::new(CachedResolver::new(config.dns_cache_ttl)));
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

pub async fn execute(client: &Client, request: &RenderedRequest) -> Result<ResponseSummary> {
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
    let headers = response.headers().clone();
    let body = response.bytes().await?;
    Ok(summarize(
        status,
        &headers,
        body,
        started.elapsed().as_millis(),
    ))
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use std::time::{Duration, Instant};

    use crate::http::dns_cache::DnsCache;

    #[test]
    fn dns_cache_reuses_entries_until_ttl_expires() {
        let cache = DnsCache::default();
        let now = Instant::now();
        let addrs = vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)];

        cache.store("example.com", addrs.clone(), now, Duration::from_secs(300));

        assert_eq!(
            cache.get("example.com", now + Duration::from_secs(299)),
            Some(addrs)
        );
        assert_eq!(
            cache.get("example.com", now + Duration::from_secs(301)),
            None
        );
    }
}
