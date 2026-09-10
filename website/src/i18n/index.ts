export type Lang = 'en' | 'zh';

const translations = {
  en: {
    nav: {
      features: 'Features',
      security: 'Security',
      docs: 'Docs',
      download: 'Download',
      github: 'GitHub',
      getStarted: 'Get Started',
    },
    hero: {
      badge: 'Open-source · Apache-2.0 · Rust · Claude Code compatible',
      title: ['Open source. Total control. ', { text: 'Keys never leave', italic: true }, ' your machine.'],
      subtitle: 'The open-source AI agent workspace — terminal to desktop, one Rust engine, any LLM provider. Every action replayable, every cost visible, and your API keys stay on your machine.',
      costHint: 'BYOK pay-per-use · session budget caps · no telemetry by default',
      getStarted: 'Get Started',
      starGithub: 'Star on GitHub',
    },
    terminal: {
      lines: [
        { type: 'prompt', text: '$ shannon' },
        { type: 'output', text: 'shannon v0.11 · Rust · any-model engine · local-first' },
        { type: 'prompt', text: '> /goal make CI green --budget 5' },
        { type: 'tool', text: '✓ Analyzed 3 failing workflows (read-only)' },
        { type: 'tool', text: '✓ Fixed matrix timeout in .github/workflows/ci.yml' },
        { type: 'tool', text: '✓ Pushed branch · spend $1.12 / $5.00' },
        { type: 'output', text: 'GOAL_COMPLETE — every step appended to events.jsonl' },
        { type: 'prompt', text: '$ shannon trace replay --session last' },
        { type: 'output', text: '▶ replaying 14 events · secrets redacted · nothing left your machine' },
      ],
    },
    trust: {
      title: 'Two commitments behind every feature',
      cards: [
        {
          title: 'Open source, total control',
          desc: 'An agent you can actually audit — code, behavior, and cost.',
          bullets: [
            'Apache-2.0, every line auditable · 11,752 automated tests',
            'Event-sourced sessions: shannon trace show / replay / diff',
            'Budget caps, context breakdown, cache hit-rate visibility',
            'Any model, no lock-in · Claude Code ecosystem compatible',
          ],
        },
        {
          title: 'Keys never leave your machine',
          desc: 'Your credentials talk directly to the provider you choose. No middleman.',
          bullets: [
            'secret-guard: outbound messages redacted before they reach the model — byte-stable, so prompt caching keeps hitting',
            'Integration credentials only in the OS keyring',
            'Landlock / Seatbelt sandbox + permission profiles + prompt-injection scanning',
            'No telemetry by default · local voice input sends audio nowhere',
          ],
        },
      ],
    },
    features: {
      title: 'Features',
      items: [
        { num: '01', title: 'Any LLM Provider', desc: 'BYOK: Anthropic, OpenAI, DeepSeek, Z.ai (GLM), Ollama, or any OpenAI-compatible endpoint — one config, direct connection, no middleman server.' },
        { num: '02', title: 'Open Source & Auditable', desc: 'Apache-2.0 with event-sourced sessions. shannon trace show / replay / diff reconstructs exactly what your agent did — a dashcam for AI agents.' },
        { num: '03', title: 'Secret & Key Safety', desc: 'secret-guard plugin redacts secrets from outbound messages (cache-safe), OS keyring storage, Landlock/Seatbelt sandbox, prompt-injection scanning, no telemetry.' },
        { num: '04', title: 'Autonomous Goals', desc: '/goal hands your agent an objective, not a prompt: budget caps, anti-spin guards, stall strikes, automatic resumption, results in a triage inbox.' },
        { num: '05', title: 'Multi-Agent Teams', desc: 'OS-process agents on isolated git worktrees. /batch runs best-of-N attempts in parallel — compare diffs side by side, adopt the winner.' },
        { num: '06', title: 'Native Desktop App', desc: 'Tauri 2 + React 19, not Electron. Simple mode for everyone, advanced mode with multi-panel workspace, integrated terminal, and memory graph.' },
        { num: '07', title: 'IM & Mobile Dispatch', desc: 'Dispatch tasks from Telegram, Discord, Slack, Feishu, or DingTalk; approve from your phone via QR pairing. Results push back to the original chat.' },
        { num: '08', title: 'Engineering Discipline', desc: '11,752+ automated tests, zero clippy warnings, cargo-deny and semver gates in CI. Published eval harness on SWE-bench Verified and Terminal-Bench.' },
      ],
    },
    comparison: {
      title: 'Why Shannon?',
      items: [
        { value: '11,752+', label: 'Automated Tests' },
        { value: '20', label: 'Workspace Crates' },
        { value: '4', label: 'Surfaces, One Engine' },
        { value: '5', label: 'IM Channels Inbound' },
      ],
      rows: [
        { feature: 'License', shannon: 'Apache-2.0, fully open', other: 'Proprietary' },
        { feature: 'Execution', shannon: 'Local-first, your machine', other: 'Cloud VMs & sandboxes' },
        { feature: 'Your keys', shannon: 'Direct to provider + outbound secret redaction', other: 'Vendor-managed cloud credential stores' },
        { feature: 'Cost', shannon: 'Pay-per-use, budget caps, visible breakdown', other: 'Subscription quotas & credits' },
        { feature: 'Auditability', shannon: 'Event-sourced sessions, trace replay/diff', other: 'Often a black box' },
        { feature: 'Test coverage', shannon: '11,752 tests, zero clippy warnings', other: 'n/a (closed source)' },
      ],
      footnote: 'As of September 2026. Cloud agents compared: Claude Code, Codex, Grok Bot. Sources: docs/competitive-research-2026-09.md in the repository.',
    },
    cta: {
      title: 'Own your AI workspace.',
      button: 'Get Started',
    },
    footer: {
      copyright: '\u00a9 2026 Shannon Contributors.',
      license: 'Apache-2.0',
    },
  },
  zh: {
    nav: {
      features: '功能',
      security: '安全',
      docs: '文档',
      download: '下载',
      github: 'GitHub',
      getStarted: '开始使用',
    },
    hero: {
      badge: '开源 · Apache-2.0 · Rust · 兼容 Claude Code',
      title: ['完全开源，尽在掌控；', { text: '密钥不出门', italic: true }, '，数据不搬家。'],
      subtitle: '开源 AI agent 工作台——从终端到桌面，一个 Rust 引擎，任意大模型。每一步可回放，每一分成本可见，API 密钥永不离开你的电脑。',
      costHint: 'BYOK 按量付费 · 会话预算上限 · 默认零遥测',
      getStarted: '开始使用',
      starGithub: 'GitHub 加星',
    },
    terminal: {
      lines: [
        { type: 'prompt', text: '$ shannon' },
        { type: 'output', text: 'shannon v0.11 · Rust · 任意模型引擎 · 本地优先' },
        { type: 'prompt', text: '> /goal 让 CI 变绿 --budget 5' },
        { type: 'tool', text: '✓ 分析 3 条失败的流水线（只读）' },
        { type: 'tool', text: '✓ 修复 .github/workflows/ci.yml 矩阵超时' },
        { type: 'tool', text: '✓ 推送分支 · 已花费 $1.12 / $5.00' },
        { type: 'output', text: 'GOAL_COMPLETE —— 每一步都已追加到 events.jsonl' },
        { type: 'prompt', text: '$ shannon trace replay --session last' },
        { type: 'output', text: '▶ 回放 14 个事件 · secret 已脱敏 · 无任何数据离开你的电脑' },
      ],
    },
    trust: {
      title: '每个功能背后的两条承诺',
      cards: [
        {
          title: '开源可控',
          desc: '一个真正可审计的 agent——代码、行为、成本全部在你手里。',
          bullets: [
            'Apache-2.0 每行可审计 · 11,752 个自动化测试',
            '事件溯源会话：shannon trace show / replay / diff',
            '预算上限、上下文拆解、缓存命中率可见',
            '任意模型零锁定 · 兼容 Claude Code 生态',
          ],
        },
        {
          title: '密钥不出门',
          desc: '凭据直连你选择的提供商，没有中间服务器。',
          bullets: [
            'secret-guard：消息出站到模型前先脱敏——字节稳定，prompt 缓存照常命中',
            '集成凭据只存 OS keyring',
            'Landlock / Seatbelt 沙箱 + 权限配置 + 提示注入扫描',
            '默认零遥测 · 本地语音输入音频零出站',
          ],
        },
      ],
    },
    features: {
      title: '功能特性',
      items: [
        { num: '01', title: '任意大模型（BYOK）', desc: 'Anthropic、OpenAI、DeepSeek、智谱 GLM、Ollama 或任何 OpenAI 兼容端点——一份配置、直连提供商、没有中间服务器。' },
        { num: '02', title: '开源可审计', desc: 'Apache-2.0 + 事件溯源会话。shannon trace show / replay / diff 完整还原 agent 干的每一步——agent 的行车记录仪。' },
        { num: '03', title: '密钥与 Secret 安全', desc: 'secret-guard 插件出站脱敏（不烧缓存）、OS keyring 存储、Landlock/Seatbelt 沙箱、提示注入扫描、零遥测。' },
        { num: '04', title: '自主目标', desc: '/goal 给 agent 派目标而不只是 prompt：预算上限、anti-spin 守卫、阻塞退避、自动续跑，结果进收件箱。' },
        { num: '05', title: '多 Agent 团队', desc: '进程级 agent 跑在隔离的 git worktree 上。/batch 并行 best-of-N——并排对比 diff，采纳最优。' },
        { num: '06', title: '原生桌面应用', desc: 'Tauri 2 + React 19，而非 Electron。Simple 模式人人会用，Advanced 模式带多面板工作区、集成终端与记忆图谱。' },
        { num: '07', title: 'IM 与手机派活', desc: '从 Telegram、Discord、Slack、飞书、钉钉派任务；扫码配对后在手机上审批。结果回推原会话。' },
        { num: '08', title: '工程纪律', desc: '11,752+ 个自动化测试、零 clippy 告警、CI 中 cargo-deny 与 semver 门禁。公开 SWE-bench Verified 与 Terminal-Bench 评测管线。' },
      ],
    },
    comparison: {
      title: '为什么选择 Shannon？',
      items: [
        { value: '11,752+', label: '自动化测试' },
        { value: '20', label: 'Workspace Crate' },
        { value: '4', label: '形态，一个引擎' },
        { value: '5', label: 'IM 入站渠道' },
      ],
      rows: [
        { feature: '许可', shannon: 'Apache-2.0 完全开源', other: '闭源' },
        { feature: '执行位置', shannon: '本地优先，你自己的电脑', other: '云 VM 与云沙箱' },
        { feature: '你的密钥', shannon: '直连提供商 + 出站 secret 脱敏', other: '厂商托管的云凭据' },
        { feature: '成本', shannon: '按量付费、预算上限、拆解可见', other: '订阅额度与 credits' },
        { feature: '可审计性', shannon: '事件溯源会话，trace 回放/diff', other: '多为黑盒' },
        { feature: '测试覆盖', shannon: '11,752 个测试，零 clippy 告警', other: '不适用（闭源）' },
      ],
      footnote: '截至 2026 年 9 月。云端 agent 对比对象：Claude Code、Codex、Grok Bot。来源见仓库 docs/competitive-research-2026-09.md。',
    },
    cta: {
      title: '把 AI 工作台装回自己电脑。',
      button: '开始使用',
    },
    footer: {
      copyright: '\u00a9 2026 Shannon 贡献者。',
      license: 'Apache-2.0',
    },
  },
} as const;

export type Translations = typeof translations.en;
export type TranslationKey = keyof typeof translations;

export function getTranslations(lang: Lang): Translations {
  return translations[lang];
}

export function detectLang(): Lang {
  if (typeof navigator !== 'undefined' && navigator.language.startsWith('zh')) {
    return 'zh';
  }
  return 'en';
}

export function getStoredLang(): Lang {
  if (typeof localStorage === 'undefined') return detectLang();
  const stored = localStorage.getItem('shannon-lang');
  if (stored === 'zh' || stored === 'en') return stored;
  return detectLang();
}

export function setStoredLang(lang: Lang): void {
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem('shannon-lang', lang);
  }
}

/** Base URL with trailing slash */
export const BASE = (import.meta.env.BASE_URL as string).replace(/\/?$/, '/');
