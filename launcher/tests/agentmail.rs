use serde_json::{Value, json};
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn real_mail_stdio_exposes_only_management_and_fails_closed_without_ingress() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_heimdall-launcher"))
        .arg("--mail-mcp")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    for request in [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        json!({"jsonrpc":"2.0","method":"tools/call","params":{"name":"set_enabled","arguments":{"name":"github-review-request","enabled":true}}}),
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"set_enabled","arguments":{"name":"github-review-request","enabled":true,"path":"/tmp/untrusted"}}}),
        json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"filters","arguments":{}}}),
        json!({"jsonrpc":"2.0","id":5,"method":"unknown"}),
    ] {
        writeln!(input, "{request}").unwrap();
    }
    drop(input);
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success());
    let responses: Vec<Value> = String::from_utf8(result.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(responses.len(), 5);
    assert_eq!(
        responses[0]["result"]["serverInfo"]["name"],
        "heimdall-mail"
    );
    let names: Vec<_> = responses[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["filters", "set_enabled"]);
    assert_eq!(responses[2]["error"]["code"], -32602);
    let status: Value = serde_json::from_str(
        responses[3]["result"]["content"][0]["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(status["configured"], false);
    assert_eq!(responses[4]["error"]["code"], -32601);
}
