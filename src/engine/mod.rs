pub mod precheck;
pub mod progress;
pub mod rate_limiter;
pub mod scheduler;
pub mod worker;

use anyhow::Result;

use crate::config::Config;

pub async fn run(config: Config) -> Result<()> {
    scheduler::run(config).await
}
