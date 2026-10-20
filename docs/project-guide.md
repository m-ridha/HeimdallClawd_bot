# Project walkthrough

## How the pieces connect

Telegram messages reach ZeroClaw inside the agent service. The Rust launcher prepares private configuration, checks authentication and starts ZeroClaw. Its stdio MCP entry points provide managed skills, Telegram transfers, Codespaces and email-rule management. ZeroClaw calls the selected model provider. For workspace file or command operations, ZeroClaw calls the separate executor over authenticated private MCP. The executor performs the operation in its own persistent workspace and returns the result.

Changing the model keeps the same agent and isolated tool boundary. Conversation state lives on the agent's `/data` volume. User tool files live on the executor's `/data/workspace` volume; neither belongs in Git. Optional `launcher/src/agentmail.rs` admits signed, filtered email events, supervises ZeroClaw behind one public listener and hosts the narrow email-rule management MCP server. See [email events](agentmail-events.md). The skills and Codespaces servers share bounded stdio framing in `launcher/src/mcp_stdio.rs`; their tool validation remains separate.

## Folders

| Folder | Purpose |
| --- | --- |
| `.github/workflows/` | Build-only validation and the published stable GitHub Release workflow: build/test, deploy the two services, then attach verified release assets. GitHub environment secrets/variables supply deployment credentials and IDs. |
| `deploy/bot/` | Docker build recipes, ZeroClaw configuration template, pinned source checksums and generated release checksums, maintained policy and compatibility patches (see [NOTICE](../deploy/bot/NOTICE.md)), and required upstream licenses. ZeroClaw source is downloaded and verified during a build; a full checkout is not maintained here. |
| `deploy/tests/` | Contracts against pinned ZeroClaw crates: configuration/secrets, Telegram dispatch, tool inventory, prompt assembly, MCP/skills, HTTP origins and native children. |
| `launcher/src/` | The small Rust startup wrapper: requires the Telegram owner ID, validates allowed users and integration token settings, writes non-secret config, checks ChatGPT auth, starts ZeroClaw, refreshes managed instructions while preserving old copies and USER.md, and exposes restricted skill install/list tools. It also exposes restricted Codespaces tools using the GitHub CLI; remote commands run in GitHub-authorized repositories admitted by the configured compute policy. It does not implement the language model engine. |
| `launcher/tests/` | Launcher/configuration checks, including root Linux initialization and rejection of missing owner/authentication. |
| `executor/src/` | The Rust private MCP service and Linux-restricted helpers implementing `bash`, `file_read`, `file_write`, `file_read_base64` and `file_write_base64`. |
| `executor/tests/` | Executor API, filesystem, timeout/cancellation, and Linux isolation checks. |
| `executor/contract/` | A test using ZeroClaw's real MCP client against the executor. |
| `.railway/` | Python IaC for both services and persistent volumes; pinned Python SDK. Runtime variables remain exclusively on Railway. |
| `tools/` | Release packaging/deployment scripts and the clean-tree verification script. |
| `tools/tests/` | Release-helper checks that prevent an unrelated successful deployment from masking a failed release deployment. |
| `docs/` | Operations, release setup, this walkthrough and observed verification results. |

`Cargo.toml` in each Rust crate lists its dependencies; `Cargo.lock` pins their versions. Generated `target/` directories contain local build outputs and are ignored by Git. `.git/` stores Git history and authorship metadata. `.gitignore` excludes credentials, database files and build outputs; `.dockerignore` excludes unnecessary/private paths from Docker uploads.

## Where to make changes

- Model selection and new provider credentials: [model switching](operations.md#switching-models-in-telegram) and [provider setup](operations.md#add-another-provider-through-railway-cli). These retain the same primary agent and tool boundary.
- Tool grants, integrations and activation prerequisites: [capabilities](capabilities.md) and the template's exact policy lists. Adding an MCP server alone grants nothing.
- GitHub API edits, offline tools, Codespaces and remote tests: [repository workflow](github-workflow.md).
- Signed email rules, SOP assignment and uncertain-delivery recovery: [AgentMail events](agentmail-events.md).
- Build caching, release packaging and deployment: [releases](releases.md). Infrastructure ownership and safe planning: [Railway IaC](railway-iac.md).
- Historical checks and their limits: [verification](verification.md). Unscheduled work belongs in [ideas](ideas.md).
