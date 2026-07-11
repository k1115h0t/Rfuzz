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

    fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    use crate::matcher::signature::ResponseSignature;
    use crate::output::OutputRecord;

    use super::JsonlWriter;
    use crate::output::ResultWriter;

    #[derive(Default)]
    struct SharedState {
        bytes: Vec<u8>,
        flushes: usize,
    }

    #[derive(Clone, Default)]
    struct FlushTrackingWriter(Arc<Mutex<SharedState>>);

    impl Write for FlushTrackingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .expect("tracking writer poisoned")
                .bytes
                .extend(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.0.lock().expect("tracking writer poisoned").flushes += 1;
            Ok(())
        }
    }

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

    #[test]
    fn write_record_flushes_immediately() {
        let tracking = FlushTrackingWriter::default();
        let state = Arc::clone(&tracking.0);
        let sig = ResponseSignature {
            status: 200,
            size: 12,
            words: 2,
            lines: 1,
            elapsed_ms: 30,
            location: None,
            title: None,
            body_hash: 42,
        };
        let record = OutputRecord::new(
            "https://example.com/admin".to_string(),
            "admin".to_string(),
            BTreeMap::from([("DIR".to_string(), "admin".to_string())]),
            &sig,
        );
        let mut writer = JsonlWriter::new(Box::new(tracking));

        writer.write_record(&record, None).unwrap();

        let state = state.lock().unwrap();
        assert_eq!(state.flushes, 1);
        assert!(
            String::from_utf8_lossy(&state.bytes).contains("\"url\":\"https://example.com/admin\"")
        );
    }
}
