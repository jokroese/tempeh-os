use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;

pub(crate) struct CsvLog {
    writer: BufWriter<File>,
}

impl CsvLog {
    pub(crate) fn create(
        path: impl Into<PathBuf>,
        header: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let file = File::create(&path)?;
        let mut writer = BufWriter::new(file);
        writeln!(writer, "{header}")?;
        writer.flush()?;
        eprintln!("Saving CSV log to {}.", path.display());
        Ok(Self { writer })
    }

    pub(crate) fn write_row(&mut self, row: &str) -> Result<(), Box<dyn std::error::Error>> {
        writeln!(self.writer, "{row}")?;
        // Supervised hardware tests are often stopped with Ctrl-C.
        // Flush every row so the file remains useful after interruption.
        self.writer.flush()?;
        Ok(())
    }
}
