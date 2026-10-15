# Project walkthrough

## How the pieces connect

Telegram messages reach ZeroClaw inside the agent service. The Rust launcher prepares private configuration, checks authentication and starts ZeroClaw. It also hosts a narrow stdio MCP skill installer. ZeroClaw calls the selected model provider. For workspace file or command operations, ZeroClaw calls the separate executor over authenticated private MCP. The executor performs the operation in its own persistent workspace and returns the result.

Changing the model keeps the same agent and isolated tool boundary. Conversation state lives on the agent's `/data` volume. User tool files live on the executor's `/data/workspace` volume; neither belongs in Git.

## Folders

| Folder | Purpose |
| --- | --- |
| `.github/workflows/` | The version-tag release workflow: build/test, deploy the two services, then publish a GitHub release. GitHub environment secrets/variables supply deployment credentials and IDs. |
| `deploy/bot/` | Docker build recipes,  ZeroClaw configuration template, pinned source/binary checksums, maintained Telegram, remote-skill and WASM interface patches, and required upstream licenses. ZeroClaw source is downloaded and verified during a build; a full checkout is not maintained here. |
| `deploy/tests/` | Integration checks against the actual pinned ZeroClaw configuration library, including environment-only keys and secret exclusion from saved config. |
| `launcher/src/` | The small Rust startup wrapper: requires the Telegram owner ID, validates allowed users and private key settings, writes non-secret config, checks ChatGPT auth, starts ZeroClaw, and exposes restricted skill install/list tools. It also exposes restricted Codespaces tools using the GitHub CLI; remote commands run in allowlisted GitHub sandboxes. It does not implement the language model engine. |
| `launcher/tests/` | Launcher/configuration checks, including root Linux initialization and rejection of missing owner/authentication. |
| `executor/src/` | The Rust private MCP service and Linux-restricted helpers implementing `bash`, `file_read`, and `file_write`. |
| `executor/tests/` | Executor API, filesystem, timeout/cancellation, and Linux isolation checks. |
| `executor/contract/` | A test using ZeroClaw's real MCP client against the executor. |
| `.railway/` | Python IaC for both services and persistent volumes; pinned Python SDK. Runtime variables remain exclusively on Railway. |
| `tools/` | Release packaging/deployment scripts and the clean-tree verification script. |
| `tools/tests/` | Release-helper checks that prevent an unrelated successful deployment from masking a failed release deployment. |
| `docs/` | Operations, release setup, this walkthrough and observed verification results. |

`Cargo.toml` in each Rust crate lists its dependencies; `Cargo.lock` pins their versions. Generated `target/` directories contain local build outputs and are ignored by Git. `.git/` stores Git history and authorship metadata. `.gitignore` excludes credentials, database files and build outputs; `.dockerignore` excludes unnecessary/private paths from Docker uploads.

## Use another model on OpenRouter

Your existing OpenRouter key can access additional models allowed by that account. You do not need a new deployment merely to select another model on the existing provider slot.

1. Copy the exact model ID from that model's OpenRouter page. Use the full ID, including its provider prefix. Check its price and tool-calling support; the free router's price does not apply to a different selected model.
2. In Telegram, send `/models openrouter.free`. Here `openrouter.free` is the configured provider slot's name.
3. Send `/model EXACT_OPENROUTER_MODEL_ID`, replacing the placeholder with the copied ID. This changes the model on that slot; the slot name does not force every explicitly selected model to be free.
4. Send `/model` to check the current provider/model.
5. Ask it to read a harmless existing workspace file to verify tools work with the selected model.
6. Use `/model free` to return to `openrouter/free`, or `/model codex` to return to the ChatGPT subscription route.

These are session selections. Check `/model` again after a restart.

## Add a different provider through Railway CLI

This example adds an Anthropic provider alias named `custom`. It does not require editing Rust or committing credentials. Other supported families use the same schema pattern, with their own exact model IDs and credentials.

1. Open a terminal in this repository. Run `railway link` if needed; choose the HeimdallClawd project and production environment. Confirm the agent service is named `heimdallclawd`. Log in with `railway login` only if the CLI is signed out.
2. Obtain the provider's API key and exact supported model ID. A ChatGPT Plus sign-in does not supply an Anthropic API key.
3. Set the model, replacing `YOUR_MODEL_ID`:

   ```sh
   railway variable set --service heimdallclawd --skip-deploys \
     ZEROCLAW_providers__models__anthropic__custom__model=YOUR_MODEL_ID
   ```

4. Run this command, paste the key at the hidden prompt, and press Enter:

   ```sh
   printf 'Paste provider API key (hidden), then press Enter: '
   read -r -s provider_key
   printf '\n'
   ```

5. Store it privately, then clear the shell variable:

   ```sh
   printf '%s' "$provider_key" | railway variable set \
     ZEROCLAW_providers__models__anthropic__custom__api_key \
     --stdin --skip-deploys --service heimdallclawd
   unset provider_key
   ```

6. Apply both settings with one deployment:

   ```sh
   railway redeploy --service heimdallclawd --yes
   ```

7. Wait for the agent deployment to report healthy. Then send these as separate Telegram messages, substituting the exact same model ID:

   ```text
   /models anthropic.custom
   /model YOUR_MODEL_ID
   /model
   ```

   `/models` changes the provider only, so set the model before sending a normal prompt.

8. Verify a chat reply and a harmless remote file read. `/model codex` returns to the subscription route in one command.

The pattern is `ZEROCLAW_providers__models__PROVIDER__ALIAS__FIELD`. Keep provider/alias tokens lowercase. `api_key`, `model`, and (when required) `uri` are common fields. Unknown schema paths stop startup, so use a family supported by the pinned ZeroClaw release. Add these settings to the agent service, not the executor. Provider keys belong in Railway variables and are not saved in the repository or TOML. API usage is billed by the provider separately from Railway infrastructure.

For Ollama, use a supported `ollama` alias with an exact local model ID and an always-on reachable endpoint instead of an API key. An Ollama server on your Mac cannot answer while the Mac is off. Its networking needs separate verification.

Expanded features and activation limits are described in [capabilities](capabilities.md). Railway infrastructure migration status is documented in [Railway IaC](railway-iac.md).
