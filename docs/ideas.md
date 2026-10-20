# Ideas

## Audit follow-ups (16 October 2026)

- Pin GitHub Actions to verified commit SHAs, add dependency advisories/updates, and design cheap PR checks separately from native release validation.
- Evaluate per-job process-tree memory/disk accounting under Railway constraints; RLIMIT_AS is per process, not a cgroup quota.
- Review supervisor capability dropping and PID-reuse-safe cleanup with Linux tests before changing process-group guarantees.
- Document/test an actual backup restore procedure and bounded volume monitoring after confirming the Railway plan's available backup features; favor frugal options within the operator’s current budget.
- Add explicit networking-ownership checks: keep the executor private while preserving the intentionally public, paired agent dashboard and signed email ingress. Review Codespaces devcontainer permission defaults without reducing owner-approved workflows.
