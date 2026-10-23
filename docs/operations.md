# Operations

HeimdallClawd uses pinned ZeroClaw 0.8.6 with a Rust launcher and a separate Rust workspace executor. The runtime revision and source archive checksum are in `deploy/bot/runtime.lock.json`. The deployment image applies the private-Telegram gate documented in `deploy/bot/NOTICE.md`.

## Railway services

| Service | Persistent mount | Port | Resource ceiling |
| --- | --- | --- | --- |
| heimdallclawd | `/data` (500 MB) | 3888 | 0.5 GB, 0.5 vCPU |
| heimdall-executor | `/data/workspace` (200 MB) | 3889 | 0.25 GB, 0.25 vCPU |

Keep one replica per service and sleeping disabled. Monitor costs and use an operator-selected budget; favor frugal infrastructure. Set `PORT` explicitly in Railway: its injected port takes precedence over Docker's environment default. The agent's public domain serves authenticated clients and signed email events; Telegram polling alone needs no public domain. The executor stays private and listens on IPv6 for Railway private networking.

## Variables

The bot requires `TELEGRAM_BOT_TOKEN`, `TELEGRAM_OWNER_ID`, and `HEIMDALL_EXECUTOR_TOKEN`. No Telegram owner ID is hardcoded; omitting it stops startup. Keep token values in Railway variables only. `TELEGRAM_ALLOWED_USER_IDS` accepts comma-separated numeric IDs with optional surrounding spaces; the owner is always included and duplicates are removed. A blank variable means owner only. Empty entries within a nonempty list, zero, negative and malformed entries fail startup. Only private chats from permitted accounts are accepted. All permitted accounts share the executor workspace; conversation keys include the sender identity.

The subscription model is selected in `[providers.models.openai.codex].model` in `deploy/bot/config.template.toml`; the launcher keeps the Codex route consistent with that selection. When changing it, update/remove its model-specific context capacity at the same time. The executor URL comes from the embedded `workspace` MCP server definition, not a separate environment setting. The transfer bridge validates its private origin and exact `/mcp` path; it does not read mutable deployed TOML. Set the executor's `EXECUTOR_TOKEN` to the same private value as the bot's executor token.

### Configuration ownership and variable cleanup

Non-secret shared behavior belongs in the template: `[providers.models.openai.codex].model` selects the subscription model; `[heimdall.agentmail].enabled` and `[[heimdall.agentmail.filters]]` configure email admission. The launcher removes its `heimdall` section before passing configuration to ZeroClaw. There is no environment fallback for these settings.

Keep `HEIMDALL_CODESPACE_REPOS` in Railway because it is deployment-specific access policy. Keep Telegram owner/allowed IDs, credentials, `PORT`, platform-provided variables and emergency recovery controls deployment-specific. An empty additional-user variable is optional and can be omitted without removing allowlist support. Native non-secret `ZEROCLAW_...` overrides should be used only for intentional deployment differences; otherwise edit their canonical template sections. Never move provider keys or paired-token digests into source.

After releasing the migrated image, remove the old ingress-enable and model-selection variables from Railway. The pre-migration image still relies on the ingress-enable variable: removing it before replacement can disable the email proxy on restart. Endpoint selection is now entirely the validated embedded workspace MCP URL. No new image or SOP migration has been deployed merely by changing this repository; follow [release procedure](releases.md) and [email migration](agentmail-events.md#required-sop-manifest-change) before removing rollout-critical variables.

## Authentication and state

Subscription credentials live in `/data/zeroclaw`, together with configuration and SQLite sessions. This directory belongs to UID 1000 and is private. A native auth check runs before Telegram polling. Launcher/startup messages go to Railway stdout. The normal daemon terminal formatter is off; inspect structured runtime diagnostics in the paired dashboard. Local diagnostics rotate at 1 MiB with three archives and three-day archive retention; tool/prompt payload capture remains disabled. There is no automatic API-key fallback.

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

Set `ZEROCLAW_web_search__tavily_api_key` privately on the agent service in Railway and restart/redeploy that service to load the variable. Source changes use the release workflow; setting a key for the existing integration does not require a source change. The launcher inherits this environment variable and ZeroClaw resolves it natively. Do not put the key in the configuration template or source. The integration test uses a dummy key, asserts its exact in-memory value, calls native `Config::save()`, and asserts the saved TOML contains no dummy key.

The native secret-resolution fixture is `deploy/tests/config-secrets.rs`; the source-build release workflow installs it into the checksum-pinned ZeroClaw configuration crate and runs it. Portable launcher tests check exact tool permissions and settings; the separate native fixture proves secret resolution and non-persistence. Neither test needs your actual key.

After deploying, ask the bot: “Use Tavily to search for the latest stable Rust release. Include source links and the release date.” Confirm a real tool call and response. This tests live credentials/API access, which dummy-key tests cannot establish. Keep Tavily pay-as-you-go disabled if you want usage to stop at the free allowance. Search queries are sent to Tavily; the API key is not a model-visible tool argument.

## Switching models in Telegram

The ChatGPT subscription model remains the default. Add or replace `ZEROCLAW_providers__models__openrouter__free__api_key` privately in the bot service's Railway Variables to enable the optional `openrouter/free` route. ZeroClaw reads the native environment override directly; it is not written into TOML. All API-backed provider credentials use the same `ZEROCLAW_providers__models__PROVIDER__ALIAS__api_key` convention.

Send `/model free` to select OpenRouter's free router for your conversation. Send `/model codex` to return to the configured subscription model. `/model` displays the current selection. Routes change the provider and model together, while retaining the primary agent, its conversation and the same permitted tools. These are session overrides; verify the current selection after a restart. No automatic paid-provider fallback is configured.

OpenRouter's free router chooses an available free model supporting the request's features, including tool calling. Its availability, context capacity and rate limits vary; see [the free-router guide](https://openrouter.ai/docs/guides/routing/routers/free-router) and [limits](https://openrouter.ai/docs/api_reference/limits). Selecting this route sends the conversation and tool results needed for the request to OpenRouter and its selected provider. Railway infrastructure usage still counts toward the account budget.

Additional providers, including Ollama, require explicit configuration and verification. An Ollama instance on a powered-off Mac is unavailable; it needs a separate always-on host. Keep the existing tool boundary when adding providers.

### Select another model on the OpenRouter slot

1. Use the exact OpenRouter model ID and confirm its tool support, price and account availability. The alias `free` does not make a different explicitly selected model free.
2. Send `/models openrouter.free`, then `/model EXACT_OPENROUTER_MODEL_ID` as separate Telegram messages. Provider selection alone does not set a compatible model.
3. Send `/model` to confirm the selection and test a harmless workspace read. `/model free` restores `openrouter/free`; `/model codex` restores the subscription route. Verify session selections again after a restart.

## Add another provider through Railway CLI

Native environment overrides can create a named provider entry without editing Rust. The pattern is `ZEROCLAW_providers__models__PROVIDER__ALIAS__FIELD`; use lowercase names and a provider family supported by the pinned runtime. `api_key`, `model` and, where required, `uri` are common fields. Unknown schema paths stop startup.

For example, configure an Anthropic slot named `custom`:

1. Sign in with `railway login` if needed and link the local CLI to the intended project/environment with `railway link`. This is interactive operator setup; the project-token release workflow does **not** run `railway link`.
2. Obtain the provider's API credential and exact supported model ID. A ChatGPT subscription does not supply another provider's API key.
3. Set the model, replacing the placeholder, then enter the key at a hidden prompt and pass it through stdin:

   ```sh
   railway variable set --service heimdallclawd --skip-deploys \
     ZEROCLAW_providers__models__anthropic__custom__model=YOUR_MODEL_ID
   printf 'Paste provider API key (hidden), then press Enter: '
   read -r -s provider_key
   printf '\n'
   printf '%s' "$provider_key" | railway variable set \
     ZEROCLAW_providers__models__anthropic__custom__api_key \
     --stdin --skip-deploys --service heimdallclawd
   unset provider_key
   ```

4. Redeploy the agent once to load both settings: `railway redeploy --service heimdallclawd --yes`. This affects the running service; perform it only as an authorized operator action.
5. After it is healthy, send `/models anthropic.custom`, `/model YOUR_MODEL_ID` and `/model` as separate messages. Verify a reply and an actual remote tool call. `/model codex` restores the subscription route.

Set these variables on the agent, never the executor. Keys are resolved in memory rather than saved in TOML. The configuration fixture verifies absent-provider creation and secret exclusion. Provider API usage is billed separately from Railway infrastructure.

For another OpenRouter slot, use the `openrouter` family with a different alias and exact model ID. For Ollama, configure a supported alias with a reachable `uri` and model; a server on a powered-off Mac is unavailable. Endpoint connectivity requires separate verification. None of these additions enables automatic fallback.

## Building

Build and release procedures, generated checksums/provenance and compile-cache boundaries are owned by [releases](releases.md). Executables and generated release provenance are not committed. Use the build-only workflow for native validation; Railway artifact images package already verified binaries.

## Skill installation

Git is installed in both agent image recipes. The agent has a dedicated stdio MCP server implemented by `heimdall-launcher --skills-mcp`, exposing only `skills__install` and `skills__list`. Ask Heimdall to install a named skill from the official ZeroClaw registry, or to list its installed skills. The model may also select these tools when a task needs a skill.

The installer accepts only bare lowercase registry names, not URLs, paths, command options or extra arguments. It invokes ZeroClaw's native install/audit implementation without a shell, forces the official registry, permits script-containing skills, disables bulk syncing only inside the short-lived installer subprocess, clears inherited provider/Telegram credentials, and bounds subprocess time and captured output. Git global configuration, hooks and local/ext protocols are disabled for the operation. This does not give the model a general shell on the agent container. The workspace executor remains network-disabled.

The registry installer targets the primary agent's `managed` bundle, persisted under `/data/zeroclaw/shared/skills/managed`. Additional operator-created native bundles can be attached with the procedure below. Bulk Open Skills synchronization is enabled separately; its persistent cache is `/data/zeroclaw/shared/open-skills`. Compact skill prompting and `read_skill` keep full skill instructions on demand. Installing a skill grants neither extra tools nor system packages. Registry audits are filters, not a guarantee of safe instructions; tasks still run under the configured tool policy.

Other registries and arbitrary source URLs are not exposed by this installer. Skill shell/script commands run through the remote executor. Bounded regular text files from the audited skill directory are copied into `.skills/TOOL_NAME` before execution; symlinks, oversized files and excessive trees are rejected. System dependencies must already exist in the executor. Verify a named skill's installation, visibility to the agent and behavior after deploying this version. CLI/stdio and configuration tests run locally; an actual registry installation through the deployed bot remains a live verification step.

### Operator-installed skill bundles

Use ZeroClaw's native CLI inside the bot container, with its existing `HOME` and `ZEROCLAW_CONFIG_DIR` set to `/data/zeroclaw`. Run native mutations as `heimdall` (UID/GID 1000), not root, so the runtime can continue to read its configuration and files. For example, from an operator container shell running as root:

```sh
gosu heimdall zeroclaw skills bundle add superpowers
gosu heimdall zeroclaw skills install https://github.com/obra/superpowers \
  --skill brainstorming --bundle superpowers
# Install any other selected skills before refreshing.
gosu heimdall heimdall-launcher --refresh-skills
gosu heimdall zeroclaw skills list --agent primary
```

Then use the paired dashboard's existing reload action (`POST /admin/reload`, with the current paired bearer and remote administration enabled). The refresh command updates the on-disk primary-agent bundle assignment; it does not contact the gateway or activate a running agent. Native reload rereads the configuration and recreates daemon subsystems; it is not an in-flight skill refresh. Serialize native CLI, dashboard configuration edits and refresh operations, and finish them before requesting reload. Do not put bearer tokens in URLs, command history or chat.

Every registered bundle is attached to `primary`: `managed` first, then other aliases alphabetically, matching native first-name-wins precedence. Extra aliases must be 1–64 lowercase ASCII letters, digits, hyphens or underscores. Native directory mappings and include/exclude filters are retained. `managed` remains template-owned at startup. No filesystem watcher or runtime patch is used, and unregistered directories are not attached. Native audit and tool grants continue to apply; installing a collection does not install its plugin hooks or enable delegation.

At startup the launcher imports only the extra `[skill_bundles.*]` definitions from the existing persistent `config.toml`, then rebuilds the primary assignment in the freshly rendered configuration. Thus bundles, custom directories and filters survive a restart or redeployment with the same `/data` volume, including when refresh was not run before restart. Other saved configuration remains managed by the template and environment. Malformed, oversized, nonregular or symlinked saved configuration fails startup or refresh with sanitized errors instead of being discarded. Native bundle removal is respected on the next refresh/reload or startup; remove registered bundles through the native CLI rather than deleting their directories. Volumes are not backups, and this feature does not survive volume loss.

No new Railway variables, tool grants or services are required. After deployment, verify a synthetic or selected audited skill with `skills list --agent primary`, authenticated reload and a new Telegram request. Runtime listing is covered by the build workflow; paired reload and Telegram use still require live verification.

## AgentMail API authentication

Set only `AGENTMAIL_AUTHORIZATION` in the **heimdallclawd** Railway service to the complete authorization value (replace the placeholder with your actual key):

```text
Bearer <your actual API key>
```

No separate `AGENTMAIL_API_KEY` variable is required. Deploy the variable change and the release containing this template. The launcher-generated configuration contains only:

```toml
[http_request.secrets]
agentmail = "${AGENTMAIL_AUTHORIZATION}"
```

ZeroClaw resolves the reference from its inherited Railway environment when `http_request` uses `auth_secret="agentmail"`. This tool expects the complete Authorization header value; it does not prepend `Bearer` or interpolate a prefix inside the reference. Do not replace the reference with the real key or commit the variable's value. The isolated executor does not need these variables.

For a read-only check, ask Heimdall to GET `https://api.agentmail.to/v0/inboxes` using that named auth secret and report success or a sanitized error. Use the secret only at `https://api.agentmail.to`. If the reference variable is absent, unrelated features can still start; AgentMail calls fail until it is configured. The template is regenerated at startup, so manual edits to the deployed TOML are not persistent.

## AgentMail email events

Optional signed email ingress uses the existing agent container and ZeroClaw's authenticated native SOP webhook endpoint. Rules start disabled and can be enabled/disabled through `mail__filters` and `mail__set_enabled`. See [email event setup, filters and recovery](agentmail-events.md) for Railway variables, webhook registration, the one public listener and delivery limits. This does not provision public access automatically.

## GitHub access

Use a dedicated GitHub account and keep its token only in the **heimdallclawd** Railway service. Never add it to the executor, Git, GitHub Actions variables or chat.

Set `GITHUB_TOKEN` to the raw personal access token, without `Bearer` or whitespace. This single variable serves both GitHub REST requests and Codespaces. The launcher validates it and derives `HEIMDALL_GITHUB_AUTHORIZATION=Bearer <token>` only in ZeroClaw's process environment. Do not configure that internal variable in Railway. The template stores only `github = "${HEIMDALL_GITHUB_AUTHORIZATION}"` in `[http_request.secrets]`; the resolved token is never written into TOML. Remove the older separate authorization/Codespaces token variables if present.

Ask Heimdall to call `https://api.github.com/user` with `auth_secret="github"`; the tool supplies the header without exposing the token in model arguments. The runtime enforces that secret only at `https://api.github.com:443`; AgentMail is bound to `https://api.agentmail.to:443`. Requests without named secrets retain general public HTTP access. Grant only the repository permissions needed for the requested operations; token permissions govern reads and writes.

For Codespaces, also set `HEIMDALL_CODESPACE_REPOS` to a comma-separated allowlist such as `octo/sandbox,octo/another-sandbox`. Entries are validated and normalized to lowercase. Use repositories belonging to the dedicated account and intended for remote execution. The `gh` subprocess receives the same raw `GITHUB_TOKEN` as `GH_TOKEN` in its otherwise cleared environment. A classic token with the `codespace` scope is supported by the GitHub CLI; private-repository work can need `repo` too. Fine-grained token support varies by endpoint. See [GitHub CLI Codespaces authentication](https://cli.github.com/manual/gh_codespace).

Do not run `gh auth login` or persist a token file. Missing token settings leave unrelated bot features available, but GitHub calls fail on use; a present malformed `GITHUB_TOKEN` stops startup. Missing Codespaces repository settings produce a tool error on use.

The launcher exposes `codespace__status`, `codespace__list`, `codespace__create`, `codespace__stop`, `codespace__delete` and `codespace__exec`. They have explicit primary-agent grants, are not available in pipelines, and do not enable host shell tools. Mutations and execution check the real account listing against the configured repository policy. Explicit repository lists remain supported; standalone `*` permits GitHub-authorized repositories. `list` reports the account's Codespaces. Creates use machine `basicLinux32gb`, 30-minute idle timeout and 24-hour stopped retention, with at most two account Codespaces allowed at creation time. The local call deadline is 80 seconds and each captured stream is bounded to 32 KiB. Killing a local SSH process does not guarantee a remote job stops; stop the Codespace after work, and use short remote commands. Output overflow fails with a bounded error rather than silently reporting a partial success.

Codespaces compute/storage and GitHub quotas are separate from Railway billing. Idle timeout and retention reduce usage; they are not a hard spending limit. Configure the dedicated GitHub account's budget before creating Codespaces. Never create, delete or execute in a Codespace without a user request. These instructions guide the model; the tool grants themselves permit the operations. Codespaces may expose their own repository credentials to remote commands, so use sandbox repositories and do not configure production secrets there.

Both agent Docker recipes install checksum-verified GitHub CLI and OpenSSH; the version is pinned in those Dockerfiles. No third Railway service is added and the isolated workspace executor remains credential-free and network-disabled.

After deployment, first test GitHub's read-only `/user` API and `codespace__list`. Create a sandbox only when requested, run a harmless remote command, and stop/delete it after verification. These operations have not been live-verified by local tests. Managed instruction files refresh at startup; see [instruction updates](#managed-agent-instructions).

### Codespaces error diagnostics

A nonzero GitHub control-plane CLI exit returns a JSON object in the MCP error text with `code=github_command_failed`, `category`, numeric-or-null `exit_code`, `classification=heuristic`, and a fixed `hint`. Control-plane stderr is inspected only for recognized patterns and never copied to error responses. Codespaces SSH commands instead return bounded, token-redacted untrusted stdout/stderr as structured result data, with CLI exit code and truncation flags; inspect succeeded before claiming execution succeeded. A nonzero CLI status can represent a remote command or connection failure. Categories distinguish authentication, rate limits, permission/policy rejection, missing/inaccessible resources, TLS, network, service availability and unknown failures. HTTP 403 alone cannot distinguish token permissions from a policy/network restriction, and HTTP 404 can hide private resources. Remote commands can emit misleading text, so these are clues rather than verified diagnoses. Unknown failures stay unknown; do not infer a permissions problem merely from a nonzero exit. Other validation, capture and timeout failures retain their short sanitized errors.

Agent-facing instructions explicitly name `github` as the HTTP secret alias. Managed `TOOLS.md` edits reach existing deployments at their next startup. No credential or raw CLI diagnostic needs to be shown to the model or operator to classify a failure. Inspect actual Codespaces state before retrying a mutation or remote command.

See [GitHub repository workflow](github-workflow.md) for API-first repository editing, offline Git, Codespaces policy and CI fallback.

## Managed agent instructions

At each startup the launcher synchronizes embedded `TOOLS.md` and `IDENTITY.md` into the primary agent control workspace. Replaced content is preserved in hidden `.<name>.saved-<unique suffix>` files with private permissions; identical files are not rewritten or backed up again. Saved copies are historical references, not active instructions. Personal preferences belong in `USER.md`, which the launcher never changes. Record the preferred IANA timezone there (for example, `Default timezone: <IANA timezone>`). For a reminder without an explicit timezone, the agent uses that preference and asks if it is unknown or ambiguous. Requests with an explicit timezone take precedence. This is agent guidance, not a scheduler-level default; existing jobs retain their configured schedules and timezones. Instruction reads reject symlinks, non-regular files and content larger than 128 KiB rather than overwriting it blindly. These saved copies are migration safeguards on the same volume, not independent backups.

The identity and tool guide distinguish ordinary isolated executor work from GitHub Codespaces work. Their files and execution environments are separate; choosing Codespaces never gives local Railway host execution access.

## Tool discovery and recovery

`codespace__status` reads local configuration without calling GitHub or starting a Codespace. It returns configured/missing/invalid token and repository configuration states, the validated repository allowlist, CLI availability and fixed resource/time/output limits. It never includes credentials and explicitly reports that authentication and network reachability have not been verified. Use it before choosing a repository; use `codespace__list` for actual remote state.

Executor admission waits up to three seconds outside the active-map lock, then returns `executor_busy` (-32001, no execution started) from `duplicate_request` (-32002, that request ID is active within the same MCP client session). The HTTP readiness gate remains 503 until isolation passes. Retry an unstarted busy request after current work completes; do not duplicate an active operation or fall back to host tools. Tool descriptions state the 30-second executor deadline, 64 KiB output ceiling, two-job capacity and fresh-process working-directory behavior.

Skill failures expose fixed heuristic categories for registry/network, missing skills, audit/policy and unknown failures; raw CLI failure output is withheld and output overflow is reported explicitly. Transfers expose sanitized categories for authentication, permission/policy, missing resources, duplicate/busy requests, rate limits, network/timeouts and service availability. A timeout does not prove that a delivery or write failed: inspect state before retrying. No raw HTTP response, token-bearing URL or service error description is returned.

`transfer__send` requires `recipient` explicitly, including for the owner. Use the intended requesting private-chat identity when known; ask only when ambiguous. The recipient must still be in the Telegram allowlist and the 48 KiB transfer ceiling is unchanged. Missing recipients fail before any transfer operation starts.

### GitHub Codespaces creation rejected with CS_NET_01

If GitHub reports HTTP 422 with `CS_NET_01` and says creation is blocked from the current network, the CLI diagnostic category is `network_policy`. A 422 without that marker remains unknown rather than being assumed to be a network rejection. Reproducing the same rejection in GitHub's web UI isolates it from the bot implementation. Check VPN/proxy settings and, where appropriate, test a normal alternative connection. If it persists, contact GitHub Support with the error code and time of the failed request; do not include tokens. Railway requests originate from Railway's network, so fixing browser-side connectivity does not establish that bot-side creation works. Do not rotate tokens, change resource limits or repeatedly retry creation as a workaround for this error. GitHub's public documentation does not explain the exact CS_NET_01 decision criteria.

## Heartbeat

The hourly worker runs as the primary agent and delivers only to the configured owner. ZeroClaw creates an empty `HEARTBEAT.md` in `/data/zeroclaw/agents/primary/workspace`. Add explicit task lines there through the Railway dashboard terminal, for example `- [low] Check the assigned inbox and notify me only about actionable new mail`. This is the agent control workspace, not the executor's `/data/workspace`; writing a same-named executor file does not configure heartbeat. Keep credentials out of instructions. Empty or paused tasks do not run. A fallback task can instead use the native `ZEROCLAW_heartbeat__message` variable. Use cron for exact reminder times. Heartbeat work consumes model/search quotas.

## Additional providers and private clients

The template selects Deepgram `nova-3` for voice/audio with automatic language detection, and Cohere `embed-v5.0-fast` for hybrid semantic memory. Add these **secrets to the agent service only**, as raw API keys without an Authorization prefix:

| Railway variable | Value |
| --- | --- |
| `ZEROCLAW_providers__transcription__deepgram__voice__api_key` | Your Deepgram API key |
| `ZEROCLAW_memory__embedding_api_key` | Your Cohere API key |

The template contains no keys; ZeroClaw resolves native environment overrides in memory. Saving runtime configuration must not persist these credentials. Do not add keys to the executor or GitHub variables. The primary agent's transcription alias is `deepgram.voice`; a global transcription key alone does not configure that alias.

Cohere uses `custom:https://api.cohere.ai/compatibility/v1`, which ZeroClaw resolves to `/embeddings`. The [Cohere compatibility API](https://docs.cohere.com/docs/compatibility-api) supports OpenAI-style embedding requests but does not support a dimensions parameter. Therefore configuration uses the model's [documented default of 2,048 dimensions](https://docs.cohere.com/docs/cohere-embed), rather than claiming a reduced vector size. Its 1,024-entry cache limits cached vector storage; it does not cap all memory/database growth. Keep model and dimensions together when changing providers; existing vectors may need reindexing.

Voice audio is sent to Deepgram, and memory/query text is sent to Cohere. Their quotas/billing are separate from ChatGPT subscriptions and Railway billing. Provider errors are not evidence of a working integration; after deployment, send a short voice message and verify its transcription, then store and recall a synthetic fact with different wording. Check provider usage to confirm embedding requests; keyword recall alone does not prove embeddings worked.

The embedded dashboard and ACP endpoint share the agent's HTTPS domain, targeting port 3888 with pairing required. Dashboard daemon reload and SOP administration require `gateway.allow_remote_admin=true` and a current paired bearer, enforced by the native gateway even behind the email adapter. Pair in the browser using a code obtained from the Railway console; `/pair/code` exposes status only. Shutdown and reading/generating pairing codes stay console-only. Deploy the configuration and runtime change before retrying a reload rejected by the old policy. Reload applies the runtime's current configuration; it does not deploy a new image or replace the launcher's startup rendering. Dashboard changes to managed settings can be overwritten on the next deployment; keep durable settings in the repository or Railway variables. ACP clients use `/acp` and a paired authorization header; never put tokens in URLs. A compatible client bridge runs on the editor side, not in the workspace executor. See [public networking ownership](../.railway/README.md).

WebAuthn activation needs `ZEROCLAW_webauthn__enabled`, `ZEROCLAW_webauthn__rp_id` and `ZEROCLAW_webauthn__rp_origin` matching the actual HTTPS client origin. Remote nodes need `ZEROCLAW_nodes__auth_token` as a Railway secret and a configured private client before enabling `ZEROCLAW_nodes__enabled`. Leave both inactive until those prerequisites are met.

Run the bundled synthetic harness in the dashboard terminal:

```sh
gosu heimdall zeroclaw eval run --suite /usr/share/heimdall/evals --mode replay
```

Live mode is not implemented upstream. Supply additional synthetic fixtures to a separate suite directory for broader replay coverage. Do not copy private conversations into the repository or release assets.


## Authentication and storage recovery

Normal startup remains fail-closed. Native ChatGPT refresh has a 60-second deadline; failure or timeout stops startup with a sanitized error, without printing OAuth diagnostics. Both services log read-only filesystem capacity at startup. These are snapshots, not ongoing monitoring or quotas. Railway volume alerts depend on the account's available features; none are automatically configured by this repository.

For an agent that cannot start because authentication is missing/revoked or `/data` is full:

1. In the agent service's Railway Variables, temporarily set `HEIMDALL_RECOVERY=1` and deploy that variable change. Only literal `1` enables recovery; absent or `0` selects normal startup, and other values fail validation. Recovery is never entered automatically.
2. Check for the explicit `HEIMDALL RECOVERY: bot OFFLINE` startup message. `/health` returns HTTP 200 with body `recovery`; this means the maintenance listener is running, **not** that Telegram, authentication or tools work. No ZeroClaw daemon or Telegram polling starts. Other paths return 503. Do not add public routes or publish releases while recovery is enabled; the maintenance listener does not serve the dashboard or email ingress.
3. Open the Railway dashboard terminal. Run `df -h /data` and `df -i /data` to check capacity and inodes. If needed, inspect top-level sizes locally with `du -x -h -d 1 /data`. Do not paste auth files, environment values or private file listings into chat/logs. Free space before attempting login. Recovery itself skips config/instruction writes and identity-directory initialization, so full persistent storage does not block this listener.
4. Remove only operator-confirmed disposable artifacts. Preserve `/data/zeroclaw/auth-profiles.json`, `.secret_key`, databases, user workspace files and volume attachments. Do not delete the whole state directory or database sidecar files. The volume is not a backup; arrange and verify backups separately. Prefer investigating growth while free space remains; 80% usage is an advisory point to act, not an enforced quota.
5. For authentication repair, run the device-login command from the setup section as `heimdall`. Complete sign-in without sharing credentials. Recovery assumes this existing deployment already has the state directory; it does not initialize a fresh installation. If the directory is absent or permissions are broken, repair them through the existing setup procedure before login.
6. Remove `HEIMDALL_RECOVERY` (or set it to `0`) and redeploy. Confirm normal authentication validation, service health and an actual Telegram reply. Recovery mode has no automatic expiry; leaving it enabled keeps the bot offline. A green Railway deployment alone is insufficient acceptance.

For full executor storage, keep its isolation readiness gate intact. The supervisor remains running but MCP/health are unready when the startup probe fails. Use the executor dashboard terminal to inspect `df -h /data/workspace`, `df -i /data/workspace` and `du -x -h -d 1 /data/workspace`; remove only confirmed disposable files. Clean up before redeploying, then verify startup isolation passes and a synthetic workspace read/write succeeds. Never copy credentials into the executor or enable host tools as a workaround.

Recovery acceptance requires a disposable Railway environment: dashboard-terminal access, repaired sign-in/storage, normal readiness and an actual Telegram reply after recovery is disabled. Local process/health tests alone do not establish those outcomes. See the dated [verification record](verification.md).


## Trusted Telegram users

Every allowlisted account receives the same primary-agent tool grants and auto-approval policy. Treat additional users as fully trusted operators: they can use configured GitHub/AgentMail credentials through the admitted tools, manage Codespaces and cron tasks, send messages, and access shared workspace data and permitted session/memory tools. The allowlist does not create separate user sandboxes or per-user credential scopes. Keep `TELEGRAM_OWNER_ID` and optional additional IDs limited to trusted accounts. Use a dedicated GitHub identity and the smallest token permissions that support the intended repositories and operations; this does not reduce the configured agent tool inventory.

## Executor admission and command results

Queued executor requests reserve their session-scoped ID immediately. Cancellation or response disconnect removes an unstarted reservation. Once admitted, the existing 30-second execution deadline applies; the three-second admission budget fits inside the client's 35-second tool timeout. A queue timeout is a JSON-RPC -32001 error in the HTTP 200 response stream, and explicit queued cancellation returns -32800. Duplicate IDs in the same session are still rejected before admission; no host fallback is introduced.


See [structured command results](../executor/README.md#structured-command-results) for `bash` exit status, termination reason and bounded stdout/stderr. A successful MCP transport is not proof that the command succeeded.

## Logging privacy

Terminal logging privacy: daemon startup omits `--verbose` because a pinned-runtime fixture showed verbose failed-MCP diagnostics could print private tool-result text. The production terminal formatter stays off; launcher/recovery/storage status messages remain available. The regression exercises actual Telegram and MCP failures with synthetic data through the native subscriber. It checks terminal output only, not every runtime path or persisted diagnostic record; existing payload-capture settings remain off. Returned tool errors still reach the agent for troubleshooting. Production-log inspection remains a separate post-deployment check.

## Native child-process isolation

The text browser runs through a launcher wrapper with an empty credential environment, an ephemeral HOME/TMPDIR, and a Linux Landlock filesystem policy. It can read system binaries, libraries, public configuration and its private scratch directory; it cannot read agent authentication files, conversation storage, SSH keys, or `/proc`.

The registry installer retains its existing cleared environment and official-registry restrictions. Landlock grants the rendered configuration, managed skill storage, and read-only skill/plugin metadata directories, while a private temporary registry cache avoids granting access to the agent workspace. Codespaces receives only its GitHub token and access to its own CLI state and scratch directory. Its intentionally authorized remote commands can still access credentials available inside the Codespace; this policy protects the Railway agent's other credentials.

Native descendants inherit filesystem restrictions and a seccomp denial of process-memory inspection. These profiles require Linux Landlock ABI 3 or newer and fail closed without it; there is no unrestricted fallback. Existing skill/GitHub timeout, output and process-group handling remains. The native text-browser transport still has its upstream redirect/DNS limitations; prefer `web_fetch` for untrusted URLs. Browser state is ephemeral rather than an authenticated persistent browsing profile.

After native validation, verify Landlock compatibility on Railway and exercise text browsing, skill listing/installing, and Codespaces before treating the change as live-verified. No production deployment is implied by local/CI checks.

The native OAuth refresh is a trusted control-plane operation: it still reads/writes the sign-in state needed to refresh the subscription and is not subjected to the browser/registry filesystem profile. Its environment is explicitly limited to PATH, HOME and ZEROCLAW_CONFIG_DIR, and its diagnostics remain discarded. The long-running agent/MCP servers retain the credentials required by their granted integrations.

These are credential-access profiles, not isolated tenants or a complete native-process sandbox. Landlock ABI 3 does not mediate every metadata operation, and native network clients retain their intended connectivity. A browser killed abruptly may leave its private scratch directory until container replacement; normal completion cleans it. No live-provider or Railway verification is claimed by the native tests.

## Upgrading to ZeroClaw v0.8.6

The runtime is pinned to the immutable v0.8.6 commit and archive checksum in `deploy/bot/runtime.lock.json`. This upgrade retains the existing provider variable names, subscription sign-in, persistent volume paths and executor protocol. No new secret or service is required. Take an operator-controlled backup of persistent state before publishing the upgrade; volumes are not backups. Do not copy authentication files or conversation databases into Git or build artifacts.

The upstream runtime now supplies typed Telegram transcription initialization and crate-local WASM interface paths. The current local patch purposes are documented in [NOTICE](../deploy/bot/NOTICE.md), and the exact zero-fuzz application order lives in [the Dockerfile](../deploy/bot/Dockerfile). They also cover SOP-authoring guidance, Responses history repair; do not treat the original upgrade patch count as current. `tools-external` and `tool-project-intel` are explicit build features because v0.8.6 no longer includes these adapters through `agent-runtime` alone.

`[tools].optional = ["*"]` preserves full built-in availability; it does not grant execution. The primary profile grants `tool_search` for deferred built-in schemas. Native memory reranking uses MMR and deduplication with a two-times candidate pool, without a new API call; previous documentation incorrectly described the available stage as unimplemented. MCP remains eager so script skills can find the connected workspace executor. The launcher still regenerates configuration at startup: dashboard, RPC or CLI config edits are not durable deployment configuration. Use Railway overrides or a reviewed template change for settings that must survive restart.

Before rollout, confirm existing memory, response-cache and audit SQLite files (including WAL/SHM companions) are regular, singly linked, owned by the service user and inaccessible to group/others. v0.8.6 rejects unsafe files. If startup reports this condition, enter recovery mode, stop database writers, back up state and correct the specific file's ownership/mode after checking it is not a symlink or hardlink. Do not recursively change permissions or delete databases to make startup pass.

After deployment, verify private text and voice messages, model switching, a GitHub read through the `github` alias, web search, text browsing, tool discovery, remote skill execution and workspace write/read/command tools. Check that group messages are rejected and the protected executor fixture remains unreadable. Review scheduled-task and heartbeat delivery after restart. None of these live checks is implied by a successful source build.

The embedded dashboard, ACP, WASM plugin host and WebAuthn build support remain included. New OIDC enrollment, A2A peers, additional providers/search backends, plugin packages and remote browser automation require operator-selected endpoints, credentials and grants. No public endpoint or implicit paid-provider fallback is introduced. Pairing codes are now opaque longer values and admin-gated; remote clients must authenticate and should match the daemon version. Installed WASM packages must target this release's WIT contract and be verified before use.

## Context budgets and long tasks

The default Codex profile declares `context_window = 475_000` under `[providers.models.openai.codex]` and a `max_context_tokens = 192_000` working budget under `[runtime_profiles.primary]`. Capacity and budget are distinct: the former bounds input for that selected model; the latter triggers proactive trimming of older complete turns. A large current turn, tool batch or system/tool prompt can still exceed a finite capacity after older turns have been removed. A larger budget permits more input and can increase latency and subscription usage; it does not guarantee completion.

Existing native `ZEROCLAW_...` Railway overrides take precedence over the template; update or remove an old context-budget override if it would mask these defaults.

The capacity is a conservative starting point for `gpt-6.1-sol`, derived from observed local Codex metadata (500,000 tokens with 95% usable), not live verification of the Railway subscription route. The [OpenAI API model page](https://developers.openai.com/api/docs/models/gpt-6.1-sol) describes API capacity; do not assume every subscription route exposes the same limit. Check the deployed model metadata before increasing it. When changing the template model, remove the old capacity or configure verified capacity for the new provider/model. Native route resolution also prevents a different selected model or OpenRouter route from borrowing Codex capacity. An unconfigured capacity remains unknown even if a runtime fallback uses the working budget.

`compact_context = false`, deferred built-in discovery, tool grants, iteration/action limits and the 70,000-character tool-result allowance remain unchanged. Do not set `max_context_tokens = 0` to fix overflow: it disables proactive budget trimming. The pinned runtime's `context_compression` settings are unsupported and do not enable summarization.

The managed [TOOLS guide](../deploy/bot/TOOLS.md#long-tasks-and-sop-authoring) describes bounded phases, workspace checkpoints, targeted reads and SOP proposal syntax. It is guidance, not automatic resumption or an enforced duplicate-review ledger. `/new` starts a fresh Telegram session; save a checkpoint first if the task must continue. Review-request email events dispatch their assigned SOP directly through `/sop/webhook/<sop-name>`, without a chat turn asking the model to start it. The installed SOP must declare that exact webhook trigger and execution ownership; a Manual trigger alone is insufficient. The default rule assigns `github-pr-review`; custom template rules need the optional `sop` field. The adapter reports admission; completion notification belongs in the SOP. See [rule assignments and failure diagnostics](agentmail-events.md#rules).

For local context-window errors, inspect sanitized diagnostics for `estimated_tokens`, `model_context_window`, `model_context_window_source`, `context_token_budget` and `provider_attempted`; a fallback/local rejection is not evidence that the provider rejected the request. Keep prompt/tool payload logging off. For rejected SOP proposals, correct the reported fields/Markdown, inspect the candidate and explicitly apply it; do not repeat malformed proposals or assume a rejected proposal was installed.

## Missing tool outputs in OpenAI/Codex history

`No tool output found for function call ...` indicates an incomplete Responses tool exchange, not proof of expired credentials or a model context limit. It can persist across turns when saved provider replay contains a pruned/rewritten call or a retained call has lost its result. `/new` is an immediate Telegram workaround that starts a fresh session; save any recoverable task state first.

The Responses adapter reconciles raw replay with canonical retained calls and inserts a fixed unknown-outcome error result for an unmatched call before the next user message or end of input. Real tool outputs and reasoning for retained calls are preserved. This is outbound request repair: it neither executes/retries tools nor rewrites the conversation database, and the unknown marker does not establish whether the original action ran. Inspect external state before retrying a potentially mutating operation. The same adapter serves OpenAI Responses and Codex, across chat and webhook entry points.

If OpenAI still emits its typed missing-output stream error, classify it as a non-retryable request-validation error rather than sending the identical request repeatedly. Other transient provider errors keep their existing retry policy. Diagnose remaining errors from the dashboard's provider retry/exhaustion event; Railway stdout may contain only the sanitized terminal summary. Keep tool/prompt payload capture off.
