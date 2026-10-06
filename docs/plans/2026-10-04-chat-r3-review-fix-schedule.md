# 对话页 R3 深度审查修复排期 v2(三项确认已定案;R8 五项裁定为推荐稿,待最终拍板)

> **已整合取代(2026-10-05)**: 本文件内容连同五项裁定的确认结果,已综合进权威版改进方案 `2026-10-05-chat-r3-improvement-plan.md`。后续执行以该文件为准,本文件仅存档审查与裁定过程。

**日期**: 2026-10-04 · **基线**: dev @ f91b97353(PR #266 合并点,执行前须重新 fetch)· **来源**: 2026-10-04 对话页深度审查(3 P0 / 9 P1 / 9 P2,报告见会话记录;本文件为可执行排期稿)

**裁定状态**: R8-①②③(原「待确认」三项)用户已确认采纳(2026-10-04);决策点 1-5 的推荐结论见 §五(含推理与翻案条件)。

---

## 〇、审查结论摘要

- 已排除与既有台账(R1/R2/Wave3/F 系列/测试计划附录 A)重复的已裁决项(A-10、D7、continueTarget、F-voice-gate 等)。
- 问题集中在三类:后端共享可变状态(working_dir/CWD、registry)、多会话/多窗口时序边角(steer、队列、latch)、终态与清理生命周期(权限弹窗、删除会话、事件丢失兜底)。
- 原报告 3 处「待确认」已全部查证完毕,裁定建议见 §一(R8-①②③),其中 2 项确认、1 项关闭。

---

## 一、R8 裁决建议(原「待确认」项的调查结论,供拍板)

### R8-① P0-2 working_dir 污染 —— **确认,影响面为结构性,建议两步修复(止血 + 正解)**

**调查结论**(全部亲核):

1. **引擎全链路直接读进程 CWD**:
   - project instructions(CLAUDE.md/AGENTS.md): `crates/shannon-core/src/query_engine/system_prompt.rs:87` 在轮首 prompt 构建时读 `std::env::current_dir()`;
   - env block(cwd/date/platform/git,模型被告知的"你在哪个目录"): `agent_loop.rs:653`;
   - bash 执行: `crates/shannon-tools/src/system.rs:1859` 构造 `ProcessRequest` 不带 cwd → 子进程继承**spawn 时刻**的进程 CWD(`providers.rs:220/328/438/515` 仅在 cwd 有值时设置);
   - repo map 回退: `repo_map_injector.rs:224`。
2. **唯一的按会话修复只覆盖了 memory key**: `desktop/src/commands_memory.rs:88-91` 在引擎构建时冻结 `current_dir()` 作为 memory 项目键(types.rs:985-991 注释自认「desktop used to flip the process cwd on every session switch, racing the memory project key」——**竞态是团队已知的,但只修了 1/4 个消费点**)。且冻结的源值本身就可能是错的: 多窗口下最后一次 `switch_session` 决定进程 CWD,另一窗口发送时冻结的是错目录。
3. **`switch_session` 翻转进程 CWD + 全局 working_dir**: `commands_sessions.rs:1103-1110`(另 `:1183` 改目录、`:1245` worktree 创建同样翻转);而 `send_message` **不**按目标会话重设 CWD,附件解析读全局指针(`commands.rs:1259-1266`,注释还特意禁用了 CWD 回退,但没禁用全局指针)。
4. **三层实际影响**:
   - (a) 多窗口/跨会话发送: turn 的 project instructions、env block、memory key、bash 目录全部可能用错会话的目录;
   - (b) 单窗口 turn 内漂移: 后台 turn 流式中用户切走,CWD 翻转 → 该 turn **后续** bash 在新目录执行,而 env block 告诉模型的还是旧目录(相对路径失败、git 看错仓库);
   - (c) 附件解析: 已证实按全局指针解析(`commands.rs:1259-1266`)。

**推荐修复(两步)**:
- **止血(B2-1, S-M)**: `send_message` 按 target 会话的持久 `working_dir` 先 `set_current_dir` 再建引擎;附件解析改读会话 meta。诚实声明: 进程级 set 仍是全局可变状态,两窗口对不同目录会话**同时**发送的瞬间仍可错——只把窗口从「随时错」缩到「并发瞬间可错」,不能根治。
- **正解(B2-2, M-L)**: `QueryEngineConfig.working_directory` 已存在(types.rs:985)且 memory 已用——把其余三个读点(project instructions、env block、bash 默认 cwd)统一改为 prefer `config.working_directory`、fallback CWD;desktop 按会话 meta 设置;完成后移除 `switch_session` 等处的进程级 `set_current_dir`。**推荐两步都做,B2-1 先行,B2-2 立项跟进。**

### R8-② P1-4 删除运行中会话 —— **确认,后端不取消,建议 cancel + registry 回收**

**调查结论**:`delete_session`(`commands_sessions.rs:1268-1322`)只做四件事: 删 L0 目录、移出 sessions 列表、清 worktree、emit SESSIONS_UPDATED。**不取消运行中的 query**。后果链:

1. run 继续跑完——继续烧 API token/费用,直到自然结束;
2. tee 持有的 writer 写向已 unlink 的 inode(Unix 下静默进黑洞,不报错、不持久化);
3. 终态事件仍 emit,前端已删该 key 的 query 记录,`setSessionQuerying(key,false)` 在 map 里重建死条目(轻微泄漏);
4. **加重 P0-3**: registry 条目不删,已删会话的 SessionState(含其无界事件通道里累积的全部历史事件)在进程生命周期内**永远无法释放**——每删一个会话泄漏一份。

**推荐**: `delete_session` 在删 L0 前先执行与 `cancel_session_query` 同款的 take+cancel(发完即删,无需等待 terminal;后续写黑洞无害);给 `SessionRegistry` 加 `remove`,删除时一并回收。前端 `deleteSessionAction` 补齐清理(见 B1-3)。规模 S-M。

### R8-③ P1-9 流式中切回丢乐观气泡 —— **不成立,关闭**

**调查结论**: user 消息在**轮首**即由引擎写入 L0——`agent_loop.rs:817-818`(producer 启动时立即 `tee.record_user_message_with_count()` + `record_turn_start()`);且 user message 属于 tee 的**强制 durable boundary flush** 类别(`tee.rs:637` 注释列明,T6 测试覆盖边界刷盘语义)。因此 `switch_session` 的 L0 投影在流式中途切回时必含本轮 user 气泡,「无问有答」场景不存在。

**推荐**: 关闭该项,无需代码修复。可选: 在 session-switch e2e 里补一条断言钉住该契约(流式中切走→切回,user 气泡在场),防未来 tee 时序重构时回归。规模 S,可选。

---

## 二、批次 1 · 正确性与数据安全(信任修复,不可整体滑期;~5-7 天,6 PR 可并行)

| PR | 内容 | 规模 | 验收要点 |
|---|---|---|---|
| **B1-1** | P0-1 steer 槽位 Map 化: `pendingRef` 改按 sessionKey 的 Map;投递完成只清自己 key;被覆盖前先归还原会话 composer 或 toast | S-M | 跨会话两次 steer 均送达;投递期间新 park 不被抹;`ChatInputSteer` 扩展跨会话用例 |
| **B1-2** | P1-1 权限弹窗生命周期: 三个终态处理器按 key 清 `permissionRequest` + `noteSessionApproval(sid,false)`;`PERMISSION_REQUEST` 补 query_id 新鲜度过滤;删除会话时同步清其弹窗 | S | 审批挂起时 cancel/fail → 弹窗消失黄点灭;不再出现 300s 后点允许报 not found |
| **B1-3** | R8-② + P1-4 删除清理: 后端 cancel + registry remove;前端 `deleteSessionAction` 补清 `shannon.draft.<id>`、`queryingSessions`/`cancelInFlightSessions`/`sessionSources`/`pendingInjectedMemories` | S-M | 删除运行中会话 → run 取消、token 停烧;localStorage 无残留键;重启后 registry 无死条目 |
| **B1-4** | P1-3 终态兜底: 普通 stop 补 settle 超时兜底(参照 steer 的 15s 模式);cancel 返回后或 isQuerying 静默超阈值时 `get_status` 单次对账(`commands_chat.rs:193` 已有 querying 字段) | S-M | 人为丢 terminal 事件(测试桩)→ composer 数秒内自愈;停止按钮不再永久 cancelling |
| **B1-5** | P1-5 useVoice provider 重建: 配置签名变化时重建实例(或 startRecording 时按最新 config 解析) | S | 本地 STT 用户冷启动直进 /chat 录音走本地;设置切换后同挂载周期生效 |
| **B1-6** | P1-7/P1-8/P2-1 发送路径三小修: catch 条件清 latch(仅本次发送拥有该 run 时);被拒恢复不覆盖非空 composer(转 blockedPayload/banner);regenerate 检查 `sendMessage` 返回值 | S×3 | 双窗口同会话并发输家不清赢家闩锁;settle 瞬间打字不被顶掉;budget 拒绝 regenerate 时报错而非成功 toast |

**批次内优先序**: B1-1(数据丢失)> B1-3(费用+泄漏)> B1-2 > B1-4 > B1-5 > B1-6。
**滑期例外(对抗性自查修订)**: B1-4(终态兜底)是健壮性修复而非数据/费用损失,触发概率在批次内最低(emit 丢失本身罕见)——若工程排期紧,可单独滑至批次 2 开头;其余五项不可滑。

---

## 三、批次 2 · 结构性修复(立项,~1-2 周,串行为主)

| PR | 内容 | 规模 | 验收要点 |
|---|---|---|---|
| **B2-1** | R8-① 止血: `send_message` 按 target 会话 wd 先 `set_current_dir` 再建引擎;附件解析改读会话 meta | S-M | 多窗口下向不同目录会话先后发送,prompt env block/memory key/bash 均落各自目录(并发同时发送的残留窗口由 B2-2 根治) |
| **B2-2** | R8-① 正解: shannon-core `working_directory` 全链路(project instructions/env block/bash 默认 cwd 三个读点 prefer config),desktop 按会话设置;完成后移除 `switch_session`/`change_working_dir`/worktree 创建的进程级 `set_current_dir` | M-L | grep 全仓无生产路径 `set_current_dir`;turn 内切换会话不再影响进行中 turn 的 bash 目录;回归: 每读点一测试 |
| **B2-3** | P0-3 无界通道处置(**推荐定案: 删除**): `route_event`/`try_send_event`/`events_tx/events_rx` 全链移除(`app.emit` 已覆盖前端;`take_event_receiver` 生产零调用已核实);SessionState 瘦身,配合 B1-3 的 registry remove 后删除会话零泄漏 | S | 长时间多会话使用内存平稳;session_registry 相关测试同步清理;P2-5b 若复活按其原设计重建(有界+消费者一起建) |

---

## 四、批次 3 · 体验/性能/卫生(可滑一个迭代)

| 项 | 内容 | 规模 |
|---|---|---|
| **B3-1** | P1-2 后台会话队列可见性(**推荐定案: 徽标,不做 auto-drain**): SidebarSessions rail 行加「队列 N」chip(rail 已有 running 圆点/elapsed/goal 徽标三套同构先例;AppContext 已持有 per-session promptQueues,投影即可)。auto-drain 挂触发器: 用户反馈「希望后台继续发」时按预算感知方案立项 | S |
| **B3-2** | P1-6 流式 Markdown 增量渲染: 已定稿段落缓存 + 仅活跃尾部重解析;或流式期关闭 rehype-highlight。**批次 3 内最不该滑的一项,容量出现时优先** | M |
| **B3-3** | 卫生簇: P2-2 粘贴图文混排保文本;P2-3 slash/mention listbox 补 `aria-controls`/清死 id/修 157 行过时注释;P2-5 biased cancel 加 defensive break(`commands.rs:2228` Failed 臂);P2-6 滚动细节(流式期 `auto`、FAB listener ref 化);P2-7 useSessionBudget 对齐 useBudgetGuard 的 ref 模式;P2-8 worktree 失败孤儿会话回滚 | S×6 |
| **B3-4** | P2-4 远程图片策略(**推荐定案: 默认拦截**): 渲染时门控——`LocalImage` 对 http(s) src 渲染占位(域名+尺寸)+ 单图「加载」确认按钮 + AdvancedSettings 全局开关(默认关);不动 sanitize schema,外链在浏览器打开的路径(P0-A interceptor)不受影响 | S-M |
| **B3-5** | 测试补充包: B1-1 跨会话 steer;B1-3 删除清理;B1-2 权限终态;R8-③ tee 契约钉(可选);长回复(>8K token)流式主线程长任务采样 | S×5 |

### 搁置(明确触发器,非「等数据」)
- **队列/steer 持久化**(P2-9): D7 已裁定不持久化;若 B3-1 落地(积压可见),随下次季度复审一并再议 crash-recovery。
- **biased cancel 吞 Completed 的时序竞态**(P2-5 的另一半): defensive break 落地后,剩余的 Completed-vs-Cancelled 同刻竞态接受现状(概率极低,有 L0 日志可对账)。

---

## 五、R8 裁定推荐(决策点 1-5,含推理与翻案条件;待最终拍板)

### 决策 1 · R8-① 修复深度 —— **推荐: 止血 B2-1 立即做,正解 B2-2 下一迭代必做(不自动滑)**

- **为什么两步都做**: B2-1 只覆盖发送时点的正确性(主导流程: 单窗口多会话),S-M 一天内可落;但它有三个无法根治的残留——① turn 内漂移仍在(进行中 turn 的 bash 每次 spawn 读实时 CWD);② `set_current_dir` 到引擎 producer 读 CWD 之间是竞态窗口,且 set 的瞬间会把**另一个正在流式的会话**的 bash 一并带偏(进程 CWD 是全局单例,架构性无解);③ 目录已删时 `set_current_dir` 失败被吞,需补 warn。只有 B2-2(`working_directory` 全链路)把 CWD 从进程单例变成引擎实例属性,才同时消除 ①②。
- **为什么不直接并成一个 PR**: B2-2 动 shannon-core 三个读点 + 六个引擎构建点 + 须回归 REPL/server 的 fallback 行为,blast radius 大;拆两步是风险控制,不是重复劳动。
- **翻案条件**: 若某迭代 shannon-core 有整段容量,可合并为一个 PR 序列(先正解后删止血代码);若产品裁定多窗口/后台会话为非目标,B2-2 可降级为「接受现状 + 文档化」,但需明示。

### 决策 2 · P1-2 后台队列 —— **推荐: 侧栏「队列 N」徽标(本轮落),auto-drain 不做、挂触发器**

- **auto-drain 被低估的三个成本**: ① drain 编排活在 Chat 页 effect 里,与 steer settle 插队(`hasPendingSteer` 门)、`drainBlockedRef` 熔断、A-22 历史记录精细咬合——搬到 AppContext 等于重写全应用打磨最久的状态机;② 行为变化是预算性的: 用户切走后队列逐条烧 token,无在场确认,与 budget guard 的产品语义冲突;③ 失败路径没有落点——被拒消息现在写回 composer,后台会话没有 composer,需要新的通知/回执面,牵出 notification 通道(R1 backlog)。
- **徽标为什么够**: rail 已有 running 圆点/elapsed/goal 徽标三套同构先例,加 chip 是投影 per-session promptQueues 的小改;它不改变「不 drain」的事实,但把静默滞留变成可见滞留——信任问题的根源是「不知道」,不是「没自动发」。
- **翻案条件**: 用户反馈「希望后台继续发」或竞品对齐需要 → 届时按**预算感知 auto-drain** 立项: 仅 drain 明确入队的、每条过 backend pre-turn guard、失败进 inbox/通知而非 composer。

### 决策 3 · P0-3 通道处置 —— **推荐: 删除**

- **bounded+drop-oldest 但无消费者 = 积到上限就丢**,纯浪费,唯一效果是把无界变有界;接消费者则必须先回答「谁消费、消费什么」——唯一候选 P2-5b SessionsPanel spike 未挂载、其自列未做项里就含消费者与背压、且 chat.v2 决策(2026-09)已裁走自研路线。为休眠代码在主路径保留每事件 clone 的通道是本末倒置。
- **附带收益**: route_event 的每事件 payload clone 消失;配合 B1-3 的 registry remove,删除会话零泄漏。
- **翻案条件**: 仅当 P2-5b(SessionsPanel)确认复活排期——届时在复活分支里按原设计重建(有界+消费者一起建),而不是现在预支。

### 决策 4 · P2-4 远程图片 —— **推荐: 默认拦截 + 按图确认 + 全局开关(默认关)**

- **威胁是真实的**: 模型输出的 markdown 渲染在 webview,`![](https://…?d=<上下文片段>)` 即可外洩会话内容/探活 IP——与团队已裁决的导出沙箱(R1)、MCP 审批语义同族;在「local-first + keychain + 0600」的产品人设下维持现状是双标。
- **为什么不采用 schema 级删 http(s)**: 那会连「在浏览器打开」都伤及。渲染时门控(LocalImage 组件)保留原始 href,P0-A interceptor 路径不受影响;单图确认是明确授权,占位显示域名+尺寸避免盲点。
- **翻案条件**: 若产品定位变更(如内置浏览器渲染成为一等公民),改为「会话级允许 + 域名白名单」而非全局默认放行。

### 决策 5 · 滑期授权 —— **推荐批准,附两处对抗性自查修订**

| 范围 | 滑期政策 |
|---|---|
| 批次 1(B1-1/2/3/5/6) | **不可滑**(数据丢失/费用/状态正确性) |
| B1-4(终态兜底) | 批次内唯一可滑项: 排期紧时滑至批次 2 开头(健壮性修复,触发概率最低) |
| B2-1(止血) | **不可滑** |
| B2-2(正解) | 下一迭代**必做**;不自动滑,翻案需明示(见决策 1) |
| B2-3 / 批次 3 | 可滑一季度;B3-2(流式 Markdown 性能)为批次 3 内容量优先项 |

- **修订 1**: B1-4 的滑期例外(理由见批次 1 表下注)。
- **修订 2**: 所有滑期动作沿用 R7-⑤ 前置(fetch + 重叠文件域检查);B2-2 与当前 mobile/relay 主线(#263-#266)文件域不相交,可并行不互斥。

---

## 六、变更记录

- v1(2026-10-04): 初稿。三个待确认项调查结论(R8-①②③)写入 §一;P1-9 关闭。
- v2(2026-10-04): R8-①②③ 经用户确认定案;§五改写为决策点 1-5 的推荐裁定(含推理、对立方案与翻案条件);B2-3 定为删除;B3-1 定为徽标方案;B3-4 明确渲染时门控形态;批次 1 增补 B1-4 滑期例外;滑期矩阵更新。
