# Versioned releases

The `Release to Railway` workflow runs when a stable `vX.Y.Z` tag is pushed. It can also be rerun manually with an existing tag. The tag's commit must belong to `main`.

The build job uses a native Linux x86_64 Rust 1.99 / Debian Bookworm container. It verifies the pinned ZeroClaw archive, applies the maintained patch, and runs the native Telegram, MCP client, configuration and executor checks. It packages the resulting runtime with the launcher, records its actual checksum and source provenance, and verifies the deployable bot image. CI-generated executable hashes can differ from the documented local cross-build; packaging checks the checksum recorded for that exact build.

The deployment job uses the GitHub `production` environment. Configure:

| Setting | Type | Value |
| --- | --- | --- |
| `RAILWAY_TOKEN` | Secret | Railway project token for the production environment |
| `RAILWAY_PROJECT_ID` | Variable | Your Railway project ID |
| `RAILWAY_ENVIRONMENT_ID` | Variable | Your Railway production environment ID |
| `RAILWAY_AGENT_SERVICE_ID` | Variable | Your agent service ID (agent) |
| `RAILWAY_EXECUTOR_SERVICE_ID` | Variable | Your executor service ID (executor) |

Runtime bot/provider keys stay in Railway. Do not copy them to GitHub or release assets. The workflow deploys the executor first and waits for its exact deployment to report `SUCCESS`; then it deploys and waits for the bot. A failed deployment stops publication. Existing volumes, runtime variables and spending limits are preserved. Two services cannot be deployed atomically: if the bot fails after the executor succeeds, inspect compatibility and recover in Railway before rerunning.

After both deployments succeed, the final job publishes the GitHub release with the bot package and `SHA256SUMS`. The package contains source/configuration and the Linux runtime executable, with no authentication state. The runtime remains Rust; Node is used only to install the Railway CLI in the deployment job. GitHub-hosted builds use the repository's Actions allowance.

For a future release, commit the final changes to `main`, then:

```sh
git tag -a v0.2.0 -m 'HeimdallClawd v0.2.0'
git push origin main
git push origin v0.2.0
```

Use a new version rather than moving a published tag. The initial local tag is `v0.1.0`; its remote push is intentionally left to the project owner along with the main-branch history replacement. Push the updated `main` before the tag so the workflow can verify ancestry. The workflow itself has been checked locally but its first GitHub-hosted execution awaits that push.
