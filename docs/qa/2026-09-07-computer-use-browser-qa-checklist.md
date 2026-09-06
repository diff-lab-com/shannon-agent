# 计算机操控 / 浏览器能力 真机 QA 清单

> **背景**: `feat/use-browser-computer-upload`（PR #73）与 `feat/p3-follow-ups` 的自动化测试覆盖了 wire 形态与解析逻辑，但三项能力依赖真实环境，无法在 ubuntu CI 中证明。本文档是合并/发布前的**人工验证清单**，逐项打钩后方可关闭对应验证债。
>
> **关联**: [P3 调研](../plans/2026-09-06-p3-future-research.md) §评审决策记录；[实施文档](../plans/2026-09-06-computer-use-browser-upload-implementation.md) §已知边界

---

## QA-1 macOS：AppleScript 工具（T13 Tier 1）

前置：Mac（任何 Apple Silicon / Intel，macOS 13+），Shannon 以 `--features computer-use`（或任意包含 shannon-tools 的构建）安装。

| # | 步骤 | 预期 |
|---|---|---|
| 1 | `shannon --prompt 'run this applescript: return 1+1' --allowed-tools applescript`（或在 REPL 让模型调用 `applescript`） | 首次调用弹出 **Automation TCC 提示**（针对目标 app；纯 `return 1+1` 不触达 app，应直接返回 `2`） |
| 2 | 让模型执行 `tell application "Notes" to get name of every note` | 首次触发 Notes 的 Automation 授权；授权后返回笔记名列表 |
| 3 | 拒绝授权场景：系统设置中关闭 Notes 的 Automation 权限后重跑 | 工具返回 is_error，错误信息含 osascript 的 -1743（not authorized）语义 |
| 4 | JXA 路径：`{"target":"applescript","language":"JavaScript","script":"1+1"}` | 返回 `2` |
| 5 | Shortcuts 路径：`{"target":"shortcuts","name":"<用户已有的快捷指令>"}` | 快捷指令执行并返回输出；不存在的名称 → is_error |
| 6 | 权限策略：REPL 中确认 `applescript` 调用触发 High-risk **逐次确认**（或按已配置的 approval mode） | 确认弹窗出现；拒绝后模型收到错误 |
| 7 | 超时：执行 `delay 60` | 30 秒超时，is_error 含 "timed out after 30s" |

## QA-2 Linux Wayland：libei 后端（T10 Phase 1）

前置：GNOME-Wayland 会话（Ubuntu 24.04 / Fedora 40 原生，非 XWayland），无 Xwayland fallback 的测试窗口（如原生 GTK 应用）。

| # | 步骤 | 预期 |
|---|---|---|
| 1 | `cargo build -p shannon-cli --features computer-use-libei` | 编译通过（无额外系统包；portal 是运行时依赖） |
| 2 | `echo $WAYLAND_DISPLAY`（有值）且 `echo $DISPLAY`（空）下运行 `computer screenshot` | 首次触发 **xdg-desktop-portal RemoteDesktop 授权对话框**；授权后返回截图 |
| 3 | 同会话用 xdo 构建（`--features computer-use`）执行 `computer click` | 失败信息包含 "X11-only input backend … rebuild with `--features computer-use-libei`" 提示（而非裸 Input init failed） |
| 4 | 授权后让模型执行 click/type/key_press 组合 | 动作在原生 Wayland 窗口生效（Portal 注入） |
| 5 | 截图降采样验证：4K 显示器下截图 metadata 的 width/height ≤ 1024×768 参考系 | 坐标点击仍命中目标（缩放契约一致） |

## QA-3 browser_toolset 真 API 冒烟（T12 Option C）

前置：Anthropic API key（Opus 4.5 / Sonnet 4.5 可用）。

| # | 步骤 | 预期 |
|---|---|---|
| 1 | `SHANNON_ANTHROPIC_TOOLSETS=1 shannon --model claude-opus-4-5 -p "open https://example.com and tell me the page title"` | 请求被 API 接受（无 400 invalid tool）；模型经由服务端浏览器返回标题 |
| 2 | 同一请求抓 wire（`SHANNON_LOG`/tee 的 events.jsonl 中 request header + body） | `anthropic-beta` 含 `computer-use-2025-11-24`；`tools[]` 末尾有 `{"type":"browser_toolset_20260801"}`；本地 browser 工具已被剪除 |
| 3 | 关闭 flag 重跑同 prompt | 走 MCP Playwright 路径（或提示 `/browser setup`），行为与合并前一致 |
| 4 | 非 Anthropic provider（如 OpenAI）+ flag 开启 | 请求体不含 toolset 条目（Option C 保证） |
| 5 | 不支持的模型（claude-3-5-sonnet）+ flag 开启 | 不注入 toolset、不加 beta（model gate 生效） |

## QA-4 `/browser doctor` 三平台快检（T14 地基）

| 平台 | 步骤 | 预期 |
|---|---|---|
| Linux（有 Chrome） | REPL `/browser doctor` | ✓ System browser (linux-path): /usr/bin/google-chrome…；MCP 状态行正确 |
| Linux（无浏览器容器） | 同上 | 列出全部 searched 路径 + apt/dnf/pacman/snap 安装指引 |
| 任意 | `SHANNON_BROWSER_PATH=/custom/chrome /browser doctor` | env 优先级生效（需存在该文件） |
| macOS / Windows | 同上 | 各自探测路径表正确（macOS /Applications；Windows ProgramFiles） |

---

## 通过标准

QA-1/2/3 全部打钩 → 对应验证债关闭（P3 文档 §评审决策 2/3 的实施视为完整）。
QA-4 属体验项，失败不阻塞合并，但需记录到 issue。

## 已知非目标（不做）

- Windows/macOS 的 computer 工具真机回归（enigo/xcap 上游已覆盖，行为与合并前一致）
- SkyLight 背景虚拟光标（T13-Tier3，不做）
