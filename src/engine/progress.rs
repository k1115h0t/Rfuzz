use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Timeout,
    Dns,
    Connect,
    Tls,
    Redirect,
    FileDescriptor,
    Request,
    Other,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ErrorCounts {
    pub timeout: usize,
    pub dns: usize,
    pub connect: usize,
    pub tls: usize,
    pub redirect: usize,
    pub file_descriptor: usize,
    pub request: usize,
    pub other: usize,
}

impl ErrorCounts {
    fn record(&mut self, kind: ErrorKind) {
        match kind {
            ErrorKind::Timeout => self.timeout += 1,
            ErrorKind::Dns => self.dns += 1,
            ErrorKind::Connect => self.connect += 1,
            ErrorKind::Tls => self.tls += 1,
            ErrorKind::Redirect => self.redirect += 1,
            ErrorKind::FileDescriptor => self.file_descriptor += 1,
            ErrorKind::Request => self.request += 1,
            ErrorKind::Other => self.other += 1,
        }
    }

    fn nonzero_parts(&self) -> Vec<(&'static str, usize)> {
        [
            ("timeout", self.timeout),
            ("dns", self.dns),
            ("connect", self.connect),
            ("tls", self.tls),
            ("redirect", self.redirect),
            ("fd", self.file_descriptor),
            ("request", self.request),
            ("other", self.other),
        ]
        .into_iter()
        .filter(|(_, count)| *count > 0)
        .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressSnapshot {
    pub total: usize,
    pub completed: usize,
    pub requests: usize,
    pub matched: usize,
    pub errors: usize,
    pub skipped: usize,
    pub avg_elapsed_ms: Option<u128>,
    pub error_counts: ErrorCounts,
    pub requests_per_sec: f64,
    pub eta_secs: Option<u64>,
}

#[derive(Debug)]
pub struct ProgressState {
    total: usize,
    completed: usize,
    requests: usize,
    matched: usize,
    errors: usize,
    skipped: usize,
    total_elapsed_ms: u128,
    error_counts: ErrorCounts,
}

impl ProgressState {
    pub fn new(total: usize) -> Self {
        Self {
            total,
            completed: 0,
            requests: 0,
            matched: 0,
            errors: 0,
            skipped: 0,
            total_elapsed_ms: 0,
            error_counts: ErrorCounts::default(),
        }
    }

    pub fn record_response(&mut self, matched: bool, elapsed_ms: u128) {
        self.completed += 1;
        self.requests += 1;
        self.total_elapsed_ms += elapsed_ms;
        if matched {
            self.matched += 1;
        }
    }

    pub fn record_error(&mut self, kind: ErrorKind, elapsed_ms: u128) {
        self.completed += 1;
        self.requests += 1;
        self.errors += 1;
        self.total_elapsed_ms += elapsed_ms;
        self.error_counts.record(kind);
    }

    pub fn record_skipped_by(&mut self, count: usize) {
        self.completed += count;
        self.skipped += count;
    }

    pub fn snapshot(&self, elapsed_secs: f64) -> ProgressSnapshot {
        let requests_per_sec = if elapsed_secs > 0.0 {
            self.requests as f64 / elapsed_secs
        } else {
            0.0
        };
        let completed_per_sec = if elapsed_secs > 0.0 {
            self.completed as f64 / elapsed_secs
        } else {
            0.0
        };
        let avg_elapsed_ms =
            (self.requests > 0).then(|| self.total_elapsed_ms / self.requests as u128);
        let remaining = self.total.saturating_sub(self.completed);
        let eta_secs = if completed_per_sec > 0.0 {
            Some((remaining as f64 / completed_per_sec).ceil() as u64)
        } else {
            None
        };

        ProgressSnapshot {
            total: self.total,
            completed: self.completed,
            requests: self.requests,
            matched: self.matched,
            errors: self.errors,
            skipped: self.skipped,
            avg_elapsed_ms,
            error_counts: self.error_counts,
            requests_per_sec,
            eta_secs,
        }
    }
}

#[derive(Clone)]
pub struct ProgressReporter {
    inner: Arc<ProgressReporterInner>,
}

struct ProgressReporterInner {
    state: Mutex<ProgressState>,
    started: Instant,
    last_refresh: Mutex<Instant>,
    bar: Option<ProgressBar>,
}

impl ProgressReporter {
    pub fn new(total: usize, enabled: bool) -> Self {
        let bar = enabled.then(|| {
            let bar = ProgressBar::with_draw_target(
                Some(total as u64),
                ProgressDrawTarget::stderr_with_hz(4),
            );
            let style = ProgressStyle::with_template(
                "{spinner:.green} [{bar:40.cyan/blue}] {pos}/{len} {percent}% | {msg}",
            )
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=>-");
            bar.set_style(style);
            bar.enable_steady_tick(Duration::from_millis(250));
            bar
        });

        let reporter = Self {
            inner: Arc::new(ProgressReporterInner {
                state: Mutex::new(ProgressState::new(total)),
                started: Instant::now(),
                last_refresh: Mutex::new(Instant::now() - Duration::from_millis(200)),
                bar,
            }),
        };
        reporter.refresh(true);
        reporter
    }

    pub fn record_response(&self, matched: bool, elapsed_ms: u128) {
        self.update(|state| state.record_response(matched, elapsed_ms));
    }

    pub fn record_error(&self, kind: ErrorKind, elapsed_ms: u128) {
        self.update(|state| state.record_error(kind, elapsed_ms));
    }

    pub fn record_skipped_by(&self, count: usize) {
        self.update(|state| state.record_skipped_by(count));
    }

    pub fn finish(&self) {
        if let Some(bar) = &self.inner.bar {
            self.refresh(true);
            bar.finish_and_clear();
        }
    }

    fn update(&self, f: impl FnOnce(&mut ProgressState)) {
        {
            let mut state = self.inner.state.lock().expect("progress state poisoned");
            f(&mut state);
        }
        self.refresh(false);
    }

    fn refresh(&self, force: bool) {
        let Some(bar) = &self.inner.bar else {
            return;
        };
        if !force {
            let mut last_refresh = self
                .inner
                .last_refresh
                .lock()
                .expect("progress refresh state poisoned");
            let now = Instant::now();
            if now.duration_since(*last_refresh) < Duration::from_millis(200) {
                return;
            }
            *last_refresh = now;
        }
        let snapshot = {
            let state = self.inner.state.lock().expect("progress state poisoned");
            state.snapshot(self.inner.started.elapsed().as_secs_f64())
        };
        bar.set_position(snapshot.completed as u64);
        bar.set_message(format_snapshot(snapshot));
    }
}

fn format_snapshot(snapshot: ProgressSnapshot) -> String {
    format!(
        "matched {} | errors {} | skipped {} | avg {} | {:.1} req/s | err {} | ETA {}",
        snapshot.matched,
        snapshot.errors,
        snapshot.skipped,
        snapshot
            .avg_elapsed_ms
            .map(format_millis)
            .unwrap_or_else(|| "--".to_string()),
        snapshot.requests_per_sec,
        format_error_counts(snapshot.error_counts),
        snapshot
            .eta_secs
            .map(format_duration)
            .unwrap_or_else(|| "--:--".to_string())
    )
}

fn format_error_counts(counts: ErrorCounts) -> String {
    let parts = counts.nonzero_parts();
    if parts.is_empty() {
        return "-".to_string();
    }
    parts
        .into_iter()
        .take(4)
        .map(|(label, count)| format!("{label} {count}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn format_millis(ms: u128) -> String {
    if ms >= 1000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{ms}ms")
    }
}

fn format_duration(total_secs: u64) -> String {
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;
    if hours > 0 {
        format!("{hours:02}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes:02}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_progress_counters_and_eta() {
        let mut state = ProgressState::new(10);

        state.record_response(true, 100);
        state.record_response(false, 200);
        state.record_error(ErrorKind::Timeout, 3000);
        state.record_skipped_by(2);

        let snapshot = state.snapshot(2.0);

        assert_eq!(snapshot.total, 10);
        assert_eq!(snapshot.completed, 5);
        assert_eq!(snapshot.requests, 3);
        assert_eq!(snapshot.matched, 1);
        assert_eq!(snapshot.errors, 1);
        assert_eq!(snapshot.skipped, 2);
        assert_eq!(snapshot.requests_per_sec, 1.5);
        assert_eq!(snapshot.avg_elapsed_ms, Some(1100));
        assert_eq!(snapshot.error_counts.timeout, 1);
        assert_eq!(snapshot.eta_secs, Some(2));
    }

    #[test]
    fn formats_eta_and_request_rate() {
        let error_counts = ErrorCounts {
            timeout: 2,
            dns: 1,
            ..ErrorCounts::default()
        };
        let text = format_snapshot(ProgressSnapshot {
            total: 100,
            completed: 50,
            requests: 50,
            matched: 2,
            errors: 3,
            skipped: 4,
            avg_elapsed_ms: Some(1234),
            error_counts,
            requests_per_sec: 12.345,
            eta_secs: Some(65),
        });

        assert_eq!(
            text,
            "matched 2 | errors 3 | skipped 4 | avg 1.2s | 12.3 req/s | err timeout 2,dns 1 | ETA 01:05"
        );
    }
}
