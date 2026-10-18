# Notice

The image builds ZeroClaw v0.8.6, revision c473221313341a853e04122f93bb9bd26b5e6c47, from <https://github.com/zeroclaw-labs/zeroclaw>.

ZeroClaw is distributed under MIT OR Apache-2.0. Both upstream license files accompany this deployment recipe. The local patch rejects Telegram updates outside private chats before callback processing or media downloads. The runtime otherwise uses upstream subscription authentication, provider integration, channel routing, SQLite sessions, and MCP client.

The HTTP origin patch binds Heimdall named GitHub and AgentMail secrets to their exact HTTPS origins before credential resolution; other public HTTP requests remain available.

ZeroClaw v0.8.6 supplies canonical typed transcription initialization and crate-local WIT interface paths upstream; no local patch is needed for either. The retained patches enforce private Telegram ingress, exact HTTP secret origins, sanitized HTTP diagnostics, isolated native children and remote skill execution.
