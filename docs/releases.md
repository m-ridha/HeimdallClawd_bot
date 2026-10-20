# Versioned releases

## Release policy

`Release to Railway` runs only when a stable GitHub Release is published. Tag pushes, drafts and prereleases do not deploy. Tags must be `vX.Y.Z` without suffixes, and the tagged commit must belong to `main`. Publishing is an operator-authorized action; never move a published tag to pick up a fix.

The deployment job uses the GitHub `production` environment:

| Setting | Type | Purpose |
| --- | --- | --- |
| `RAILWAY_TOKEN` | Secret | Project token scoped to the production environment |
| `RAILWAY_PROJECT_ID` | Variable | Project ID |
| `RAILWAY_ENVIRONMENT_ID` | Variable | Production environment ID |
| `RAILWAY_AGENT_SERVICE_ID` | Variable | Agent service ID |
| `RAILWAY_EXECUTOR_SERVICE_ID` | Variable | Executor service ID |

Runtime bot/provider keys stay in Railway, not GitHub or release assets. Account-level environment protection and private vulnerability reporting must be configured separately; repository documentation does not enable them.

## Publish a release

1. Open a PR and merge reviewed changes into `main` (use the repository's squash/semantic-subject convention).
2. On GitHub's **Releases** page, draft a new stable `vX.Y.Z` tag targeting that commit, add release notes, and publish when authorized.
3. Watch **Actions → Release to Railway**. The workflow builds and validates binaries, verifies token scope, plans/applies infrastructure, and checks package checksums.
4. The executor deploys first. Its exact deployment must report `SUCCESS` before the agent is uploaded and checked the same way.
5. Only after both succeed does the workflow attach the verified package and `SHA256SUMS` to the existing release. It does not create another release.

Two services cannot deploy atomically. If the bot fails after the executor succeeds, inspect compatibility and recover in Railway before rerunning. The GitHub Release remains published when a run fails. A rerun uses that release's original workflow/source commit; a source or workflow fix requires a new version. Volumes and unmanaged runtime variables are preserved; neither is a backup.

## Validate without deployment

Run **Actions → Build and verify release binaries → Run workflow**. Choose the workflow branch and set the separate source-ref input to the exact branch name or full 40-character commit SHA; use the SHA for reproducible evidence. The default `v0.0.0` release label is package metadata only and publishes/deploys nothing. Manual branch validation does not require main ancestry; published releases do.

The native Linux x86_64 build uses pinned Rust and Debian stages. It verifies the source archive, applies the maintained patches with zero fuzz, and compiles the runtime plus launcher/executor support binaries. Native validation covers launcher bootstrap, Telegram policy/transcription, Responses history and gateway behavior, real MCP clients, secret persistence, remote skill isolation, executor confinement and the bundled synthetic replay evaluation. Portable macOS tests do not establish Linux confinement.

For a local two-phase build, export the `runtime-binary` and `support-binaries` Buildx targets, combine their executables in ignored `prebuilt/`, then build `release-binaries`. Required contexts and fixtures are defined in [the bot Dockerfile](../deploy/bot/Dockerfile); do not replace validation with unverified compile artifacts.

## Faster builds

Compilation and runtime performance are different. The two compile targets run on separate runners and export successful GHCR caches before downstream validation. The expensive runtime layer depends on pinned source, patches and compiler settings rather than launcher/config files. Changes to those inputs can require a cold compile; configuration-only changes can reuse the compiled runtime.

The build uses repository-scoped `build-cache` registry tags for compiled-runtime, support and validated layers. Its generated GitHub token needs `packages: write` and package access; no new credential is required. Caches contain source/compiler artifacts, not Railway runtime credentials. Release compilation uses LTO off, 16 codegen units and optimization level 1; change these only with measured evidence. See [historical measurements](verification.md#native-validation-costs--15-october-2026), not old run times as a present performance guarantee.

### Native contract compilation cache

`native-test-compile` compiles contracts with `--no-run` before current executable/configuration inputs are copied. `runtime-build` receives those inputs and executes the matching tests; the workflow bypasses cache for that stage only. This preserves fresh execution while reusing expensive test compilation. Validation receives executables rather than release compiler layers and removes unused runner installations to limit disk pressure.

BuildKit exports binaries without loading the compiler image into Docker. Dashboard assets use Node 26 in a build stage; Node is not in the agent runtime. The deployment workflow uses Node 24 only to install the pinned Railway CLI. Railway artifact Dockerfiles verify prebuilt checksums rather than running Cargo. Preserve the WASM feature and native integration checks when optimizing build time.

## Project-token authentication

The release workflow checks token/project/environment scope, then runs IaC directly. Do not add `railway link`: CLI 5.68.0 queries account workspaces during linking, which a project token cannot authorize. IaC obtains its project/environment from `RAILWAY_TOKEN`; upload commands pass explicit IDs. An Unauthorized from linking does not prove expiration.

The preflight sends an explicit application User-Agent and exposes only sanitized status/category errors. HTTP 403 is not by itself evidence of token invalidity. See [IaC ownership and planning](railway-iac.md) for the infrastructure procedure and the prohibition on destructive automatic apply.

## Deployment log privacy

The workflow masks project/environment/service IDs before Railway commands; the deployment helper masks newly returned deployment IDs and logs the `agent`/`executor` label. Exact IDs remain internal for rollout verification. These protections affect new logs only. Historical logs, release assets, Git history and authorship require separate exposure review; ordinary release work does not authorize deleting evidence or rewriting history.

## Release input hygiene

`tools/release-files.txt` is the reviewed package manifest. Add required sources explicitly: unlisted local files are omitted, and missing files or symlinked source paths stop packaging. `tools/package-release.py` generates executable checksums and patch provenance inside each upload context; generated copies and executables are not committed. Docker contexts separately exclude credentials. Neither exclusion replaces secret scanning of the reviewed inputs.
