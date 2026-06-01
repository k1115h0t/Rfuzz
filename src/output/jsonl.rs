use std::io::Write;

use anyhow::Result;

use super::{OutputRecord, ResultWriter};

pub struct JsonlWriter {
    writer: Box<dyn Write + Send>,
}

impl JsonlWriter {
    pub fn new(writer: Box<dyn Write + Send>) -> Self {
        Self { writer }
    }
}

impl ResultWriter for JsonlWriter {
    fn write_record(
        &mut self,
        record: &OutputRecord,
        _raw: Option<&super::RawExchange>,
    ) -> Result<()> {
        serde_json::to_writer(&mut self.writer, record)?;
        writeln!(self.writer)?;
        self.writer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::matcher::signature::ResponseSignature;
    use crate::output::OutputRecord;

    #[test]
    fn serializes_jsonl_record() {
        let sig = ResponseSignature {
            status: 200,
            size: 12,
            words: 2,
            lines: 1,
            elapsed_ms: 30,
            location: None,
            title: Some("ok".to_string()),
            body_hash: 42,
        };
        let record = OutputRecord::new(
            "https://example.com/admin".to_string(),
            "admin".to_string(),
            BTreeMap::from([("DIR".to_string(), "admin".to_string())]),
            &sig,
        );
        let line = serde_json::to_string(&record).unwrap();
        assert!(line.contains("\"url\":\"https://example.com/admin\""));
        assert!(line.contains("\"status\":200"));
        assert!(line.contains("\"input\":\"admin\""));
    }
}
