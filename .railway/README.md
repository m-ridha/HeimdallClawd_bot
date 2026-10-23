# Railway Python infrastructure

The single authoring file is `railway.py`. The service name, volume names/mounts, regions, replica counts, resource limits, health checks and restart retry settings are versioned here. Application code is uploaded only by the stable-release workflow; the repository source are disconnected to prevent direct GitHub push deployment.

Service variables are **unmanaged**. IaC does not read, create, change or delete them. Keep Telegram tokens/IDs, API keys and future model variables exclusively in Railway. The agent volume stays attached; retired storage is retained until migration is verified.

The agent's generated Railway HTTPS domain is platform-managed and targets container port **3888**. Generated `*.up.railway.app` domains are intentionally omitted from IaC; the SDK's `domains` field manages custom domains instead. If adopting a custom agent domain later, specify its target port explicitly as 3888 (the SDK otherwise defaults to 8080). See [Railway's domain reference](https://docs.railway.com/infrastructure-as-code/reference#custom-domains) and [AgentMail activation](../docs/agentmail-events.md).

Use Python 3.10+ and Railway CLI 5.68.0:

```sh
python3 -m venv /tmp/heimdall-iac
/tmp/heimdall-iac/bin/pip install -r .railway/requirements.txt
PATH="/tmp/heimdall-iac/bin:$PATH" railway config plan
```

For an interactive account CLI, link only the intended checkout/project/environment first; verify the target before planning. Do not relink the operator’s other checkout. Review each plan. The project-token release workflow does not use railway link. No destructive flag is part of our workflow: service deletion, volume detachment/shrinkage or other destructive changes must stop for explicit review. Runtime variable values and local plan/state files must never be committed.

Publishing a stable GitHub Release installs the pinned SDK, plans/applies this infrastructure and then deploys the checksum-verified agent. Pushing main alone does not deploy. The runtime remains Rust; Python is only the infrastructure authoring tool.

See [single-service migration](../docs/single-service-migration.md) for the one-time destructive service removal, retained volume and Trial resource limits.
