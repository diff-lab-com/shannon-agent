// Starter prompts shared by the chat-canvas welcome card (WelcomeState) and
// the sidebar zero-session guide card (U7). One source of example copy, two
// presentations — the surfaces can't drift apart. The sidebar shows the
// first two only; the canvas card shows all four.
//
// `prompt` is the English source text; `promptKey` resolves the localized
// variant (review §5: the four prompts used to render English in every
// locale). Keys ship in en + zh-CN; other locales fall back to en.

export interface WelcomeExample {
  icon: string
  titleKey: string
  prompt: string
  promptKey: string
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
  },
  {
    icon: 'code',
    titleKey: 'welcomeState.example.code',
    prompt: 'Build a REST API endpoint in Rust that accepts JSON, validates input, and returns a typed response.',
    promptKey: 'welcomeState.example.code.prompt',
  },
]
