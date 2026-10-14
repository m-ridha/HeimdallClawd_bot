# Operations

HeimdallClawd uses pinned ZeroClaw 0.8.5 with a Rust launcher and a separate Rust workspace executor. The runtime revision and source archive checksum are in `deploy/bot/runtime.lock.json`. The deployment image applies the private-Telegram gate documented in `deploy/bot/NOTICE.md`.

## Railway services

| Service | Persistent mount | Port | Resource ceiling |
| --- | --- | --- | --- |
| heimdallclawd | `/data` (500 MB) | 3888 | 0.5 GB, 0.5 vCPU |
| heimdall-executor | `/data/workspace` (200 MB) | 3889 | 0.25 GB, 0.25 vCPU |

Keep one replica per service, sleeping disabled, and the account's $5 hard spending limit. Set `PORT` explicitly in Railway: its injected port takes precedence over Docker's environment default. Neither service needs a public domain. The executor listens on IPv6 for Railway private networking.

## Variables

The bot requires `TELEGRAM_BOT_TOKEN`, `TELEGRAM_OWNER_ID`, and `HEIMDALL_EXECUTOR_TOKEN`. No Telegram owner ID is hardcoded; omitting it stops startup. Keep token values in Railway variables only. `TELEGRAM_ALLOWED_USER_IDS` accepts comma-separated numeric IDs with optional surrounding spaces; the owner is always included and duplicates are removed. A blank variable means owner only. Empty entries within a nonempty list, zero, negative and malformed entries fail startup. Only private chats from permitted accounts are accepted. All permitted accounts share the executor workspace; conversation keys include the sender identity.

`HEIMDALL_MODEL` selects the subscription model and defaults to `gpt-6.1-sol`. `HEIMDALL_EXECUTOR_URL`, when supplied, must equal `http://heimdall-executor.railway.internal:3889`; the MCP endpoint is `/mcp`. Set the executor's `EXECUTOR_TOKEN` to the same private value as the bot's executor token.

## Authentication and state

Subscription credentials live in `/data/zeroclaw`, together with configuration and SQLite sessions. This directory belongs to UID 1000 and is private. A native auth check runs before Telegram polling. Operational logs go to Railway stdout; local trace persistence and tool/prompt payload capture are disabled. There is no automatic API-key fallback.

For an interactive device sign-in, use the bot's Railway dashboard terminal:

```sh
gosu heimdall zeroclaw auth login --config-dir /data/zeroclaw --model-provider openai-codex --device-code
```

Complete the official login page yourself. Never paste authentication files or tokens into chat, source control, or logs.

Conversation history uses `/data/zeroclaw/data/sessions/sessions.db`; the agent's SQLite memory lives under its workspace within `/data/zeroclaw/agents/primary/workspace`. These paths survive image replacement while the volume remains attached. Executor files live on its separate volume. A volume is persistent storage, not a backup: retain a private backup before deleting a volume or service. An additional database is unnecessary for this single-instance setup.

## Tool boundary

The primary agent has isolated workspace operations, Tavily search, public web fetching/HTTP/text browsing, agent cron management, SOP/pipeline tools, messaging tools, and restricted Telegram transfers. Parallel tools are enabled; the executor admits at most two concurrent jobs. Native host shell/filesystem/git tools, shell-only scheduling, and agent delegation remain blocked. Skill commands are redirected to the isolated executor by a maintained upstream patch; there is no local fallback when a remote skill command fails.

The remote worker has no Telegram or provider credentials. Helpers run with a cleared environment, separate UID, Linux seccomp restrictions, no networking, a 30-second timeout, 64 KiB output/content bounds, and process-group cleanup. Binary transfer tools use the same path protections with a 48 KiB file limit. Failed startup isolation checks keep the worker unhealthy. See [capabilities](capabilities.md) for scheduling, extensions, transfers and deployment verification.

## Tavily web search

The maintained template enables Tavily basic search with three results and a 15-second timeout. `web_search_tool` is included in the primary agent's allowlist and auto-approval list. The workspace executor remains network-disabled. The agent also has web_fetch, http_request and a Lynx-backed text_browser. Tavily search does not provide interactive browser sessions.

Set `ZEROCLAW_web_search__tavily_api_key` privately on the agent service in Railway, then deploy the updated source through the release workflow. The launcher inherits this environment variable and ZeroClaw resolves it natively. Do not put the key in the configuration template or source. The integration test uses a dummy key, asserts its exact in-memory value, calls native `Config::save()`, and asserts the saved TOML contains no dummy key.

Local checks:

```sh
cargo test --locked --manifest-path launcher/Cargo.toml
```

The native secret-resolution fixture is `deploy/tests/config-secrets.rs`; the source-build release workflow installs it into the checksum-pinned ZeroClaw configuration crate and runs it. Portable launcher tests check exact tool permissions and settings; the separate native fixture proves secret resolution and non-persistence. Neither test needs your actual key.

After deploying, ask the bot: “Use Tavily to search for the latest stable Rust release. Include source links and the release date.” Confirm a real tool call and response. This tests live credentials/API access, which dummy-key tests cannot establish. Keep Tavily pay-as-you-go disabled if you want usage to stop at the free allowance. Search queries are sent to Tavily; the API key is not a model-visible tool argument.

## Switching models in Telegram

The ChatGPT subscription model remains the default. Add or replace `ZEROCLAW_providers__models__openrouter__free__api_key` privately in the bot service's Railway Variables to enable the optional `openrouter/free` route. ZeroClaw reads the native environment override directly; it is not written into TOML. All API-backed provider credentials use the same `ZEROCLAW_providers__models__PROVIDER__ALIAS__api_key` convention.

Send `/model free` to select OpenRouter's free router for your conversation. Send `/model codex` to return to the configured subscription model. `/model` displays the current selection. Routes change the provider and model together, while retaining the primary agent, its conversation and the same permitted tools. These are session overrides; verify the current selection after a restart. No automatic paid-provider fallback is configured.

OpenRouter's free router chooses an available free model supporting the request's features, including tool calling. Its availability, context capacity and rate limits vary; see [the free-router guide](https://openrouter.ai/docs/guides/routing/routers/free-router) and [limits](https://openrouter.ai/docs/api_reference/limits). Selecting this route sends the conversation and tool results needed for the request to OpenRouter and its selected provider. Railway infrastructure usage still counts toward the account budget.

Additional providers, including Ollama, require explicit configuration and verification. An Ollama instance on a powered-off Mac is unavailable; it needs a separate always-on host. Keep the existing tool boundary when adding providers.

## Add another provider through Railway CLI

Native ZeroClaw environment overrides can create a named provider entry without changing source. Use lowercase provider/alias names supported by the pinned runtime. For example, to configure an Anthropic slot named `custom` after linking the CLI to this project's production environment:

```sh
railway variable set --service heimdallclawd --skip-deploys \
  ZEROCLAW_providers__models__anthropic__custom__model=YOUR_MODEL_ID
read -r -s provider_key
printf '%s' "$provider_key" | railway variable set \
  ZEROCLAW_providers__models__anthropic__custom__api_key \
  --stdin --skip-deploys --service heimdallclawd
unset provider_key
railway redeploy --service heimdallclawd --yes
```

The silent `read` waits for you to paste the provider key and press Enter. Keep the key out of shell command history. Use that provider's exact model ID and API credential; do not literally use `YOUR_MODEL_ID`. The key remains an environment override rather than a TOML value. The configuration-library fixture verifies creation of a previously absent named provider and secret exclusion from saved config.

After deployment succeeds, send `/models anthropic.custom`, then `/model YOUR_MODEL_ID` to the bot as separate messages. `/models` changes only the provider, so the second command is required before asking a model question. `/model codex` returns to the subscription route in one command. New providers retain the same primary agent/tool boundary; verify an actual tool call after switching. API provider charges are separate from Railway's infrastructure budget. For an additional OpenRouter slot, replace `anthropic` with `openrouter`, keep a different alias, and supply its exact OpenRouter model ID. For Ollama, configure a reachable `uri` instead of an API key and verify private connectivity separately.

## Building

`deploy/bot/Dockerfile` builds everything from the checksum-pinned source on a native Linux x86_64 builder. For Railway's shorter build window, `deploy/bot/Dockerfile.artifact` packages the CI-tested prebuilt runtime and launcher executables without compiling on Railway. The expected executable hash is in `deploy/bot/runtime.sha256`, with build provenance in `deploy/bot/artifact.lock.json`. Executables are not stored in Git.

`deploy/bot/build-linux.sh OUTPUT_DIRECTORY` builds the Linux executable using Rust 1.99 and cargo-zigbuild. Install the Linux Rust target and the build tools first. The helper verifies the source archive and applies all maintained runtime patches. Review the new hash after rebuilding; never bypass the packaging checksum check. Place the reviewed executable in the upload context's `prebuilt/` directory and use the artifact Dockerfile as its root Dockerfile. Keep credentials, runtime state, Git data and build caches out of uploads.

## Skill installation

Git is installed in both agent image recipes. The agent has a dedicated stdio MCP server implemented by `heimdall-launcher --skills-mcp`, exposing only `skills__install` and `skills__list`. Ask Heimdall to install a named skill from the official ZeroClaw registry, or to list its installed skills. The model may also select these tools when a task needs a skill.

The installer accepts only bare lowercase registry names, not URLs, paths, command options or extra arguments. It invokes ZeroClaw's native install/audit implementation without a shell, forces the official registry, permits script-containing skills, disables bulk syncing only inside the short-lived installer subprocess, clears inherited provider/Telegram credentials, and bounds subprocess time and captured output. Git global configuration, hooks and local/ext protocols are disabled for the operation. This does not give the model a general shell on the agent container. The workspace executor remains network-disabled.

Skills are assigned through the primary agent's `managed` bundle and persist under `/data/zeroclaw/shared/skills/managed`. Bulk Open Skills synchronization is enabled separately; its persistent cache is `/data/zeroclaw/shared/open-skills`. Compact skill prompting and `read_skill` keep full skill instructions on demand. Installing a skill grants neither extra tools nor system packages. Registry audits are filters, not a guarantee of safe instructions; tasks still run under the configured tool policy.

Other registries and arbitrary source URLs are not exposed by this installer. Skill shell/script commands run through the remote executor. Bounded regular text files from the audited skill directory are copied into `.skills/TOOL_NAME` before execution; symlinks, oversized files and excessive trees are rejected. System dependencies must already exist in the executor. Verify a named skill's installation, visibility to the agent and behavior after deploying this version. CLI/stdio and configuration tests run locally; an actual registry installation through the deployed bot remains a live verification step.
