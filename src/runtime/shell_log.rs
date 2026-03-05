use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use std::time::Instant;

pub struct ShellLogger {
    stdin_raw: BufWriter<File>,
    stdin_log: BufWriter<File>,
    stdout_raw: BufWriter<File>,
    stdout_log: BufWriter<File>,
    test_start: Instant,
    stdin_at_line_start: bool,
    stdout_at_line_start: bool,
}

impl ShellLogger {
    pub fn create(log_dir: &Path, scoped_name: &str, test_start: Instant) -> io::Result<Self> {
        std::fs::create_dir_all(log_dir)?;
        let open = |suffix: &str| -> io::Result<BufWriter<File>> {
            let path = log_dir.join(format!("{scoped_name}.{suffix}"));
            Ok(BufWriter::new(File::create(path)?))
        };
        Ok(Self {
            stdin_raw: open("stdin.raw")?,
            stdin_log: open("stdin.log")?,
            stdout_raw: open("stdout.raw")?,
            stdout_log: open("stdout.log")?,
            test_start,
            stdin_at_line_start: true,
            stdout_at_line_start: true,
        })
    }

    pub fn log_stdin(&mut self, data: &[u8]) {
        let _ = self.stdin_raw.write_all(data);
        let _ = self.stdin_raw.flush();
        self.stdin_at_line_start =
            write_timestamped(&mut self.stdin_log, data, self.stdin_at_line_start, &self.test_start);
        let _ = self.stdin_log.flush();
    }

    pub fn log_stdout(&mut self, data: &[u8]) {
        let _ = self.stdout_raw.write_all(data);
        let _ = self.stdout_raw.flush();
        self.stdout_at_line_start =
            write_timestamped(&mut self.stdout_log, data, self.stdout_at_line_start, &self.test_start);
        let _ = self.stdout_log.flush();
    }
}

/// Writes data with timestamp prefixes inserted only at the beginning of lines.
/// Returns whether the stream is at a line start after writing (i.e. data ended with `\n`).
fn write_timestamped(
    w: &mut BufWriter<File>,
    data: &[u8],
    at_line_start: bool,
    test_start: &Instant,
) -> bool {
    if data.is_empty() {
        return at_line_start;
    }

    let prefix = timestamp_prefix(test_start);
    let mut pos = 0;

    if at_line_start {
        let _ = w.write_all(prefix.as_bytes());
    }

    while pos < data.len() {
        if let Some(nl) = data[pos..].iter().position(|&b| b == b'\n') {
            let end = pos + nl + 1;
            let _ = w.write_all(&data[pos..end]);
            if end < data.len() {
                let _ = w.write_all(prefix.as_bytes());
            }
            pos = end;
        } else {
            let _ = w.write_all(&data[pos..]);
            pos = data.len();
        }
    }

    data.last() == Some(&b'\n')
}

fn timestamp_prefix(test_start: &Instant) -> String {
    let elapsed = test_start.elapsed();
    let secs = elapsed.as_secs();
    let millis = elapsed.subsec_millis();
    format!("[+{secs}.{millis:03}s] ")
}
