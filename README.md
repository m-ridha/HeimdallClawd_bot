# HeimdallClawd

A private Telegram assistant running ZeroClaw 0.8.5 and Rust services on Railway. It uses ChatGPT subscription sign-in and three isolated remote workspace tools: command execution, file reads and file writes. Tavily supplies optional web search through ZeroClaw’s native search tool; configure its key privately in Railway.

Conversation history and workspace files persist on separate Railway volumes, so the bot works while your Mac is off. Only permitted accounts in private chats can use it. Set `TELEGRAM_ALLOWED_USER_IDS` to a comma-separated list to add users; the owner is always included. Permitted users share the tool workspace.

See [operations](docs/operations.md) for configuration, authentication, persistence, limits and builds, and [verification](docs/verification.md) for observed checks. The [executor](executor/README.md) documents the Linux tool boundary. [Versioned releases](docs/releases.md) describes the GitHub Actions build and Railway deployment workflow.

The current deployment uses `gpt-6.1-sol`. OpenRouter’s `openrouter/free` is an optional route: use `/model free` and `/model codex` to switch. Other API and Ollama providers require configuration and verification. The account's $5 hard spending limit remains in place.

For a folder-by-folder explanation and model setup instructions, see the [project walkthrough](docs/project-guide.md).

Heimdall can install named skills from the official registry through restricted `skills__install` and `skills__list` tools. Installed skills persist on the agent volume. Scheduling, SOPs/pipelines, public web access, script-containing skills and bounded Telegram file transfers are configured in the source; see [capabilities](docs/capabilities.md) for activation limits and live tests. Publishing a stable GitHub Release triggers deployment; pushing a tag alone does not.

Railway uses legacy Config as Code today; see [IaC migration notes](docs/railway-iac.md) for the documented December 1, 2026 cutoff and a migration approach that preserves secrets and volumes.
