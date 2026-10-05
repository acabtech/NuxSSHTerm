//! Minimal file logger for backend errors.
//!
//! Writes one line per error to `~/.config/nuxsshterm/log` (append-only). Kept
//! dependency-free: a Unix-epoch timestamp is good enough for a diagnostic log.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

pub fn log_path() -> PathBuf {
    crate::store::config_dir().join("log")
}

/// Append an error line to the log file. Never panics — logging is best-effort.
pub fn error(context: &str, err: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "?".into());
    let line = format!("[{ts}] {context}: {err}\n");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_path()) {
        let _ = f.write_all(line.as_bytes());
    }
}