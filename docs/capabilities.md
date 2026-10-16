# Enabled capabilities

These changes describe the next release, not a verified live deployment. Heartbeat is enabled for the owner; headless browser automation and browser delegation are not configured because Tavily provides search/content access rather than a WebDriver or browser-agent service.

| Capability | Configuration / use |
| --- | --- |
| Scheduled agent tasks | Scheduler enabled; cron_add/list/update/remove/run/runs permitted. Ask for a reminder or recurring report, specifying timezone. Use agent jobs with Telegram announce delivery; shell-only jobs remain unavailable. SQLite state is under the persistent ZeroClaw data directory. |
| Parallel tools | Enabled for independent actions. Executor accepts two concurrent jobs; scheduler and SOP concurrency are capped at one. Further simultaneous executor work can report busy; retry after the running work completes. |
| SOPs | Definitions persist under `/data/zeroclaw/shared/sops`; list/execute/advance/status/approve/workshop tools are allowed. Reusable procedures must be created before execution. |
| Pipelines | execute_pipeline enabled with twelve-step maximum and an explicit subtool allowlist. |
| Hooks | Lifecycle hook framework enabled. No custom hook handler or webhook endpoint is installed. |
| WASM plugins | Cranelift WASM runtime included, plugin loading/discovery enabled under `/data/zeroclaw/shared/plugins`. Maximum four logical instances, 32 MiB linear memory per store and ten-second calls. No plugin is bundled. Plugin-specific configuration, egress and exact tool grants are still required when installing a package; plugin limits do not cap total process memory. |
| Open Skills | Bulk community collection sync enabled in `/data/zeroclaw/shared/open-skills`. Compact prompting reads individual skill instructions with read_skill. First synchronization consumes disk/network and can slow startup. |
| Skill scripts | Script-containing skills allowed. Shell/script manifest tools copy up to 32 regular UTF-8 files, 64 KiB each / 256 KiB total, into the executor and run there. Symlinks are rejected. Operating-system dependencies and interpreter availability still matter; arbitrary downloaded packages are not installed automatically. |
| Web access | Tavily search, bounded web_fetch and http_request, Lynx text_browser, and automatic enrichment of up to two incoming links. Public hosts allowed; private-host access remains restricted. |
| GitHub | Optional environment-backed GitHub REST authentication and restricted Codespaces status/list/create/stop/delete/exec tools. Requires one owner-configured `GITHUB_TOKEN`; Codespaces supports an explicit repository list or standalone `*` for GitHub-authorized repositories; separate GitHub billing. See [setup](operations.md#github-access). Not live-verified. |
| Telegram messaging | Poll, reaction, channel_room, ask_user and send_via tools allowed. Actual operations depend on Telegram support, bot permissions and configured peers. No other chat channel is installed. |
| File transfer | transfer__send delivers executor files to an explicitly specified allowlisted Telegram recipient. transfer__import imports incoming files from the primary agent's telegram_files directory. transfer__receive accepts a Telegram file_id. Binary transfers are capped at 48 KiB; larger files are rejected. Native deliver_file is ACP-oriented, and native file_upload/file_download tools still need owner-configured endpoints. |
| Extra MCP servers | MCP remains enabled and supports owner-configured additional servers/bundles. No unknown external service is connected automatically. |

## Extended native capabilities (not live-verified)

The primary profile contains 55 exact policy grants; this is not the exposed Telegram inventory. Native deliver_file is ACP-only, endpoint-gated transfer tools are omitted without endpoints, and ACP excludes persistent memory tools. The additions below preserve the separate executor and the private gateway. Some supported features still require operator configuration; build support alone does not make an integration functional.

| Capability | Status and limits |
| --- | --- |
| Voice transcription | Enabled with Deepgram `nova-3`, automatic language detection and an 8 MiB / 120-second input limit. The primary agent selects `deepgram.voice`; its Railway API key is required. ChatGPT sign-in does not supply a transcription or embedding API key. |
| Heartbeat | Enabled for `primary`, hourly, two-phase, two-minute task timeout, 24 run records. Delivery is explicitly to `TELEGRAM_OWNER_ID`. Empty task instructions do no work. See [operations](operations.md#heartbeat). |
| Automatic media processing | Enabled for images and audio. Audio requires the Deepgram key; visual understanding depends on the selected model. Video summarization remains off because this revision documents it as a placeholder. |
| Image inspection / screenshots | `image_info` is granted with native workspace path guards for agent-side images. Executor files must be transferred first. Native screenshot is not granted: it starts an agent-container shell and needs a desktop. Remote capture requires an external service. |
| Session management | `sessions_current`, `sessions_list`, `sessions_history`, `sessions_send` granted. sessions_send appends a user message to history only; it does not notify Telegram or start a turn. Use send_via for delivery. These inspect the shared bot session store; permitted accounts should be trusted to share that visibility. Reset/delete tools are not registered by this runtime's tool factory. |
| Knowledge graph / bundles | Persistent graph enabled at `/data/zeroclaw/data/knowledge.db`, capped at 10,000 nodes, with `knowledge` granted. Named bundles exist in the schema but no bundle ingestion implementation was found in this pinned runtime; use graph capture instead. |
| Semantic memory | Hybrid retrieval configured with Cohere `embed-v5.0-fast` through its OpenAI-compatible endpoint, 2,048 dimensions and a 1,024-entry embedding cache. The separate Railway key is required; ChatGPT sign-in is not used for embeddings. |
| Memory enhancements | SQLite fact extraction and hygiene snapshots enabled. Reranking remains off: the pinned runtime reports its stage as unimplemented. Bounded response cache: 128 disk / 16 hot entries, 15-minute TTL. Upstream bypasses response caching when tools are available. Memory store/recall/forget tools granted. Snapshots on the same volume are not backups. |
| Project intelligence | `project_intel` and `report_template` granted. Analyze supplied Git/issue summaries and produce reports; they do not automatically connect a project or tracker. |
| Cloud / security | `cloud_ops`, `cloud_patterns`, `security_ops` granted. Advisory analysis, vulnerability parsing and playbook guidance; no cloud credentials or automatic infrastructure remediation added. Approval gates retained. |
| ACP / editors | ACP channel build support included, primary agent selected by default, its existing MCP bundles enabled for ACP, maximum four sessions / 30-minute idle timeout. Gateway pairing remains required. Client and private connectivity still need setup. |
| Embedded dashboard | Dashboard assets built from the pinned source and bundled via `embedded-web`. No public domain or remote-admin access is added. |
| Remote nodes | Configuration prepared for two nodes; disabled until a shared authentication token and remote client are configured. LAN mDNS stays off on Railway. No tunnel service is installed. |
| Diagnostics | Rotating diagnostics: 1 MiB rotation threshold, three archives, three-day archive retention. Tool I/O and LLM payload capture remain off, per owner preference. The threshold is a rotation trigger, not a strict per-event/disk quota. |
| WebAuthn | Compiled support included; inactive until the actual HTTPS relying-party domain/origin and client access are configured. Existing subscription sign-in remains the model authentication. |
| Evaluation harness | Bundled synthetic replay fixture, run during native validation. It tests harness wiring with a deterministic echo tool, not live model quality or the production executor. This runtime does not implement live evaluation mode. |

## Additional MCP servers and plugins

Use Railway native environment overrides for endpoint credentials; never commit them. A new MCP server needs its transport and URL/command configuration, a named MCP bundle listing that server, the bundle appended to `agents.primary.mcp_bundles`, and its exact prefixed tool names appended to `risk_profiles.primary.allowed_tools` and `auto_approve` when desired. Keep the existing workspace, skills and transfer bundles and tool entries. Registering a server alone does not grant its tools.

WASM plugin packages similarly need their exported tool names granted explicitly. Use the pinned ZeroClaw plugin CLI in the Railway dashboard terminal for package installation; configure only required host capabilities and network destinations. Enabling the framework does not install a package or a browser CLI.

Do not edit `/data/zeroclaw/config.toml` expecting changes to survive startup: the launcher regenerates it. Use the maintained template or native `ZEROCLAW_...` environment overrides.

## Live verification after publishing a release

1. Ask for a reminder in two minutes, delivered to your Telegram conversation; list it, wait for delivery and inspect its run history. Then create and cancel a second reminder. Confirm timezones for recurring jobs.
2. Fetch a public URL and run a Tavily search. Ask for text navigation on a simple public website. Confirm actual tool results.
3. Ask for two independent file reads concurrently, then repeat the protected fixture check. Confirm both reads succeed and `/run/heimdall-control/probe` still returns Permission denied.
4. Ask Heimdall to send the existing small `zeroclaw-ready.txt` as a Telegram document. Upload a small binary file and ask it to import it into the executor. Verify byte-for-byte contents with a checksum.
5. Install a suitable small registry skill, read its instructions, and run a compatible script tool. Verify the command runs in the executor and that missing dependencies are reported accurately.
6. Create a simple SOP and a short permitted pipeline. Verify state persists after a restart. Install and test a specific WASM plugin separately before depending on it.

Scheduled work, proactive messages and automatic link enrichment consume provider/search quotas. The Railway hard spending cap remains separate from those quotas.

GitHub repository edits do not depend on Codespaces. See the [repository workflow](github-workflow.md) for API commits/PRs, updated offline Git support and build/test paths.
