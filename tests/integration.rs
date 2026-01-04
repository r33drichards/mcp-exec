use std::process::{Command, Stdio};
use std::io::{BufRead, BufReader, Write};

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
