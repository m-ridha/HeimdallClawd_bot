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

Runtime bot/provider keys stay in Railway, not GitHub or release assets. Account-level environment protection and private vulnerability reporting must be configured separately; repository documentation does not enable them.

## Publish a release

1. Open a PR and merge reviewed changes into `main` (use the repository's squash/semantic-subject convention).
2. On GitHub's **Releases** page, draft a new stable `vX.Y.Z` tag targeting that commit, add release notes, and publish when authorized.
3. Watch **Actions → Release to Railway**. The workflow builds and validates binaries, verifies token scope, plans/applies infrastructure, and checks package checksums.
4. The agent deploys; its exact deployment must report `SUCCESS`.
5. Only after it succeeds does the workflow attach the verified package and `SHA256SUMS` to the existing release. It does not create another release.

The GitHub Release remains published when a run fails. A rerun uses that release's original workflow/source commit; a source or workflow fix requires a new version. Volumes and unmanaged runtime variables are preserved; neither is a backup.

## Validate without deployment

Run **Actions → Build and verify release binaries → Run workflow**. Choose the workflow branch and set the separate source-ref input to the exact branch name or full 40-character commit SHA; use the SHA for reproducible evidence. The default `v0.0.0` release label is package metadata only and publishes/deploys nothing. Manual branch validation does not require main ancestry; published releases do.

The native Linux x86_64 build verifies source/checksums, applies retained patches
with zero fuzz and compiles runtime and launcher binaries. Contracts cover native
configuration/tool grants, Telegram, auth, MCP integrations, HTTP origins,
Responses history and SOP behavior. The final image checks installed development
binaries and sudo access. Portable macOS tests do not prove native Linux behavior.

For a local two-phase build, export the `runtime-binary` and `support-binaries` Buildx targets, combine their executables in ignored `prebuilt/`, then build `release-binaries`. Required contexts and fixtures are defined in [the bot Dockerfile](../deploy/bot/Dockerfile); do not replace validation with unverified compile artifacts.

## Inspect a build run

For CLI dispatch, use the [contributor command](../AGENTS.md#run-github-actions-from-the-command-line).
After an authorized dispatch, use its returned URL. If no URL was returned, match
branch, actor and dispatch time before selecting a run; do not blindly rerun.

```sh
gh run list --repo m-ridha/HeimdallClawd_bot --workflow build.yml \
  --event workflow_dispatch --branch "$workflow_ref" --limit 5 \
  --json databaseId,createdAt,headBranch,headSha,status,conclusion,url
gh run view RUN_ID --repo m-ridha/HeimdallClawd_bot \
  --json status,conclusion,url,jobs
gh run view --job JOB_ID --repo m-ridha/HeimdallClawd_bot --log
```

Use the workflow branch selected at dispatch for `$workflow_ref` and replace
RUN_ID/JOB_ID with verified IDs. Event headSha may identify the workflow-definition
commit; check source checkout evidence for the separate source-ref input.
Report source SHA, URL and actual conclusion. A run for an older commit does not
validate unpushed local changes; queued or running is not passed.

## Faster builds

Compilation and runtime performance are different. The two compile targets run on separate runners and export successful GHCR caches before downstream validation. The expensive runtime layer depends on pinned source, patches and compiler settings rather than launcher/config files. Changes to those inputs can require a cold compile; configuration-only changes can reuse the compiled runtime.

The build uses repository-scoped `build-cache` registry tags for compiled-runtime, support and validated layers. Its generated GitHub token needs `packages: write` and package access; no new credential is required. Caches contain source/compiler artifacts, not Railway runtime credentials. Release compilation uses LTO off, 16 codegen units and optimization level 1; change these only with measured evidence. See [historical measurements](verification.md#native-validation-costs--15-october-2026), not old run times as a present performance guarantee.

### Native contract compilation cache

`native-test-compile` compiles contracts with `--no-run` before current executable/configuration inputs are copied. `runtime-build` receives those inputs and executes the matching tests; the workflow bypasses cache for that stage only. This preserves fresh execution while reusing expensive test compilation. Validation receives executables rather than release compiler layers and removes unused runner installations to limit disk pressure.

BuildKit exports binaries without loading the compiler image into Docker. Dashboard assets and the main-image Node/npm toolkit use Node 26. The daemon itself remains Rust. The deployment workflow uses Node 24 only to install the pinned Railway CLI. The sole production image recipe, Dockerfile.artifact, verifies prebuilt checksums rather than compiling the runtime or launcher. Preserve the WASM feature and native integration checks when optimizing build time.

## Project-token authentication

The release workflow checks token/project/environment scope, then runs IaC directly. Do not add `railway link`: CLI 5.68.0 queries account workspaces during linking, which a project token cannot authorize. IaC obtains its project/environment from `RAILWAY_TOKEN`; upload commands pass explicit IDs. An Unauthorized from linking does not prove expiration.

The preflight sends an explicit application User-Agent and exposes only sanitized status/category errors. HTTP 403 is not by itself evidence of token invalidity. See [IaC ownership and planning](railway-iac.md) for the infrastructure procedure and the prohibition on destructive automatic apply.

## Deployment log privacy

The workflow masks project/environment/service IDs before Railway commands; the deployment helper masks newly returned deployment IDs and logs the `agent` label. Exact IDs remain internal for rollout verification. These protections affect new logs only. Historical logs, release assets, Git history and authorship require separate exposure review; ordinary release work does not authorize deleting evidence or rewriting history.

## Release input hygiene

`tools/release-files.txt` lists runtime-only upload inputs: the image recipe,
entrypoint, evaluation fixture, licenses/notice and runtime pin. Prebuilt binaries,
checksums and provenance are added by `tools/package-release.py`. The launcher
embeds its configuration/instructions, so their source files, Rust sources/tests
and upstream patches are not uploaded to Railway. They remain in the tagged
repository; patch hashes remain in provenance. The build Dockerfile contains only
compilation/validation/export targets; Dockerfile.artifact is the sole image recipe.

Unlisted local files are omitted. Missing files and symlinks stop packaging,
including provenance-only patch inputs. Docker context exclusions independently
protect credentials; neither exclusion replaces input secret scanning. Compilers,
Clippy/rustfmt, standard libraries and rustdoc are retained, while installed Rust
HTML/man reference pages are omitted from the image.

## Build-time and runtime privilege checks

Railway image-build steps can set `no_new_privs`, which blocks sudo elevation.
The artifact Dockerfile validates the sudoers policy and switching to UID 1000
without elevating again during the build. GitHub Actions tests actual sudo access
in a running image. After deployment, verify `gosu heimdall sudo -n id -u` returns
`0` in the Railway console; a successful image build alone does not prove live
root access. Do not disable platform safeguards to make an image build pass.
