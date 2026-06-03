export type Lang = 'en' | 'zh';

const translations = {
  en: {
    nav: {
      features: 'Features',
      docs: 'Docs',
      download: 'Download',
      github: 'GitHub',
      getStarted: 'Get Started',
    },
    hero: {
      badge: 'v0.1.0 · Rust · Apache-2.0',
      title: ['The ', { text: 'open-source', italic: true }, ' AI coding assistant'],
      subtitle: 'Works with any LLM provider — Anthropic, OpenAI, Ollama, or any OpenAI-compatible endpoint. Written in Rust for performance and safety.',
      installTabs: {
        cargo: 'Cargo',
        curl: 'curl',
        brew: 'Homebrew',
      },
      installCommands: {
        cargo: 'cargo install --git https://github.com/shannon-agent/shannon-code.git',
        curl: 'curl -fsSL https://github.com/shannon-agent/shannon-code/releases/latest/download/install.sh | sh',
        brew: 'brew install shannon-agent/tap/shannon',
      },
      copied: 'Copied!',
      copy: 'Copy',
      starGithub: 'Star on GitHub',
    },
    terminal: {
      lines: [
        { type: 'prompt', text: '$ shannon' },
        { type: 'output', text: 'Shannon Code v0.1.0 · Rust · Multi-provider' },
        { type: 'output', text: 'Connected to claude-sonnet-4-20250514 (Anthropic)' },
        { type: 'prompt', text: '> Fix the auth bug in src/login.rs' },
        { type: 'tool', text: '✓ Read src/login.rs (42 lines)' },
        { type: 'tool', text: '✓ Edit src/login.rs — fix token validation' },
        { type: 'tool', text: '✓ Bash: cargo test — 3/3 passed' },
        { type: 'output', text: 'Fixed. The token expiry check was comparing timestamps in' },
        { type: 'output', text: 'different units (seconds vs milliseconds). Added proper conversion.' },
      ],
    },
    features: {
      title: 'Features',
      items: [
        { num: '01', title: 'Multi-Provider LLM', desc: 'Connect to Anthropic, OpenAI, Ollama, DeepSeek, or any OpenAI-compatible endpoint with a single config.' },
        { num: '02', title: 'MCP Extensions', desc: 'Full Model Context Protocol support — Claude Code compatible. Dynamic tool discovery with fuzzy search.' },
        { num: '03', title: 'Multi-Agent Teams', desc: 'Coordinate multiple AI agents with worktree isolation, per-agent model config, and parallel task dispatch.' },
        { num: '04', title: 'Permission System', desc: 'Rule-based + LLM auto-classifier with 4-tier precedence. Strict, balanced, permissive, or custom profiles.' },
        { num: '05', title: 'Session & Memory', desc: 'Persistent sessions, context compression, memory extraction, checkpoint/undo with diff preview.' },
        { num: '06', title: 'VS Code Extension', desc: 'WebView chat panel with Markdown rendering, diff viewer, and NDJSON subprocess communication.' },
      ],
    },
    comparison: {
      title: 'Why Shannon Code?',
      items: [
        { value: '4+', label: 'LLM Providers' },
        { value: '7,889', label: 'Tests' },
        { value: '0', label: 'Hidden Fees' },
        { value: '12', label: 'Modular Crates' },
      ],
      rows: [
        { feature: 'LLM Providers', shannon: 'Anthropic, OpenAI, Ollama, any compatible', other: 'Single vendor' },
        { feature: 'Cost Transparency', shannon: 'No hidden fees, no cache manipulation', other: 'Dynamic billing headers inflate costs 10-20x' },
        { feature: 'Test Coverage', shannon: '7,889 tests, every file covered', other: 'Often zero tests' },
        { feature: 'Extensibility', shannon: 'MCP protocol, plugins, skills, hooks', other: 'Limited or closed' },
        { feature: 'Code Audit', shannon: 'Every line visible in source', other: 'Black box' },
      ],
    },
    cta: {
      title: 'Get started in 30 seconds',
      command: 'cargo install --git https://github.com/shannon-agent/shannon-code.git',
      button: 'Read the Docs',
    },
    footer: {
      copyright: '© 2026 Shannon Code Contributors.',
      license: 'Apache-2.0',
    },
  },
  zh: {
    nav: {
      features: '功能',
      docs: '文档',
      download: '下载',
      github: 'GitHub',
      getStarted: '开始使用',
    },
    hero: {
      badge: 'v0.1.0 · Rust · Apache-2.0',
      title: ['开源的', { text: 'AI 编程助手', italic: true }, ''],
      subtitle: '支持任何 LLM 提供商 — Anthropic、OpenAI、Ollama 或任何 OpenAI 兼容端点。使用 Rust 编写，高性能且安全。',
      installTabs: {
        cargo: 'Cargo',
        curl: 'curl',
        brew: 'Homebrew',
      },
      installCommands: {
        cargo: 'cargo install --git https://github.com/shannon-agent/shannon-code.git',
        curl: 'curl -fsSL https://github.com/shannon-agent/shannon-code/releases/latest/download/install.sh | sh',
        brew: 'brew install shannon-agent/tap/shannon',
      },
      copied: '已复制！',
      copy: '复制',
      starGithub: 'GitHub 加星',
    },
    terminal: {
      lines: [
        { type: 'prompt', text: '$ shannon' },
        { type: 'output', text: 'Shannon Code v0.1.0 · Rust · 多提供商支持' },
        { type: 'output', text: '已连接 claude-sonnet-4-20250514 (Anthropic)' },
        { type: 'prompt', text: '> 修复 src/login.rs 中的认证 bug' },
        { type: 'tool', text: '✓ 读取 src/login.rs（42 行）' },
        { type: 'tool', text: '✓ 编辑 src/login.rs — 修复 token 验证逻辑' },
        { type: 'tool', text: '✓ Bash: cargo test — 3/3 通过' },
        { type: 'output', text: '已修复。token 过期检查的单位不一致（秒 vs 毫秒），' },
        { type: 'output', text: '已添加正确的单位转换。' },
      ],
    },
    features: {
      title: '功能特性',
      items: [
        { num: '01', title: '多提供商 LLM', desc: '连接 Anthropic、OpenAI、Ollama、DeepSeek 或任何 OpenAI 兼容端点，一个配置即可。' },
        { num: '02', title: 'MCP 扩展', desc: '完整的模型上下文协议支持 — 兼容 Claude Code 生态。动态工具发现与模糊搜索。' },
        { num: '03', title: '多 Agent 团队', desc: '协调多个 AI Agent，工作树隔离，每 Agent 独立模型配置，并行任务调度。' },
        { num: '04', title: '权限系统', desc: '基于规则 + LLM 自动分类，4 级优先级。严格、均衡、宽松或自定义配置。' },
        { num: '05', title: '会话与记忆', desc: '持久化会话、上下文压缩、记忆提取、检查点/撤销与 Diff 预览。' },
        { num: '06', title: 'VS Code 扩展', desc: 'WebView 聊天面板，Markdown 渲染，Diff 查看器，NDJSON 子进程通信。' },
      ],
    },
    comparison: {
      title: '为什么选择 Shannon Code？',
      items: [
        { value: '4+', label: 'LLM 提供商' },
        { value: '7,889', label: '测试' },
        { value: '0', label: '隐藏费用' },
        { value: '12', label: '模块化 Crate' },
      ],
      rows: [
        { feature: 'LLM 提供商', shannon: 'Anthropic、OpenAI、Ollama、任何兼容端点', other: '单一供应商' },
        { feature: '成本透明', shannon: '无隐藏费用，不操纵缓存', other: '动态计费头使成本膨胀 10-20 倍' },
        { feature: '测试覆盖', shannon: '7,889 个测试，每个文件均有覆盖', other: '通常零测试' },
        { feature: '可扩展性', shannon: 'MCP 协议、插件、技能、钩子', other: '有限或封闭' },
        { feature: '代码审计', shannon: '每行代码在源码中可见', other: '黑盒' },
      ],
    },
    cta: {
      title: '30 秒开始使用',
      command: 'cargo install --git https://github.com/shannon-agent/shannon-code.git',
      button: '阅读文档',
    },
    footer: {
      copyright: '© 2026 Shannon Code 贡献者。',
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
