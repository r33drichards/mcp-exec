use async_trait::async_trait;
use uuid::Uuid;
use crate::log_store::LogStore;
use crate::types::{ExecutionStatus, LogMatch};

#[derive(Clone)]
pub struct InMemoryLogStore;

#[async_trait]
impl LogStore for InMemoryLogStore {
    async fn create(&self, _id: Uuid, _cmd: String) -> Result<(), String> { todo!() }
    async fn append(&self, _id: Uuid, _data: &[u8]) -> Result<(), String> { todo!() }
    async fn read(&self, _id: Uuid, _offset: u64) -> Result<(Vec<u8>, u64), String> { todo!() }
    async fn search(&self, _id: Uuid, _pattern: &str) -> Result<Vec<LogMatch>, String> { todo!() }
    async fn get_status(&self, _id: Uuid) -> Result<ExecutionStatus, String> { todo!() }
    async fn set_status(&self, _id: Uuid, _status: ExecutionStatus) -> Result<(), String> { todo!() }
}
