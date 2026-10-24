# GitHub repository work

The main service supports direct Git/gh and networked development. The raw
GITHUB_TOKEN controls repository permissions. Codespaces remains optional remote
compute; its repository list does not restrict REST calls or native Git.

Use shell for clone/fetch/edit/test/commit/push on a feature branch, then gh pr
create. Configure HTTPS Git using gh auth setup-git; never put credentials in
remote URLs or run gh auth token. Rust/Cargo/rustfmt/Clippy, Bash, Python 3,
curl, jq, Node/npm and SSH are installed. Root commands use sudo -n. System
package changes are ephemeral; maintain durable dependencies in Dockerfiles.
Heavy builds can exceed Trial RAM/disk even with root access.

## Setup

Keep the existing raw `GITHUB_TOKEN` agent-service secret. The owner must give that account repository access, and give the token the necessary permissions. Successful metadata access does not prove write permissions. For fine-grained tokens, typical needs are Contents read/write, Pull requests read/write, Issues read/write when needed, and Actions read/write for dispatching workflows. Editing workflow files can additionally require Workflows permission. Codespaces has separate authorization requirements. Organization approval/SSO and branch protection can further constrain access; the bot cannot grant itself those permissions.

Choose `HEIMDALL_CODESPACE_REPOS`:

- `owner/repository,other/repository` retains an explicit compute allowlist.
- A standalone `*` lets GitHub authorize repositories without that extra per-repository list. Mixed wildcard/list values are invalid. Repository syntax, authenticated account listing checks, token permissions and existing Codespaces resource limits remain enforced.

GitHub API operations cover any repository the token is authorized to access, regardless of this variable.

The primary runtime explicitly disables compact context so the complete managed TOOLS.md fits the native 20,000-character per-file bootstrap budget. A pinned-runtime prompt contract checks that the complete guide survives assembly with the production-effective settings. This increases prompt size compared with compact mode; deferred tool discovery limits schema overhead while native file tools remain available.

Managed TOOLS.md/IDENTITY.md instructions refresh at startup and explain native Git/gh, optional API editing, Codespaces and CI fallback. No external GitHub skill is required; CLI-oriented instructions can use the installed main-service toolkit.

## Diagnose before mutating

Ask Heimdall to use the github secret alias for read-only `GET /user`, `GET /rate_limit` and `GET /repos/{owner}/{repository}`, then inspect the requested branch/files. Report HTTP statuses and relevant rate-limit/reset/retry headers without credentials. Check `codespace__status`, then `codespace__list` if remote compute is needed. A configuration-presence check is not authentication verification. HTTP 401, 403/429 and 404 require different investigation; 403 alone does not establish a rate-limit issue. Do not rotate tokens or retry mutations blindly.

Verify repository writes only when requested: create a feature branch, make a small synthetic change, open a PR, and report the returned commit/PR URL. Prefer PRs to default-branch changes. Never claim a build/test passed without actual execution results. Do not use a deployment/release workflow as a substitute for a test workflow.

## API references

- [Git database workflow](https://docs.github.com/en/rest/guides/using-the-rest-api-to-interact-with-your-git-database): read base commit/tree, create changed blobs/tree, create a commit and update the feature ref without force.
- [Repository contents](https://docs.github.com/en/rest/repos/contents): small file reads/updates and initializing empty repositories.
- [Git references](https://docs.github.com/en/rest/git/refs): feature branch creation and non-forced reference updates.
- [Actions workflows](https://docs.github.com/en/rest/actions/workflows): list workflows and dispatch configured inputs when permitted.

Respect API/tool response and file limits. A truncated recursive tree is not a complete checkout; fetch smaller directories or use remote compute. Hosted archives/log downloads can redirect off api.github.com; never send the named GitHub secret to another host to follow a redirect.

## Existing SSH key on the agent volume

An owner-installed `/data/zeroclaw/.ssh/id_ed25519` persists while the same `/data` volume remains attached; redeployment alone does not erase it. The volume is not a backup. The private key must remain private and readable only by the intended runtime user. Registering its public key with GitHub and configuring SSH selection are separate prerequisites for direct SSH Git transport.

## Diagnosing interrupted PR creation

Send JSON request bodies as serialized strings with `Content-Type: application/json`. For multi-file text edits, use inline `content` entries in one Git Trees request with `base_tree` to reduce round trips. Verify remote state before retrying a mutation after an ambiguous failure or resuming an interrupted turn.

HTTP diagnostics persist only a response status or local category (invalid body, timeout, connection, transport); they omit URLs, payloads and credentials. They do not cover failures in runtime wrappers before HTTP execution. Rate-limit and retry header values are exposed to the agent; other response-header values are redacted. The agent should immediately report a sanitized failure summary to the requesting user, including method/path without query parameters and status/category, without copying private API payloads. Historical task-level failure notices cannot recover an omitted underlying response. Payload tracing stays disabled.
