#![cfg(target_os = "linux")]
use heimdall_launcher::child_policy::{Scope, Session};
use std::{fs, process::Command};
#[test]
fn filesystem_and_process_memory_boundaries_are_inherited() {
    // This is a native Linux security test, never a portable substitute.
    assert_eq!(
        unsafe { libc::geteuid() },
        0,
        "Native isolation tests require root"
    );
    let fixture = tempfile::tempdir().unwrap();
    let secret = fixture.path().join("private-auth");
    fs::write(&secret, "TEST_private_credential").unwrap();
    let session = Session::new().unwrap();
    let mut command = Command::new("/bin/sh");
    command.env_clear().env("PATH", "/usr/bin:/bin").args([
        "-c",
        "printf public-output; cat \"$1\"; cat /proc/1/environ",
        "probe",
        secret.to_str().unwrap(),
    ]);
    session.configure(&mut command, Scope::Scratch).unwrap();
    let output = command.output().unwrap();
    assert!(!output.status.success());
    assert_eq!(output.stdout, b"public-output");
    assert!(!String::from_utf8_lossy(&output.stderr).contains("TEST_private_credential"));
    let executable = session.directory().join("probe");
    fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    let mut child = Command::new(executable);
    child
        .env_clear()
        .env("HEIMDALL_CHILD_PROBE", "1")
        .args(["--exact", "process_memory_probe"]);
    session.configure(&mut child, Scope::Scratch).unwrap();
    assert!(child.status().unwrap().success());
}
#[test]
fn process_memory_probe() {
    if std::env::var_os("HEIMDALL_CHILD_PROBE").is_none() {
        return;
    }
    // SAFETY: TRACEME and invalid process_vm arguments have no side effects when denied.
    assert_eq!(unsafe { libc::ptrace(libc::PTRACE_TRACEME, 0, 0, 0) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EPERM)
    );
    assert_eq!(
        unsafe { libc::syscall(libc::SYS_process_vm_readv, libc::getppid(), 0, 0, 0, 0, 0) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EPERM)
    );
}

#[test]
fn skills_bootstrap_accepts_existing_data_without_reading_it() {
    assert_eq!(
        unsafe { libc::geteuid() },
        0,
        "Native isolation tests require root"
    );
    fs::create_dir_all("/data/zeroclaw/shared/skills").unwrap();
    fs::create_dir_all("/data/zeroclaw/data").unwrap();
    fs::write("/data/zeroclaw/config.toml", "schema_version = 3").unwrap();
    fs::write("/data/zeroclaw/data/private-fixture", "TEST_private_data").unwrap();
    fs::write("/data/zeroclaw/auth-profiles.json", "TEST_private_auth").unwrap();
    let session = Session::new().unwrap();
    let mut missing_cache = Command::new("/bin/true");
    assert!(
        session
            .configure(&mut missing_cache, Scope::Skills)
            .is_err(),
        "Missing production bootstrap directory must fail closed"
    );
    fs::create_dir_all("/data/zeroclaw/data/skills-registry").unwrap();
    let mut probe = Command::new("/bin/sh");
    probe.env_clear().args([
        "-c",
        // Registry and managed bundle are usable without reading private siblings.
        r#"
        set -eu
        test -d /data/zeroclaw/data
        mkdir -p /data/zeroclaw/data
        echo ok > /data/zeroclaw/data/skills-registry/probe
        test "$(cat /data/zeroclaw/data/skills-registry/probe)" = ok
        mkdir -p /data/zeroclaw/shared/skills/managed
        cp /data/zeroclaw/data/skills-registry/probe /data/zeroclaw/shared/skills/managed/probe
        if cat /data/zeroclaw/data/private-fixture; then exit 1; fi
        if cat /data/zeroclaw/auth-profiles.json; then exit 1; fi
        "#,
    ]);
    session.configure(&mut probe, Scope::Skills).unwrap();
    let output = probe.output().unwrap();
    assert!(
        output.status.success(),
        "Private cache and managed bundle must be writable; daemon data must stay unreadable"
    );
    assert!(
        output.stdout.is_empty(),
        "Private fixture content must remain unreadable"
    );
    if let Ok(runtime) = std::env::var("HEIMDALL_TEST_RUNTIME") {
        // Offline positive native install: a fresh synthetic registry avoids
        // external services while exercising the actual CLI under Scope::Skills.
        fs::write(
            "/data/zeroclaw/config.toml",
            "schema_version = 3\n[skill_bundles.managed]\ndirectory = \"shared/skills/managed\"\n",
        )
        .unwrap();
        let registry = std::path::Path::new("/data/zeroclaw/data/skills-registry");
        fs::create_dir_all(registry.join("skills/fixture-skill")).unwrap();
        fs::write(registry.join(".zeroclaw-skills-registry-sync"), "synced").unwrap();
        let skill = "---\nname: fixture-skill\ndescription: synthetic fixture\n---\n\nSummarize a short document.\n";
        fs::write(registry.join("skills/fixture-skill/SKILL.md"), skill).unwrap();
        let mut install = Command::new(runtime);
        install
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
            .env("ZEROCLAW_CONFIG_DIR", "/data/zeroclaw")
            .args(["skills", "install", "fixture-skill", "--bundle", "managed"]);
        session.configure(&mut install, Scope::Skills).unwrap();
        let output = install.output().unwrap();
        assert!(
            output.status.success(),
            "Native fixture installation failed"
        );
        assert_eq!(
            fs::read_to_string("/data/zeroclaw/shared/skills/managed/fixture-skill/SKILL.md")
                .unwrap(),
            skill
        );
    } else {
        eprintln!("Native skill installation requires HEIMDALL_TEST_RUNTIME");
    }
    fs::remove_dir_all("/data/zeroclaw").unwrap();
}
