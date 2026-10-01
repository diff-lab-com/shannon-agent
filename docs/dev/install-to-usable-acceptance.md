# 安装 → 可用 端到端验收清单（Install → Usable Acceptance）

> R2 最大教训：url-only 远程 MCP 的断链事故中，「安装」「列表」「聊天」三个环节**各自**
> 都有测试且全部通过，但整条链是断的——装完的 server 永远进不了对话。逐环走查抓不住这类
> 缺陷，只有「安装 → 可用」的端到端断言才能抓住。
>
> 规则：**任何可安装面（installer/loader/registry）合入前，必须对下表对应行做完五步验收**，
> 或至少有一条自动化的端到端测试覆盖同一链路。新增可安装面时先在此文档加行，再写实现。

## 五步验收框架

每条线按同一链路验收，每步都要有期望结果与验证方法：

| 步骤 | 含义 |
|------|------|
| 1 安装 | 用户操作（hub 安装 / JSON 粘贴 / 手动放文件）成功，落盘位置正确 |
| 2 重启/热加载 | 要么明确要求重启（且 UI 如实提示），要么**不重启即可生效**（热加载） |
| 3 列表态 | 对应列表页/命令能看到新条目，徽章/状态如实（不可用≠假装可用） |
| 4 聊天内可见 | 系统提示/工具 schema 里出现对应条目（模型知道它存在） |
| 5 可调用 | 在聊天里实际触发一次，结果正确返回 |

状态标记：**已自动化**（指向测试文件）· **人工步骤** · **待自动化**（归属 backlog / 波次）。

---

## 1. MCP stdio（registry 安装 / JSON 粘贴 / 手动编辑 settings.json）

| 步骤 | 期望结果 | 验证方法 | 状态 |
|------|----------|----------|------|
| 安装 | `~/.shannon/settings.json#mcpServers` 新增一行（unified store，CLI/hub/桌面共享） | 检查 JSON；或 Extensions → MCP 安装 | 已自动化：`desktop/src/config.rs`（`save_mcp_servers` upsert / 不丢行） |
| 重启/热加载 | 无需重启：进程池后台 seed；连接失败不阻塞首窗 | 启动后打开 MCP 设置页 | 已自动化：`desktop/src/mcp.rs`（seed 逻辑）；池冷启动语义见 `desktop/src/main.rs` setup 注释 |
| 列表态 | `list_mcp_servers` 返回该 server，`connected=true`（Healthy），`tool_count`>0 | MCP 设置页状态徽章 | 已自动化：`desktop/src/mcp.rs::seed_skips_disabled_and_url_only_entries`（反面）+ 人工确认 Healthy 徽章 |
| 聊天内可见 | 下一 turn `assemble_mcp_tools` 把 `mcp__<server>__<tool>` 注册进 ToolRegistry 并进 schema | 聊天中让模型列出可用工具，或看请求 payload | 已自动化：`desktop/src/mcp.rs`（register 语义）；**待自动化**：命令级「装完下一 turn 可见」断言（backlog：R2 波次跟进） |
| 可调用 | 聊天里触发一个真实工具调用并返回结果 | 手工：装 filesystem/stdio echo server，聊天中调用 | 人工步骤 |

## 2. MCP remote url-only（当前 = 诚实态；**W2-1 接线后更新此行**）

| 步骤 | 期望结果 | 验证方法 | 状态 |
|------|----------|----------|------|
| 安装 | hub oauth_remote 安装成功，落盘为 `url`-only 条目（`command` 为空） | 检查 `settings.json#mcpServers` | 已自动化：`desktop/src/extensions/mcp_installers.rs`（`shannon:transport: oauth_remote`） |
| 重启/热加载 | **当前无传输层**：不存在「重启后可用」的承诺；`load_mcp_servers` 如实列出（`command` 空） | 读配置 / `get_mcp_server_config` | 已自动化：`desktop/src/config.rs`（url-only 行加载 + 保存不覆盖，`save_skips_url_only_rows_instead_of_clobbering_them` 等） |
| 列表态 | 列表如实显示不可连接（`connected=false`/offline，0 工具），**不得假装可用**；seed 明确跳过 url-only 行 | MCP 设置页看徽章 | 已自动化：`desktop/src/mcp.rs::seed_skips_disabled_and_url_only_entries` |
| 聊天内可见 | **不出现**在工具 schema——系统提示/注册表不向模型承诺不存在的工具 | 聊天让模型列工具，应无该 server 条目 | 已自动化（反面）：同上 seed 测试 |
| 可调用 | 当前预期=不可调用（诚实态）。W2-1 接通 remote 传输后本行改写为「装完即可调用」 | — | **待自动化（W2-1）**。注：UI 侧 restart 按钮目前仅按 busy 态禁用，尚无 url-only 专属禁用/徽章；W2-1 一并补齐并更新本行 |

## 3. 技能（skill：装 → 不重启、下一 turn 即可触发）——本分支已自动化

| 步骤 | 期望结果 | 验证方法 | 状态 |
|------|----------|----------|------|
| 安装 | hub 安装把 SKILL.md 写到 `~/.shannon/skills/<plugin>/` | 检查目录 | 已自动化：`desktop/src/extensions/skill_installers.rs` |
| 重启/热加载 | **无需重启**：打开 slash 菜单即触发 hydrate + 热注册（`list_skills` 内 `register_missing_skill_tools`，幂等） | 安装后不重启，直接开 slash 菜单 | 已自动化：`desktop/src/commands_mcp.rs::installed_skill_becomes_chat_callable_without_restart` + `desktop/src/skill_tools.rs::register_missing_skill_tools_is_idempotent_and_counts_new_only` |
| 列表态 | slash 菜单列出 `/name`（home + 项目目录合并，G1 P0-2.2） | 输入 `/` 看补全 | 已自动化：同上（`list_skills_inner` 断言 trigger） |
| 聊天内可见 | 下一 turn 系统提示包含 `/name` 且带 `skill_<name>` 映射说明 | 让模型复述可用 skills，或抓 system prompt | 已自动化：同上（`skills_for_chat_prompt` 断言） |
| 可调用 | 模型（或用户 `/trigger`）走 `skill_<id>` 工具，渲染 `${0}` 参数返回正文 | 聊天中输入 `/name args` | 已自动化：同上（工具 `execute` 断言）；人工步骤：真机端到端跑一次 hub 安装 |

## 4. Agent（扁平 TOML → 运行时 loader）

| 步骤 | 期望结果 | 验证方法 | 状态 |
|------|----------|----------|------|
| 安装 | hub 安装 agent 为 `<plugin>.toml` 到 `~/.shannon/agents/`（legacy 目录自动迁移） | 检查目录 | 已自动化：`desktop/src/extensions/agent_installers.rs`（`list_installed_agents_sees_flat_toml`、`migrate_legacy_agent_dirs_in`） |
| 重启/热加载 | 运行时 loader 每次列取消时从盘发现（`discover_agent_directories`），无需重启 | 安装后直接打开 agent 选择 | 已自动化：`desktop/src/extensions/agent_installers.rs` + loader 在 `shannon_skills::agent_loader` |
| 列表态 | `list_agent_definitions` 返回新 agent（`.claude/agents/*.md`、`.shannon/agents/*.md` 合并去重） | Agent 面板刷新 | 人工步骤；**待自动化**：命令级端到端（装 TOML → `list_agent_definitions` 可见，backlog） |
| 聊天内可见 | agent 面板选中后进入对话（或 sub-agent 注册表可见） | 聊天中 `agents` 列表 | 人工步骤 |
| 可调用 | 通过 `agent_spawn`（agent teams 开启时）真实执行 sub-agent | 聊天中触发一次委派 | 人工步骤 |

## 5. 数据源（安装 → 查询 → Add-to-chat 落 composer）

| 步骤 | 期望结果 | 验证方法 | 状态 |
|------|----------|----------|------|
| 安装 | hub 安装数据源（GitHub/Notion/Jira/…），配置落盘 | Extensions → Data Sources | 已自动化：`desktop/src/extensions/data_source_installers.rs` |
| 重启/热加载 | 已安装列表即读即显（`list_installed_data_sources`），无需重启 | 重开面板 | 人工步骤 |
| 列表态 | 目录/已装列表含新数据源 | Data Sources 页 | 已自动化：`desktop/src/extensions/data_source_catalog.rs`（目录解析） |
| 查询 | `query_data_source(slug, query)` 经 fetcher 返回结果 | 面板内查询 | 已自动化：`desktop/src/extensions/data_source_fetchers/*` 各源单测 |
| 可调用（Add-to-chat） | 查询结果「Add to chat」经 composerBridge 落到输入框，随下一 turn 发送 | 点 Add to chat，看 composer | 已自动化：`desktop/ui/src/__tests__/DataSourcesQuery.test.tsx`（TS 门禁：`desktop/ui && pnpm lint && pnpm test:ci`） |

---

## 维护约定

- 每行「已自动化」的测试若被改名/移动，同步更新本表。
- W2-1（remote MCP 传输）落地后，**必须**重写第 2 行为完整五步可用态，并删除「诚实态」标注。
- 新增可安装面（新的 installer/loader）= 在本文档新增一行 + 至少一条五步链自动化测试。
