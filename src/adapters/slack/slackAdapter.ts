import { createServer, type IncomingMessage, type Server, type ServerResponse } from "node:http";
import { createHmac, timingSafeEqual } from "node:crypto";

import {
  type AdapterContext,
  type ApprovalDecision,
  type ApprovalReq,
  type ChannelAdapter,
  type MessageReceipt,
  type NormalizedInbound,
  type ReplyTarget,
  type SendOpts,
} from "../types.js";
import { type AdapterConfig } from "../../config/types.js";

/**
 * Slack adapter (Events API + Web API).
 *
 * Inbound: Slack pushes **Events API** POSTs to a callback URL. The body is
 * either a `url_verification` challenge (echo `challenge`) or an
 * `event_callback` whose `event` is a `message` (text). Button clicks arrive
 * as **interactive** payloads — form-encoded `payload=<json>` whose parsed
 * `type` is `block_actions`. Every request is signed: the
 * `X-Slack-Signature` header is `v0=` + HMAC-SHA256(signing_secret,
 * `v0:{X-Slack-Request-Timestamp}:{rawBody}`), checked timing-safe; the
 * timestamp is rejected past a 5 min replay window.
 *
 * Outbound: `POST {apiBase}/chat.postMessage` with
 * `Authorization: Bearer <bot token>`; `thread_ts` carries thread continuity
 * (F4). Streaming edits use `chat.update`.
 *
 * Sessions: a Slack channel is the base session; a thread is its own session
 * (`threading = true` → `sessionKeyOf` appends `thread_ts`). `target.threadId`
 * is the thread root ts.
 *
 * Approval: Slack **Block Kit** action buttons (`approvalButtons = true`). The
 * request renders as an actions block; a click's `action_id` resolves the
 * pending promise. Engine 300s timeout → Deny.
 *
 * Docs: https://api.slack.com/apis/connections/events-api
 *       https://api.slack.com/methods/chat.postMessage
 */

// ── signature (pure, round-trip tested) ────────────────────────────────

/** Compute the Slack `v0=` signature for (timestamp, rawBody, signingSecret). */
export function computeSlackSignature(
  timestamp: string,
  rawBody: string,
  signingSecret: string,
): string {
  const basestring = `v0:${timestamp}:${rawBody}`;
  return "v0=" + createHmac("sha256", signingSecret).update(basestring).digest("hex");
}

/** Timing-safe verification of an incoming `X-Slack-Signature`. */
export function verifySlackSignature(
  timestamp: string,
  rawBody: string,
  signingSecret: string,
  provided: string,
): boolean {
  const expected = Buffer.from(computeSlackSignature(timestamp, rawBody, signingSecret));
  const actual = Buffer.from(provided);
  if (expected.length !== actual.length) return false;
  return timingSafeEqual(expected, actual);
}

/** Reject timestamps outside a ±window to deter replay. */
export function isFreshTimestamp(
  timestamp: string,
  now: number = Date.now(),
  windowMs: number = 5 * 60_000,
): boolean {
  const ts = Number(timestamp);
  if (!Number.isFinite(ts)) return false;
  return Math.abs(now / 1000 - ts) <= windowMs / 1000;
}

// ── inbound normalization (pure) ───────────────────────────────────────

interface SlackEventMessage {
  type?: string; // "message"
  text?: string;
  channel?: string;
  user?: string;
  ts?: string;
  thread_ts?: string;
  bot_id?: string; // skip bot echoes
  subtype?: string;
}
interface SlackEventCallback {
  type?: string; // "event_callback"
  event?: SlackEventMessage;
}
interface SlackUrlVerification {
  type?: string; // "url_verification"
  challenge?: string;
}
interface SlackBlockAction {
  action_id?: string;
  value?: string;
}
interface SlackInteractivePayload {
  type?: string; // "block_actions"
  actions?: SlackBlockAction[];
  channel?: { id?: string };
  message?: { ts?: string; thread_ts?: string };
  user?: { id?: string };
}

export type SlackInboundResult =
  | { kind: "challenge"; challenge: string }
  | { kind: "message"; message: NormalizedInbound }
  | { kind: "button"; actionId: string; channel?: string; threadTs?: string }
  | { kind: "ignore" };

/**
 * Parse a decoded Slack payload (JSON for events, parsed `payload` for
 * interactive) into a typed inbound result.
 */
export function normalizeSlackEvent(payload: unknown): SlackInboundResult {
  if (typeof payload !== "object" || payload === null) return { kind: "ignore" };

  // Interactive payloads carry their own top-level `type`.
  const interactive = payload as SlackInteractivePayload;
  if (interactive.type === "block_actions") {
    const action = interactive.actions?.[0];
    const actionId = typeof action?.action_id === "string" ? action.action_id : "";
    return {
      kind: "button",
      actionId,
      channel: interactive.channel?.id,
      threadTs: interactive.message?.thread_ts,
    };
  }

  // Events API envelopes.
  const env = payload as SlackUrlVerification & SlackEventCallback;
  if (env.type === "url_verification" && typeof env.challenge === "string") {
    return { kind: "challenge", challenge: env.challenge };
  }
  if (env.type === "event_callback") {
    const ev = env.event;
    if (!ev || ev.type !== "message") return { kind: "ignore" };
    if (ev.bot_id || ev.subtype) return { kind: "ignore" }; // skip bots/aliases
    if (typeof ev.text !== "string" || ev.text.length === 0) return { kind: "ignore" };
    if (!ev.channel || !ev.user || !ev.ts) return { kind: "ignore" };
    const tsNum = Number.parseFloat(ev.ts);
    return {
      kind: "message",
      message: {
        platform: "slack",
        chatId: ev.channel,
        // A thread message carries thread_ts; a top-level message does not.
        threadId: typeof ev.thread_ts === "string" ? ev.thread_ts : ev.ts,
        senderId: ev.user,
        senderName: ev.user,
        text: ev.text,
        timestamp: Number.isFinite(tsNum) ? tsNum * 1000 : Date.now(),
        isDirect: ev.channel.startsWith("D"),
        raw: payload,
      },
    };
  }
  return { kind: "ignore" };
}

// ── outbound request builders (pure) ───────────────────────────────────

export interface SlackSendArgs {
  apiBase: string;
  token: string;
  channel: string;
  text: string;
  /** Thread root ts — keeps replies inside the thread (F4). */
  threadTs?: string;
  /** Block Kit payload (approval buttons). When set, `text` is the fallback. */
  blocks?: unknown[];
}

export interface SlackRequest {
  url: string;
  method: "POST";
  headers: Record<string, string>;
  body: string;
}

export function buildPostMessageRequest(args: SlackSendArgs): SlackRequest {
  const body: Record<string, unknown> = { channel: args.channel, text: args.text };
  if (args.threadTs) body.thread_ts = args.threadTs;
  if (args.blocks && args.blocks.length > 0) body.blocks = args.blocks;
  return {
    url: `${args.apiBase}/chat.postMessage`,
    method: "POST",
    headers: {
      "content-type": "application/json; charset=utf-8",
      authorization: `Bearer ${args.token}`,
    },
    body: JSON.stringify(body),
  };
}

/** Build a chat.update request (streaming edit-in-place). Pure. */
export function buildUpdateRequest(args: {
  apiBase: string;
  token: string;
  channel: string;
  ts: string;
  text: string;
}): SlackRequest {
  return {
    url: `${args.apiBase}/chat.update`,
    method: "POST",
    headers: {
      "content-type": "application/json; charset=utf-8",
      authorization: `Bearer ${args.token}`,
    },
    body: JSON.stringify({ channel: args.channel, ts: args.ts, text: args.text }),
  };
}

export function formatApprovalPrompt(req: ApprovalReq): string {
  const tag = req.isDestructive ? "⚠️ DESTRUCTIVE" : "tool";
  const diff = req.diffPreview ? `\n\n${req.diffPreview}` : "";
  return `🔐 Approval (${tag}): *${escapeMrkdwn(req.toolName)}*\n${escapeMrkdwn(req.description)}${diff}`;
}

/** Block Kit section + actions row with Allow/Deny buttons. action_id encodes choice:id. */
export function buildApprovalBlocks(requestId: string, prompt: string): unknown[] {
  return [
    { type: "section", text: { type: "mrkdwn", text: prompt } },
    {
      type: "actions",
      elements: [
        {
          type: "button",
          action_id: `allow:${requestId}`,
          style: "primary",
          text: { type: "plain_text", text: "✅ Allow" },
        },
        {
          type: "button",
          action_id: `deny:${requestId}`,
          style: "danger",
          text: { type: "plain_text", text: "❌ Deny" },
        },
      ],
    },
  ];
}

/** Parse a button action_id ("allow:<id>" | "deny:<id>"). */
export function parseApprovalAction(
  actionId: string,
): { choice: "allow" | "deny"; requestId: string } | null {
  const idx = actionId.indexOf(":");
  if (idx < 0) return null;
  const choice = actionId.slice(0, idx);
  const requestId = actionId.slice(idx + 1);
  if ((choice !== "allow" && choice !== "deny") || requestId.length === 0) return null;
  return { choice, requestId };
}

function escapeMrkdwn(s: string): string {
  // Escape the Slack mrkdwn special characters so tool names/descriptions
  // can't inject formatting.
  return s.replace(/[*_`>]/g, (m) => `\\${m}`);
}

// ── adapter ────────────────────────────────────────────────────────────

export interface SlackAdapterOptions {
  /** Keyring key for the bot OAuth token (xoxb-…/xapp-…). Default `slack/bot-token`. */
  botTokenKey?: string;
  /** Keyring key for the signing secret. Default `slack/signing-secret`. */
  signingSecretKey?: string;
  /** Web API base. Default `https://slack.com/api`. */
  apiBase?: string;
  /** Local callback port. Default 9873. */
  webhookPort?: number;
  /** Callback path. Default `/slack`. */
  webhookPath?: string;
  /** Injectable for tests; defaults to the global fetch. */
  fetchImpl?: typeof fetch;
  /** When true, skip signature + timestamp checks (dev only). Default false. */
  skipSignatureVerify?: boolean;
}

interface PendingApproval {
  requestId: string;
  resolve: (choice: "allow" | "deny") => void;
}

export function createSlackAdapter(cfg: AdapterConfig, ctx: AdapterContext): ChannelAdapter {
  const o = (cfg.options ?? {}) as SlackAdapterOptions;
  const botTokenKey = o.botTokenKey ?? "slack/bot-token";
  const signingSecretKey = o.signingSecretKey ?? "slack/signing-secret";
  const apiBase = (o.apiBase ?? "https://slack.com/api").replace(/\/+$/, "");
  const webhookPort = o.webhookPort ?? 9873;
  const webhookPath = o.webhookPath ?? "/slack";
  const fetchImpl = o.fetchImpl ?? fetch;
  const skipSig = o.skipSignatureVerify === true;

  let botToken: string | null = null;
  let signingSecret: string | null = null;
  let onMessage: ((m: NormalizedInbound) => void) | null = null;
  let server: Server | null = null;
  const pending = new Map<string, PendingApproval>();

  function resolveButton(actionId: string): void {
    const parsed = parseApprovalAction(actionId);
    if (!parsed) return;
    const p = pending.get(parsed.requestId);
    if (!p) return;
    pending.delete(parsed.requestId);
    p.resolve(parsed.choice);
  }

  async function handlePost(req: IncomingMessage, res: ServerResponse): Promise<void> {
    const raw = await readBody(req);
    if (req.url == null || new URL(req.url, "http://localhost").pathname !== webhookPath) {
      res.statusCode = 404;
      res.end();
      return;
    }
    if (!skipSig) {
      const signature = header(req, "x-slack-signature") ?? "";
      const timestamp = header(req, "x-slack-request-timestamp") ?? "";
      if (!signingSecret || !signature) {
        res.statusCode = 403;
        res.end("bad signature");
        return;
      }
      if (!isFreshTimestamp(timestamp)) {
        res.statusCode = 403;
        res.end("stale timestamp");
        return;
      }
      if (!verifySlackSignature(timestamp, raw, signingSecret, signature)) {
        res.statusCode = 403;
        res.end("bad signature");
        return;
      }
    }

    // Interactive payloads are form-encoded: `payload=<urlencoded JSON>`.
    // Events API bodies are plain JSON.
    let payload: unknown;
    const ct = (header(req, "content-type") ?? "").toLowerCase();
    if (ct.includes("application/x-www-form-urlencoded")) {
      const params = new URLSearchParams(raw);
      const encoded = params.get("payload");
      if (!encoded) {
        res.statusCode = 400;
        res.end("missing payload");
        return;
      }
      try {
        payload = JSON.parse(encoded);
      } catch {
        res.statusCode = 400;
        res.end("bad payload json");
        return;
      }
    } else {
      try {
        payload = JSON.parse(raw);
      } catch {
        res.statusCode = 400;
        res.end("bad json");
        return;
      }
    }

    const result = normalizeSlackEvent(payload);
    switch (result.kind) {
      case "challenge":
        res.statusCode = 200;
        res.end(JSON.stringify({ challenge: result.challenge }));
        return;
      case "message":
        onMessage?.(result.message);
        break;
      case "button":
        if (result.actionId) resolveButton(result.actionId);
        break;
      default:
        break;
    }
    res.statusCode = 200;
    res.end();
  }

  async function doSend(
    target: ReplyTarget,
    text: string,
    blocks?: unknown[],
  ): Promise<MessageReceipt> {
    if (!botToken) throw new Error("slack: start() not called or token missing");
    const req = buildPostMessageRequest({
      apiBase,
      token: botToken,
      channel: target.chatId,
      text,
      threadTs: target.threadId,
      blocks,
    });
    const res = await fetchImpl(req.url, { method: req.method, headers: req.headers, body: req.body });
    if (!res.ok) {
      const detail = await res.text().catch(() => "");
      throw new Error(`slack postMessage failed: HTTP ${res.status} ${detail}`);
    }
    const data = (await res.json()) as { ok?: boolean; error?: string; ts?: string; channel?: string };
    if (!data.ok) {
      throw new Error(`slack postMessage error: ${data.error ?? "unknown"}`);
    }
    return { messageId: data.ts ?? "", threadId: target.threadId ?? data.channel };
  }

  async function doEdit(target: ReplyTarget, text: string, messageId: string): Promise<MessageReceipt> {
    if (!botToken) throw new Error("slack: start() not called or token missing");
    const req = buildUpdateRequest({
      apiBase,
      token: botToken,
      channel: target.chatId,
      ts: messageId,
      text,
    });
    const res = await fetchImpl(req.url, { method: req.method, headers: req.headers, body: req.body });
    if (!res.ok) {
      const detail = await res.text().catch(() => "");
      throw new Error(`slack update failed: HTTP ${res.status} ${detail}`);
    }
    const data = (await res.json()) as { ok?: boolean; error?: string };
    if (!data.ok) {
      throw new Error(`slack update error: ${data.error ?? "unknown"}`);
    }
    return { messageId, editedAt: Date.now() };
  }

  return {
    platform: "slack",
    capabilities: {
      threading: true, // Slack native threads → sessionKeyOf appends thread_ts
      pairing: false,
      approvalButtons: true, // Block Kit action buttons
      streaming: "partial",
    },
    async start(): Promise<void> {
      botToken = await ctx.getSecret(botTokenKey);
      if (!botToken) throw new Error(`slack: secret "${botTokenKey}" missing from keyring`);
      signingSecret = await ctx.getSecret(signingSecretKey);
      if (!signingSecret) throw new Error(`slack: secret "${signingSecretKey}" missing from keyring`);
      server = createServer((req, res) => {
        if (req.method === "POST") {
          void handlePost(req, res).catch((err) => {
            ctx.logger.warn(`slack POST failed: ${(err as Error).message}`);
            try {
              res.statusCode = 500;
              res.end();
            } catch {
              // ignore — response already flushed
            }
          });
        } else {
          res.statusCode = 405;
          res.end();
        }
      });
      await new Promise<void>((resolve, reject) => {
        server!.once("error", reject);
        server!.listen(webhookPort, () => {
          server!.off("error", reject);
          resolve();
        });
      });
      ctx.logger.info(`slack callback listening on :${webhookPort}${webhookPath}`);
    },
    async stop(): Promise<void> {
      if (server) {
        await new Promise<void>((resolve) => server!.close(() => resolve()));
        server = null;
      }
    },
    onMessage(handler): void {
      onMessage = handler;
    },
    async send(target, text, opts?: SendOpts): Promise<MessageReceipt> {
      if (opts?.editMessageId) {
        return await doEdit(target, text, opts.editMessageId);
      }
      return await doSend(target, text);
    },
    async requestApproval(target, req): Promise<ApprovalDecision> {
      const prompt = formatApprovalPrompt(req);
      const blocks = buildApprovalBlocks(req.requestId, prompt);
      await doSend(target, prompt, blocks);
      return await new Promise<ApprovalDecision>((resolve) => {
        pending.set(req.requestId, {
          requestId: req.requestId,
          resolve: (choice) => resolve({ requestId: req.requestId, choice }),
        });
        // Engine 300s approval timeout → Deny if no button click arrives.
      });
    },
    resolveSessionConversation(rawId): { baseChatId: string } {
      return { baseChatId: rawId };
    },
  };
}

function readBody(req: IncomingMessage): Promise<string> {
  return new Promise((resolve, reject) => {
    let data = "";
    req.setEncoding("utf8");
    req.on("data", (chunk: string) => {
      data += chunk;
    });
    req.on("end", () => resolve(data));
    req.on("error", reject);
  });
}

function header(req: IncomingMessage, name: string): string | undefined {
  const v = req.headers[name.toLowerCase()];
  return Array.isArray(v) ? v[0] : v;
}
