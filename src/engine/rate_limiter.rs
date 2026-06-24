use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use rand::Rng;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct RateLimiter {
    interval: Option<Duration>,
    next_allowed: Arc<Mutex<Instant>>,
}

impl RateLimiter {
    pub fn new(rate_per_sec: Option<u64>) -> Self {
        let interval = rate_per_sec
            .filter(|rate| *rate > 0)
            .map(|rate| Duration::from_secs_f64(1.0 / rate as f64));
        Self {
            interval,
            next_allowed: Arc::new(Mutex::new(Instant::now())),
        }
    }

    pub async fn wait(&self) {
        let Some(interval) = self.interval else {
            return;
        };

        let mut next_allowed = self.next_allowed.lock().await;
        let now = Instant::now();
        if *next_allowed > now {
            tokio::time::sleep(*next_allowed - now).await;
        }
        *next_allowed = Instant::now() + interval;
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DelayConfig {
    min: Duration,
    max: Duration,
}

impl Default for DelayConfig {
    fn default() -> Self {
        Self {
            min: Duration::ZERO,
            max: Duration::ZERO,
        }
    }
}

impl DelayConfig {
    pub fn parse(raw: Option<&str>) -> Result<Self> {
        let Some(raw) = raw else {
            return Ok(Self::default());
        };
        if let Some((min, max)) = raw.split_once('-') {
            let min = parse_seconds(min)?;
            let max = parse_seconds(max)?;
            if min > max {
                return Err(anyhow!("delay range minimum cannot exceed maximum"));
            }
            Ok(Self { min, max })
        } else {
            let delay = parse_seconds(raw)?;
            Ok(Self {
                min: delay,
                max: delay,
            })
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.max > Duration::ZERO
    }

    pub async fn wait(&self) {
        if !self.is_enabled() {
            return;
        }
        let delay = if self.min == self.max {
            self.min
        } else {
            let min_ms = self.min.as_millis() as u64;
            let max_ms = self.max.as_millis() as u64;
            Duration::from_millis(rand::thread_rng().gen_range(min_ms..=max_ms))
        };
        tokio::time::sleep(delay).await;
    }
}

fn parse_seconds(raw: &str) -> Result<Duration> {
    let seconds = raw
        .trim()
        .parse::<f64>()
        .map_err(|_| anyhow!("invalid delay '{}'", raw))?;
    if !seconds.is_finite() {
        return Err(anyhow!("delay must be finite"));
    }
    if seconds < 0.0 {
        return Err(anyhow!("delay cannot be negative"));
    }
    Ok(Duration::from_secs_f64(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixed_and_range_delay() {
        assert_eq!(
            DelayConfig::parse(Some("0.5")).unwrap().min,
            Duration::from_millis(500)
        );
        let range = DelayConfig::parse(Some("0.1-0.2")).unwrap();
        assert_eq!(range.min, Duration::from_millis(100));
        assert_eq!(range.max, Duration::from_millis(200));
    }

    #[test]
    fn rejects_non_finite_delay_values() {
        for value in ["NaN", "inf", "1-inf"] {
            let error = DelayConfig::parse(Some(value)).unwrap_err().to_string();

            assert!(error.contains("delay must be finite"));
        }
    }
}
