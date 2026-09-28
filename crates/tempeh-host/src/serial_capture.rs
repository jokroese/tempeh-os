use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde_json::json;

pub(crate) struct SerialCapture {
    path: PathBuf,
    writer: BufWriter<File>,
}

impl SerialCapture {
    pub(crate) fn create(csv_path: &Path) -> io::Result<Self> {
        let path = csv_path.with_extension("serial.jsonl");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        Ok(Self {
            path,
            writer: BufWriter::new(file),
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn line(&mut self, elapsed_s: f32, line: &str) -> io::Result<()> {
        self.write(json!({
            "received_at_utc": Utc::now().to_rfc3339(),
            "elapsed_s": elapsed_s,
            "kind": "serial",
            "line": line.trim_end_matches(['\r', '\n']),
        }))
    }

    pub(crate) fn event(&mut self, elapsed_s: f32, kind: &str, message: &str) -> io::Result<()> {
        self.write(json!({
            "received_at_utc": Utc::now().to_rfc3339(),
            "elapsed_s": elapsed_s,
            "kind": kind,
            "message": message,
        }))
    }

    fn write(&mut self, value: serde_json::Value) -> io::Result<()> {
        writeln!(self.writer, "{value}")?;
        self.writer.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_original_lines_and_refuses_overwrite() {
        let csv = std::env::temp_dir().join(format!(
            "tempeh-capture-{}-{}.csv",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut capture = SerialCapture::create(&csv).unwrap();
        capture
            .line(1.5, "state,1,fault,actuator_failed\r\n")
            .unwrap();
        capture.event(2.0, "parse", "bad record").unwrap();
        assert_eq!(
            SerialCapture::create(&csv).err().unwrap().kind(),
            io::ErrorKind::AlreadyExists
        );
        let path = capture.path().to_owned();
        drop(capture);
        let lines = fs::read_to_string(&path).unwrap();
        let records: Vec<serde_json::Value> = lines
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(records[0]["line"], "state,1,fault,actuator_failed");
        assert_eq!(records[1]["kind"], "parse");
        fs::remove_file(path).unwrap();
    }
}
