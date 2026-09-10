# Mobile Dispatch

Approve and dispatch tasks from your phone while the engine keeps running on your machine.

## Pair

1. Make sure the engine is reachable: `shannon serve` (binds locally, port `33420`).
2. In the desktop app, open the pairing page and **scan the QR code** with your phone.
3. Your phone opens the local companion web app over your LAN.

## What you can do from the phone

- **See running tasks** and their progress (streamed live).
- **Approve or deny** permission requests — the same approval cards you'd see on the desktop.
- **Dispatch new tasks** to your machine, optionally with attachments.

## Security notes

- Pairing is per-device and revocable from the desktop app.
- The companion app talks to your engine directly on your LAN — no cloud relay holds your session.
- Sensitive actions still require explicit approval, wherever you approve them from.
