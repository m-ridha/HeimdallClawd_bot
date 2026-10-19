# Notice

The image builds ZeroClaw v0.8.6, revision c473221313341a853e04122f93bb9bd26b5e6c47, from <https://github.com/zeroclaw-labs/zeroclaw>.

ZeroClaw is distributed under MIT OR Apache-2.0. Both upstream license files accompany this deployment recipe. The local patch rejects Telegram updates outside private chats before callback processing or media downloads. The runtime otherwise uses upstream subscription authentication, provider integration, channel routing, SQLite sessions, and MCP client.

The HTTP origin patch binds Heimdall named GitHub and AgentMail secrets to their exact HTTPS origins before credential resolution; other public HTTP requests remain available.

The gateway admin authentication patch treats requests marked by a trusted proxy as remote for daemon reload and SOP administration, requiring the native paired bearer and remote-admin policy instead of granting loopback CLI authority. Genuine host CLI requests keep their existing behavior; pairing-code generation still requires the host admin secret.

ZeroClaw v0.8.6 supplies canonical typed transcription initialization and crate-local WIT interface paths upstream; no local patch is needed for either. The retained patches enforce private Telegram ingress, exact HTTP secret origins, sanitized HTTP diagnostics, isolated native children, remote skill execution, paired proxy administration and actionable SOP workshop guidance.

The SOP workshop guidance patch corrects action-specific parameter descriptions and reports the required Markdown step syntax without echoing the rejected procedure. Proposal validation and explicit application remain upstream behavior.
