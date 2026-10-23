//! Shared bounded newline-delimited JSON framing for stdio MCP servers.
use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};
const REQUEST_LIMIT: u64 = 131072;

pub(crate) fn serve(
    input: &mut impl BufRead,
    output: &mut impl Write,
    handle: impl FnMut(Value) -> Option<Value>,
) -> io::Result<()> {
    serve_bounded(input, output, REQUEST_LIMIT, None, handle)
}

/// A recoverable oversize response is optional; otherwise oversize input closes the server.
pub(crate) fn serve_bounded(
    input: &mut impl BufRead,
    output: &mut impl Write,
    limit: u64,
    oversized_message: Option<&str>,
    mut handle: impl FnMut(Value) -> Option<Value>,
) -> io::Result<()> {
    loop {
        let mut line = Vec::new();
        let size = input
            .by_ref()
            .take(limit + 1)
            .read_until(b'\n', &mut line)?;
        if size == 0 {
            return Ok(());
        }
        let response = if size as u64 > limit {
            let Some(message) = oversized_message else {
                return Err(io::Error::other("MCP request too large"));
            };
            if !line.ends_with(b"\n") {
                input.skip_until(b'\n')?;
            }
            Some(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":message}}))
        } else {
            match serde_json::from_slice(&line) {
                Ok(request) => handle(request),
                Err(_) => Some(
                    json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Invalid JSON"}}),
                ),
            }
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
    fn recoverable_oversize_and_invalid_bytes_allow_next_request() {
        let mut input = io::Cursor::new(b"xxxxxxxxxxxxxxxxx\n\xff\n{\"id\":7}\n");
        let mut output = Vec::new();
        serve_bounded(&mut input, &mut output, 16, Some("Too large"), Some).unwrap();
        let replies: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[0]["error"]["message"], "Too large");
        assert_eq!(replies[1]["error"]["code"], -32700);
        assert_eq!(replies[2]["id"], 7);
    }
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
        serve(&mut input, &mut output, |_| None).unwrap();
    }
}
