# Railway Python infrastructure

The project now uses `.railway/railway.py` with Railway CLI 5.68.0 and Python SDK 0.7.0. Python authoring is a Railway beta feature. See [Railway IaC](https://docs.railway.com/infrastructure-as-code) and [.railway/README.md](../.railway/README.md).

The October 15, 2026 migration was imported with `--variables unmanaged`, reviewed and applied to production. Legacy `railway.toml` files are removed, and release packaging no longer generates them. Both original services and volumes were preserved: `heimdall-data` at `/data` (500 MB), and `executor-workspace` at `/data/workspace` (200 MB). Replica counts, placement and CPU/memory limits are unchanged. Runtime variables are unmanaged; IaC neither reads nor writes them, including future provider keys or Telegram IDs.

The agent's GitHub source and legacy root-directory override were cleared. Both services receive application code only through release upload contexts. A final live `railway config plan` reported the configuration already up to date; `railway config migrate status` reported no services reading Config as Code. Normalized platform defaults are omitted from authoring to avoid perpetual diffs: ON_FAILURE is the default restart policy, and application sleeping defaults to false.

On each stable release, GitHub Actions installs the pinned Python SDK, links to the production environment using its project token, generates a pinned plan and applies it without a destructive confirmation flag. A destructive plan must stop rather than silently delete resources. Then the workflow uploads and health-checks the executor followed by the agent.

To inspect locally, activate a Python 3.10+ virtual environment with `.railway/requirements.txt` installed, link to production and run `railway config plan`. Do not pass `--include-variables`, `--decrypt-variables` or `--show-values` in routine checks. Keep local plan artifacts and state files out of Git.

Infrastructure changes do not speed Rust compilation. The separate cached build workflow handles binary compilation and validation.

The agent's generated HTTPS domain is managed outside the authoring graph; see [public networking ownership](../.railway/README.md). It does not require an IaC apply. The executor remains private.
