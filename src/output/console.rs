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
        if self.silent {
            writeln!(self.writer, "{}", record.url)?;
        } else {
            writeln!(
                self.writer,
                "{} [Status: {}, Size: {}, Words: {}, Lines: {}]",
                record.input, record.status, record.size, record.words, record.lines
            )?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }
}
