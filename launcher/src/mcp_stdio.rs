//! Shared framing for the skills and Codespaces MCP servers.
use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};
const REQUEST_LIMIT: u64 = 131072;

pub(crate) fn serve(
    input: &mut impl BufRead,
    output: &mut impl Write,
    mut handle: impl FnMut(Value) -> Option<Value>,
) -> io::Result<()> {
    loop {
        let mut line = String::new();
        let size = input
            .by_ref()
            .take(REQUEST_LIMIT + 1)
            .read_line(&mut line)?;
        if size == 0 {
            return Ok(());
        }
        if size as u64 > REQUEST_LIMIT {
            return Err(io::Error::other("MCP request too large"));
        }
        let response = match serde_json::from_str(&line) {
            Ok(request) => handle(request),
            Err(_) => Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Invalid JSON"}}),
            ),
        };
        if let Some(response) = response {
            writeln!(output, "{response}")?;
            output.flush()?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framing_preserves_notifications_parse_errors_and_limits() {
        let mut input = io::Cursor::new(b"{invalid}\n{}\n{\"id\":7}\n");
        let mut output = Vec::new();
        serve(&mut input, &mut output, |req| {
            req.get("id").map(|_| req.clone())
        })
        .unwrap();
        let lines: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0]["error"]["code"], -32700);
        assert_eq!(lines[1]["id"], 7);
        let mut output = Vec::new();
        // Exactly 128 KiB including the newline remains valid framing.
        let mut input = io::Cursor::new(format!("{}\n", " ".repeat(131071)));
        serve(&mut input, &mut output, |_| {
            panic!("invalid JSON must not dispatch")
        })
        .unwrap();
        let mut input = io::Cursor::new(vec![b' '; 131073]);
        assert_eq!(
            serve(&mut input, &mut output, |_| panic!(
                "oversized input must not dispatch"
            ))
            .unwrap_err()
            .to_string(),
            "MCP request too large"
        );
        let mut input = io::Cursor::new(vec![0xff, b'\n']);
        assert!(serve(&mut input, &mut output, |_| None).is_err());
    }
}
