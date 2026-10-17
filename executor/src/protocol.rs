use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const LIMIT: usize = 65536;
pub const BODY_LIMIT: usize = 131072;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reply {
    pub id: String,
    pub content: String,
    pub is_error: bool,
    pub truncated: bool,
    /// Present only for a supervised bash command, never for file tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandOutcome>,
}

/// The supervisor event that ended collection, independent of shell exit status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminationReason {
    Completed,
    Timeout,
    Cancelled,
    OutputLimit,
}

/// A bounded command result. Status fields precede output in the text encoding
/// so clients that shorten error details do not hide a silent failure's status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutcome {
    /// The observed shell exit code; null when terminated by a signal.
    pub exit_code: Option<i32>,
    pub termination_reason: TerminationReason,
    /// True only when bytes were discarded at the combined output limit.
    pub truncated: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Reply {
    /// Keep the MCP 2024-11-05 text envelope. Only bash text becomes JSON;
    /// file/base64 data must remain verbatim for existing transfer clients.
    pub fn mcp_result(&self) -> Result<Value, serde_json::Error> {
        let text = match &self.command {
            Some(command) => serde_json::to_string(command)?,
            None => self.content.clone(),
        };
        Ok(serde_json::json!({
            "content": [{"type": "text", "text": text}],
            "isError": self.is_error,
        }))
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub tool: String,
    pub arguments: Value,
}
impl Job {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.id.is_empty() || self.id.len() > 128 || !self.arguments.is_object() {
            return Err("Invalid request ID or arguments");
        }
        match self.tool.as_str() {
            "bash" => match self.arguments["command"].as_str() {
                Some(s) if !s.is_empty() && s.len() <= 32768 => Ok(()),
                _ => Err("Command must contain 1–32768 bytes"),
            },
            "file_read" | "file_write" | "file_read_base64" | "file_write_base64" => {
                crate::files::validate_path(
                    self.arguments["path"].as_str().ok_or("path is required")?,
                )
                .map_err(|_| "Invalid workspace path")?;
                if self.tool == "file_write" || self.tool == "file_write_base64" {
                    let content = self.arguments["content"]
                        .as_str()
                        .ok_or("content is required")?;
                    if content.len() > LIMIT {
                        return Err("Write exceeds 64 KiB");
                    }
                }
                Ok(())
            }
            _ => Err("Unknown tool"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn command_text_contains_typed_status_even_without_output() {
        for (code, reason, truncated, error, name) in [
            (
                Some(0),
                TerminationReason::Completed,
                false,
                false,
                "completed",
            ),
            (
                Some(7),
                TerminationReason::Completed,
                false,
                true,
                "completed",
            ),
            (None, TerminationReason::Completed, false, true, "completed"),
            (None, TerminationReason::Timeout, false, true, "timeout"),
            (None, TerminationReason::Cancelled, false, true, "cancelled"),
            (
                Some(0),
                TerminationReason::OutputLimit,
                true,
                true,
                "output_limit",
            ),
        ] {
            let reply = Reply {
                id: "fixture".into(),
                content: "legacy supervisor annotation".into(),
                is_error: error,
                truncated,
                command: Some(CommandOutcome {
                    exit_code: code,
                    termination_reason: reason,
                    truncated,
                    stdout: String::new(),
                    stderr: String::new(),
                }),
            };
            let result = reply.mcp_result().unwrap();
            let text = result["content"][0]["text"].as_str().unwrap();
            let outcome: Value = serde_json::from_str(text).unwrap();
            assert_eq!(result["isError"], error);
            assert_eq!(
                outcome,
                json!({
                    "exit_code": code,
                    "termination_reason": name,
                    "truncated": truncated,
                    "stdout": "",
                    "stderr": "",
                })
            );
            assert_eq!(
                serde_json::from_str::<CommandOutcome>(text).unwrap(),
                reply.command.unwrap()
            );
        }
    }

    #[test]
    fn file_text_and_base64_results_remain_verbatim() {
        for (content, error, truncated) in [
            ("file written\n", false, false),
            ("{\"exit_code\":7}\n", false, false),
            ("AAECA/8=", false, false),
            ("Tool failed: Permission denied", true, false),
            ("prefix\n[output truncated; execution stopped]", false, true),
        ] {
            let reply = Reply {
                id: "file".into(),
                content: content.into(),
                is_error: error,
                truncated,
                command: None,
            };
            assert_eq!(
                reply.mcp_result().unwrap(),
                json!({
                    "content": [{"type":"text", "text":content}], "isError":error,
                })
            );
            let serialized = serde_json::to_value(&reply).unwrap();
            assert!(serialized.get("command").is_none());
        }
    }

    #[test]
    fn legacy_reply_deserializes_without_command_metadata() {
        let reply: Reply = serde_json::from_value(json!({
            "id":"old", "content":"ok", "is_error":false, "truncated":false,
        }))
        .unwrap();
        assert!(reply.command.is_none());
    }

    #[test]
    fn status_precedes_large_output_and_worst_case_wire_size_is_bounded() {
        let reply = Reply {
            id: "large".into(),
            content: String::new(),
            is_error: true,
            truncated: false,
            command: Some(CommandOutcome {
                exit_code: Some(7),
                termination_reason: TerminationReason::Completed,
                truncated: false,
                stdout: "\0".repeat(65536),
                stderr: String::new(),
            }),
        };
        let result = reply.mcp_result().unwrap();
        let text = result["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with(
            "{\"exit_code\":7,\"termination_reason\":\"completed\",\"truncated\":false,"
        ));
        let wire = serde_json::to_vec(&json!({"jsonrpc":"2.0", "id":3, "result":result})).unwrap();
        // The configured native MCP client response bound remains unchanged.
        assert!(wire.len() < 524288, "{} bytes", wire.len());
    }
}
