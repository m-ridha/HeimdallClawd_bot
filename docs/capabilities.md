# Enabled capabilities

This describes checked-in configuration, not a live verification claim. The main
service runs native tools with internet and passwordless sudo root access.

| Capability | Configuration and prerequisites |
| --- | --- |
| Shell, filesystem and Git | Native shell/read/write/edit/search/Git; full autonomy and command admission, 300-second shell timeout. Root can access credentials and change the bot. |
| Development toolkit | Final image includes gh, Git, Bash, Python 3, curl, jq, Node/npm, SSH, ripgrep, GCC and pinned Rust/Cargo/rustfmt/Clippy. Dependencies can be downloaded. Large builds may exceed Trial resources. |
| Scheduling | Agent cron tools plus native shell schedule; Telegram delivery requires explicit recipient and a known timezone. |
| Delegation | Native spawn_subagent/delegate granted; reachable targets and recursion limits remain upstream policy. Work shares credentials/resources. |
| Parallel tools, pipelines, SOPs | Enabled; pipeline remains explicitly scoped and capped at twelve steps. SOP definitions/approval/state persist. |
| Skills and hooks | Native skill scripts, registry install/list, Open Skills sync and lifecycle hooks. Files and registered bundle definitions persist on the same /data volume; startup restores assignments. Refresh plus reload is for new bundles without restarting. See [skill persistence](operations.md#operator-installed-skill-bundles). |
| WASM plugins | Cranelift host enabled with discovery and bounded plugin calls. No plugin bundled; packages need exact tool grants and their own configuration. |
| Web and browser | Tavily search, fetch, HTTP, Lynx and native local Chromium automation. ChromeDriver is loopback-only; no remote browser account required. Public URL restrictions remain on web tools; shell networking is broader. |
| Media | image_info; Deepgram voice with existing key; media preprocessing. No desktop screenshot environment; browser screenshots use browser actions. Upstream video summarization is a placeholder. |
| Messaging | Telegram polls/reactions/notifications and small bounded transfers. Room/delivery/upload tools depend on the active channel or endpoint. No new messaging account is configured. |
| Memory, sessions, knowledge | SQLite, Cohere semantic retrieval with existing key, graph capture, fact extraction, snapshots, MMR and bounded cache. Session send appends history rather than delivering a notification. |
| Local utilities | Calculator, weather, canvas, TodoWrite, llm_task, model switch/routing, proxy configuration and peer messaging tools granted. Runtime/provider and peer prerequisites still apply. |
| Backup and data management | Backup tool retains at most two archives of selected data directories; no automatic schedule. Retention tool is preview-only upstream. Same-volume archives are not independent backups. |
| Project/cloud/security reports | Enabled advisory tools and report templates; analysis does not establish external remediation. |
| GitHub | Native Git/gh using GITHUB_TOKEN, exact-origin github HTTP alias and optional Codespaces compute. GitHub permissions/billing remain separate. |
| Email | Existing signed AgentMail sender/content admission and native SOP dispatch; rule toggles and safe diagnostics. No automatic replay. |
| Dashboard and ACP | Embedded paired public dashboard, remote reload policy and ACP support. Compatible clients require setup. |
| Diagnostics and evaluation | Redacted rotating diagnostics; tool/prompt payload capture off. Synthetic replay harness wired in CI; not model-quality evidence. |

## Configurable native integrations

Remote nodes, mDNS, tunnels, WebAuthn enrollment, A2A peers, additional chat
channels, vendor coding CLIs, additional model/search providers and hardware
require actual endpoints, clients, credentials or devices. They are not activated
by an all-tools selector. No automatic paid-provider fallback is configured.
Browser delegation requires a separate browser-agent CLI; local browser automation
is available instead. OIDC and hardware cannot be made useful by enabling flags
without prerequisites. See the [patch rationale](../deploy/bot/NOTICE.md).

## Additional MCP servers and plugins

The retained MCP bundles are skills, transfer, Codespaces and mail. New servers
need transport configuration, an attached bundle and exact prefixed grants in
allowed_tools/auto_approve; pipeline access is separate. Registration grants
nothing. Keep secrets in Railway native environment overrides, never TOML.

## Live verification after publishing a release

1. Verify versions for gh, Python, jq, curl, Node/npm, Cargo, rustc, rustfmt and
   cargo clippy; run sudo -n id -u and confirm root access.
2. Clone an authorized test repository, edit/test on a feature branch and create
   a PR only when instructed; verify actual results and remote state.
3. Exercise native local file operations, a public download, a browser page and
   screenshot. Confirm ChromeDriver has no public Railway port.
4. Round-trip a small Telegram file and verify its checksum/recipient.
5. Test an installed native skill, a scheduled reminder, heartbeat, SOP and
   pipeline; confirm state after restart and that group messages are rejected.
6. Check signed email admission and diagnostics without replaying old events.

Monitor resource use: tool availability does not mean every Rust build or browser
workload fits 1 GB RAM, 1 GB ephemeral storage or 500 MB persistent storage.

## Boundaries and further reading

- [Execution migration](single-service-migration.md)
- [GitHub workflow](github-workflow.md)
- [Trusted users](operations.md#trusted-telegram-users)
- [Recovery](operations.md#authentication-and-storage-recovery)
- [Narrow subprocess profiles](operations.md#narrow-subprocess-profiles)
- [Runtime upgrade](operations.md#upgrading-to-zeroclaw-v086)
- [Context budgets](operations.md#context-budgets-and-long-tasks)
