use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

#[derive(Debug, Clone, Default)]
pub struct DnsCache {
    entries: Arc<Mutex<HashMap<String, CachedDnsEntry>>>,
}

#[derive(Debug, Clone)]
struct CachedDnsEntry {
    addrs: Vec<SocketAddr>,
    expires_at: Instant,
}

impl DnsCache {
    pub fn get(&self, host: &str, now: Instant) -> Option<Vec<SocketAddr>> {
        let host = normalize_host(host);
        let mut entries = self.entries.lock().ok()?;
        let entry = entries.get(&host)?;
        if entry.expires_at <= now {
            entries.remove(&host);
            return None;
        }
        Some(entry.addrs.clone())
    }

    pub fn store(&self, host: &str, addrs: Vec<SocketAddr>, now: Instant, ttl: Duration) {
        let host = normalize_host(host);
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(
                host,
                CachedDnsEntry {
                    addrs,
                    expires_at: now + ttl,
                },
            );
        }
    }
}

#[derive(Debug, Clone)]
pub struct CachedResolver {
    cache: DnsCache,
    ttl: Duration,
}

impl CachedResolver {
    pub fn new(ttl: Duration) -> Self {
        Self {
            cache: DnsCache::default(),
            ttl,
        }
    }
}

impl Resolve for CachedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let cache = self.cache.clone();
        let ttl = self.ttl;
        let host = name.as_str().to_string();
        Box::pin(async move {
            let now = Instant::now();
            if let Some(addrs) = cache.get(&host, now) {
                return Ok(Box::new(addrs.into_iter()) as Addrs);
            }

            let addrs = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|error| -> Box<dyn std::error::Error + Send + Sync> { Box::new(error) })?
                .collect::<Vec<_>>();
            cache.store(&host, addrs.clone(), Instant::now(), ttl);
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

fn normalize_host(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}
