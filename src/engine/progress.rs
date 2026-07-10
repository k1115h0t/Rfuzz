use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use serde::Serialize;

use crate::matcher::signature::ResponseSignature;

const ETA_WINDOW_SECS: f64 = 5.0;

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
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

    pub fn nonzero_parts(&self) -> Vec<(&'static str, usize)> {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResponseSignatureCount {
    pub status: u16,
    pub size: usize,
    pub words: usize,
    pub lines: usize,
    pub body_hash: u64,
    pub count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ResponseSignatureKey {
    status: u16,
    size: usize,
    words: usize,
    lines: usize,
    body_hash: u64,
}

impl From<&ResponseSignature> for ResponseSignatureKey {
    fn from(signature: &ResponseSignature) -> Self {
        Self {
            status: signature.status,
            size: signature.size,
            words: signature.words,
            lines: signature.lines,
            body_hash: signature.body_hash,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProgressSnapshot {
    pub total: usize,
    pub completed: usize,
    pub requests: usize,
    pub matched: usize,
    pub errors: usize,
    pub skipped: usize,
    pub error_counts: ErrorCounts,
    pub top_signatures: Vec<ResponseSignatureCount>,
    pub stop_on_match_triggered: bool,
    pub eta_secs: Option<u64>,
}

#[derive(Debug)]
struct ProgressSample {
    elapsed_secs: f64,
    completed: usize,
}

#[derive(Debug)]
pub struct ProgressState {
    total: usize,
    completed: usize,
    requests: usize,
    matched: usize,
    errors: usize,
    skipped: usize,
    error_counts: ErrorCounts,
    signature_counts: BTreeMap<ResponseSignatureKey, usize>,
    stop_on_match_triggered: bool,
    samples: VecDeque<ProgressSample>,
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
            error_counts: ErrorCounts::default(),
            signature_counts: BTreeMap::new(),
            stop_on_match_triggered: false,
            samples: VecDeque::new(),
        }
    }

    pub fn record_response(&mut self, matched: bool, _elapsed_ms: u128) {
        self.completed += 1;
        self.requests += 1;
        if matched {
            self.matched += 1;
        }
    }

    pub fn record_response_signature(&mut self, matched: bool, signature: &ResponseSignature) {
        self.record_response(matched, signature.elapsed_ms);
        *self
            .signature_counts
            .entry(ResponseSignatureKey::from(signature))
            .or_default() += 1;
    }

    pub fn record_error(&mut self, kind: ErrorKind, _elapsed_ms: u128) {
        self.completed += 1;
        self.requests += 1;
        self.errors += 1;
        self.error_counts.record(kind);
    }

    pub fn record_skipped_by(&mut self, count: usize) {
        self.completed += count;
        self.skipped += count;
    }

    pub fn record_stop_triggered(&mut self) {
        self.stop_on_match_triggered = true;
    }

    pub fn snapshot(&mut self, elapsed_secs: f64) -> ProgressSnapshot {
        self.record_sample(elapsed_secs);
        let remaining = self.total.saturating_sub(self.completed);
        let eta_secs = self.eta_secs(remaining);

        ProgressSnapshot {
            total: self.total,
            completed: self.completed,
            requests: self.requests,
            matched: self.matched,
            errors: self.errors,
            skipped: self.skipped,
            error_counts: self.error_counts,
            top_signatures: self.top_signatures(),
            stop_on_match_triggered: self.stop_on_match_triggered,
            eta_secs,
        }
    }

    fn top_signatures(&self) -> Vec<ResponseSignatureCount> {
        let mut counts = self
            .signature_counts
            .iter()
            .map(|(key, count)| ResponseSignatureCount {
                status: key.status,
                size: key.size,
                words: key.words,
                lines: key.lines,
                body_hash: key.body_hash,
                count: *count,
            })
            .collect::<Vec<_>>();
        counts.sort_by(|left, right| {
            right.count.cmp(&left.count).then_with(|| {
                (
                    left.status,
                    left.size,
                    left.words,
                    left.lines,
                    left.body_hash,
                )
                    .cmp(&(
                        right.status,
                        right.size,
                        right.words,
                        right.lines,
                        right.body_hash,
                    ))
            })
        });
        counts.truncate(5);
        counts
    }

    fn record_sample(&mut self, elapsed_secs: f64) {
        if self.completed == 0 {
            return;
        }

        let should_record = self
            .samples
            .back()
            .is_none_or(|sample| sample.completed != self.completed);
        if !should_record {
            return;
        }

        self.samples.push_back(ProgressSample {
            elapsed_secs,
            completed: self.completed,
        });

        while self.samples.len() > 2
            && self
                .samples
                .get(1)
                .is_some_and(|sample| elapsed_secs - sample.elapsed_secs >= ETA_WINDOW_SECS)
        {
            self.samples.pop_front();
        }
    }

    fn eta_secs(&self, remaining: usize) -> Option<u64> {
        if remaining == 0 {
            return Some(0);
        }

        let first = self.samples.front()?;
        let last = self.samples.back()?;
        let completed_delta = last.completed.saturating_sub(first.completed);
        let elapsed_delta = last.elapsed_secs - first.elapsed_secs;
        if completed_delta == 0 || elapsed_delta <= 0.0 {
            return None;
        }

        let completed_per_sec = completed_delta as f64 / elapsed_delta;
        Some((remaining as f64 / completed_per_sec).ceil() as u64)
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

#[derive(Debug, Default)]
struct PrecheckProgressState {
    attempt: usize,
    attempts: usize,
    round_total: usize,
    round_completed: usize,
    probes: usize,
    reachable: usize,
    round_unreachable: usize,
}

impl PrecheckProgressState {
    fn start_round(&mut self, attempt: usize, attempts: usize, round_total: usize) {
        self.attempt = attempt;
        self.attempts = attempts;
        self.round_total = round_total;
        self.round_completed = 0;
        self.round_unreachable = 0;
    }

    fn record_probe(&mut self, reachable: bool) {
        self.round_completed = self.round_completed.saturating_add(1).min(self.round_total);
        self.probes = self.probes.saturating_add(1);
        if reachable {
            self.reachable = self.reachable.saturating_add(1);
        } else {
            self.round_unreachable = self.round_unreachable.saturating_add(1);
        }
    }

    fn message(&self) -> String {
        format!(
            "round {}/{} | probes {} | reachable {} | unreachable {}",
            self.attempt, self.attempts, self.probes, self.reachable, self.round_unreachable
        )
    }
}

pub struct PrecheckProgressReporter {
    state: PrecheckProgressState,
    bar: Option<ProgressBar>,
}

impl PrecheckProgressReporter {
    pub fn new(enabled: bool) -> Self {
        let bar = enabled.then(|| {
            let bar = ProgressBar::with_draw_target(
                Some(0),
                ProgressDrawTarget::stderr_with_hz(4),
            );
            let style = ProgressStyle::with_template(
                "PRECHECK {spinner:.green} [{bar:40.magenta/blue}] {pos}/{len} {percent}% | {msg} | ETA {eta}",
            )
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=>-");
            bar.set_style(style);
            bar.enable_steady_tick(Duration::from_millis(250));
            bar
        });
        Self {
            state: PrecheckProgressState::default(),
            bar,
        }
    }

    pub fn start_round(&mut self, attempt: usize, attempts: usize, round_total: usize) {
        self.state.start_round(attempt, attempts, round_total);
        if let Some(bar) = &self.bar {
            bar.reset();
            bar.set_length(round_total as u64);
            bar.set_message(self.state.message());
        }
    }

    pub fn record_probe(&mut self, reachable: bool) {
        self.state.record_probe(reachable);
        if let Some(bar) = &self.bar {
            bar.set_position(self.state.round_completed as u64);
            bar.set_message(self.state.message());
        }
    }

    pub fn finish(&mut self) {
        if let Some(bar) = self.bar.take() {
            bar.finish_and_clear();
        }
    }
}

impl Drop for PrecheckProgressReporter {
    fn drop(&mut self) {
        self.finish();
    }
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

    pub fn record_response_signature(&self, matched: bool, signature: &ResponseSignature) {
        self.update(|state| state.record_response_signature(matched, signature));
    }

    pub fn record_error(&self, kind: ErrorKind, elapsed_ms: u128) {
        self.update(|state| state.record_error(kind, elapsed_ms));
    }

    pub fn record_skipped_by(&self, count: usize) {
        self.update(|state| state.record_skipped_by(count));
    }

    pub fn record_stop_triggered(&self) {
        self.update(|state| state.record_stop_triggered());
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        let mut state = self.inner.state.lock().expect("progress state poisoned");
        state.snapshot(self.inner.started.elapsed().as_secs_f64())
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
            let mut state = self.inner.state.lock().expect("progress state poisoned");
            state.snapshot(self.inner.started.elapsed().as_secs_f64())
        };
        bar.set_position(snapshot.completed as u64);
        bar.set_message(format_snapshot(snapshot));
    }
}

fn format_snapshot(snapshot: ProgressSnapshot) -> String {
    format!(
        "matched {} | errors {} | skipped {} | err {} | ETA {}",
        snapshot.matched,
        snapshot.errors,
        snapshot.skipped,
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
    fn tracks_progress_counters_and_recent_eta() {
        let mut state = ProgressState::new(10);

        state.record_response(true, 100);
        assert_eq!(state.snapshot(1.0).eta_secs, None);

        state.record_response(false, 200);
        state.record_error(ErrorKind::Timeout, 3000);
        state.record_skipped_by(2);

        let snapshot = state.snapshot(3.0);

        assert_eq!(snapshot.total, 10);
        assert_eq!(snapshot.completed, 5);
        assert_eq!(snapshot.requests, 3);
        assert_eq!(snapshot.matched, 1);
        assert_eq!(snapshot.errors, 1);
        assert_eq!(snapshot.skipped, 2);
        assert_eq!(snapshot.error_counts.timeout, 1);
        assert_eq!(snapshot.eta_secs, Some(3));
    }

    #[test]
    fn drops_stale_eta_samples() {
        let mut state = ProgressState::new(100);

        state.record_response(false, 100);
        state.snapshot(1.0);
        state.record_skipped_by(9);
        state.snapshot(2.0);
        state.record_response(false, 100);
        let snapshot = state.snapshot(8.0);

        assert_eq!(snapshot.completed, 11);
        assert_eq!(snapshot.eta_secs, Some(534));
    }

    #[test]
    fn formats_eta() {
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
            error_counts,
            top_signatures: Vec::new(),
            stop_on_match_triggered: false,
            eta_secs: Some(65),
        });

        assert_eq!(
            text,
            "matched 2 | errors 3 | skipped 4 | err timeout 2,dns 1 | ETA 01:05"
        );
    }

    #[test]
    fn tracks_precheck_progress_by_retry_round() {
        let mut state = PrecheckProgressState::default();

        state.start_round(1, 3, 2);
        state.record_probe(true);
        state.record_probe(false);
        assert_eq!(state.round_completed, 2);
        assert_eq!(state.probes, 2);
        assert_eq!(state.reachable, 1);
        assert_eq!(state.round_unreachable, 1);
        assert_eq!(
            state.message(),
            "round 1/3 | probes 2 | reachable 1 | unreachable 1"
        );

        state.start_round(2, 3, 1);
        assert_eq!(state.round_completed, 0);
        assert_eq!(state.round_unreachable, 0);
        state.record_probe(true);
        assert_eq!(state.probes, 3);
        assert_eq!(state.reachable, 2);
    }
}
