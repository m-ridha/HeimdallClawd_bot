# HeimdallClawd

A private Telegram assistant that runs [ZeroClaw](https://github.com/zeroclaw-labs/zeroclaw) and two small Rust services on Railway. It signs in with a ChatGPT subscription, keeps conversation history and workspace files on persistent volumes, and runs workspace file and command operations in a separate, network-disabled executor. Optional GitHub Codespaces commands run remotely in allowlisted repositories. It keeps working while your own computer is off.

Contributor instructions are in [AGENTS.md](AGENTS.md). See [operations](docs/operations.md) for configuration, authentication, persistence and limits; [verification](docs/verification.md) for observed checks and what is not yet live-verified; and [capabilities](docs/capabilities.md) for activation limits and live tests.

## Features

- Private Telegram chats only. The owner (`TELEGRAM_OWNER_ID`) is always allowed; add more users with `TELEGRAM_ALLOWED_USER_IDS`. Additional permitted users are fully trusted operators: they share the workspace, accessible memory/sessions and configured credentialed tools. See the [privilege model](docs/operations.md#trusted-telegram-users) before adding anyone.
- ChatGPT subscription sign-in with `gpt-6.1-sol` by default (`HEIMDALL_MODEL`), plus an optional OpenRouter free route. Switch in Telegram with `/model free` and `/model codex`. Other providers are added with native `ZEROCLAW_...` environment overrides and are not enabled by default.
- An isolated workspace executor reached over authenticated private MCP: `bash`, `file_read`, `file_write` and 48 KiB binary `file_read_base64`/`file_write_base64`. Helpers run as a separate UID with seccomp restrictions, no networking, a 30-second timeout and 64 KiB output/content bounds.
- Tavily web search, bounded web fetch and HTTP requests, and a Lynx text browser. Tavily is search, not an interactive browser.
- Scheduled agent tasks (reminders, recurring reports), SOPs and permitted multi-step pipelines.
- Named skills from the official ZeroClaw registry through restricted `skills__install` and `skills__list` tools. Script-containing skills run in the executor, never beside provider credentials.
- Bounded Telegram file transfer between the executor workspace and allowlisted users.
- WASM plugin and hook frameworks are enabled; no plugin is bundled.
- Optional signed AgentMail email events with sender/content filters and Telegram-controlled rule enable/disable. See [email event setup](docs/operations.md#agentmail-email-events).
- Native host shell, filesystem, git, shell-only scheduling and delegation tools stay blocked.

Release deployment is automatic only when a stable GitHub Release is published. Owner-directed heartbeat is enabled; headless browser automation is not configured.

## Architecture

```text
Telegram ─► heimdallclawd (launcher ► ZeroClaw) ──private MCP──► heimdall-executor
              /data volume (500 MB)                              /data/workspace volume (200 MB)
```

| Path | Responsibility |
| --- | --- |
| `launcher/` | Rust startup wrapper: validates the owner/allowlist, renders non-secret config, checks ChatGPT auth, starts ZeroClaw, and hosts the `--skills-mcp`, `--transfer-mcp` and `--codespace-mcp` stdio servers |
| `executor/` | Rust private MCP service and Linux-restricted helpers (`src/`), API/filesystem/isolation tests (`tests/`), and a real ZeroClaw MCP client contract (`contract/`) |
| `deploy/bot/` | Dockerfiles, ZeroClaw config template, pinned runtime lock and checksums, maintained patches, managed agent instructions (`TOOLS.md`, `IDENTITY.md`), licenses |
| `deploy/executor/`, `deploy/tests/` | Executor artifact Dockerfile; integration tests installed into the pinned ZeroClaw source during the build |
| `.railway/` | Python infrastructure definition for both services and volumes |
| `.github/workflows/` | Reusable build/verify workflow and the published-release deployment workflow |
| `tools/`, `tools/tests/` | Release packaging, deployment and token-scope scripts with their unit tests |
| `docs/` | Operations, releases, IaC, capabilities, verification and project walkthrough |

The launcher regenerates `config.toml` on every start; secrets reach ZeroClaw only as environment overrides and are never written to TOML. When email ingress is enabled, the launcher supervises ZeroClaw behind one HTTP listener, filtering `/agentmail/events` and proxying other gateway traffic, including upgrades. The executor holds no Telegram or provider credentials. See the [project walkthrough](docs/project-guide.md) and the [executor README](executor/README.md) for details.

## Prerequisites

Install the Rust toolchain at **1.99** (edition 2024), Git and Python **3.10+** (CI uses 3.12). Docker with a running daemon is needed to reproduce native Linux builds. Node.js 24 is used only in CI to install the Railway CLI. Infrastructure work additionally needs Railway CLI **5.68.0** and the pinned Python SDK:

```sh
python3 -m venv /tmp/heimdall-iac
/tmp/heimdall-iac/bin/pip install -r .railway/requirements.txt
```

No `.env` file or credentials are needed for the local checks below. There is no local run target: the launcher needs root Linux initialization, and the executor needs a root Linux x86_64 supervisor for its seccomp filter.

## Quick start

After cloning the repository and entering its root directory:

```sh
cargo test --locked --manifest-path launcher/Cargo.toml
cargo test --locked --manifest-path executor/Cargo.toml
python3 -B -m unittest discover -s tools/tests -v
```

Lint both crates strictly:

```sh
cargo clippy --locked --manifest-path launcher/Cargo.toml --all-targets -- -D warnings
cargo clippy --locked --manifest-path executor/Cargo.toml --all-targets -- -D warnings
```

On macOS these run only the portable tests. Linux-only isolation, bootstrap and native MCP checks are gated with `cfg(target_os = "linux")`, require root, and are not counted as passing on macOS. They run in the release build workflow; to validate a branch without deploying, run **Actions → Build and verify release binaries → Run workflow** and enter the branch name. Its default `v0.0.0` label is only package metadata.

The IaC guard test is skipped unless `railway_sdk` is installed in the Python environment running the tests.

## Configuration

Runtime values live in Railway variables only. Never put them in Git, GitHub, release assets or logs.

| Service | Variable | Purpose |
| --- | --- | --- |
| heimdallclawd | `TELEGRAM_BOT_TOKEN` | Required secret from BotFather |
| heimdallclawd | `TELEGRAM_OWNER_ID` | Required numeric owner ID; no default |
| heimdallclawd | `HEIMDALL_EXECUTOR_TOKEN` | Required secret; must equal the executor's `EXECUTOR_TOKEN` |
| heimdallclawd | `TELEGRAM_ALLOWED_USER_IDS` | Optional comma-separated positive IDs; blank means owner only |
| heimdallclawd | `HEIMDALL_MODEL` | Optional subscription model, default `gpt-6.1-sol` |
| heimdallclawd | `PORT` | Set explicitly (3888); Railway's injected port overrides Docker's default |
| heimdallclawd | `ZEROCLAW_web_search__tavily_api_key` | Optional Tavily key |
| heimdallclawd | `ZEROCLAW_providers__models__openrouter__free__api_key` | Optional key for the `openrouter/free` route |
| heimdallclawd | `GITHUB_TOKEN` | Optional raw GitHub token shared by HTTP requests and restricted Codespaces tools; no `Bearer` prefix |
| heimdallclawd | `HEIMDALL_CODESPACE_REPOS` | Comma-separated `owner/repo` allowlist for Codespaces operations |
| heimdallclawd | `AGENTMAIL_AUTHORIZATION` | Optional full `Bearer ...` value for the AgentMail secret reference |
| heimdall-executor | `EXECUTOR_TOKEN` | Required secret, at least 32 characters |
| heimdall-executor | `PORT` | Set explicitly (3889) |

Sign in to ChatGPT once from the bot's Railway dashboard terminal; never paste authentication files or tokens into chat, Git or logs:

```sh
gosu heimdall zeroclaw auth login --config-dir /data/zeroclaw --model-provider openai-codex --device-code
```

See [operations](docs/operations.md) for provider setup, Tavily, AgentMail, skills and the tool boundary.

## Releases and deployment

Publishing a stable (non-prerelease) GitHub Release with a `vX.Y.Z` tag deploys that tag's source after a main-ancestry check. A tag push alone, a draft or a prerelease never deploys. The workflow builds and tests the binaries natively, applies the Railway infrastructure plan, deploys the executor and waits for its exact deployment to report `SUCCESS`, then does the same for the bot, and finally attaches the verified package and `SHA256SUMS` to the existing release. Use a new version rather than moving a published tag.

Configure the GitHub `production` environment with secret `RAILWAY_TOKEN` (a project token scoped to the production environment) and variables `RAILWAY_PROJECT_ID`, `RAILWAY_ENVIRONMENT_ID`, `RAILWAY_AGENT_SERVICE_ID` and `RAILWAY_EXECUTOR_SERVICE_ID`. See [versioned releases](docs/releases.md) for the step-by-step process, caching and recovery.

Infrastructure is defined in [`.railway/railway.py`](.railway/railway.py) with Railway CLI 5.68.0 and Python SDK 0.7.0. Service variables are unmanaged by IaC and volumes are preserved. See [IaC operations](docs/railway-iac.md) and [.railway/README.md](.railway/README.md).

Keep one replica per service and sleeping disabled. Operate frugally, monitor usage, and configure budgets to suit the deployment. The executor and bot need no public domain; the executor listens on IPv6 for Railway private networking.

See [GitHub access setup](docs/operations.md#github-access) for token permissions, Codespaces limits and verification.

Agent tool and identity instructions refresh on startup; personal notes belong in `USER.md`. See [instruction updates](docs/operations.md#managed-agent-instructions) and [tool recovery](docs/operations.md#tool-discovery-and-recovery).
