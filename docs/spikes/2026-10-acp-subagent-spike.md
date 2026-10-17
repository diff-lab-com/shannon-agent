# Spike 报告:ACP 子代理(真 Claude Code 经 ACP 编排)——可行性、工作量、风险与 R2 设计输入

> **任务**:Task 4(PR-3/D-1),定义见 `docs/plans/2026-10-17-followup-implementation-tasks.md` §PR-3,
> 意见源 `docs/reviews/2026-10-15-deepseek-harness-modularity-analysis.md` P1-3(F8 降格为"定位级假设"后,由本 spike 回填重估)。
> **日期**:2026-10-17 · **分支**:`feat/acp-spike` · **原型**:`spikes/acp-client/`(独立 crate,不入 workspace、不接 CI,默认不合并)
> **Timebox**:3 个工作日硬限。实际耗时远低于 timebox,**未触发止损**。

---

## ① 可行性结论:**有条件可行**

**一句话依据**:真 Claude Code 经 Zed 系桥接适配器 `@agentclientprotocol/claude-agent-acp@0.93.0` 作为 ACP 子代理,在本沙箱内**两次端到端跑通**(spawn → `initialize` → `session/new` → `session/prompt` 流式回传 → `stopReason=end_turn`),且凭据隔离实测通过(父进程植入伪造凭据,子进程 `/proc/<pid>/environ` 实测零泄漏);**条件**是:Shannon 必须引入并跟随一个 Node 侧适配器进程 + Claude Code 自持登录,协议本身(换行分隔 JSON-RPC,三个方法)不是瓶颈,生产化的难点在生命周期、审批面与五方版本耦合(见 ②③)。

**实测证据**(均为本机复现命令+日志,可复现步骤见 `spikes/acp-client/README.md`):

| 步骤 | 证据 |
|---|---|
| 握手 | `spikes/acp-client/evidence/e2e-run-trimmed.jsonl` 首行:返回 `protocolVersion:1`、`agentInfo:{name:"@agentclientprotocol/claude-agent-acp",version:"0.93.0"}`、`authMethods:[]` |
| 会话创建 | 同文件 `session/new` 响应:`sessionId:"0dbd3553-71ae-40ca-88dc-36af03488fd1"`(适配器 stderr 显示 session/create 全程 **513ms**,`evidence/e2e-run-trimmed.jsonl:9-14`:`totalMs` 依次 511→512→512→512→513,register 阶段收尾) |
| 结构化提示 → 流式结果 | `session/prompt` 后逐 token 收到 `session/update{update:{sessionUpdate:"agent_message_chunk",content:{type:"text",...}}}`,聚合文本 `{"ok": true, "answer": 4}`,响应 `{"stopReason":"end_turn", usage:{inputTokens:26040, outputTokens:164, ...}}` |
| 复现(热) | run-2(7.8s 墙钟,含进程冷启动之外的全程):`{"ok": true, "answer": 42}`,`end_turn`,日志 `evidence/e2e-run-2-trimmed.jsonl` |
| 凭据隔离 | `--poison` 在父进程植入 `SHANNON_LLM_API_KEY`/`ANTHROPIC_API_KEY`/`ANTHROPIC_AUTH_TOKEN`/`OPENAI_API_KEY`;`evidence/child-env{,-2}.txt` 记录子进程实际 environ(仅 PATH/HOME 等 7 项),末行 `ISOLATION CHECK: PASS` |

**关于本沙箱无官方 Anthropic 登录的说明**:本机 Claude Code 的鉴权走 `~/.claude/settings.json` 的 `env.ANTHROPIC_AUTH_TOKEN`+`ANTHROPIC_BASE_URL`(代理端点,usage `_meta` 显示实际模型映射为 `glm-5.3-flash[1m]`)。这**恰好构成 provider 自持凭据的实证**:子代理链路的凭据完全来自 Claude Code 自己的配置存储,Shannon 侧凭据链零参与、零透传,委派照样成功。真实机器上的验证步骤与此处完全一致(前提仅是那台机器的 `claude` CLI 已用自己的账号/API key 登录)。

---

## ② 工作量重估(对照 P1-3"改造代价最小"论断)

**原论断不成立,需改写为:"骨架代价最小,生产代价中等"**——协议骨架的便宜是真实的,但它只占生产化工作量的零头。

**实测骨架成本(证据:本仓库 `spikes/acp-client/`,单人)**:
- ACP v1 客户端骨架(spawn/initialize/session-new/session-prompt/流式聚合/env 隔离/全量协议日志)≈ **310 行 Rust**(`spikes/acp-client/src/main.rs` 实测 310 行;tokio+serde,无特殊依赖),**半天**内完成并两次跑通。协议是换行分隔的 JSON-RPC 2.0(`\n` 分帧、UTF-8、无 Content-Length 头),学习成本近乎为零。
- 另有一条省力路径未走(留作 R2 参考):Zed 官方 Rust crate `agent-client-protocol@3.4.0`(crates.io,2026-10-16 更新,活跃),类型齐全,连适配器同款 TS SDK(`@agentclientprotocol/sdk@1.9.0`)也存在。

**骨架 → 生产还差什么**(每项都来自实测/适配器源码证据,非猜测):

| # | 缺口 | 实测/证据 | 量级 |
|---|---|---|---|
| 1 | **审批/权限面**:`session/request_permission` 是 ACP client 的基线义务。适配器建会话默认 `currentModeId:"default"(Manual)`——子代理每次动工具都会来问 client | 本次实测只发纯文本提示故未触发;`session/new` 响应含五档 mode(default/acceptEdits/plan/auto/bypassPermissions) | **最大隐藏项**:策略引擎+人工审批 UI 或自动策略,1~2 人周 |
| 2 | **会话生命周期**:ACP v1 的会话级方法(`session/delete`/`session/close` 等)均为**可选能力**(本机 initialize 实测宣布 close/delete/fork/list/resume),且只覆盖会话层——**连接层(进程)没有 shutdown 方法**:守护、kill、重启、`session/load` 恢复、stdio 半关闭与僵尸回收都是 client 自己的事 | 本次原型用 `kill_on_drop` 粗暴收尾;适配器 README 详述 cancel/后台子代理/`perTaskStopAffordance` 等复杂语义 | 3~5 天 |
| 3 | **取消语义**:`session/cancel` 通知 → agent 须尽快停并回 `stopReason:"cancelled"`;嵌套子代理的取消边界(前台停/后台跑)由适配器 `_meta` 选项控制 | 适配器 README"Subagent sessions"整节;未实测(超出 spike 范围) | 2~3 天 |
| 4 | **错误恢复**:JSON-RPC error 分型、auth 失效重连、适配器崩溃重启;本机实测到两条真实错误路径(`cwd` 非绝对路径 `-32602`;占位 sessionId `-32603 "Session not found"`),说明错误是结构化可恢复的 | `evidence/error-samples.jsonl`(2026-10-17 专测重跑,两条错误响应逐字在档) | 2~3 天 |
| 5 | **凭据轮换/健康检查**:适配器靠私有通知 `_auth/status_update` 广播登录态(`kind:"none"`);轮换=重登录+重连,属 Claude Code 侧运维 | 实测每次 initialize 后收到 `_auth/status_update` | 1~2 天 |
| 6 | **结果形状映射**:流式 chunk 聚合、`tool_call/tool_call_update`/`plan`/`thought` 等 update 变体回填 Shannon 的 `ToolOutput`/事件流 | 原型只聚合了 text chunk | 2~3 天 |

**区间结论**:
- 骨架(已验证):**0.5 天**(实测)。
- 单 provider(Claude Code)生产可用:上述 1-6 全做,**约 2~3 人周**。
- 多 provider 注册表 + 运维化(守护/观测/轮换/CI):**再加 2~4 人周**。Codex 等第二 provider 理论上复用同一 client(ACP 是通用协议),但适配器生态各自独立(`codex acp` 另一套),不承诺复用收益。
- 对照原报告:P1-3 说"改造代价最小"隐含的是"比自研私有协议/CLI 包装便宜"。**这个方向仍然成立**(协议标准化确实免去了为每个 CLI 手写 spawn/解析/流式协议),但"最小"会误导排期——真正的大头不是协议,是**审批面与进程生命周期**,这两样换任何协议都躲不掉。

---

## ③ 协议/版本风险清单

| 风险 | 证据/机制 | 缓解 |
|---|---|---|
| **ACP 版本漂移**:v1 在滚动加扩展(本机 initialize 响应已含 `providers`、`sessionCapabilities.fork/list/resume`、`steering` 等超基线能力);**v2 草案已存在**并含 subagents 基线,适配器靠 `CLAUDE_AGENT_ACP_EXPERIMENTAL_V2=1` 开启 | agentclientprotocol.com 有 `/protocol/v1/` 与 `/protocol/v2/` 两套文档;适配器 README"Experimental ACP v2"节 | 锁 `protocolVersion:1` + 显式能力探测;v2 稳定前不追 |
| **适配器单点耦合**:唯一生产级 Claude Code 桥 = `@agentclientprotocol/claude-agent-acp`(前身 `@zed-industries/claude-code-acp` **已弃用改名**,npm 弃用公告在案);0.93.0 依赖 `@anthropic-ai/claude-agent-sdk@0.3.300`;preview 通道**每 push 一发** | npm view 实测(2026-10-17):deprecated 字段 + time.modified=2026-10-16 | 锁精确版本(本 spike 即锁 0.93.0);升级走人工窗口 |
| **Claude Code 版本漂移**:SDK **自带打包 CLI**(bunfs 提取式分发),并非本机 `claude` 2.1.250——子代理实际行为由 npm 依赖树里的 CLI 版本决定,与系统安装解耦 | `node_modules/@anthropic-ai/claude-agent-sdk/package.json` = 0.3.300;提取机制见该包内 `extractFromBunfs.js` 头注释——"Extracts a file from Bun's \$bunfs virtual filesystem to a real temp directory so it can be spawned as a subprocess"(文件随 npm 安装存在,未随本报告入库) | 版本矩阵进 CI 冒烟(若生产化);不依赖系统 CLI 版本假设 |
| **Rust 侧类型耦合**:Zed 的 `agent-client-protocol` crate 与适配器 TS SDK 是**两套独立版本线**(3.4.0 vs sdk 1.9.0),capability enum 可能滞后于适配器新能力 | crates.io + npm 实测 | 手写类型面(如本原型)或 crate + `_meta` 透传兜底 |
| **stdio 生命周期坑**:① stdout 纯净性——agent 不得向 stdout 写任何非 ACP 消息(CC 历史上有打印行为,靠适配器兜住;本次实测干净);② 换行分帧,消息内禁止换行;③ 连接无 shutdown 方法,只能 kill(见 ②-2);④ stderr 无结构(本次收到 `[authStatus] session account carries no identity signal` 等裸日志) | agentclientprotocol.com/protocol/v1/transports.md + 本次日志 | 进程监控 + 超时看门狗;stderr 仅当诊断流 |
| **参数强约束**:`cwd` 必须绝对路径,传 `.` 即 `-32602` 拒绝 | `evidence/error-samples.jsonl` Case A:`Invalid params: `cwd` must be an absolute path, but received: .` | 调用侧规范化(canonicalize) |
| **认证路径不可协商**:`authMethods:[]` ——适配器不提供 ACP `authenticate` 流,凭据**完全**依赖 CC 侧配置(登录/keychain/settings env);CC 未登录的机器上 session/new 会失败,该路径本次未实测 | initialize 响应实测 `authMethods:[]` + `_auth/status_update{kind:"none"}` | 部署前置检查(CC 登录态探活);作为装配条件而非运行时能力 |
| **模型路由不受 Shannon 控制**:本机 usage `_meta` 显示实际模型是 CC 配置映射的 `glm-5.3-flash[1m]`——Shannon 的 `model` 参数只能经会话 `configOptions`(适配器实测暴露 `id:"model"` 选择器)**请求**,最终路由由 provider 配置决定 | session/new 响应 configOptions + usage `_meta.quota.model_usage` | 文档明确"ACP provider 的 model 参数是建议而非保证" |
| **适配器私有扩展面**:`_meta.claudeCode.*`、`_auth/status_update`、JetBrains AIR 扩展族——用得越深,与该适配器的耦合越紧 | 适配器 README | 只依赖基线 + 少量 _meta;扩展面留 adapter 内部 |

---

## ④ R2 设计输入(可行 → provider 注册表怎么接)

**现状锚点**:`crates/shannon-agents/src/executor.rs:25` 的 `AgentExecutor` trait(`execute(system_prompt, task, model, tools) -> ToolOutput` + `execute_with_history`),现有实现 `LlmAgentExecutor`/`MockAgentExecutor`;`SubAgentRegistry` 目前**单 executor**(`sub_agent.rs:217,240` 的 `set_executor(Arc<dyn AgentExecutor>)`)——R2 要把"单槽"变"按 provider 选择"。

**建议形状**:

```rust
// 1) 注册表:从单槽到按 provider 选择
enum ProviderId { Llm, Acp { adapter: String /* e.g. "claude-code" */ } }
struct ExecutorRegistry { by_provider: HashMap<ProviderId, Arc<dyn AgentExecutor>> }
// SubAgent 定义增加 provider 字段;registry.set_executor → registry.select(provider)

// 2) ACP provider 的接口形状(已被本 spike 验证可落地)
struct AcpAgentExecutor {
    bridge: AcpBridgeConfig {   // 对应适配器/agent 的启动配置
        cmd: String, args: Vec<String>,
        env: EnvAllowlist,      // 就是本原型 scrubbed_env() 的生产化
    },
    // 连接与会话池:进程长驻 + 多会话复用,而非每任务 spawn(实测 spawn→首字节 <1s,但 CLI 冷启动贵)
}
// AgentExecutor 映射(spike 已验证的部分):
//   execute(system_prompt, task, ...) =
//     确保连接(initialize,协议版本锁 1)→ session/new{cwd: 工作区根, mcpServers: []}
//     → session/prompt{prompt:[{type:"text", text: task}]} → 聚合 agent_message_chunk
//     → ToolOutput::text;stopReason != end_turn 映射为错误(结构化 error.data 可直接透传)
```

**三处接口缝隙,R2 排期时必须给答案**(都有 workaround,但都是适配器扩展面而非协议基线):
1. `system_prompt` 参数:ACP `session/new` 基线无 system prompt 位;适配器宣布了 `sessionCapabilities.systemPrompt.mode.override` 扩展能力(本次 initialize 响应实测含该字段),或退化为拼进首条 prompt。
2. `tools` 参数:子代理工具面由 Claude Code 自带工具集决定,Shannon 只能经权限 mode/configOptions 收窄,不能像 LlmAgentExecutor 那样精确点名工具列表。
3. `execute_with_history`:不应映射为拼 history 重放,而应映射为 **ACP 会话续用**(同 sessionId 追问 / `session/load`+replay,适配器宣布 `loadSession:true`)——这是 ACP 相对无状态 LLM 调用的真正增量,也是 cross-model oracle(差距 #6)顺路评估的落点。

**安全面答案的起点**(回应 F8 第一问):凭据隔离已实测可落地(env allowlist + provider 自持凭据,`--poison` 对照法可进生产测试);剩下的审批责任划分=②-1 的权限面:`default(Manual)` 模式下 Shannon 作为 client 收敛审批决策,`auto/bypassPermissions` 模式=把审批权完全让渡给子代理——建议 R2 默认收敛(显式 allowlist 工具 + 其余转人工/拒绝),并把该开关暴露为 agent 定义字段而非全局配置。

**建议的 R2 切入顺序**:ExecutorRegistry 改造(纯重构,低风险)→ AcpAgentExecutor(②-6 结果映射 + ②-2 生命周期,单 provider)→ 权限面策略(②-1)→ 取消/恢复(②-3/4)。

---

## 附:复现与环境

```bash
cd spikes/acp-client && npm install && cargo build
./target/debug/acp-client --cwd "$PWD" \
  --prompt 'Reply with exactly one line of JSON: {"ok": true, "answer": <the answer to 7*6>} and nothing else.' \
  --log logs/my-run.jsonl --record-env logs/my-env.txt --poison
# 预期:流式输出 {"ok": true, "answer": 42};stopReason=end_turn;ISOLATION CHECK: PASS
```

环境锚点(2026-10-17):适配器 `@agentclientprotocol/claude-agent-acp@0.93.0`(SDK 0.3.300)· Rust crate `agent-client-protocol@3.4.0` · 本机 Claude Code CLI 2.1.250(仅作环境参照,SDK 实际用自带 CLI)· node v24.19 · cargo 1.88.0。完整协议日志(`evidence/e2e-run-trimmed.jsonl`,184 行 = 179 条原始消息 + 5 条截断标记行)与隔离证据(`child-env{,-2}.txt`)、错误样本(`error-samples.jsonl`)随原型入库;完整未截断日志不入库。

**止损说明**:未触发。全程无连续死端(唯一的协议级受挫是 `cwd` 相对路径被拒,一次修正即通过),原型一次构建成功。
