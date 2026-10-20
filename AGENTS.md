# Agent guide for HeimdallClawd

## Start here

This guide contains durable instructions and user decisions. All required context is in this repository; private planning files and historical chat exports are not prerequisites. Inspect affected code before treating a requirement or a historical passing check as current behavior.

Read the user's current instructions first, then consult:

- [README](README.md) for product, setup and configuration.
- [Operations](docs/operations.md), [capabilities](docs/capabilities.md) and the [executor README](executor/README.md) for behavior, limits and the tool boundary.
- [Releases](docs/releases.md) and [Railway IaC](docs/railway-iac.md) for deployment and infrastructure.
- [Verification](docs/verification.md) for observed evidence and what is still unverified, and the [project walkthrough](docs/project-guide.md) for folder purposes.
- The affected code and tests. Dated verification and audit entries describe the state at that time; the current source and tests win.

## Stack, setup, and checks

Rust (edition 2024, toolchain 1.99) for `launcher/` and `executor/`, which are separate crates with their own `Cargo.toml` and `Cargo.lock` (there is no workspace). Python 3.10+ standard library for `tools/`. Python SDK `railway-sdk==0.7.0` and Railway CLI 5.68.0 for `.railway/`. Docker multi-stage builds compile the pinned ZeroClaw runtime on a native Linux x86_64 builder. Prefer the standard library and existing dependencies; do not silently upgrade the toolchain, SDK, CLI, base images or pinned runtime.

Follow the [README prerequisites and quick start](README.md#prerequisites) for a fresh clone. No `.env` file or credentials are needed for local checks, and there is no local run target.

```sh
cargo test --locked --manifest-path launcher/Cargo.toml
cargo test --locked --manifest-path executor/Cargo.toml
cargo clippy --locked --manifest-path launcher/Cargo.toml --all-targets -- -D warnings
cargo clippy --locked --manifest-path executor/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path launcher/Cargo.toml    # likewise for executor
python3 -B -m unittest discover -s tools/tests -v
```

- Run the Cargo and Python checks above for implementation changes. Format Rust with rustfmt defaults; no `rustfmt.toml` exists, so do not add one without approval. Do not reformat unrelated code.
- `tools/tests/test_iac.py` is skipped unless `railway_sdk` is installed. When touching `.railway/`, install `.railway/requirements.txt` into a virtual environment so it actually runs, then review `railway config plan` read-only.
- Linux-only tests (`#![cfg(target_os = "linux")]`) assert root and exercise seccomp, UID changes, symlink bootstrap and the native MCP client. They do not run meaningfully on macOS, and an ARM64 Docker engine cannot compile the x86_64 seccomp list. Never report them as passing from a macOS run. Validate them with **Actions → Build and verify release binaries → Run workflow** on the branch (default `v0.0.0` label, no deployment), and report that run's result.
- Neither repository build/release workflow runs automatically on pull requests. `build.yml` is reusable and manually dispatched; `release.yml` runs only on published releases. GitHub-managed code scanning can run separately. State which checks you actually ran. Report unavailable tools and exact commands/results; never present historical checks as a fresh run.
- For documentation-only changes, check links/paths, consistency with the code, and `git diff --check`; runtime tests are unnecessary. For workflow changes, also run `actionlint` when available.
- Keep `Cargo.lock` changes in the same commit as the `Cargo.toml` change that caused them. Docker builds and CI use `--locked`.

## Product constraints

HeimdallClawd is a private, single-owner Telegram assistant. The bot holds credentials; a separate executor holds none and does all file and command work.

- Accept only private chats from permitted accounts. `TELEGRAM_OWNER_ID` is required and never hardcoded; omitting it stops startup. IDs are positive decimal integers without leading zeros that fit `i64`; the owner is always included and duplicates are removed. Malformed entries fail startup rather than being skipped.
- Secrets live only in Railway variables. Never put them in the config template, TOML, Git, GitHub variables, release assets, logs or error messages. ZeroClaw reads them through native `ZEROCLAW_...` environment overrides; the launcher regenerates `config.toml` at startup, so manual edits to the deployed file do not persist.
- Authentication is ChatGPT subscription sign-in with a native refresh check before polling. There is no automatic API-key or paid-provider fallback. Other providers are explicit, owner-configured additions.
- Host tools stay blocked: `shell`, `file_read`, `file_write`, `git_operations`, `schedule` and `delegate` remain in `excluded_tools`, `allowed_commands` stays empty, and there is no host execution fallback when the executor fails.
- Tool grants are explicit. Registering an MCP server or plugin grants nothing; add its exact prefixed tool names to both `allowed_tools` and `auto_approve` (and to `pipeline.allowed_tools` only when pipelines should use it). Update the pinned count and expectations in `launcher/tests/config.rs` in the same change.
- Keep infrastructure frugal and avoid unnecessary always-on services. Budget is operator-controlled rather than a fixed project constraint. Keep one replica per service, sleeping disabled, the existing IaC region, and the existing volume names, sizes and mounts: `heimdall-data` at `/data` (500 MB) and `executor-workspace` at `/data/workspace` (200 MB). Volumes are storage, not backups.
- Public web access allows public hosts only (`allow_private_hosts = false`). The workspace executor stays network-disabled.
- For reminders without an explicit timezone, use the timezone recorded in the agent’s `USER.md`; ask the requesting user if it is unknown. An explicit timezone in the request takes precedence (see `deploy/bot/TOOLS.md`).
- Append unscheduled ideas to the existing `docs/ideas.md` rather than implementing them. A backlog or capability list is not authorization to build it all.

## Architecture

| Path | Responsibility |
| --- | --- |
| `launcher/src/main.rs` | Root-only identity preparation (symlink-safe, `O_NOFOLLOW`), config rendering, auth validation, `exec` into ZeroClaw |
| `launcher/src/config.rs` | Allowlist parsing, template rendering, secret overrides (portable, no I/O) |
| `launcher/src/agentmail.rs` | Opt-in signed email admission, deterministic filters, SOP assignments, dispatch diagnostics, gateway proxy/supervisor and rule-management MCP; tests under `launcher/src/agentmail/` |
| `launcher/src/skills_mcp.rs`, `transfer_mcp.rs`, `codespace_mcp.rs` | Narrow stdio MCP servers for registry skills, bounded Telegram transfers and GitHub Codespaces |
| `launcher/src/startup.rs`, `child_policy.rs`, `mcp_stdio.rs` | Bounded auth/recovery startup, native child credential/filesystem profiles, shared bounded stdio framing |
| `executor/src/` | `main` (helper dispatch and startup probe), `server` (axum routes, auth, admission), `mcp` (JSON-RPC adapter), `process` (supervisor), `linux` (seccomp, rlimits, probe), `files` (fd-relative workspace I/O), `protocol` (jobs, limits) |
| `executor/tests/`, `executor/contract/` | API, filesystem, MCP and Linux isolation tests; real ZeroClaw MCP client contract |
| `deploy/bot/` | `Dockerfile` (source build and validation), `Dockerfile.artifact` (prebuilt packaging), `config.template.toml`, `TOOLS.md`, `runtime.lock.json`, `patches/` and generated release provenance |
| `deploy/tests/`, `deploy/executor/` | Tests installed into the pinned ZeroClaw crates at build time; executor artifact Dockerfile |
| `.railway/` | `railway.py` IaC, requirements, README |
| `.github/workflows/` | `build.yml` (compile, validate, package) and `release.yml` (deploy) |
| `tools/`, `tools/tests/` | `package-release.py`, `deploy-release.py`, `verify-railway-token.py`, `verify-clean-tree.py` and their tests |

- Dependencies point inward. The launcher's `config` module stays pure and testable; process, filesystem and network work stays in the MCP and `main` modules. The executor splits transport (`server`, `mcp`), supervision (`process`) and confinement (`linux`, `files`). Keep handlers thin and keep protocol limits in `protocol.rs`.
- The executor service exposes exactly `bash`, `file_read`, `file_write`, `file_read_base64` and `file_write_base64`. Adding or removing a tool means updating `mcp.rs`, `Job::validate`, the helper dispatch in `main.rs`, the seccomp allowlist if needed, `executor/tests/mcp.rs`, `executor/contract/mcp-client.rs`, the config template and the docs that name the tool list.
- `launcher/src/config.rs` edits fixed template paths (`peer_groups.telegram_home.external_peers`, the Codex provider model, the `codex` model route). Restructuring `config.template.toml` requires updating `render_config` and its tests together. The template, `TOOLS.md` and `IDENTITY.md` text are embedded with `include_str!`; if you add another embedded file, add it to the `launcher-build` `COPY` lines in `deploy/bot/Dockerfile`.
- `TOOLS.md` and `IDENTITY.md` are managed by `launcher/src/instructions.rs` and refreshed at startup. Replaced copies are preserved as hidden .*.saved-* files; custom notes belong in USER.md, which is never changed. New embedded instruction files must be included in Docker COPY lines.
- Telegram policy lives in `private-telegram.patch`, remote skill dispatch in `remote-skills.patch`; typed transcription and WIT paths use upstream implementations. Patches must apply with `patch --fuzz=0 -p1` to the pinned source and carry their own tests. Do not edit a patch by hand without re-running the build workflow, and do not remove the production WASM feature to speed builds.
- The pinned ZeroClaw revision and source SHA-256 appear in `deploy/bot/runtime.lock.json`, `deploy/bot/Dockerfile` (`runtime-source`), `executor/Dockerfile` (`contract`), `deploy/bot/NOTICE.md`, the image labels and the docs. Upgrading the runtime changes all of them together, rebases the patches, and needs a full native build; never bypass the checksum check.
- The runtime-compile layer must depend only on the pinned source, patches and compiler settings, not on launcher or configuration files, so configuration-only releases reuse the compile cache. Preserve the separate `runtime-binary` and `support-binaries` targets and the GHCR registry cache. Release compilation uses LTO off, 16 codegen units and optimization level 1; change this only with measured evidence.
- `.dockerignore` excludes `docs/` and `.railway/` from build contexts, and executables are never committed (`prebuilt/` and `target/` are ignored).

## Security invariants

Preserve these accepted behaviors; each has a regression test that must keep passing.

- Executor helpers run as UID/GID 64173 with a cleared environment, `PATH=/usr/bin:/bin`, `no_new_privs`, resource limits and the x86_64 seccomp allowlist. Do not add a syscall without a concrete need, a comment in the PR explaining it, and a Linux test. `clone3` stays denied with ENOSYS for glibc fallback; namespace-creating `clone` flags stay denied; networking, `ptrace`, `setsid` and `setpgid` stay denied.
- Commands time out at 30 seconds, output and file content are bounded to 64 KiB (48 KiB for binary transfer), at most two jobs run concurrently, and the whole process group is killed on completion, timeout, cancellation and truncation. The executor is a subreaper; orphan descendants must not survive.
- Workspace paths are relative, without traversal, and resolved with `openat` and `O_NOFOLLOW`; symlinks are never followed and writes are atomic replacements. Root startup must never `chown` or initialize anything inside the user-controlled workspace (`entrypoint.sh` and the `/data/workspace/.tmp` symlink test).
- Executor health stays unready, and MCP returns 503, unless the startup isolation probe passes. Bearer comparison stays constant time. Request bodies stay bounded and IDs stay bounded and typed.
- Control files (`/run/heimdall-control`, `EXECUTOR_TOKEN`) must stay unreadable to helpers.
- The skills installer accepts only bare lowercase registry names; no URLs, paths, flags or extra arguments. It forces the official registry, clears the environment (no Telegram or provider credentials), disables Git hooks and local/ext protocols, and bounds time and output.
- Telegram transfers are capped at 48 KiB, recipients must be in the allowlist, imports read only below the agent's `telegram_files` directory with symlink-safe traversal, and executor credentials never reach workspace jobs.
- Error messages and logs never echo tokens, authorization values, auth files or user content. Observability keeps tool/prompt payload capture off; diagnostic persistence rotates at 1 MiB with three archives and three-day archive retention. Keep tests asserting that secrets are absent from rendered and saved configuration.
- Reject, do not repair, invalid configuration: invalid allowlist entries, invalid model names, short tokens and a nonstandard `HEIMDALL_EXECUTOR_URL` stop startup.

## Build, release and infrastructure

- Published non-prerelease GitHub Releases with a `vX.Y.Z` tag deploy through `.github/workflows/release.yml`. Suffixes are not accepted (the build workflow and `package-release.py` reject them). Tag pushes, drafts and prereleases never deploy. Preserve the main-ancestry check, serialized deployments (`concurrency: railway-production`), checksum verification of the package, executor-then-bot ordering, exact-deployment-ID `SUCCESS` polling and asset upload only after both succeed.
- Two services cannot be deployed atomically. If the bot fails after the executor succeeds, inspect compatibility and recover in Railway before rerunning. A source fix needs a new version; a published tag is never moved, and rerunning a release uses its original workflow commit.
- Preserve checksum-verified prebuilt binaries in both Railway upload contexts (`sha256sum -c` in each artifact Dockerfile, no `cargo` there), and the validation ordering that exports compile caches before downstream tests. Keep the disk-space cleanup in the validation job.
- Infrastructure lives in `.railway/railway.py`. Keep service variables unmanaged (`variables={"managed": False}`), keep both repository sources disconnected, and keep volume attachments, region, replica counts and resource limits exact; `tools/tests/test_iac.py` pins them. Apply infrastructure only inside the published-release workflow; never run `railway config apply`, `railway up` or `railway redeploy` against production without task authorization. Never pass `--include-variables`, `--decrypt-variables` or `--show-values` in routine checks, and never commit plan or state files. A destructive plan must stop for explicit review; no destructive flag is part of the workflow.
- `RAILWAY_TOKEN` must be a project token scoped to the production environment, held as a secret. Do not add `railway link` to the release flow. Keep the token preflight's explicit application User-Agent and its sanitized, status-only errors. An `Unauthorized` from `railway link` does not prove the token expired.
- Release configuration: secret `RAILWAY_TOKEN` and variables `RAILWAY_PROJECT_ID`, `RAILWAY_ENVIRONMENT_ID`, `RAILWAY_AGENT_SERVICE_ID` and `RAILWAY_EXECUTOR_SERVICE_ID` in the GitHub `production` environment. Never turn the token into a plain variable. Publishing a release requires task authorization.
- When removing a retired identifier from the repository, use `tools/verify-clean-tree.py` (with `--history` for reachable commits). Hosting caches and third-party copies cannot be erased by this repository.

## Working discipline and privacy

- Continue authorized reversible work autonomously, with concise updates. Prepare concrete code and tests before human steps such as Railway dashboard sign-in. Do not invent results or silently replace pinned tools, architecture or boundaries.
- Follow the types in [Semantic Commit Messages](https://gist.github.com/joshbuchea/6f47e86d2510bce28f8e7f42ae84c716), with this project's subject format for commits and PR titles: `<type>: [<scope>] <subject>` when a scope is useful (for example, `feat: [transfer] add Telegram file receive tool`), or `<type>: <subject>` without a scope. Use a concise present-tense subject; do not use parentheses around the scope. Types: `feat`, `fix`, `docs`, `style`, `refactor`, `test`, `chore`.
- Suggested scopes: `launcher`, `executor`, `runtime`, `config`, `telegram`, `skills`, `transfer`, `docker`, `ci`, `release`, `railway`, `tools`, `deps`, `docs`. Use `chore` for build, CI, dependency and infrastructure maintenance (for example, `chore: [ci] pin build cache scope`); a change that alters shipped behavior is `feat` or `fix` even when it touches workflows or Dockerfiles. Examples: `fix: [executor] kill orphaned descendants after timeout`, `refactor: [launcher] split allowlist validation`, `test: [executor] cover symlink rejection`, `docs: [operations] document Tavily key setup`.
- Name branches by the same classification: `<type>/<short-kebab-case-subject>` (a project convention extending the commit guide), such as `fix/executor-descendant-cleanup`.
- Keep each commit to one reviewable change, with tests and docs for that change. Add a body only to explain why or note material limitations; do not repeat the diff. Never mix formatting churn with behavior changes.
- PR descriptions explain the concrete change, how it was validated (exact commands and the build-workflow run when native Linux checks matter), the Railway or variable changes an operator must make, and material limitations such as "not live-verified". Use semantic subjects when listing commits.
- Use GitHub **Squash and merge** when merging authorized PRs, unless the user explicitly requests another method. Use a semantic commit subject for the resulting squash commit. Merge into `main` before releasing; a GitHub Release is titled with its tag (`vX.Y.Z`) and its notes summarize the semantic commits since the previous release.
- Inspect `git status` and preserve staged and user edits. Do not commit, push, publish, merge, set Railway variables or resolve remote threads without task authorization. Do not delegate unless explicitly requested by the user or applicable instructions.
- Keep README focused on durable product, setup and usage guidance. Put operator procedures in `docs/operations.md` and `docs/capabilities.md`, release and IaC procedure in their docs, and dated observations in `docs/verification.md`. Record information once and cross-link; do not duplicate prose or evidence. Distinguish requirements, automated results, live results, human reports and unverified items; never describe a change as live-verified before it has been deployed and exercised. When behavior, tool lists, limits or variables change, update every doc and test that states them.
- Never expose or commit tokens, bot tokens, executor tokens, provider or Tavily keys, `Authorization` values, `auth.json`, `auth-profiles.json`, `.secret_key`, `.env*`, SQLite databases, Telegram message content or workspace files. Use obviously fake fixtures (for example `123456:TEST_token_abcdefghijklmnop`). Never paste credentials into chat, logs or PRs, and prefer `--stdin` or a hidden prompt when setting a Railway variable.

## Rust style and boundary conventions

- Write idiomatic Rust that favors clarity, simplicity and maintainability. Avoid magic strings and numbers, duplicated policy, hidden side effects and ambiguous arguments. Name policy values (limits, IDs, ports, paths) once near their owner, as `protocol::LIMIT` and `STATE` do, without a global constants module.
- Preserve the boundaries above. Keep `unsafe` minimal, confined to libc calls that need it, with a `SAFETY` comment stating the invariant. Prefer fd-relative syscalls over path-based ones for anything user-controlled.
- Handle errors explicitly and keep messages safe and actionable. Distinguish absence from failure and avoid partial-success responses. Do not use `unwrap`, `expect` or indexing on external input in production paths; reserve them for invariants established by validation just before, and for tests. Do not suppress an error to simplify a success path.
- Validate at the boundary before side effects: configuration at startup, MCP arguments through `Job::validate` and each tool's allowed-argument list, paths through `validate_path`. Keep bounds checks and encodings consistent between the launcher and the executor.
- Load configuration once and pass explicit values to functions; avoid hidden environment reads. Keep subprocess environments cleared and explicit. Propagate cancellation, bound all reads, timeouts and concurrency, and clean up child processes on every exit path.
- Prefer guard clauses, small focused functions and explicit types. Document public items that other modules rely on. Avoid speculative flexibility, oversized functions and unrelated stylistic churn.
- Python tooling uses only the standard library, validates inputs (tag and commit patterns, UUIDs, ELF architecture), sanitizes errors so credentials and API responses are never printed, and exits with a short message. Keep scripts testable by exposing functions and patching I/O in `tools/tests`.
- Shell in workflows and Dockerfiles uses `set -euo pipefail` where scripts are multi-step, pins versions (actions, CLI, SDK, base images) and never echoes secrets. Do not weaken checksum, tag, scope or ancestry checks to make a run pass.

## Rust and Python testing

- Add focused regression tests for observable contracts, edge cases, cancellation, concurrency and failure paths, and keep exact expectations explicit (pinned tool lists and counts, error text that must not leak, boundary-case literals) so tests do not derive their expected result from the implementation.
- Keep portable tests portable and Linux tests gated with `#![cfg(target_os = "linux")]` that assert root. Do not relax a root, UID or seccomp assertion to make a test pass elsewhere. Tests that need the runtime binary read `HEIMDALL_TEST_RUNTIME` and skip with a message when it is absent.
- Tests under `deploy/tests/` and `executor/contract/` are copied into the pinned ZeroClaw workspace by `deploy/bot/Dockerfile` or `executor/Dockerfile` and compiled against its APIs. When adding one, add the `COPY` line and its `cargo test` invocation, and keep it consistent with the pinned revision. They cannot be built from this repository alone.
- Use temporary directories, fixed IDs, synthetic data, loopback listeners on port 0 and fake credentials. Generate ephemeral OS-random keys for signing/verification behavior tests rather than embedding cryptographic keys; explicitly labeled standard test vectors can remain fixed. Tests must not require real Telegram, ChatGPT, Tavily, OpenRouter or Railway credentials or network services. A panic inside a spawned task does not fail the test, so return results from spawned tasks and assert them in the test body.
- Prefer explicit synchronization or deadlines over fixed sleeps where practical; where timing is the behavior under test (timeouts, cancellation), keep generous margins and assert the outcome, not the duration.
- Python tests use `unittest`, patch `subprocess`, `urllib` and the clock, and assert sanitized error messages.
- Verify with the commands in [Stack, setup, and checks](#stack-setup-and-checks). A broad refactor must preserve test intent and coverage, not merely shorten assertions.

## GitHub repository workflow

The owner wants work on any repository the GitHub token can access. REST operations use the agent's github secret alias. Codespaces retains explicit repository lists and adds standalone `HEIMDALL_CODESPACE_REPOS=*` for GitHub-authorized repositories; actual account listing and syntax checks remain mandatory. Offline Git is included in executor images; it receives no credentials or networking. Managed instructions explain API-first edits/PRs and Codespaces or authorized GitHub Actions for networked tests. Do not diagnose GitHub access from an executor download failure.
