# Verification

## Public proxy security review — 2026-10-19

Review of the pinned native gateway found four `/admin/sop/*` routes that authorize loopback callers independently of forwarded headers. The adapter now blocks the entire `/admin/` namespace before forwarding. GET/POST regressions cover all four routes; ordinary SOP API forwarding and bidirectional upgrades remain covered. The full portable launcher and executor suites, strict Clippy for both crates, the pinned native configuration-library test, all 18 Python checks with the IaC SDK, formatting and diff checks passed. A redacted Gitleaks scan of the complete PR diff detected no secrets. Native Linux validation [39181448667](https://github.com/m-ridha/HeimdallClawd_bot/actions/runs/39181448667) passed for the preceding commit; this follow-up still requires its own Linux run. No live email delivery or native SOP mutation was performed.

## Public agent networking — 2026-10-19

Created a platform-managed generated HTTPS domain for the existing agent service, targeting port 3888. Live HTTPS `/health` returned 200; anonymous `/api/config` returned 401. The executor had no public domain or TCP proxy. Using `.railway/.venv`, all 18 Python checks passed with no SDK skip, and the production read-only `railway config plan` reported the configuration already up to date after domain creation. Generated domains remain outside IaC; no apply or application deployment was needed. This verifies routing and anonymous-access rejection, not AgentMail adapter activation or delivery.

## AgentMail email adapter preparation — 2026-10-19

Review corrections: omitted-body retrieval considers every enabled sender rule even when another rule already matches the subject. All asynchronous ledger access uses Tokio's blocking pool; unchanged ledger reads do not rewrite/fsync the file. Regressions cover both matched rule IDs after hydration, a held file lock with a responsive current-thread async runtime, and byte-preserving read-only ledger access.

Added opt-in signed email ingress inside the existing agent container, sender/content rules, durable disabled-by-default rule management, duplicate reservations, native gateway dispatch and owner-only Telegram response delivery. Portable fixtures exercise sender spoofing, Svix body/timestamp authentication, disabled/duplicate admission, one matching task dispatch, management state, ordinary HTTP forwarding and bidirectional upgrade traffic. Both Cargo suites, strict Clippy checks and Python helper checks passed locally on macOS (the pinned Railway SDK-dependent test was skipped). The configuration-library fixture also passed against pinned ZeroClaw v0.8.6, checking the extra MCP bundle and environment-only gateway bearer authentication without secret persistence.

Linux-specific confinement tests do not run meaningfully on macOS. Native Actions validation and real AgentMail-to-Railway/Telegram verification remain separate; no webhook, public domain, Railway variable, deployment or email was created during these local checks. See [activation and delivery limits](agentmail-events.md).

Observed on 14 October 2026. Runtime: ZeroClaw 0.8.5, revision `cb2b20a9fe59004b7ef03e123942d31f29236d15`. Linux executable hashes and compiler provenance were recorded in the historical artifact provenance; current hashes/provenance are generated into each release package.

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

## Failed-build correction and Python IaC migration

The failing release completed its cold WASM runtime compilation in 18m32s, then failed the launcher bootstrap assertion because the executable was absent from its fixed production path. The corrected launcher validation stage copies it to `/usr/local/bin/zeroclaw` first. The full launcher suite, including that bootstrap case, passes in local Linux x86_64 Docker using the existing checksum-pinned executable. The remote-skill integration contract passes locally, along with Python packaging/IaC guards, actionlint and strict launcher Clippy.

The Python IaC migration was applied to production with unmanaged variables and unchanged volumes/resource limits. The final plan reports no drift, migration status reports no Config as Code services, and the CLI reports both repository sources absent. Compile caches now export before downstream validation; runtime/support compilation runs in parallel. A fresh build-only GitHub run is needed to measure the cold-build improvement and verify cache export/import on hosted runners.

## Local GitHub integration checks — 15 October 2026

The working-tree GitHub integration is not deployed or live-verified. Fresh macOS checks ran `cargo test --locked --manifest-path launcher/Cargo.toml` (also with `HEIMDALL_TEST_RUNTIME` set to the local pinned runtime), the corresponding executor test command, strict `cargo clippy --locked --manifest-path ... --all-targets -- -D warnings` for both crates, Rust format checks, the Python `tools/tests` suite with the pinned Railway SDK installed, and `git diff --check`. The portable tests exercise MCP discovery, malformed arguments, missing credentials, restricted subprocess environments, output bounds and exact configuration grants. A redacted scan of tracked working-tree files found no secrets. Linux-only suites are excluded on macOS; no native Actions run was dispatched because this task explicitly keeps changes local and uncommitted. Authenticated GitHub API access, Codespaces lifecycle/SSH and remote cancellation remain unverified.

The targeted command `docker build --platform linux/amd64 --target gh-download --file deploy/bot/Dockerfile --tag heimdall-gh-download-check .` passed archive checksum validation and extraction. `docker run --rm --platform linux/amd64 --network none heimdall-gh-download-check /usr/local/bin/gh --version` reported 2.102.0. This checks the CLI download stage only, not a full native runtime build or Linux isolation suite.

The single-token revision uses only operator-supplied `GITHUB_TOKEN`. Fresh launcher tests (including the local pinned-runtime configuration smoke check), executor tests, strict Clippy for both crates, formatting, ten Python tests with the pinned SDK, and `git diff --check` passed again. Token formatting tests reject prefixed/whitespace values and confirm the generated TOML contains only the internal environment reference. Native Linux isolation and authenticated GitHub/Codespaces operations remain unverified for this local revision.

## Local Codespaces diagnostic checks — 15 October 2026

Fresh `cargo test --locked --manifest-path launcher/Cargo.toml` (with the local pinned runtime supplied as `HEIMDALL_TEST_RUNTIME`) and `cargo test --locked --manifest-path executor/Cargo.toml` passed. Strict `cargo clippy --locked --manifest-path ... --all-targets -- -D warnings` passed for both crates, as did format checks, ten Python tests via the pinned-SDK virtual environment and `git diff --check`. New fixtures cover fixed diagnostic categories, ambiguous HTTP statuses, signal exits, preservation of MCP `isError`, and exclusion of synthetic credentials, URLs and user content. These are portable mocked checks, not authenticated GitHub failures. Native Linux/Actions and live Codespaces checks remain pending; no deployment was performed.

## Local agent-experience improvements — 15 October 2026

Managed instruction refresh, legacy-copy preservation, symlink/oversized-file rejection, Codespaces configuration discovery without credentials/network calls, explicit file-send recipients, executor busy/duplicate errors, and sanitized skill/transfer diagnostics were implemented locally. The pinned-runtime configuration smoke test accepts 41 exact primary-agent tool grants, including `codespace__status`; host execution remains excluded. These changes are not deployed or live-verified. Startup now refreshes maintained TOOLS.md and IDENTITY.md, preserving changed old copies; USER.md is untouched. Native Linux bootstrap, isolation and real MCP client tests require the build workflow and have not run for this uncommitted revision.

Fresh verification for these changes: both crates passed `cargo test --locked --manifest-path <crate>/Cargo.toml` and `cargo clippy --locked --manifest-path <crate>/Cargo.toml --all-targets -- -D warnings`; Rust formatting, ten Python tests with the pinned SDK and `git diff --check` passed. Launcher checks included the local pinned-runtime configuration smoke test. The credential-free loopback transfer test required sandbox permission to bind its listener; it then passed. No live API calls, Codespaces operations, production changes, commits or pushes were performed.

## PR #6 native Codespaces contract correction — 15 October 2026

Review identified a stale five-tool expectation in deploy/tests/codespace-mcp.rs and its omission from Docker validation. The corrected test expects six tools, calls status through the pinned ZeroClaw MCP client with GITHUB_TOKEN and HEIMDALL_CODESPACE_REPOS explicitly removed from the child environment, and asserts missing configuration plus no authentication/network verification. Docker now copies and runs heimdall_codespace_stdio alongside the existing native MCP/skills contracts.

The test was copied into the local checksum-pinned upstream checkout and passed with `HEIMDALL_TEST_SKILLS_MCP=<local launcher executable> cargo test --locked -p zeroclaw-tools --no-default-features --test heimdall_codespace_stdio`. This was a macOS test of the production client protocol, not Linux isolation validation. Both repository Cargo test suites, strict Clippy, Python safeguards and git diff checks passed again. A new branch build is still needed to validate the added Docker test invocation on Linux; the existing workflow run tests the earlier commit.

## Native validation costs — 15 October 2026

Successful build-only run [38705630380](https://github.com/m-ridha/HeimdallClawd_bot/actions/runs/38705630380), commit 4c277e3, spent 24m13s in the validation/export step. The contract compile/test layer consumed 14m54s; its reported individual test runtimes were seconds or less. Registry cache export consumed 6m59s. This is a measurement of the old Docker ordering, not the proposed optimization.

The local optimization splits native contract compilation (`--no-run`) from execution against current release inputs. Eleven Python safeguard tests (including a new cache-boundary/execution-parity guard), actionlint, both crates' portable Cargo tests and strict Clippy passed. The local pinned runtime was supplied for launcher configuration smoke checks; the loopback test ran with the required local-listener sandbox permission. Diff checks pass for the pipeline changes; the pre-existing executor/.gitignore edit is untouched. No native workflow has tested the new ordering, and no cold or warm speedup is claimed. A new branch build and a subsequent cached build are needed to measure it.

## 2026-10-15: extended capabilities, local checks

The uncommitted capability branch adds 55 explicit tool grants, owner-directed hourly heartbeat, image/audio preprocessing, SQLite fact extraction and snapshots, graph storage, session/advisory tools, bounded diagnostic rotation without tool/prompt payload capture, and replay fixtures. Native runtime build flags add embedded dashboard, WebAuthn and ACP channel support. Deepgram Nova-3 and Cohere embed-v5.0-fast are selected; their environment-backed keys and live verification remain outstanding; remote clients and WebAuthn origins await setup. Screenshot execution remains blocked. Upstream knowledge-bundle ingestion, reranking and live evaluations are not implemented and are not claimed as enabled.

Fresh macOS checks: launcher `cargo test --locked` with `HEIMDALL_TEST_RUNTIME=/tmp/heimdall-zeroclaw-source/target/debug/zeroclaw` passed after allowing its loopback HTTP fixture; executor `cargo test --locked` passed portable tests. Both crates passed `cargo clippy --locked --all-targets -- -D warnings` and formatting checks. The pinned runtime accepted the new effective configuration and preserved secret overrides without saving credentials. Python unittest discovery passed all 11 tests under the pinned IaC SDK environment. The synthetic `zeroclaw eval run --suite deploy/bot/evals --mode replay` passed one case / four expectations. Actionlint, `git diff --check`, and the redacted diff secret scan passed.

Linux-only suites did not run on macOS. The original b30cb1f Docker feature combination, dashboard assets and native Linux contracts passed [build 38756450119](https://github.com/m-ridha/HeimdallClawd_bot/actions/runs/38756450119) in all three jobs. The subsequent inventory contract needs its own native build. Nothing in this entry establishes live Telegram, external-provider, remote-client, WebAuthn or deployment verification.

Provider follow-up: the primary agent now references `deepgram.voice` with Nova-3 and native language detection. Cohere embed-v5.0-fast uses its documented OpenAI-compatible endpoint with 2,048 dimensions and a 1,024-entry embedding cache. The pinned `zeroclaw-config` integration test was compiled and run locally (`cargo test --locked -p zeroclaw-config --no-default-features --features schema-export --test heimdall_secrets`); it confirmed both synthetic key overrides resolve correctly and remain absent after `config.save()`. Launcher configuration smoke checks also read back the provider aliases/model/dimensions using the pinned macOS binary. This does not verify either external API or the new Linux image features.

## 2026-10-16: PR #9 review follow-up

The original capability revision b30cb1f passed native Linux [run 38756450119](https://github.com/m-ridha/HeimdallClawd_bot/actions/runs/38756450119). A review-driven contract now renders the launcher configuration, redirects state into a temporary directory, builds the production native tool factory and assembles ScopedToolRegistry for Telegram and ACP. It asserts all fourteen added built-in schemas are structurally valid, with the explicit ACP exclusion of persistent memory tools; host shell/file/git/screenshot/delegation tools remain absent. It also checks ACP-only delivery and omitted endpoint-gated transfers. MCP connections and peripherals are deliberately not opened by this built-in inventory test; separate transport contracts cover MCP.

The new contract passed locally against the pinned runtime with `cargo test --locked -p zeroclaw-runtime --no-default-features --test heimdall_inventory` and `HEIMDALL_TEST_SKILLS_MCP` pointing to the built launcher. Both crates' portable tests and strict Clippy, eleven Python tests with the pinned SDK, actionlint, formatting and diff checks passed. Native Linux verification of this follow-up is pending. Updated instructions distinguish stored-session appends from actual Telegram notification/turn execution, and docs distinguish 55 policy grants from exposed inventory.

Release acceptance remains outstanding: deploy the new artifact, restart, inspect the actual primary Telegram inventory for all fourteen added schemas, and exercise synthetic memory/graph/image/voice checks. Deepgram/Cohere keys must be configured, and Cohere request usage must be observed; successful keyword recall alone is insufficient. Check whether existing memory needs an explicit reindex. No live provider calls or production deployment were performed during this review follow-up.

## 2026-10-16: supplied audit, local remediation

See [assessment and remaining risks](audit-review.md). Named-secret origin enforcement and private-address rejection passed two integration checks against the pinned ZeroClaw tools crate. The new patch applies cleanly to an unmodified copy of its target file. Launcher and executor portable tests, strict Clippy for both crates, thirteen Python tests with the pinned SDK, formatting, actionlint and diff checks passed. An existing route-order-dependent test was corrected to assert the Codex route by hint and preserve the free route's model.

`cargo check --locked --manifest-path executor/Cargo.toml --target x86_64-unknown-linux-gnu --tests` passed, checking the new Linux code and tests. This is cross-target type checking, not execution: the address-space cap, stacked seccomp filters, real pthread probe, timestamp/copy/archive compatibility and temporary-directory cleanup still require the branch's native Linux workflow before merge. The updated runtime patch has not completed a native release build. No live API calls, infrastructure changes or deployment were performed.

## 2026-10-16: GitHub repository workflow, local checks

The local changes add offline Git to both executor image recipes and their native test builders, plus a Linux regression for initialization, branching, diffing and committing. Codespaces accepts an explicit standalone `*` repository policy while retaining syntax validation, account listing checks and resource limits. Managed instructions explain GitHub REST editing, offline Git, remote builds and authorized CI dispatch; see [GitHub workflow](github-workflow.md).

Both crates passed portable Cargo tests, strict Clippy and formatting checks. Launcher tests used the local pinned runtime and permission for loopback fixtures. All thirteen Python tests passed with the pinned IaC SDK. Linux cross-target type checking passed; native Linux execution of the Git regression is pending. No Linux isolation result is inferred from macOS tests.

Railway service inspection showed a successful deployment, the existing persistent `/data` volume and the names `GITHUB_TOKEN` and `HEIMDALL_CODESPACE_REPOS`; presence does not establish token validity or permissions. No variable values or private keys were read. No repository mutations, Railway variable changes or deployment were performed. Access to the requested initial repository remains unverified. These changes are based on the audit remediation branch and require those fixes as well.

## 2026-10-16: failed native run diagnosis

[Run 38843995178](https://github.com/m-ridha/HeimdallClawd_bot/actions/runs/38843995178) failed in the support-binaries executor test: `cp -p` reported Operation not permitted at the compatibility assertion. The runtime-binary compile job succeeded; downstream validation was skipped and the parallel launcher build was canceled.

A disposable native ARM Debian Bookworm trace showed `cp -p` reading POSIX ACL metadata with fgetxattr and writing it with fsetxattr. Injecting EPERM for fgetxattr reproduced the exact error and exit 1; denying optional fadvise64/copy_file_range still allowed copying. The executor filter now admits only the two descriptor-based ACL calls, subject to normal kernel UID/DAC checks. The Linux regression additionally verifies nonempty copied content, mode and timestamps. No networking or process-control grant was added.

This is syscall diagnosis, not execution of the production x86_64 filter. Cross-target type checking passes; the updated native suite remains pending. No new Actions run was dispatched, as requested.

## 2026-10-16: archive creation follow-up

[Run 38848634945](https://github.com/m-ridha/HeimdallClawd_bot/actions/runs/38848634945) passed the strengthened copy assertions, then failed in archive creation with `tar: compat.tar: Cannot open: Operation not permitted`. The earlier ACL correction therefore resolved the copy failure; this run does not establish later Git or isolation assertions.

Static inspection of Debian Bookworm x86_64 tar showed its imported `creat` symbol; the matching libc implementation issues syscall 85 (creat). This syscall was missing from the executor allowlist even though open/openat creation was already allowed. The fix admits creat with ordinary UID/DAC enforcement. Native regression assertions now identify copy, archive creation and extraction separately, remove the original before extraction and verify extracted content. Production x86_64 execution remains pending; no new Actions run was dispatched.

## 2026-10-16: PR #11 review corrections

Native [run 38849345937](https://github.com/m-ridha/HeimdallClawd_bot/actions/runs/38849345937) completed successfully for the pre-review-fix head b600c4c, including native contracts and packaging. This establishes the earlier copy/archive/Git Linux checks; it does not validate subsequent review changes or a deployment.

Both review findings were confirmed. Compact mode's 6,000-character file cutoff omitted the detailed GitHub guide; the primary runtime now explicitly uses full context, whose native per-file budget is 20,000 characters. A contract in the existing native inventory suite reads the production guide and rendered launcher config, applies the effective compact/system-prompt settings to the pinned prompt builder and asserts the complete guide survives. It passed locally against the pinned runtime. No host reader or new skill is granted; the larger prompt has additional context cost.

Truncated Codespaces output could retain an incomplete token. Output redaction now removes trailing token prefixes at truncated boundaries before returning diagnostics, while retaining complete-token replacement. Regressions cover stdout/stderr, 4 KiB read boundaries, the capture limit, a fully retained token at the limit and a token with overlapping start/end characters. Both crates' portable tests, strict Clippy, formatting, all thirteen Python tests with the pinned IaC SDK, actionlint and diff checks passed. Updated native Linux execution and post-release prompt acceptance remain pending. No Actions run or deployment was dispatched for these corrections.

### Follow-up: overlapping redaction spans

The follow-up review correctly reproduced a flaw in bb8506d: a fully captured token followed by `h` could make prefix trimming delete its final `g`, leaving a near-complete token unmatched. Redaction now finds every full-token span and the longest truncated suffix-prefix span in the original bytes, merges overlapping spans, then renders replacements. It bounds the returned UTF-8 text to the stream limit. Regressions assert exact redacted output for the reviewer's synthetic case in both streams and for overlapping full matches. All three redaction tests, both portable Cargo suites, strict Clippy, formatting and thirteen Python tests passed. Native execution of this revision remains pending; no workflow was started.

## 2026-10-16: audit v4, first implementation phase

The audit's first phase (R2/R5) adds opaque MCP correlation namespaces and panic-safe active-job cleanup without changing the five tools, two-job capacity, deadlines, helper privileges or readiness checks. Portable tests cover unique initialization headers, malformed headers, session-scoped cancellation, ignored sessionless cancellation and cleanup/ID reuse after task panic and abort. The pinned real-client synthetic HTTP contract passed locally, proving session-header echo and first-call ID 3. Its local test copy excluded the file-level Linux gate solely to run that portable HTTP fixture; the root-only executor contract was not executed.

Both crates' portable Cargo tests, strict Clippy, formatting and thirteen Python tests with the pinned IaC SDK passed. Executor x86_64 Linux test type checking passed. The real-client Linux contract now runs two fresh clients whose first commands wait for one another, exercising concurrent identical request IDs. That test and native isolation still need the branch's native workflow; no Actions run or deployment was started. No live provider credentials were used. Recovery, logging verification and admission waiting are subsequent phases, not claimed implemented here.

## 2026-10-16: audit v4, recovery phase

The local implementation adds explicit `HEIMDALL_RECOVERY=1` before owner/secret reads, state initialization and config/instruction writes. On Linux it drops the listener's identity to the existing runtime UID without changing persistent ownership. Its bounded HTTP listener returns labeled recovery health and no operational bot endpoints; no daemon/provider call starts. Authentication validation has a 60-second deadline, discarded diagnostics and process-group cleanup. Both supervisors report read-only filesystem capacity; the executor readiness gate is unchanged. Operations now documents storage checks, manual cleanup, sign-in and exiting recovery, including the intentionally misleading-to-automation green deployment state and the prohibition on releases during recovery.

Fresh portable launcher/executor suites and strict Clippy passed, including strict recovery configuration, safe auth failure/timeout and a real recovery subprocess serving labeled health without owner/provider setup. Executor x86_64 Linux type checking passed. Launcher cross-target checking could not complete because `x86_64-linux-gnu-gcc` is unavailable for its ring dependency. This is not native execution. Root identity/bootstrap tests, full-volume recovery and Railway dashboard-terminal behavior remain unverified; no native Actions run, Railway variable change or deployment was performed.

## 2026-10-16: audit v4 logging, trusted users and admission

A pinned-runtime fixture exercised real Telegram send failures and failed MCP tool calls with synthetic token/message values through the production terminal subscriber. Verbose mode leaked the synthetic failed-tool message (not the synthetic Telegram token); quiet mode emitted neither. Daemon argument construction now omits verbose mode and has a portable regression asserting its exact arguments. The quiet-mode privacy fixture passed locally and is copied into the channels integration suite for future native validation. This verifies the covered terminal paths only; persisted diagnostics, other paths and live Railway logs are not newly verified.

README/operations now describe additional Telegram accounts as fully trusted operators sharing credentialed tool capabilities, sessions and workspace data. The executor reserves session-scoped IDs and waits fairly for at most three seconds in the response task, outside its active-map lock. Queued disconnect/cancellation prevents execution, removes reservations, and preserves the two-job capacity. Timeout returns JSON-RPC -32001 in the HTTP 200 stream; explicit queued cancellation returns -32800. Portable regressions cover slot release, expiration, duplicate IDs and cancellation/disconnect cleanup. The existing 30-second execution and 35-second MCP client deadlines are unchanged.

Native Linux builds and disposable Railway recovery tests remain deferred at the owner's request. No live logs or credentials were accessed for this fixture, and no deployment was performed.

Fresh local checks for this follow-up: both portable Cargo suites and strict Clippy, executor x86_64 Linux type checking, formatting, thirteen Python tests with the pinned IaC SDK, actionlint and diff checks passed. The new logging fixture passes with production quiet mode. The cache guard was updated to pin five compile commands and their identical execution counterparts, including the logging contract. No native workflow was dispatched.

## 2026-10-16: executor Python and jq

The local change installs Debian Bookworm python3 and jq alongside Git in both executor production recipes and their native test builders. It adds a Linux regression for standard-library JSON/Base64/path/tempfile operations and spawning jq under the actual executor restrictions. No syscall, credential, network or resource-policy grant changed. Managed instructions describe offline scripting and checking the Codespaces toolkit independently.

Both portable Cargo suites, strict Clippy, formatting, thirteen Python tests with the pinned IaC SDK and diff checks passed. Executor x86_64 Linux test type checking passed; it does not execute Python/jq under seccomp. The native scripting regression and deployed-image checks remain pending. No native workflow or deployment was started for this change.

## 2026-10-16: typed-provider Telegram voice initialization

The second supplied log export contains explicit transcription-manager initialization failures after the variable-update deployment. Railway reported that deployment successful and the expected Deepgram variable name present; no key values were read. In the pinned source, Telegram's legacy-first initialization can leave its voice-limit configuration unset even after a typed provider manager is attached. The voice handler then skips before download/transcription.

A synthetic loopback Telegram/typed-local-whisper contract reproduced the original silent skip (timeout). The generated typed-transcription patch replaces the Telegram factory chain with canonical provider-and-limit initialization. The same contract then passed, delivering `[Voice]` plus synthetic transcript text to the channel receiver. A pinned unit regression passed for typed Deepgram registration and limits, with disabled behavior preserved; missing-key rejection is also covered. No live Deepgram request or real audio was used. Release provenance now records the patch digest, and native channels validation compiles/runs the contract.

Both portable Cargo suites, strict Clippy, thirteen Python tests with the pinned IaC SDK and patch application checks passed locally. Native Linux build and deployed voice acceptance remain pending; neither a workflow nor deployment was started for this fix. The separate Python/jq changes remain uncommitted alongside this work.

## 2026-10-18: offline Rust executor CI validation

Native Linux [run 39118114066](https://github.com/m-ridha/HeimdallClawd_bot/actions/runs/39118114066) passed for commit `190b9253bb8ba6058bfc88f50c74af6f9c012dad`. The confined smoke test exercises Rust tooling versions, offline standard-library Cargo tests, formatting, Clippy, the updated address-space cap and restricted local IPC. The per-process virtual-address ceiling increased from 96 MiB to 1 GiB; Railway's 250 MB container RAM cap and the 30-second command deadline remain unchanged. This is synthetic native CI evidence, not a live Railway result. Post-release verification on the deployed executor remains pending.

## 18 October 2026 — ZeroClaw v0.8.6 upgrade preparation

The upgrade pins revision `c473221313341a853e04122f93bb9bd26b5e6c47` and its downloaded source archive checksum. Source review found upstream replacements for typed transcription initialization and crate-local WIT paths; those two local patches were removed. The five remaining patches were regenerated against the pinned source, preserving their policy and contract tests. Production and native-test builds explicitly include the external-tool and project-intelligence adapters introduced by the upstream feature split.

The native transcription contract now uses the upstream production channel factory with a typed loopback provider, rather than a method supplied by a local patch. It checks the configured duration limit before downloading/transcribing a valid synthetic voice note. Tool inventory checks cover built-in discovery, excluded-host-tool visibility, ACP memory exclusion and the external adapters.

Native Linux workflow validation and live Railway upgrade checks are tracked in the upgrade PR. Local macOS checks cannot establish Linux isolation or deployed compatibility. No production deployment or persistent-state migration has been performed as part of source preparation.

Source inspection also confirmed that native memory reranking is implemented, correcting the earlier capability description. The template now enables MMR/deduplication with a two-times candidate pool, and the inventory contract materializes the production settings and checks duplicate collapse without any external provider.
