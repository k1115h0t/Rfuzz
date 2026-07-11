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

    fn flush(&mut self) -> Result<()> {
        Ok(())
    }
}

pub fn build_writer(
    config: &OutputConfig,
    mirror_writer: Option<Box<dyn Write + Send>>,
) -> Result<Box<dyn ResultWriter>> {
    let writer: Box<dyn ResultWriter> = match config.format {
        OutputFormat::Console => Box::new(console::ConsoleWriter::new(
            open_text_writer(config.path.as_deref())?,
            config.silent,
        )),
        OutputFormat::Jsonl => Box::new(jsonl::JsonlWriter::new(open_text_writer(
            config.path.as_deref(),
        )?)),
        OutputFormat::Csv => Box::new(csv::CsvResultWriter::new(open_text_writer(
            config.path.as_deref(),
        )?)),
    };

    if config.path.is_some() {
        Ok(Box::new(MirroredResultWriter::new(
            writer,
            Box::new(console::ConsoleWriter::new(
                mirror_writer.unwrap_or_else(open_stderr_writer),
                config.silent,
            )),
        )))
    } else {
        Ok(writer)
    }
}

fn open_text_writer(path: Option<&str>) -> Result<Box<dyn Write + Send>> {
    if let Some(path) = path {
        Ok(Box::new(BufWriter::new(File::create(path)?)))
    } else {
        Ok(Box::new(BufWriter::new(io::stdout())))
    }
}

fn open_stderr_writer() -> Box<dyn Write + Send> {
    Box::new(BufWriter::new(io::stderr()))
}

struct MirroredResultWriter {
    primary: Box<dyn ResultWriter>,
    mirror: Box<dyn ResultWriter>,
}

impl MirroredResultWriter {
    fn new(primary: Box<dyn ResultWriter>, mirror: Box<dyn ResultWriter>) -> Self {
        Self { primary, mirror }
    }
}

impl ResultWriter for MirroredResultWriter {
    fn write_record(&mut self, record: &OutputRecord, raw: Option<&RawExchange>) -> Result<()> {
        self.primary.write_record(record, raw)?;
        self.mirror.write_record(record, raw)?;
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.primary.flush()?;
        self.mirror.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::Write;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::config::{OutputConfig, OutputFormat};

    use super::*;

    #[derive(Clone)]
    struct SharedBuffer(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBuffer {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("mirror buffer poisoned").extend(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn file_output_mirrors_matches_to_injected_writer() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before epoch")
            .as_nanos();
        let output_path = std::env::temp_dir().join(format!("rfuzz-mirror-test-{unique}.jsonl"));
        let mirror = Arc::new(Mutex::new(Vec::new()));
        let config = OutputConfig {
            path: Some(output_path.to_string_lossy().into_owned()),
            format: OutputFormat::Jsonl,
            silent: false,
            output_directory: None,
            error_log: None,
            summary_json: None,
            progress: true,
        };
        let record = OutputRecord::new(
            "https://example.com/login".to_string(),
            "PASS=admin,USER=alice".to_string(),
            BTreeMap::from([
                ("PASS".to_string(), "admin".to_string()),
                ("USER".to_string(), "alice".to_string()),
            ]),
            &ResponseSignature {
                status: 200,
                size: 12,
                words: 2,
                lines: 1,
                elapsed_ms: 35,
                location: None,
                title: None,
                body_hash: 42,
            },
        );

        let mut writer =
            build_writer(&config, Some(Box::new(SharedBuffer(Arc::clone(&mirror))))).unwrap();
        writer.write_record(&record, None).unwrap();
        writer.flush().unwrap();

        let mirrored = String::from_utf8(mirror.lock().unwrap().clone()).unwrap();
        assert!(mirrored
            .contains("[MATCH] PASS=admin,USER=alice -> https://example.com/login [Status: 200"));

        let file_output = std::fs::read_to_string(&output_path).unwrap();
        assert!(file_output.contains("\"url\":\"https://example.com/login\""));
        let _ = std::fs::remove_file(output_path);
    }
}
