//! Agent-facing workshop diagnostics against the actual pinned implementation.
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use zeroclaw_api::tool::Tool;
use zeroclaw_config::schema::SopConfig;
use zeroclaw_runtime::{sop::SopEngine, tools::SopWorkshopTool};

#[tokio::test]
async fn heimdall_sop_workshop_repairs_rejected_proposals_before_explicit_apply() {
    let root = tempfile::tempdir().unwrap();
    let engine = Arc::new(Mutex::new(SopEngine::new(SopConfig {
        sops_dir: Some("shared/sops".into()),
        ..SopConfig::default()
    })));
    engine.lock().unwrap().reload(root.path());
    let tool = SopWorkshopTool::new(engine.clone(), root.path().to_owned());
    let schema = tool.parameters_schema();
    assert_eq!(
        schema["properties"]["description"]["description"],
        "Required for action=propose, including when manifest_toml is supplied"
    );
    assert!(
        schema["properties"]["procedure_markdown"]["description"]
            .as_str()
            .unwrap()
            .contains("## Steps")
    );
    let manifest = r#"[sop]
name = "fixture-review"
description = "Fixture review"
version = "0.1.0"

[[triggers]]
type = "manual"
"#;
    let mut args = json!({"action":"propose","sop_name":"fixture-review","manifest_toml":manifest,
        "procedure_markdown":"## Steps\n\n1. **Inspect** - Read the current PR diff.\n"});
    let missing = tool.execute(args.clone()).await.unwrap();
    assert!(!missing.success);
    assert_eq!(
        missing.error.as_deref(),
        Some("SOP propose requires a string 'description', even when manifest_toml is supplied")
    );
    args["description"] = json!("Fixture review");
    let valid = args["procedure_markdown"].clone();
    args["procedure_markdown"] =
        json!("## Procedure\n\n1. **Inspect** - Fixture private text must not appear in errors.\n");
    let malformed = tool.execute(args.clone()).await.unwrap();
    assert!(!malformed.success);
    let error = malformed.error.unwrap();
    assert!(error.contains("## Steps"));
    assert!(error.contains("numbered steps"));
    assert!(!error.contains("Fixture private text"));
    assert!(
        !root
            .path()
            .join("shared/sops/fixture-review/SOP.md")
            .exists()
    );
    args["procedure_markdown"] = valid;
    let proposed = tool.execute(args).await.unwrap();
    assert!(proposed.success, "{:?}", proposed.error);
    let proposal: Value = serde_json::from_str(&proposed.output).unwrap();
    let id = proposal["id"].as_str().unwrap();
    assert!(
        !root
            .path()
            .join("shared/sops/fixture-review/SOP.md")
            .exists(),
        "propose must not install a definition"
    );
    let inspected = tool
        .execute(json!({"action":"inspect","id":id}))
        .await
        .unwrap();
    assert!(inspected.success, "{:?}", inspected.error);
    let applied = tool
        .execute(json!({"action":"apply","id":id}))
        .await
        .unwrap();
    assert!(applied.success, "{:?}", applied.error);
    let definition =
        std::fs::read_to_string(root.path().join("shared/sops/fixture-review/SOP.md")).unwrap();
    assert_eq!(
        definition,
        "## Steps\n\n1. **Inspect** - Read the current PR diff."
    );
    assert_eq!(
        engine
            .lock()
            .unwrap()
            .get_sop("fixture-review")
            .unwrap()
            .steps
            .len(),
        1
    );
}
