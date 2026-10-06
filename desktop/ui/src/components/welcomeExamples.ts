// Starter prompts shared by the chat-canvas welcome card (WelcomeState) and
// the sidebar zero-session guide card (U7). One source of example copy, two
// presentations — the surfaces can't drift apart. The sidebar shows the
// first two only; the canvas card shows all four.
//
// `prompt` is the English source text; `promptKey` resolves the localized
// variant (review §5: the four prompts used to render English in every
// locale). Keys ship in en + zh-CN; other locales fall back to en.
//
// D5 方案① (主动任务推荐): `coding` marks the coding-oriented cards —
// they only render on the canvas welcome card when the workspace probe
// (`detect_workspace_markers`) reports a code-project marker. Office cards
// always show. The sidebar zero-session card is untouched (static first
// two, which are office cards).

export interface WelcomeExample {
  icon: string
  titleKey: string
  prompt: string
  promptKey: string
  /** Coding-oriented example: gated on a workspace code marker (D5 方案①). */
  coding?: boolean
}

export const WELCOME_EXAMPLES: WelcomeExample[] = [
  {
    icon: 'mail',
    titleKey: 'welcomeState.example.email',
    prompt: 'Draft a friendly follow-up email to a candidate who went silent after the onsite. Keep it short and warm.',
    promptKey: 'welcomeState.example.email.prompt',
  },
  {
    icon: 'summarize',
    titleKey: 'welcomeState.example.summarize',
    prompt: 'Summarize the document below into 5 bullet points and a one-paragraph TL;DR for a busy exec.',
    promptKey: 'welcomeState.example.summarize.prompt',
  },
  {
    icon: 'travel_explore',
    titleKey: 'welcomeState.example.research',
    prompt: 'Research the top 3 Rust web frameworks in 2026. Compare them on ecosystem, async support, and learning curve. Cite sources.',
    promptKey: 'welcomeState.example.research.prompt',
    coding: true,
  },
  {
    icon: 'code',
    titleKey: 'welcomeState.example.code',
    prompt: 'Build a REST API endpoint in Rust that accepts JSON, validates input, and returns a typed response.',
    promptKey: 'welcomeState.example.code.prompt',
    coding: true,
  },
]

/**
 * D5 方案① 「换一批」 — in-place Fisher-Yates shuffle of a COPY of the
 * deck (lodash is not a dependency, and this is six lines). Takes the
 * random source as a parameter (default Math.random) so tests can spy it
 * for a deterministic permutation. Never mutates the input array.
 */
export function shuffleExamples<T>(deck: readonly T[], random: () => number = Math.random): T[] {
  const out = [...deck]
  for (let i = out.length - 1; i > 0; i--) {
    const j = Math.floor(random() * (i + 1))
    ;[out[i], out[j]] = [out[j]!, out[i]!]
  }
  return out
}
