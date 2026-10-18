use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use zeroclaw_api::attribution::{Attributable, Role, ToolKind};
use zeroclaw_api::tool::{Tool, ToolOutput, ToolResult};
use zeroclaw_config::policy::SecurityPolicy;
use zeroclaw_runtime::skills::{Skill, SkillTool, skills_to_tools_with_context};

struct Remote {
    name: &'static str,
    calls: Arc<Mutex<Vec<Value>>>,
}
impl Attributable for Remote {
    fn role(&self) -> Role {
        Role::Tool(ToolKind::Plugin)
    }
    fn alias(&self) -> &str {
        self.name
    }
}
#[async_trait]
impl Tool for Remote {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "test remote executor"
    }
    fn parameters_schema(&self) -> Value {
        json!({})
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.calls.lock().unwrap().push(args);
        Ok(ToolResult {
            success: true,
            output: ToolOutput::default(),
            error: None,
        })
    }
}

#[tokio::test]
async fn heimdall_script_files_and_commands_stay_remote() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("run.sh"), "echo ready").unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let writer: Arc<dyn Tool> = Arc::new(Remote {
        name: "workspace__file_write",
        calls: calls.clone(),
    });
    let remote: Arc<dyn Tool> = Arc::new(Remote {
        name: "workspace__bash",
        calls: calls.clone(),
    });
    let spec = SkillTool {
        name: "run".into(),
        description: "run".into(),
        kind: "script".into(),
        command: "bash run.sh".into(),
        args: HashMap::new(),
        target: None,
        locked_args: HashMap::new(),
        timeout_secs: None,
    };
    let policy = Arc::new(SecurityPolicy {
        allowed_commands: vec![],
        ..SecurityPolicy::default()
    });
    let mut skill: Skill = serde_json::from_value(json!({
        "name":"example", "description":"remote-only fixture", "version":"1.0.0", "tools":[spec]
    }))
    .unwrap();
    skill.location = Some(root.path().join("SKILL.toml"));
    let tools = skills_to_tools_with_context(&[skill], policy, &[remote, writer], &[]);
    let tool = tools
        .iter()
        .find(|tool| tool.name() == "example__run")
        .unwrap();
    assert!(tool.execute(json!({})).await.unwrap().success);
    let recorded = calls.lock().unwrap().clone();
    assert_eq!(recorded[0]["path"], ".skills/example__run/run.sh");
    assert_eq!(recorded[0]["content"], "echo ready");
    assert!(
        recorded[1]["command"]
            .as_str()
            .unwrap()
            .ends_with("&& bash run.sh")
    );
    assert!(
        !recorded[1]["command"]
            .as_str()
            .unwrap()
            .contains(root.path().to_str().unwrap())
    );
    calls.lock().unwrap().clear();
    std::os::unix::fs::symlink("/etc/passwd", root.path().join("escape")).unwrap();
    assert!(tool.execute(json!({})).await.is_err());
    assert!(calls.lock().unwrap().is_empty());
}
