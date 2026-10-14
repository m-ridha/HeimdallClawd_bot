# Railway Python infrastructure

The single authoring file is `railway.py`. Both service names, volume names/mounts, regions, replica counts, resource limits, health checks and restart retry settings are versioned here. Application code is uploaded only by the stable-release workflow; both repository sources are disconnected to prevent direct GitHub push deployment.

Service variables are **unmanaged**. IaC does not read, create, change or delete them. Keep Telegram tokens/IDs, API keys, executor credentials and future model variables exclusively in Railway. The existing two volumes remain attached with their original names, sizes and paths.

Use Python 3.10+ and Railway CLI 5.68.0:

```sh
python3 -m venv /tmp/heimdall-iac
/tmp/heimdall-iac/bin/pip install -r .railway/requirements.txt
PATH="/tmp/heimdall-iac/bin:$PATH" railway config plan
PATH="/tmp/heimdall-iac/bin:$PATH" railway config apply --yes
```

Link to the intended project and production environment first. Review each plan. No destructive flag is part of our workflow: service deletion, volume detachment/shrinkage or other destructive changes must stop for explicit review. Runtime variable values and local plan/state files must never be committed.

Publishing a stable GitHub Release installs the pinned SDK, plans/applies this infrastructure and then deploys the checksum-verified executor followed by the agent. Pushing main alone does not deploy. The runtime remains Rust; Python is only the infrastructure authoring tool.
