# AgentMail email events

The optional Rust adapter runs inside the existing agent service. It uses ZeroClaw's authenticated `/webhook?agent=primary`; no extra Railway service or additional channel feature is required. The launcher owns public `PORT` (normally 3888), binds ZeroClaw at `127.0.0.1:3887`, and proxies dashboard, pairing, WebSocket and ACP traffic. Pairing remains required. The gateway admin authentication patch treats forwarded reload and SOP administration as remote requests: native authentication validates the current paired bearer and requires remote administration to be enabled, including after token revocation or policy changes. The proxy forwards these dashboard routes and `/pair/code`, which reports status without revealing a code. Shutdown and pairing-code secret routes stay console-only. Without the opt-in flag, the original direct gateway starts.

## Setup

Set these variables in the **agent Railway service**, never GitHub or Git:

| Variable | Value |
| --- | --- |
| `HEIMDALL_AGENTMAIL_ENABLED` | `1` to run the adapter; absent or `0` starts the direct gateway |
| `HEIMDALL_AGENTMAIL_WEBHOOK_SECRET` | AgentMail's webhook-specific `whsec_...` signing secret, distinct from the API key |
| `HEIMDALL_AGENTMAIL_GATEWAY_TOKEN` | A newly generated random bearer of at least 32 characters; keep private |
| `AGENTMAIL_AUTHORIZATION` | Existing full `Bearer ...` API credential; used for tools and retrieval of omitted bodies |
| `HEIMDALL_AGENTMAIL_FILTERS` | Optional complete JSON array of rules; absent uses the GitHub rule below |

1. Deploy a release containing this feature through the normal release workflow.
2. Use the existing agent HTTPS domain, targeting container port 3888. Generated Railway domains stay platform-managed outside IaC; see [public networking ownership](../.railway/README.md). The authenticated dashboard/gateway share that domain; retain pairing. Keep the executor private.
3. In AgentMail, create an inbox-scoped webhook for `message.received` with URL `https://YOUR_AGENT_DOMAIN/agentmail/events`. Copy its signing secret directly into Railway, never into Telegram or logs. See [webhook registration](https://docs.agentmail.to/webhooks-overview) and [Svix verification](https://docs.agentmail.to/webhook-verification).
4. Configure the variables and deploy the variable change. Internal port 3887 is reserved.
5. Install the intended `github-pr-review` SOP first (through Heimdall or the dashboard). The default rule explicitly asks the primary agent to run it after verifying the live GitHub request. A missing SOP must be reported, not silently replaced.
6. Tell Heimdall “List the email filters,” then “Enable github-review-request.” The management tool arguments are `{"name":"github-review-request","enabled":true}`; disabling uses `false`. All rules initially default to disabled.
7. Verify a real review-request notification produces a Telegram response and unrelated mail does not start a task. These live checks require operator execution.

The adapter adds the internal bearer digest through native `ZEROCLAW_gateway__paired_tokens` environment configuration, preserving explicitly configured clients in that JSON array. If `ZEROCLAW_gateway__webhook_secret` is configured, the adapter also supplies it on internal requests. The bearer and signing secret are never rendered into TOML. Starting with a pre-authorized adapter suppresses the runtime's initial pairing-code banner; get a fresh code from the Railway console as the runtime user with `gosu heimdall zeroclaw gateway get-paircode --new --host 127.0.0.1 --port 3887`. Remote pairing uses the same public listener. The launcher's regenerated configuration remains authoritative after restart.

## Rules

One stable `/agentmail/events` endpoint handles all rules. Adding another sender/content combination means updating the complete `HEIMDALL_AGENTMAIL_FILTERS` JSON array and redeploying, without a new route, folder or matcher implementation. Only this rule is supplied initially:

```json
[{"name":"github-review-request","sender":"notifications@github.com","contains":"requested your review on:","sop":"github-pr-review"}]
```

Up to 16 rules are accepted. Exact `name`, `sender` and nonempty `contains` fields are required; optional `sop` names an installed procedure (1–128 ASCII letters, digits, underscores or hyphens). Rules without `sop` retain notification behavior. Existing custom arrays must explicitly add `sop` to enable that assignment; duplicate names or unknown fields stop startup. Sender is an exact case-insensitive mailbox match, including a conventional display-name form. Quoted display names can contain commas, semicolons and escaped quotes; multiple mailboxes, unclosed quotes and CR/LF are rejected. `contains` is a case-insensitive literal substring of the subject or extracted plain-text body, falling back to plain text when extraction is absent. Quoted messages can produce false positives; the admitted agent is instructed to check current GitHub review state before acting. Email content never grants operator authority to change code or send email.

`mail__filters` lists configuration, optional SOP assignments, aggregate state and the ten most recent event diagnostics; `mail__set_enabled` changes one existing rule when an operator asks. Flags persist on `/data` across deployments. New/renamed rule IDs start disabled; removed rules are inactive. Enabling does not backfill old mail. Disabling does not cancel an already admitted task. These tools cannot configure Railway credentials or public routing.

## Admission and delivery

- Verify the raw-body Svix HMAC and timestamp (five-minute tolerance); accept only `message.received`.
- Disabled, unrelated and duplicate events return 200 without an LLM call. Busy requests return 503 for AgentMail to retry. At most two events process concurrently.
- Webhook bodies are capped at 128 KiB and ten seconds of reading. When AgentMail omits a large body, a sender-matching enabled rule can retrieve the message from the fixed authenticated AgentMail API, with redirects disabled, a ten-second timeout and 1 MiB response cap. Event-provided `body_url` is never followed. Missing credentials or failed/oversized retrieval returns 503 without calling a model. See [payload limits](https://docs.agentmail.to/api-reference/webhooks/events/message-received).
- A match reserves `event_id` durably, returns 202, then submits identifiers, rule IDs and a bounded subject to ZeroClaw in a separate event session. The configured primary provider handles the task independently of Telegram's per-chat model selection. The adapter does not send the email body as a task instruction.
- `/webhook` uses the native long-running request budget (600 seconds by default), instead of the ordinary 30-second control-plane timeout. The adapter waits 630 seconds for that gateway call and bounds Telegram delivery separately to 20 seconds. If overriding the native long-running budget, keep it below the adapter deadline. Authentication, request-size limits, two-job admission and reservation-based duplicate suppression remain enforced. The adapter sends the final response as plain text to `TELEGRAM_OWNER_ID`, capped at 4,000 characters. The destination is fixed by the operator, never the sender.
- After reservation there is **no automatic replay**. Crashes, ambiguous timeouts and failed Telegram delivery can leave incomplete events, but redelivery cannot repeat credentialed agent actions. Failures record an uncertain outcome with a fixed diagnostic category, stage and observed HTTP status; raw errors and response bodies are never recorded. The agent is prompted to execute and advance the assigned SOP through its steps, honoring approval gates. This is agent-driven SOP execution, not a native webhook-trigger binding or a guarantee that every admitted task finishes. This is duplicate prevention, not guaranteed delivery.

## State and recovery

Rule flags and reservation metadata live in `/data/zeroclaw/agentmail-state.json`, mode 0600, coordinated with management tools through an exclusive file lock. Bodies, subjects and credentials are not stored there. Corrupt state fails closed. The ledger caps unique events at 10,000 and never silently evicts reservations. Before archival, disable ingress and preserve the ledger privately; deleting it removes duplicate protection for retained/replayed events. Treat a reserved or uncertain event as potentially executed: inspect actual GitHub/Telegram state before manually requesting work again.

Recent diagnostics distinguish `gateway` failures (HTTP rejection, timeout/transport, invalid response) from `telegram` delivery failures. A `phase` distinguishes request setup/headers, HTTP status handling, and response-body reads. Body-read timeouts and transport interruptions preserve their categories and the already received HTTP status; malformed JSON/schema and oversized bodies have separate safe classifications. `gateway_completed=true` means the adapter received a valid final gateway result before delivery failed; `false` means completion was not confirmed, **not** that no actions ran. A successful HTTP response with an unusable body is also uncertain. Older entries have no diagnostic details; the change cannot recover them.

Correlate a supplied event ID with `recent_events.event_ref` using its SHA-256 hex digest; raw event IDs remain only in the private reservation ledger. Do not paste email content or credentials into logs. Before repeating an uncertain review, inspect the PR's actual reviews and SOP runs. Never clear reservations just to make delivery retry.

For an installed review SOP, route step failures to a final diagnostic step using native `on_failure = goto`. Have that step store a minimal, sanitized `daily` memory entry and advance with `status=failed` when earlier work failed; the normal success path should not create a failure record. A crashed/cancelled agent or gateway timeout may prevent that recovery step from running, so adapter diagnostics are still required. Updating an installed SOP is separate from deploying this repository; the launcher does not manage operator SOP definitions.

The ledger is persistent storage, not a backup. Gateway/session storage retains its existing policies for admitted prompts and model results. No deployment or real webhook verification is implied by local fixture tests.
