use std::fs::File;
use std::io::{BufWriter, Write};

use anyhow::Result;
use serde::Serialize;

use crate::input::modes::InputCase;
use crate::template::render::InputMap;

#[derive(Debug, Serialize)]
pub struct ErrorLogRecord {
    pub url: Option<String>,
    pub input: String,
    pub input_values: InputMap,
    pub error: String,
}

pub struct ErrorLogger {
    writer: BufWriter<File>,
}

impl ErrorLogger {
    pub fn new(path: &str) -> Result<Self> {
        Ok(Self {
            writer: BufWriter::new(File::create(path)?),
        })
    }

    pub fn write_case_error(
        &mut self,
        input: &InputCase,
        url: Option<&str>,
        error: &anyhow::Error,
    ) -> Result<()> {
        let record = ErrorLogRecord {
            url: url.map(ToOwned::to_owned),
            input: input.display.clone(),
            input_values: input.values.clone(),
            error: format!("{:#}", error),
        };
        serde_json::to_writer(&mut self.writer, &record)?;
        writeln!(self.writer)?;
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }
}

pub fn build_error_logger(path: Option<&str>) -> Result<Option<ErrorLogger>> {
    path.map(ErrorLogger::new).transpose()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use anyhow::anyhow;

    use super::*;

    #[test]
    fn writes_error_log_jsonl_record() {
        let path = std::env::temp_dir().join(format!(
            "rfuzz-error-log-{}.jsonl",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let input = InputCase {
            display: "USER=admin,PASS=secret".to_string(),
            values: BTreeMap::from([
                ("USER".to_string(), "admin".to_string()),
                ("PASS".to_string(), "secret".to_string()),
            ]),
        };

        let mut logger = ErrorLogger::new(path.to_str().unwrap()).unwrap();
        logger
            .write_case_error(
                &input,
                Some("https://example.com/login"),
                &anyhow!("operation timed out"),
            )
            .unwrap();
        logger.flush().unwrap();

        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("\"url\":\"https://example.com/login\""));
        assert!(contents.contains("\"USER\":\"admin\""));
        assert!(contents.contains("operation timed out"));

        let _ = fs::remove_file(path);
    }
}
