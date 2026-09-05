import { describe, expect, it, vi } from "vitest";

import {
  type AdapterCapabilities,
  type ChannelAdapter,
  type MessageReceipt,
  type ReplyTarget,
  type SessionConversation,
} from "../../adapters/types.js";
import { type Logger } from "../../adapters/types.js";
import { type EngineEvent } from "../../engine/runtime.js";
import { type EngineWsClient } from "../../engine/wsClient.js";
import { createApprovalTurnHandler } from "../approvalTurnHandler.js";
import { createDefaultTurnHandler } from "../defaultTurnHandler.js";
import {
  formatTaskCompleted,
  formatTaskFailed,
  formatTaskStarted,
  titleFromText,
  withTaskLifecycle,
} from "../lifecycle.js";

const noopLogger: Logger = { debug() {}, info() {}, warn() {}, error() {} };

function mockAdapter(streaming: "none" | "partial" = "none"): ChannelAdapter & {
  sent: string[];
} {
  const sent: string[] = [];
  const capabilities: AdapterCapabilities = {
    threading: false,
    pairing: false,
    approvalButtons: false,
    streaming,
  };
  return {
    platform: "telegram",
    capabilities,
    start: async () => {},
    stop: async () => {},
    onMessage: () => {},
    send: async (_t: ReplyTarget, text: string): Promise<MessageReceipt> => {
      sent.push(text);
      return { messageId: `m${sent.length}` };
    },
    requestApproval: async () => ({ requestId: "r", choice: "allow" }),
    resolveSessionConversation: (id: string): SessionConversation => ({ baseChatId: id }),
    sent,
  } as unknown as ChannelAdapter & { sent: string[] };
}

function mockClient(events: EngineEvent[]): EngineWsClient {
  return {
    connect: async () => {},
    close: async () => {},
    cancel: () => {},
    runQuery: async function* (): AsyncGenerator<EngineEvent> {
      for (const e of events) yield e;
    },
  } as unknown as EngineWsClient;
}

describe("formatters", () => {
  it("titleFromText collapses whitespace and truncates", () => {
    expect(titleFromText("  帮我\n写   周报 ")).toBe("帮我 写 周报");
    expect(titleFromText("x".repeat(40))).toBe(`${"x".repeat(30)}…`);
  });
  it("lifecycle stamps carry the title / error", () => {
    expect(formatTaskStarted("写周报")).toBe("🚀 已开始任务：写周报");
    expect(formatTaskCompleted("写周报")).toBe("✅ 任务完成：写周报");
    expect(formatTaskFailed("写周报", "boom")).toBe("❌ 任务失败：写周报\nboom");
  });
});

const base = {
  inbound: {
    platform: "telegram" as const,
    chatId: "c1",
    senderId: "u1",
    senderName: "ed",
    text: "帮我写周报",
    timestamp: 1,
    isDirect: true,
  },
  replyTarget: { platform: "telegram" as const, chatId: "c1" },
  logger: noopLogger,
};

describe("withTaskLifecycle over the default handler", () => {
  it("pushes start → answer → completed on success", async () => {
    const adapter = mockAdapter();
    const handler = withTaskLifecycle(createDefaultTurnHandler());
    await handler.handle({
      ...base,
      client: mockClient([
        { type: "text", content: "好的" },
        { type: "completed", model: "m" },
      ]),
      adapter,
    });
    expect(adapter.sent).toEqual([
      "🚀 已开始任务：帮我写周报",
      "好的",
      "✅ 任务完成：帮我写周报",
    ]);
  });

  it("pushes failed (no completed) when the engine fails", async () => {
    const adapter = mockAdapter();
    const handler = withTaskLifecycle(createDefaultTurnHandler());
    await handler.handle({
      ...base,
      client: mockClient([{ type: "failed", error: "engine exploded" }]),
      adapter,
    });
    expect(adapter.sent).toEqual([
      "🚀 已开始任务：帮我写周报",
      "⚠️ engine exploded",
      "❌ 任务失败：帮我写周报\nengine exploded",
    ]);
  });

  it("pushes started but not completed when the turn is cancelled", async () => {
    const adapter = mockAdapter();
    const handler = withTaskLifecycle(createDefaultTurnHandler());
    await handler.handle({
      ...base,
      client: mockClient([{ type: "cancelled" }]),
      adapter,
    });
    expect(adapter.sent).toEqual(["🚀 已开始任务：帮我写周报"]);
  });

  it("a push failure never fails the turn", async () => {
    const adapter = mockAdapter();
    vi.spyOn(adapter, "send").mockRejectedValueOnce(new Error("network down"));
    const handler = withTaskLifecycle(createDefaultTurnHandler());
    await expect(
      handler.handle({
        ...base,
        client: mockClient([{ type: "completed", model: "m" }]),
        adapter,
      }),
    ).resolves.toBeUndefined();
    // The failed start push was logged (not thrown); the turn still completed
    // and its completion stamp went out.
    expect(adapter.sent).toEqual(["✅ 任务完成：帮我写周报"]);
  });
});

describe("withTaskLifecycle over the approval handler", () => {
  it("still reports lifecycle around approval flows", async () => {
    const adapter = mockAdapter();
    const handler = withTaskLifecycle(
      createApprovalTurnHandler({ engineBaseUrl: "http://mock", fetchImpl: async () => ({ ok: true } as Response) }),
    );
    await handler.handle({
      ...base,
      client: mockClient([{ type: "completed", model: "m" }]),
      adapter,
    });
    expect(adapter.sent).toEqual([
      "🚀 已开始任务：帮我写周报",
      "✅ 任务完成：帮我写周报",
    ]);
  });
});
