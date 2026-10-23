# AgentMail email events

The optional Rust adapter runs inside the existing agent service. It uses ZeroClaw's authenticated `/sop/webhook/<sop-name>`; no extra Railway service or additional channel feature is required. The launcher owns public `PORT` (normally 3888), binds ZeroClaw at `127.0.0.1:3887`, and proxies dashboard, pairing, WebSocket and ACP traffic. Pairing remains required. The gateway admin authentication patch treats forwarded reload and SOP administration as remote requests: native authentication validates the current paired bearer and requires remote administration to be enabled, including after token revocation or policy changes. The proxy forwards these dashboard routes and `/pair/code`, which reports status without revealing a code. Shutdown and pairing-code secret routes stay console-only. With `[heimdall.agentmail].enabled = false` in the template, the original direct gateway starts. The checked-in deployment enables the adapter.

For the distinction between the gateway endpoint and webhook channel, timeout rationale and public reachability, see [webhook architecture](webhook-architecture.md).

## Setup

Enable the adapter with `[heimdall.agentmail].enabled = true` in `deploy/bot/config.template.toml`. Set these credentials in the **agent Railway service**, never GitHub or Git:

| Variable | Value |
| --- | --- |
| `HEIMDALL_AGENTMAIL_WEBHOOK_SECRET` | AgentMail's webhook-specific `whsec_...` signing secret, distinct from the API key |
| `HEIMDALL_AGENTMAIL_GATEWAY_TOKEN` | A newly generated random bearer of at least 32 characters; keep private |
| `AGENTMAIL_AUTHORIZATION` | Existing full `Bearer ...` API credential; used for tools and retrieval of omitted bodies |

1. Deploy a release containing this feature through the normal release workflow.
2. Use the existing agent HTTPS domain, targeting container port 3888. Generated Railway domains stay platform-managed outside IaC; see [public networking ownership](../.railway/README.md). The authenticated dashboard/gateway share that domain; retain pairing. Keep ChromeDriver loopback-only.
3. In AgentMail, create an inbox-scoped webhook for `message.received` with URL `https://YOUR_AGENT_DOMAIN/agentmail/events`. Copy its signing secret directly into Railway, never into Telegram or logs. See [webhook registration](https://docs.agentmail.to/webhooks-overview) and [Svix verification](https://docs.agentmail.to/webhook-verification).
4. Configure the credentials and release the updated template/launcher. Internal port 3887 is reserved.
5. Install the intended `github-pr-review` SOP first (through Heimdall or the dashboard), with explicit execution ownership (for example `agent = "primary"`) and the exact Webhook trigger below. A Manual trigger alone does not match. Its steps must treat payload identifiers as untrusted, retrieve the referenced email if needed, verify the live GitHub request/head and prior reviews, perform the review and notify the operator on success or failure. Do not recursively invoke the same SOP. Keep approval gates. Do not share its trigger path with another SOP; unexpected dispatch results fail closed in the adapter, but cannot undo work already started.
6. Tell Heimdall “List the email filters,” then “Enable github-review-request.” The management tool arguments are `{"name":"github-review-request","enabled":true}`; disabling uses `false`. All rules initially default to disabled.
7. Verify a real review-request notification produces a Telegram response and unrelated mail does not start a task. These live checks require operator execution.

The adapter adds the internal bearer digest through native `ZEROCLAW_gateway__paired_tokens` environment configuration, preserving explicitly configured clients in that JSON array. If `ZEROCLAW_gateway__webhook_secret` is configured, the adapter also supplies it on internal requests. The bearer and signing secret are never rendered into TOML. Starting with a pre-authorized adapter suppresses the runtime's initial pairing-code banner; get a fresh code from the Railway console as the runtime user with `gosu heimdall zeroclaw gateway get-paircode --new --host 127.0.0.1 --port 3887`. Remote pairing uses the same public listener. The launcher's regenerated configuration remains authoritative after restart.

## Rules

### Required SOP manifest change

Add this trigger to the installed SOP manifest, alongside any Manual trigger you want to retain:

```toml
agent = "primary"

[[triggers]]
type = "webhook"
path = "/sop/webhook/github-pr-review"
```

The SOP subsystem must be enabled in the daemon. Reload the definition using the supported dashboard/SOP workflow. This source change does not migrate installed definitions or run existing events. Missing triggers return 404; missing SOP infrastructure returns 503. Validate a synthetic event after deployment before enabling production rules.

One stable `/agentmail/events` endpoint handles all rules. Adding another sender/content combination means editing `[[heimdall.agentmail.filters]]` entries in `deploy/bot/config.template.toml` and releasing the updated launcher, without a new route, folder or matcher implementation. Only this rule is supplied initially:

```toml
[[heimdall.agentmail.filters]]
name = "github-review-request"
sender = "notifications@github.com"
contains = "requested your review on:"
sop = "github-pr-review"
```

This section belongs to Heimdall's launcher, not native ZeroClaw configuration. It is removed from rendered `config.toml`. Credentials stay in Railway variables. Enabled flags persist separately; new or renamed rule names start disabled.

Up to 16 rules are accepted. Exact `name`, `sender` and nonempty `contains` fields are required; optional `sop` names an installed procedure (1–128 ASCII letters, digits, underscores or hyphens). Rules without `sop` send only a fixed Telegram notification, with no model call or email content. Custom template rules must explicitly add `sop` to enable that assignment; duplicate names or unknown fields stop startup. Sender is an exact case-insensitive mailbox match, including a conventional display-name form. Quoted display names can contain commas, semicolons and escaped quotes; multiple mailboxes, unclosed quotes and CR/LF are rejected. `contains` is a case-insensitive literal substring of the subject or extracted plain-text body, falling back to plain text when extraction is absent. Quoted messages can produce false positives; the admitted agent is instructed to check current GitHub review state before acting. Email content never grants operator authority to change code or send email.

`mail__filters` lists configuration, optional SOP assignments, aggregate state and the ten most recent event diagnostics; `mail__set_enabled` changes one existing rule when an operator asks. Flags persist on `/data` across deployments. New/renamed rule IDs start disabled; removed rules are inactive. Enabling does not backfill old mail. Disabling does not cancel an already admitted task. These tools cannot configure Railway credentials or public routing.

## Admission and delivery

- Verify the raw-body Svix HMAC and timestamp (five-minute tolerance); accept only `message.received`.
- Disabled, unrelated and duplicate events return 200 without an LLM call. Busy requests return 503 for AgentMail to retry. At most two events process concurrently.
- Webhook bodies are capped at 128 KiB and ten seconds of reading. When AgentMail omits a large body, a sender-matching enabled rule can retrieve the message from the fixed authenticated AgentMail API, with redirects disabled, a ten-second timeout and 1 MiB response cap. Event-provided `body_url` is never followed. Missing credentials or failed/oversized retrieval returns 503 without calling a model. See [payload limits](https://docs.agentmail.to/api-reference/webhooks/events/message-received).
- A match reserves `event_id` durably and returns 202 before background dispatch. Distinct SOP names are dispatched once each, in sorted order, through `/sop/webhook/<sop-name>` using paired authentication, the optional gateway secret and an idempotency key. Multiple rules assigned to the same SOP do not start duplicate runs for one event.
- Payload contains only bounded inbox/message identifiers and matched rule names; no subject/body or instruction prompt. Native content-safety checks and SOP approval gates still apply. Agent selection comes from SOP ownership, independently of Telegram's per-chat model selection.
- The normal upstream gateway request timeout remains 30 seconds. The adapter allows 35 seconds for each admission request, including response parsing; it does not wait for completion. Telegram admission notification is bounded separately to 20 seconds. Acknowledgement latency is not guaranteed: native admission can perform deterministic work. A timeout remains an ambiguous outcome.
- Only one matching result for the configured SOP with `started` or `coalesced` and a bounded run ID is treated as admitted. HTTP errors, missing/malformed results, unexpected SOPs, skipped/deferred or blocked results are failures, not completed reviews. Each confirmed admission is saved immediately; a later dispatch or Telegram failure retains earlier run references. There is no atomic transaction across multiple SOPs.
- The adapter sends a fixed admission notice to `TELEGRAM_OWNER_ID`; it does not forward a final model response. Completion/failure notifications must be authored in the SOP. Rules without an SOP send a fixed notification and never start agent execution.
- After reservation there is **no automatic replay**. Crashes, timeouts and delivery failures can leave incomplete events; redelivery cannot automatically repeat credentialed actions. Successful admission is not successful execution. Inspect native run status and external GitHub state before any manual retry.

## State and recovery

Startup failures identify the invalid credential variable and required format without printing its value. `AGENTMAIL_AUTHORIZATION` requires the full `Bearer ...` value; the gateway token is the raw token, and the webhook secret is `whsec_` followed by standard Base64 decoding to at least 16 bytes. Template and admission-state failures have separate startup messages. A state error requires inspecting permissions and schema while preserving the reservation ledger; rotating credentials does not require deleting state.

Rule flags and reservation metadata live in `/data/zeroclaw/agentmail-state.json`, mode 0600, coordinated with management tools through an exclusive file lock. Bodies, subjects and credentials are not stored there. Confirmed run references (`sop`, `run_id`, admission `status`) are retained and exposed in recent diagnostics; use native SOP status for execution outcome. Corrupt state fails closed. The ledger caps unique events at 10,000 and serialized state at 2 MB, whichever is reached first; it never silently evicts reservations. An update exceeding the byte limit fails before writing, preserving the previous readable ledger. A run admitted before a failed metadata write can still be uncertain; inspect native SOP state before any retry. Before archival, disable ingress and preserve the ledger privately; deleting it removes duplicate protection for retained/replayed events. Treat a reserved or uncertain event as potentially executed: inspect actual GitHub/Telegram state before manually requesting work again.

Recent diagnostics distinguish `gateway` failures (HTTP rejection, timeout/transport, invalid response) from `telegram` delivery failures. A `phase` distinguishes request setup/headers, HTTP status handling, and response-body reads. Body-read timeouts and transport interruptions preserve their categories and the already received HTTP status; malformed JSON/schema and oversized bodies have separate safe classifications. `gateway_completed=true` is a legacy field: for new SOP events it means a dispatch response was validated before notification failed, not that the SOP finished; `false` means admission was not fully confirmed, **not** that no actions ran. A successful HTTP response with an unusable body is also uncertain. Older entries have no diagnostic details; the change cannot recover them.

Correlate a supplied event ID with `recent_events.event_ref` using its SHA-256 hex digest; raw event IDs remain only in the private reservation ledger. Do not paste email content or credentials into logs. Before repeating an uncertain review, inspect the PR's actual reviews and SOP runs. Never clear reservations just to make delivery retry.

For an installed review SOP, route step failures to a final diagnostic step using native `on_failure = goto`. Have that step store a minimal, sanitized `daily` memory entry and advance with `status=failed` when earlier work failed; the normal success path should not create a failure record. A crashed/cancelled agent or gateway timeout may prevent that recovery step from running, so adapter diagnostics are still required. Updating an installed SOP is separate from deploying this repository; the launcher does not manage operator SOP definitions.

The ledger is persistent storage, not a backup. Gateway/session storage retains its existing policies for admitted prompts and model results. No deployment or real webhook verification is implied by local fixture tests.
