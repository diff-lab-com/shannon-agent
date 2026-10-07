# browser-use / computer-use 审查、修复与改进记录（2026-10-07）

分支：`feat/computer-use-browser-remote-improvements`（worktree `.worktrees/browser-computer-use`）。
调研依据：`docs/research/2026-10-07-mobile-remote-desktop-control-use-cases.md`（移动遥控场景）与 `docs/research/2026-10-07-browser-computer-use-competitor-comparison.md`（竞品框架对比）。

## 1. 审查发现的问题与修复（computer-use，`crates/shannon-tools/src/computer_use.rs`）

| # | 严重度 | 问题 | 修复 |
|---|---|---|---|
| C1 | **高（正确性）** | 坐标源空间错配：截图按纵横比缩到 ≤1024×768（1920×1080 屏→1024×576），但模型坐标一律按 1024×768 参考系缩放 → 非 4:3 屏 y 方向系统性偏 33% | 工具记录每显示器"最近截图实际像素尺寸"；所有坐标动作按 `截图尺寸→屏幕尺寸` 缩放（`scale_coordinate_from`），未截图时回落参考系。schema/描述同步更新坐标契约 |
| C2 | 中 | 未知键名静默错按：`str_to_key` 兜底取首字符（"F13"→'f'，"printscreen"→'p'），模型看到"成功"实际按错键 | 改为 `Option`，未知键返回明确错误；补 F13–F24、Insert、pgup/pgdn、option、plus 等别名 |
| C3 | 中 | `wait` 时长无上限（duration=1e9 会挂死循环，遥控场景尤甚） | `validate_wait`：拒绝负数/非有限，上限 `MAX_WAIT_SECONDS=60`，超限在结果里说明 |
| C4 | 中 | `left_click_drag` 直接从起点"瞬移"到终点按下-拖动-松开；HTML5 dnd/canvas/滑块需要中间 move 事件 | 按 ~12px/步（≤30 步、10ms 间隔）插值 |
| C5 | 低 | `type` 结果报字符数用 `text.len()`（字节数） | 改 `chars().count()` |
| C6 | 低 | macOS 截图失败无权限指引（10.15+ 屏幕录制 TCC；macOS 15 周期性重授权） | 截图失败信息附 System Settings 指引 |
| C7 | 改进 | 缺 Anthropic 20251124 已有的 `zoom` | 新增：原生分辨率裁剪区域（截图空间坐标+size），小屏/验证码/密集工具栏可读 |
| C8 | 改进 | 缺 `cursor_position` | 新增：回报指针全局/显示器局部/截图空间三份坐标（`monitor_index_for_point` 支持缝隙点就近归属），滚动/移窗后免整屏截图即可重新锚定 |
| 备注 | — | macOS 曾疑似 Retina 2x 偏移，核实为**不存在**：xcap `Monitor::width()` 在 macOS 返回逻辑点，与 CGEvent/enigo 同一坐标系；Windows/Linux 两侧均为物理像素 | 文档注释写明各平台坐标系约定 |

## 2. 审查发现的问题与修复（browser-use，`crates/shannon-browser/src/session.rs` + `browser_tools.rs`）

| # | 严重度 | 问题 | 修复 |
|---|---|---|---|
| B1 | **高（功能缺口）** | 无下拉框支持：`fill` 对 `<select>` 返回 unsupported，点选 option 不可靠 | 新工具 `browser_select_option`：按 value→精确 label→忽略大小写 label 三级匹配，派发 input+change |
| B2 | 高（遥控带宽/效率） | 无等待原语：加载态只能反复截图烧往返（移动遥控场景的核心成本） | 新工具 `browser_wait_for`：250ms 轮询等文本出现，超时上限 60s；read-only（保持 concurrency-safe 不变量） |
| B3 | 高（功能缺口） | 无文件上传（报销/投递/导入类委托任务必需） | 新工具 `browser_upload`：本地文件经 base64+DataTransfer 落进第 N 个 `input[type=file]`（**含隐藏输入**），触发站点自身 change handler，≤20MB |
| B4 | 中 | `scroll` 用 `window.scrollBy`：只滚主文档，wheel 惰性加载/内嵌滚动区/hover 工具栏全失效 | 改 CDP `Input.dispatchMouseEvent` MouseWheel 于视口中心，失败回落 JS |
| B5 | 中 | `press_key` 不支持组合键（"Control+Shift+Tab" 会当单字符乱发） | 解析修饰符（CDP 位：Alt1/Ctrl2/Meta4/Shift8）+ 主键带 modifier 位派发；带修饰符时清空 text 载荷（ctrl+a 不得插入 'a'）；未知键名报错（原首字符兜底同 C2） |
| B6 | 中 | 无 hover：CSS `:hover` 菜单/工具提示不可达 | 新工具 `browser_hover`（`hover_element`/`hover_at`，真 mouseMoved） |
| B7 | 中 | 截图仅 PNG（遥控流式进度带宽浪费 ~10×） | `browser_screenshot` 新增 `format=jpeg` + `quality`；media_type 元数据正确 |
| B8 | 中 | 无产物导出（收据/确认单/报告交付） | 新工具 `browser_pdf`：`Page.printToPDF` 写盘，默认临时目录时间戳文件名 |
| B9 | **高（正确性，测试中发现）** | `click_element` 在 hover 会改变布局的页面上打偏：先测坐标再移鼠标，指针落位触发 `:hover` 显示/收起菜单使按钮位移，press/release 打在旧坐标（Playwright 用稳定性等待解决的同款竞态） | `element_center` 两段测量：先 scrollIntoView+测，指针落位后**重测矩形中心**再按压 |
| B10 | 低 | 元素索引漏 `[role=combobox/searchbox/switch]` | 选择器补充（click/fill/snapshot 共用同一选择器常量） |
| 备注 | — | e2e 揭示架构约束：`ChromeSession::global()` 绑定首个 runtime，多 `#[tokio::test]` 共享进程会互相打死 → e2e 合并为单测试多场景；nextest 为 e2e 加串行组（8 个并发 headed Chrome 会互相压死）；**关闭最后标签页会让 headed Chrome 退出** → 场景内不关标签 | `tests/browser_e2e.rs` 重写为单测试多场景 + `browser_e2e` 串行组；离线环境优雅跳过 example.com |

## 3. 新增测试
- `computer_use.rs` 单测 +22：截图空间缩放（16:9/退化源/钳制/与参考系差异断言）、unscale 往返、多显示器点位归属（含缝隙就近）、zoom 裁剪矩形（映射/回落/越界钳制）、`ctrl++` 语法、严格键名接受/拒绝表、wait 钳制、新 schema 枚举与 size 属性、反序列化、无 feature stub 报错。
- `shannon-browser` session 单测 +3：`parse_key_parts`（组合与 `+` 键）、`modifier_bit`（CDP 位表+别名）、`normalize_key`（映射/别名/拒收未知与空）。
- `browser_tools.rs` 单测：5 个新工具名稳定性、destructive/concurrency 标志矩阵（含 read-only⇒concurrency-safe 不变量）、新 schema 形状。
- `browser_e2e.rs`：真实 Chrome 的 hermetic（base64 data: URL）场景——select_option 按标签/值选择与错误分支、hover 揭示 CSS 菜单、wait_for 成功/超时、upload 命中隐藏 input 与越界报错、PNG vs JPEG 大小与魔数、PDF 导出与魔数、真 wheel 滚动 scrollY 断言、组合键派发与未知键报错；原网络流程离线自跳过。
- 顺手修复（测试隔离）：`shannon-tools/src/agent.rs` 的 `test_agent_defs_builtin_has_system_prompts` 改为断言**内置定义**而非"合并用户 HOME 覆盖后"的注册表——任何在 `~/.shannon/agents/` 放了 code-reviewer 覆盖的机器上 fresh build 都会红（本机即复现）。

## 4. 已知未修（记录为后续）
- dev 分支既有 `shannon-api-protocol::codegen_drift every_pub_protocol_type_is_emitted` 失败（ApprovalModeRequest/ApprovalModeState 未进生成契约）——与本 PR 无关，不掺和。
- 下载文件落盘追踪、网络拦截封装、actionability auto-wait、macOS AX Tier-2、移动模拟——见竞品对比文档 §3。
- macOS/Windows 真机回归沿用既有 `#[ignore]` 测试（`macos_real_machine.rs` / `windows_real_machine.rs`），本机为 Linux 无法执行，PR 描述中注明。

## 5. 验证
- `cargo nextest run -p shannon-tools`：1841/1841 通过（含新单测与 schema 快照再生成）。
- `cargo nextest run -p shannon-browser --features local-browser`：13/13 通过。
- `cargo test --test browser_e2e`（真实 Chrome）：通过。
- `cargo nextest run --workspace --profile ci`：结果见 PR 描述（除上述 dev 既有失败外全绿）。
