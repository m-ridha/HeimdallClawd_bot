# ZeroClaw 0.8.6 native-feature and patch audit

Source authority: immutable revision in deploy/bot/runtime.lock.json, with the
archive SHA-256 verified before inspection. The [versioned documentation](https://docs.zeroclaw.com/v0.8.6/en/)
is guidance; constructors, schema and compiled features establish actual support.

## Native features used

The full built-in selector is retained, with explicit execution grants rather
than permission wildcards. Native shell/filesystem/edit/search/Git, shell
scheduling, delegation/subagents, memory export/purge, calculator, weather,
canvas/TodoWrite, LLM tasks, model routing/switch, proxy settings, backups and
retention previews augment existing cron/SOP/pipeline/session/knowledge/report
capabilities. Full autonomy and unrestricted command admission are intentional.
Native environment filtering still applies: GITHUB_TOKEN is explicitly available
to gh; root access can reach credentials irrespective of shell filtering.

Browser automation adds browser-native to the feature pin, uses installed
Chromium and a loopback-only supervised ChromeDriver, and needs no remote account.
Browser delegation is a different tool requiring a separate authenticated coding
CLI; enabling it without that CLI would create a broken capability.

Existing supported provider integrations, voice, semantic memory, signed email,
WASM plugins, hooks, dashboard, ACP and replay evaluation remain. Remote nodes,
WebAuthn enrollment, hardware, extra channels, A2A peers and vendor integrations
still require real devices/endpoints/clients/credentials. No fake setup is added.
Desktop screenshots require a desktop; headless browser screenshots are supported.
Video summarization and confirmed data-retention purge are upstream placeholders,
not functioning capabilities that configuration can activate.

## Removed patches

| Patch | Native replacement / reason |
| --- | --- |
| remote-skills.patch | Upstream SkillShellTool runs scripts locally under the native policy. No remote executor or file staging is needed. |
| native-child.patch | Upstream text browser and native skill execution replace the credential-isolation wrapper. General root commands deliberately bypass that former boundary; do not retain a misleading isolation claim. Narrow integration MCP child profiles remain. |

## Retained patches

| Patch | Why native configuration does not replace it |
| --- | --- |
| private-telegram.patch | Allowlist configuration alone does not establish private-chat rejection before callback/media processing. Preserve the tested private-chat boundary. |
| http-secret-origins.patch | General domain allowlists do not bind each named credential to its own exact TLS origin. |
| http-diagnostics.patch | Existing generic tool failures do not supply the same sanitized actionable status/category diagnostics. Payload logging is not an acceptable replacement. |
| gateway-admin-auth.patch | A loopback reverse proxy must not turn public callers into local administrators. Native remote-admin configuration alone does not preserve the original caller boundary. |
| sop-workshop-guidance.patch | Corrects incomplete parameter/validation guidance; no native setting supplies the corrected tool descriptions. |
| responses-tool-history.patch | Repairs unmatched call/result replay and deterministic retry classification; larger context budgets or retries do not repair history. |

No patch is removed solely because its security restriction is inconvenient.
The two removals follow the explicitly approved execution-model change. Retained
patches still require zero-fuzz application and their native contracts; this audit
is not a claim of live deployment or validation before those checks run.
