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
            "file_read" | "file_write" => {
                crate::files::validate_path(
                    self.arguments["path"].as_str().ok_or("path is required")?,
                )
                .map_err(|_| "Invalid workspace path")?;
                if self.tool == "file_write" {
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
