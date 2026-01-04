use async_trait::async_trait;
use regex::Regex;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::log_store::LogStore;
use crate::types::{ExecutionStatus, LogMatch};

struct ExecutionData {
    cmd: String,
    status: ExecutionStatus,
    logs: Vec<u8>,
}

#[derive(Clone)]
pub struct InMemoryLogStore {
    data: Arc<RwLock<HashMap<Uuid, ExecutionData>>>,
}

impl InMemoryLogStore {
    pub fn new() -> Self {
        Self {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl LogStore for InMemoryLogStore {
    async fn create(&self, id: Uuid, cmd: String) -> Result<(), String> {
        let mut data = self.data.write().await;
        if data.contains_key(&id) {
            return Err(format!("Execution {} already exists", id));
        }
        data.insert(id, ExecutionData {
            cmd,
            status: ExecutionStatus::Running,
            logs: Vec::new(),
        });
        Ok(())
    }

    async fn append(&self, id: Uuid, bytes: &[u8]) -> Result<(), String> {
        let mut data = self.data.write().await;
        let exec = data.get_mut(&id).ok_or_else(|| format!("Execution {} not found", id))?;
        exec.logs.extend_from_slice(bytes);
        Ok(())
    }

    async fn read(&self, id: Uuid, offset: u64) -> Result<(Vec<u8>, u64), String> {
        let data = self.data.read().await;
        let exec = data.get(&id).ok_or_else(|| format!("Execution {} not found", id))?;
        let offset = offset as usize;
        if offset >= exec.logs.len() {
            return Ok((Vec::new(), exec.logs.len() as u64));
        }
        let bytes = exec.logs[offset..].to_vec();
        let new_offset = exec.logs.len() as u64;
        Ok((bytes, new_offset))
    }

    async fn search(&self, id: Uuid, pattern: &str) -> Result<Vec<LogMatch>, String> {
        let data = self.data.read().await;
        let exec = data.get(&id).ok_or_else(|| format!("Execution {} not found", id))?;

        let regex = Regex::new(pattern).map_err(|e| format!("Invalid regex: {}", e))?;
        let logs_str = String::from_utf8_lossy(&exec.logs);

        let mut matches = Vec::new();
        let mut byte_offset: u64 = 0;

        for line in logs_str.lines() {
            if regex.is_match(line) {
                matches.push(LogMatch {
                    line: line.to_string(),
                    offset: byte_offset,
                });
            }
            byte_offset += line.len() as u64 + 1; // +1 for newline
        }

        Ok(matches)
    }

    async fn get_status(&self, id: Uuid) -> Result<ExecutionStatus, String> {
        let data = self.data.read().await;
        let exec = data.get(&id).ok_or_else(|| format!("Execution {} not found", id))?;
        Ok(exec.status.clone())
    }

    async fn set_status(&self, id: Uuid, status: ExecutionStatus) -> Result<(), String> {
        let mut data = self.data.write().await;
        let exec = data.get_mut(&id).ok_or_else(|| format!("Execution {} not found", id))?;
        exec.status = status;
        Ok(())
    }
}
