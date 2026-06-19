pub mod memory;
pub mod file;

use async_trait::async_trait;
use uuid::Uuid;
use crate::types::{ExecutionStatus, LogMatch};

#[async_trait]
pub trait LogStore: Send + Sync + Clone + 'static {
    async fn create(&self, id: Uuid, cmd: String) -> Result<(), String>;
    async fn append(&self, id: Uuid, data: &[u8]) -> Result<(), String>;
    async fn read(&self, id: Uuid, offset: u64) -> Result<(Vec<u8>, u64), String>;
    async fn search(&self, id: Uuid, pattern: &str) -> Result<Vec<LogMatch>, String>;
    async fn get_status(&self, id: Uuid) -> Result<ExecutionStatus, String>;
    async fn set_status(&self, id: Uuid, status: ExecutionStatus) -> Result<(), String>;
    /// List all known executions with their current status.
    async fn list_executions(&self) -> Result<Vec<(Uuid, ExecutionStatus)>, String>;
}

#[derive(Clone)]
pub enum AnyLogStore {
    Memory(memory::InMemoryLogStore),
    File(file::FileLogStore),
}

#[async_trait]
impl LogStore for AnyLogStore {
    async fn create(&self, id: Uuid, cmd: String) -> Result<(), String> {
        match self {
            AnyLogStore::Memory(s) => s.create(id, cmd).await,
            AnyLogStore::File(s) => s.create(id, cmd).await,
        }
    }

    async fn append(&self, id: Uuid, data: &[u8]) -> Result<(), String> {
        match self {
            AnyLogStore::Memory(s) => s.append(id, data).await,
            AnyLogStore::File(s) => s.append(id, data).await,
        }
    }

    async fn read(&self, id: Uuid, offset: u64) -> Result<(Vec<u8>, u64), String> {
        match self {
            AnyLogStore::Memory(s) => s.read(id, offset).await,
            AnyLogStore::File(s) => s.read(id, offset).await,
        }
    }

    async fn search(&self, id: Uuid, pattern: &str) -> Result<Vec<LogMatch>, String> {
        match self {
            AnyLogStore::Memory(s) => s.search(id, pattern).await,
            AnyLogStore::File(s) => s.search(id, pattern).await,
        }
    }

    async fn get_status(&self, id: Uuid) -> Result<ExecutionStatus, String> {
        match self {
            AnyLogStore::Memory(s) => s.get_status(id).await,
            AnyLogStore::File(s) => s.get_status(id).await,
        }
    }

    async fn set_status(&self, id: Uuid, status: ExecutionStatus) -> Result<(), String> {
        match self {
            AnyLogStore::Memory(s) => s.set_status(id, status).await,
            AnyLogStore::File(s) => s.set_status(id, status).await,
        }
    }

    async fn list_executions(&self) -> Result<Vec<(Uuid, ExecutionStatus)>, String> {
        match self {
            AnyLogStore::Memory(s) => s.list_executions().await,
            AnyLogStore::File(s) => s.list_executions().await,
        }
    }
}
