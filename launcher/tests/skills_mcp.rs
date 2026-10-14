use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
#[test]
fn real_stdio_server_initializes_lists_tools_and_rejects_arbitrary_sources() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_heimdall-launcher"))
        .arg("--skills-mcp")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    for request in [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"install","arguments":{"name":"https://example.com/skill"}}}),
    ] {
        writeln!(input, "{request}").unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], request["id"]);
        match request["id"].as_u64().unwrap() {
            1 => assert_eq!(response["result"]["serverInfo"]["name"], "heimdall-skills"),
            2 => assert_eq!(response["result"]["tools"].as_array().unwrap().len(), 2),
            3 => assert_eq!(response["error"]["code"], -32602),
            _ => unreachable!(),
        }
    }
    drop(input);
    assert!(child.wait().unwrap().success());
}
