# 对话页 AI 改进方案(R3 审查轮 · 综合版)

**日期**: 2026-10-05 · **基线**: dev @ f91b97353(PR #266 合并点,执行前须重新 fetch)· **状态**: 调查与裁定齐备,可执行
**关联文档**: `2026-10-04-chat-r3-review-fix-schedule.md`(排期稿,内容已由本文整合取代)/ `task_plan.md`(R1/R2 修复台账)/ `2026-10-02-chat-testing-plan.md`(测试体系)/ `2026-09-26-desktop-chat-ui-round2-design.md`(Round2)
**裁定状态**: R8-①②③(专项调查)用户已确认(2026-10-04);R9-①~⑤(五项决策)用户已确认(2026-10-05)。

---

## 〇、背景、范围与方法

**触发**: 对核心 AI 对话页(`desktop/ui/src/pages/Chat.tsx` 及其全部支撑层)做一轮深度审查,产出可执行改进方案。

**范围**: 前端——`pages/Chat.tsx`、`pages/chat/*`(MessageArea/ComposerPanel/RightDock 等)、`components/chat/*`(ChatInput/MessageBubble/Markdown/StreamingResponse 等)、`context/AppContext|ChatContext` 聊天切片、`hooks/useSteerSend|useVoice|useBudgetGuard|useSessionBudget` 等;后端——`desktop/src/commands.rs`、`commands_chat.rs`、`commands_sessions.rs`、`commands_permissions.rs`、`session_registry.rs`、`crates/shannon-core`(引擎 tee、bash 工具、working_directory 配置)、`crates/shannon-tools`(system.rs)。

**方法**: 核心文件全部人工精读;外围链路三路并行探索(前后端桥接与流式、会话与状态管理、既有问题台账与测试覆盖);全部高危发现逐条回仓二次核实;所有发现对照 R1/R2/Wave3/F 系列修复台账与测试计划附录去重(已裁决项不重复列入)。

**产出**: 3 P0 + 8 P1 + 9 P2(原 P1-9 经专项调查关闭);3 项专项调查结论(R8);5 项决策裁定(R9)。

---

## 一、数据流速览(修复涉及的链路)

一条消息的完整生命周期,后续各 PR 均落在这条链上:

1. **发送**: Composer Enter → `Chat.tsx:507 handleSend`(流式中入队 `:549`/steer `:591`)→ `AppContext.sendMessage`(`AppContext.tsx:628-767`,乐观气泡 + 清流桶 + per-session 闩锁)→ `invoke('send_message')`(`tauri-api.ts:118`)。
2. **后端执行**: `commands.rs` send_message(`:1210` 起)——附件收集(读**全局** working_dir `:1259-1266`)→ 每会话 `querying` 闩锁(`:1271-1277`)→ 取消令牌(`:1280-1284`)→ 引擎逐轮重建 + `restore_session`(`:1600-1650`)→ spawn 流式任务,立即返回 `query_id`。
3. **引擎**: `agent_loop.rs` producer——轮首写 L0 tee(`:817-818` user 消息 + turn/start,durable boundary flush)→ 事件经 256 有界 channel(`engine/events.rs:27`)+ `AbortOnDropStream`。
4. **回传**: desktop `stream_step`(`commands.rs:1180-1192`,`tokio::select! biased` 取消优先)映射为 `query:text/tool-start/tool-result/tool-progress/thinking/usage/completed/failed/cancelled` 全局 emit(fire-and-forget),同时 `route_event` 克隆进进程内 per-session 无界通道(`:1712-1713`,零消费者)。
5. **渲染**: `AppContext` 一次性注册全部 listener——query_id 归属/新鲜度双过滤(`:286-415`)→ 按 session 分桶(`streamingBucketsRef`)→ 50ms 节流投影到可见会话 → 终态提交气泡并清闩锁(`:1190-1420`)。

---

## 二、总体评估

**成熟面(经 R1→R2→Wave3→F 系列共 35 个修复 PR + 两轮测试体系建设的沉淀,以下机制为「改动时不得破坏的契约」)**:

| 机制 | 位置 | 作用 |
|---|---|---|
| query_id 退休集 + owner 反查路由 | `AppContext.tsx:286-415` | 迟到/串写事件过滤,stop→重发竞态 |
| 切换单调 token + 同会话早退 | `AppContext.tsx:854-905` | 慢响应不回写旧状态 |
| 乐观气泡按对象引用回滚 | `AppContext.tsx:683-702, 748-754` | 同文气泡不误删 |
| IME 三重守卫 | `ChatInput.tsx:644-659, 1239-1248` | Chrome/Safari/Firefox 三种 composition 时序 |
| 队列熔断 + 容量上限 | `Chat.tsx:638-666`、`AppContext` D7 注释 | 被拒后不连环烧队列 |
| L0 tee 轮首落盘 + durable boundary flush | `agent_loop.rs:817-818`、`tee.rs:637,T6` | 切换/重载投影契约(R8-③ 已核实) |
| 引擎侧取消兜底 | `AbortOnDropStream`、bash `kill_on_drop`、流 idle 超时(有测试) | 无孤儿进程/无死等 |
| 附件 preflight + 诚实徽标 | `ChatInput.tsx:243-286`、`commands.rs` 收据 | 发送前可见拒绝原因 |
| budget 双向守卫(前端横幅 + 后端 pre-turn/mid-turn) | `useBudgetGuard`、send_message | 预算触顶可见可控 |

**问题分布定性**: 本轮发现集中在三类此前修复轮次覆盖较少的区域——① 后端共享可变状态(进程 CWD、全局 working_dir、registry);② 多会话/多窗口时序边角(steer 槽位、后台队列、跨窗口闩锁);③ 终态与清理生命周期(权限弹窗、删除会话、事件丢失兜底)。

---

## 三、问题清单

### 3.1 P0(3 项,全部确认)

**P0-1 steer 单槽位跨会话覆盖,用户消息静默丢失** → B1-1
- 位置: `useSteerSend.ts:80`(全局单槽 `pendingRef`)、`:146-152`(无条件覆盖)、`:172-176`(投递完成无条件清槽);`Chat.tsx:603-607`(接受时已清 composer 与草稿)。
- 影响: 会话 A steer 未 settle 时切到 B 再 steer,A 的文本永久丢失且无提示;投递期间新 park 的 steer 会被 `.then` 的 null 抹掉。
- Imp-2 修复只解决了「settle 属于哪个会话」的路由,未解决槽位抢占。

**P0-2 `switch_session` 修改进程级 CWD 与全局 working_dir,跨会话污染** → B2-1/B2-2
- 位置: `commands_sessions.rs:1103-1110`(每次切换 `set_current_dir` + 写全局 `desktop_cfg.working_dir`;另 `:1183` 改目录、`:1245` worktree 创建同款);消费端——附件解析读全局指针(`commands.rs:1259-1266`)、`resolve_working_dir` 回退进程 CWD(`commands_agents.rs:196-204`)。
- 引擎四个 CWD 读点: project instructions(`system_prompt.rs:87`)、env block(`agent_loop.rs:653`)、bash 执行(`system.rs:1859` 构造 `ProcessRequest` 不带 cwd → 子进程继承 spawn 时刻进程 CWD,`providers.rs:220/328/438/515`)、repo map 回退(`repo_map_injector.rs:224`)。
- 唯一的按会话修复只覆盖 memory key(`commands_memory.rs:88-91` 引擎构建时冻结 CWD;`types.rs:985-991` 注释自认竞态已知),且多窗口下冻结的源值本身可能就错。
- 三层影响: (a) 多窗口/跨会话发送时 prompt/memory key/bash 目录全用错;(b) turn 内切换会话,进行中 turn 的后续 bash 在新目录执行而模型仍被告知旧目录;(c) 附件按全局指针解析。

**P0-3 进程内 per-session 事件通道无界且零消费者,内存无界增长** → B2-3(删除)
- 位置: `session_registry.rs:246`(unbounded channel,注释自认无背压)、`:261-269`(`try_send_event` 永远成功)、`:274-277`(`take_event_receiver` 生产零调用,grep 全仓仅测试);每事件克隆入队 `commands.rs:1712-1713`。
- 影响: 所有会话所有轮次的完整事件常驻内存,长时间挂机单调增长;配合 P1-4 的 registry 不回收,删除会话也无法释放。
- 关联: chat-upgrade.md P2-5b spike 的「有界 channel 背压」未做项即指此通道,但 `route_event` 在生产主路径,非休眠代码。

### 3.2 P1(8 项;原 P1-9 经 R8-③ 调查关闭)

**P1-1 权限审批弹窗与运行终态脱节(幽灵弹窗)** → B1-2
- `AppContext` 三个终态处理器(`:1190-1420`)均不清理 `permissionRequest` 与 rail 黄点;唯一清除点是用户作答(`:944`);后端 300s 超时自动 Deny(`commands_permissions.rs:99-120`),届时再点「允许」报 not found。`PERMISSION_REQUEST` 处理器亦无 query_id 新鲜度过滤,迟到请求也会弹。

**P1-2 后台会话 prompt 队列永不 drain** → B3-1(徽标)
- drain effect 仅存在于 Chat 页且只投影可见会话(`Chat.tsx:639-666`;`isQuerying`/`promptQueue` 均为 `windowSessionId ?? currentSessionId` 投影);AppContext 无跨会话 drain。会话 A 排队后切走,队列无限期滞留且无感知。

**P1-3 终态事件丢失无兜底,composer 可能卡死** → B1-4
- 后端全部 emit 为 fire-and-forget(`commands.rs:1893/1951/2102/2255` 等),无 ack/重放(对比 terminal 模块有 seq+replay,`tauri-api.ts:3067-3076`);前端闩锁只在三个终态处理器清除。另有取锁与存令牌之间的取消空窗(`commands.rs:1271-1284`),落在空窗的 cancel 静默成功、无 `query:cancelled` 可等——steer 有 15s 兜底(`useSteerSend.ts:70,108-124`),普通 stop 没有。`get_status` 已返回 per-session `querying`(`commands_chat.rs:193-212`),对账抓手现成。

**P1-4 删除会话清理缺口(草稿泄漏 + 槽位残留 + 后端不取消)** → B1-3
- 前端 `deleteSessionAction`(`AppContext.tsx:907-928`)不清 `shannon.draft.<id>`(只有可见会话的发送/清空路径会清,`Chat.tsx:82-84`)、`queryingSessions`/`cancelInFlightSessions`/`sessionSources`/`pendingInjectedMemoriesRef`;后端 `delete_session`(`commands_sessions.rs:1268-1322`)不取消运行中的 query、不回收 registry 条目(R8-② 确认,后果链见 §四)。

**P1-5 useVoice provider 首挂载冻结,本地 STT 用户静默走云端** → B1-5
- `useVoice.ts:68-75` provider 仅首次构建;`ChatInput.tsx:335-351` 传入的 `config?.voice_local?.enabled` 在冷启动直进 /chat 时为 undefined → 按 cloud 构建,配置加载后不重建。与 P2-5e 意图相反,涉及隐私预期。

**P1-6 流式 Markdown 全量重解析(O(n²))** → B3-2
- `StreamingResponse.tsx:59` 每个 flush(50ms 节流)对**全部累积文本**重跑 remark-gfm + remark-math + rehype-highlight + sanitize + katex(`Markdown.tsx:51-95`);长回复下 UI 线程占用随长度线性上涨,jsdom 测不出。

**P1-7 双窗口同会话并发:被拒方 catch 无条件清赢家闩锁** → B1-6
- `AppContext` sendMessage catch(~`:756`)无条件 `setSessionQuerying(target,false)`;后端锁是每会话单锁(`commands.rs:1271-1277`)。输家把赢家的闩锁清掉 → UI 显示空闲但流仍在跑,直到 run settle。

**P1-8 发送被拒的恢复路径覆盖正在输入的草稿** → B1-6
- 队列 drain 被拒(`Chat.tsx:659-661`)与手动发送被拒恢复(`:579-581`)都直接 `setInput(...)`,settle 瞬间/IPC 在途期间用户正在输入的内容被顶掉。

### 3.3 P2(9 项)

| ID | 问题 | 位置 | 承接 |
|---|---|---|---|
| P2-1 | regenerate 不检查 `sendMessage` 返回值,被拒仍 toast 成功(与 `commitEdit` 的正确分支不一致) | `MessageBubble.tsx:279-294` | B1-6 |
| P2-2 | 粘贴图文混排丢文本(整体 preventDefault 只留图片) | `ChatInput.tsx:597-602` | B3-3 |
| P2-3 | slash/mention listbox 未与 textbox 关联(无 `aria-controls`/`aria-activedescendant`);`:157-158` 注释与 `:1006-1011` 的 P2-9 修订方案自相矛盾;`slashOptionId` 为死代码 | `ChatInput.tsx:157-158, 1213-1250` | B3-3 |
| P2-4 | 模型输出 markdown 中的远程图片直接加载(tracking/借 URL 外洩上下文) | `Markdown.tsx:421-452` + sanitize schema | B3-4 |
| P2-5 | `stream_step` biased 取消优先可吞同刻 `Completed`(与 L0 呈现分叉);交互循环 `Failed` 臂无 break(隐式契约) | `commands.rs:1184-1192, 2228-2281` | B3-3 |
| P2-6 | 滚动细节: 流式期每 50ms 重触发 smooth scroll;FAB scroll listener 随 messages.length 重订阅 | `Chat.tsx:425-434`、`MessageArea.tsx:259-269` | B3-3 |
| P2-7 | `useSessionBudget` 切会话销毁重建监听有空窗(同族问题 `useBudgetGuard.ts:45-58` 已用 ref 模式修复,模式未统一) | `useSessionBudget.ts:43-54` | B3-3 |
| P2-8 | `createSessionInWorktree` 失败遗留孤儿会话(注释自认) | `AppContext.tsx:815-844` | B3-3 |
| P2-9 | 队列/steer 纯内存,崩溃即丢(D7 已裁定不持久化,仅随 B3-1 复审 crash-recovery 边角) | `AppContext.tsx:84-87` | 搁置 |

---

## 四、专项调查结论(R8,用户已确认 2026-10-04)

### R8-① P0-2 影响面 —— 确认,结构性,两步修复

完整证据链见 §3.1 P0-2。要点: 竞态是团队已知的(types.rs 注释),但 4 个 CWD 消费点只修了 memory key 一处;冻结的源值在多窗口下本身可错。**修复分两步**: 止血 B2-1(desktop 侧,`send_message` 按目标会话 wd 先 set + 附件改读会话 meta)+ 正解 B2-2(shannon-core `working_directory` 全链路,完成后移除全部进程级翻转)。诚实声明: 止血仍有三个残留——turn 内漂移、set→读之间的竞态窗口(且 set 瞬间会把另一流式会话的 bash 带偏,进程单例架构性无解)、set 失败被吞需补 warn。

### R8-② P1-4 删除运行中会话 —— 确认不取消,后果链四条

`delete_session` 只做删 L0 目录/移出列表/清 worktree/emit。后果: ① run 继续烧 API 费用至自然结束;② tee writer 写向已 unlink 的 inode(静默进黑洞);③ 终态事件路由到死 key,`setSessionQuerying` 重建死闩锁条目;④ **加重 P0-3**——registry 条目不回收,已删会话的无界通道连同全部历史事件在进程生命周期内无法释放。

### R8-③ P1-9 切回丢乐观气泡 —— 不成立,关闭

user 消息在**轮首**写入 L0(`agent_loop.rs:817-818`,producer 启动即 `record_user_message_with_count` + `record_turn_start`),且属 tee 的**强制 durable boundary flush** 类别(`tee.rs:637`,T6 测试钉住)。流式中切走再切回,投影必含本轮 user 气泡,「无问有答」不存在。处置: 关闭,可选加一条 e2e 断言钉契约(B3-5)。

---

## 五、裁定记录(R9,用户已确认 2026-10-05)

**R9-① R8-① 修复深度**: 止血 B2-1 立即做 + 正解 B2-2 下一迭代必做(不自动滑)。核心理由: 止血只覆盖发送时点且残留三个窗口(见 R8-①),正解把 CWD 从进程单例变为引擎实例属性是唯一根治;拆两步是 blast radius 控制(B2-2 动 shannon-core 三读点 + 六构建点 + 须回归 REPL/server fallback)。翻案条件: 产品裁定多窗口/后台会话为非目标 → B2-2 降级「接受现状 + 文档化」。

**R9-② P1-2 后台队列**: 侧栏「队列 N」徽标(本轮落),auto-drain 不做。核心理由: auto-drain 三个被低估成本——drain 编排与 steer 插队/熔断/A-22 咬合搬动即重写状态机;后台连发是预算行为变化与 budget guard 语义冲突;失败路径无落点(后台无 composer,牵出 notification backlog)。徽标把静默滞留变可见滞留,信任问题的根源是「不知道」而非「没自动发」。翻案触发器: 用户反馈想要后台续发 → 按预算感知 auto-drain 立项(仅明确入队的、逐条过 pre-turn guard、失败进 inbox)。

**R9-③ P0-3 通道处置**: 删除。核心理由: bounded 无消费者 = 积满即丢纯浪费;接消费者的唯一候选 P2-5b spike 未挂载、自列未做项含消费者与背压、chat.v2 已裁走自研路线。附带收益: 每事件 payload clone 消失,配合 B1-3 删除会话零泄漏。翻案条件: 仅当 P2-5b 确认复活,届时在复活分支按原设计重建(有界+消费者一起建)。

**R9-④ P2-4 远程图片**: 默认拦截 + 按图确认 + 全局开关(默认关,挂 AdvancedSettings)。核心理由: `![](https://…?d=<上下文片段>)` 渲染即外洩,模型输出是天然注入面,与导出沙箱(R1)/MCP 审批同族;「local-first + keychain + 0600」人设下维持现状是双标。不采用 schema 级删 http(s)(伤及浏览器打开路径),采用渲染时门控(LocalImage 组件),占位显示域名+尺寸。翻案条件: 内置浏览器渲染成为一等公民 → 改「会话级允许 + 域名白名单」。

**R9-⑤ 滑期政策**: 批次 1(B1-1/2/3/5/6)与 B2-1 不可滑;**B1-4 为批次内唯一可滑项**(健壮性修复、触发概率最低,可滑至批次 2 开头);B2-2 下一迭代必做、翻案需明示;B2-3 与批次 3 可滑一季度,其中 B3-2(流式 Markdown 性能)为容量优先项。所有滑期沿用 R7-⑤ 前置(fetch + 重叠文件域检查);B2-2 与 mobile/relay 主线(#263-#266)文件域不相交,可并行。

---

## 六、改进方案(批次与 PR)

### 批次 1 · 正确性与数据安全(信任修复,不可整体滑;~5-7 天,6 PR 可并行)

| PR | 内容 | 规模 | 验收要点 |
|---|---|---|---|
| **B1-1** | P0-1 steer 槽位 Map 化: `pendingRef` 改按 sessionKey 的 Map;投递完成只清自己 key;被覆盖前归还原会话 composer 或 toast | S-M | 跨会话两次 steer 均送达;投递期间新 park 不被抹;`ChatInputSteer` 扩展跨会话用例 |
| **B1-2** | P1-1 权限弹窗生命周期: 三个终态处理器按 key 清 `permissionRequest` + `noteSessionApproval(sid,false)`;`PERMISSION_REQUEST` 补 query_id 新鲜度过滤;删除会话同步清其弹窗 | S | 审批挂起时 cancel/fail → 弹窗消失黄点灭;不再出现 300s 后点允许报 not found |
| **B1-3** | R8-② + P1-4 删除清理: 后端 delete_session 先 take+cancel(发完即删不等待)+ `SessionRegistry::remove`;前端补清 draft 键、`queryingSessions`/`cancelInFlightSessions`/`sessionSources`/`pendingInjectedMemories` | S-M | 删除运行中会话 → run 取消、token 停烧;localStorage 无残留键;重启后 registry 无死条目 |
| **B1-4** | P1-3 终态兜底: 普通 stop 补 settle 超时(参照 steer 15s 模式);cancel 返回后或 isQuerying 静默超阈值时 `get_status` 单次对账 | S-M | 测试桩丢弃 terminal 事件 → composer 数秒自愈;停止按钮不再永久 cancelling |
| **B1-5** | P1-5 useVoice provider 重建: 配置签名变化时重建实例(或 startRecording 时按最新 config 解析) | S | 本地 STT 用户冷启动直进 /chat 录音走本地;设置切换同挂载周期生效 |
| **B1-6** | P1-7/P1-8/P2-1 发送路径三小修: catch 仅在本次发送拥有该 run 时清闩锁;被拒恢复不覆盖非空 composer(转 blockedPayload/banner);regenerate 检查返回值 | S×3 | 双窗口输家不清赢家闩锁;settle 瞬间打字不被顶掉;budget 拒绝时报错而非成功 toast |

优先序: B1-1 > B1-3 > B1-2 > B1-4 > B1-5 > B1-6;B1-4 滑期例外见 R9-⑤。

### 批次 2 · 结构性修复(立项,~1-2 周,串行为主)

| PR | 内容 | 规模 | 验收要点 |
|---|---|---|---|
| **B2-1** | R8-① 止血: `send_message` 按目标会话 wd 先 `set_current_dir`(失败补 warn)再建引擎;附件解析改读会话 meta | S-M | 多窗口下向不同目录会话先后发送,prompt env block/memory key/bash 均落各自目录 |
| **B2-2** | R8-① 正解: `QueryEngineConfig.working_directory` 全链路——project instructions(`system_prompt.rs:87`)、env block(`agent_loop.rs:653`)、bash 默认 cwd(`system.rs:1859` 一带)三个读点 prefer config、fallback CWD;desktop 按会话 meta 设置;完成后移除 `switch_session`(`:1103`)/`change_working_dir`(`:1183`)/worktree(`:1245`)/send_message 的全部进程级 `set_current_dir` | M-L | grep 全仓生产路径零 `set_current_dir`;turn 内切换不再影响进行中 turn;每读点一个单测;REPL/server fallback 行为回归通过 |
| **B2-3** | R9-③ 无界通道删除: `route_event`/`try_send_event`/`events_tx/events_rx`/`take_event_receiver` 全链移除;SessionState 瘦身 | S | 长跑内存平稳;session_registry 测试同步清理;P2-5b 复活时按原设计重建(有界+消费者) |

依赖: B2-1 先行;B2-2 落地后回删 B2-1 的 set_current_dir;B2-3 独立可并行。

### 批次 3 · 体验/性能/卫生(可滑一季度)

| 项 | 内容 | 规模 |
|---|---|---|
| **B3-1** | P1-2 队列可见性(R9-②): SidebarSessions rail 行加「队列 N」chip(同构先例: running 圆点/elapsed/goal 徽标;投影 per-session promptQueues 即可) | S |
| **B3-2** | P1-6 流式 Markdown 增量渲染: 已定稿段落(空行界)缓存 + 仅活跃尾部重解析;或流式期关 rehype-highlight、定稿再高亮。批次 3 内容量优先项 | M |
| **B3-3** | 卫生簇六件: P2-2 粘贴保文本 / P2-3 listbox 关联+清死 id+修注释 / P2-5 defensive break(`commands.rs:2228` Failed 臂)/ P2-6 滚动细节 / P2-7 监听模式统一 / P2-8 孤儿会话回滚 | S×6 |
| **B3-4** | P2-4 远程图片门控(R9-④): LocalImage 渲染时门控——http(s) 占位(域名+尺寸)+ 单图「加载」+ AdvancedSettings 开关(默认关,含 i18n 四语言);sanitize schema 不动 | S-M |
| **B3-5** | 测试补充包(见 §七) | S×5 |

---

## 七、回归与测试计划

**新增(与 PR 一一对应)**:
1. B1-1: 跨会话 steer 双投递、投递中新 park 不被抹(`ChatInputSteer` 扩展)。
2. B1-2: 审批挂起 × {cancel, fail, 删除会话} → 弹窗清、黄点灭(`chat-script.approval` 扩展)。
3. B1-3: 删除运行中会话 → 后端 cancel 生效;localStorage `shannon.draft.<id>` 无残留;registry 无死条目( Rust 侧单测)。
4. B1-4: 测试桩丢弃 terminal 事件 → settle 兜底触发、`get_status` 对账复位。
5. B1-5: config 后到/变更 → provider 重建(本地路由断言)。
6. B1-6: 双窗口同会话输家不清闩锁;被拒恢复保留输入;regenerate 拒绝分支。
7. B2-1/2: 每 CWD 读点一个单测(config 设置时用 config、未设置回退 CWD);e2e 多窗口先后发送目录断言。
8. B3-1: 后台排队 → rail 徽标计数;可见会话 QueueChips 不回归。
9. B3-2: >8K token 长回复流式主线程长任务采样(perf spec 扩展)。
10. B3-4: 远程图占位/确认/开关三态;本地路径不受影响。
11. 可选: R8-③ tee 契约钉(流式中切走→切回,user 气泡在场)。

**不得回归的既有锚点**: 32 条 chat-script journey、fuzz 13 变异 + KNOWN_FUZZ_WEDGES、axe 动态全规则 a11y、visual-matrix 12 基线(light×dark;B3-4 占位态需补基线)、i18n-theme、multi-turn-stream/session-switch/cancel-matrix 等;§二表格所列防御机制为代码评审时的「不得破坏」清单。

**门禁**: vitest 全绿 + e2e 默认矩阵 + a11y + visual matrix + i18n;批次 2 另需 crates 侧 `cargo test`(shannon-core/shannon-tools)与 `chat_contract_smoke`。

---

## 八、风险与缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| B2-2 blast radius(shannon-core 三读点 + 六构建点,REPL/server 共用) | 其他宿主行为变化 | fallback 保持 CWD 语义;每读点单测;`chat_contract_smoke` + REPL 手工回归 |
| B2-1 残留窗口(turn 内漂移、并发瞬间) | 长期存在直至 B2-2 | R9-① 已明示接受;B2-2 下一迭代必做不自动滑 |
| B2-3 删通道误伤未来需求 | P2-5b 复活需重建 | 复活分支按原设计重建(有界+消费者),文档已留痕 |
| B1-3 cancel 后即删,terminal 事件晚到 | 前端死 key 重建闩锁条目 | 前端删除时已清 query 记录,事件按 owner 路由自然丢弃;单测钉住 |
| B3-4 默认拦截改变现有阅读体验 | 用户困惑「图没了」 | 占位显示域名+尺寸+单图加载;开关可全局放行;release note 说明动机 |
| B3-2 改渲染结构 | sanitize/KaTeX 顺序、代码块高亮回归 | 保留「sanitize 在前、katex 在后」契约;visual-matrix + Markdown 单测全量跑 |

---

## 九、执行方式

沿用 R7-⑤ 既定管线: worktree 按文件域拆 brief → agent 并行实现 → 双组审查 → 顺序合并。批次 1 六个 PR 文件域互不相交可全并行;批次 2 内 B2-1 → B2-2 串行、B2-3 独立;执行前 fetch + 重叠文件域检查(本轮 dev 主线在 mobile/relay,#263-#266,与全部批次文件域不相交)。合并顺序: 批次 1 按 B1-1 → B1-3 → B1-2 → B1-6 → B1-5 → (B1-4) 收口后进批次 2。

---

## 十、搁置项与触发器

| 项 | 处置 | 触发器 |
|---|---|---|
| 队列/steer 持久化(P2-9) | 维持 D7 裁定 | B3-1 徽标上线后随季度复审一并议 crash-recovery |
| biased cancel 吞 Completed 的同刻竞态(P2-5 另一半) | defensive break 落地后接受现状 | 概率极低且有 L0 日志可对账;用户直报即升级 |
| steering 第三档 / 多窗口后台完成 chips | 维持既有搁置 | 沿用 task_plan.md 既有触发器(季度复审/用户直报) |
| auto-drain(R9-② 翻案面) | 不做 | 用户反馈「希望后台继续发」→ 预算感知方案立项 |

---

## 十一、整轮验收标准(Definition of Done)

1. 批次 1 全绿: §七新增测试 1-6 落地,既有锚点零回归。
2. 内存: 多会话长时间使用(>100 turn/会话)+ 反复创建删除会话,进程 RSS 平稳(对应 P0-3 + R8-②④)。
3. 目录正确性: 多窗口/先后发送场景下 prompt env block、memory key、bash、附件四者均落目标会话目录(B2-1 验收;B2-2 后 grep 零生产 `set_current_dir`)。
4. 行为: 跨会话 steer 不丢文本;审批挂起时 cancel 不留幽灵弹窗;删除运行中会话停烧 token。
5. 文档: 本文件随各 PR 落地勾销对应条目;CHANGELOG 按批次归并。

---

## 十二、变更记录

- v1(2026-10-05): 综合三轮工作成文——深度审查发现清单(§三)、专项调查 R8(§四)、五项裁定 R9(§五)、批次方案(§六)及测试/风险/执行/验收(§七-十一)。取代 `2026-10-04-chat-r3-review-fix-schedule.md`。
