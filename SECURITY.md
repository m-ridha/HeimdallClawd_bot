# Security policy

HeimdallClawd is a private Telegram assistant intended for an operator-controlled deployment. Public source availability does not make its bot, gateway, or credentials public services. Security fixes target the current `main` branch; older releases are not maintained independently.

## Reporting a vulnerability

Use this repository's **Security → Report a vulnerability** option when private vulnerability reporting is enabled. Include the affected revision, prerequisites, boundary crossed, and a minimal reproduction using synthetic data. Do not include real credentials, private conversations, or workspace files, and do not open a public issue containing exploit details. If that private reporting option is unavailable, do not post sensitive details publicly; wait until a private reporting channel is available.

## Supported boundaries

- Telegram access is restricted to configured accounts in private chats. Additional allowed accounts share a privileged assistant and are trusted operators, not isolated tenants.
- The agent holds provider and integration credentials. A separate, network-disabled executor runs file and command jobs without those credentials and applies filesystem, syscall, identity and resource restrictions.
- Native host shell/file/Git tools remain excluded. Executor failure must not enable a host execution fallback.
- Named HTTP credentials are bound to approved HTTPS origins. HTTP/fetch tools enforce destination restrictions; the native text browser has the redirect/DNS limitations documented in operations.
- Logs must not capture credentials, private message bodies, or tool payloads. Error diagnostics must remain bounded and sanitized.

See [capabilities](docs/capabilities.md), [executor boundaries](executor/README.md), and [operations](docs/operations.md) for the exact supported configuration and limits.

## Limitations

External web content, repository files, skills, and tool output can contain prompt injection. Instructions alone are not a security boundary. An agent authorized to use external APIs can perform consequential actions with its configured privileges. Use appropriately scoped credentials and review repository/devcontainer trust before running remote code.

Executor limits are not aggregate container quotas; persistent volumes are not backups. Diagnostic redaction and secret scanners cannot guarantee detection of every encoded secret. Known limitations and unverified behavior remain documented in [native child-process isolation](docs/operations.md#native-child-process-isolation) and [verification](docs/verification.md).

Before making a deployment repository public, inspect reachable Git history, commit metadata, Actions logs/artifacts, release assets, and package visibility. `.gitignore` does not remove already tracked material. If a real credential is exposed, revoke or rotate it; deleting a file or rewriting history alone is insufficient.
