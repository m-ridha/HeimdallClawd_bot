//! Production built-in factory and scope admission, without external MCP connections.
use std::collections::HashSet;
use std::sync::Arc;
use tools::scoped::{ScopedAssembly, ScopedToolRegistry};
use zeroclaw_config::{policy::SecurityPolicy, schema::Config};
use zeroclaw_memory::sqlite::SqliteMemory;
use zeroclaw_runtime::{platform, tools};

const ADDED: [&str; 15] = [
    "tool_search",
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
    let inject = zeroclaw_runtime::agent::memory_inject::MemoryInjectConfig::from_memory_config(
        &config.memory,
        5,
    );
    assert!(inject.rerank_enabled);
    assert_eq!(inject.rerank.candidate_pool_cap, 10);
    assert_eq!(
        inject.rerank.strategy,
        zeroclaw_memory::rerank::RerankStrategy::Mmr { lambda: 0.7 }
    );
    let entry = |id: &str| {
        serde_json::from_value(serde_json::json!({
            "id": id, "key": id, "content":"Synthetic memory fact for duplicate collapse",
            "category":"core", "timestamp":"2026-01-01T00:00:00Z", "score":0.95
        }))
        .unwrap()
    };
    let ranked = zeroclaw_memory::rerank::run(
        vec![entry("first"), entry("duplicate")],
        &inject.rerank,
        |_| true,
    );
    assert_eq!(
        ranked.len(),
        1,
        "enabled native stage must collapse duplicate facts"
    );
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
        )
        .unwrap();
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
        if !acp {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = [0; 2048];
                socket.read(&mut request).await.unwrap();
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 17\r\nConnection: close\r\n\r\nnative-network-ok").await.unwrap();
            });
            let shell = assembled
                .registry
                .iter()
                .find(|tool| tool.name() == "shell")
                .unwrap();
            let result = shell.execute(serde_json::json!({"command":format!("curl --noproxy '*' --fail --max-time 10 http://{address}/")})).await.unwrap();
            if !result.success {
                server.abort();
                let _ = server.await;
                panic!("native network command failed: {:?}", result.error);
            }
            tokio::time::timeout(std::time::Duration::from_secs(15), server)
                .await
                .unwrap()
                .unwrap();
            assert!(result.output.contains("native-network-ok"));
        }

        for name in ADDED.into_iter().chain([
            "browser",
            "file_edit",
            "content_search",
            "glob_search",
            "spawn_subagent",
            "calculator",
            "backup",
            "data_management",
            "TodoWrite",
            "web_search_tool",
            "text_browser",
            "web_fetch",
            "http_request",
        ]) {
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
            "delegate",
            "schedule",
        ] {
            assert!(names.contains(name), "missing native {name}, ACP={acp}");
        }
        let search = assembled
            .registry
            .iter()
            .find(|tool| tool.name() == "tool_search")
            .unwrap();
        let found = search
            .execute(serde_json::json!({"query":"select:memory_forget"}))
            .await
            .unwrap();
        assert!(found.success);
        assert_eq!(
            found.output.contains("<function>"),
            !acp,
            "discovery must honor ACP memory isolation"
        );
        // Core tools stay eager; tool_search exposes schemas for deferred built-ins.
        let hidden = assembled.registry.hidden_builtin_names();
        assert!(
            !hidden.contains("shell"),
            "shell must stay eager, ACP={acp}"
        );
        assert!(
            hidden.contains("git_operations"),
            "Git schema must start deferred, ACP={acp}"
        );
        let selected = search
            .execute(serde_json::json!({"query":"select:git_operations"}))
            .await
            .unwrap();
        assert!(selected.success);
        let schemas: Vec<serde_json::Value> = selected
            .output
            .lines()
            .filter_map(|line| line.strip_prefix("<function>")?.strip_suffix("</function>"))
            .map(|schema| serde_json::from_str(schema).unwrap())
            .collect();
        let [schema] = schemas.as_slice() else {
            panic!("discovery must return exactly one Git schema, ACP={acp}");
        };
        assert_eq!(schema["name"], "git_operations");
        assert_eq!(schema["parameters"]["type"], "object");
        assert_eq!(
            schema["parameters"]["properties"]["operation"]["type"],
            "string"
        );
        assert_eq!(
            schema["parameters"]["required"],
            serde_json::json!(["operation"])
        );
        assert!(
            !assembled
                .registry
                .hidden_builtin_names()
                .contains("git_operations"),
            "discovery must make the Git schema visible, ACP={acp}"
        );
        assert_eq!(names.contains("deliver_file"), acp);
        for name in ["file_upload", "file_upload_bundle", "file_download"] {
            assert!(!names.contains(name), "unconfigured endpoint: {name}");
        }
    }
}

#[test]
fn managed_github_guide_is_visible_with_production_prompt_budget() {
    let temp = tempfile::tempdir().unwrap();
    let launcher = std::env::var("HEIMDALL_TEST_SKILLS_MCP").unwrap();
    let output = std::process::Command::new(launcher)
        .arg("--render-config")
        .env_clear()
        .env("TELEGRAM_OWNER_ID", "1001")
        .output()
        .unwrap();
    assert!(output.status.success());
    let config: Config = toml::from_str(std::str::from_utf8(&output.stdout).unwrap()).unwrap();
    let guide = std::fs::read_to_string(std::env::var("HEIMDALL_TEST_TOOLS").unwrap()).unwrap();
    std::fs::write(temp.path().join("TOOLS.md"), &guide).unwrap();
    let compact = config.effective_compact_context("primary");
    assert!(!compact, "managed guide requires the full bootstrap budget");
    let prompt =
        zeroclaw_runtime::agent::system_prompt::build_system_prompt_with_mode_and_effective_tools(
            temp.path(),
            "fixture-model",
            &[
                ("http_request", "Public HTTP"),
                ("read_skill", "Load skills"),
            ],
            |name| ["http_request", "read_skill"].contains(&name),
            &[],
            None,
            if compact { Some(6000) } else { None },
            Some(config.risk_profile_for_agent("primary").unwrap()),
            true,
            config.effective_skills_prompt_mode("primary"),
            compact,
            config.effective_max_system_prompt_chars("primary"),
            true,
            false,
            None,
        );
    assert!(guide.contains("## Working on GitHub repositories"));
    assert!(
        prompt.contains(guide.trim()),
        "complete managed guide must survive prompt assembly"
    );
    assert!(prompt.contains("force=false"));
    assert!(prompt.contains("A GitHub skill is optional guidance"));
    assert!(!prompt.contains("[System prompt truncated"));
}
