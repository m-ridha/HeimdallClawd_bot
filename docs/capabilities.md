# Enabled capabilities

These changes describe the next release, not a verified live deployment. Heartbeat is still disabled; headless browser automation and browser delegation are not configured because Tavily provides search/content access rather than a WebDriver or browser-agent service.

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
| Telegram messaging | Poll, reaction, channel_room, ask_user and send_via tools allowed. Actual operations depend on Telegram support, bot permissions and configured peers. No other chat channel is installed. |
| File transfer | transfer__send delivers executor files to allowlisted Telegram users. transfer__import imports incoming files from the primary agent's telegram_files directory. transfer__receive accepts a Telegram file_id. Binary transfers are capped at 48 KiB; larger files are rejected. Native deliver_file is ACP-oriented, and native file_upload/file_download tools still need owner-configured endpoints. |
| Extra MCP servers | MCP remains enabled and supports owner-configured additional servers/bundles. No unknown external service is connected automatically. |

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
