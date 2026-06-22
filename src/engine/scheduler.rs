use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fs::File;
use std::io::BufWriter;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use serde::Serialize;
use tokio::sync::Mutex;
use tokio::task::JoinSet;

use crate::config::{Config, FuzzMode, ResponseBodyConfig, ScheduleMode};
use crate::engine::precheck::{self, PrecheckSkipper};
use crate::engine::progress::{ErrorKind, ProgressReporter};
use crate::engine::rate_limiter::RateLimiter;
use crate::engine::worker;
use crate::http::client;
use crate::http::client::build_client;
use crate::http::request::RenderedRequest;
use crate::input::modes::{
    clusterbomb, clusterbomb_ordered, clusterbomb_ordered_rotate_window, clusterbomb_rotate_window,
    pitchfork, InputCase, InputCases, ScopeCases,
};
use crate::input::wordlist::{load_wordlists, WordlistData, WordlistLoadOptions, WordlistSource};
use crate::output::{
    build_writer, error_log::build_error_logger, error_log::ErrorLogger, raw::save_raw_exchange,
    OutputRecord, RawExchange,
};

const CASE_QUEUE_MULTIPLIER: usize = 20;
const CASE_QUEUE_MAX: usize = 8192;

pub async fn run(config: Config) -> Result<()> {
    if config.future.auto_calibration.enabled {
        tracing::warn!(
            "auto-calibration flags are reserved for v0.2 and are not implemented in v0.1"
        );
    }

    let mut wordlists = load_wordlists(
        &config.input.wordlists,
        &WordlistLoadOptions {
            ignore_comments: config.input.ignore_wordlist_comments,
            extensions: config.input.extensions.clone(),
        },
    )?;
    if config.execution.plan_only() {
        let plan_wordlists = wordlists.clone();
        let cases = build_input_cases(&config, wordlists)?;
        print_execution_plan(&config, &plan_wordlists, cases)?;
        return Ok(());
    }

    let client = build_client(&config.request)?;
    let precheck_skipper = precheck::run(&config, &wordlists, client.clone()).await?;
    let precheck_skipped =
        fast_filter_precheck_failures(&config, &mut wordlists, precheck_skipper.as_deref());
    let precheck_skipper = if precheck_skipped > 0 {
        None
    } else {
        precheck_skipper
    };

    let cases = build_input_cases(&config, wordlists)?;

    let total_cases = cases.total().saturating_add(precheck_skipped);
    if total_cases == 0 {
        return Err(anyhow!("no input cases generated"));
    }

    let replay_client = if let Some(replay_proxy) = &config.request.replay_proxy {
        let mut replay_config = config.request.clone();
        replay_config.proxy = Some(replay_proxy.clone());
        replay_config.replay_proxy = None;
        Some(build_client(&replay_config)?)
    } else {
        None
    };
    let limiter = RateLimiter::new(config.execution.rate_per_sec);
    let writer = Arc::new(Mutex::new(build_writer(&config.output)?));
    let error_logger = Arc::new(Mutex::new(build_error_logger(
        config.output.error_log.as_deref(),
    )?));
    let matcher = Arc::new(config.matcher.clone());
    let progress = ProgressReporter::new(total_cases, config.output.progress);
    if precheck_skipped > 0 {
        progress.record_skipped_by(precheck_skipped);
    }
    if cases.is_empty() {
        progress.finish();
        flush_writer(writer).await?;
        flush_error_logger(error_logger).await?;
        emit_summary(&config, &progress)?;
        return Ok(());
    }

    if config.future.stop.stop_on_match.is_some() && !config.future.stop.stop_scope.is_empty() {
        let result = if cases.scope_is_prefix(&config.future.stop.stop_scope) {
            run_with_scoped_stop(
                config.clone(),
                cases,
                client,
                replay_client,
                limiter,
                writer.clone(),
                error_logger.clone(),
                matcher,
                progress.clone(),
                precheck_skipper.clone(),
            )
            .await
        } else {
            run_with_global_scoped_stop(
                config.clone(),
                cases,
                client,
                replay_client,
                limiter,
                writer.clone(),
                error_logger.clone(),
                matcher,
                progress.clone(),
                precheck_skipper.clone(),
            )
            .await
        };
        progress.finish();
        let writer_flush_result = flush_writer(writer).await;
        let flush_result = flush_error_logger(error_logger).await;
        let summary_result = emit_summary(&config, &progress);
        return result
            .and(writer_flush_result)
            .and(flush_result)
            .and(summary_result);
    }

    run_with_case_queue(
        config.clone(),
        cases,
        client,
        replay_client,
        limiter,
        writer.clone(),
        error_logger.clone(),
        matcher,
        progress.clone(),
        precheck_skipper.clone(),
    )
    .await?;
    progress.finish();
    flush_writer(writer).await?;
    flush_error_logger(error_logger).await?;
    emit_summary(&config, &progress)?;
    Ok(())
}

fn build_input_cases(config: &Config, wordlists: Vec<WordlistData>) -> Result<InputCases> {
    Ok(match config.input.mode {
        FuzzMode::Sniper => {
            tracing::warn!("sniper mode is reserved; using pitchfork behavior for v0.1");
            pitchfork(wordlists, config.input.budget_requests)
        }
        FuzzMode::Pitchfork => pitchfork(wordlists, config.input.budget_requests),
        FuzzMode::Clusterbomb => {
            if config.execution.schedule.mode == ScheduleMode::RotateWindow {
                let target_key = config
                    .execution
                    .schedule
                    .target_key
                    .as_deref()
                    .ok_or_else(|| anyhow!("-schedule rotate-window requires -target-key"))?;
                if config.input.order.is_empty() {
                    clusterbomb_rotate_window(
                        wordlists,
                        target_key,
                        config.execution.schedule.target_window,
                        config.execution.schedule.target_burst,
                        config.input.budget_requests,
                    )?
                } else {
                    clusterbomb_ordered_rotate_window(
                        wordlists,
                        &config.input.order,
                        target_key,
                        config.execution.schedule.target_window,
                        config.execution.schedule.target_burst,
                        config.input.budget_requests,
                    )?
                }
            } else if config.input.order.is_empty() {
                clusterbomb(wordlists, config.input.budget_requests)?
            } else if let Some(target_key) = ordered_schedule_target_key(config) {
                clusterbomb_ordered_rotate_window(
                    wordlists,
                    &config.input.order,
                    target_key,
                    config.execution.schedule.target_window,
                    config.execution.schedule.target_burst,
                    config.input.budget_requests,
                )?
            } else {
                clusterbomb_ordered(wordlists, &config.input.order, config.input.budget_requests)?
            }
        }
    })
}

fn print_execution_plan(
    config: &Config,
    wordlists: &[WordlistData],
    mut cases: InputCases,
) -> Result<()> {
    let total = cases.total();
    if total == 0 {
        return Err(anyhow!("no input cases generated"));
    }

    let placeholders = request_placeholders(config);
    eprintln!("Rfuzz dry run / 执行计划");
    eprintln!("  mode / 模式: {}", mode_label(config.input.mode));
    eprintln!(
        "  schedule / 调度: {}",
        schedule_label(config.execution.schedule.mode)
    );
    eprintln!("  estimated requests / 预计请求数: {}", total);
    eprintln!("  concurrency / 并发: {}", config.execution.concurrency);
    eprintln!(
        "  rate limit / 限速: {}",
        config
            .execution
            .rate_per_sec
            .map(|rate| format!("{rate}/s"))
            .unwrap_or_else(|| "unlimited / 不限".to_string())
    );
    eprintln!("  timeout / 超时: {}s", config.request.timeout.as_secs());
    eprintln!(
        "  precheck / 预检查: {}",
        if config.precheck.enabled { "on" } else { "off" }
    );
    eprintln!(
        "  output / 输出: {}",
        config.output.path.as_deref().unwrap_or("stdout / 标准输出")
    );
    if let Some(path) = &config.output.summary_json {
        eprintln!("  summary-json / 摘要 JSON: {path}");
    }
    eprintln!(
        "  response body / 响应 body: ignore={}, max={} bytes, preview={} bytes",
        config.request.response_body.ignore,
        config.request.response_body.max_bytes,
        config.request.response_body.preview_bytes
    );
    eprintln!(
        "  placeholders / 占位符: {}",
        if placeholders.is_empty() {
            "-".to_string()
        } else {
            placeholders.into_iter().collect::<Vec<_>>().join(",")
        }
    );
    eprintln!("  wordlists / 字典:");
    for (index, data) in wordlists.iter().enumerate() {
        let source = config
            .input
            .wordlists
            .get(index)
            .map(|spec| wordlist_source_label(&spec.source))
            .unwrap_or_else(|| "?".to_string());
        eprintln!(
            "    - {}: {} values from {}",
            data.keyword,
            data.values.len(),
            source
        );
    }

    if let Some(input) = cases.next() {
        let render_values = config.input.encoders.apply_to_map(&input.values);
        let rendered = RenderedRequest::from_config(&config.request, &render_values)?;
        eprintln!("  first request / 首个最终请求:");
        eprintln!("{}", rendered.raw.trim_end());
    }

    if config.request.raw_request.is_some() {
        eprintln!(
            "  note / 提示: raw request Content-Length will be recalculated after rendering."
        );
    }
    if config.matcher.uses_response_body() && config.request.response_body.ignore {
        eprintln!(
            "  warning / 警告: regex match/filter is configured but -ignore-body is enabled."
        );
    }
    Ok(())
}

fn request_placeholders(config: &Config) -> BTreeSet<String> {
    let mut placeholders = BTreeSet::new();
    if let Some(url) = &config.request.url {
        placeholders.extend(url.placeholders());
    }
    if let Some(raw_request) = &config.request.raw_request {
        placeholders.extend(raw_request.placeholders());
    }
    for (_, template) in &config.request.headers {
        placeholders.extend(template.placeholders());
    }
    for cookie in &config.request.cookies {
        placeholders.extend(cookie.placeholders());
    }
    if let Some(body) = &config.request.body {
        placeholders.extend(body.placeholders());
    }
    placeholders
}

fn wordlist_source_label(source: &WordlistSource) -> String {
    match source {
        WordlistSource::Path(path) => path.clone(),
        WordlistSource::Stdin => "stdin".to_string(),
    }
}

fn mode_label(mode: FuzzMode) -> &'static str {
    match mode {
        FuzzMode::Sniper => "sniper",
        FuzzMode::Pitchfork => "pitchfork",
        FuzzMode::Clusterbomb => "clusterbomb",
    }
}

fn schedule_label(mode: ScheduleMode) -> &'static str {
    match mode {
        ScheduleMode::Default => "default",
        ScheduleMode::RotateWindow => "rotate-window",
    }
}

#[derive(Debug, Serialize)]
struct RunSummary {
    total: usize,
    completed: usize,
    requests: usize,
    matched: usize,
    filtered: usize,
    errors: usize,
    skipped: usize,
    error_counts: crate::engine::progress::ErrorCounts,
    top_signatures: Vec<crate::engine::progress::ResponseSignatureCount>,
    output: Option<String>,
    raw_output_directory: Option<String>,
    error_log: Option<String>,
    stop_on_match_triggered: bool,
}

fn emit_summary(config: &Config, progress: &ProgressReporter) -> Result<()> {
    let snapshot = progress.snapshot();
    let filtered = snapshot
        .requests
        .saturating_sub(snapshot.matched.saturating_add(snapshot.errors));
    let summary = RunSummary {
        total: snapshot.total,
        completed: snapshot.completed,
        requests: snapshot.requests,
        matched: snapshot.matched,
        filtered,
        errors: snapshot.errors,
        skipped: snapshot.skipped,
        error_counts: snapshot.error_counts,
        top_signatures: snapshot.top_signatures.clone(),
        output: config.output.path.clone(),
        raw_output_directory: config.output.output_directory.clone(),
        error_log: config.output.error_log.clone(),
        stop_on_match_triggered: snapshot.stop_on_match_triggered,
    };

    eprintln!("Rfuzz summary / 任务摘要");
    eprintln!(
        "  total={} matched={} filtered={} errors={} skipped={}",
        summary.total, summary.matched, summary.filtered, summary.errors, summary.skipped
    );
    eprintln!(
        "  error categories / 错误分类: {}",
        format_error_counts(summary.error_counts)
    );
    eprintln!(
        "  top response signatures / 高频响应签名: {}",
        format_top_signatures(&summary.top_signatures)
    );
    eprintln!(
        "  output / 输出文件: {}",
        summary.output.as_deref().unwrap_or("stdout / 标准输出")
    );
    if let Some(path) = &summary.raw_output_directory {
        eprintln!("  raw output / 原文输出目录: {path}");
    }
    if let Some(path) = &summary.error_log {
        eprintln!("  error log / 错误日志: {path}");
    }
    eprintln!(
        "  stop-on-match triggered / 命中停止触发: {}",
        summary.stop_on_match_triggered
    );

    if let Some(path) = &config.output.summary_json {
        let writer = BufWriter::new(File::create(path)?);
        serde_json::to_writer_pretty(writer, &summary)?;
        eprintln!("  summary json / 摘要 JSON: {path}");
    }

    Ok(())
}

fn format_error_counts(counts: crate::engine::progress::ErrorCounts) -> String {
    let parts = counts.nonzero_parts();
    if parts.is_empty() {
        return "-".to_string();
    }
    parts
        .into_iter()
        .map(|(label, count)| format!("{label}:{count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_top_signatures(signatures: &[crate::engine::progress::ResponseSignatureCount]) -> String {
    if signatures.is_empty() {
        return "-".to_string();
    }
    signatures
        .iter()
        .map(|signature| {
            format!(
                "{}x status={} size={} words={} lines={} hash={}",
                signature.count,
                signature.status,
                signature.size,
                signature.words,
                signature.lines,
                signature.body_hash
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn ordered_schedule_target_key(config: &Config) -> Option<&str> {
    config
        .execution
        .schedule
        .target_key
        .as_deref()
        .or(config.precheck.key.as_deref())
        .or_else(|| config.future.stop.stop_scope.first().map(String::as_str))
}

fn next_precheck_case(
    cases: &mut InputCases,
    precheck_skipper: Option<Arc<PrecheckSkipper>>,
    progress: &ProgressReporter,
) -> Option<InputCase> {
    for input in cases.by_ref() {
        if should_skip_precheck(&input, precheck_skipper.as_deref()) {
            progress.record_skipped_by(1);
        } else {
            return Some(input);
        }
    }
    None
}

fn fast_filter_precheck_failures(
    config: &Config,
    wordlists: &mut [crate::input::wordlist::WordlistData],
    precheck_skipper: Option<&PrecheckSkipper>,
) -> usize {
    let Some(skipper) = precheck_skipper else {
        return 0;
    };
    if config.input.mode != FuzzMode::Clusterbomb || config.input.budget_requests.is_some() {
        return 0;
    }
    if skipper.failed_values().is_empty() {
        return 0;
    }

    let Some(position) = wordlists
        .iter()
        .position(|wordlist| wordlist.keyword == skipper.key())
    else {
        return 0;
    };
    let before = wordlists[position].values.len();
    let other_total = wordlists
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != position)
        .map(|(_, wordlist)| wordlist.values.len())
        .fold(1usize, usize::saturating_mul);

    wordlists[position]
        .values
        .retain(|value| !skipper.failed_values().contains(value));

    before
        .saturating_sub(wordlists[position].values.len())
        .saturating_mul(other_total)
}

fn case_queue_capacity(concurrency: usize) -> usize {
    let concurrency = concurrency.max(1);
    concurrency
        .saturating_mul(CASE_QUEUE_MULTIPLIER)
        .min(CASE_QUEUE_MAX.max(concurrency))
}

fn fill_case_queue(
    cases: &mut InputCases,
    precheck_skipper: Option<Arc<PrecheckSkipper>>,
    progress: &ProgressReporter,
    queue: &mut VecDeque<InputCase>,
    capacity: usize,
) {
    while queue.len() < capacity {
        let Some(input) = next_precheck_case(cases, precheck_skipper.clone(), progress) else {
            break;
        };
        queue.push_back(input);
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_with_case_queue(
    config: Config,
    mut cases: InputCases,
    client: reqwest::Client,
    replay_client: Option<reqwest::Client>,
    limiter: RateLimiter,
    writer: Arc<Mutex<Box<dyn crate::output::ResultWriter>>>,
    error_logger: Arc<Mutex<Option<ErrorLogger>>>,
    matcher: Arc<crate::matcher::legacy::MatcherConfig>,
    progress: ProgressReporter,
    precheck_skipper: Option<Arc<PrecheckSkipper>>,
) -> Result<()> {
    let queue_capacity = case_queue_capacity(config.execution.concurrency);
    let mut case_queue = VecDeque::with_capacity(queue_capacity);
    let mut join_set = JoinSet::new();

    fill_case_queue(
        &mut cases,
        precheck_skipper.clone(),
        &progress,
        &mut case_queue,
        queue_capacity,
    );
    while join_set.len() < config.execution.concurrency {
        let Some(input) = case_queue.pop_front() else {
            break;
        };
        spawn_case(
            &mut join_set,
            input,
            client.clone(),
            config.request.clone(),
            limiter.clone(),
            config.execution.delay,
            config.input.encoders.clone(),
            writer.clone(),
            error_logger.clone(),
            matcher.clone(),
            replay_client.clone(),
            config.output.output_directory.clone(),
            progress.clone(),
            None,
        );
    }

    while let Some(result) = join_set.join_next().await {
        result??;
        fill_case_queue(
            &mut cases,
            precheck_skipper.clone(),
            &progress,
            &mut case_queue,
            queue_capacity,
        );
        while join_set.len() < config.execution.concurrency {
            let Some(input) = case_queue.pop_front() else {
                break;
            };
            spawn_case(
                &mut join_set,
                input,
                client.clone(),
                config.request.clone(),
                limiter.clone(),
                config.execution.delay,
                config.input.encoders.clone(),
                writer.clone(),
                error_logger.clone(),
                matcher.clone(),
                replay_client.clone(),
                config.output.output_directory.clone(),
                progress.clone(),
                None,
            );
        }
    }

    Ok(())
}

#[allow(dead_code, clippy::too_many_arguments)]
async fn run_with_order_batches(
    config: Config,
    mut cases: InputCases,
    client: reqwest::Client,
    replay_client: Option<reqwest::Client>,
    limiter: RateLimiter,
    writer: Arc<Mutex<Box<dyn crate::output::ResultWriter>>>,
    error_logger: Arc<Mutex<Option<ErrorLogger>>>,
    matcher: Arc<crate::matcher::legacy::MatcherConfig>,
    progress: ProgressReporter,
    enable_scope_stop: bool,
    precheck_skipper: Option<Arc<PrecheckSkipper>>,
) -> Result<()> {
    let batch_scope = config.input.order[..config.input.order.len().saturating_sub(1)].to_vec();
    let scope_stop = enable_scope_stop.then(|| {
        Arc::new(Mutex::new(ScopeStopTracker::new(
            config.future.stop.stop_scope.clone(),
            config.future.stop.stop_on_match.unwrap_or(1),
        )))
    });

    while let Some(mut batch_cases) = cases.next_scope(&batch_scope) {
        let mut join_set = JoinSet::new();
        while join_set.len() < config.execution.concurrency {
            let Some(input) = next_unstopped_scope_case(
                &mut batch_cases,
                scope_stop.clone(),
                precheck_skipper.clone(),
                &progress,
            )
            .await
            else {
                break;
            };
            spawn_case(
                &mut join_set,
                input,
                client.clone(),
                config.request.clone(),
                limiter.clone(),
                config.execution.delay,
                config.input.encoders.clone(),
                writer.clone(),
                error_logger.clone(),
                matcher.clone(),
                replay_client.clone(),
                config.output.output_directory.clone(),
                progress.clone(),
                scope_stop.clone(),
            );
        }

        while let Some(result) = join_set.join_next().await {
            result??;
            if let Some(input) = next_unstopped_scope_case(
                &mut batch_cases,
                scope_stop.clone(),
                precheck_skipper.clone(),
                &progress,
            )
            .await
            {
                spawn_case(
                    &mut join_set,
                    input,
                    client.clone(),
                    config.request.clone(),
                    limiter.clone(),
                    config.execution.delay,
                    config.input.encoders.clone(),
                    writer.clone(),
                    error_logger.clone(),
                    matcher.clone(),
                    replay_client.clone(),
                    config.output.output_directory.clone(),
                    progress.clone(),
                    scope_stop.clone(),
                );
            }
        }
    }

    Ok(())
}

async fn next_unstopped_scope_case(
    scope_cases: &mut ScopeCases,
    scope_stop: Option<Arc<Mutex<ScopeStopTracker>>>,
    precheck_skipper: Option<Arc<PrecheckSkipper>>,
    progress: &ProgressReporter,
) -> Option<InputCase> {
    for input in scope_cases.by_ref() {
        if should_skip_precheck(&input, precheck_skipper.as_deref()) {
            progress.record_skipped_by(1);
            continue;
        }
        let skip = if let Some(scope_stop) = &scope_stop {
            scope_stop.lock().await.should_skip(&input)
        } else {
            false
        };
        if skip {
            progress.record_stop_triggered();
            progress.record_skipped_by(1);
        } else {
            return Some(input);
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
fn spawn_case(
    join_set: &mut JoinSet<Result<()>>,
    input: InputCase,
    client: reqwest::Client,
    request_config: crate::config::RequestConfig,
    limiter: RateLimiter,
    delay: crate::engine::rate_limiter::DelayConfig,
    encoders: crate::input::encoder::EncoderSet,
    writer: Arc<Mutex<Box<dyn crate::output::ResultWriter>>>,
    error_logger: Arc<Mutex<Option<ErrorLogger>>>,
    matcher: Arc<crate::matcher::legacy::MatcherConfig>,
    replay_client: Option<reqwest::Client>,
    output_directory: Option<String>,
    progress: ProgressReporter,
    scope_stop: Option<Arc<Mutex<ScopeStopTracker>>>,
) {
    join_set.spawn(async move {
        match worker::execute_case(
            client,
            request_config,
            limiter,
            delay,
            encoders,
            input.clone(),
        )
        .await
        {
            Ok(result) => {
                let signature = result.response.signature.clone();
                let matched_input = result.input.clone();
                match handle_match_result(
                    result,
                    matcher.as_ref(),
                    writer,
                    replay_client,
                    output_directory,
                )
                .await
                {
                    Ok(matched) => {
                        if matched {
                            if let Some(scope_stop) = &scope_stop {
                                scope_stop.lock().await.record_match(&matched_input);
                            }
                        }
                        progress.record_response_signature(matched, &signature);
                    }
                    Err(error) => {
                        progress.record_error(ErrorKind::Other, 0);
                        return Err(error);
                    }
                }
                Ok(())
            }
            Err(error) => {
                let kind = worker::classify_error(&error.error);
                let elapsed_ms = error.elapsed_ms;
                write_worker_error_log(error_logger, &error).await?;
                progress.record_error(kind, elapsed_ms);
                Ok(())
            }
        }
    });
}

#[allow(clippy::too_many_arguments)]
async fn run_with_global_scoped_stop(
    config: Config,
    mut cases: InputCases,
    client: reqwest::Client,
    replay_client: Option<reqwest::Client>,
    limiter: RateLimiter,
    writer: Arc<Mutex<Box<dyn crate::output::ResultWriter>>>,
    error_logger: Arc<Mutex<Option<ErrorLogger>>>,
    matcher: Arc<crate::matcher::legacy::MatcherConfig>,
    progress: ProgressReporter,
    precheck_skipper: Option<Arc<PrecheckSkipper>>,
) -> Result<()> {
    let max_hits = config.future.stop.stop_on_match.unwrap_or(1);
    let scope_stop = Arc::new(Mutex::new(ScopeStopTracker::new(
        config.future.stop.stop_scope.clone(),
        max_hits,
    )));
    let mut join_set = JoinSet::new();
    while join_set.len() < config.execution.concurrency {
        let Some(input) = next_unstopped_case(
            &mut cases,
            scope_stop.clone(),
            precheck_skipper.clone(),
            &progress,
        )
        .await
        else {
            break;
        };
        spawn_case(
            &mut join_set,
            input,
            client.clone(),
            config.request.clone(),
            limiter.clone(),
            config.execution.delay,
            config.input.encoders.clone(),
            writer.clone(),
            error_logger.clone(),
            matcher.clone(),
            replay_client.clone(),
            config.output.output_directory.clone(),
            progress.clone(),
            Some(scope_stop.clone()),
        );
    }

    while let Some(result) = join_set.join_next().await {
        result??;
        if let Some(input) = next_unstopped_case(
            &mut cases,
            scope_stop.clone(),
            precheck_skipper.clone(),
            &progress,
        )
        .await
        {
            spawn_case(
                &mut join_set,
                input,
                client.clone(),
                config.request.clone(),
                limiter.clone(),
                config.execution.delay,
                config.input.encoders.clone(),
                writer.clone(),
                error_logger.clone(),
                matcher.clone(),
                replay_client.clone(),
                config.output.output_directory.clone(),
                progress.clone(),
                Some(scope_stop.clone()),
            );
        }
    }
    Ok(())
}

async fn next_unstopped_case(
    cases: &mut InputCases,
    scope_stop: Arc<Mutex<ScopeStopTracker>>,
    precheck_skipper: Option<Arc<PrecheckSkipper>>,
    progress: &ProgressReporter,
) -> Option<InputCase> {
    for input in cases.by_ref() {
        if should_skip_precheck(&input, precheck_skipper.as_deref()) {
            progress.record_skipped_by(1);
            continue;
        }
        if scope_stop.lock().await.should_skip(&input) {
            progress.record_stop_triggered();
            progress.record_skipped_by(1);
        } else {
            return Some(input);
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
async fn run_with_scoped_stop(
    config: Config,
    mut cases: InputCases,
    client: reqwest::Client,
    replay_client: Option<reqwest::Client>,
    limiter: RateLimiter,
    writer: Arc<Mutex<Box<dyn crate::output::ResultWriter>>>,
    error_logger: Arc<Mutex<Option<ErrorLogger>>>,
    matcher: Arc<crate::matcher::legacy::MatcherConfig>,
    progress: ProgressReporter,
    precheck_skipper: Option<Arc<PrecheckSkipper>>,
) -> Result<()> {
    let max_hits = config.future.stop.stop_on_match.unwrap_or(1);
    let fast_precheck_skip_scope = precheck_skipper
        .as_ref()
        .is_some_and(|skipper| skipper.can_fast_skip_scope(&config.future.stop.stop_scope));
    let mut join_set = JoinSet::new();
    while join_set.len() < config.execution.concurrency {
        let Some(scope_cases) = cases.next_scope(&config.future.stop.stop_scope) else {
            break;
        };
        spawn_scope(
            &mut join_set,
            scope_cases,
            max_hits,
            client.clone(),
            config.request.clone(),
            limiter.clone(),
            config.execution.delay,
            config.input.encoders.clone(),
            writer.clone(),
            error_logger.clone(),
            matcher.clone(),
            replay_client.clone(),
            config.output.output_directory.clone(),
            progress.clone(),
            precheck_skipper.clone(),
            fast_precheck_skip_scope,
        );
    }

    while let Some(result) = join_set.join_next().await {
        result??;
        if let Some(scope_cases) = cases.next_scope(&config.future.stop.stop_scope) {
            spawn_scope(
                &mut join_set,
                scope_cases,
                max_hits,
                client.clone(),
                config.request.clone(),
                limiter.clone(),
                config.execution.delay,
                config.input.encoders.clone(),
                writer.clone(),
                error_logger.clone(),
                matcher.clone(),
                replay_client.clone(),
                config.output.output_directory.clone(),
                progress.clone(),
                precheck_skipper.clone(),
                fast_precheck_skip_scope,
            );
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn spawn_scope(
    join_set: &mut JoinSet<Result<()>>,
    mut scope_cases: ScopeCases,
    max_hits: usize,
    client: reqwest::Client,
    request_config: crate::config::RequestConfig,
    limiter: RateLimiter,
    delay: crate::engine::rate_limiter::DelayConfig,
    encoders: crate::input::encoder::EncoderSet,
    writer: Arc<Mutex<Box<dyn crate::output::ResultWriter>>>,
    error_logger: Arc<Mutex<Option<ErrorLogger>>>,
    matcher: Arc<crate::matcher::legacy::MatcherConfig>,
    replay_client: Option<reqwest::Client>,
    output_directory: Option<String>,
    progress: ProgressReporter,
    precheck_skipper: Option<Arc<PrecheckSkipper>>,
    fast_precheck_skip_scope: bool,
) {
    join_set.spawn(async move {
        let mut hits = 0usize;
        while let Some(input) = scope_cases.next() {
            if hits >= max_hits {
                progress.record_stop_triggered();
                progress.record_skipped_by(1 + scope_cases.len_remaining());
                break;
            }
            if should_skip_precheck(&input, precheck_skipper.as_deref()) {
                let skipped = if fast_precheck_skip_scope {
                    1 + scope_cases.len_remaining()
                } else {
                    1
                };
                progress.record_skipped_by(skipped);
                if fast_precheck_skip_scope {
                    break;
                }
                continue;
            }

            match worker::execute_case(
                client.clone(),
                request_config.clone(),
                limiter.clone(),
                delay,
                encoders.clone(),
                input.clone(),
            )
            .await
            {
                Ok(result) => {
                    let signature = result.response.signature.clone();
                    let matched = handle_match_result(
                        result,
                        matcher.as_ref(),
                        writer.clone(),
                        replay_client.clone(),
                        output_directory.clone(),
                    )
                    .await?;
                    progress.record_response_signature(matched, &signature);
                    if matched {
                        hits += 1;
                    }
                }
                Err(error) => {
                    let kind = worker::classify_error(&error.error);
                    let elapsed_ms = error.elapsed_ms;
                    write_worker_error_log(error_logger.clone(), &error).await?;
                    progress.record_error(kind, elapsed_ms);
                }
            }
        }
        Ok(())
    });
}

fn should_skip_precheck(input: &InputCase, precheck_skipper: Option<&PrecheckSkipper>) -> bool {
    precheck_skipper.is_some_and(|skipper| skipper.should_skip(input))
}

async fn write_worker_error_log(
    error_logger: Arc<Mutex<Option<ErrorLogger>>>,
    error: &worker::WorkerError,
) -> Result<()> {
    if let Some(logger) = error_logger.lock().await.as_mut() {
        logger.write_case_error(&error.input, error.url.as_deref(), &error.error)?;
    }
    Ok(())
}

async fn flush_error_logger(error_logger: Arc<Mutex<Option<ErrorLogger>>>) -> Result<()> {
    if let Some(logger) = error_logger.lock().await.as_mut() {
        logger.flush()?;
    }
    Ok(())
}

async fn flush_writer(writer: Arc<Mutex<Box<dyn crate::output::ResultWriter>>>) -> Result<()> {
    writer.lock().await.flush()
}

#[derive(Debug)]
struct ScopeStopTracker {
    keywords: Vec<String>,
    max_hits: usize,
    hits: HashMap<Vec<String>, usize>,
}

impl ScopeStopTracker {
    fn new(keywords: Vec<String>, max_hits: usize) -> Self {
        Self {
            keywords,
            max_hits,
            hits: HashMap::new(),
        }
    }

    fn should_skip(&self, input: &InputCase) -> bool {
        if self.max_hits == 0 {
            return true;
        }
        self.hits
            .get(&self.key(input))
            .is_some_and(|hits| *hits >= self.max_hits)
    }

    fn record_match(&mut self, input: &InputCase) {
        *self.hits.entry(self.key(input)).or_default() += 1;
    }

    fn key(&self, input: &InputCase) -> Vec<String> {
        self.keywords
            .iter()
            .map(|keyword| input.values.get(keyword).cloned().unwrap_or_default())
            .collect()
    }
}

async fn handle_match_result(
    mut result: worker::WorkerResult,
    matcher: &crate::matcher::legacy::MatcherConfig,
    writer: Arc<Mutex<Box<dyn crate::output::ResultWriter>>>,
    replay_client: Option<reqwest::Client>,
    output_directory: Option<String>,
) -> Result<bool> {
    let matched = if matcher.uses_response_body() {
        let (signature, raw) = result.response.signature_and_raw_response();
        matcher.should_output(signature, raw)
    } else {
        matcher.should_output(&result.response.signature, "")
    };
    if !matched {
        return Ok(false);
    }

    result.response.ensure_title();
    let response_raw = result.response.raw_response().to_string();
    let raw = RawExchange {
        request: result.request_raw,
        response: response_raw,
    };
    let record = OutputRecord::new(
        result.url,
        result.input.display,
        result.input.values,
        &result.response.signature,
    );
    if let Some(dir) = output_directory {
        let _ = save_raw_exchange(&dir, &record, &raw)?;
    }
    writer.lock().await.write_record(&record, Some(&raw))?;
    if let Some(replay_client) = replay_client {
        let _ = client::execute(
            &replay_client,
            &result.rendered_request,
            ResponseBodyConfig::ignore(),
        )
        .await;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashSet};
    use std::sync::{Arc as StdArc, Mutex as StdMutex};

    use clap::Parser;
    use regex::Regex;

    use super::*;
    use crate::http::request::RenderedRequest;
    use crate::http::response::ResponseSummary;
    use crate::input::modes::InputCase;
    use crate::matcher::legacy::MatcherConfig;
    use crate::matcher::signature::ResponseSignature;
    use crate::output::{OutputRecord, ResultWriter};

    fn wl(keyword: &str, values: &[&str]) -> crate::input::wordlist::WordlistData {
        crate::input::wordlist::WordlistData {
            keyword: keyword.to_string(),
            values: values.iter().map(|value| value.to_string()).collect(),
        }
    }

    struct CollectingWriter {
        records: StdArc<StdMutex<Vec<OutputRecord>>>,
    }

    impl ResultWriter for CollectingWriter {
        fn write_record(
            &mut self,
            record: &OutputRecord,
            _raw: Option<&crate::output::RawExchange>,
        ) -> Result<()> {
            self.records.lock().unwrap().push(record.clone());
            Ok(())
        }
    }

    #[tokio::test]
    async fn regex_matcher_checks_complete_raw_response() {
        let records = StdArc::new(StdMutex::new(Vec::new()));
        let writer: Arc<Mutex<Box<dyn ResultWriter>>> =
            Arc::new(Mutex::new(Box::new(CollectingWriter {
                records: records.clone(),
            })));
        let matcher = MatcherConfig {
            match_regex: vec![Regex::new("Set-Cookie: session_id=").unwrap()],
            ..MatcherConfig::default()
        };
        let result = worker::WorkerResult {
            input: InputCase {
                values: BTreeMap::from([("FUZZ".to_string(), "admin".to_string())]),
                display: "admin".to_string(),
            },
            url: "https://example.com/login".to_string(),
            request_raw: "POST /login HTTP/1.1\r\n\r\n".to_string(),
            rendered_request: RenderedRequest {
                method: reqwest::Method::POST,
                url: "https://example.com/login".to_string(),
                headers: Vec::new(),
                body: None,
                raw: "POST /login HTTP/1.1\r\n\r\n".to_string(),
            },
            response: ResponseSummary::from_parts(
                ResponseSignature {
                    status: 200,
                    size: 2,
                    words: 1,
                    lines: 1,
                    elapsed_ms: 10,
                    location: None,
                    title: None,
                    body_hash: 0,
                },
                "HTTP/1.1 200\r\nSet-Cookie: session_id=abc\r\n\r\nok".to_string(),
            ),
        };

        let matched = handle_match_result(result, &matcher, writer, None, None)
            .await
            .unwrap();

        assert!(matched);
        assert_eq!(records.lock().unwrap().len(), 1);
    }

    #[test]
    fn scope_stop_tracker_skips_non_contiguous_scopes_after_match() {
        let mut tracker = ScopeStopTracker::new(vec!["URLFUZZ".to_string()], 1);
        let url1_first = InputCase {
            values: BTreeMap::from([
                ("URLFUZZ".to_string(), "url1".to_string()),
                ("UFUZZ".to_string(), "alice".to_string()),
                ("PFUZZ".to_string(), "p1".to_string()),
            ]),
            display: "URLFUZZ=url1,UFUZZ=alice,PFUZZ=p1".to_string(),
        };
        let url2 = InputCase {
            values: BTreeMap::from([
                ("URLFUZZ".to_string(), "url2".to_string()),
                ("UFUZZ".to_string(), "alice".to_string()),
                ("PFUZZ".to_string(), "p1".to_string()),
            ]),
            display: "URLFUZZ=url2,UFUZZ=alice,PFUZZ=p1".to_string(),
        };
        let url1_later = InputCase {
            values: BTreeMap::from([
                ("URLFUZZ".to_string(), "url1".to_string()),
                ("UFUZZ".to_string(), "alice".to_string()),
                ("PFUZZ".to_string(), "p2".to_string()),
            ]),
            display: "URLFUZZ=url1,UFUZZ=alice,PFUZZ=p2".to_string(),
        };

        assert!(!tracker.should_skip(&url1_first));
        tracker.record_match(&url1_first);
        assert!(!tracker.should_skip(&url2));
        assert!(tracker.should_skip(&url1_later));
    }

    #[test]
    fn fast_precheck_filter_skips_failed_clusterbomb_values_as_full_product() {
        let cli = crate::cli::Cli::parse_from([
            "rfuzz",
            "-u",
            "URLFUZZ/login",
            "-w",
            "urls.txt:URLFUZZ",
            "-w",
            "users.txt:UFUZZ",
            "-w",
            "passes.txt:PFUZZ",
            "--precheck-key",
            "URLFUZZ",
        ]);
        let config = Config::try_from(cli).unwrap();
        let mut wordlists = vec![
            wl("URLFUZZ", &["u1", "u2", "u3"]),
            wl("UFUZZ", &["alice", "bob"]),
            wl("PFUZZ", &["p1", "p2"]),
        ];
        let skipper = PrecheckSkipper::new(
            "URLFUZZ".to_string(),
            HashSet::from(["u1".to_string(), "u3".to_string()]),
        );

        let skipped = fast_filter_precheck_failures(&config, &mut wordlists, Some(&skipper));

        assert_eq!(skipped, 8);
        assert_eq!(wordlists[0].values, vec!["u2"]);
    }

    #[test]
    fn case_queue_capacity_is_bounded() {
        assert_eq!(case_queue_capacity(1), 20);
        assert_eq!(case_queue_capacity(100), 2000);
        assert_eq!(case_queue_capacity(10_000), 10_000);
    }
}
