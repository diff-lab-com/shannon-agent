# IM Channels

Dispatch work to your machine from the chat apps you already use. Shannon currently supports five inbound channels:

- **Telegram**
- **Discord**
- **Slack**
- **飞书 (Feishu)**
- **钉钉 (DingTalk)**

## Setup

In the desktop app: **Settings → Social Connections** → pick a channel and paste the bot token/credentials from the platform's developer console. Detailed per-platform walkthroughs live in [`docs/integrations/im-channels.md`](https://github.com/diff-lab-com/shannon-agent/blob/dev/docs/integrations/im-channels.md) in the repository.

## Routing rules

- **Direct messages** are answered directly.
- **Group chats** require an @mention or a `/shannon` prefix (configurable) so the bot doesn't react to every message.
- Inbound messages become tasks in your local engine; progress (started / completed / failed) is pushed back to the original chat.

## Security baseline

- Credentials are stored **only in the OS keyring** — never in chat context, never in session logs.
- Webhook payloads are HMAC-SHA256 signed and verified.
- Sensitive operations triggered from IM fall back to an approval card **in the IM thread** — the agent asks before it acts.
- Inbound messages pass the same prompt-injection scanning as local input.
