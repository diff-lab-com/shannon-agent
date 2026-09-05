import { type NormalizedInbound, type Platform } from "../adapters/types.js";
import { type AdapterTriggerConfig } from "../config/types.js";

/**
 * Inbound trigger gating (P1-4).
 *
 * Decides whether a NormalizedInbound arms a task on the engine, and rewrites
 * the text so the trigger syntax (`/shannon` prefix, @mention tokens) never
 * reaches the engine prompt.
 *
 * v1 default policy (per plan P1-4):
 *   - DM / private chat → respond directly (`dmDirect`, default on);
 *   - group chat → require a platform-native @mention of the bot OR a
 *     `/shannon` prefix (`groupMode: "mentionOrPrefix"`);
 *   - per-platform overrides live in `AdapterConfig.options.trigger`.
 *
 * Mention detection is per-platform because each platform surfaces mentions
 * differently (Slack `<@U…>` tokens, Telegram message entities, Discord
 * `mentions` + `<@!id>` tokens, Feishu `mentions[].key` placeholders,
 * DingTalk only delivers group messages that @mention the robot at all).
 * Platforms without native mention info (matrix) still work via the prefix.
 */

/** Prefix that arms a group message when there is no mention. */
export const DEFAULT_TRIGGER_PREFIX = "/shannon";

/** How a message qualified (diagnostics + tests). */
export type TriggerVia = "dm" | "mention" | "prefix" | "group-any" | "ignored";

export interface TriggerResult {
  triggered: boolean;
  via: TriggerVia;
  /** Text to hand the engine: trigger syntax stripped. Unchanged when ignored. */
  text: string;
}

/** Extract the optional `options.trigger` blob, tolerating wrong shapes. */
export function resolveTriggerConfig(options?: Record<string, unknown>): AdapterTriggerConfig {
  const raw = options?.trigger;
  if (typeof raw !== "object" || raw === null) return {};
  const t = raw as Record<string, unknown>;
  return {
    groupMode: t.groupMode === "any" ? "any" : t.groupMode === "mentionOrPrefix" ? "mentionOrPrefix" : undefined,
    dmDirect: typeof t.dmDirect === "boolean" ? t.dmDirect : undefined,
    prefix: typeof t.prefix === "string" && t.prefix.length > 0 ? t.prefix : undefined,
    mentionNames: Array.isArray(t.mentionNames)
      ? t.mentionNames.filter((n): n is string => typeof n === "string" && n.length > 0)
      : undefined,
  };
}

/** Strip the trigger prefix. Returns the remainder, or null when absent/empty. */
export function stripPrefix(text: string, prefix: string): string | null {
  const trimmed = text.replace(/^\s+/, "");
  if (!trimmed.startsWith(prefix)) return null;
  const after = trimmed.slice(prefix.length);
  // Word boundary: `/shannonfoo` is a different command, not a trigger.
  if (after.length > 0 && !/^\s/.test(after)) return null;
  const rest = after.replace(/^\s+/, "");
  // A bare `/shannon` with nothing after it has no task content — ignore.
  return rest.length > 0 ? rest : null;
}

/** Remove every `@name` token from `mentionNames` found in the text. */
function stripNamedMention(text: string, mentionNames: readonly string[]): string | null {
  if (mentionNames.length === 0) return null;
  let cleaned = text;
  let matched = false;
  for (const name of mentionNames) {
    const re = new RegExp(`@${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}`, "gi");
    if (re.test(cleaned)) {
      matched = true;
      cleaned = cleaned.replace(re, "");
    }
  }
  if (!matched) return null;
  const rest = cleaned.replace(/\s+/g, " ").trim();
  return rest.length > 0 ? rest : null;
}

interface TgEntity {
  type?: string;
  offset?: number;
  length?: number;
}

/** Delete the (offset, length) ranges — Telegram offsets are UTF-16 code units. */
function stripRanges(text: string, ranges: Array<{ offset: number; length: number }>): string {
  const sorted = [...ranges]
    .filter((r) => Number.isInteger(r.offset) && Number.isInteger(r.length) && r.length > 0)
    .sort((a, b) => b.offset - a.offset);
  let out = text;
  for (const r of sorted) {
    out = out.slice(0, r.offset) + out.slice(r.offset + r.length);
  }
  return out.replace(/\s+/g, " ").trim();
}

/**
 * Platform-native bot-mention check. Returns the mention-stripped text, or
 * null when the platform gives no mention signal for this message.
 */
export function stripPlatformMention(
  platform: Platform,
  text: string,
  raw: unknown,
  mentionNames: readonly string[],
): string | null {
  // Generic `@name` override first — works on every platform.
  const named = stripNamedMention(text, mentionNames);
  if (named !== null) return named;

  switch (platform) {
    case "slack": {
      // Slack renders a bot highlight as an <@U…> token anywhere in the text.
      if (!/<@[A-Z0-9]+(\|[^>]+)?>/i.test(text)) return null;
      const rest = text.replace(/<@[A-Z0-9]+\|[^>]+>|<@[A-Z0-9]+>/gi, "").replace(/\s+/g, " ").trim();
      return rest.length > 0 ? rest : null;
    }
    case "telegram": {
      // Telegram marks @username mentions with message entities.
      const msg = (raw as { message?: { entities?: TgEntity[] } } | null)?.message;
      const entities = msg?.entities ?? [];
      const ranges = entities
        .filter((e) => e.type === "mention" || e.type === "text_mention")
        .map((e) => ({ offset: e.offset ?? 0, length: e.length ?? 0 }));
      if (ranges.length === 0) return null;
      const rest = stripRanges(text, ranges);
      return rest.length > 0 ? rest : null;
    }
    case "discord": {
      // Discord carries a mentions[] array and <@!id> tokens in content.
      const mentions = (raw as { mentions?: unknown[] } | null)?.mentions;
      const hasMentions = Array.isArray(mentions) && mentions.length > 0;
      const tokenRe = /<@!?\d+>/g;
      if (!hasMentions && !tokenRe.test(text)) return null;
      const rest = text.replace(tokenRe, "").replace(/\s+/g, " ").trim();
      return rest.length > 0 ? rest : null;
    }
    case "feishu": {
      // Feishu v1 events list mentions as {key: "@_user_1"} placeholders in text.
      const keys = (raw as { event?: { message?: { mentions?: Array<{ key?: unknown }> } } } | null)
        ?.event?.message?.mentions;
      if (!Array.isArray(keys)) return null;
      let cleaned = text;
      let matched = false;
      for (const m of keys) {
        if (typeof m.key !== "string" || m.key.length === 0) continue;
        if (cleaned.includes(m.key)) {
          matched = true;
          cleaned = cleaned.split(m.key).join(" ");
        }
      }
      if (!matched) return null;
      const rest = cleaned.replace(/\s+/g, " ").trim();
      return rest.length > 0 ? rest : null;
    }
    case "dingtalk": {
      // Custom robots only deliver group messages that @mention the bot, so
      // any group text that reached here was addressed to us.
      const rest = text.trim();
      return rest.length > 0 ? rest : null;
    }
    default:
      // wecom/whatsapp are DM-only; matrix/whatsapp have no mention signal —
      // the /shannon prefix is the group trigger there.
      return null;
  }
}

/**
 * Evaluate the trigger policy for one inbound. Pure — the bootstrap calls this
 * between `adapter.onMessage` and `router.handleInbound` and forwards the
 * cleaned text on a positive decision.
 */
export function evaluateTrigger(
  inbound: NormalizedInbound,
  cfg?: AdapterTriggerConfig,
): TriggerResult {
  const groupMode = cfg?.groupMode ?? "mentionOrPrefix";
  const dmDirect = cfg?.dmDirect ?? true;
  const prefix = cfg?.prefix ?? DEFAULT_TRIGGER_PREFIX;
  const mentionNames = cfg?.mentionNames ?? [];

  if (inbound.isDirect) {
    if (!dmDirect) return { triggered: false, via: "ignored", text: inbound.text };
    return { triggered: true, via: "dm", text: inbound.text };
  }

  if (groupMode === "any") {
    return { triggered: true, via: "group-any", text: inbound.text };
  }

  const mentionStripped = stripPlatformMention(inbound.platform, inbound.text, inbound.raw, mentionNames);
  if (mentionStripped !== null) {
    return { triggered: true, via: "mention", text: mentionStripped };
  }

  const prefixStripped = stripPrefix(inbound.text, prefix);
  if (prefixStripped !== null) {
    return { triggered: true, via: "prefix", text: prefixStripped };
  }

  return { triggered: false, via: "ignored", text: inbound.text };
}
