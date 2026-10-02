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
    let exec_request = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"exec","arguments":{"bin":"echo","args":["hello_world_test"],"timeout":10}}}"#;
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
    let enqueue = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"exec","arguments":{"bin":"echo","args":["task_output"],"timeout":10},"task":{"ttl":60000}}}"#;
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
    let enqueue = r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"exec","arguments":{"bin":"sleep","args":["30"],"timeout":60},"task":{}}}"#;
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

    // Verify all 4 tools are listed
    assert!(tools_response.contains("\"name\":\"kill\""), "Should list kill tool: {}", tools_response);
    assert!(tools_response.contains("exec"), "Should list exec tool: {}", tools_response);
    assert!(tools_response.contains("stream_logs"), "Should list stream_logs tool: {}", tools_response);
    assert!(tools_response.contains("search_logs"), "Should list search_logs tool: {}", tools_response);

    child.kill().ok();
}

/// Call a tool and return the parsed JSON-RPC response.
fn call_tool(
    stdin: &mut impl Write,
    reader: &mut impl BufRead,
    id: u64,
    name: &str,
    arguments: serde_json::Value,
) -> serde_json::Value {
    let request = serde_json::json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
    });
    let response = send_request(stdin, reader, &request.to_string());
    serde_json::from_str(&response).unwrap_or_else(|e| panic!("not JSON ({}): {}", e, response))
}

/// The JSON a tool returned in its text content.
fn tool_json(response: &serde_json::Value) -> serde_json::Value {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("no text content: {}", response));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("tool text is not JSON ({}): {}", e, text))
}

/// Poll stream_logs until the execution is no longer running (or `seconds` have passed);
/// returns its logs and final status.
fn wait_for_end(
    stdin: &mut impl Write,
    reader: &mut impl BufRead,
    exec_id: &str,
    seconds: u64,
) -> (String, String) {
    let deadline = std::time::Instant::now() + Duration::from_secs(seconds);
    loop {
        let r = tool_json(&call_tool(stdin, reader, 99, "stream_logs", serde_json::json!({ "id": exec_id, "offset": 0 })));
        let status = r["status"].as_str().unwrap_or_default().to_string();
        if status != "running" || std::time::Instant::now() > deadline {
            return (r["logs"].as_str().unwrap_or_default().to_string(), status);
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn start(stdin: &mut impl Write, reader: &mut impl BufRead, arguments: serde_json::Value) -> String {
    let started = tool_json(&call_tool(stdin, reader, 2, "exec", arguments));
    assert_eq!(started["status"], "started", "{}", started);
    started["id"].as_str().expect("an id").to_string()
}

fn process_alive(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn gone_soon(pid: &str) -> bool {
    for _ in 0..50 {
        if !process_alive(pid) {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

#[test]
fn test_exec_runs_the_program_without_a_shell() {
    let (mut child, mut stdin, mut reader) = start_initialized_server();

    // Arguments reach the program as they are: nothing is expanded, split or run.
    let args = serde_json::json!(["$HOME", "*", "a b", "; echo injected", "`id`", "$(id)"]);
    let id = start(&mut stdin, &mut reader, serde_json::json!({ "bin": "printf", "args": ["[%s]\\n", "$HOME", "*", "a b", "; echo injected", "`id`", "$(id)"], "timeout": 10 }));
    let (logs, status) = wait_for_end(&mut stdin, &mut reader, &id, 10);
    assert_eq!(status, "completed:0");
    let expected: String = args.as_array().unwrap().iter().map(|a| format!("[{}]\n", a.as_str().unwrap())).collect();
    assert_eq!(logs, expected);

    // `args` may be left out.
    let id = start(&mut stdin, &mut reader, serde_json::json!({ "bin": "true", "timeout": 10 }));
    assert_eq!(wait_for_end(&mut stdin, &mut reader, &id, 10).1, "completed:0");

    // A shell is a program like any other, and visible as one.
    let id = start(&mut stdin, &mut reader, serde_json::json!({ "bin": "sh", "args": ["-c", "echo one | tr a-z A-Z; exit 3"], "timeout": 10 }));
    assert_eq!(wait_for_end(&mut stdin, &mut reader, &id, 10), ("ONE\n".to_string(), "completed:3".to_string()));

    // A program that does not exist: the execution fails, naming it.
    let id = start(&mut stdin, &mut reader, serde_json::json!({ "bin": "no-such-program-xyz", "timeout": 10 }));
    let (_, status) = wait_for_end(&mut stdin, &mut reader, &id, 10);
    assert!(status.starts_with("failed:") && status.contains("no-such-program-xyz"), "{}", status);

    child.kill().ok();
}

#[test]
fn test_exec_refuses_the_old_form_and_bad_arguments() {
    let (mut child, mut stdin, mut reader) = start_initialized_server();

    let error = |response: serde_json::Value| -> String {
        assert!(response.get("error").is_some(), "should be an error: {}", response);
        response["error"]["message"].as_str().unwrap_or_default().to_string()
    };

    // The old form is an error that names the fields that exist.
    let message = error(call_tool(&mut stdin, &mut reader, 2, "exec", serde_json::json!({ "cmd": "echo hi", "timeout": 10 })));
    assert!(message.contains("cmd") && message.contains("bin") && message.contains("args"), "{}", message);
    // Also next to the new form: an unknown field is never ignored.
    let message = error(call_tool(&mut stdin, &mut reader, 3, "exec", serde_json::json!({ "bin": "echo", "cmd": "id", "timeout": 10 })));
    assert!(message.contains("unknown field `cmd`"), "{}", message);

    let message = error(call_tool(&mut stdin, &mut reader, 4, "exec", serde_json::json!({ "bin": "echo", "args": "hi", "timeout": 10 })));
    assert!(message.contains("sequence") || message.contains("args"), "{}", message);
    let message = error(call_tool(&mut stdin, &mut reader, 5, "exec", serde_json::json!({ "bin": "", "timeout": 10 })));
    assert!(message.contains("bin must not be empty"), "{}", message);
    let message = error(call_tool(&mut stdin, &mut reader, 6, "exec", serde_json::json!({ "bin": "pwd", "timeout": 10, "cwd": "relative/dir" })));
    assert!(message.contains("absolute"), "{}", message);
    let message = error(call_tool(&mut stdin, &mut reader, 7, "exec", serde_json::json!({ "bin": "pwd", "timeout": 10, "cwd": "/no/such/directory" })));
    assert!(message.contains("not a directory"), "{}", message);
    let message = error(call_tool(&mut stdin, &mut reader, 8, "exec", serde_json::json!({ "bin": "env", "timeout": 10, "env": { "A=B": "c" } })));
    assert!(message.contains("not a variable name"), "{}", message);
    let message = error(call_tool(&mut stdin, &mut reader, 9, "exec", serde_json::json!({ "bin": "echo" })));
    assert!(message.contains("timeout"), "{}", message);

    child.kill().ok();
}

#[test]
fn test_exec_cwd_and_env() {
    let (mut child, mut stdin, mut reader) = start_initialized_server();

    let dir = std::fs::canonicalize(std::env::temp_dir()).unwrap();
    let id = start(
        &mut stdin,
        &mut reader,
        serde_json::json!({
            "bin": "sh", "args": ["-c", "pwd; echo \"$MCP_EXEC_TEST_A|$MCP_EXEC_TEST_B\"; test -n \"$PATH\" && echo inherited"],
            "timeout": 10, "cwd": dir.to_str().unwrap(), "env": { "MCP_EXEC_TEST_A": "one two", "MCP_EXEC_TEST_B": "" }
        }),
    );
    let (logs, status) = wait_for_end(&mut stdin, &mut reader, &id, 10);
    assert_eq!(status, "completed:0");
    assert_eq!(logs, format!("{}\none two|\ninherited\n", dir.display()));

    child.kill().ok();
}

#[test]
fn test_timeout_kills_the_process_group() {
    let (mut child, mut stdin, mut reader) = start_initialized_server();

    // The shell starts a child of its own, prints its pid, and waits for it.
    let id = start(&mut stdin, &mut reader, serde_json::json!({ "bin": "sh", "args": ["-c", "sleep 60 & echo $!; wait"], "timeout": 1 }));
    let started = std::time::Instant::now();
    let (logs, status) = wait_for_end(&mut stdin, &mut reader, &id, 20);
    // Promptly: not when the grandchild would have ended by itself.
    assert_eq!(status, "timeout");
    assert!(started.elapsed() < Duration::from_secs(10), "took {:?}", started.elapsed());
    let pid = logs.trim();
    assert!(!pid.is_empty() && pid.chars().all(|c| c.is_ascii_digit()), "no pid in logs: {:?}", logs);
    assert!(gone_soon(pid), "the command's child {} is still running after the timeout", pid);

    child.kill().ok();
}

#[test]
fn test_kill_tool() {
    let (mut child, mut stdin, mut reader) = start_initialized_server();

    let id = start(&mut stdin, &mut reader, serde_json::json!({ "bin": "sh", "args": ["-c", "sleep 60 & echo $!; wait"], "timeout": 120 }));
    // Wait for the pid of the shell's child to be printed.
    let mut pid = String::new();
    for _ in 0..100 {
        let r = tool_json(&call_tool(&mut stdin, &mut reader, 3, "stream_logs", serde_json::json!({ "id": id, "offset": 0 })));
        pid = r["logs"].as_str().unwrap_or_default().trim().to_string();
        if !pid.is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }
    assert!(process_alive(&pid), "the command's child {:?} should be running", pid);

    let killed = tool_json(&call_tool(&mut stdin, &mut reader, 4, "kill", serde_json::json!({ "id": id })));
    assert_eq!(killed, serde_json::json!({ "id": id, "status": "cancelled" }));
    assert_eq!(wait_for_end(&mut stdin, &mut reader, &id, 10).1, "cancelled");
    assert!(gone_soon(&pid), "the command's child {} is still running after kill", pid);

    // Killing it again, or a finished execution, reports the status it has.
    let again = tool_json(&call_tool(&mut stdin, &mut reader, 5, "kill", serde_json::json!({ "id": id })));
    assert_eq!(again["status"], "cancelled");
    let done = start(&mut stdin, &mut reader, serde_json::json!({ "bin": "true", "timeout": 10 }));
    assert_eq!(wait_for_end(&mut stdin, &mut reader, &done, 10).1, "completed:0");
    let finished = tool_json(&call_tool(&mut stdin, &mut reader, 6, "kill", serde_json::json!({ "id": done })));
    assert_eq!(finished["status"], "completed:0");

    // An id that is not a UUID, and one nobody has.
    let bad = tool_json(&call_tool(&mut stdin, &mut reader, 7, "kill", serde_json::json!({ "id": "nope" })));
    assert!(bad["status"].as_str().unwrap().starts_with("error:"), "{}", bad);
    let unknown = tool_json(&call_tool(&mut stdin, &mut reader, 8, "kill", serde_json::json!({ "id": "33333333-3333-4333-8333-333333333333" })));
    assert!(unknown["status"].as_str().unwrap().starts_with("error:"), "{}", unknown);

    child.kill().ok();
}

#[test]
fn test_output_that_is_not_utf8_is_kept() {
    let (mut child, mut stdin, mut reader) = start_initialized_server();

    // A line with an invalid byte, on each stream, followed by more output.
    let id = start(
        &mut stdin,
        &mut reader,
        serde_json::json!({ "bin": "sh", "args": ["-c", "printf 'a\\377b\\nafter_out\\n'; printf 'x\\376y\\nafter_err\\n' >&2; printf 'no newline'"], "timeout": 10 }),
    );
    let (logs, status) = wait_for_end(&mut stdin, &mut reader, &id, 10);
    assert_eq!(status, "completed:0");
    for expected in ["a\u{fffd}b\n", "after_out\n", "x\u{fffd}y\n", "after_err\n", "no newline\n"] {
        assert!(logs.contains(expected), "{:?} is missing from {:?}", expected, logs);
    }
    let found = tool_json(&call_tool(&mut stdin, &mut reader, 3, "search_logs", serde_json::json!({ "id": id, "pattern": "^after_" })));
    assert_eq!(found["matches"].as_array().map(|m| m.len()), Some(2), "{}", found);

    child.kill().ok();
}

#[test]
fn test_a_command_that_leaves_a_process_behind_still_completes() {
    let (mut child, mut stdin, mut reader) = start_initialized_server();

    // The shell exits at once; what it left running holds the output pipe open.
    let id = start(&mut stdin, &mut reader, serde_json::json!({ "bin": "sh", "args": ["-c", "sleep 8 & echo started"], "timeout": 30 }));
    let started = std::time::Instant::now();
    let (logs, status) = wait_for_end(&mut stdin, &mut reader, &id, 20);
    assert_eq!((logs.as_str(), status.as_str()), ("started\n", "completed:0"));
    assert!(started.elapsed() < Duration::from_secs(5), "took {:?}", started.elapsed());

    child.kill().ok();
}
