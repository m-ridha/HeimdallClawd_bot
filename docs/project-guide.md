# Project walkthrough

## How the pieces connect

Telegram messages reach ZeroClaw inside the agent service. The Rust launcher prepares private configuration, checks authentication and starts ZeroClaw. Its stdio MCP entry points provide managed skills, Telegram transfers, Codespaces and email-rule management. ZeroClaw calls the selected model provider. Workspace operations run natively in the main service with internet and sudo root access.

Changing the model keeps the same agent and root-capable execution model. Conversation state lives on the agent's `/data` volume. User tool files live in `/data/zeroclaw/agents/primary/workspace`; neither belongs in Git. Optional `launcher/src/agentmail.rs` admits signed, filtered email events, supervises ZeroClaw behind one public listener and hosts the narrow email-rule management MCP server. See [email events](agentmail-events.md). All four stdio MCP servers share bounded framing in `launcher/src/mcp_stdio.rs`; each retains its own request limits and tool validation.

## Folders

| Folder | Purpose |
| --- | --- |
| `.github/workflows/` | Build-only validation and the published stable GitHub Release workflow: build/test, deploy the main service, then attach verified release assets. GitHub environment secrets/variables supply deployment credentials and IDs. |
| `deploy/bot/` | Build-only Docker targets and the sole production image recipe (Dockerfile.artifact), ZeroClaw configuration template, pinned source checksums and generated release checksums, maintained policy and compatibility patches (see [NOTICE](../deploy/bot/NOTICE.md)), and required upstream licenses. ZeroClaw source is downloaded and verified during a build; a full checkout is not maintained here. |
| `deploy/tests/` | Contracts against pinned ZeroClaw crates: configuration/secrets, Telegram dispatch, tool inventory, prompt assembly, MCP/skills, HTTP origins and browser-output cleanup. |
| `launcher/src/` | The small Rust startup wrapper: requires the Telegram owner ID, validates allowed users and integration token settings, writes non-secret config, checks ChatGPT auth, starts ZeroClaw, refreshes managed instructions while preserving old copies and USER.md, and exposes restricted skill install/list tools. It also exposes restricted Codespaces tools using the GitHub CLI; remote commands run in GitHub-authorized repositories admitted by the configured compute policy. It does not implement the language model engine. |
| `launcher/tests/` | Launcher/configuration checks, including root Linux initialization and rejection of missing owner/authentication. |
| `.railway/` | Python IaC for the main service and retained persistent volumes; pinned Python SDK. Runtime variables remain exclusively on Railway. |
| `tools/` | Release packaging/deployment scripts and the clean-tree verification script. |
| `tools/tests/` | Release-helper checks that prevent an unrelated successful deployment from masking a failed release deployment. |
| `docs/` | Operations, release setup, this walkthrough and observed verification results. |

`launcher/Cargo.toml` declares launcher dependencies; its Cargo.lock pins their versions. The upstream runtime has its own checksum-verified source and lockfile. Generated `target/` directories contain local build outputs and are ignored by Git. `.git/` stores Git history and authorship metadata. `.gitignore` excludes credentials, database files and build outputs; `.dockerignore` excludes unnecessary/private paths from Docker uploads.

## Where to make changes

- Model selection and new provider credentials: [model switching](operations.md#switching-models-in-telegram) and [provider setup](operations.md#add-another-provider-through-railway-cli). These retain the same primary agent and tool boundary.
- Tool grants, integrations and activation prerequisites: [capabilities](capabilities.md) and the template's exact policy lists. Adding an MCP server alone grants nothing.
- Native Git/gh, optional API edits, Codespaces and remote tests: [repository workflow](github-workflow.md).
- Signed email rules, SOP assignment and uncertain-delivery recovery: [AgentMail events](agentmail-events.md).
- Build caching, release packaging and deployment: [releases](releases.md). Infrastructure ownership and safe planning: [Railway IaC](railway-iac.md).
- Historical checks and their limits: [verification](verification.md). Unscheduled work belongs in [ideas](ideas.md).

## Documentation ownership

| Document | Owns |
| --- | --- |
| README.md | Product, prerequisites and first-use path |
| AGENTS.md | Contributor standards, checks, paired change requirements and authorization rules |
| deploy/bot/TOOLS.md and IDENTITY.md | Managed instructions actually embedded into the deployed assistant |
| operations.md | Provider/auth setup, skill persistence, recovery and operator runbooks |
| capabilities.md | Tool catalog and genuine activation prerequisites |
| agentmail-events.md / webhook-architecture.md | Email setup/recovery and the admission-versus-execution contract |
| releases.md / railway-iac.md | Build, release, infrastructure and rollout procedure |
| single-service-migration.md | One-time workspace preservation and executor retirement |
| deploy/bot/NOTICE.md | Runtime licensing, pin and reasons for retained patches |
| verification.md / ideas.md | Concise observed evidence/limits and deferred work |

Update the owner and cross-link from other guides. Keep historical observations
separate from current instructions; do not use a successful old run as fresh
validation. Do not add private handoffs or completed implementation plans as
contributor prerequisites. Source and tests resolve implementation details; a
document or skill alone neither installs a binary nor grants a capability.
