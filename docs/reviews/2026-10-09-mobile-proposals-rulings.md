# mobile 两份跨仓提案的 mono 侧裁决（2026-10-09）

> 对象：shannon-mobile `docs/tool-result-cards-proposal-2026-10.md`（先做，小）与
> `docs/mono-b6-group-proposal-2026-10.md`（后做，中）。提案事实勘察基线 `4be17b1e`；
> 本裁决与落地基线 `e67596a4c`（origin/dev，#357 缓期批 2 合入后）——两批均不触
> 缓期批 2 改动面（desktop UI / 导出 / 徽章），勘察结论全部复核仍成立。
> 落地遵循 B 批模式：wire 一律 additive、可选键缺省即不支持；mono PR 合入时把
> 定稿契约回写 mobile 仓 `docs/cross-repo-adaptation-spec.md`（新 §R / §S）。

---

## 一、tool-result-cards 提案

### §3.0 两个前置裁决（提案自荐方案，均**核准**）

1. **capabilities 广播不引入** —— 核准。真实 gateway 无广播面（§Q3 已裁决
   `shannon/capabilities` 为 M0 mock 专属）；additive 事件字段无方法级缺席信号也不需要：
   「数据到了就渲染、没到就维持纯文本」的天然降级即诚实形态。历史 artifacts 面
   （§R-3）沿 §P/§Q 的 METHOD_NOT_FOUND 先例，v1 不做方法级探测。
2. **方案 (a) 既有载体加可选键，不建独立 artifact 事件流** —— 核准。提案四条理由
   全部成立，补一条最强佐证：**引擎 WS 网关拒绝一切非 UUID session_id 的 query 帧**
   （`api_server.rs` `Uuid::parse_str`），独立 `shannon/session.artifact` 事件流若沿用
   会话语义还得先解决键位；而 `task.progress.tool` 已在 wire 上、`transcript` 投影
   已有挂靠点，(a) 是纯投影恢复。

### 提问清单五项裁决

| # | 问题 | 裁决 |
| --- | --- | --- |
| 1 | §K3 Ruling 修订（任务流转发 tool 帧） | **接受**。`taskTurnHandler` default 分支拆出 `tool_use`/`tool_result` 两臂，经 `hub.pushTaskToolFrame`（镜像 `pushTaskDelta` 的 frontRunningTask 归因）推 `task.progress {session_id, tool}`。原 Ruling（2026-10-02）是「无消费方时代的一刀切」，修订记录随 §K3 回写：**§K3 修订（2026-10-09）——任务流增发 tool 帧，全部可选键，旧 gateway 不发、旧手机忽略；`task.message`/`query.failed` 终态语义不变**。 |
| 2 | 引擎 WS 字段恢复落在哪个批次 | **本批**（`feat/tool-result-wire`）。`shannon-api-protocol` codegen 门由本批独占；与 #354 同域敏感点已核对——本次只给 struct-like 变体加 `Option` 字段（schemars 生成路径无混合变体枚举 drift 风险），再生后全量 diff 审查。 |
| 3 | 历史折叠空文本边 | **接受空气泡条目** `{role:"assistant", content:"", artifacts:[…]}`。产物是内容非瞬态、必须进 replay 环（§O4 语义）——丢弃会造成「在线有卡、重进无卡」的自相矛盾；空气泡是旧 peer 的升级自愈外观噪音。仅在该 turn 确有 artifacts 时才发空宿主条目。 |
| 4 | `output` 截断口径 | **wire 无截断上限**（现状即全量：直发路径 `tool_result.output` 今天就不截断；L0 全量保留不动）。mobile 卡体自限：默认折叠 + 「展开」+ 长按全量复制，渲染上限手机自定。mono 不给权威数——给了就会被当成 wire 保证。 |
| 5 | meta 结构冻结时点 | **v1 透传不解释；mono 侧无已规划的 meta 结构提案**。现存唯一稳定生产者是 sandbox classification（§4.12 `{"classification":…}`）。文件变更卡（`files_changed`）/GitHub 事件卡等稳定结构出现后再立项，两仓以 spec §R 增补为准，不各猜一套。 |

### 对提案草案的三处事实修正（落地时按此执行，回写 §R 时同步勘误）

1. **WS live 面 v1 无 `duration_ms`**：`QueryEvent::ToolUseResult` 本身不带该字段
   （只有 L0 `ToolResultPayload` 有）；发射点散布 agent_loop 6+ 处，为 live 卡合成
   计时属于造假。裁决：live WS 帧只恢复 `tool_use_id`/`is_error`/`meta`/`ts`（`ts` 为
   WS 转发时刻 stamp，沿 §L1 approval `ts` 先例）；**时长只出现在历史 artifacts**
   （投影读 L0 真值）。降级矩阵本就按缺键隐藏，无破坏。
2. **`title` 恒省**：引擎不对开放集工具做 per-tool 参数推导（封闭逻辑违 §Q1 口径）。
   历史卡标题 mobile 回落 tool 名；live 卡可从 `input` 推导（mobile 侧既有蓝图）。
3. **`summary` 恒省**：手机持有全量 `body`，两行摘要自切，不需要 wire 键。

---

## 二、mono-b6-group 提案（B6.0 起步同批裁决）

### 开放问题三项裁决

1. **审批 TTL 覆盖（B6.0-4 硬前置）**：裁决**引擎侧 per-request TTL** 路线——
   `shannon-api-protocol` 的 `query` 客户端帧增 optional `approval_ttl_ms`
   （`#[serde(default)]`，缺省/旧 gateway = 300s 不变），引擎 resolver 的
   `tokio::time::timeout` 按其放宽（上限钳制 24h）；gateway 审批注册表对带
   `quoteWindow` 的请求对齐 retention 并到期**主动 deny + `group.system(quote-expired)`**。
   **否决**「gateway 单侧延长、引擎 300s 先到导致窗口实际缩水」的降级路线——
   对用户展示 30 分钟倒计时却在 5 分钟后静默失效是说谎，宁可推迟。
   实施时点随 B6.0-4（`quoteWindow` 出现真实生产者时），B6.0 起步不含。
2. **群转录存储落点**：裁决 **gateway 自管群注册表与转录**
   （`~/.shannon/groups/<groupId>/group.json` + `transcript.jsonl`，宿主内部文件、
   不入 wire、不动 `usage.jsonl`）。理由：2026-10-03「L0 单点」裁决针对**引擎会话**
   的数据面（不经 gateway 直读会话目录）；群是宿主编排实体，成员 turn 的引擎会话
   是一次性 UUID、群转录在编排层才有语义——入 SessionStore 需引擎新增会话类型，
   工作量上浮且 wire 无收益。`session.history {sessionId: grp-*}` 由 gateway 截获
   （先查群注册表、命中由群转录应答 §J2 形状、未命中回落引擎），手机路由链零改动。
   `session.list` 不含群键——群经 `group.list` 独立投影（双源，02 侧已按此设计）。
3. **阵容生成**：确认无冲突——规划查询是 gateway 自调用的引擎 query 面（无工具轮、
   无审批面、不经手机会话）。v1 起步先落**通用三 slot 模板**（诚实降级路径先行），
   规划查询作为增量为后续批次；`group.create` 契约不变（`members` 缺省 = 编排器规划，
   v1 起步规划即模板，响应如实返回模板成员，无假成功问题）。

### B6.0 起步范围确认（本批实施）

- **含**：B6.0-1（群实体/`group.list`/转录回放截获）、B6.0-2（`group.create` 双路径校验
  + `group.message` + 成员 turn v1 = §K 管线 + B0 归属）、B6.0-3（`group.handoff` /
  `group.member` / `group.system` 三事件、v1 编排器确定性交接链）、B6.0-5
  （`group.archive`）。
- **审批 group 键最小面随批**：成员 turn 内的 `approval.request` 增 optional
  `group {groupId, member, ruleTrigger?}`（归因 + `handoff-first` 一次性标志），
  经既有 `hub.requestApproval`/`settleApproval` 往返，decide 签名管道零改动；
  `approval.list`/`snapshot` 条目同键透传。`ruleTrigger` v1 只有 `handoff-first` 可
  确定性产出——`payments-ask-first` 无支付类工具分类器（引擎 kind 是开放集），
  `over-pool`/`over-share` 属 B6.1，均缺席不造。
- **不含（随后续）**：B6.0-4 拍板卡变体全量（`quoteWindow`/TTL/`payments-ask-first`，
  TTL 按上文裁决 1）、B6.1 池记账与台账事件、B6.2 日报与复盘、规划查询、桌面群管理面、
  成员菜单、R14 深链、`group.create` 签名、成员间自主移交。

### 起步实现的两点补充契约（回写 §S 时一并钉定）

1. **群事件的推送扇出**：`group.*` 事件与成员 turn 的 `task.progress`/`task.message`
   **广播到全部已连接设备**（群是宿主级实体，非设备私有——与 §K3 任务「仅推发起设备」
   有意不同）；seq/replay 沿既有 hub 面语义。
2. **成员 turn 的引擎会话**：引擎 WS 门拒绝非 UUID session_id——成员 turn 引擎会话 =
   每轮新生 UUID（用后即弃，不进 `session.list` 语义），wire 事件由编排器统一重键为
   `session_id = groupId`。
