# Ideas

## Audit follow-ups (16 October 2026)

- Confine native browser/Git subprocess environments and credential-file access, preserving provider overrides and skill functionality; clearing only the parent environment does not solve this boundary.
- Pin GitHub Actions to verified commit SHAs, add dependency advisories/updates, and design cheap PR checks separately from native release validation.
- Evaluate per-job process-tree memory/disk accounting under Railway constraints; RLIMIT_AS is per process, not a cgroup quota.
- Review supervisor capability dropping and PID-reuse-safe cleanup with Linux tests before changing process-group guarantees.
- Document/test an actual backup restore procedure and bounded volume monitoring after confirming the Railway plan's available backup features; favor frugal options within the operator’s current budget.
- Assert private gateway exposure in IaC and review Codespaces devcontainer permission defaults without reducing owner-approved sandbox workflows.
