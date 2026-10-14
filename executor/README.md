# HeimdallClawd executor

Private Rust tool service for the ZeroClaw bot. Deploy using the Dockerfile; mount a separate persistent volume at `/data/workspace`, explicitly set `PORT=3889` (Railway otherwise supplies8080 despite the Docker image ENV), configure a random `EXECUTOR_TOKEN` of at least 32 characters, and expose only Railway private networking on port 3889.

The root HTTP supervisor starts helpers as UID/GID 64173 with a cleared environment, no inherited control descriptors, no_new_privs, and a Linux x86_64 seccomp allowlist. Helpers cannot use networking or alter process groups. Commands have a 30-second wall timeout and 64 KiB combined output limit. File tools accept relative paths, reject symlinks and traversal, and bound content to 64 KiB. Commands can read ordinary container runtime files; no bot credentials belong in this service. The workspace is shared by every allowed Telegram user.

Startup verifies restrictions, files, commands, descendant cleanup, timeout, output limits and cancellation before health becomes ready. Temporary files live in `/run/heimdall-tmp`; root startup must never initialize a child path within the user-controlled workspace.

`cargo test --locked` on macOS checks portable API/filesystem behavior. The Docker builder runs Linux checks as root; Linux security checks are not counted as passing on macOS. Run `cargo clippy --all-targets -- -D warnings` for lint checks.

Configure the bot with `HEIMDALL_EXECUTOR_URL=http://heimdall-executor.railway.internal:3889` and `HEIMDALL_EXECUTOR_TOKEN` matching the executor token. The bot connects to the authenticated HTTP MCP endpoint `/mcp`, exposing only `bash`, `file_read` and `file_write`. Credentials remain in the control process; helpers receive none. A missing/invalid configuration stops startup. No host execution fallback is configured.
