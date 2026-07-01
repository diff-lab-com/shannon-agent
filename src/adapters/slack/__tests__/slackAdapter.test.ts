import { describe, expect, it } from "vitest";
import { createHmac } from "node:crypto";

import { type Logger } from "../../types.js";
import { type AdapterConfig } from "../../../config/types.js";
import { assertAdapterContract } from "../../contract.js";
import {
  buildApprovalBlocks,
  buildPostMessageRequest,
  buildUpdateRequest,
  computeSlackSignature,
  createSlackAdapter,
  formatApprovalPrompt,
  isFreshTimestamp,
  normalizeSlackEvent,
  parseApprovalAction,
  verifySlackSignature,
} from "../slackAdapter.js";

const noopLogger: Logger = { debug() {}, info() {}, warn() {}, error() {} };

describe("verifySlackSignature", () => {
  it("accepts a correct v0 signature and rejects a bad one", () => {
    const secret = "shhh";
    const body = JSON.stringify({ type: "event_callback" });
    const ts = "1700000000";
    const good = computeSlackSignature(ts, body, secret);
    expect(verifySlackSignature(ts, body, secret, good)).toBe(true);
    expect(verifySlackSignature(ts, body, secret, "v0=deadbeef")).toBe(false);
    expect(verifySlackSignature(ts, body, "wrong", good)).toBe(false);
  });

  it("computeSlackSignature matches the documented v0 scheme", () => {
    const secret = "8f742231b10ee888ff9dd81f";
    const ts = "1531420618";
    const body = "token=xxx&team=T&api_app_id=A";
    // Recompute independently of the helper to guard against a regression.
    const expected =
      "v0=" + createHmac("sha256", secret).update(`v0:${ts}:${body}`).digest("hex");
    expect(computeSlackSignature(ts, body, secret)).toBe(expected);
  });
});

describe("isFreshTimestamp", () => {
  it("accepts a recent timestamp and rejects stale/invalid", () => {
    const now = 1_700_000_000_000;
    expect(isFreshTimestamp(String(Math.floor(now / 1000)), now)).toBe(true);
    expect(isFreshTimestamp(String(Math.floor(now / 1000) - 600), now)).toBe(false); // 10 min old
    expect(isFreshTimestamp("notanumber", now)).toBe(false);
  });
});

describe("normalizeSlackEvent", () => {
  it("echoes a url_verification challenge", () => {
    const r = normalizeSlackEvent({ type: "url_verification", challenge: "CHK" });
    expect(r).toEqual({ kind: "challenge", challenge: "CHK" });
  });

  it("normalizes a channel message event", () => {
    const r = normalizeSlackEvent({
      type: "event_callback",
      event: { type: "message", text: "hi", channel: "C1", user: "U1", ts: "1700000000.001" },
    });
    expect(r.kind).toBe("message");
    if (r.kind !== "message") throw new Error("expected message");
    const msg = r.message;
    expect(msg.chatId).toBe("C1");
    expect(msg.threadId).toBe("1700000000.001"); // top-level → own ts
    expect(msg.isDirect).toBe(false);
    expect(msg.senderId).toBe("U1");
  });

  it("uses thread_ts when present (thread message)", () => {
    const r = normalizeSlackEvent({
      type: "event_callback",
      event: {
        type: "message",
        text: "in thread",
        channel: "C1",
        user: "U1",
        ts: "1700000000.005",
        thread_ts: "1700000000.001",
      },
    });
    expect((r as { message: { threadId: string } }).message.threadId).toBe("1700000000.001");
  });

  it("flags a DM channel (D...) as direct", () => {
    const r = normalizeSlackEvent({
      type: "event_callback",
      event: { type: "message", text: "dm", channel: "D123", user: "U1", ts: "1.1" },
    });
    expect((r as { message: { isDirect: boolean } }).message.isDirect).toBe(true);
  });

  it("skips bot echoes, subtypes, and non-message events", () => {
    expect(
      normalizeSlackEvent({
        type: "event_callback",
        event: { type: "message", text: "x", channel: "C1", user: "U1", ts: "1", bot_id: "B1" },
      }).kind,
    ).toBe("ignore");
    expect(
      normalizeSlackEvent({
        type: "event_callback",
        event: { type: "message", text: "x", channel: "C1", user: "U1", ts: "1", subtype: "message_changed" },
      }).kind,
    ).toBe("ignore");
    expect(normalizeSlackEvent({ type: "event_callback", event: { type: "reaction_added" } }).kind).toBe(
      "ignore",
    );
  });

  it("extracts a block_actions button click", () => {
    const r = normalizeSlackEvent({
      type: "block_actions",
      actions: [{ action_id: "allow:r9", value: "allow" }],
      channel: { id: "C1" },
      message: { ts: "1.1", thread_ts: "1.0" },
    });
    expect(r).toEqual({ kind: "button", actionId: "allow:r9", channel: "C1", threadTs: "1.0" });
  });

  it("ignores malformed payloads", () => {
    expect(normalizeSlackEvent(null).kind).toBe("ignore");
    expect(normalizeSlackEvent({}).kind).toBe("ignore");
  });
});

describe("buildPostMessageRequest / buildUpdateRequest", () => {
  it("posts a message with thread continuity + auth header", () => {
    const req = buildPostMessageRequest({
      apiBase: "https://slack.com/api",
      token: "xoxb-1",
      channel: "C1",
      text: "hi",
      threadTs: "1.0",
    });
    expect(req.url).toBe("https://slack.com/api/chat.postMessage");
    expect(req.headers.authorization).toBe("Bearer xoxb-1");
    expect(JSON.parse(req.body)).toMatchObject({ channel: "C1", text: "hi", thread_ts: "1.0" });
  });

  it("includes Block Kit blocks when supplied", () => {
    const req = buildPostMessageRequest({
      apiBase: "https://slack.com/api",
      token: "t",
      channel: "C1",
      text: "hi",
      blocks: [{ type: "section" }],
    });
    expect(JSON.parse(req.body)).toMatchObject({ blocks: [{ type: "section" }] });
  });

  it("builds a chat.update request", () => {
    const req = buildUpdateRequest({
      apiBase: "https://slack.com/api",
      token: "t",
      channel: "C1",
      ts: "1.1",
      text: "edited",
    });
    expect(req.url).toBe("https://slack.com/api/chat.update");
    expect(JSON.parse(req.body)).toMatchObject({ channel: "C1", ts: "1.1", text: "edited" });
  });
});

describe("approval helpers", () => {
  it("formatApprovalPrompt tags destructive + escapes mrkdwn", () => {
    const p = formatApprovalPrompt({
      requestId: "r1",
      toolName: "*rm*",
      toolInput: {},
      description: "del",
      isDestructive: true,
      diffPreview: null,
    });
    expect(p).toContain("DESTRUCTIVE");
    expect(p).toContain("\\*rm\\*");
  });

  it("buildApprovalBlocks encodes choice + id in action_ids", () => {
    const blocks = buildApprovalBlocks("req-9", "prompt") as Array<{
      type: string;
      elements?: Array<{ action_id: string }>;
    }>;
    const actions = blocks.find((b) => b.type === "actions")?.elements ?? [];
    expect(actions.map((e) => e.action_id).sort()).toEqual(["allow:req-9", "deny:req-9"]);
  });

  it("parseApprovalAction round-trips and rejects junk", () => {
    expect(parseApprovalAction("allow:req-9")).toEqual({ choice: "allow", requestId: "req-9" });
    expect(parseApprovalAction("deny:req-9")).toEqual({ choice: "deny", requestId: "req-9" });
    expect(parseApprovalAction("bogus")).toBeNull();
    expect(parseApprovalAction("allow:")).toBeNull();
  });
});

describe("createSlackAdapter contract", () => {
  it("passes assertAdapterContract", () => {
    const cfg: AdapterConfig = { platform: "slack", enabled: true };
    const adapter = createSlackAdapter(cfg, { logger: noopLogger, getSecret: async () => null });
    expect(() => assertAdapterContract(adapter)).not.toThrow();
    expect(adapter.platform).toBe("slack");
    expect(adapter.capabilities.threading).toBe(true);
    expect(adapter.capabilities.approvalButtons).toBe(true);
    expect(adapter.capabilities.streaming).toBe("partial");
  });

  it("fails fast when the bot token secret is missing at start", async () => {
    const adapter = createSlackAdapter(
      { platform: "slack", enabled: true },
      { logger: noopLogger, getSecret: async () => null },
    );
    const startCtx = { logger: noopLogger, getSecret: async () => null };
    await expect(adapter.start(startCtx)).rejects.toThrow(/slack\/bot-token.*missing/);
  });

  it("fails fast when the signing secret is missing at start", async () => {
    const adapter = createSlackAdapter(
      { platform: "slack", enabled: true },
      { logger: noopLogger, getSecret: async (k: string) => (k === "slack/bot-token" ? "tok" : null) },
    );
    const startCtx = { logger: noopLogger, getSecret: async () => null };
    await expect(adapter.start(startCtx)).rejects.toThrow(/slack\/signing-secret.*missing/);
  });
});
