use std::collections::{BTreeMap, HashMap};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
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

/// How long to keep waiting for a command's output after the command itself has
/// ended. A process that outlives it and still holds the pipe (a daemon it left
/// behind, or one that moved out of the process group) would otherwise keep the
/// execution "running" forever; past this the readers carry on in the background,
/// appending to the log, while the status reflects the command.
const OUTPUT_DRAIN: Duration = Duration::from_secs(1);

/// What to run: a program and its arguments, executed directly (no shell).
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub bin: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
}

pub fn spawn_command<S: LogStore>(
    store: S,
    registry: TaskRegistry,
    cancel: CancellationToken,
    id: Uuid,
    spec: CommandSpec,
    timeout_secs: u64,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let result = run_command(&store, &cancel, id, &spec, timeout_secs).await;


        let status = match result {
            // A kill or tasks/cancel that raced with the command's own exit still wins.
            _ if cancel.is_cancelled() => ExecutionStatus::Cancelled,
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

/// Copy a stream into the log line by line, as bytes: output that is not valid
/// UTF-8 is stored as it is (readers of the log decode it lossily), instead of
/// ending the stream at the first such line.
fn pump<S: LogStore, R: AsyncRead + Unpin + Send + 'static>(store: S, id: Uuid, stream: R, name: &'static str) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream);
        let mut line = Vec::new();
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    // Keep the log line-oriented: offsets from search_logs assume it.
                    if line.last() != Some(&b'\n') {
                        line.push(b'\n');
                    }
                    if let Err(e) = store.append(id, &line).await {
                        tracing::error!("Failed to append {}: {}", name, e);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to read {}: {}", name, e);
                    break;
                }
            }
        }
    })
}

/// Kill the command and everything it started: it leads a process group of its own
/// (see `run_command`), so one signal reaches its children too. Killing only the
/// direct child would leave those running, holding the output pipes open.
fn kill_group(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // SAFETY: killpg only sends a signal; the group id is the pid of a child we
        // spawned as a group leader and have not yet reaped.
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGKILL);
        }
    }
    // Also covers non-unix targets, and a group that is already gone.
    let _ = child.start_kill();
}

/// Wait for the output readers, but not forever (see `OUTPUT_DRAIN`).
async fn drain(stdout: JoinHandle<()>, stderr: JoinHandle<()>) {
    let _ = timeout(OUTPUT_DRAIN, async {
        let _ = stdout.await;
        let _ = stderr.await;
    })
    .await;
}

async fn run_command<S: LogStore>(
    store: &S,
    cancel: &CancellationToken,
    id: Uuid,
    spec: &CommandSpec,
    timeout_secs: u64,
) -> Result<i32, ExecutorError> {
    let mut command = Command::new(&spec.bin);
    command
        .args(&spec.args)
        .envs(&spec.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    // Its own process group, so that a timeout or a kill ends what it started too.
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command
        .spawn()
        .map_err(|e| ExecutorError::Failed(format!("Failed to start {:?}: {}", spec.bin, e)))?;

    let stdout = child.stdout.take().ok_or_else(|| ExecutorError::Failed("Failed to capture stdout".to_string()))?;
    let stderr = child.stderr.take().ok_or_else(|| ExecutorError::Failed("Failed to capture stderr".to_string()))?;

    let stdout_handle = pump(store.clone(), id, stdout, "stdout");
    let stderr_handle = pump(store.clone(), id, stderr, "stderr");

    // Wait for the process, racing the timeout against an explicit cancellation.
    let wait_result = tokio::select! {
        result = timeout(Duration::from_secs(timeout_secs), child.wait()) => result,
        _ = cancel.cancelled() => {
            kill_group(&mut child);
            let _ = child.wait().await;
            drain(stdout_handle, stderr_handle).await;
            return Err(ExecutorError::Cancelled);
        }
    };

    match wait_result {
        Ok(Ok(exit_status)) => {
            drain(stdout_handle, stderr_handle).await;
            Ok(exit_status.code().unwrap_or(-1))
        }
        Ok(Err(e)) => {
            drain(stdout_handle, stderr_handle).await;
            Err(ExecutorError::Failed(format!("Process error: {}", e)))
        }
        Err(_) => {
            kill_group(&mut child);
            let _ = child.wait().await;
            drain(stdout_handle, stderr_handle).await;
            Err(ExecutorError::Timeout)
        }
    }
}
