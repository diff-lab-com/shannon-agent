/**
 * Shared free-text allow/deny recognition for buttonless approval channels
 * (P2-1). Extracted from the DingTalk adapter so the mobile dispatch channel
 * resolves approval replies with the exact same token set instead of growing a
 * second dialect. The DingTalk adapter re-exports this as `parseChoice`, so
 * its behavior and tests are unchanged.
 */

const ALLOW_TOKENS: readonly string[] = ["allow", "yes", "y", "同意", "允许", "✅"];
const DENY_TOKENS: readonly string[] = ["deny", "no", "n", "拒绝", "否", "❌"];

/** Recognize a free-text allow/deny reply. Returns null when the text is neither. */
export function parseApprovalChoice(text: string): "allow" | "deny" | null {
  const t = text.trim().toLowerCase();
  if (ALLOW_TOKENS.includes(t)) return "allow";
  if (DENY_TOKENS.includes(t)) return "deny";
  return null;
}
