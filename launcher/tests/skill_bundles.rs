use heimdall_launcher::{
    config::render_config,
    skill_bundles::{attach, read_config},
};
use std::fs;

fn base() -> String {
    render_config(&["1001".into()], "gpt-6.1-sol").unwrap()
}

#[test]
fn native_bundle_definitions_survive_refresh_and_fresh_startup_rendering() {
    let original = base();
    let mut installed: toml::Value = toml::from_str(&original).unwrap();
    installed["skill_bundles"].as_table_mut().unwrap().insert(
        "custom-workflow".into(),
        toml::toml! {
            directory = "shared/custom-custom-workflow"
            include = ["brainstorming"]
            exclude = ["dispatching-parallel-agents"]
        }
        .into(),
    );
    installed["skill_bundles"]
        .as_table_mut()
        .unwrap()
        .insert("ponytail".into(), toml::Value::Table(Default::default()));
    installed["skill_bundles"]
        .as_table_mut()
        .unwrap()
        .insert("ecc".into(), toml::Value::Table(Default::default()));
    installed["gateway"]["require_pairing"] = false.into();
    installed["web_search"]
        .as_table_mut()
        .unwrap()
        .insert("tavily_api_key".into(), "fixture_private_key".into());
    let installed = toml::to_string(&installed).unwrap();
    let refreshed = attach(&installed, Some(&installed)).unwrap();
    let restarted = attach(&original, Some(&refreshed)).unwrap();
    let cfg: toml::Value = toml::from_str(&restarted).unwrap();
    assert_eq!(
        cfg["agents"]["primary"]["skill_bundles"]
            .as_array()
            .unwrap(),
        &[
            toml::Value::from("managed"),
            toml::Value::from("custom-workflow"),
            toml::Value::from("ecc"),
            toml::Value::from("ponytail"),
            toml::Value::from("superpowers")
        ]
    );
    assert_eq!(
        cfg["skill_bundles"]["custom-workflow"]["directory"].as_str(),
        Some("shared/custom-custom-workflow")
    );
    assert_eq!(
        cfg["skill_bundles"]["custom-workflow"]["include"][0].as_str(),
        Some("brainstorming")
    );
    assert_eq!(
        cfg["skill_bundles"]["custom-workflow"]["exclude"][0].as_str(),
        Some("dispatching-parallel-agents")
    );
    assert_eq!(cfg["gateway"]["require_pairing"].as_bool(), Some(true));
    assert!(!restarted.contains("fixture_private_key"));
    assert_eq!(attach(&original, Some(&restarted)).unwrap(), restarted);

    // Native bundle removal deletes its definition; startup must not resurrect it.
    let mut removed = cfg;
    removed["skill_bundles"]
        .as_table_mut()
        .unwrap()
        .remove("custom-workflow");
    let restarted = attach(&original, Some(&toml::to_string(&removed).unwrap())).unwrap();
    assert!(!restarted.contains("custom-workflow"));
}

#[test]
fn first_boot_and_managed_policy_stay_template_owned() {
    let original = base();
    let attached = attach(&original, None).unwrap();
    let cfg: toml::Value = toml::from_str(&attached).unwrap();
    assert_eq!(
        cfg["agents"]["primary"]["skill_bundles"]
            .as_array()
            .unwrap(),
        &[
            toml::Value::from("managed"),
            toml::Value::from("ecc"),
            toml::Value::from("ponytail"),
            toml::Value::from("superpowers")
        ]
    );
    let saved = "[skill_bundles.managed]\ndirectory = 'shared/elsewhere'\n";
    let cfg: toml::Value = toml::from_str(&attach(&original, Some(saved)).unwrap()).unwrap();
    assert_eq!(
        cfg["skill_bundles"]["managed"]["directory"].as_str(),
        Some("shared/skills/managed")
    );
    assert_eq!(
        cfg["agents"]["primary"]["skill_bundles"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn malformed_state_fails_without_echoing_contents() {
    for saved in [
        "fixture_secret = [",
        "skill_bundles = []",
        "[skill_bundles.'../fixture_secret']",
        "[skill_bundles.extra]\ninclude = false",
        "[skill_bundles.extra]\ndirectory = 123",
        "[skill_bundles.extra]\nfixture_secret = 'private'",
    ] {
        let error = attach(&base(), Some(saved)).unwrap_err();
        assert!(!error.contains("fixture_secret"));
        assert!(!error.contains("private"));
    }
}

#[test]
fn persistent_config_reads_are_bounded_regular_and_no_follow() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    assert!(read_config(&path).unwrap().is_none());
    fs::write(&path, "safe fixture").unwrap();
    assert_eq!(read_config(&path).unwrap().as_deref(), Some("safe fixture"));
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(read_config(&link).is_err());
    assert!(read_config(dir.path()).is_err());
    fs::write(&path, vec![b'x'; 1_048_577]).unwrap();
    assert!(read_config(&path).is_err());
    fs::write(&path, [0xff]).unwrap();
    assert!(read_config(&path).is_err());
}

#[test]
fn native_runtime_lists_newly_attached_bundle() {
    let Ok(binary) = std::env::var("HEIMDALL_TEST_RUNTIME") else {
        eprintln!("native skill discovery requires HEIMDALL_TEST_RUNTIME");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let skill = dir.path().join("shared/skills/superpowers/bundle-fixture");
    fs::create_dir_all(&skill).unwrap();
    fs::write(skill.join("SKILL.md"), "---\nname: bundle-fixture\ndescription: Synthetic bundle visibility check\n---\n\nSummarize the task clearly.\n").unwrap();
    let config = attach(&base(), Some("[skill_bundles.superpowers]\n")).unwrap();
    fs::write(dir.path().join("config.toml"), config).unwrap();
    let output = std::process::Command::new(binary)
        .args(["skills", "list", "--agent", "primary"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", dir.path())
        .env("ZEROCLAW_CONFIG_DIR", dir.path())
        .env("ZEROCLAW_skills__open_skills_enabled", "false")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("bundle-fixture"));
}
