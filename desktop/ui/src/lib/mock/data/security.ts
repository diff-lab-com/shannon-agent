// Demo twin of the backend's prompt-injection scanner
// (desktop/src/extensions/security.rs `scan_prompt_injection`): same
// PATTERNS, same classifier, same payload shape. Mock-mode SecurityBadges
// and the Dangerous-install gate therefore behave like the real backend —
// including the seeded Dangerous skill (data/catalog.ts) whose description
// trips `system_override` + `data_exfil`.
//
// Keep in sync with the Rust PATTERNS table: the strings are *detection*
// substrings scanned case-insensitively; they are never executed.

export interface MockInjectionMatch {
  pattern: string
  matched_substring: string
  category: string
}

export interface MockInjectionReport {
  risk: 'clean' | 'suspicious' | 'dangerous'
  matches: MockInjectionMatch[]
  match_count: number
}

const PATTERNS: ReadonlyArray<readonly [string, string]> = [
  // System override — clearest attack signal.
  ['ignore previous instructions', 'system_override'],
  ['ignore all previous instructions', 'system_override'],
  ['ignore the previous', 'system_override'],
  ['disregard previous', 'system_override'],
  ['forget your instructions', 'system_override'],
  ['you are not an ai', 'system_override'],
  ['you are now a', 'system_override'],
  ['new instructions:', 'system_override'],
  ['system prompt:', 'system_override'],
  // Tool abuse — installer tries to run dangerous ops.
  ['rm -rf', 'tool_abuse'],
  ['sudo ', 'tool_abuse'],
  ['curl ', 'data_exfil'],
  ['wget ', 'data_exfil'],
  ['exec(', 'tool_abuse'],
  ['eval(', 'tool_abuse'],
  // Data exfiltration — outbound leak patterns.
  ["send the user's", 'data_exfil'],
  ['upload the contents', 'data_exfil'],
  ['post the api key', 'data_exfil'],
  ['base64 encode the', 'data_exfil'],
  // Guard bypass.
  ['bypass safety', 'ignore_guard'],
  ['bypass the safety', 'ignore_guard'],
  ["don't ask for permission", 'ignore_guard'],
  ['do not ask for permission', 'ignore_guard'],
]

export function scanPromptInjectionDemo(text: string): MockInjectionReport {
  const lower = (text ?? '').toLowerCase()
  const matches: MockInjectionMatch[] = []
  const categories = new Set<string>()
  for (const [pattern, category] of PATTERNS) {
    const idx = lower.indexOf(pattern)
    if (idx !== -1) {
      matches.push({
        pattern,
        // Same rule as the Rust scanner: echo the original-casing substring.
        matched_substring: text.slice(idx, idx + pattern.length),
        category,
      })
      categories.add(category)
    }
  }
  let risk: MockInjectionReport['risk'] = 'clean'
  if (matches.length > 0) {
    risk = categories.has('system_override') || matches.length >= 3 ? 'dangerous' : 'suspicious'
  }
  return { risk, matches, match_count: matches.length }
}
