//! Production built-in factory and scope admission, without external MCP connections.
use std::collections::HashSet;
use std::sync::Arc;
use tools::scoped::{ScopedAssembly, ScopedToolRegistry};
use zeroclaw_config::{policy::SecurityPolicy, schema::Config};
use zeroclaw_memory::sqlite::SqliteMemory;
use zeroclaw_runtime::{platform, tools};

const ADDED: [&str; 14] = [
    "image_info",
    "sessions_current",
    "sessions_list",
    "sessions_history",
    "sessions_send",
    "knowledge",
    "project_intel",
    "report_template",
    "cloud_ops",
    "cloud_patterns",
    "security_ops",
    "memory_store",
    "memory_recall",
    "memory_forget",
];

#[tokio::test]
async fn heimdall_native_inventory_survives_production_scope_for_telegram_and_acp() {
    let temp = tempfile::tempdir().unwrap();
    let launcher = std::env::var("HEIMDALL_TEST_SKILLS_MCP").unwrap();
    let output = std::process::Command::new(launcher)
        .arg("--render-config")
        .env_clear()
        .env("TELEGRAM_OWNER_ID", "1001")
        .env("HEIMDALL_MODEL", "gpt-6.1-sol")
        .output()
        .unwrap();
    assert!(output.status.success());
    let rendered = String::from_utf8(output.stdout)
        .unwrap()
        .replace("/data/zeroclaw", temp.path().to_str().unwrap());
    let mut config: Config = toml::from_str(&rendered).unwrap();
    config.data_dir = temp.path().join("data");
    config.config_path = temp.path().join("config.toml");
    std::fs::create_dir_all(&config.data_dir).unwrap();
    let workspace = config.agent_workspace_dir("primary");
    std::fs::create_dir_all(&workspace).unwrap();
    let config = Arc::new(config);
    let security = Arc::new(SecurityPolicy::for_agent(&config, "primary").unwrap());
    let memory = Arc::new(SqliteMemory::new("primary", &workspace).unwrap());
    for acp in [false, true] {
        let runtime: Arc<dyn platform::RuntimeAdapter> =
            Arc::from(platform::create_runtime(&config.runtime).unwrap());
        let built = tools::all_tools_with_runtime(
            config.clone(),
            &security,
            config.risk_profile_for_agent("primary").unwrap(),
            "primary",
            runtime.clone(),
            memory.clone(),
            None,
            None,
            &config.browser,
            &config.http_request,
            &config.web_fetch,
            &workspace,
            &config.agents,
            None,
            &config,
            None,
            false,
            None,
            None,
            None,
            None,
        );
        let assembled = ScopedToolRegistry::assemble(ScopedAssembly {
            config: &config,
            agent_alias: "primary",
            security: &security,
            built,
            skills: &[],
            runtime,
            caller_allowed: None,
            // These contracts cover built-ins; MCP transports have separate native tests.
            connect_mcp: false,
            connect_peripherals: false,
            exclude_memory: acp,
            acp_delivery: acp,
            list_deferred_mcp_specs: false,
            emit_assembly_logs: false,
            mcp_registry: None,
        })
        .await;
        let names: HashSet<_> = assembled.registry.iter().map(|tool| tool.name()).collect();
        for name in ADDED {
            let expected =
                !(acp && ["memory_store", "memory_recall", "memory_forget"].contains(&name));
            assert_eq!(names.contains(name), expected, "{name}, ACP={acp}");
            if expected {
                let tool = assembled
                    .registry
                    .iter()
                    .find(|tool| tool.name() == name)
                    .unwrap();
                let spec = tool.spec();
                assert_eq!(spec.name, name);
                assert!(!spec.description.trim().is_empty(), "{name}");
                assert_eq!(spec.parameters["type"], "object", "{name}");
                assert!(spec.parameters["properties"].is_object(), "{name}");
                if let Some(required) = spec.parameters.get("required") {
                    for field in required.as_array().unwrap() {
                        assert!(
                            spec.parameters["properties"]
                                .get(field.as_str().unwrap())
                                .is_some(),
                            "{name}"
                        );
                    }
                }
            }
        }
        for name in [
            "shell",
            "file_read",
            "file_write",
            "git_operations",
            "screenshot",
            "delegate",
            "schedule",
        ] {
            assert!(!names.contains(name), "unexpected {name}, ACP={acp}");
        }
        assert_eq!(names.contains("deliver_file"), acp);
        for name in ["file_upload", "file_upload_bundle", "file_download"] {
            assert!(!names.contains(name), "unconfigured endpoint: {name}");
        }
    }
}
