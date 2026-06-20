use async_trait::async_trait;
use regex::Regex;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use uuid::Uuid;

use crate::log_store::LogStore;
use crate::types::{ExecutionStatus, LogMatch};

#[derive(Clone)]
pub struct FileLogStore {
    dir: PathBuf,
}

impl FileLogStore {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        std::fs::create_dir_all(&dir).ok();
        Self { dir }
    }

    fn log_path(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{}.log", id))
    }

    fn status_path(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{}.status", id))
    }

    fn meta_path(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{}.meta", id))
    }
}

#[async_trait]
impl LogStore for FileLogStore {
    async fn create(&self, id: Uuid, cmd: String) -> Result<(), String> {
        let log_path = self.log_path(id);
        let status_path = self.status_path(id);
        let meta_path = self.meta_path(id);

        // Atomically create log file, fails if already exists
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&log_path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    format!("Execution {} already exists", id)
                } else {
                    e.to_string()
                }
            })?;

        std::fs::write(&meta_path, &cmd).map_err(|e| e.to_string())?;

        let status = serde_json::to_string(&ExecutionStatus::Running)
            .map_err(|e| e.to_string())?;
        std::fs::write(&status_path, status).map_err(|e| e.to_string())?;

        Ok(())
    }

    async fn append(&self, id: Uuid, data: &[u8]) -> Result<(), String> {
        let path = self.log_path(id);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .map_err(|e| format!("Failed to open log file: {}", e))?;
        file.write_all(data).map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn read(&self, id: Uuid, offset: u64) -> Result<(Vec<u8>, u64), String> {
        let path = self.log_path(id);
        let mut file = std::fs::File::open(&path)
            .map_err(|e| format!("Failed to open log file: {}", e))?;

        let file_len = file.metadata().map_err(|e| e.to_string())?.len();

        if offset >= file_len {
            return Ok((Vec::new(), file_len));
        }

        file.seek(SeekFrom::Start(offset)).map_err(|e| e.to_string())?;

        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).map_err(|e| e.to_string())?;

        Ok((buffer, file_len))
    }

    async fn search(&self, id: Uuid, pattern: &str) -> Result<Vec<LogMatch>, String> {
        let path = self.log_path(id);
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read log file: {}", e))?;

        let regex = Regex::new(pattern).map_err(|e| format!("Invalid regex: {}", e))?;

        let mut matches = Vec::new();
        let mut byte_offset: u64 = 0;

        for line in content.lines() {
            if regex.is_match(line) {
                matches.push(LogMatch {
                    line: line.to_string(),
                    offset: byte_offset,
                });
            }
            byte_offset += line.len() as u64 + 1;
        }

        Ok(matches)
    }

    async fn get_status(&self, id: Uuid) -> Result<ExecutionStatus, String> {
        let path = self.status_path(id);
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read status file: {}", e))?;
        serde_json::from_str(&content).map_err(|e| e.to_string())
    }

    async fn set_status(&self, id: Uuid, status: ExecutionStatus) -> Result<(), String> {
        let path = self.status_path(id);
        let content = serde_json::to_string(&status).map_err(|e| e.to_string())?;
        std::fs::write(&path, content).map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn list_executions(&self) -> Result<Vec<(Uuid, ExecutionStatus)>, String> {
        let entries = std::fs::read_dir(&self.dir)
            .map_err(|e| format!("Failed to read storage directory: {}", e))?;

        let mut executions = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("status") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Ok(id) = Uuid::parse_str(stem) else {
                continue;
            };
            if let Ok(status) = self.get_status(id).await {
                executions.push((id, status));
            }
        }
        Ok(executions)
    }
}
