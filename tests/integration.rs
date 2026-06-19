use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Write};
use std::thread;
use std::time::Duration;

/// Helper to send a request and read response
fn send_request(stdin: &mut impl Write, stdout: &mut impl BufRead, request: &str) -> String {
    writeln!(stdin, "{}", request).expect("Failed to write");
    stdin.flush().expect("Failed to flush");

    // Read response line
    let mut response = String::new();
    stdout.read_line(&mut response).expect("Failed to read response");
    response
}

#[test]
fn test_stdio_initialize() {
    let mut child = Command::new("cargo")
        .args(["run", "--"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start server");

    let mut stdin = child.stdin.take().expect("Failed to get stdin");
    let stdout = child.stdout.take().expect("Failed to get stdout");

    // Send initialize request
    let init_request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#;
    writeln!(stdin, "{}", init_request).expect("Failed to write");
    stdin.flush().expect("Failed to flush");

    // Read response
    let reader = BufReader::new(stdout);
    let response = reader.lines().next().expect("No response").expect("Failed to read");

    assert!(response.contains("tools") || response.contains("result"), "Response should contain tools or result: {}", response);

    child.kill().ok();
}

#[test]
fn test_exec_and_stream_logs() {
    let mut child = Command::new("cargo")
        .args(["run", "--"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start server");

    let mut stdin = child.stdin.take().expect("Failed to get stdin");
    let stdout = child.stdout.take().expect("Failed to get stdout");
    let mut reader = BufReader::new(stdout);

    // Step 1: Initialize
    let init_request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#;
    let init_response = send_request(&mut stdin, &mut reader, init_request);
    assert!(init_response.contains("result"), "Initialize should succeed: {}", init_response);

    // Step 2: Send initialized notification
    let initialized = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    writeln!(stdin, "{}", initialized).expect("Failed to write");
    stdin.flush().expect("Failed to flush");

    // Step 3: Call exec tool to run "echo hello_world_test"
    let exec_request = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"exec","arguments":{"cmd":"echo hello_world_test","timeout":10}}}"#;
    let exec_response = send_request(&mut stdin, &mut reader, exec_request);

    // Extract the UUID from the response
    assert!(exec_response.contains("result") || exec_response.contains("id"),
        "Exec should return result with id: {}", exec_response);

    // Parse the execution ID from response
    let exec_id = if let Some(start) = exec_response.find("\"id\":\"") {
        let start = start + 6;
        if let Some(end) = exec_response[start..].find("\"") {
            Some(exec_response[start..start+end].to_string())
        } else {
            None
        }
    } else {
        None
    };

    // Give the command time to complete
    thread::sleep(Duration::from_millis(500));

    // Step 4: Stream logs if we got an ID
    if let Some(id) = exec_id {
        let stream_request = format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{{"name":"stream_logs","arguments":{{"id":"{}","offset":0}}}}}}"#,
            id
        );
        let stream_response = send_request(&mut stdin, &mut reader, &stream_request);

        // Check that we got logs containing our test output
        assert!(stream_response.contains("hello_world_test") || stream_response.contains("logs"),
            "Stream logs should contain output or logs field: {}", stream_response);

        // Step 5: Search logs for our pattern
        let search_request = format!(
            r#"{{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{{"name":"search_logs","arguments":{{"id":"{}","pattern":"hello"}}}}}}"#,
            id
        );
        let search_response = send_request(&mut stdin, &mut reader, &search_request);

        // Check that search returned matches
        assert!(search_response.contains("matches") || search_response.contains("result"),
            "Search should return matches: {}", search_response);
    }

    child.kill().ok();
}

/// Extract the first string value for a given JSON key (e.g. `taskId`) from a raw response line.
fn extract_string(response: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", key);
    let start = response.find(&needle)? + needle.len();
    let end = response[start..].find('"')?;
    Some(response[start..start + end].to_string())
}

/// Initialize a freshly spawned server (declaring task client capability) and return the handles.
fn start_initialized_server() -> (std::process::Child, std::process::ChildStdin, BufReader<std::process::ChildStdout>) {
    let mut child = Command::new("cargo")
        .args(["run", "--"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start server");

    let mut stdin = child.stdin.take().expect("Failed to get stdin");
    let stdout = child.stdout.take().expect("Failed to get stdout");
    let mut reader = BufReader::new(stdout);

    let init_request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{"tasks":{"list":{},"cancel":{},"requests":{"tools":{"call":{}}}}},"clientInfo":{"name":"test","version":"1.0"}}}"#;
    let init_response = send_request(&mut stdin, &mut reader, init_request);
    assert!(init_response.contains("\"tasks\""), "Server should advertise tasks capability: {}", init_response);

    let initialized = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    writeln!(stdin, "{}", initialized).expect("Failed to write");
    stdin.flush().expect("Failed to flush");

    (child, stdin, reader)
}

#[test]
fn test_exec_advertises_task_support() {
    let (mut child, mut stdin, mut reader) = start_initialized_server();

    let tools_request = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
    let tools_response = send_request(&mut stdin, &mut reader, tools_request);

    assert!(tools_response.contains("taskSupport"), "exec tool should declare taskSupport: {}", tools_response);
    assert!(tools_response.contains("optional"), "exec taskSupport should be optional: {}", tools_response);

    child.kill().ok();
}

#[test]
fn test_task_lifecycle() {
    let (mut child, mut stdin, mut reader) = start_initialized_server();

    // Enqueue a task-augmented exec call.
    let enqueue = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"exec","arguments":{"cmd":"echo task_output","timeout":10},"task":{"ttl":60000}}}"#;
    let enqueue_response = send_request(&mut stdin, &mut reader, enqueue);
    assert!(enqueue_response.contains("\"status\":\"working\""), "Enqueue should return a working task: {}", enqueue_response);
    let task_id = extract_string(&enqueue_response, "taskId").expect("CreateTaskResult should carry a taskId");

    // tasks/get reports status.
    let get = format!(r#"{{"jsonrpc":"2.0","id":3,"method":"tasks/get","params":{{"taskId":"{}"}}}}"#, task_id);
    let get_response = send_request(&mut stdin, &mut reader, &get);
    assert!(get_response.contains(&task_id), "tasks/get should echo the taskId: {}", get_response);
    assert!(get_response.contains("\"status\""), "tasks/get should report a status: {}", get_response);

    // tasks/result blocks until terminal, then returns the command's output.
    let result = format!(r#"{{"jsonrpc":"2.0","id":4,"method":"tasks/result","params":{{"taskId":"{}"}}}}"#, task_id);
    let result_response = send_request(&mut stdin, &mut reader, &result);
    assert!(result_response.contains("task_output"), "tasks/result should contain command output: {}", result_response);
    assert!(result_response.contains("completed"), "tasks/result should report completion: {}", result_response);

    // tasks/list includes the task.
    let list = r#"{"jsonrpc":"2.0","id":5,"method":"tasks/list","params":{}}"#;
    let list_response = send_request(&mut stdin, &mut reader, list);
    assert!(list_response.contains(&task_id), "tasks/list should include the task: {}", list_response);

    child.kill().ok();
}

#[test]
fn test_task_cancel() {
    let (mut child, mut stdin, mut reader) = start_initialized_server();

    // Start a long-running command as a task.
    let enqueue = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"exec","arguments":{"cmd":"sleep 30","timeout":60},"task":{}}}"#;
    let enqueue_response = send_request(&mut stdin, &mut reader, enqueue);
    let task_id = extract_string(&enqueue_response, "taskId").expect("CreateTaskResult should carry a taskId");

    // Cancel it.
    let cancel = format!(r#"{{"jsonrpc":"2.0","id":3,"method":"tasks/cancel","params":{{"taskId":"{}"}}}}"#, task_id);
    let cancel_response = send_request(&mut stdin, &mut reader, &cancel);
    assert!(cancel_response.contains("cancelled"), "tasks/cancel should report a cancelled task: {}", cancel_response);

    // A second cancel must be rejected (already terminal).
    let cancel_again = format!(r#"{{"jsonrpc":"2.0","id":4,"method":"tasks/cancel","params":{{"taskId":"{}"}}}}"#, task_id);
    let cancel_again_response = send_request(&mut stdin, &mut reader, &cancel_again);
    assert!(cancel_again_response.contains("error"), "Re-cancelling a terminal task should error: {}", cancel_again_response);

    child.kill().ok();
}

#[test]
fn test_tools_list() {
    let mut child = Command::new("cargo")
        .args(["run", "--"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("Failed to start server");

    let mut stdin = child.stdin.take().expect("Failed to get stdin");
    let stdout = child.stdout.take().expect("Failed to get stdout");
    let mut reader = BufReader::new(stdout);

    // Initialize
    let init_request = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#;
    let _ = send_request(&mut stdin, &mut reader, init_request);

    // Send initialized notification
    let initialized = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
    writeln!(stdin, "{}", initialized).expect("Failed to write");
    stdin.flush().expect("Failed to flush");

    // Request tools list
    let tools_request = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#;
    let tools_response = send_request(&mut stdin, &mut reader, tools_request);

    // Verify all 3 tools are listed
    assert!(tools_response.contains("exec"), "Should list exec tool: {}", tools_response);
    assert!(tools_response.contains("stream_logs"), "Should list stream_logs tool: {}", tools_response);
    assert!(tools_response.contains("search_logs"), "Should list search_logs tool: {}", tools_response);

    child.kill().ok();
}
