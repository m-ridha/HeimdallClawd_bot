# Versioned releases

The `Release to Railway` workflow runs when a stable GitHub Release is published (`release: published`). Pushing a tag by itself does not deploy. Draft and prerelease releases do not deploy. The release tag must use `vX.Y.Z`, and its commit must belong to `main`.

The build job uses a native Linux x86_64 Rust 1.99 / Debian Bookworm container. It verifies the pinned ZeroClaw archive, applies the maintained Telegram, remote-skill and WASM interface-path patches, and runs the native Telegram, MCP client, configuration and executor checks. It packages the resulting runtime, launcher and executor binaries after testing, records its actual checksum and source provenance, and verifies the deployable bot image. CI-generated executable hashes can differ from the documented local cross-build; packaging checks the checksum recorded for that exact build.

The deployment job uses the GitHub `production` environment. Configure:

| Setting | Type | Value |
| --- | --- | --- |
| `RAILWAY_TOKEN` | Secret | Railway project token for the production environment |
| `RAILWAY_PROJECT_ID` | Variable | Your Railway project ID |
| `RAILWAY_ENVIRONMENT_ID` | Variable | Your Railway production environment ID |
| `RAILWAY_AGENT_SERVICE_ID` | Variable | Your agent service ID (agent) |
| `RAILWAY_EXECUTOR_SERVICE_ID` | Variable | Your executor service ID (executor) |

Runtime bot/provider keys stay in Railway. Do not copy them to GitHub or release assets. The workflow deploys the executor first and waits for its exact deployment to report `SUCCESS`; then it deploys and waits for the bot. A failed deployment stops the workflow and prevents asset upload; the triggering GitHub Release remains published. Existing volumes, runtime variables and spending limits are preserved. Two services cannot be deployed atomically: if the bot fails after the executor succeeds, inspect compatibility and recover in Railway before rerunning.

After both deployments succeed, the final job attaches the bot package and `SHA256SUMS` to the already-published GitHub Release. It does not create a second release. The package contains source/configuration and the Linux runtime executable, with no authentication state. The runtime remains Rust; Node is used only to install the Railway CLI in the deployment job. GitHub-hosted builds use the repository's Actions allowance.

For a release:

1. Commit and push the final changes to `main`.
2. Open the repository's **Releases** page and choose **Draft a new release**.
3. Create a new stable tag (for example, `v0.1.1`) targeting the updated `main`.
4. Add release notes and click **Publish release**.
5. Watch **Actions → Release to Railway**. The workflow builds, deploys both services and attaches assets after success.

Use a new version rather than moving a published tag. The earlier `v0.1.0` tag points to the original build attempt; publish a new version containing the fixes. A failed run can be rerun from Actions after diagnosing the failure, but a source-code fix needs a new release tag so it builds the corrected commit.

## Faster builds

The successful baseline run spent 21m36s in runtime build/testing, 38s packaging, and about two minutes deploying. Rust runtime speed does not imply fast compilation of dependencies and release optimization.

The release job now uses Docker Buildx with a repository-scoped GHCR registry cache. The runtime compiler layer depends on the pinned upstream source and patches, not the launcher/configuration files. A configuration-only release can reuse that compiled runtime. Registry caching works across distinct release tags; tag-scoped GitHub Actions caches alone would not provide that reuse. The build job needs `packages: write` and authenticates with its generated `github.token`; no additional secret is needed. GHCR package access policies must allow this repository's workflow to read/write its cache. The cache contains source/compiler artifacts, not Railway runtime credentials.

Release compilation uses thin LTO, 16 codegen units and size optimization `s`, replacing upstream's fat LTO / one-codegen-unit / `z` settings. This trades some possible binary-size savings for shorter compilation; runtime memory and performance should be measured after deployment. The first expanded WASM build is still a cold build and may take longer than the baseline. Subsequent speed depends on cache hits; no speedup has yet been measured on GitHub.

BuildKit exports only the three executables rather than loading a compiler image into Docker. Both Railway upload contexts use checksum-verified prebuilt binaries, so Railway no longer compiles the launcher or repeats the upstream MCP client compilation for the executor. Native Linux tests stay in the release build; deployment still waits for the exact executor and agent deployments to become healthy. Only publishing a stable GitHub Release triggers deployment.
