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

1. Push changes to a branch, open a pull request, and merge the reviewed changes into `main`.
2. Open the repository's **Releases** page and choose **Draft a new release**.
3. Create a new stable tag (for example, `v0.1.1`) targeting the updated `main`.
4. Add release notes and click **Publish release**.
5. Watch **Actions → Release to Railway**. The workflow builds, deploys both services and attaches assets after success.

Use a new version rather than moving a published tag. The earlier `v0.1.0` tag points to the original build attempt; publish a new version containing the fixes. A failed run can be rerun from Actions after diagnosing the failure, but a source-code fix needs a new release tag so it builds the corrected commit.

## Faster builds

The successful baseline run spent 21m36s in runtime build/testing, 38s packaging, and about two minutes deploying. Rust runtime speed does not imply fast compilation of dependencies and release optimization.

The release job now uses Docker Buildx with a repository-scoped GHCR registry cache. The runtime compiler layer depends on the pinned upstream source and patches, not the launcher/configuration files. A configuration-only release can reuse that compiled runtime. Registry caching works across distinct release tags; tag-scoped GitHub Actions caches alone would not provide that reuse. The build job needs `packages: write` and authenticates with its generated `github.token`; no additional secret is needed. GHCR package access policies must allow this repository's workflow to read/write its cache. The cache contains source/compiler artifacts, not Railway runtime credentials.

Release compilation uses LTO `off`, 16 codegen units and optimization level `1`, replacing upstream's fat LTO / one-codegen-unit / `z` settings. This trades some possible binary-size savings for shorter compilation; runtime memory and performance should be measured after deployment. The first expanded WASM build is still a cold build and may take longer than the baseline. Subsequent speed depends on cache hits; the first revised GitHub run compiled the runtime in 13m21s versus 18m32s in the earlier failed run (a single-run comparison, not a controlled benchmark).

BuildKit exports only the three executables rather than loading a compiler image into Docker. Both Railway upload contexts use checksum-verified prebuilt binaries, so Railway no longer compiles the launcher or repeats the upstream MCP client compilation for the executor. Native Linux tests stay in the release build; deployment still waits for the exact executor and agent deployments to become healthy. Only publishing a stable GitHub Release triggers deployment.

## Failed cold build and revised validation

The subsequent failed run spent 18m32s compiling the WASM-enabled runtime from scratch. GHCR reported its cache tag missing. Compilation succeeded; a launcher Linux bootstrap test then failed because `/usr/local/bin/zeroclaw` had not been populated before testing. The runtime was only present at `/runtime/target/release/zeroclaw`. Tests failed before BuildKit cache export, losing that successful compilation for future runs.

The fixed Docker stage installs the actual executable at the production path before Linux launcher tests. A local Linux x86_64 Docker check using the existing pinned executable passes the previously failing bootstrap test and the full launcher suite.

`build.yml` is a reusable build-only workflow. Its runtime and launcher/executor matrix jobs run on separate runners in parallel, each exporting a successful compilation cache immediately. Downstream native validation downloads executable artifacts and imports support/test caches; its failure no longer prevents the compiled runtime cache from being saved. The test suite keeps Telegram policy, real MCP client contracts, secret persistence, remote script isolation and native launcher checks. Remote-skill behavior now uses a small integration test instead of linking the runtime's entire multi-thousand-case unit-test harness into every release. LTO is disabled and optimization reduced to level 1; this trades potential binary/runtime efficiency for shorter cold builds. No change removes the production WASM feature.

Run **Build and verify release binaries → Run workflow** to validate main and warm caches without deployment. Its default v0.0.0 label is only test-package metadata and never publishes a release. Publishing a stable GitHub Release calls the same build workflow, then applies Python IaC and deploys. Compile work cannot be removed when runtime source/features/profile change; unchanged layers can be reused. Native validation remains a deployment gate, and its successful layers can also be reused.

The first revised build saved both compilation caches, but its validation runner exhausted disk space when release compiler layers and native test artifacts were combined. Validation now starts from the patched source stage and receives only executables from the compile jobs, avoiding the release object layers. It also removes unused Android, .NET and GHC installations from the disposable validation runner. Downloaded executable permissions are restored before tests.

Manual build-only runs accept branch refs for PR verification; published release tags still must belong to `main`. To reproduce the two-phase Docker build locally, export `runtime-binary` and `support-binaries` with Buildx local outputs, combine their executables in `prebuilt/`, then build `release-binaries`.

## Project-token authentication

The production `RAILWAY_TOKEN` secret must be a Railway project token scoped to the configured project and environment. The release workflow verifies this scope without logging the token or API response, then runs IaC directly. Do not add `railway link` to this flow: CLI 5.68.0 queries account workspaces during linking, which a project token cannot authorize. The IaC CLI resolves the project and environment from `RAILWAY_TOKEN` itself. Application uploads continue to pass explicit project, environment and service IDs.

An `Unauthorized` failure from `railway link` does not establish that the deployment token expired. If the separate token scope check fails, check the secret and its scope in Railway. Rerunning a published release uses its original workflow commit; merge workflow fixes and publish a new version to use them.

The token preflight sends an explicit application User-Agent. Railway rejects Python's default urllib User-Agent with HTTP 403 (observed without credentials); that transport rejection does not establish token validity. HTTP failures report only their numeric status, while network/JSON errors and GraphQL authentication errors have distinct sanitized messages.

## Native contract compilation cache

The validation stage is a release gate, not just binary copying. It checks the root Linux launcher/bootstrap, Telegram dispatch policy, actual ZeroClaw MCP clients against the isolated executor and stdio tools, environment-only configuration secrets, and remote skill dispatch before exporting verified executables for packaging. Removing the workflow step breaks the following packaging step because its inputs are produced there. Replacing it with unverified compile artifacts would drop these integration checks; portable tests and a successful optimized build do not establish those contracts.

The prior Docker ordering copied the launcher, executor and config before compiling contract tests. Changing those inputs therefore invalidated the expensive test build layer even when upstream source and contracts were unchanged. See [measured validation costs](verification.md#native-validation-costs--15-october-2026).

`native-test-compile` now compiles the same contracts with `--no-run` before any release executable or runtime configuration is copied. `runtime-build` then copies the current inputs and executes the same tests. The workflow bypasses cache only for `runtime-build`, so contracts execute freshly against current artifacts while the expensive compilation parent stays reusable. Existing `validated` GHCR cache exports/imports retain both stages; source, patches or test-fixture changes invalidate the compilation cache, while launcher/configuration-only changes can reuse it. No tests, WASM features, checksum gates or deployment checks are removed. The first build after the stage change can still be cold and expensive. The expected warm-cache speedup is not yet measured; native workflow verification is pending.

The extended-capabilities build adds embedded dashboard assets, WebAuthn and ACP channel support. Dashboard generation uses Node 24 only in a build stage; Node is not installed in the agent image. Its source and dependencies come from the checksum-pinned runtime and npm lockfile. The first build after changing these features is a cold compile; subsequent configuration-only builds retain the separate cached runtime target. Native validation also runs the bundled synthetic replay evaluation.

The native channels suite includes `heimdall_transcription`, a synthetic Telegram download/transcription/dispatch contract against a loopback typed provider. The typed Deepgram constructor/limit regression runs under the existing `--lib heimdall` filter. `typed-transcription.patch` is applied after the private Telegram patch and its digest is recorded in generated release provenance.
