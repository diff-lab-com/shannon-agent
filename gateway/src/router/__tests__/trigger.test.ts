import { describe, expect, it } from "vitest";

import { type NormalizedInbound, type Platform } from "../../adapters/types.js";
import {
  DEFAULT_TRIGGER_PREFIX,
  evaluateTrigger,
  resolveTriggerConfig,
  stripPlatformMention,
  stripPrefix,
} from "../trigger.js";

/** Build an inbound with sensible defaults; overrides via `over`. */
function inbound(platform: Platform, text: string, over: Partial<NormalizedInbound> = {}): NormalizedInbound {
  return {
    platform,
    chatId: "CHAT1",
    senderId: "U1",
    senderName: "ed",
    text,
    timestamp: 1700000000000,
    ...over,
  };
}

describe("stripPrefix", () => {
  it("strips the /shannon prefix and leading whitespace", () => {
    expect(stripPrefix("/shannon 帮我写周报", DEFAULT_TRIGGER_PREFIX)).toBe("帮我写周报");
    expect(stripPrefix("  /shannon\tdeploy", DEFAULT_TRIGGER_PREFIX)).toBe("deploy");
  });
  it("returns null when the prefix is absent or bare", () => {
    expect(stripPrefix("hello", DEFAULT_TRIGGER_PREFIX)).toBeNull();
    expect(stripPrefix("/shannon", DEFAULT_TRIGGER_PREFIX)).toBeNull();
    expect(stripPrefix("/shannon   ", DEFAULT_TRIGGER_PREFIX)).toBeNull();
    expect(stripPrefix("/shannonfoo", DEFAULT_TRIGGER_PREFIX)).toBeNull();
  });
});

describe("stripPlatformMention (per-platform native signals)", () => {
  it("slack: <@U…> tokens trigger and are stripped", () => {
    const out = stripPlatformMention("slack", "<@U123> 帮我查一下", null, []);
    expect(out).toBe("帮我查一下");
  });
  it("slack: no mention token → null", () => {
    expect(stripPlatformMention("slack", "普通消息", null, [])).toBeNull();
  });
  it("telegram: mention entities trigger and are stripped", () => {
    const raw = {
      message: {
        text: "@shannon_bot do the thing",
        entities: [{ type: "mention", offset: 0, length: 12 }],
      },
    };
    expect(stripPlatformMention("telegram", "@shannon_bot do the thing", raw, [])).toBe(
      "do the thing",
    );
  });
  it("telegram: non-mention entities (hashtag) do not trigger", () => {
    const raw = {
      message: { text: "#tag hello", entities: [{ type: "hashtag", offset: 0, length: 4 }] },
    };
    expect(stripPlatformMention("telegram", "#tag hello", raw, [])).toBeNull();
  });
  it("discord: mentions[] + <@!id> tokens trigger and are stripped", () => {
    const raw = { mentions: [{ id: "987", username: "shannon" }] };
    expect(stripPlatformMention("discord", "<@987> run tests", raw, [])).toBe("run tests");
    expect(stripPlatformMention("discord", "<@!987> run tests", raw, [])).toBe("run tests");
  });
  it("discord: no mentions → null", () => {
    expect(stripPlatformMention("discord", "run tests", { mentions: [] }, [])).toBeNull();
  });
  it("feishu: mentions[].key placeholders are stripped", () => {
    const raw = { event: { message: { mentions: [{ key: "@_user_1" }] } } };
    expect(stripPlatformMention("feishu", "@_user_1 帮我排期", raw, [])).toBe("帮我排期");
  });
  it("dingtalk: group delivery implies the bot was @mentioned (always trigger)", () => {
    expect(stripPlatformMention("dingtalk", "帮我订会议室", null, [])).toBe("帮我订会议室");
  });
  it("platforms without a mention signal (matrix) → null", () => {
    expect(stripPlatformMention("matrix", "hello", null, [])).toBeNull();
  });
  it("generic mentionNames work on any platform", () => {
    expect(stripPlatformMention("telegram", "@ShannonBot 帮我", null, ["shannonbot"])).toBe(
      "帮我",
    );
  });
});

describe("evaluateTrigger — DM direct response (default on)", () => {
  const platforms: Platform[] = ["telegram", "discord", "slack", "feishu", "dingtalk"];
  for (const p of platforms) {
    it(`${p}: DM answers directly, text untouched`, () => {
      const r = evaluateTrigger(inbound(p, "帮我写周报", { isDirect: true }));
      expect(r).toEqual({ triggered: true, via: "dm", text: "帮我写周报" });
    });
  }
  it("dmDirect:false disables DM response", () => {
    const r = evaluateTrigger(inbound("slack", "hi", { isDirect: true }), { dmDirect: false });
    expect(r).toEqual({ triggered: false, via: "ignored", text: "hi" });
  });
});

describe("evaluateTrigger — group chat needs mention or /shannon prefix (default)", () => {
  const cases: Array<[Platform, string, unknown]> = [
    ["telegram", "@shannon_bot deploy", { message: { entities: [{ type: "mention", offset: 0, length: 12 }] } }],
    ["discord", "<@987> deploy", { mentions: [{ id: "987" }] }],
    ["slack", "<@U123> deploy", null],
    ["feishu", "@_user_1 deploy", { event: { message: { mentions: [{ key: "@_user_1" }] } } }],
    ["dingtalk", "deploy", null],
  ];
  for (const [platform, text, raw] of cases) {
    it(`${platform}: @mention triggers and strips the mention`, () => {
      const r = evaluateTrigger(inbound(platform, text, { raw }));
      expect(r.triggered).toBe(true);
      expect(r.via).toBe("mention");
      expect(r.text).toBe("deploy");
    });
  }

  // Platforms with a real "unaddressed group message" case: every platform
  // except dingtalk (whose custom robot only delivers @mention traffic at all).
  const ignorable: Platform[] = ["telegram", "discord", "slack", "feishu"];
  for (const p of ignorable) {
    it(`${p}: non-mention group chat is ignored`, () => {
      const r = evaluateTrigger(inbound(p, "随便聊聊", { raw: null }));
      expect(r).toEqual({ triggered: false, via: "ignored", text: "随便聊聊" });
    });
    it(`${p}: /shannon prefix triggers and is stripped`, () => {
      const r = evaluateTrigger(inbound(p, "/shannon 部署到测试环境", { raw: null }));
      expect(r).toEqual({ triggered: true, via: "prefix", text: "部署到测试环境" });
    });
  }
  it("dingtalk: any group text implies an @mention (platform fires only on mention)", () => {
    expect(evaluateTrigger(inbound("dingtalk", "部署", { raw: null }))).toEqual({
      triggered: true,
      via: "mention",
      text: "部署",
    });
  });
});

describe("evaluateTrigger — config overrides", () => {
  it("groupMode any answers every group message", () => {
    const r = evaluateTrigger(inbound("slack", "hello", { raw: null }), { groupMode: "any" });
    expect(r).toEqual({ triggered: true, via: "group-any", text: "hello" });
  });
  it("custom prefix", () => {
    const r = evaluateTrigger(inbound("slack", "/shy run", { raw: null }), { prefix: "/shy" });
    expect(r).toEqual({ triggered: true, via: "prefix", text: "run" });
    expect(evaluateTrigger(inbound("slack", "/shannon run", { raw: null }), { prefix: "/shy" }).triggered).toBe(
      false,
    );
  });
});

describe("resolveTriggerConfig", () => {
  it("reads options.trigger and tolerates garbage", () => {
    expect(resolveTriggerConfig(undefined)).toEqual({});
    expect(resolveTriggerConfig({ trigger: "nope" })).toEqual({});
    expect(resolveTriggerConfig({ trigger: { groupMode: "any", dmDirect: false, prefix: "/x", mentionNames: ["bot", 3, ""] } })).toEqual({
      groupMode: "any",
      dmDirect: false,
      prefix: "/x",
      mentionNames: ["bot"],
    });
  });
});
