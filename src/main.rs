mod cli;
mod config;
mod engine;
mod http;
mod input;
mod matcher;
mod output;
mod runtime_limits;
mod template;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .without_time()
        .init();

    let cli = cli::parse();
    let mut config = config::Config::try_from(cli)?;
    runtime_limits::apply_startup_limits(&mut config);
    engine::run(config).await
}
