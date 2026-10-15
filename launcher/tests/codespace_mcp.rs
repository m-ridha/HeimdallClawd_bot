use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
#[test]
fn real_stdio_server_lists_tools_and_fails_closed_without_credentials() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_heimdall-launcher"))
        .arg("--codespace-mcp")
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
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"create","arguments":{"repo":"octo/sandbox","machine":"largest"}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"status","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"exec","arguments":{"name":"octo-box-1","command":"id"}}}),
    ] {
        writeln!(input, "{request}").unwrap();
        input.flush().unwrap();
        let mut line = String::new();
        output.read_line(&mut line).unwrap();
        let response: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(response["id"], request["id"]);
        match request["id"].as_u64().unwrap() {
            1 => assert_eq!(
                response["result"]["serverInfo"]["name"],
                "heimdall-codespace"
            ),
            2 => assert_eq!(response["result"]["tools"].as_array().unwrap().len(), 6),
            5 => {
                assert_eq!(response["result"]["isError"], false);
                let text = response["result"]["content"][0]["text"].as_str().unwrap();
                let value: Value = serde_json::from_str(text).unwrap();
                assert_eq!(value["token"], "missing");
                assert_eq!(value["authentication_verified"], false);
            }
            // Extra arguments are rejected before any credential or process is touched.
            3 => assert_eq!(response["error"]["code"], -32602),
            // A well-formed call without configuration reports a tool error and runs nothing.
            4 => {
                assert_eq!(response["result"]["isError"], true);
                assert!(
                    response["result"]["content"][0]["text"]
                        .as_str()
                        .unwrap()
                        .contains("GITHUB_TOKEN")
                );
            }
            _ => unreachable!(),
        }
    }
    drop(input);
    assert!(child.wait().unwrap().success());
}
