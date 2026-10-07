# MULTIWINDOW-AUDIT — 多窗口(Wave 3 spike F-20)现状审计

- **日期**:2026-10-07 · **分支**:`feat/ui-v2-backlog`(ui-redesign-v2 worktree)· **域**:W10 多窗口审计
- **任务**:审计「会话独立 OS 窗口」现状全链路 + demo 走查 + 顺手小修(结构性缺口只记录不实施)
- **对位**:ADVERSARIAL-REVIEW **F-20**(「会话独立 OS 窗口是 Codex 未满足需求,本稿未覆盖,留待 spike」)

## TL;DR

1. **「会话独立窗口」已经是完整落地的 P1-1 功能**,不是半成品:侧栏入口 → Rust 侧 `WebviewWindow` 创建(`session-<uuid>` 标签)→ `/?windowSession=<uuid>` 路由进 window mode → 事件按会话过滤 → 关闭/主窗退出/重启恢复三条回收路径全部有实现、有测试、有 e2e(nightly)。
2. **副窗口的状态真实性分级**:会话流(文本/思考/工具/usage)、审批弹窗、ask-user、预算徽章、composer/草稿 —— **真实可用**(广播 + 按窗过滤,不是假象);sessions/配置类状态靠 `sessions:updated`/`config:updated` 广播重取 —— 可用;**toast 无跨窗去重**(chime 有)、subagent 横幅无会话过滤 —— 有瑕疵。
3. 发现并**已修 2 个明确 bug**(详见 §4):① 重启恢复会为已删除会话开出 ghost 窗口,且 ghost 的 boot `switch_session` 会把死 id 提升为全局 active 指针(后端剪枝 + 3 个单测);② nightly e2e 的「无侧栏」断言锚在 `complementary` role 上,会被塌缩态 RightDock 的 `<aside>` 命中,目前靠 lazy-chunk 时序碰巧通过(改锚 `data-sidebar`)。
4. **与 Claude Code Desktop 的 pane 模型差距**:「一窗一会话」的 OS 窗口形态与 pane 模型的「一窗多 pane」是两代形态;事件过滤/会话遥测等 pane 级地基可复用,缺的是同窗多会话布局、pane 导航、以及每窗独立 active 指针(全局单 active 指针是当前最大的结构性约束)。Tauri 2 尚无同窗 multi-webview 稳定能力,pane 化在现技术栈下 = 同一 webview 内的多实例路由布局,工作量见 §7。

---

## §1 现状链路(入口 → 创建 → 路由/状态 → 关闭回收)

路径前缀:`WORKTREE/desktop/`(下文简写)。前端 `ui/src`,后端 `src`(Rust)。

```
┌─ 主窗口 (label=main, tauri.conf.json app.windows) ─────────────────────┐
│ Sidebar 会话行 ⋯ 菜单「在新窗口打开」                                    │
│   components/SidebarSessions.tsx:1029  → api.openSessionWindow(id)     │
│   lib/tauri-api.ts:1120                → invoke('open_session_window') │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   ▼
┌─ Rust: src/session_window_commands.rs ─────────────────────────────────┐
│ open_session_window_inner (:193)                                       │
│  · UUID 校验 → 标签 session-<uuid>(:51 SESSION_WINDOW_PREFIX)          │
│  · 按 label 去重:已存在 → focus(existing)(:199)                      │
│  · WebviewWindowBuilder: 1080x760 / min 800x600,                       │
│    title = session_title(sessions)(空标题回落 "Shannon"),             │
│    WebviewUrl::App("/windowSession=<uuid>")                            │
│  · 注册 SessionWindowRegistry(AppState)→ 持久化到                      │
│    DesktopConfig.open_session_windows(src/config.rs:130,               │
│    ~/.shannon/desktop/config.json)                                     │
│ 冻结契约: open_session_window / list_session_windows /                  │
│           close_session_window(拒非 session-* 标签)/                   │
│           reveal_session_in_main(focus main + 定向 emit)               │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   ▼  新 webview 以 /?windowSession=<uuid> 启动
┌─ 前端: window mode(UI 形态)──────────────────────────────────────────┐
│ lib/windowSession.ts:30 parseWindowSession(UUID 硬校验,非法→null=主窗)│
│ context/AppContext.tsx:268   windowSessionId(内存态,一次解析)        │
│ AppContext.tsx:1962-1968     boot 绑定:switchToSession(windowSessionId)│
│                              (不用后端全局 active 会话)               │
│ components/Layout.tsx:70     isWindowMode → 侧栏不渲染、--sidebar-w=0、 │
│                              原生标题跟随会话改名(:158-163 setTitle)   │
│ components/Header.tsx:197+   「SESSION WINDOW」徽章 +                   │
│                              「在主窗口打开」(reveal)+「关闭窗口」      │
│ pages/Chat.tsx:146           visibleSessionId = windowSessionId ?? …    │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   ▼
┌─ 事件层(跨窗口状态同步的承重墙)────────────────────────────────────┐
│ 后端:所有 query:*/permission/budget/sessions/config 事件均为            │
│       app.emit 全局广播(src/commands*.rs,无 emit_to("main"))          │
│ ACL: capabilities/session-windows.json = [main, session-*] 授           │
│       core:event:default + core:window:allow-set-title                  │
│ 前端:windowSession.ts:50 isEventForCurrentWindow —— 副窗口只消费        │
│       payload.session_id == windowSessionId 的事件(无 sid 旧后端        │
│       降级为不过滤);AppContext 全部 query:* 处理器(:1491-1811)         │
│       与 PERMISSION_REQUEST(:1811)都过这把筛                            │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   ▼
┌─ 关闭 / 回收(三条路径)────────────────────────────────────────────┐
│ ① 副窗 Header「关闭」→ close_session_window(getCurrentWindow().label)  │
│    (Header.tsx:254-258;后端拒非 session-* 标签,主窗不可被此路径关)   │
│ ② 标题栏 ✕ / OS teardown                                               │
│ ①②→ main.rs:634 on_window_event(Destroyed) →                          │
│      cleanup_destroyed_window(注册表摘除 + 持久化刷新)+                │
│      terminal kill_window_sessions(回收该窗 PTY,main.rs:659-671)      │
│ ③ 主窗关闭 = 退出应用:handle_main_window_destroyed(:323)——            │
│      APP_EXITING 置位(副窗清理不再排空持久列表)→ 先持久化当前列表      │
│      → 逐个 close 副窗                                                  │
│ 重启恢复:main.rs:702 setup → restore_session_windows(:353):            │
│      sanitize UUID → 【本次新增】按 L0 store listing 剪掉已删会话 id    │
│      → 逐个 open(dedupe 路径)→ self-heal 回写成功集合                 │
└────────────────────────────────────────────────────────────────────────┘
```

**伴生窗(CompanionPage,Office Wave 3 C3)**:同一 SPA 的 `/companion` 无 chrome 路由,固定 label `companion`,420x320 常驻置顶(可关),`open_companion_window` / `set_companion_always_on_top`;跨窗消息用 `emitTo("main")` 定向(`shannon:companion-prompt`),主窗 `App.tsx` 的 `CompanionPromptBridge` 收到后经 `pushComposerDraft` 落为**草稿**(不自动发送);无持久化、不随重启恢复。src/companion_window_commands.rs、ui/src/lib/companionBridge.ts。

## §2 跨窗口状态同步审计(副窗口各状态面真实性)

后端事件一律**全局广播**(Tauri `emit`),窗口级过滤在前端。副窗口(每窗一个独立 React 树/store 实例,无跨窗内存共享)各状态面:

| 状态面 | 副窗口真实可用? | 机制 | 备注 |
|---|---|---|---|
| 会话流(文本/思考) | ✅ | `query:text/thinking` 广播 + `isEventForCurrentWindow` 窗筛 + per-session bucket | 外窗会话的 token 被筛掉且**不锁 composer**(nightly e2e 断言) |
| 工具卡/进度/run tab | ✅ | `query:tool-*` 同上,`key !== visibleKey` 不投影 | |
| usage/预算徽章 | ✅ | `get_session_budget/usage` 是普通命令(任意窗可调);`budget:warning/exceeded` 广播后 `useSessionBudget` 按 sessionId 过滤刷新 | Header 徽章依赖 `currentSessionId`,而 boot 绑定已把它设为钉住会话,成立 |
| 审批弹窗 | ✅(按窗) | `permission-request` 广播;副窗只弹本会话 | **主窗全放行**(任意会话都弹主窗)——现状已钉死在 nightly e2e,见 §6-E |
| ask-user 卡 | ✅(F2 会话作用域) | 渲染层按 `windowSessionId ?? currentSessionId` 过滤;`ask-user-resolved` 广播清除他窗待答卡 | 无 session_id 的歧义请求仍每窗都弹(文档化的回退) |
| toast | ⚠️ | sonner Toaster 每窗一份;`session:auto-archived`/`auto-unarchived`/`model-override-fallback` 等无跨窗去重 → **每窗各弹一次** | chime 已有 F3 跨窗去重(`lib/notificationChime.ts`),toast 没有平移;见 §6-C |
| 声音 chime | ✅ | F3:稳定 payload key 跨窗去重,只响一次 | |
| 会话列表/钉 pins | ✅ | `sessions:updated`/`session-pins-changed` 广播 → 每窗 `refreshSessions` | 副窗标题跟随改名也靠这条(Layout effect) |
| 配置/模型目录 | ✅ | `config:updated` 广播 → refreshConfig/ProviderStatus/Models(P1-12 注释明确覆盖"从会话窗改配置"场景) | |
| subagent 横幅 | ❌ | `subagent:start/stop` **无 session_id 字段**,每窗都 setSubagentLive → 副窗会显示**他窗**的 subagent 横幅 | §6-D,payload 缺字段,前端无法修 |
| composer 草稿 | ✅ | 草稿按会话 id 存 localStorage;副窗写的是自己钉住会话的 key;伴生窗 capture 只投主窗(信任契约) | |

**结论**:副窗口不是「只读镜像」也不是「假活」——发送、流式、停止、审批、预算都是真实工作的;瑕疵集中在「无会话身份的事件」(subagent)与「无跨窗去重的 UI 反馈」(toast)。

## §3 走查记录(demo 模式,2026-10-07)

**方式与边界**:demo 模式(`pnpm demo`,mock backend)没有 Tauri shell,`open/close/reveal_session_in_main` 是**有意的 unmocked 项**(`src/__tests__/mock-handlers-coverage.test.ts:68-71`「session windows need the real Tauri shell」)。因此浏览器走查按 nightly e2e 同款方法:**主窗页面 A** + **以 `/?windowSession=<uuid>` 直接引导的页面 B**——这正是原生 `WebviewWindow` 创建时加载的同一 URL,SPA 侧状态与原生窗口完全一致;**原生 OS 窗口(标题栏/任务栏/窗口管理器行为)浏览器截不到**,该层由 Rust 侧代码与常量测试钉住(`session_window_commands.rs` 契约测试)。

走查驱动:armed `multi-window.yaml` 剧本(与 `e2e/chat-script.multi-window.spec.ts` 同源),Playwright chromium,1440x900。

| 截图(screenshots/) | 内容 |
|---|---|
| `multiwindow-01-entry-menu.png` | 主窗:会话行 ⋯ 菜单露出「Open in Ne…(w 新窗口)」入口 |
| `multiwindow-02-demo-limitation-toast.png` | demo 点击入口 → 文档化的「This feature is not available in demo mode.」toast(即 §3 开头的 unmocked 项) |
| `multiwindow-03-session-window-mode.png` | **会话窗 boot 态**:标题 "Window Session Two"、SESSION WINDOW 徽章、在主窗口打开/关闭两控件、无侧栏、footer 用量正常 |
| `multiwindow-04-session-window-own-stream.png` | 副窗内发送 → 本会话流式渲染完成;此前注入的外会话(s1)token 全程未渲染 |
| `multiwindow-05-main-window-both-sessions.png` | 主窗同时显示两会话、侧栏行活动点;两页互不串流 |

探针结果(走查脚本实测输出):

```json
{ "badge": true, "openInMain": true, "closeWindow": true,
  "title": "Window Session Two", "foreignHidden": true, "ownStream": true }
```

- `noSidebar` 一项在首轮探针里误报 `false`:**塌缩态 RightDock 的 `<aside aria-label="Right dock">` 也带 complementary role**(`pages/chat/RightDock.tsx:405`,关闭时 `inert` 挂载)。精确锚 `document.querySelectorAll('[data-sidebar]').length === 0` 证实侧栏确实不渲染 → 直接催生了 §4-2 的 e2e 修复。
- 走查中未点击「在主窗口打开」/「关闭窗口」两控件(reveal 在 demo 无 handler,只会 toast;nightly e2e 同样只钉存在性)——真实行为由 §1 的冻结契约 + Rust 单测覆盖。

## §4 已实施的小修(均带测试,已验证)

### 4-1 重启恢复不再为已删除会话开 ghost 窗口(后端,我域内文件)

- **现象**:`restore_session_windows` 只做 UUID 形状校验;持久列表里已删除会话的 id(应用关闭期间被删/手改配置)会被原样恢复成窗口。且 ghost 窗的 boot `switchToSession` → 后端 `switch_session` 对不存在的会话走 `None => Vec::new()` 分支**不报错,还把死 id `set_active` 成全局 active 会话**(`src/commands_sessions.rs:1453-1467`)——主窗若在此后冷启动,`get_conversation` 可能直接 adopt 一个已删除的会话。
- **修复**:`desktop/src/session_window_commands.rs` — 新增纯函数 `drop_dead_session_ids(:150)`,`restore_session_windows(:353)` 在打开前用一次 `l0_store().list()` 剪掉死 id;listing 失败降级为原行为(逐个 restore 失败仍会 self-heal 持久列表)。+3 个单测。
- **验证**:`cargo test --lib session_window_commands` 10/10 通过;`cargo fmt --check` 该文件 0 diff;clippy 无新告警。~40 行(含注释与测试)。

### 4-2 nightly e2e「无侧栏」断言换语义锚(前端测试)

- **现象**:`e2e/chat-script.multi-window.spec.ts` 断言 `pageB.getByRole('complementary')` count 0。RightDock 关闭时也渲染 complementary role 的塌缩 aside,且 `/chat` 是 lazy chunk——断言在 chunk 挂载前采到瞬时 0 就立即通过(**假阳性**,实测 boot 时 count=0、+1.5s 后=1,spec 仍绿纯属时序运气)。
- **修复**:同文件改锚 Sidebar 自有的 `[data-sidebar]`(与单测 `SessionWindowMode.test.tsx` 同锚),并把缘由写进注释。
- **验证**:`npx playwright test --config playwright.chat-nightly.config.ts chat-script.multi-window` 3/3 通过;`vitest run src/__tests__/SessionWindowMode.test.tsx` 5/5 通过。

## §5 与 Claude Code Desktop pane 模型的差距清单

对位基准:Claude Code Desktop 的会话组织是**单 OS 窗口内的多 pane/tab**——会话在窗口内并排(split)或成 tab,键盘驱动的 pane focus 环、会话在 pane 间移动,侧栏/全局状态单实例共享。(基于公开行为描述,非源码级;Shannon 侧结论均以本仓库代码为准。)

**可直接复用的现有资产**(pane 化不必重做):

1. **pane 级事件隔离**:`isEventForCurrentWindow` + per-session bucket 已经把「哪个会话的事件投影到哪块屏」解耦——窗筛换成 pane 筛(`windowSessionId` 变 pane 注册表)是同构改造;
2. **会话遥测**:`sessionActivity` 本就支持多会话并发跟踪(主窗跨会话可见),是 pane 活动指示器的现成数据源;
3. **审批/ask-user/budget 的会话路由**、chime 跨窗去重、boot 绑定/标题同步——都是 pane 级语义,可直接继承;
4. **OS 窗口形态**保留为「弹出独立会话」的冗余形态(对位 Codex 未满足需求,仍是差异点)。

**缺失(pane 模型有、Shannon 无)**:

| # | 能力 | 现状 | 缺口性质 |
|---|---|---|---|
| G1 | 同窗多会话并排(split pane)/tab 化 | 一个窗口至多一个会话;window mode 隐藏侧栏、钉死一会话 | 布局层全新工作;Tauri 2 无同窗 multi-webview 稳定能力 → 只能做**同 webview 内多 chat 实例**(AppContext 需从「单 visible 会话」改为「per-pane 注册表」) |
| G2 | pane 键盘导航/focus 环/会话换位 | 无;窗口间只有 Alt-Tab | 依赖 G1 |
| G3 | 每 pane 独立 active 指针 | **后端全局单 active 会话**(`registry.set_active`);`switch_session` 有副作用 | 结构性(§6-B),pane 化前置 |
| G4 | 会话生命周期 → 视图联动 | 删除会话不关其副窗(§6-A) | pane 模型里 delete 必然收 pane;现状窗模型同样需要 |
| G5 | pane 级 toast/通知路由 | toast 每窗重复(§6-C) | 小 |
| G6 | 深链/重载鲁棒性 | 副窗 F5 退化为主窗形态(§6-F) | 小 |

## §6 结构性缺口记录(**只记录,未实施**)

- **A. delete 不关副窗(活窗口半边)**:`delete_session`(`src/commands_sessions.rs:1653`)取消查询、删 L0、清 registry,但**不关闭该会话已打开的 `session-<uuid>` 窗口**,也不主动清 `open_session_windows`(重启半边已由 §4-1 修复)。活窗残留成「钉着死会话的空窗」。建议路线:delete 成功分支里 `get_webview_window("session-<id>").close()`(Destroyed 钩子自动清注册表+持久化,~6 行);或前端 `deleteSessionAction` 检测 `windowSessionId === id` 自关。涉及 sessions 域文件,留给域内实施。
- **B. 全局 active-session 单指针 + boot 竞态**:副窗 boot 用 `switch_session`(带 `set_active` 副作用);启动恢复在 `setup` 内 `block_on` 先跑,主窗随后 `get_conversation` 读的是**最后恢复的副窗会话**——多窗恢复时主窗可能 adopt 副窗的会话。建议路线:① 副窗 boot 改只读 `load_session`(不动 active 指针),或 ② `get_conversation` 增加显式 sessionId 参数,或 ③ 恢复后把 active 指针归还快照。pane 化(G3)前必须解决,中工作量。
- **C. toast 无跨窗去重**:chime 的 F3 模式(`chimeKey` + 首响胜出)可平移到 `session:auto-archived`/`auto-unarchived`/`model-override-fallback`(后两者已有 per-window dedupe set,跨窗没有)。小工作量,收益是「弹一次而不是每窗一弹」。
- **D. `subagent:start/stop` 无会话身份**:payload 只有 agentId/agentName/team,任何窗都会显示任何会话的 subagent 横幅。需后端 payload 加 `session_id` + 前端过滤,跨栈小改。
- **E. 主窗审批弹窗全放行**:`isEventForCurrentWindow(null)=true` → 主窗显示任何会话都弹任何会话的审批(nightly e2e 第三例钉死为现状)。pane 模型应路由到「该会话所在的 pane/窗」;现状至少算可用(主窗永远能审批),改造与 G1 一起做。
- **F. 副窗内重载退化**:`/?windowSession=<id>` 被 `<Navigate to="/chat">` replace 掉 query,F5/Ctrl+R 后 `parseWindowSession` 得 null → 同一窗口变完整主窗形态(侧栏回来、pin 丢失)。Tauri 生产环境无默认 reload 快捷键,触发面小。建议:`Navigate` 保留 query 或 `windowSessionId` 镜像进 `sessionStorage`。几行。
- **G. 伴生窗 prompt 只进主窗 composer**:设计如此(信任契约:只落草稿);会话窗 composer 收不到。记录备忘,若 pane 化需重定义「composer 归属」。

## §7 建议路线与工作量估计

| 阶段 | 内容 | 依赖 | 估计 |
|---|---|---|---|
| R1 立即(小修收尾) | §6-A(delete 关窗,~6-15 行);§6-F(重载保留 pin);§6-C(toast F3 平移) | 无 | 各 0.5 天内,均带测试 |
| R2 副窗体验补齐 | §6-B(active 指针去副作用:副窗 boot 改只读);§6-D(subagent payload 加 session_id) | R1 | 各 1-2 天(后者跨栈) |
| R3 pane 化 spike(对位 Claude Code) | 同 webview 多 chat 实例:AppContext 单 visible 会话 → per-pane 注册表(每 pane 独立 visibleSessionId/事件筛/bucket 投影);布局层 split/tab;G5/E 随做 | R2-B 是硬前置 | 2-3 人周 spike + 2-3 人周打磨;**不改** `open_session_window` 冻结契约(OS 窗形态保留为「弹出」) |
| 不建议 | 追 Tauri 同窗 multi-webview(2.x 无稳定能力);为 pane 化重写事件层(现有窗筛即 pane 筛的前身) | — | — |

## 附:本次改动文件

- `desktop/src/session_window_commands.rs`(ghost-restore 剪枝 + 3 单测,已验证)
- `desktop/ui/e2e/chat-script.multi-window.spec.ts`(无侧栏断言换 `data-sidebar` 锚,已验证)
- `docs/design/ui-redesign-2026-10/MULTIWINDOW-AUDIT.md`(本报告)+ `screenshots/multiwindow-0{1..5}*.png`(走查截图)
- 未 git commit(纪律要求);未触碰 batch/welcome/MCP/locale 域文件
