# Railway infrastructure operations

Infrastructure is authored in [`.railway/railway.py`](../.railway/railway.py) using the pinned SDK and CLI. [.railway/README.md](../.railway/README.md) describes resource and networking ownership; [release deployment](releases.md) owns the apply/upload sequence.

## Inspect a plan

1. Create a Python 3.10+ virtual environment and install `.railway/requirements.txt`.
2. For interactive account-based use, sign in/link the local CLI to the intended project and environment. The project-token release workflow does **not** use `railway link`.
3. Run `PATH="/path/to/venv/bin:$PATH" railway config plan` from the repository. Treat it as a read-only check; a clean plan is evidence only for that environment at that time.
4. Do not pass `--include-variables`, `--decrypt-variables` or `--show-values` in routine checks. Keep plan/state artifacts out of Git.

## Apply policy

The published stable-release workflow verifies the project token, generates a plan with `--file .railway/railway.py`, then applies that exact plan without a destructive confirmation flag. The agent upload has an exact-deployment health check. Do not apply or redeploy production outside an explicitly authorized task. A destructive plan must stop for review; never detach/shrink a volume or delete a service to make a plan pass.

Keep variables unmanaged, repository sources disconnected, one replica per service, sleeping disabled, and existing placement, resource limits and volume attachments. `tools/tests/test_iac.py` pins these settings. Generated agent domains remain platform-managed; the retired service must be migrated before its removal. Infrastructure changes do not speed Rust compilation.

The original migration and observed production checks are historical evidence in [verification](verification.md#failed-build-correction-and-python-iac-migration), not proof that the current environment has no drift.

See [single-service migration](single-service-migration.md) before publishing the first single-service release.
