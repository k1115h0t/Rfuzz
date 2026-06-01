pub mod console;
pub mod csv;
pub mod error_log;
pub mod jsonl;
pub mod raw;

use std::fs::File;
use std::io::{self, BufWriter, Write};

use anyhow::Result;
use serde::Serialize;

use crate::config::{OutputConfig, OutputFormat};
use crate::matcher::signature::ResponseSignature;
use crate::template::render::InputMap;

#[derive(Debug, Clone, Serialize)]
pub struct OutputRecord {
    pub url: String,
    pub status: u16,
    pub size: usize,
    pub words: usize,
    pub lines: usize,
    pub time_ms: u128,
    pub input: String,
    pub input_values: InputMap,
    pub location: Option<String>,
    pub title: Option<String>,
    pub body_hash: u64,
}

#[derive(Debug, Clone)]
pub struct RawExchange {
    pub request: String,
    pub response: String,
}

impl OutputRecord {
    pub fn new(
        url: String,
        display: String,
        input_values: InputMap,
        sig: &ResponseSignature,
    ) -> Self {
        Self {
            url,
            status: sig.status,
            size: sig.size,
            words: sig.words,
            lines: sig.lines,
            time_ms: sig.elapsed_ms,
            input: display,
            input_values,
            location: sig.location.clone(),
            title: sig.title.clone(),
            body_hash: sig.body_hash,
        }
    }
}

pub trait ResultWriter: Send {
    fn write_record(&mut self, record: &OutputRecord, raw: Option<&RawExchange>) -> Result<()>;
}

pub fn build_writer(config: &OutputConfig) -> Result<Box<dyn ResultWriter>> {
    match config.format {
        OutputFormat::Console => Ok(Box::new(console::ConsoleWriter::new(
            open_text_writer(config.path.as_deref())?,
            config.silent,
        ))),
        OutputFormat::Jsonl => Ok(Box::new(jsonl::JsonlWriter::new(open_text_writer(
            config.path.as_deref(),
        )?))),
        OutputFormat::Csv => Ok(Box::new(csv::CsvResultWriter::new(open_text_writer(
            config.path.as_deref(),
        )?))),
    }
}

fn open_text_writer(path: Option<&str>) -> Result<Box<dyn Write + Send>> {
    if let Some(path) = path {
        Ok(Box::new(BufWriter::new(File::create(path)?)))
    } else {
        Ok(Box::new(BufWriter::new(io::stdout())))
    }
}
