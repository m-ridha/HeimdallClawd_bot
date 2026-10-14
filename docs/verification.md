# Verification

Observed on 14 October 2026. Runtime: ZeroClaw 0.8.5, revision `cb2b20a9fe59004b7ef03e123942d31f29236d15`. Linux executable hashes and compiler provenance are recorded in `deploy/bot/artifact.lock.json`.

## Passed

- Native Linux executor isolation tests and startup probes: UID/credential boundaries, workspace files and commands, protected fixture denial, bounded output, timeout, cancellation and descendant cleanup.
- Pinned native MCP client against the actual executor: incorrect-token rejection, discovery of exactly three tools, write/read/pwd, protected-file denial, cancellation cleanup and subsequent connection recovery. The current executor deployment (identifier kept in Railway) passed its startup probes.
- Three pinned Telegram dispatcher regressions on Linux: non-private updates rejected, private authorization retained, and group callbacks unable to approve tools. The full upstream test suite was not run.
- Seven native Linux launcher checks: strict allowlist/configuration, memory-only secret overrides, optional provider/model choices, symlink-safe initialization, file permissions and invalid-auth startup rejection.
- Native configuration-library fixture: exact placeholder Telegram token and MCP header resolution, secret absence from saved config, derived session data directory and gateway bind compatibility. This fixture ran locally against the pinned library.
- Railway bot deployment (identifier kept in Railway): image checksum checks, HTTP health check, one active successful deployment, Telegram polling and private MCP connection with three tools.
- Native SQLite conversation persistence: engine logs confirmed one retained conversation restored into memory. Image replacement and an explicit in-place restart restored that conversation. Subscription authentication also survived both.
- Independent code review completed for authentication, tool isolation and conversation persistence.

A current idle sample after restart reported approximately 10.1 MB for the bot and 1.1 MB for the executor. These are individual samples, not a monthly usage forecast; the preceding hour included rollout activity.

## Live Telegram results

The owner confirmed a real `gpt-6.1-sol` subscription response recalling earlier workspace and protected-file checks. A subsequent request read the existing `ready.txt` unchanged (`HeimdallClawd tools ready`), wrote and read `zeroclaw-ready.txt` (`ZeroClaw tools ready`), returned `/data/workspace` from `pwd`, and received `Permission denied` for `/run/heimdall-control/probe`.

The native authentication refresh succeeds, and the active conversation database and both workspace files remain available. Final MCP-only executor deployment (identifier kept in Railway) passed its native Linux build/contract/startup checks and is healthy. Final bot deployment (identifier kept in Railway) is healthy, retains the subscription default and adds optional OpenRouter routing. Current portable checks pass: six launcher tests, eleven executor tests, and strict Clippy for both crates. The current bot image passed seven native Linux launcher checks, including its root bootstrap test.

## OpenRouter option

The optional route selects `openrouter/free` explicitly; Codex remains the default and no paid fallback is configured. Native configuration-library tests verify exact private key resolution and absence from saved configuration. The owner selected the free route and confirmed a remote read returned `ZeroClaw tools ready`. Native configuration tests also verify creating an additional provider alias from environment variables without persisting its API key.

## Limits of verification

Ollama has not been live-tested. Full native tool-registry behavior across every failure, skill and scheduling path was source-reviewed rather than exhaustively integration-tested. Git history replacement cannot erase third-party hosting caches or copies held outside this repository.

## Release automation

The release package was generated locally from the verified Linux executable; checksum/provenance and exclusion of private state were checked. Two deployment-helper regressions pass: another healthy deployment cannot mask a pending or failed target deployment. Workflow YAML, shell syntax and actionlint checks passed. A first full GitHub-hosted release build/deploy has not run; the owner will push the prepared local history and version tag.

## Tavily configuration checks

The source template selects Tavily, three results and a 15-second timeout. Eight launcher tests pass, including exact four-tool allowlist/auto-approval checks. The pinned native configuration-library fixture passes with a dummy Tavily key: the environment override resolves exactly in memory and remains absent from TOML after explicit `Config::save()`. Strict launcher Clippy also passes. This change has not been deployed or tested against the live Tavily API; deployment and a Telegram search are still required.

## Release-trigger and skills changes

The first Actions build failed compiling upstream config unit tests with `schema-export` disabled. Enabling that feature in the CI test invocation compiles the same config unit-test target successfully locally; this is a feature-selection fix, not a skipped check. The workflow is now release-published only and uploads assets to the existing release after successful deployment.

Both runtime images install Git. The primary agent gets two scoped skill-management tools through a stdio MCP server. Three unit checks, an actual stdio protocol test and a pinned native MCP-client handshake/discovery test pass, including command-source validation and removal of a preconfigured dummy credential from the subprocess environment. The native skills-list command also succeeds without inherited API credentials. Eight configuration tests, the native secret-resolution fixture, Clippy, release-helper tests and actionlint pass. A new GitHub-hosted build and a live bot skill installation remain required after publishing the corrected release.

## October 15 capability update — local verification

The expanded configuration, Telegram transfer bridge, binary executor file operations, two-job executor admission and release packaging changes pass the portable launcher/executor suites and strict Clippy. The pinned native runtime accepts the configuration and dummy secret overrides; saving configuration does not persist those fixture credentials. The expanded WASM feature set passes cargo check after correcting its WIT source path. Two upstream regression tests cover remote skill command dispatch and bounded skill copying. Four Python release/deployment tests, actionlint and shell syntax checks pass. A full tracked/untracked source snapshot scan with redacted Gitleaks reports zero findings.

These changes are not yet live-verified. The local Docker engine is ARM64, while the executor seccomp implementation and production release target are Linux x86_64; an ARM64 Docker build cannot compile that architecture-specific syscall list. Native Linux x86_64 isolation/MCP tests remain mandatory in the release workflow. Browser interaction is not configured: the selected Tavily integration is search/content access. Hook/plugin frameworks require actual handlers/packages to exercise them. End-to-end scheduled delivery, Telegram file transfers, skill execution, SOPs and pipeline behavior must be checked after publishing the new release. No live Railway resources or IaC ownership were changed by these local checks.
