# Railway infrastructure configuration

The project currently uses legacy Railway **Config as Code**: `deploy/bot/railway.toml`, `executor/railway.toml`, and a root `railway.toml` generated in the release upload context. They specify Docker builds, health checks and restart behavior. Services, volume attachments, resource limits and secrets are currently managed in Railway.

Railway's newer [Infrastructure as Code](https://docs.railway.com/infrastructure-as-code) manages a whole project/environment through `.railway/railway.ts` (generally available), or Python/Go authoring (beta). As documented on October 15, 2026, legacy Config as Code is deprecated and stops being read on December 1, 2026. This project has not migrated yet.

IaC would let us version the two services, persistent volume mounts, replica/resource settings and secret references together. `preserve()` leaves existing secret and personal-ID values on Railway. IaC preview can show drift before applying. This does not speed up Rust compilation; build caching and prebuilt release packages address that separately.

A migration should start with a read-only import/preview using a compatible Railway CLI:

```sh
railway config pull
railway config plan
```

Do not pass `--include-variables`: the default import uses preserve references rather than writing secret or personal variable values into source. Inspect the generated file before committing it. Import alone does not migrate live service ownership: Railway blocks plans that try to manage a service still governed by Config as Code.

Follow Railway's documented migration procedure to clear legacy config ownership, translate health/restart/Docker settings and preview again. Preserve both existing volumes and all runtime variables. Review a plan with no unexpected resource deletion, variable deletion, volume detachment or resize before applying. Keep the service source omitted when IaC should manage settings without enabling GitHub push deployment. Application deployments should continue to require a published stable release.

The live migration is a separate operation: no `.railway/` authoring file has been applied and no existing service or volume was changed as part of the capability/build update.
