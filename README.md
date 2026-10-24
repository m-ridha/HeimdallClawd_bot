# HeimdallClawd

A private Telegram assistant that runs [ZeroClaw](https://github.com/zeroclaw-labs/zeroclaw) with a Rust launcher on Railway. It signs in with a ChatGPT subscription, keeps conversation history and workspace files on the persistent `/data` volume, and runs native networked commands in the main service, with explicitly enabled root access. Optional GitHub Codespaces commands run remotely in repositories admitted by the configured policy and GitHub permissions. It keeps working while your own computer is off.

These capabilities describe repository configuration; the deployed release can differ. Contributor instructions are in [AGENTS.md](AGENTS.md). See [operations](docs/operations.md) for configuration, authentication, persistence and limits; [verification](docs/verification.md) for observed checks and what is not yet live-verified; and [capabilities](docs/capabilities.md) for activation limits and live tests.

## Features

- Private Telegram chats only. The owner (`TELEGRAM_OWNER_ID`) is always allowed; add more users with `TELEGRAM_ALLOWED_USER_IDS`. Additional permitted users are fully trusted operators: they share the workspace, accessible memory/sessions and configured credentialed tools. See the [privilege model](docs/operations.md#trusted-telegram-users) before adding anyone.
- ChatGPT subscription sign-in with `gpt-6.1-sol` selected in the config template, plus optional Gemini OAuth (`gemini-3.8-flash`) and OpenRouter free routes. Switch in Telegram with `/model gemini`, `/model free` and `/model codex`. Other providers are added with native `ZEROCLAW_...` environment overrides and are not enabled by default.
- Native shell/filesystem/Git, internet access, and passwordless sudo root execution. The main image includes gh, Python 3, curl, jq, Node/npm, Bash, SSH, and pinned Rust/Cargo/rustfmt/Clippy.
- Tavily web search, bounded web fetch and HTTP requests, and a Lynx text browser. Tavily is search, not an interactive browser.
- Scheduled agent tasks (reminders, recurring reports), SOPs and permitted multi-step pipelines.
- Named skills from the official ZeroClaw registry through restricted `skills__install` and `skills__list` tools. Skill commands execute locally under native policy.
- Bounded Telegram file transfer between the local workspace and allowlisted users.
- WASM plugin and hook frameworks are enabled; no plugin is bundled.
- Optional signed AgentMail email events with sender/content filters, per-rule SOP assignments, sanitized failure diagnostics and Telegram-controlled rule enable/disable. See [email event setup](docs/operations.md#agentmail-email-events).
- Native scheduling, delegation, local Chromium automation and additional local utilities. See [capabilities](docs/capabilities.md) for prerequisites.

Release deployment is automatic only when a stable GitHub Release is published. Owner-directed heartbeat is enabled; local headless browser automation is enabled.

## Architecture

```text
Telegram / paired dashboard / signed email → heimdallclawd (launcher → ZeroClaw)
                                              /data persistent volume (500 MB)
                                              native tools + internet + sudo root
```

| Path | Responsibility |
| --- | --- |
| `launcher/` | Rust startup wrapper: validates the owner/allowlist, renders non-secret config, checks ChatGPT auth, starts ZeroClaw, and hosts the skills, transfer, Codespaces and email-filter stdio servers |
| `deploy/bot/` | Dockerfiles, ZeroClaw config template, pinned runtime lock and checksums, maintained patches, managed agent instructions (`TOOLS.md`, `IDENTITY.md`), licenses |
| `deploy/tests/` | integration tests installed into the pinned ZeroClaw source during the build |
| `.railway/` | Python infrastructure definition for main service and volumes |
| `.github/workflows/` | Reusable build/verify workflow and the published-release deployment workflow |
| `tools/`, `tools/tests/` | Release packaging, deployment and token-scope scripts with their unit tests |
| `docs/` | Operations, releases, IaC, capabilities, verification and project walkthrough |

The launcher regenerates managed `config.toml` settings on every start. API/bot keys reach ZeroClaw through environment overrides; native subscription sign-in stays private on the data volume. Extra skill-bundle definitions, directory mappings and filters are preserved from the saved config. When email ingress is enabled, the launcher supervises ZeroClaw behind one HTTP listener, filtering `/agentmail/events` and proxying other gateway traffic, including upgrades. See the [project walkthrough](docs/project-guide.md) and [migration guide](docs/single-service-migration.md) for details.

## Prerequisites

Install the Rust toolchain at **1.99** (edition 2024), Git and Python **3.10+** (CI uses 3.12). Docker with a running daemon is needed to reproduce native Linux builds. ZeroClaw and the launcher run as Rust binaries. The main image also includes Node/npm for agent tasks; Node 26 builds the dashboard and supplies that toolkit, while CI uses Node 24 to install the Railway CLI. Host Node is not required for the portable checks. Infrastructure work additionally needs Railway CLI **5.68.0** and the pinned Python SDK:

```sh
python3 -m venv /tmp/heimdall-iac
/tmp/heimdall-iac/bin/pip install -r .railway/requirements.txt
```

No `.env` file or credentials are needed for the local checks below. There is no local run target: the launcher needs root Linux initialization, native integration checks require a Linux builder.

## Quick start

After cloning the repository and entering its root directory:

```sh
cargo test --locked --manifest-path launcher/Cargo.toml
python3 -B -m unittest discover -s tools/tests -v
```

Lint and check formatting:

```sh
cargo clippy --locked --manifest-path launcher/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path launcher/Cargo.toml -- --check
```

On macOS these run only the portable tests. Linux-only bootstrap, narrow subprocess and upstream integration checks are gated with `cfg(target_os = "linux")`, require root, and are not counted as passing on macOS. They run in the release build workflow; to validate a branch without deploying, run **Actions → Build and verify release binaries → Run workflow** and set the source ref to the exact branch name or full 40-character commit SHA. Prefer the SHA for reproducibility; the workflow selector and source-ref input are separate. Its default `v0.0.0` label is only package metadata.

The IaC guard test is skipped unless `railway_sdk` is installed in the Python environment running the tests.

## Configuration

Deployment-specific values and secrets live in Railway variables. Shared non-secret behavior belongs in the config template; never put credentials in Git, GitHub variables, release assets or logs.

| Service | Variable | Purpose |
| --- | --- | --- |
| heimdallclawd | `TELEGRAM_BOT_TOKEN` | Required secret from BotFather |
| heimdallclawd | `TELEGRAM_OWNER_ID` | Required numeric owner ID; no default |
| heimdallclawd | `TELEGRAM_ALLOWED_USER_IDS` | Optional comma-separated positive IDs; blank means owner only |
| heimdallclawd | `PORT` | Set explicitly (3888); Railway's injected port overrides Docker's default |
| heimdallclawd | `ZEROCLAW_web_search__tavily_api_key` | Optional Tavily key |
| heimdallclawd | `ZEROCLAW_providers__models__openrouter__free__api_key` | Optional key for the `openrouter/free` route |
| heimdallclawd | `GITHUB_TOKEN` | Optional raw GitHub token for native gh/Git, HTTP requests and Codespaces; no `Bearer` prefix |
| heimdallclawd | `HEIMDALL_CODESPACE_REPOS` | Comma-separated `owner/repo` list, or standalone `*` for GitHub-authorized repositories; see [GitHub setup](docs/operations.md#github-access) |
| heimdallclawd | `AGENTMAIL_AUTHORIZATION` | Optional full `Bearer ...` value for the AgentMail secret reference |

Sign in to ChatGPT once from the bot's Railway dashboard terminal (and optionally sign in to Gemini for the `/model gemini` route); never paste authentication files or tokens into chat, Git or logs:

```sh
gosu heimdall zeroclaw auth login --config-dir /data/zeroclaw --model-provider openai-codex --device-code
gosu heimdall heimdall-launcher --gemini-login
```

If the daemon was already running when `/data/zeroclaw/.gemini/oauth_creds.json` was first created (or if a Workspace/Standard Google account requires setting `ZEROCLAW_providers__models__gemini__default__oauth_project`), reload via the paired dashboard or redeploy once so the runtime loads the new Gemini configuration. See [authentication setup](docs/operations.md#authentication-and-state) and [operations](docs/operations.md) for provider setup, Tavily, AgentMail, skills and the tool boundary.

## Releases and deployment

Publishing a stable (non-prerelease) GitHub Release with a `vX.Y.Z` tag deploys that tag's source after a main-ancestry check. A tag push alone, a draft or a prerelease never deploys. The workflow builds and tests the binaries natively, applies the Railway infrastructure plan, deploys the agent and waits for its exact deployment to report `SUCCESS`, and finally attaches the verified package and `SHA256SUMS` to the existing release. Use a new version rather than moving a published tag.

Configure the GitHub `production` environment with secret `RAILWAY_TOKEN` (a project token scoped to the production environment) and variables `RAILWAY_PROJECT_ID`, `RAILWAY_ENVIRONMENT_ID`, `RAILWAY_AGENT_SERVICE_ID`. See [versioned releases](docs/releases.md) for the step-by-step process, caching and recovery, including separate [build-time and runtime privilege checks](docs/releases.md#build-time-and-runtime-privilege-checks).

Infrastructure is defined in [`.railway/railway.py`](.railway/railway.py) with Railway CLI 5.68.0 and Python SDK 0.7.0. Service variables are unmanaged by IaC and volumes are preserved. See [IaC operations](docs/railway-iac.md) and [.railway/README.md](.railway/README.md).

Keep one replica per service and sleeping disabled. Operate frugally, monitor usage, and configure budgets to suit the deployment. Telegram polling needs no public domain; [AgentMail events](docs/agentmail-events.md) use the agent's authenticated public gateway. See the migration guide before retiring old workspace storage.

See [GitHub access setup](docs/operations.md#github-access) for token permissions, Codespaces limits and verification.

Installed skill files persist when stored under the existing `/data` volume.
Restart/redeploy automatically restores registered bundle assignments;
`heimdall-launcher --refresh-skills` plus paired dashboard reload is for attaching
new bundles without restarting. The template names bundles but does not download
their contents. See [skill persistence](docs/operations.md#operator-installed-skill-bundles).

Agent tool and identity instructions refresh on startup; personal notes belong in `USER.md`. See [instruction updates](docs/operations.md#managed-agent-instructions) and [tool recovery](docs/operations.md#tool-discovery-and-recovery).
