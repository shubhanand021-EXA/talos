# Channels — Command Talos from Telegram & Discord

The gateway daemon owns two external surfaces — **Telegram** (bot long-polling)
and **Discord** (incoming-webhook sends + bot-REST polling). Every inbound
message becomes a Supervisor task that passes the **PolicyBroker** before
anything executes. Reads run immediately; writes need the desktop; high-risk
actions park in `waiting_policy` with a _"Task requires desktop approval.
Opening HITL gate."_ reply; wildcard-destructive commands (`delete_all`,
`rm -rf /`, `format`, …) are **denied by the broker's default-deny** and never
execute.

## Telegram setup

1. Message **@BotFather** on Telegram: `/newbot`, pick a name and username.
   Copy the HTTP API token (`123456:ABC-DEF…`).
2. Open Talos → Settings → **Channels** → paste the token under _telegram_ →
   **Link bot**. The token goes straight into the OS keychain
   (`com.talos.desktop` / `channel.telegram.token`); it is never written to
   disk, logged, or returned by any API.
3. The daemon long-polls `getUpdates` every 30s and answers in the same chat.
   **Allowlist first:** only chats listed in `.talos/channels.json`
   (`telegram.allowedChatIds`) are served — everyone else is dropped silently
   (no oracle for scanners).
4. Start chatting: `summarize the ledger` executes and replies with the
   evidence-backed result as markdown.

## Discord setup

1. **Send-only (simplest):** Server Settings → Integrations → Webhooks →
   New Webhook → Copy Webhook URL. Paste it under _discord_ → **Link bot**.
2. **Two-way:** [Discord Developer Portal](https://discord.com/developers) →
   New Application → Bot → copy the token (no privileged intents needed) →
   OAuth2 URL Generator (`bot` scope, Send Messages + Read Message History) →
   invite to your server. Paste the token; set the channel id in
   `.talos/channels.json` (`discord.allowedChannelIds` + `channelId`).
3. The daemon sends via the webhook (or bot-REST fallback) and polls
   `GET /channels/{id}/messages` with the bot token — no gateway websocket,
   no message-content intent required.

## `.talos/channels.json` (secret-free)

```json
{
  "telegram": {
    "enabled": true,
    "allowedChatIds": ["123456789"],
    "pollIntervalSecs": 30
  },
  "discord": {
    "enabled": false,
    "allowedChannelIds": [],
    "channelId": null,
    "pollIntervalSecs": 30
  }
}
```

Secrets live in the keychain only. Unlinking drops them and detaches the
adapter immediately.

## Security model

- **Fail closed:** no credential, no adapter; unknown senders get no reply;
  oversize/empty messages are rejected before parsing.
- **Broker-authoritative routing:** every message is classified
  (`crates/channels/src/classifier.rs`) and evaluated. Only `Allow` executes.
  `RequireHumanReview` → task parks in `waiting_policy` + HITL reply.
  `Deny` / `RequireEvidence` / `RequireCompensation` → task marked `failed`
  - denial reply with the broker's reason. Nothing from chat can skip the
    queue, the DAG planner, or the ledger — receipts and verdicts are journaled
    like any local task (`channel.received`, `channel.fired`, `channel.denied`,
    `channel.policy_required`).
- **Wildcard-destructive guard:** unscoped destructive phrases resolve to a
  least-privilege shape the broker's Rule 6 default-deny rejects.
