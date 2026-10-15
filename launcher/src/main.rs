use heimdall_launcher::config::{parse_allowed_ids, render_config, secret_overrides};
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
        .envs(vars.iter().cloned());
    command
}
fn main() -> io::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--skills-mcp") {
        return heimdall_launcher::skills_mcp::serve();
    }
    if std::env::args().nth(1).as_deref() == Some("--transfer-mcp") {
        return heimdall_launcher::transfer_mcp::serve();
    }
    if std::env::args().nth(1).as_deref() == Some("--codespace-mcp") {
       return heimdall_launcher::codespace_mcp::serve();
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
    let vars = secret_overrides(&token, &executor).map_err(fail)?;
    if let Ok(url) = std::env::var("HEIMDALL_EXECUTOR_URL")
        && url != "http://heimdall-executor.railway.internal:3889"
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
    prepare_identity()?;
    atomic_write(Path::new(STATE).join("config.toml").as_path(), &config)?;
    let workspace = Path::new(STATE).join("agents/primary/workspace");
    fs::create_dir_all(&workspace)?;
    for directory in ["shared/sops", "shared/plugins", "shared/skills/managed"] {
        fs::create_dir_all(Path::new(STATE).join(directory))?;
    }
    let tools = workspace.join("TOOLS.md");
    if !tools.exists() {
        atomic_write(&tools, include_str!("../../deploy/bot/TOOLS.md"))?;
    }
    let identity = workspace.join("IDENTITY.md");
    if !identity.exists() {
        atomic_write(
            &identity,
            "# Identity\nYou are HeimdallClawd, a private assistant. Use the supplied workspace__bash, workspace__file_read and workspace__file_write tools for all user file and command requests. They act in a separate writable executor workspace. Your local control filesystem is not the user's tool workspace. Never claim a tool succeeded without its result.\n",
        )?;
    }
    let profile = Path::new(STATE).join("auth-profiles.json");
    if !profile.is_file() {
        return Err(fail(
            "ChatGPT sign-in required: run gosu heimdall zeroclaw auth login --config-dir /data/zeroclaw --model-provider openai-codex --device-code in Railway terminal",
        ));
    }
    // Native refresh rejects missing/wrong-provider profiles and unusable tokens.
    // Capture all diagnostics privately: OAuth errors can contain credentials.
    let validation = runtime(&vars)
        .args(["auth", "refresh", "--model-provider", "openai-codex"])
        .output()?;
    if !validation.status.success() {
        return Err(fail(
            "ChatGPT authentication validation failed; complete ZeroClaw device sign-in in the Railway terminal",
        ));
    }
    println!(
        "HeimdallClawd: ZeroClaw 0.8.5; {} allowed Telegram user(s); remote workspace MCP; port {port}",
        ids.len()
    );
    use std::os::unix::process::CommandExt;
    Err(runtime(&vars)
        .args([
            "daemon",
            "--host",
            "0.0.0.0",
            "--port",
            &port.to_string(),
            "--verbose",
        ])
        .exec())
}
