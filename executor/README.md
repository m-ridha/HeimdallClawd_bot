# HeimdallClawd executor

Private Rust tool service for the ZeroClaw bot. Deploy using the Dockerfile; mount a separate persistent volume at `/data/workspace`, explicitly set `PORT=3889` (Railway otherwise supplies 8080 despite the Docker image ENV), configure a random `EXECUTOR_TOKEN` of at least 32 characters, and expose only Railway private networking on port 3889.

The root HTTP supervisor starts helpers as UID/GID 64173 with a cleared environment, no inherited control descriptors, no_new_privs, and a Linux x86_64 seccomp allowlist. Helpers cannot use networking or alter process groups. Commands have a 30-second wall timeout and 64 KiB combined output limit. File tools accept relative paths, reject symlinks and traversal, and bound content to 64 KiB. Commands can read ordinary container runtime files; no bot credentials belong in this service. The workspace is shared by every allowed Telegram user.

Startup verifies restrictions, files, commands, descendant cleanup, timeout, output limits and cancellation before health becomes ready. Temporary files live in per-job directories under root-owned `/run/heimdall-tmp`, cleaned on completion/cancellation; Programs that hardcode `/tmp` instead of honoring TMPDIR can leave files outside this cleanup boundary. A 1 GiB per-process address-space limit bounds large single-process allocations. This is not a process-tree memory quota. NPROC=64 is shared by the helper UID; file-size limits are per file, not total disk quotas. clone3 returns ENOSYS for glibc fallback; timestamp/copy/archive (including descriptor-based POSIX ACL preservation), file-lock and allocator syscalls are supported. Signaling other jobs remains blocked. Root startup must never initialize a child path within the user-controlled workspace.

`cargo test --locked` on macOS checks portable API/filesystem behavior. The Docker builder runs Linux checks as root; Linux security checks are not counted as passing on macOS. Run `cargo clippy --all-targets -- -D warnings` for lint checks.

Configure the bot with `HEIMDALL_EXECUTOR_URL=http://heimdall-executor.railway.internal:3889` and `HEIMDALL_EXECUTOR_TOKEN` matching the executor token. The bot connects to the authenticated HTTP MCP endpoint `/mcp`, exposing only `bash`, `file_read`, `file_write`, `file_read_base64` and `file_write_base64`. Credentials remain in the control process; helpers receive none. A missing/invalid configuration stops startup. No host execution fallback is configured.

The updated image includes Git for offline init/status/diff/branch/commit operations, Python 3 for standard-library scripting, and jq for JSON processing. Use `python3 -B` to avoid bytecode-cache writes. Python scripts share the 1 GiB per-process address-space cap, time/output limits, UID and syscall restrictions; third-party packages and pip are not provided. Git fetch/clone/push and package downloads remain unavailable in this network-disabled executor; use the agent GitHub API path, Codespaces or authorized CI. Git configuration belongs in the repository or per-command flags because helper HOME is per-job. No token is supplied to workspace Git.

MCP initialization returns an opaque UUID `Mcp-Session-Id`; clients echo it on subsequent requests. Active calls and cancellation are keyed by session plus typed JSON-RPC ID, so independent clients can reuse IDs. Sessions are correlation namespaces for already bearer-authenticated clients, not additional authorization. Canonical UUID headers are required when supplied. Legacy sessionless calls get unique execution keys; ambiguous ID-only cancellation is ignored, while response disconnect still cancels that job. Active-map entries, cancellation tokens and permits are released on task unwind or abort. No session registry or persistent session storage is needed.

Admission queues fairly for at most three seconds while reserving the session-scoped ID. Cancellation/disconnect during admission prevents execution and cleans the reservation. Queue expiration returns JSON-RPC -32001 in the response stream; execution still has its separate 30-second limit. At most two jobs execute concurrently.

## Structured command results

For `bash`, the single MCP `content[0].text` block is now a JSON object:

```json
{"exit_code":7,"termination_reason":"completed","truncated":false,"stdout":"","stderr":""}
```

- `exit_code` is the observed shell exit code, not a boolean or a synthetic `128 + signal` code. It is `null` when the helper was terminated by a signal. A timeout/cancellation can retain an already observed exit code; the termination reason, not that code alone, determines success.
- `termination_reason` is `completed`, `timeout`, `cancelled` or `output_limit`. `completed` means the supervisor finished collecting output normally; the shell can still have exited nonzero or by a signal.
- `truncated` explicitly reports discarded bytes at the output cap. Exact-limit output followed by EOF is not truncated. Timeout/cancellation do not themselves set it; they can interrupt execution before all possible output is produced.
- `stdout` and `stderr` contain only captured command bytes decoded with UTF-8 replacement for invalid sequences, never supervisor annotations. They share the existing **64 KiB raw-byte budget**, not 64 KiB each. JSON escaping/replacement can make the wire representation larger. Cross-stream ordering is not promised.

`isError` stays false only for a completed command with exit code zero. Timeout, cancellation and output-limit termination always set it, even if the shell had already exited successfully before excess buffered output was observed. Process-group cleanup, admission, execution timeouts, isolation and concurrency limits are unchanged. Cancellation after a client disconnect still cleans up the job, but cannot deliver a result to that disconnected client.

JSON is carried in the existing MCP 2024-11-05 text envelope so the pinned client's success and error paths both expose status; duplicating full streams into `structuredContent` would risk exceeding its unchanged response-size bound. Status fields precede streams because that client sanitizes and shortens error details. Consumers of bash text must parse the JSON rather than treat it as raw stdout. File and base64 tool text, error and truncation behavior are unchanged. Infrastructure/launch failures keep the existing generic error text and do not invent a command exit code or termination reason. Queued requests that never start retain their existing JSON-RPC errors.

No Railway variables or operator configuration changes are required. These changes need the normal executor release and native Linux validation before they are considered live-verified.

### Offline Rust tooling

The executor images bundle the pinned Rust 1.99 toolchain, Cargo, rustfmt, Clippy, GCC and standard C development libraries. Use `workspace__bash` to run `cargo fmt --all -- --check`, `cargo test --offline --locked -j 1`, and `cargo clippy --offline --locked -j 1 --all-targets -- -D warnings` in the repository directory. Generate a missing lockfile with `cargo generate-lockfile --offline` only when appropriate for the task. Verify deployed versions before relying on these tools.

No dependency cache or additional targets are bundled. Standard-library projects and repositories with already available dependencies are the intended offline use case. Use a workspace-local `CARGO_HOME` when a persistent cache is needed; helper HOME is temporary. Keep compilation to one job and use `CARGO_INCREMENTAL=0` and `RUSTFLAGS="-C codegen-units=1 -C debuginfo=0"` to reduce build resource use. The per-process virtual-address ceiling was raised from 96 MiB to 1 GiB for Rust; the 250 MB container RAM cap, 30-second deadline and network isolation are unchanged. Native Linux CI passed the confined Rust smoke test with this policy; live Railway verification remains pending. A passing smoke test does not establish that every repository build fits. If a check exceeds these limits or needs dependencies, use Codespaces or authorized GitHub Actions and report the local check as unverified. Never claim successful Rust validation from tool availability alone.

The 1 GiB address-space ceiling allows compiler library mappings; it is not a RAM reservation or a process-tree quota. The executor container still has a 250 MB physical-memory limit shared by all jobs. Large builds or concurrent compilations can exhaust it; prefer one build at a time and move heavier work to Codespaces or authorized CI.

Rust subprocess startup uses unnamed Unix `SEQPACKET` socket pairs for its private exec-error channel. The filter permits only that exact family/type/protocol and address-free connected I/O. Creating network sockets, connecting to endpoints, process-group escape and cross-job signaling remain blocked. The native regression exercises local pair I/O and denied alternative socket types.
