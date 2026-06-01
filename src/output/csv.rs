use std::io::Write;

use anyhow::Result;
use serde::Serialize;

use super::{OutputRecord, ResultWriter};

#[derive(Serialize)]
struct CsvRecord<'a> {
    url: &'a str,
    status: u16,
    size: usize,
    words: usize,
    lines: usize,
    time_ms: u128,
    input: &'a str,
}

pub struct CsvResultWriter {
    writer: ::csv::Writer<Box<dyn Write + Send>>,
}

impl CsvResultWriter {
    pub fn new(writer: Box<dyn Write + Send>) -> Self {
        Self {
            writer: ::csv::Writer::from_writer(writer),
        }
    }
}

impl ResultWriter for CsvResultWriter {
    fn write_record(
        &mut self,
        record: &OutputRecord,
        _raw: Option<&super::RawExchange>,
    ) -> Result<()> {
        self.writer.serialize(CsvRecord {
            url: &record.url,
            status: record.status,
            size: record.size,
            words: record.words,
            lines: record.lines,
            time_ms: record.time_ms,
            input: &record.input,
        })?;
        self.writer.flush()?;
        Ok(())
    }
}
