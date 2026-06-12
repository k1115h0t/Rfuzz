use std::fs::File;
use std::io::{BufRead, BufReader};

use crate::config::{Config, ScheduleMode};
use crate::input::wordlist::WordlistSource;

const RESERVED_FDS: usize = 128;
const FDS_PER_WORKER: usize = 4;
const DEFAULT_KEEPALIVE_IDLE_SECS: usize = 90;
const FDS_PER_DNS_LOOKUP: usize = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FdLimitPlan {
    pub soft_limit: usize,
    pub requested_concurrency: usize,
    pub effective_concurrency: usize,
    pub max_concurrency: usize,
    pub required_soft_limit: usize,
    pub keepalive_target_fds: usize,
    pub dns_fds: usize,
    pub output_fds: usize,
}

impl FdLimitPlan {
    pub fn from_soft_limit(requested_concurrency: usize, soft_limit: u64) -> Self {
        let requested_concurrency = requested_concurrency.max(1);
        let soft_limit = usize::try_from(soft_limit).unwrap_or(usize::MAX);
        let max_concurrency = soft_limit
            .saturating_sub(RESERVED_FDS)
            .checked_div(FDS_PER_WORKER)
            .unwrap_or(0)
            .max(1);
        let effective_concurrency = requested_concurrency.min(max_concurrency);
        let required_soft_limit =
            RESERVED_FDS.saturating_add(requested_concurrency.saturating_mul(FDS_PER_WORKER));

        Self {
            soft_limit,
            requested_concurrency,
            effective_concurrency,
            max_concurrency,
            required_soft_limit,
            keepalive_target_fds: 0,
            dns_fds: 0,
            output_fds: 0,
        }
    }

    pub fn from_config(config: &Config, soft_limit: u64) -> Self {
        let mut plan = Self::from_soft_limit(config.execution.concurrency, soft_limit);
        plan.keepalive_target_fds = estimate_keepalive_target_fds(config);
        plan.dns_fds = if config.request.dns_cache {
            config
                .request
                .dns_max_concurrent
                .saturating_mul(FDS_PER_DNS_LOOKUP)
        } else {
            0
        };
        plan.output_fds = usize::from(config.output.path.is_some())
            .saturating_add(usize::from(config.output.error_log.is_some()));
        plan.required_soft_limit = plan
            .required_soft_limit
            .saturating_add(plan.keepalive_target_fds)
            .saturating_add(plan.dns_fds)
            .saturating_add(plan.output_fds);
        plan
    }

    pub fn adjusted(&self) -> bool {
        self.effective_concurrency != self.requested_concurrency
    }

    pub fn soft_limit_insufficient(&self) -> bool {
        self.soft_limit < self.required_soft_limit
    }
}

pub fn apply_startup_limits(config: &mut Config) {
    let Some(soft_limit) = current_fd_soft_limit() else {
        return;
    };
    let plan = FdLimitPlan::from_config(config, soft_limit);
    config.execution.concurrency = plan.effective_concurrency;
    print_fd_limit_hint(plan);
}

fn print_fd_limit_hint(plan: FdLimitPlan) {
    if plan.adjusted() || plan.soft_limit_insufficient() {
        eprintln!(
            " WARN fd-limit: 当前 ulimit -n={}，当前参数估算至少需要 {}。已请求 -t={}，实际 -t={}。请在启动前执行：ulimit -n {}，或降低 -t/-rate，或关闭 keepalive。",
            plan.soft_limit,
            plan.required_soft_limit,
            plan.requested_concurrency,
            plan.effective_concurrency,
            plan.required_soft_limit
        );
    } else {
        eprintln!(
            "# fd-limit: 当前 ulimit -n={}，当前参数估算至少需要 {}，-t={}，OK",
            plan.soft_limit, plan.required_soft_limit, plan.effective_concurrency
        );
    }
}

fn estimate_keepalive_target_fds(config: &Config) -> usize {
    if !config.request.keepalive {
        return 0;
    }
    let Some(target_key) = target_key(config) else {
        return 0;
    };

    let target_count = target_wordlist_value_count(config, target_key);
    let estimated_window = if config.precheck.enabled && config.precheck.key.as_deref().is_some() {
        estimated_recent_target_count(config)
    } else if config.execution.schedule.mode == ScheduleMode::RotateWindow {
        config.execution.schedule.target_window
    } else {
        config.execution.concurrency
    };

    target_count.map_or(estimated_window, |count| count.min(estimated_window))
}

fn estimated_recent_target_count(config: &Config) -> usize {
    let requests_per_sec = config
        .execution
        .rate_per_sec
        .map(|rate| usize::try_from(rate).unwrap_or(usize::MAX))
        .unwrap_or(config.execution.concurrency)
        .max(1);
    requests_per_sec.saturating_mul(DEFAULT_KEEPALIVE_IDLE_SECS)
}

fn target_key(config: &Config) -> Option<&str> {
    config
        .execution
        .schedule
        .target_key
        .as_deref()
        .or(config.precheck.key.as_deref())
        .or_else(|| config.future.stop.stop_scope.first().map(String::as_str))
}

fn target_wordlist_value_count(config: &Config, target_key: &str) -> Option<usize> {
    let wordlist = config
        .input
        .wordlists
        .iter()
        .find(|wordlist| wordlist.keyword == target_key)?;
    let WordlistSource::Path(path) = &wordlist.source else {
        return None;
    };
    let file = File::open(path).ok()?;
    let reader = BufReader::new(file);
    let base_count = reader
        .lines()
        .map_while(Result::ok)
        .filter(|line| {
            !config.input.ignore_wordlist_comments || !line.trim_start().starts_with('#')
        })
        .count();
    Some(base_count.saturating_mul(1 + config.input.extensions.len()))
}

#[cfg(unix)]
fn current_fd_soft_limit() -> Option<u64> {
    let mut limits = std::mem::MaybeUninit::<libc::rlimit>::uninit();
    let result = unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, limits.as_mut_ptr()) };
    if result != 0 {
        return None;
    }
    let limits = unsafe { limits.assume_init() };
    if limits.rlim_cur == libc::RLIM_INFINITY {
        None
    } else {
        Some(limits.rlim_cur as u64)
    }
}

#[cfg(not(unix))]
fn current_fd_soft_limit() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use clap::Parser;

    use super::*;
    use crate::cli::Cli;

    #[test]
    fn fd_limit_plan_estimates_keepalive_target_pool_from_rate() {
        let path = temp_wordlist_path();
        let values = (0..10_000)
            .map(|index| format!("http://127.0.0.1:{index}\n"))
            .collect::<String>();
        fs::write(&path, values).unwrap();

        let wordlist = format!("{}:URLFUZZ", path.display());
        let cli = Cli::parse_from([
            "rfuzz",
            "-u",
            "URLFUZZ/login",
            "-w",
            &wordlist,
            "--precheck-key",
            "URLFUZZ",
            "--rate",
            "90",
            "-t",
            "100",
            "-o",
            "result.jsonl",
        ]);
        let config = Config::try_from(cli).unwrap();

        let plan = FdLimitPlan::from_config(&config, 1024);

        fs::remove_file(path).unwrap();
        assert_eq!(plan.keepalive_target_fds, 90 * DEFAULT_KEEPALIVE_IDLE_SECS);
        assert_eq!(plan.dns_fds, 64);
        assert_eq!(plan.output_fds, 1);
        assert_eq!(plan.required_soft_limit, 128 + 100 * 4 + 90 * 90 + 64 + 1);
        assert!(plan.soft_limit_insufficient());
    }

    fn temp_wordlist_path() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("rfuzz-test-targets-{unique}.txt"))
    }
}
