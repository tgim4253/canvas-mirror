use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use canvas_mirror_model::{LogEntryDto, LogLevel};
use chrono::{Local, Utc};

#[derive(Debug, Clone)]
pub struct RuntimeLogFile {
    path: PathBuf,
    file: Arc<Mutex<File>>,
}

impl RuntimeLogFile {
    pub fn create(log_dir: impl AsRef<Path>, prefix: &str) -> io::Result<Self> {
        let log_dir = log_dir.as_ref();
        fs::create_dir_all(log_dir)?;
        let file_name = format!("{}-{}.log", prefix, Local::now().format("%Y%m%d-%H%M%S"));
        let path = log_dir.join(file_name);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;

        Ok(Self {
            path,
            file: Arc::new(Mutex::new(file)),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append_runtime_logs(&self, logs: &[LogEntryDto]) -> io::Result<()> {
        if logs.is_empty() {
            return Ok(());
        }

        let mut file = self.lock_file()?;
        for log in logs {
            writeln!(
                file,
                "{} {} [{}] {}",
                log.at.to_rfc3339(),
                format_log_level(&log.level),
                log.scope,
                log.message
            )?;
        }
        file.flush()
    }

    pub fn append_line(
        &self,
        level: &str,
        scope: &str,
        message: impl AsRef<str>,
    ) -> io::Result<()> {
        let mut file = self.lock_file()?;
        writeln!(
            file,
            "{} {} [{}] {}",
            Utc::now().to_rfc3339(),
            level,
            scope,
            message.as_ref()
        )?;
        file.flush()
    }

    fn lock_file(&self) -> io::Result<std::sync::MutexGuard<'_, File>> {
        self.file
            .lock()
            .map_err(|_| io::Error::other("runtime log file lock poisoned"))
    }
}

fn format_log_level(level: &LogLevel) -> &'static str {
    match level {
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use canvas_mirror_model::{LogEntryDto, LogLevel};
    use chrono::Utc;
    use uuid::Uuid;

    use super::*;

    #[test]
    fn creates_log_file_and_appends_entries() {
        let dir = std::env::temp_dir().join(format!(
            "canvas-mirror-runtime-log-{}",
            Uuid::new_v4().simple()
        ));
        let log_file = RuntimeLogFile::create(&dir, "runtime").expect("log file should be created");

        log_file
            .append_line("info", "test", "runtime log initialized")
            .expect("plain log line should be appended");
        log_file
            .append_runtime_logs(&[LogEntryDto {
                at: Utc::now(),
                level: LogLevel::Warn,
                scope: "room".to_string(),
                message: "example warning".to_string(),
            }])
            .expect("runtime logs should be appended");

        let contents = fs::read_to_string(log_file.path()).expect("log file should be readable");
        assert!(contents.contains("[test] runtime log initialized"));
        assert!(contents.contains("warn [room] example warning"));

        drop(log_file);
        fs::remove_dir_all(&dir).expect("temporary log directory should be removed");
    }
}
