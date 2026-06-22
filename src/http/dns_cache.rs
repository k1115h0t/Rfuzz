use std::collections::HashMap;
use std::error::Error;
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use tokio::sync::{Notify, Semaphore};

#[derive(Debug, Clone, Default)]
pub struct DnsCache {
    entries: Arc<Mutex<HashMap<String, TimedDnsEntry>>>,
}

#[derive(Debug, Clone)]
enum CachedDnsEntry {
    Success(Vec<SocketAddr>),
    Failure(String),
}

#[derive(Debug, Clone)]
enum CachedDnsLookup {
    Success(Vec<SocketAddr>),
    Failure(String),
}

#[derive(Debug, Clone)]
struct TimedDnsEntry {
    value: CachedDnsEntry,
    expires_at: Instant,
}

impl DnsCache {
    fn get(&self, host: &str, now: Instant) -> Option<CachedDnsLookup> {
        let host = normalize_host(host);
        let mut entries = self.entries.lock().ok()?;
        let entry = entries.get(&host)?;
        if entry.expires_at <= now {
            entries.remove(&host);
            return None;
        }
        Some(match &entry.value {
            CachedDnsEntry::Success(addrs) => CachedDnsLookup::Success(addrs.clone()),
            CachedDnsEntry::Failure(message) => CachedDnsLookup::Failure(message.clone()),
        })
    }

    pub fn store(&self, host: &str, addrs: Vec<SocketAddr>, now: Instant, ttl: Duration) {
        let host = normalize_host(host);
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(
                host,
                TimedDnsEntry {
                    value: CachedDnsEntry::Success(addrs),
                    expires_at: now + ttl,
                },
            );
        }
    }

    pub fn store_failure(&self, host: &str, message: String, now: Instant, ttl: Duration) {
        if ttl == Duration::ZERO {
            return;
        }
        let host = normalize_host(host);
        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(
                host,
                TimedDnsEntry {
                    value: CachedDnsEntry::Failure(message),
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
    negative_ttl: Duration,
    in_flight: InFlightLookups,
    lookup_limit: Arc<Semaphore>,
}

type BoxedDnsError = Box<dyn Error + Send + Sync>;
type InFlightLookups = Arc<Mutex<HashMap<String, Arc<Notify>>>>;

enum ResolveRole {
    Leader(Arc<Notify>),
    Waiter(Arc<Notify>),
}

impl CachedResolver {
    pub fn new(ttl: Duration, negative_ttl: Duration, max_concurrent: usize) -> Self {
        Self {
            cache: DnsCache::default(),
            ttl,
            negative_ttl,
            in_flight: Arc::new(Mutex::new(HashMap::new())),
            lookup_limit: Arc::new(Semaphore::new(max_concurrent.max(1))),
        }
    }
}

impl Resolve for CachedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let cache = self.cache.clone();
        let ttl = self.ttl;
        let negative_ttl = self.negative_ttl;
        let in_flight = self.in_flight.clone();
        let lookup_limit = self.lookup_limit.clone();
        let host = name.as_str().to_string();
        Box::pin(async move {
            let now = Instant::now();
            if let Some(addrs) = cache.get(&host, now) {
                return dns_lookup_to_result(&host, addrs);
            }

            loop {
                match resolve_role(&in_flight, &host) {
                    ResolveRole::Waiter(notify) => {
                        notify.notified().await;
                        if let Some(addrs) = cache.get(&host, Instant::now()) {
                            return dns_lookup_to_result(&host, addrs);
                        }
                    }
                    ResolveRole::Leader(notify) => {
                        let result =
                            resolve_uncached(&host, &cache, ttl, negative_ttl, lookup_limit).await;
                        finish_resolve(&in_flight, &host, &notify);
                        return result;
                    }
                }
            }
        })
    }
}

async fn resolve_uncached(
    host: &str,
    cache: &DnsCache,
    ttl: Duration,
    negative_ttl: Duration,
    lookup_limit: Arc<Semaphore>,
) -> Result<Addrs, BoxedDnsError> {
    let _permit = lookup_limit
        .acquire_owned()
        .await
        .map_err(|error| boxed_dns_error(format!("DNS lookup limiter closed: {error}")))?;
    match tokio::net::lookup_host((host, 0)).await {
        Ok(addrs) => {
            let addrs = addrs.collect::<Vec<_>>();
            if addrs.is_empty() {
                let message = format!("DNS lookup returned no addresses for {host}");
                cache.store_failure(host, message.clone(), Instant::now(), negative_ttl);
                return Err(boxed_dns_error(message));
            }
            cache.store(host, addrs.clone(), Instant::now(), ttl);
            Ok(Box::new(addrs.into_iter()) as Addrs)
        }
        Err(error) => {
            let message = format!("DNS lookup failed for {host}: {error}");
            cache.store_failure(host, message.clone(), Instant::now(), negative_ttl);
            Err(boxed_dns_error(message))
        }
    }
}

fn dns_lookup_to_result(host: &str, lookup: CachedDnsLookup) -> Result<Addrs, BoxedDnsError> {
    match lookup {
        CachedDnsLookup::Success(addrs) => Ok(Box::new(addrs.into_iter()) as Addrs),
        CachedDnsLookup::Failure(message) => Err(boxed_dns_error(format!(
            "cached DNS failure for {host}: {message}"
        ))),
    }
}

fn resolve_role(in_flight: &InFlightLookups, host: &str) -> ResolveRole {
    let notify = {
        let Ok(mut in_flight) = in_flight.lock() else {
            return ResolveRole::Leader(Arc::new(Notify::new()));
        };
        if let Some(notify) = in_flight.get(host) {
            return ResolveRole::Waiter(notify.clone());
        }
        let notify = Arc::new(Notify::new());
        in_flight.insert(host.to_string(), notify.clone());
        notify
    };
    ResolveRole::Leader(notify)
}

fn finish_resolve(in_flight: &InFlightLookups, host: &str, notify: &Arc<Notify>) {
    if let Ok(mut in_flight) = in_flight.lock() {
        if in_flight
            .get(host)
            .is_some_and(|stored| Arc::ptr_eq(stored, notify))
        {
            in_flight.remove(host);
        }
    }
    notify.notify_waiters();
}

fn boxed_dns_error(message: String) -> BoxedDnsError {
    Box::new(io::Error::other(message))
}

fn normalize_host(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::*;

    #[test]
    fn dns_cache_reuses_entries_until_ttl_expires() {
        let cache = DnsCache::default();
        let now = Instant::now();
        let addrs = vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)];

        cache.store("Example.COM.", addrs.clone(), now, Duration::from_secs(300));

        match cache.get("example.com", now + Duration::from_secs(299)) {
            Some(CachedDnsLookup::Success(cached)) => assert_eq!(cached, addrs),
            other => panic!("expected cached success, got {other:?}"),
        }
        assert!(cache
            .get("example.com", now + Duration::from_secs(301))
            .is_none());
    }

    #[test]
    fn dns_cache_can_store_temporary_failures() {
        let cache = DnsCache::default();
        let now = Instant::now();

        cache.store_failure(
            "missing.example",
            "name does not resolve".to_string(),
            now,
            Duration::from_secs(30),
        );

        match cache.get("missing.example.", now + Duration::from_secs(1)) {
            Some(CachedDnsLookup::Failure(message)) => {
                assert!(message.contains("name does not resolve"));
            }
            other => panic!("expected cached failure, got {other:?}"),
        }
        assert!(cache
            .get("missing.example", now + Duration::from_secs(31))
            .is_none());
    }

    #[test]
    fn zero_negative_ttl_does_not_store_failures() {
        let cache = DnsCache::default();
        let now = Instant::now();

        cache.store_failure(
            "missing.example",
            "name does not resolve".to_string(),
            now,
            Duration::ZERO,
        );

        assert!(cache.get("missing.example", now).is_none());
    }
}
