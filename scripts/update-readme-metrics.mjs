// README metrics sync (P0-1): docs/metrics.md is the single authoritative
// source for engineering numbers (tests / LOC / files / workspace members).
// This script keeps the metric blocks in README.md and README.zh-CN.md in
// sync with it — and only inside the `<!-- metrics:start:<id> -->` …
// `<!-- metrics:end:<id> -->` markers; prose outside the markers is never
// touched.
//
// Usage: node scripts/update-readme-metrics.mjs [--recount | --check]
//   (no args)  rewrite the metric blocks in both READMEs from docs/metrics.md
//   --check    verify both READMEs match docs/metrics.md; print the drift and
//              exit 1 without writing anything (CI drift tripwire)
//   --recount  recompute metrics and update docs/metrics.md in place:
//                - tests + per-crate counts via `cargo nextest list --workspace
//                  --message-format json` (same aggregation as gen-metrics.sh;
//                  10 min timeout — on failure/timeout the existing values are
//                  kept with a warning)
//                - LOC / file count via `tokei` when installed (otherwise kept)
//                - workspace members parsed from the root Cargo.toml
//
// Exit codes: 0 ok · 1 (--check) drift · 2 hard error (missing metrics,
// missing markers, …)

import { spawnSync } from 'node:child_process'
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const metricsPath = join(root, 'docs', 'metrics.md')
const cargoTomlPath = join(root, 'Cargo.toml')
const readmes = [
  { file: join(root, 'README.md'), lang: 'en' },
  { file: join(root, 'README.zh-CN.md'), lang: 'zh' },
]
const nextestTimeoutMs = 10 * 60 * 1000

function die(msg) {
  console.error(`update-readme-metrics: ${msg}`)
  process.exit(2)
}

// ── docs/metrics.md parsing ─────────────────────────────────────────────────

// Reads the handful of summary numbers and the per-crate test table. Anything
// absent stays `null`/missing and callers must cope (or hard-error on numbers
// the READMEs cannot render without).
function parseMetrics(text) {
  const metrics = { tests: null, rustFiles: null, rustLoc: null, members: null, perCrate: new Map() }
  let section = ''
  for (const line of text.split('\n')) {
    if (line.startsWith('## ')) {
      section = line.slice(3).trim()
      continue
    }
    let m
    if ((m = line.match(/^\|\s*Tests \(nextest, runnable\)\s*\|\s*(\d+)\s*\|/))) metrics.tests = Number(m[1])
    else if ((m = line.match(/^\|\s*Rust source files\s*\|\s*(\d+)\s*\|/))) metrics.rustFiles = Number(m[1])
    else if ((m = line.match(/^\|\s*Rust LOC \(code\)\s*\|\s*(\d+)\s*\|/))) metrics.rustLoc = Number(m[1])
    else if ((m = line.match(/^\|\s*Workspace members\s*\|\s*(\d+)\s*\|/))) metrics.members = Number(m[1])
    // Per-crate rows (`| crate | tests | binaries |`) only count inside the
    // "Test counts" section — the Line counts table has the same 3-cell shape.
    else if (section === 'Test counts' && (m = line.match(/^\|\s*`?([\w-]+)`?\s*\|\s*(\d+)\s*\|\s*(\d+)\s*\|/))) {
      metrics.perCrate.set(m[1], { tests: Number(m[2]), binaries: Number(m[3]) })
    }
  }
  return metrics
}

// ── Workspace members (root Cargo.toml) ─────────────────────────────────────

function wildcardToRegExp(pattern) {
  const escaped = pattern.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '[^/]*')
  return new RegExp(`^${escaped}$`)
}

// Expands the `[workspace] members` globs the same way cargo does for the
// simple patterns this repo uses (`crates/*` plus explicit directories).
// A member counts only if its directory has a Cargo.toml.
function workspaceMembers() {
  const text = readFileSync(cargoTomlPath, 'utf8')
  const block = text.match(/members\s*=\s*\[([^\]]*)\]/s)
  if (!block) die(`cannot parse [workspace] members in ${cargoTomlPath}`)
  const patterns = [...block[1].matchAll(/"([^"]+)"/g)].map(m => m[1])
  const members = []
  for (const pattern of patterns) {
    if (pattern.includes('*')) {
      const base = join(root, dirname(pattern))
      const rx = wildcardToRegExp(pattern.split('/').pop())
      for (const entry of readdirSync(base).sort()) {
        const dir = join(base, entry)
        if (rx.test(entry) && statSync(dir).isDirectory() && existsSync(join(dir, 'Cargo.toml'))) {
          members.push({ dir, underCrates: true })
        }
      }
    } else {
      const dir = join(root, pattern)
      if (!existsSync(join(dir, 'Cargo.toml'))) die(`workspace member "${pattern}" has no Cargo.toml`)
      members.push({ dir, underCrates: false })
    }
  }
  // Package name: first `name = "…"` inside the `[package]` section (before
  // the next `[` header); fall back to the directory basename.
  for (const member of members) {
    const manifest = readFileSync(join(member.dir, 'Cargo.toml'), 'utf8')
    const pkg = manifest.match(/\[package\]\s*(?:[^\[]*\n)?name\s*=\s*"([^"]+)"/)
    member.name = pkg ? pkg[1] : member.dir.split('/').pop()
  }
  return members
}

// ── Recount helpers (--recount) ─────────────────────────────────────────────

function nextestRun() {
  // Reuse the developer checkout's shared target dir when present (a worktree
  // has no target of its own); an explicit CARGO_TARGET_DIR always wins.
  const env = { ...process.env }
  const sharedTarget = join(root, '..', 'shannon-mono', 'target')
  if (!env.CARGO_TARGET_DIR && existsSync(sharedTarget)) env.CARGO_TARGET_DIR = sharedTarget
  // JSON output (the text format has no "N tests" summary line): each envelope
  // carries `test-count` plus `rust-suites[]` with per-binary testcases.
  return spawnSync('cargo', ['nextest', 'list', '--workspace', '--message-format', 'json'], {
    cwd: root,
    env,
    encoding: 'utf8',
    timeout: nextestTimeoutMs,
    maxBuffer: 256 * 1024 * 1024,
  })
}

// Same aggregation as scripts/gen-metrics.sh: total = sum of envelope
// `test-count`; per crate = grouped `rust-suites` testcase counts.
function parseNextestJson(stdout) {
  const out = { tests: 0, perCrate: new Map() }
  for (const line of stdout.split('\n')) {
    const trimmed = line.trim()
    if (!trimmed.startsWith('{')) continue // skip cargo "Compiling" status lines
    let env
    try {
      env = JSON.parse(trimmed)
    } catch {
      continue
    }
    const count = Number(env['test-count'])
    if (Number.isFinite(count)) out.tests += count
    // `rust-suites` is a map keyed by test-binary name (values carry
    // `package-name`, `binary-name` and the `testcases` array).
    const suites = env['rust-suites']
    const list = Array.isArray(suites) ? suites : suites && typeof suites === 'object' ? Object.values(suites) : []
    for (const suite of list) {
      const pkg = suite['package-name']
      if (!pkg) continue
      // `testcases` is a map keyed by test id — count its entries.
      const tc = suite.testcases
      const n = Array.isArray(tc) ? tc.length : tc && typeof tc === 'object' ? Object.keys(tc).length : 0
      const cur = out.perCrate.get(pkg) ?? { tests: 0, binaries: 0 }
      cur.tests += n
      cur.binaries += 1
      out.perCrate.set(pkg, cur)
    }
  }
  return out
}

function tokeiRun() {
  const probe = spawnSync('tokei', ['--version'], { encoding: 'utf8' })
  if (probe.error || probe.status !== 0) return { available: false }
  const res = spawnSync('tokei', ['-t', 'Rust', '-o', 'json'], { cwd: root, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 })
  if (res.error || res.status !== 0) return { available: true, ok: false }
  try {
    const rust = JSON.parse(res.stdout).Rust
    const code = Number(rust?.code)
    const files = Number(rust?.files)
    if (!Number.isFinite(code)) return { available: true, ok: false }
    return { available: true, ok: true, code, files: Number.isFinite(files) ? files : null }
  } catch {
    return { available: true, ok: false }
  }
}

// Replace the `| <label> | <old> |` summary row, or insert it after the
// `Rust LOC (code)` row when metrics.md does not record it yet.
function upsertRow(text, label, value) {
  const row = `| ${label} | ${value} |`
  const re = new RegExp(`^\\|\\s*${label.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\s*\\|.*$`, 'm')
  if (re.test(text)) return text.replace(re, row)
  const anchor = text.match(/^(\|\s*Rust LOC \(code\)\s*\|.*)$/m)
  if (!anchor) {
    console.warn(`update-readme-metrics: warning: no anchor row for "${label}" — not recorded`)
    return text
  }
  const at = anchor.index + anchor[0].length
  return `${text.slice(0, at)}\n${row}${text.slice(at)}`
}

// Rebuild the `| crate | tests | binaries |` data rows (header and separator
// stay); only called after a successful recount.
function updatePerCrateTable(text, perCrate) {
  const rows = [...perCrate.entries()]
    .sort((a, b) => b[1].tests - a[1].tests || a[0].localeCompare(b[0]))
    .map(([name, s]) => `| ${name} | ${s.tests} | ${s.binaries} |`)
  const lines = text.split('\n')
  const start = lines.findIndex(l => l.startsWith('| crate | tests | binaries |'))
  if (start === -1 || start + 1 >= lines.length || !/^\|-+\|/.test(lines[start + 1])) {
    console.warn('update-readme-metrics: warning: per-crate table not found in metrics.md — not updated')
    return text
  }
  let end = start + 2
  while (end < lines.length && lines[end].startsWith('|')) end++
  lines.splice(start + 2, end - start - 2, ...rows)
  return lines.join('\n')
}

// ── README rendering ────────────────────────────────────────────────────────

const fmt = n => n.toLocaleString('en-US')

// One-line responsibility per crate, kept here (not in metrics.md — it is
// prose, not a metric). Undescribed crates fall back to "—" until noted.
const responsibility = {
  en: {
    'shannon-core': 'API client, query engine, permissions, tools, state',
    'shannon-tools': 'Tool implementations: file ops, git, search, notebook',
    'shannon-ui': 'Terminal UI, REPL, widgets, rendering',
    'shannon-engine': 'LLM API client, streaming, compaction/context budget, permissions',
    'shannon-agents': 'Multi-agent coordination: teams, worktree isolation',
    'shannon-desktop': 'Tauri desktop app shell and commands',
    'shannon-mcp': 'MCP protocol: transport, server, client, process pool',
    'shannon-cli': 'CLI entry point (`shannon` binary)',
    'shannon-commands': 'Built-in slash commands',
    'shannon-mcp-saas': 'SaaS MCP servers (GitHub, Slack, Jira, Notion, Linear)',
    'shannon-skills': 'Skills framework: discovery, loading, execution',
    'shannon-codegen': 'Code generation utilities',
    'shannon-types': 'Shared type definitions',
    'shannon-agent': 'Out-of-process agent (JSON-RPC over stdin/stdout)',
    'shannon-api-protocol': 'Wire protocol (serde types + TS codegen)',
    'shannon-remote': 'Remote execution worlds (SSH hosts, Docker)',
    'shannon-repomap': 'Repository symbol map for LLM context (tree-sitter)',
    'shannon-tool-interface': 'Tool trait definitions',
    'shannon-server': 'HTTP API server (`shannon serve`)',
    'shannon-stability-attr': 'Stability attribute macros',
  },
  zh: {
    'shannon-core': 'API 客户端、查询引擎、权限、工具、状态',
    'shannon-tools': '工具实现：文件操作、Git、搜索、Notebook',
    'shannon-ui': '终端 UI、REPL、组件、渲染',
    'shannon-engine': 'LLM API 客户端、流式适配、压缩/上下文预算、权限',
    'shannon-agents': '多 Agent 协作：团队、工作树隔离',
    'shannon-desktop': 'Tauri 桌面应用外壳与命令',
    'shannon-mcp': 'MCP 协议：传输层、服务器、客户端、进程池',
    'shannon-cli': 'CLI 入口（`shannon` 二进制）',
    'shannon-commands': '内置斜杠命令',
    'shannon-mcp-saas': 'SaaS MCP 服务器（GitHub、Slack、Jira、Notion、Linear）',
    'shannon-skills': '技能框架：发现、加载、执行',
    'shannon-codegen': '代码生成工具',
    'shannon-types': '共享类型定义',
    'shannon-agent': '独立 Agent（JSON-RPC over stdin/stdout）',
    'shannon-api-protocol': '线协议（serde 类型 + TS 代码生成）',
    'shannon-remote': '远程执行环境（SSH 主机、Docker）',
    'shannon-repomap': '仓库符号地图（tree-sitter）',
    'shannon-tool-interface': '工具 trait 定义',
    'shannon-server': 'HTTP API 服务器（`shannon serve`）',
    'shannon-stability-attr': '稳定性属性宏',
  },
}

function renderBlocks(lang, data) {
  const desc = name => responsibility[lang][name] ?? '—'
  const fullwidth = lang === 'zh' ? (s => `（${s}）`) : (s => ` (${s})`)
  const zero = data.crates.filter(c => data.perCrate.get(c)?.tests === 0).map(c => `\`${c}\``)
  const zeroCell = zero.length === 0 ? '0' : `${zero.length}${fullwidth(zero.join(', '))}`
  const nonCrates = data.memberList.filter(m => !m.underCrates).map(m => m.dir.split('/').pop())
  const cratesCount = data.memberList.length - nonCrates.length
  const shape = `${data.members} (${cratesCount} crates + ${nonCrates.join(' + ')})`
  const shapeZh = `${data.members}（${cratesCount} 个 crate + ${nonCrates.join(' + ')}）`
  const sorted = [...data.crates].sort((a, b) => {
    const ta = data.perCrate.get(a)?.tests ?? -1
    const tb = data.perCrate.get(b)?.tests ?? -1
    return tb - ta || a.localeCompare(b)
  })
  const testsCell = name => {
    const t = data.perCrate.get(name)?.tests
    return t == null ? '—' : fmt(t)
  }

  if (lang === 'en') {
    return {
      badge: `[![Crates](https://img.shields.io/badge/crates-${data.members}-blue.svg)](./docs/metrics.md)`,
      intro: `Every line of code is auditable, and every behavior is verified by **${fmt(data.tests)} automated tests**.`,
      diffrow: `| Test coverage | **${fmt(data.tests)}** tests across ${data.members} workspace members | Often zero tests |`,
      table: [
        '| Metric | Value |',
        '|--------|-------|',
        `| Total Rust code | ${fmt(data.rustLoc)} lines |`,
        `| Source files | ${fmt(data.rustFiles)} |`,
        `| Total tests (nextest, runnable) | **${fmt(data.tests)}** |`,
        `| Crates (workspace members) | ${shape} |`,
        `| Crates with zero tests | ${zeroCell} |`,
        '| CI lint | `cargo clippy --workspace -- -D warnings` (zero warnings) |',
      ].join('\n'),
      crates: [
        '| Crate | Tests | Responsibility |',
        '|-------|-------|----------------|',
        ...sorted.map(name => `| \`${name}\` | ${testsCell(name)} | ${desc(name)} |`),
      ].join('\n'),
    }
  }
  return {
    badge: `[![Crates](https://img.shields.io/badge/crates-${data.members}-blue.svg)](./docs/metrics.md)`,
    intro: `每一行代码都可审计，每一个行为都经过 **${fmt(data.tests)}** 个自动化测试验证。`,
    diffrow: `| 测试覆盖 | **${fmt(data.tests)}** 个测试，覆盖 ${data.members} 个 workspace 成员 | 通常零测试 |`,
    table: [
      '| 指标 | 数值 |',
      '|------|------|',
      `| Rust 代码总量 | ${fmt(data.rustLoc)} 行 |`,
      `| 源文件数 | ${fmt(data.rustFiles)} |`,
      `| 总测试数（nextest 可运行） | **${fmt(data.tests)}** |`,
      `| Crate 数（workspace 成员） | ${shapeZh} |`,
      `| 零测试 Crate 数 | ${zeroCell} |`,
      '| CI 代码检查 | `cargo clippy --workspace -- -D warnings`（零警告） |',
    ].join('\n'),
    crates: [
      '| Crate | 测试数 | 职责 |',
      '|-------|--------|------|',
      ...sorted.map(name => `| \`${name}\` | ${testsCell(name)} | ${desc(name)} |`),
    ].join('\n'),
  }
}

// ── Marker processing (only content inside markers is ever rewritten) ───────

function applyBlocks(text, rendered, where) {
  const drift = []
  const seen = new Set()
  const out = text.replace(
    /<!-- metrics:start:(\w+) -->(\n?)([\s\S]*?)<!-- metrics:end:(\w+) -->/g,
    (full, startId, nl, body, endId) => {
      if (startId !== endId) die(`${where}: mismatched markers: start:${startId} / end:${endId}`)
      seen.add(startId)
      if (!(startId in rendered)) die(`${where}: unknown block id "${startId}"`)
      const want = rendered[startId]
      if (body.replace(/\n$/, '') !== want) drift.push({ id: startId, have: body.replace(/\n$/, ''), want })
      return nl
        ? `<!-- metrics:start:${startId} -->\n${want}\n<!-- metrics:end:${startId} -->`
        : `<!-- metrics:start:${startId} -->${want}<!-- metrics:end:${startId} -->`
    },
  )
  for (const id of Object.keys(rendered)) {
    if (!seen.has(id)) die(`${where}: missing block "${id}" (wrap it in <!-- metrics:start:${id} --> … <!-- metrics:end:${id} -->)`)
  }
  return { out, drift }
}

// ── Modes ───────────────────────────────────────────────────────────────────

const argv = process.argv.slice(2)
const modes = argv.filter(a => a === '--check' || a === '--recount')
if (argv.some(a => !a.startsWith('--')) || modes.length > 1) {
  die('usage: node scripts/update-readme-metrics.mjs [--recount | --check]')
}

// --recount: refresh docs/metrics.md first, then fall through to the rewrite.
if (modes.includes('--recount')) {
  const text = readFileSync(metricsPath, 'utf8')
  const warnings = []
  let updated = text

  const res = nextestRun()
  const parsed = res.error || res.status !== 0 ? null : parseNextestJson(res.stdout ?? '')
  if (parsed == null || parsed.tests === 0) {
    const why = res.error?.code === 'ETIMEDOUT'
      ? `timed out after ${nextestTimeoutMs / 60000} min`
      : (res.error ? res.error.message : `exit status ${res.status}`)
    warnings.push(`cargo nextest list failed (${why}) — keeping existing test counts`)
  } else {
    updated = upsertRow(updated, 'Tests (nextest, runnable)', String(parsed.tests))
    updated = updatePerCrateTable(updated, parsed.perCrate)
  }

  const tokei = tokeiRun()
  if (!tokei.available) warnings.push('tokei not installed — keeping existing LOC/file counts')
  else if (!tokei.ok) warnings.push('tokei output could not be parsed — keeping existing LOC/file counts')
  else {
    updated = upsertRow(updated, 'Rust LOC (code)', String(tokei.code))
    if (tokei.files != null) updated = upsertRow(updated, 'Rust source files', String(tokei.files))
  }

  // Workspace members always come from the root Cargo.toml — the count is
  // recorded in metrics.md so the READMEs can cite it as a metric.
  updated = upsertRow(updated, 'Workspace members', String(workspaceMembers().length))

  if (updated !== text) {
    writeFileSync(metricsPath, updated)
    console.log(`update-readme-metrics: updated ${metricsPath}`)
  } else {
    console.log('update-readme-metrics: docs/metrics.md already up to date')
  }
  for (const w of warnings) console.warn(`update-readme-metrics: warning: ${w}`)
}

// Shared: derive README blocks from docs/metrics.md (the single source).
const metrics = parseMetrics(readFileSync(metricsPath, 'utf8'))
if (metrics.tests == null || metrics.rustFiles == null || metrics.rustLoc == null) {
  die('docs/metrics.md is missing required numbers (tests / Rust source files / Rust LOC)')
}
const memberList = workspaceMembers()
if (metrics.members != null && metrics.members !== memberList.length) {
  die(`docs/metrics.md records ${metrics.members} workspace members but Cargo.toml expands to ${memberList.length}`)
}
const data = {
  tests: metrics.tests,
  rustFiles: metrics.rustFiles,
  rustLoc: metrics.rustLoc,
  members: metrics.members ?? memberList.length,
  memberList,
  crates: memberList.map(m => m.name),
  perCrate: metrics.perCrate,
}

const check = modes.includes('--check')
let drifted = false
for (const readme of readmes) {
  const text = readFileSync(readme.file, 'utf8')
  const rendered = renderBlocks(readme.lang, data)
  const { out, drift } = applyBlocks(text, rendered, readme.file)
  for (const d of drift) {
    drifted = true
    console.error(`update-readme-metrics: ${readme.file}: block "${d.id}" differs from docs/metrics.md`)
    for (const line of d.want.split('\n')) console.error(`  + ${line}`)
    for (const line of d.have.split('\n')) console.error(`  - ${line}`)
  }
  if (!check && out !== text) {
    writeFileSync(readme.file, out)
    console.log(`update-readme-metrics: updated ${readme.file}`)
  }
}
if (check && drifted) process.exit(1)
if (check) console.log('update-readme-metrics: README metric blocks match docs/metrics.md (no drift)')
