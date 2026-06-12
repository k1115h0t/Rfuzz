use std::io::Write;

use anyhow::Result;

use super::{OutputRecord, ResultWriter};

pub struct ConsoleWriter {
    writer: Box<dyn Write + Send>,
    silent: bool,
}

impl ConsoleWriter {
    pub fn new(writer: Box<dyn Write + Send>, silent: bool) -> Self {
        Self { writer, silent }
    }
}

impl ResultWriter for ConsoleWriter {
    fn write_record(
        &mut self,
        record: &OutputRecord,
        _raw: Option<&super::RawExchange>,
    ) -> Result<()> {
        writeln!(self.writer, "{}", format_record(record, self.silent))?;
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }
}

fn format_record(record: &OutputRecord, silent: bool) -> String {
    if silent {
        return record.url.clone();
    }

    format!(
        "[MATCH] {} -> {} [Status: {}, Size: {}, Words: {}, Lines: {}, Time: {}ms]",
        record.input,
        record.url,
        record.status,
        record.size,
        record.words,
        record.lines,
        record.time_ms
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::matcher::signature::ResponseSignature;
    use crate::output::OutputRecord;

    use super::format_record;

    #[test]
    fn console_record_shows_matched_input_and_url() {
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

        let line = format_record(&record, false);

        assert_eq!(
            line,
            "[MATCH] PASS=admin,USER=alice -> https://example.com/login [Status: 200, Size: 12, Words: 2, Lines: 1, Time: 35ms]"
        );
    }

    #[test]
    fn silent_console_record_stays_url_only() {
        let record = OutputRecord::new(
            "https://example.com/admin".to_string(),
            "admin".to_string(),
            BTreeMap::from([("DIR".to_string(), "admin".to_string())]),
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

        assert_eq!(format_record(&record, true), "https://example.com/admin");
    }
}
