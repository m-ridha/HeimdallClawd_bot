# Notice

The build uses ZeroClaw v0.8.6, revision c473221313341a853e04122f93bb9bd26b5e6c47,
from <https://github.com/zeroclaw-labs/zeroclaw>, distributed under MIT OR
Apache-2.0. Both upstream license files accompany the deployment recipe and
release package. The immutable archive checksum and build features live in
runtime.lock.json; Dockerfile verifies the archive and applies patches with zero
fuzz before compilation.

## Native behavior and retained patches

Subscription authentication, provider integration, channel routing, SQLite
sessions, typed transcription, WIT interface paths, MCP, local skill execution
and browser automation use upstream features. The single-service model does not
need the former remote-skills or native-child patches. General commands have
explicitly approved root access; narrow integration profiles do not confine them.
The browser-output patch adds DNS-pinned HTTP fetching with validation on every
redirect, restricted offline Lynx rendering, bounded input/output and child cleanup.
The narrow skills installer uses a launcher-created registry directory within the
existing Landlock profile.

| Patch | Contract not replaced by native configuration |
| --- | --- |
| tool-log-privacy.patch | Remove raw legacy tool arguments/results at the logging boundary and enforce the tool-I/O opt-out before terminal, broadcast, export and persistence. The optional OTel observer applies the same tool denylist before content processing. Tool execution and conversation history remain unchanged. |
| security-dependencies.patch | Update the pinned runtime lockfile to rustls-webpki 0.103.16, webpki-roots 1.0.10 and ctutils 0.4.4 for RustSec advisories 2026-0382, 2026-0381 and 2026-0385. |
| telegram-startup.patch | Retry only explicit Telegram command-menu size rejections with a shrinking menu; bound polling requests and report transport categories without secret URLs or raw error bodies. |
| private-telegram.patch | Reject non-private Telegram updates before callback/media processing, in addition to account allowlisting. |
| http-secret-origins.patch | Bind each named GitHub/AgentMail credential to its exact HTTPS origin before resolving it. General domain allowlists do not provide per-secret binding. |
| http-diagnostics.patch | Provide bounded, sanitized actionable HTTP status/category diagnostics without enabling payload logging. |
| gateway-admin-auth.patch | Treat trusted-proxy requests as remote, requiring current paired authentication and remote-admin policy; proxy loopback must not grant local CLI authority. |
| sop-workshop-guidance.patch | Correct action-specific parameters and required Markdown syntax without echoing rejected procedure content. Native validation and explicit application remain unchanged. |
| responses-tool-history.patch | Reconcile retained calls/results, supply unknown-outcome errors for missing outputs and avoid retrying rejected history shapes across OpenAI Responses/Codex. Never rerun tools or rewrite stored sessions. |
| browser-output.patch | Validate each HTTP redirect and pin connections to the checked addresses without proxies. Render fetched content through restricted offline Lynx; reject links/w3m. Bound fetched input and rendered stdout to 500,000 bytes and kill the child on overflow, timeout or cancellation. Upstream hands network access to the external browser and only checks the initial URL. |

AgentMail uses native /sop/* admission and the background SOP driver. Synchronous
/webhook keeps its ordinary upstream timeout; no timeout patch is needed.

Remove a patch only after a tested upstream replacement covers its contract.
Configuration, retries or extra logging do not substitute for missing behavior.
Artifact provenance retains patch hashes; source and tests stay in the tagged
repository rather than the runtime-only Railway upload.

Browser-output regressions are compiled and executed from `deploy/tests/browser-output.rs` in the native Linux contract stage. They cover redirect destination checks, DNS pinning, relative redirects, offline rendering without subrequests, redirect/input limits, backend rejection, concurrent stdin/output, the exact stdout limit, overflow with stderr held open, timeout and cancellation cleanup. The skills bootstrap test uses a local Git fixture; no external registry or credentials are needed.
