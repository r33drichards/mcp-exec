use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::task::JoinHandle;
use tokio::time::{timeout, Duration};
use uuid::Uuid;

use crate::log_store::LogStore;
use crate::types::ExecutionStatus;

pub fn spawn_command<S: LogStore>(
    store: S,
    id: Uuid,
    cmd: String,
    timeout_secs: u64,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let result = run_command(&store, id, &cmd, timeout_secs).await;

        let status = match result {
            Ok(exit_code) => ExecutionStatus::Completed(exit_code),
            Err(ExecutorError::Timeout) => ExecutionStatus::Timeout,
            Err(ExecutorError::Failed(e)) => ExecutionStatus::Failed(e),
        };

        if let Err(e) = store.set_status(id, status).await {
            tracing::error!("Failed to set status for {}: {}", id, e);
        }
    })
}

enum ExecutorError {
    Timeout,
    Failed(String),
}

async fn run_command<S: LogStore>(
    store: &S,
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

    // Wait for process with timeout
    let wait_result = timeout(Duration::from_secs(timeout_secs), child.wait()).await;

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
