# GitHub repository work

The GitHub token controls repository/API permissions. The Codespaces repository setting is a separate policy for remote compute, not a restriction on GitHub REST operations. No GitHub credential or network access is added to the workspace executor.

| Work | Available path |
| --- | --- |
| Read repository files/history, inspect issues/PRs | `http_request`, `auth_secret="github"`, HTTPS api.github.com |
| Create branches, file changes, multi-file commits and PRs | GitHub Contents/Git Database/Pulls APIs through the same HTTP tool |
| Local status/diff/branches/commits | `workspace__bash` with offline Git, Python 3 and jq, included in the updated executor image |
| Clone/fetch/push, install dependencies, run networked builds | GitHub Codespaces through existing `codespace__` tools, subject to GitHub access/billing/policy |
| CI build/test without Codespaces | An appropriate repository GitHub Actions workflow through the API, with explicit authorization for its effects |

The absence of Git/curl/wget/network in an older executor does not mean the bot cannot edit GitHub repositories. Root package installation belongs in maintained Docker recipes or a remote development environment, not in ordinary workspace commands. Offline workspace changes are not automatically uploaded; publish them through the GitHub API or a Codespace. Tests requiring uninstalled dependencies remain unverified until suitable remote compute/CI is available.

## Setup

Keep the existing raw `GITHUB_TOKEN` agent-service secret. The owner must give that account repository access, and give the token the necessary permissions. Successful metadata access does not prove write permissions. For fine-grained tokens, typical needs are Contents read/write, Pull requests read/write, Issues read/write when needed, and Actions read/write for dispatching workflows. Editing workflow files can additionally require Workflows permission. Codespaces has separate authorization requirements. Organization approval/SSO and branch protection can further constrain access; the bot cannot grant itself those permissions.

After deploying the new code, choose `HEIMDALL_CODESPACE_REPOS`:

- `owner/repository,other/repository` retains an explicit compute allowlist.
- A standalone `*` lets GitHub authorize repositories without that extra per-repository list. Mixed wildcard/list values are invalid. Repository syntax, authenticated account listing checks, token permissions and existing Codespaces resource limits remain enforced.

Do not change this variable to `*` on older code expecting only repository names. The current setting remains untouched by these local changes. GitHub API operations already cover any repository the token is authorized to access, regardless of this variable.

The primary runtime explicitly disables compact context so the complete managed TOOLS.md fits the native 20,000-character per-file bootstrap budget. A pinned-runtime prompt contract checks that the complete guide survives assembly with the production-effective settings. This increases prompt size compared with compact mode; no host file reader is granted.

The new managed TOOLS.md/IDENTITY.md instructions refresh at startup and explain API-first editing, offline Git, Codespaces and CI fallback. No external GitHub skill is required; CLI-oriented skill instructions must use a compatible remote environment instead of assuming a host shell exists.

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

Current REST operations authenticate with GITHUB_TOKEN. The Codespaces CLI uses HOME=/data/zeroclaw/codespace, not /data/zeroclaw, so it does not automatically select that key. The executor has neither the agent volume/key nor network access. No private key is copied into it. Adding an SSH key does not fix REST token permissions, API rate limits or Codespaces creation policy failures.

For a Codespace, check `git`, `gh`, `curl`, `jq`, `python3` and `ssh` before choosing commands. This project does not control other repositories' devcontainers or guarantee their installed tools. Configure/install required dependencies in that remote environment for the requested task; installing them in the executor does not enable networking there.
