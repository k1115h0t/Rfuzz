use crate::config::Config;

const RESERVED_FDS: usize = 128;
const FDS_PER_WORKER: usize = 4;
const MIN_RECOMMENDED_SOFT_LIMIT: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FdLimitPlan {
    pub soft_limit: usize,
    pub requested_concurrency: usize,
    pub effective_concurrency: usize,
    pub max_concurrency: usize,
    pub required_soft_limit: usize,
    pub recommended_soft_limit: usize,
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
        let recommended_soft_limit = required_soft_limit.max(MIN_RECOMMENDED_SOFT_LIMIT);

        Self {
            soft_limit,
            requested_concurrency,
            effective_concurrency,
            max_concurrency,
            required_soft_limit,
            recommended_soft_limit,
        }
    }

    pub fn adjusted(&self) -> bool {
        self.effective_concurrency != self.requested_concurrency
    }
}

pub fn apply_startup_limits(config: &mut Config) {
    let Some(soft_limit) = current_fd_soft_limit() else {
        return;
    };
    let plan = FdLimitPlan::from_soft_limit(config.execution.concurrency, soft_limit);
    config.execution.concurrency = plan.effective_concurrency;
    print_fd_limit_hint(plan);
}

fn print_fd_limit_hint(plan: FdLimitPlan) {
    if plan.adjusted() {
        eprintln!(
            " WARN ulimit -n={} 较低，已将并发 -t 从 {} 下调到 {}。建议运行前设置：ulimit -n {}，或继续降低 -t。",
            plan.soft_limit,
            plan.requested_concurrency,
            plan.effective_concurrency,
            plan.recommended_soft_limit
        );
    } else {
        eprintln!(
            "# fd-limit: ulimit -n={}, -t={}, 估算至少需要 {}。大量 URLFUZZ/预检查建议先设置：ulimit -n {}",
            plan.soft_limit,
            plan.effective_concurrency,
            plan.required_soft_limit,
            plan.recommended_soft_limit
        );
    }
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
