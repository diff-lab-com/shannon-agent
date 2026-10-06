# #125 遗留 Minor polish — 实施计划（单任务批量）

> 依据：PR #125（阶段三·项目实体化 + 阶段四·插件打包）终审与各任务审查遗留的 deferred Minor，经 2026-09-26 评审分诊：本批修 13 条「用户可感知/诚实性/顺手」项；其余按触发条件挂账或关闭（清单见 PR 描述）。
> 分支：`fix/projects-plugins-polish`（基于合并 #125 后的 dev）。SDD 单任务派发 + 审查（#122 模式）。

## Global Constraints（约束本任务）

1. /opc 免改区（pages/OPC*.tsx、components/opc/**）不动。
2. i18n 新 key 只写 en.json + zh-CN.json（奇偶测试强制）。
3. shannon-core / shannon-types pub API 严格加法（本批只有 A5 动 desktop crate 的 plugin 生命周期命令语义，core 不动）。
4. 测试永不改进程 HOME（tempdir 缝隙）；本地 Rust 门禁 `--no-default-features --features tauri`。
5. UI 包管理器是 **pnpm**（无 package-lock.json，勿用 npm）。
6. 不新增 Tauri 命令（ACL 不变）；不重构 #125 刚落地的结构，只做点状修复。
7. 1420 端口是用户 demo server，禁碰；e2e 用 4173。

## Task 1 — 批量 polish（13 项）

### A 组：状态/文案/诚实性

- **A1 Triage 全滤空态**：项目筛选把条目全部滤掉时，页面仍渲染全选行 + 「shown 0 of N」，不显示引导空态。`desktop/ui/src/pages/Triage.tsx`（~:625）：EmptyState 判定改用过滤后可见数（`visibleItems.length === 0` 时显示空态卡；筛选激活时的文案区分「无待处理」与「本项目无待处理」，后者带「清除筛选」入口）。补测试：筛选激活 + 全滤空 → 空态可见、全选行不可见。
- **A2 Installed 错误态**：`desktop/ui/src/components/extensions/Installed.tsx` 错误标题复用 `extensions.plugins.loadFailed`（"Could not load catalog"）文不对题；且无重试。改用专用 key（en+zh-CN）+「重试」按钮（重新拉取 addons+stats）。补测试：错误态标题、点重试触发重新拉取。
- **A3 注册表拉取失败不清空乐观状态**：`desktop/ui/src/components/SidebarSessions.tsx` projects 加载 effect 的 `.catch(() => setProjects([]))` 会把刚做的 rename/归档等本地更新抹掉。改为失败时保留现有 `projects`（仅首次加载失败才允许落空数组）。补测试：先成功加载→模拟第二次 invoke 拒绝→行数不变。
- **A4 卸载诚实文案**：`Plugins.tsx` 检查失败路径的文案说「已落料产物保持原样」，但后端反落料是 sidecar 驱动（manifest 不可读而 sidecar 完好时仍会移除）。改文案为如实描述（sidecar 记录的产物将被移除；无法确认时仅移除注册表条目），en+zh-CN 两个 key。补测试断言新文案。
- **A5 反落料警告不丢失（Rust）**：`desktop/src/commands_plugins.rs` uninstall/disable/update 在反落料之后、注册表调用失败时，已收集的 warnings 被丢弃、Err 不提产物已移除。把 warnings 并入 Err 消息（或改返回类型携带，取最小改动），使 UI toast 不再误导。补测试：注册表失败路径的 Err 包含产物移除提示。
- **A6 嵌套布局预览诚实**：`desktop/src/plugin_materialize.rs` `summarize_archive` 把 `skills/a/b/SKILL.md` 计为技能 `a`，而 `scan_bundle_dir`/落料只认 `skills/<n>/SKILL.md` 直接子级——信任卡会显示永远不会落料的技能。对齐 archive 枚举与目录扫描（仅直接子级计为技能）。补测试：嵌套布局 zip 预览不显示该技能。

### B 组：交互/无障碍

- **A7 色板键盘可达**：`SidebarSessions.tsx` 项目颜色 popover（role="menu"/menuitemradio）无方向键导航、打开不聚焦。打开时焦点入内（首个 swatch），↑↓←→ 循环移动，Enter 选中，Escape 关闭并归还焦点。补测试（键盘事件驱动）。
- **A8 迁移行禁用说明可达**：`Plugins.tsx` 迁移导入行的禁用控件说明仅 `title`（键盘/读屏不可达）。改 `aria-describedby` 指向同行可见的说明元素（或行内可见注释），保证 SR 可读。补测试。
- **A9 chip 容器 a11y**：`desktop/ui/src/components/tasks/ProjectFilterChip.tsx` 的 aria-label 挂在纯 div 上无效。改为有效语义（容器 role="group" + aria-label，或让可见文本承载、移除 div 上的 aria）。三处 chip 共用组件则一处修。补断言。
- **A10 picker 安装防抖**：`Plugins.tsx` `handleAddLocal`/`handleAddArchive` 无 in-flight busy 门（git 对话框有）。复用同一 busy 状态：安装进行中禁用「+ 添加插件」菜单入口。补测试。

### C 组：顺手修

- **A11 文档措辞**：`desktop/src/plugin_materialize.rs` 模块注释「collision-free」overstated（`-` 在名内可碰撞）→ 改为「无 `-` 时无碰撞」类准确表述。纯注释改动。
- **A12 vacuous 断言修正**：`desktop/src/inbox_commands.rs` `routine_run_without_working_dir_keeps_the_default_session_start` 的 `assert_ne!(cwd, Some("/work/unhoused-project"))` 恒真。改为正向断言：未设置 working_dir 的例行运行会话的 session/start cwd 等于 tee 的预期默认（fixture 中的实际 cwd 值）。
- **A13 Stats 缀注 i18n 化**：`Installed.tsx` stats 缀注的 `' · '` 分隔符硬编码在 ICU 消息外，且天数用本地常量显示。把分隔符并入消息格式（en/zh-CN 各一套），并优先展示服务端回显 `days`。补/改断言。

## 验收

- `pnpm`（desktop/ui）+ `cargo test -p shannon-desktop --no-default-features --features tauri` 全绿；**`cargo fmt --check` 干净**（#125 首轮 CI 曾因门禁缺 fmt 挂过 Format）；clippy（CI flags）+ `cargo doc -D warnings` 干净；i18n 奇偶绿；不新增 Tauri 命令（ACL 测试绿）。
- 每项带覆盖测试（A11 除外，纯注释）；现有测试保持绿。
