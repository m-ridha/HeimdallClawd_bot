use heimdall_launcher::config::{
    EXECUTOR_ORIGIN, github_authorization, parse_allowed_ids, render_config, secret_overrides,
};
use std::{
    fs,
    io::{self, Write},
    path::Path,
    process::Command,
};
const STATE: &str = "/data/zeroclaw";
const RUNTIME: &str = "/usr/local/bin/zeroclaw";
fn fail(message: &str) -> io::Error {
    io::Error::other(message)
}
#[cfg(target_os = "linux")]
fn prepare_identity() -> io::Result<()> {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
    if unsafe { libc::geteuid() } != 0 {
        return Err(fail(
            "Launcher requires the container root identity for initialization",
        ));
    }
    let raw = unsafe {
        libc::open(
            c"/data".as_ptr(),
            libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let root = unsafe { OwnedFd::from_raw_fd(raw) };
    if unsafe { libc::fchown(root.as_raw_fd(), 0, 0) } != 0
        || unsafe { libc::fchmod(root.as_raw_fd(), 0o755) } != 0
    {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::mkdirat(root.as_raw_fd(), c"zeroclaw".as_ptr(), 0o700) } != 0
        && io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST)
    {
        return Err(io::Error::last_os_error());
    }
    let raw = unsafe {
        libc::openat(
            root.as_raw_fd(),
            c"zeroclaw".as_ptr(),
            libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let state = unsafe { OwnedFd::from_raw_fd(raw) };
    if unsafe { libc::fchown(state.as_raw_fd(), 1000, 1000) } != 0
        || unsafe { libc::fchmod(state.as_raw_fd(), 0o700) } != 0
    {
        return Err(io::Error::last_os_error());
    }
    drop_runtime_identity()
}
#[cfg(target_os = "linux")]
fn drop_runtime_identity() -> io::Result<()> {
    // SAFETY: called before starting threads; discard supplementary groups and
    // irreversibly adopt the image's fixed, unprivileged runtime identity.
    if unsafe { libc::setgroups(0, std::ptr::null()) } != 0
        || unsafe { libc::setgid(1000) } != 0
        || unsafe { libc::setuid(1000) } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn prepare_identity() -> io::Result<()> {
    Err(fail("Production launcher requires Linux"))
}
fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    if let Err(error) = file
        .write_all(content.as_bytes())
        .and_then(|_| file.sync_all())
        .and_then(|_| fs::rename(&temp, path))
    {
        let _ = fs::remove_file(temp);
        return Err(error);
    }
    Ok(())
}
fn runtime(vars: &[(String, String)]) -> Command {
    let mut command = Command::new(RUNTIME);
    command
        .env("HOME", STATE)
        .env("ZEROCLAW_CONFIG_DIR", STATE)
        .env_remove("ZEROCLAW_DATA_DIR")
        .env_remove("HEIMDALL_GITHUB_AUTHORIZATION")
        .envs(vars.iter().cloned());
    command
}
fn main() -> io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--mail-mcp") {
        return heimdall_launcher::agentmail::serve_mcp();
    }
    if std::env::args().nth(1).as_deref() == Some("browser-child") {
        return heimdall_launcher::child_policy::browser();
    }

    if std::env::args().nth(1).as_deref() == Some("--skills-mcp") {
        return heimdall_launcher::skills_mcp::serve();
    }
    if std::env::args().nth(1).as_deref() == Some("--transfer-mcp") {
        return heimdall_launcher::transfer_mcp::serve();
    }
    if std::env::args().nth(1).as_deref() == Some("--codespace-mcp") {
        return heimdall_launcher::codespace_mcp::serve();
    }
    // Recovery must precede secret reads, identity initialization and all writes.
    if std::env::args().nth(1).is_none() {
        let recovery = match std::env::var("HEIMDALL_RECOVERY") {
            Ok(value) => value,
            Err(std::env::VarError::NotPresent) => "0".into(),
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(fail("Invalid HEIMDALL_RECOVERY"));
            }
        };
        if heimdall_launcher::startup::recovery_enabled(Some(&recovery))? {
            let port: u16 = std::env::var("PORT")
                .unwrap_or_else(|_| "3888".into())
                .parse()
                .map_err(|_| fail("Invalid PORT"))?;
            if port == 0 {
                return Err(fail("PORT must be nonzero"));
            }
            #[cfg(target_os = "linux")]
            drop_runtime_identity()?;
            heimdall_launcher::startup::report_storage("/data");
            return heimdall_launcher::startup::serve_recovery(port);
        }
    }
    let owner =
        std::env::var("TELEGRAM_OWNER_ID").map_err(|_| fail("TELEGRAM_OWNER_ID is required"))?;
    let allowed = std::env::var("TELEGRAM_ALLOWED_USER_IDS").ok();
    let ids = parse_allowed_ids(&owner, allowed.as_deref()).map_err(fail)?;
    let model = std::env::var("HEIMDALL_MODEL").unwrap_or_else(|_| "gpt-6.1-sol".into());
    let config = render_config(&ids, &model).map_err(fail)?;
    if std::env::args().nth(1).as_deref() == Some("--render-config") {
        print!("{config}");
        return Ok(());
    }
    let token =
        std::env::var("TELEGRAM_BOT_TOKEN").map_err(|_| fail("TELEGRAM_BOT_TOKEN is required"))?;
    let executor = std::env::var("HEIMDALL_EXECUTOR_TOKEN")
        .map_err(|_| fail("HEIMDALL_EXECUTOR_TOKEN is required"))?;
    let mut vars = secret_overrides(&token, &executor).map_err(fail)?;
    let mail_settings = heimdall_launcher::agentmail::Settings::from_env()?;
    match std::env::var("GITHUB_TOKEN") {
        Ok(token) => vars.push((
            "HEIMDALL_GITHUB_AUTHORIZATION".into(),
            github_authorization(&token).map_err(fail)?,
        )),
        Err(std::env::VarError::NotPresent) => {}
        Err(std::env::VarError::NotUnicode(_)) => return Err(fail("Invalid GITHUB_TOKEN")),
    }
    if let Ok(url) = std::env::var("HEIMDALL_EXECUTOR_URL")
        && url != EXECUTOR_ORIGIN
    {
        return Err(fail(
            "HEIMDALL_EXECUTOR_URL must use the configured private executor",
        ));
    }
    let port: u16 = std::env::var("PORT")
        .unwrap_or_else(|_| "3888".into())
        .parse()
        .map_err(|_| fail("Invalid PORT"))?;
    if port == 0 {
        return Err(fail("PORT must be nonzero"));
    }
    heimdall_launcher::startup::report_storage("/data");
    prepare_identity()?;
    atomic_write(Path::new(STATE).join("config.toml").as_path(), &config)?;
    let workspace = Path::new(STATE).join("agents/primary/workspace");
    fs::create_dir_all(&workspace)?;
    for directory in ["shared/sops", "shared/plugins", "shared/skills/managed"] {
        fs::create_dir_all(Path::new(STATE).join(directory))?;
    }
    heimdall_launcher::instructions::sync(&workspace)?;
    let profile = Path::new(STATE).join("auth-profiles.json");
    if !profile.is_file() {
        return Err(fail(
            "ChatGPT sign-in required: run gosu heimdall zeroclaw auth login --config-dir /data/zeroclaw --model-provider openai-codex --device-code in Railway terminal",
        ));
    }
    // Native refresh rejects missing/wrong-provider profiles and unusable tokens.
    // Capture all diagnostics privately: OAuth errors can contain credentials.
    let mut auth = Command::new(RUNTIME);
    heimdall_launcher::startup::configure_auth(&mut auth, STATE);
    heimdall_launcher::startup::validate_auth(
        auth.args(["auth", "refresh", "--model-provider", "openai-codex"]),
        heimdall_launcher::startup::AUTH_TIMEOUT,
    )?;
    println!(
        "HeimdallClawd: ZeroClaw 0.8.6; {} allowed Telegram user(s); remote workspace MCP; port {port}",
        ids.len()
    );
    use std::os::unix::process::CommandExt;
    if let Some(settings) = mail_settings {
        return heimdall_launcher::agentmail::supervise(runtime(&vars), port, settings);
    }
    Err(heimdall_launcher::startup::configure_daemon(&mut runtime(&vars), port).exec())
}
