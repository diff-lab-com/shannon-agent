# Shannon 可插拔架构评审：Cordis 化是否值得、生态兼容策略与推荐方案

- 日期：2026-08-27
- 决策问题：① Shannon 是否应改为 Cordis 式「一切皆插件」架构？② 是否兼容 dsh-plugin 生态？③ 是否兼容 TypeScript/Go/C 插件生态？
- 输入：[DSH 调研](deepseek-harness-analysis.md)（Cordis 细节见其 §2）、[Pi 调研](pi-agent-analysis.md)（§5 扩展框架）、[差距分析](shannon-gap-analysis.md)（P4/P5/P7）
- 结论速览：**不建议整体 Cordis 化；建议渐进式「接缝化 + 统一扩展模型」；dsh 生态选择性桥接（只做工具型）；多语言插件以 MCP 为主轴、WASM 为二期选项。**

## 1. 问题界定

「Cordis 可插拔架构」在 dsh 中的确切含义：插件=运行时可挂载的服务对象，共享 ctx 服务仓库、inject 依赖激活、类型化事件（emit/waterfall/parallel/serial）、可逆效应、fiber 生命周期、bundle/profile 层序组合 + patch。其成立依赖一个**同语言、可运行时加载代码、可热替换**的宿主（Node/TS）。

Shannon 的语境差异：宿主是 Rust 静态编译二进制，跨三产品形态（CLI/desktop/gateway）+ 移动端，带完整权限体系。要回答的不是「Cordis 好不好」（它在 dsh 里已被验证），而是「Shannon 需要 Cordis 解决的问题到什么程度，以及 Rust 语境下的等价物是什么」。

## 2. 必要性分析：Shannon 缺的是什么

按差距分析 P4/P5/P7，Shannon 的真实缺口按层次排列：

| 层 | 缺口 | Cordis 是否解药 |
|---|---|---|
| 进程内扩展点语义 | 五套机制（MCP/plugin manifest/hooks/skills+agents+profiles/commands）无统一生命周期与权限语义 | 部分是——需要的是统一注册模型 + 可逆性，不是「一切皆插件」 |
| 能力可替换性 | fs/子进程/沙箱/子 agent 无接缝，无法整体迁移执行环境 | 是——capability seam 是正解，且 Rust trait 天然适配 |
| 第三方生态 | plugin manifest 已有（含 .claude-plugin 兼容）但无分发约定与目录 | 不是——生态靠约定+分发+文档，不靠运行时形态 |
| 运行时动态加载/热更新 | 无 | 否——Shannon 无此刚性需求（desktop 可重启进程；CLI 生命周期短）|

结论：Shannon 需要 Cordis 的**三个思想**（服务注册、可逆效应、类型化事件），不需要它的**一个形态**（运行时代码热加载）。后者正是 dsh 社区反馈中概念上手成本、purity gate、供应链风险的来源。

## 3. 可行性分析：Rust 语境的技术边界

| Cordis 依赖的机制 | JS/TS 中 | Rust 等价物 | 代价/备注 |
|---|---|---|---|
| 运行时加载插件代码 | require/import 动态包 | dylib（cdylib+ABI）｜wasmtime（Wasm 组件模型）｜进程外（MCP/JSON-RPC） | dylib：Rust 无稳定 ABI，编译器/版本耦合，跨平台符号地狱——**不建议** |
| 热重载/服务替换 | fiber UNLOADING→LOADING | 无（进程重启）| Shannon 不需要；desktop 端可做「扩展进程」级热替换 |
| declaration merging 扩展事件词汇 | TS 语言特性 | Rust enum 不开放扩展；需 reg-ish 设计（事件 =强类型核心 + 开放 payload 区） | 可用「封闭核心事件 + 自定义 payload 通道」折衷 |
| 可逆效应 | ctx.effect() 返回清理函数 | **RAII guard / Drop 天然适配**（ScopeHandle 持有注册项，Drop 时注销）| Rust 反而更安全（编译器保证） |
| 依赖驱动激活（inject） | fiber PENDING | 构造期依赖解析 / once_cell 初始化图 | 可用启动图拓扑排序实现，复杂度可控 |
| 组合配置（bundle/profile/patch） | package.json + yml | TOML 分层合并（Shannon 已有雏形） | 直接可实现 |

判定：**「Cordis 化」在 Rust 中不可行也不必要；「接缝化 + 统一扩展模型」可行且性价比高。**

## 4. dsh-plugin 生态兼容性评审

dsh 插件的本质：npm 包，含 Cordis 配置行 + 挂载代码，运行在 dsh 的 Node 宿主内，可调用全部 ctx.* 服务与 waterfall 事件。

按插件形态分类评估兼容性：

| dsh 插件形态 | 举例 | 直兼容？ | 桥接可行性 |
|---|---|---|---|
| 声明型 | dsh-rules（规则/提示词注入类） | 否 | **高**——等价于 Shannon 的 skills/profiles，写转换器即可 |
| 工具型（包装一个 MCP server 或本地 CLI） | 语言/格式工具类 | 否 | **中高**——若插件本质是 spawn 一个 MCP server，Shannon 可读其 manifest 直接以 MCP 方式拉起，绕过 Cordis 宿主 |
| 行为型（依赖 ctx 事件/waterfall/inject） | 守卫、遥测、会话投影扩展 | 否 | **低**——语义宿主缺失，等于在 Rust 里重实现一个 Cordis 运行时（含 JS 引擎）才可能跑 |

三条可能路径：

1. **sidecar Node 宿主**：随 Shannon 分发一个 node shim 进程实现 Cordis ctx 最小面，dsh 插件跑在 shim 里经 IPC 暴露为工具/事件。问题：等于把 dsh 的整个运行时拖进 Shannon 的信任边界——供应链面积 + 权限模型冲突（Shannon 的权限体系管不住 shim 内的 JS），与「Rust 安全性」卖点自相矛盾。**不推荐。**
2. **manifest 级选择性桥接**：只认领声明型与「MCP server 包装型」插件——读其 package.json/dsh 字段，映射进 Shannon 的 manifest v2。成本低、风险可控、无运行时依赖。**推荐。**
3. **生态索引互通**：不做运行时兼容，只做发现互通（awesome/目录站互相收录 + 安装命令转换提示）。**推荐（配合 2）。**

判定：**「兼容 dsh 生态」的正确姿势是选择性吸收格式与工具型桥接，不是运行时兼容。** dsh 生态尚在 RC 早期、API 未稳定（0.1.0 未发），现在深度绑定性价比为负。

## 5. TypeScript / Go / C 插件生态兼容

四种机制对比（服务于「让任意语言作者能给 Shannon 写扩展」）：

| 机制 | 语言支持 | 进程模型 | 权限可控 | 延迟 | 现状 | 判定 |
|---|---|---|---|---|---|---|
| **MCP（stdio/SSE JSON-RPC）** | 任意 | 进程外 | 好（OS 级 + Shannon 审批） | 中（IPC） | **已实现**（shannon-mcp，工具自动发现） | **主轴，P0 强化** |
| 声明式 manifest（command/skill/agent/profile） | 无代码 | 进程内解释 | 好 | 零 | **已实现**（plugin.toml + .claude-plugin/plugin.json） | 继续扩展词汇 |
| **WASM 组件模型（wasmtime + WIT）** | TS/Go/C/Rust 均可编译目标 | 进程内沙箱 | 最好（能力制授权） | 低 | 未实现 | **二期选项（P2）**，适合高 QPS 小工具 |
| 原生 dylib | 受 ABI 限制 | 进程内 | 差 | 最低 | 未实现 | **不推荐**（ABI 地狱 + 安全倒退） |

要点：Shannon 已经站在正确的机制上——**MCP 就是跨语言插件生态的事实标准**（DSH/Pi/Claude Code 全都支持）。TS 作者写 MCP server（官方 SDK）或声明式 skill；Go/C 作者写 MCP（SDK 均有）或编译 WASM。Shannon 要补的不是「兼容某语言的插件 ABI」，而是把 MCP 体验做到位：能力协商、权限声明进 manifest、进程健康度、deferred schema（已有）、以及**事件级扩展**（目前 MCP 只有 tools，没有 hooks——Shannon 可定义 MCP 侧的 hook 订阅协议，让进程外插件也能监听生命周期事件，这是超越 Claude Code 兼容面的机会点）。

## 6. 推荐改造方案：Shannon 统一扩展模型（SEM）

### 6.1 设计原则

1. **接缝优先于插件**：先把能力 trait 化、可替换，再开放给第三方。
2. **进程外默认、进程内白名单**：第三方代码永远在 MCP 进程或未来 WASM 沙箱里；进程内扩展位只留给编译期已知的内置模块。
3. **可逆性用 Rust 语义**：注册返回 guard，Drop 即注销——比 JS 清理函数更强。
4. **事件词汇封闭核心 + 开放 payload**：核心 TraceEvent/QueryEvent enum 不开放外部变体；自定义事件走带命名空间的 payload 通道（避免 DSH 的 kind/key 不一致炸投影事故）。
5. **不变量先行**（来自 DSH 的教训）：seq 连续、请求可从日志重建、未知事件缺省拒绝——先立测试再开扩展点。

### 6.2 核心抽象（示意）

~~~
// 进程内接缝（编译期注册，无动态加载）
pub trait Service: Send + Sync { fn key(&self) -> ServiceKey; }

pub struct ServiceRegistry {
    services: DashMap<ServiceKey, Arc<dyn Service>>,
    // scope: 注册返回 guard，Drop 自动注销（可逆效应的 Rust 形态）
}
pub struct RegistrationGuard { /* Drop 时从 registry 注销并逆序执行清理 */ }

pub struct PluginContext<'a> {
    pub fn register_tool(&self, def: ToolDefinition) -> RegistrationGuard;
    pub fn on(&self, ev: CoreEvent, h: EventHandler) -> RegistrationGuard;
    pub fn provide<S: Service>(&self, svc: S) -> RegistrationGuard;
    pub fn effect(&self, cleanup: CleanupFn) -> RegistrationGuard;
}

// 能力接缝（先做这三个，收益最大）
pub trait FileSystemProvider { /* read/write/edit/list/watch */
pub trait ProcessProvider   { /* spawn/pty，含 argv 沙箱包装钩子 */
pub trait SandboxProvider  { /* 策略 + local/landlock/远程 后端 */

// 事件协调语义（对齐 DSH 四模式）
pub enum Dispatch { Emit, Waterfall, Parallel, Serial }
~~~

统一事件总线以现有 QueryEvent 为核心扩容（合并 hooks 30 事件的进程内分发位 + 权限决策点），waterfall 通道用于 tools/pre-execute 守卫与请求改写——现有 PermissionRuleChecker 与 PreToolUse hook 迁入该通道成为内置「插件」。

### 6.3 组合层（profile 演进）

- profiles.toml 增加层序语义：base（内置）→ 项目 .shannon/ → 用户全局 → CLI overlay；提供 shannon --dump-config 打印实际生效树（对齐 dsh --dump-config 的调试体验）。
- plugin manifest v2：向后兼容 plugin.toml 与 .claude-plugin/plugin.json；新增字段：mcp 引用、hook 订阅声明、权限声明（映射到既有 PluginPermission 并**接入执行点强制**，闭合差距 P7）。

### 6.4 生态层

- 约定 GitHub topic：shannon-plugin；提供 awesome 模板与「dsh/claude 插件 → Shannon 映射」转换器（§4 路径 2）。
- MCP 增强：hook 订阅协议（进程外插件监听生命周期）、manifest 内权限声明、server 健康度与重启策略。

### 6.5 阶段路线图

| 阶段 | 内容 | 规模估计 | 验收 |
|---|---|---|---|
| 0 | 事件日志 + 请求信封快照（依赖 trace 方案 P0）| 2–3 周 | 「模型可见即已记录」测试通过 |
| 1 | 统一事件总线（waterfall 守卫通道）+ 权限/hook 迁入 | 2–3 周 | PreToolUse 权限判定走单一管道；hook 与进程内监听同一分发器 |
| 2 | manifest v2 + 权限强制 + dump-config + 生态约定 | 1–2 周 | 第三方 skill/command/tool 三类插件端到端装跑；越权被拒 |
| 3 | FileSystem/Process/Sandbox 接缝化（工具层改造）| 3–5 周 | 换 sandbox provider，bash/edit/lsp 无需改代码即可进沙箱 |
| 4（可选）| WASM 组件模型试点（wasmtime + WIT 定义工具接口）| 4–6 周 | 一个 TS 编译的 WASM 工具以受限权限运行 |

### 6.6 明确不做（Non-Goals）

- 不做运行时 TS/JS 代码加载（不内嵌 JS 引擎）。
- 不做 dylib 插件 ABI。
- 不做 dsh Cordis 宿主 shim（§4 路径 1）。
- 不承诺 dsh 行为型插件兼容。
- 不拆 221 包式微包架构；crate 拆分按域渐进。

## 7. 风险与对策

| 风险 | 对策 |
|---|---|
| trait 接缝抽象错误，二次返工 | 先只做 fs/process/sandbox 三个高确定接缝；每个接缝以「现有两种实现可互换」为设计验收（如 local 与现有 sandbox.rs） |
| 事件总线重构影响面大 | 以 wrapper 起步：旧 QueryEvent 通道保持，总线做旁路聚合，逐步切换 |
| 权限强制破坏现有易用性 | 权限声明缺省 = 现状（宽松），显式声明才收紧；提供 migration 提示 |
| 生态没人来 | 先服务自己：内置功能改为经扩展位装配（hooks/routines/MCP 已是）；生态是副产品不是目标 |
| 精力挤占 trace/评测 P0 | 路线图把阶段 1–2 排在 trace 之后；本方案阶段 0 即 trace 方案 P0 |

## 8. 对照总结

| 维度 | DSH（Cordis） | Pi（扩展框架） | Shannon 推荐（SEM） |
|---|---|---|---|
| 插件形态 | 一切皆插件（含 agent loop） | TS 工厂函数扩展 | 编译期接缝 + 进程外插件 |
| 生命周期 | fiber 六态 + HMR | 加载即生效 + 失效契约 | 注册 guard（Drop 可逆）+ 进程重启 |
| 事件 | 四模式 + 开放词汇 | 六域生命周期事件 | 四模式 + 封闭核心词汇 + 命名空间 payload |
| 多语言 | 仅 TS（Python 经 ACP 子代理） | 仅 TS | MCP 任意语言 + WASM 二期 |
| 安全 | 沙箱 seam + Landlock | 无权限系统 | 既有权限体系 + 接缝强制 + MCP 边界 |
| 组合 | bundle/profile/patch | 目录发现 | profile 层序 + dump-config |
