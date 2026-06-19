use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::log_store::LogStore;
use crate::types::ExecutionStatus;

/// Runtime registry mapping an execution id to the token used to cancel it.
///
/// Cancellation tokens are inherently runtime-only state (they cannot be
/// persisted like logs/status), so they live here rather than in `LogStore`.
pub type TaskRegistry = Arc<Mutex<HashMap<Uuid, CancellationToken>>>;

pub fn new_task_registry() -> TaskRegistry {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn spawn_command<S: LogStore>(
    store: S,
    registry: TaskRegistry,
    cancel: CancellationToken,
    id: Uuid,
    cmd: String,
    timeout_secs: u64,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let result = run_command(&store, &cancel, id, &cmd, timeout_secs).await;

        let status = match result {
            Ok(exit_code) => ExecutionStatus::Completed(exit_code),
            Err(ExecutorError::Timeout) => ExecutionStatus::Timeout,
            Err(ExecutorError::Cancelled) => ExecutionStatus::Cancelled,
            Err(ExecutorError::Failed(e)) => ExecutionStatus::Failed(e),
        };

        if let Err(e) = store.set_status(id, status).await {
            tracing::error!("Failed to set status for {}: {}", id, e);
        }

        // Deregister the cancellation token now that the command has finished.
        registry.lock().await.remove(&id);
    })
}

enum ExecutorError {
    Timeout,
    Cancelled,
    Failed(String),
}

async fn run_command<S: LogStore>(
    store: &S,
    cancel: &CancellationToken,
    id: Uuid,
    cmd: &str,
    timeout_secs: u64,
) -> Result<i32, ExecutorError> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ExecutorError::Failed(format!("Failed to spawn process: {}", e)))?;

    let stdout = child.stdout.take().ok_or_else(|| ExecutorError::Failed("Failed to capture stdout".to_string()))?;
    let stderr = child.stderr.take().ok_or_else(|| ExecutorError::Failed("Failed to capture stderr".to_string()))?;

    let store_stdout = store.clone();
    let store_stderr = store.clone();

    // Stream stdout
    let stdout_handle = tokio::spawn(async move {
        let mut reader = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let line_with_newline = format!("{}\n", line);
            if let Err(e) = store_stdout.append(id, line_with_newline.as_bytes()).await {
                tracing::error!("Failed to append stdout: {}", e);
            }
        }
    });

    // Stream stderr
    let stderr_handle = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let line_with_newline = format!("{}\n", line);
            if let Err(e) = store_stderr.append(id, line_with_newline.as_bytes()).await {
                tracing::error!("Failed to append stderr: {}", e);
            }
        }
    });

    // Wait for the process, racing the timeout against an explicit cancellation.
    let wait_result = tokio::select! {
        result = timeout(Duration::from_secs(timeout_secs), child.wait()) => result,
        _ = cancel.cancelled() => {
            // Cancelled - kill the process and flush remaining output.
            child.kill().await.ok();
            let _ = stdout_handle.await;
            let _ = stderr_handle.await;
            return Err(ExecutorError::Cancelled);
        }
    };

    match wait_result {
        Ok(Ok(exit_status)) => {
            // Wait for output streams to finish
            let _ = stdout_handle.await;
            let _ = stderr_handle.await;
            Ok(exit_status.code().unwrap_or(-1))
        }
        Ok(Err(e)) => {
            let _ = stdout_handle.await;
            let _ = stderr_handle.await;
            Err(ExecutorError::Failed(format!("Process error: {}", e)))
        }
        Err(_) => {
            // Timeout - kill the process
            child.kill().await.ok();
            // Wait for streams to flush remaining output
            let _ = stdout_handle.await;
            let _ = stderr_handle.await;
            Err(ExecutorError::Timeout)
        }
    }
}
