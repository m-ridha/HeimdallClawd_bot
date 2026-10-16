# Notice

The image builds ZeroClaw v0.8.5, revision cb2b20a9fe59004b7ef03e123942d31f29236d15, from <https://github.com/zeroclaw-labs/zeroclaw>.

ZeroClaw is distributed under MIT OR Apache-2.0. Both upstream license files accompany this deployment recipe. The local patch rejects Telegram updates outside private chats before callback processing or media downloads. The runtime otherwise uses upstream subscription authentication, provider integration, channel routing, SQLite sessions, and MCP client.

The HTTP origin patch binds Heimdall named GitHub and AgentMail secrets to their exact HTTPS origins before credential resolution; other public HTTP requests remain available.
