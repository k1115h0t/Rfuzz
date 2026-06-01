use std::fs;
use std::path::Path;

use anyhow::Result;

use super::{OutputRecord, RawExchange};

pub fn save_raw_exchange(dir: &str, record: &OutputRecord, raw: &RawExchange) -> Result<String> {
    fs::create_dir_all(dir)?;
    let filename = format!(
        "{}-{}-{}.txt",
        sanitize(&record.input),
        record.status,
        record.body_hash
    );
    let path = Path::new(dir).join(filename);
    let content = format!(
        "{}\n---- request ----\n{}\n---- response ----\n{}",
        record.url, raw.request, raw.response
    );
    fs::write(&path, content)?;
    Ok(path.to_string_lossy().to_string())
}

fn sanitize(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    sanitized.trim_matches('_').chars().take(80).collect()
}
