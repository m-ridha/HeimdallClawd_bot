//! Exercise the actual logging sinks with synthetic tool input and output.
use serde_json::json;
use zeroclaw_log::{Action, Event, EventCategory, LogConfig, LogEvent, Severity};

#[test]
fn heimdall_tool_payloads_stay_out_of_active_and_rotated_logs() {
    let _guard = zeroclaw_log::__private_test_writer_lock();
    let directory = tempfile::tempdir().unwrap();
    let config = LogConfig {
        log_persistence: "rotating".into(),
        log_persistence_path: "diagnostics.jsonl".into(),
        log_persistence_max_entries_per_segment: 1,
        log_persistence_rotate_daily: false,
        log_tool_io: "off".into(),
        ..LogConfig::default()
    };
    zeroclaw_log::init_from_config(&config, directory.path());
    zeroclaw_log::try_install_capture_subscriber();
    let marker = "synthetic-tool-payload-do-not-retain";
    let output = std::process::Command::new("sh")
        .args(["-c", "printf '%s' \"$1\"", "privacy-test", marker])
        .output()
        .unwrap();
    assert!(output.status.success());
    let result = String::from_utf8(output.stdout).unwrap();
    assert_eq!(result, marker);
    let attributes = json!({
        "tool": "shell", "tool_call_id": "fixture-call", "duration_ms": 3,
        "success": true, "arguments": marker, "result": result, "output": marker,
        "input": marker, "error": marker, "tool_input": marker, "tool_output": marker,
        "error_reason": marker
    });
    zeroclaw_log::record!(
        INFO,
        Event::new(module_path!(), Action::Start)
            .with_category(EventCategory::Tool)
            .with_attrs(attributes.clone()),
        "tool_call_start"
    );
    // Also cover callers that bypass the tracing macro.
    let mut event = LogEvent::new(Severity::Info, "tool_call", EventCategory::Tool);
    event.attributes = attributes;
    zeroclaw_log::record_event(event);
    zeroclaw_log::flush_for_test().unwrap();
    let files: Vec<_> = std::fs::read_dir(directory.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    assert!(
        files.len() >= 2,
        "must inspect active and rotated diagnostics"
    );
    let mut records = 0;
    for path in files {
        let contents = std::fs::read_to_string(path).unwrap();
        assert!(!contents.contains(marker));
        for line in contents.lines() {
            let event: serde_json::Value = serde_json::from_str(line).unwrap();
            if event["attributes"]["tool"] != "shell" {
                continue;
            }
            assert_eq!(event["attributes"]["tool"], "shell");
            assert_eq!(event["attributes"]["success"], true);
            assert_eq!(event["attributes"]["tool_call_id"], "fixture-call");
            for field in [
                "input",
                "arguments",
                "result",
                "output",
                "error",
                "error_reason",
                "tool_input",
                "tool_output",
            ] {
                assert!(event["attributes"].get(field).is_none());
            }
            records += 1;
        }
    }
    assert_eq!(records, 2);
}

#[test]
fn heimdall_capture_policy_preserves_only_explicit_captures() {
    let _guard = zeroclaw_log::__private_test_writer_lock();
    let directory = tempfile::tempdir().unwrap();
    for (mode, denylisted, expected) in [
        ("off", false, false),
        ("redacted", false, true),
        ("full", false, true),
        ("full", true, false),
    ] {
        let config = LogConfig {
            log_persistence: "none".into(),
            log_tool_io: mode.into(),
            log_tool_io_denylist: if denylisted {
                vec!["shell".into()]
            } else {
                vec![]
            },
            ..LogConfig::default()
        };
        zeroclaw_log::init_from_config(&config, directory.path());
        let event = Event::new(module_path!(), Action::Start)
            .with_category(EventCategory::Tool)
            .with_attrs(json!({
                "tool": "shell", "input": "raw-input", "arguments": "raw-input",
                "result": "raw-output", "output": "raw-output", "error": "raw-error",
                "error_reason": "raw-error", "tool_input": "already-redacted",
                "tool_output": "already-redacted", "success": true
            }));
        let attrs: serde_json::Value = serde_json::from_str(&event.attrs_str()).unwrap();
        assert!(attrs.get("arguments").is_none());
        assert!(attrs.get("result").is_none());
        assert!(attrs.get("input").is_none());
        assert!(attrs.get("output").is_none());
        assert!(attrs.get("error").is_none());
        assert!(attrs.get("error_reason").is_none());
        assert_eq!(attrs.get("tool_input").is_some(), expected);
        assert_eq!(attrs.get("tool_output").is_some(), expected);
        assert_eq!(attrs["success"], true);
        let unrelated = Event::new(module_path!(), Action::Note)
            .with_attrs(json!({"result": "non-tool-metadata"}));
        assert!(unrelated.attrs_str().contains("non-tool-metadata"));
    }
}

#[test]
fn heimdall_failed_tools_do_not_persist_input_or_errors() {
    let _guard = zeroclaw_log::__private_test_writer_lock();
    zeroclaw_log::try_install_capture_subscriber();
    for (mode, denylisted) in [
        ("off", false),
        ("redacted", false),
        ("full", false),
        ("full", true),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let config = LogConfig {
            log_persistence: "rotating".into(),
            log_persistence_path: "diagnostics.jsonl".into(),
            log_persistence_max_entries_per_segment: 1,
            log_persistence_rotate_daily: false,
            log_tool_io: mode.into(),
            log_tool_io_denylist: if denylisted {
                vec!["shell".into()]
            } else {
                vec![]
            },
            ..LogConfig::default()
        };
        zeroclaw_log::init_from_config(&config, directory.path());
        let failure = || {
            Event::new(module_path!(), Action::Fail)
                .with_category(EventCategory::Tool)
                .with_outcome(zeroclaw_log::EventOutcome::Failure)
                .with_duration(3)
                .with_attrs(json!({
                    "tool": "shell", "tool_call_id": "failed-call",
                    "input": {"command": "synthetic-private-command"},
                    "error": "synthetic-private-error", "output": "synthetic-private-output"
                }))
        };
        // Match execute_one_tool's failed result and exception producers.
        zeroclaw_log::record!(WARN, failure(), "tool failed: shell");
        zeroclaw_log::record!(ERROR, failure(), "tool error: shell");
        zeroclaw_log::flush_for_test().unwrap();
        let files: Vec<_> = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
            .collect();
        assert!(files.len() >= 2, "must check active and rotated files");
        let mut severities = Vec::new();
        for path in files {
            let contents = std::fs::read_to_string(path).unwrap();
            assert!(!contents.contains("synthetic-private-command"));
            assert!(!contents.contains("synthetic-private-error"));
            assert!(!contents.contains("synthetic-private-output"));
            for line in contents.lines() {
                let event: serde_json::Value = serde_json::from_str(line).unwrap();
                if event["attributes"]["tool_call_id"] != "failed-call" {
                    continue;
                }
                assert_eq!(event["attributes"]["tool"], "shell");
                assert_eq!(event["event"]["outcome"], "failure");
                for field in ["input", "error", "output"] {
                    assert!(event["attributes"].get(field).is_none());
                }
                severities.push(event["severity_text"].as_str().unwrap().to_owned());
            }
        }
        severities.sort();
        assert_eq!(severities, ["ERROR", "WARN"]);
    }
}
