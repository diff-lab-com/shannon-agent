# Claude Code 兼容性改进方案

**Date**: 2026-04-28
**Status**: Pending Review
**Principle**: 以 Claude Code 标准为基准; 若其它方案确实更优, 标注 ⚠️ 等待决策

---

## 差距总览

| 子系统 | Shannon 现状 | Claude Code 标准 | 兼容度 | 工作量 |
|--------|-------------|-----------------|--------|--------|
| LSP | 无 | 外部进程 + `.lsp.json` | 0% | 中 |
| Skills | 有基础 | `SKILL.md` + agentskills.io | 60% | 中 |
| Plugin | 无 (仅 Tool trait + MCP) | `plugin.json` manifest | 0% | 大 |
| Hooks | 20 事件, 3 类型, 未接线 | 30 事件, 5 类型, 完整接线 | 40% | 中 |
| Memory | 有存储, 未集成 | CLAUDE.md 层级 + auto-memory | 25% | 中 |
| MCP | 基本完整 | 已兼容 | 85% | 小 |

---

## Phase A: LSP 集成 (外部进程模式)

### 设计原则
- **不嵌入 LSP 代码** — 通过 JSON-RPC stdio 与外部语言服务器通信
- 完全遵循 Claude Code 的 `.lsp.json` 配置格式
- LSP 能力暴露为内部工具 (go_to_definition, find_references, hover, rename, diagnostics)

### 配置格式 (`.lsp.json`)
```json
{
  "rust": {
    "command": "rust-analyzer",
    "args": [],
    "extensionToLanguage": { ".rs": "rust" },
    "initializationOptions": {},
    "settings": {},
    "startupTimeout": 10000,
    "shutdownTimeout": 5000,
    "restartOnCrash": true,
    "maxRestarts": 3
  },
  "typescript": {
    "command": "typescript-language-server",
    "args": ["--stdio"],
    "extensionToLanguage": { ".ts": "typescript", ".tsx": "typescriptreact" }
  }
}
```

### 配置发现路径 (优先级由低到高)
1. Plugin 内 `.lsp.json` (随 plugin 启用)
2. `~/.shannon/.lsp.json` (用户级)
3. `~/.claude/.lsp.json` (Claude Code 兼容)
4. `.lsp.json` (项目根目录)

### 架构
```
.lsp.json → LspManager
  → 启动外部进程 (rust-analyzer, gopls, etc.)
  → JSON-RPC over stdio
  → 暴露为 Tool trait 实现:
     - lsp_go_to_definition(file, line, col)
     - lsp_find_references(file, line, col)
     - lsp_hover(file, line, col)
     - lsp_rename(file, line, col, new_name)
     - lsp_diagnostics(file)
     - lsp_document_symbols(file)
     - lsp_workspace_symbols(query)
     - lsp_code_actions(file, range)
```

### 文件变更
| 文件 | 变更 |
|------|------|
| `crates/shannon-core/src/lsp/mod.rs` | 新建 — LspManager, 进程管理, JSON-RPC 客户端 |
| `crates/shannon-core/src/lsp/protocol.rs` | 新建 — LSP 协议类型 (Initialize, TextDocument, etc.) |
| `crates/shannon-core/src/lsp/config.rs` | 新建 — `.lsp.json` 加载与发现 |
| `crates/shannon-core/src/lsp/tool.rs` | 新建 — Tool trait 实现, 暴露 LSP 操作 |
| `crates/shannon-core/src/tools.rs` | 修改 — 注册 LSP 工具 |
| `crates/shannon-core/src/state.rs` | 修改 — 添加 LspManager 到 AppState |

---

## Phase B: Hook 系统完善

### 现状 vs 标准

| 维度 | Shannon | Claude Code | 差距 |
|------|---------|-------------|------|
| 事件数 | 20 | 30 | 缺 10 个 |
| 处理器类型 | command, http, prompt | + mcp_tool, agent | 缺 2 个 |
| matcher | regex, `*` | + pipe-separated `\|` | 缺语法 |
| 条件执行 | 无 | `if` 字段 | 缺 |
| 退出码语义 | 未定义 | 0=成功, 2=阻止 | 缺 |
| 环境变量控制 | 无 | `allowedEnvVars` | 缺 |
| **接线状态** | **未接线** | **完整接线** | **关键差距** |

### 新增事件 (10)
- `UserPromptExpansion` — prompt 展开后
- `PostToolBatch` — 批量工具执行后
- `ConfigChange` — 配置变更
- `InstructionsLoaded` — CLAUDE.md 加载后
- `WorktreeCreate` / `WorktreeRemove` — 工作树操作
- `Elicitation` / `ElicitationResult` — 交互式提问
- `TaskCreated` / `TaskCompleted` — 任务管理

### 新增处理器类型 (2)
- `mcp_tool` — 调用已连接的 MCP 服务器工具
- `agent` — 启动子代理 (有 Read/Grep/Glob 工具) 进行验证

### 新增 matcher 语法
- `"Edit|Write"` — pipe-separated 匹配多个工具
- `"if": "Bash(rm *)"` — 条件匹配 (工具名 + 参数模式)

### 退出码语义
```
Exit 0 → 成功, stdout 解析 JSON
Exit 2 → 阻止操作, stderr 显示给 LLM
Exit other → 非阻塞错误, 继续执行
```

### 关键接线点
```
tool_execution.rs → PreToolUse / PostToolUse / PostToolUseFailure
repl/input.rs → UserPromptSubmit
repl/mod.rs → SessionStart / SessionEnd
query.rs → PreCompact / PostCompact
```

### 文件变更
| 文件 | 变更 |
|------|------|
| `crates/shannon-core/src/hooks.rs` | 扩展事件、处理器类型、matcher 语法、退出码 |
| `crates/shannon-core/src/tools.rs` | 接线 PreToolUse / PostToolUse 到执行管道 |
| `crates/shannon-ui/src/repl/input.rs` | 接线 UserPromptSubmit |
| `crates/shannon-ui/src/repl/mod.rs` | 接线 SessionStart / SessionEnd |
| `crates/shannon-core/src/query.rs` | 接线 PreCompact / PostCompact |

---

## Phase C: Memory / Knowledge 对齐

### 现状 vs 标准

| 维度 | Shannon | Claude Code | 差距 |
|------|---------|-------------|------|
| CLAUDE.md 层级 | 基本发现 | 4 级层级 (org/user/project/local) | 缺层级 |
| 路径规则 | 无 | `.claude/rules/*.md` | 缺 |
| @import | 无 | `@path/to/file` 递归引入 | 缺 |
| Auto-memory | 有存储, 未集成 | `.claude/memory/` + MEMORY.md 索引 | 未集成 |
| Compaction 重注入 | 无 | 压缩后重注入 CLAUDE.md | 缺 |
| /remember 命令 | 无 | 有 | 缺 |

### CLAUDE.md 发现层级 (优先级由低到高)
```
/etc/claude-code/CLAUDE.md         # 组织级 (Linux)
~/.claude/CLAUDE.md                # 用户级
./CLAUDE.md 或 ./.claude/CLAUDE.md # 项目级
./CLAUDE.local.md                  # 本地个人 (gitignored)
```
- 向上遍历目录树加载所有 `CLAUDE.md` 和 `CLAUDE.local.md`
- **合并而非覆盖** — 所有文件内容拼接
- 子目录的文件按需加载

### @import 语法
```markdown
See @README for overview.
- git workflow @docs/git-instructions.md
```
- 最多 5 层递归
- 解析为被引入文件的完整内容

### Auto-memory
- Claude 自己在 `.claude/memory/` 写笔记
- `MEMORY.md` 作为索引 (前 200 行 / 25KB 在会话开始时加载)
- 压缩后重新注入已调用的 skills 和 memory

### 文件变更
| 文件 | 变更 |
|------|------|
| `crates/shannon-core/src/project_memory.rs` | 扩展 CLAUDE.md 层级发现, @import 解析 |
| `crates/shannon-core/src/memory/store.rs` | 集成 auto-memory, MEMORY.md 索引 |
| `crates/shannon-core/src/query.rs` | 注入 memory 到系统 prompt, compaction 重注入 |
| `crates/shannon-commands/src/` | 新增 `/remember` 命令 |

---

## Phase D: Skills 系统对齐

### 现状 vs 标准

| 维度 | Shannon | Claude Code | 差距 |
|------|---------|-------------|------|
| 文件格式 | `.md` + frontmatter | `SKILL.md` in directory | 缺目录结构 |
| agentskills.io | 部分兼容 | 完全兼容 | 需验证 |
| 发现范围 | bundled/user/project/MCP | + plugin/enterprise | 缺 |
| 压缩重注入 | 无 | 前 5000 tokens/skill, 25000 总预算 | 缺 |
| Plugin 命名空间 | 无 | `plugin-name:skill-name` | 缺 |
| 动态上下文 | `!command` | `!command` + `!multi-line` | 基本一致 |
| model 选择 | 有 | `model` + `effort` 字段 | 缺 effort |
| context 隔离 | 有 | `context: fork` | 需验证 |

### 目录结构变更
```
# 之前 (Shannon)
.claude/skills/commit.md

# 之后 (Claude Code 兼容)
.claude/skills/commit/
  SKILL.md              # 必需入口
  template.md           # 可选模板
  examples/sample.md    # 可选示例
  scripts/validate.sh   # 可选脚本
```

### 新增字段
```yaml
---
effort: high              # low/medium/high
context: fork             # 继承/fork/隔离
---
```

### 发现路径扩展
```
~/.claude/skills/<name>/SKILL.md    # 用户级
.claude/skills/<name>/SKILL.md      # 项目级
<plugin>/skills/<name>/SKILL.md     # Plugin 级
Enterprise managed settings          # 企业级
```

### 向后兼容
- 现有扁平 `.md` 文件继续支持
- 目录 `SKILL.md` 优先于同名扁平文件

### 文件变更
| 文件 | 变更 |
|------|------|
| `crates/shannon-skills/src/loader.rs` | 支持 SKILL.md 目录结构 |
| `crates/shannon-skills/src/definition.rs` | 添加 effort, context 字段 |
| `crates/shannon-skills/src/discovery.rs` | 扩展发现路径, plugin 命名空间 |

---

## Phase E: Plugin 系统

### 现状
Shannon 无 plugin 系统, 仅 Tool trait + MCP adapter。

### Claude Code 标准
```
my-plugin/
  .claude-plugin/
    plugin.json           # 清单
  skills/                 # 技能
  commands/               # 遗留命令
  agents/                 # 代理定义
  hooks/hooks.json        # Hook 处理器
  .mcp.json               # MCP 服务器配置
  .lsp.json               # LSP 服务器配置
  monitors/monitors.json  # 后台监控
  bin/                    # 可执行文件
  settings.json           # 默认设置
  themes/                 # 主题
  output-styles/          # 输出样式
```

### plugin.json 格式
```json
{
  "name": "plugin-name",
  "version": "1.0.0",
  "description": "...",
  "author": { "name": "...", "url": "..." },
  "skills": "./skills/",
  "agents": "./agents/",
  "hooks": "./hooks/hooks.json",
  "mcpServers": "./.mcp.json",
  "lspServers": "./.lsp.json",
  "userConfig": {
    "api_token": {
      "type": "string",
      "title": "API Token",
      "sensitive": true,
      "required": true
    }
  }
}
```

### 关键特性
- `${CLAUDE_PLUGIN_ROOT}` — 插件安装目录
- `${CLAUDE_PLUGIN_DATA}` — 持久数据目录 (跨更新保留)
- Plugin skill 命名空间: `plugin-name:skill-name`
- 安装范围: user/project/local/managed

### 文件变更
| 文件 | 变更 |
|------|------|
| `crates/shannon-core/src/plugin/mod.rs` | 新建 — PluginManager |
| `crates/shannon-core/src/plugin/manifest.rs` | 新建 — plugin.json 解析 |
| `crates/shannon-core/src/plugin/loader.rs` | 新建 — 组件发现与加载 |
| `crates/shannon-core/src/plugin/installer.rs` | 新建 — 安装/卸载 |
| `crates/shannon-skills/src/loader.rs` | 修改 — 支持 plugin 命名空间 |
| `crates/shannon-mcp/src/config.rs` | 修改 — 支持 plugin MCP 服务器 |

---

## Phase F: MCP 补齐

### 现有差距 (15%)
- 缺 `list_changed` 通知处理 (动态工具更新)
- 缺自动重连 (指数退避, 最多 5 次)
- 缺 Managed MCP (`allowedMcpServers`, `deniedMcpServers`)
- 需验证 HTTP 传输类型完全兼容

### 文件变更
| 文件 | 变更 |
|------|------|
| `crates/shannon-mcp/src/client.rs` | 添加 list_changed 处理, 自动重连 |
| `crates/shannon-mcp/src/config.rs` | 添加 Managed MCP 配置 |

---

## 执行顺序

```
Phase A (LSP)       ~1.5 周   开发体验立竿见影
Phase B (Hooks)     ~1 周     安全与流程控制基础
Phase C (Memory)    ~1 周     上下文连续性
Phase D (Skills)    ~0.5 周   格式对齐, 工作量小
Phase E (Plugin)    ~2 周     依赖 A-D 完成
Phase F (MCP)       ~0.5 周   小修小补
```

**推荐顺序**: B → C → A → D → F → E
- B (Hooks) 优先是因为它是安全基础, 且 A/C/D 都需要 hook 接线
- C (Memory) 其次是因为它影响上下文质量
- E (Plugin) 最后是因为它依赖前面所有组件

---

## 用户决策 (已确认)

1. **Plugin 分发机制**: ✅ 跟随 Claude Code 的 git/npm 模式
2. **Enterprise/Managed 设置**: ✅ 个人工具, 组织级管控列入未来规划
3. **LSP 推荐安装**: ✅ 首次需要某语言文件时提示安装
