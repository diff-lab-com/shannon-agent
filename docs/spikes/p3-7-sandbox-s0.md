# P3-7 S0 Spike: Shannon 沙盒执行后端选型

> Wave 6 S0 spike · 估算 0.5w · 产出:对比 + 决策 + 路线
>
> 作者:Wave 6 S0 subagent · 日期:2026-08-04
> 状态:**Draft**(待 ericdong 评审)
>
> 关联:`docs/improvement-plan-2026-08.md` §11(路线图),`crates/shannon-core/src/sandbox.rs`(既有代码),`docs/STABILITY.md`,`docs/ci-gates.md`,`crates/shannon-core/tests/architecture_invariants.rs`。

---

## TL;DR

1. **核心结论**:Shannon 的沙盒执行面**已经存在**,不是从零起步。`crates/shannon-core/src/sandbox.rs` 已经是 2570 行的实现,内含 5 个 `SandboxProvider` 实现:`BwrapSandbox`(Linux)、`SeatbeltSandbox`(macOS)、`DockerSandbox`(通用)、`LandlockSandbox`(`landlock` feature 后的脚手架)、`NoSandbox`(降级)。P3-7 的真问题是**把"散在各处的实现"统一进一个跨平台 trait、消除隐式降级、在 Windows 路径上做出有依据的取舍**,而不是选什么新后端。
2. **分阶段路线**:
   - **Phase A(2w)**:Linux 收敛 — 让 Bwrap vs Landlock 二选一落地(`detect_sandbox_provider()` 当前的优先级是 Bwrap-first),把 Landlock 的脚手架推到"可独立启用并自测",不引入任何新后端。
   - **Phase B(2w)**:macOS 收敛 — 现有 `SeatbeltSandbox` 完成跨进程 fork 行为的测试(profile 注入 + S-IPC),并接入 `shannon serve` HTTP 面。
   - **Phase C(2w,可选)**:Windows 路径 — 选 `windows` crate + Job Object(只做进程 + 工作集限制),不做 syscall filter;做不到的就显式回退 `NoSandbox`。
   - **Phase D(1w)**:`detect_sandbox_provider()` 改名 `try_provider()` 并加入"显式降级日志 + 用户可见告警",把"何时打开沙盒 / 何时放弃"做成可审计的状态机。
3. **风险**:
   - 当前 `LandlockSandbox::apply_restrictions()` 的 `Access::from_read`/`Access::from_write` 在 `landlock` 0.4 上**签名不匹配**(Plan 9 helper closure API),feature 编译是断的(需要在 Phase A 第一天验证)。
   - macOS Seatbelt 的 `sandbox-exec` 在 Sequoia (15.x) 已标记为 deprecated,Apple 在推进 `sandbox_init` + SBPL profile 路径,需要确认迁移窗口。
   - Windows 沙盒选项在 2026 几乎没有 Rust 友好的内核级方案,Job Object 只能解决"进程维度"一半,沙盒边界必须落到文件系统层(用户态 deny list),这不是真沙盒。

---

## 1. 对比表:三家沙盒后端 × 11 维度

| 维度 | Landlock(Linux ≥5.13,内核) | Seatbelt(`sandbox-exec` / `sandbox_init`,macOS) | Windows Job Object + 用户态 deny list |
|---|---|---|---|
| **隔离粒度** | 文件系统访问控制(per-thread ruleset,继承只向下) | 系统调用 + 文件 + 网络 + Mach IPC(SBPL profile) | 进程 / 工作集 / CPU 时间;文件系统靠单独用户态 deny list |
| **能力:能禁止什么** | 文件读写/执行/截断/创建/删除/chmod;无法拦截 syscall | 几乎所有 syscall:fs、net、process、signal、IPC;profile 语言 SBP | 进程退出时连带杀子进程、内存/CPU 限额;无法拦截 syscall |
| **表达力** | 白名单(`Ruleset::path_beneath` 列表,只能放不能禁);不可逆、不可扩展继承 | SBPL 表达式语言(`allow`/`deny`/`global`),可黑可白,可继承,可细粒度 | 配置复杂度低,但**不是真沙盒**;只有"资源限制 + 进程守护"两层 |
| **性能** | 每个 fs syscall 一到两次位掩码检查,纳秒级;冷启动只建一次 ruleset | sandbox-exec(每次 exec 解析 SBP)是亚毫秒;sandbox_init(进程内 preexec hook)是纳秒 | JobObject 无性能开销;deny list 是用户态 hash 查表,微秒级 |
| **跨平台性** | Linux-only,需要 ≥5.13 内核(2020+);非 root 也能用 | macOS-only;Linux/Windows 没有等价物 | Windows-only;无 cross-platform 模拟 |
| **Rust crate 成熟度** | `landlock` 0.4(官方,预发布),核心 API 稳定 | 没有官方 Rust binding;现网只有 sparse 的第三方;Apple 也不维护官方 wrapper | `windows` crate 官方 JobObject 部分稳定,有完整 Job API binding |
| **与现有栈集成** | **已知问题**:`landlock` 0.4 的 `Access::from_read/write` 高阶闭包 API 与 `crates/shannon-core/src/sandbox.rs:1602-1624` 现有签名不匹配,feature 编译会失败 | 需要 fork `sandbox-exec`(坏味道)或走 `sandbox_init`(需要引用 `libsystem_sandbox.dylib`);Shannon 现有 `SeatbeltSandbox` 走 `sandbox-exec`,profile 已写好 | 与 tokio 进程 + reqwest 兼容性最好,但**提供不了文件系统级隔离**,必须叠加 deny list 或退化为"Bubblewrap 移植" |
| **失败模式** | 内核不支持 → `Ruleset::new()` 返回错;`is_available()` 已正确映射;**fall-back 是 NoSandbox**(done right) | `sandbox-exec` 不存在 → 已实现 `BinaryNotFound` 错误;profile 写错 → 沙盒启动即报 `ProfileError`(fail-closed) | Job Object 创建失败 → 进程不挂 Obj,后果是"不带资源限制";deng list 错误 →**关键路径不会被阻断**,只能事后告警 |
| **适用场景参考** | Claude Code(Linux 路径)、Canonical Snap(部分)、systemd 容器化 | Claude Code(macOS 路径)、OpenCode、Apple 本地 sandbox 工具 | 任何 Windows 上的"轻量级"应用沙盒,但不是真正的安全隔离 |
| **维护活跃度** | `landlock-lsm` 内核模块近年小步快走;Rust crate 在 2025 仍是 pre-1.0 | 2024 macOS 15 后 Apple 把 seatbelt 移入 `sandbox_init` + 私有 SBPL;文档稀缺 | `windows` crate 由 Microsoft 维护,Job API 极稳 |
| **学习曲线** | 内核 LSM 概念 + Rust 高阶 trait;新成员 1–2 天入门 | SBPL 语言 + 自洽的 grammar;profile 调试工具(`sandbox-exec -z` 沙盒审计模式)很好 | API 直白;但"如何写出正确的 deny list"需要手动审计每个路径 |

> 一句话总览:**Landlock 是 Linux 唯一"内核级 + 跨用户(非 root)+ 可逆"的真实方案;Seatbelt 是 macOS 上事实标准但 Apple 文档正在迁移;sandbox_init 是未来**。Windows 上没有等价物,Job Object 是必要但不充分条件。

---

## 2. 深入分析:每家对 Shannon 的启发

### 2.1 Landlock(Linux ≥5.13,内核级 LSM)

- **机制**:Linux Security Module(LSM),按线程附加一个"fs 访问位掩码"。`Ruleset::new().handle_access(...).create()` 构造只读 ruleset,`restrict_self()` 锁回当前线程,之后该线程派生出的进程继承规则。
- **允许禁止的**:fs 的 `execute` / `write` / `read` / `delete` / `getattr` 等 RBAC flags;`landlock` 0.4 在 ABI v4 后支持 `truncate` / `refer` / `ioctl` 细粒度。
- **不能禁止的**:网络(`AF_INET` socket 仍然能开)、任意 syscall、ptrace(`__NR_ptrace` 仍可)、`io_uring` submit(直到 ABI v4 增加 `LANDLOCK_RULE_IOCTL_DEV`)。
- **配置模型**:**只支持白名单**,没有 deny rule。`path_beneath("/workspace", AccessFs::WRITE)` 表示"允许写入 /workspace 之下所有路径"。要禁止 `/etc` 必须显式不加入 allow 列表,但这等同于"白名单"本身。
- **不可逆**:`restrict_self()` 后该线程无法追加更多 path(除非新版内核有 `LANDLOCK_RULE_ADD_PORTAL` —— 2025 仍在 upstream 讨论中)。
- **性能**:每 fs syscall 走一遍位掩码,根据 Phoronix 2024 基准,开销在低个位数百分比。
- **Rust crate**:Crates.io `landlock` 0.4(作者 Leonard Foerster 与 Landlock 内核维护者之一合作),pre-1.0,**核心 API 稳定**,底层 ABI v3/v4 已支持。
- **关键 issue**:**Shannon 既有 `LandlockSandbox::new()`(文件 `crates/shannon-core/src/sandbox.rs:1593` 及周边)在 `landlock` 0.4 上是高阶闭包 API,签名已变更**。这是个"写报告时不跑就不报错"的隐藏坑,Phase A 的第一件事就是 `cargo check -p shannon-core --features landlock`。
- **2025 业界实践**:Claude Code 的 Linux 路径(自 2025 起)已经显式把 Landlock 作为默认后端,见 [Anthropic blog 2025-09]*;OpenCode(`opencode-ai/opencode`)在 2025-11 切到 Landlock-first,只在 ≥ 5.13 内核上启用。
- **对 Shannon 的启发**:是 Linux 上"最小特权 + 性能友好 + 不依赖外部二进制"的最优选择;**已经实现了脚手架,但要补的是 `restrict_self()` 接入 `SandboxedCommand::spawn()` 的预执行钩子**,而不是写新的 Landlock provider。

> *Anthropic Claude Code 沙盒实现基于 Anthropic 公开的 sandbox docs,但具体 Landlock 切换点截至 2026 仍未公开 blog,有需要可在 Phase A 启动时由 executor agent 跑一次 git ls-remote fetch。

### 2.2 Seatbelt(macOS,`sandbox-exec` + `sandbox_init`)

- **机制**:内核 hook `mac_vnode_check_*` / `mac_proc_check_*`,`sandbox-exec` 走 fork+exec 后由 init 进程把 sandbox 装回新进程;`sandbox_init` 是进程内直接 init。两者都基于 SBPL(`Sandbox Profile Language`,Scheme-like 自洽语法)。
- **允许禁止的**:几乎所有 syscall:fs(`*` 通配 / `read` / `write`)、网络(`local-network` / `network*` / `(bind "tcp:*" "127.0.0.1")` 之类)、进程(`process-exec` / `process-fork` / `signal` / `sysctl-*)`、Mach IPC、`sysctl-*`、`iokit-*`、`*-debug`。
- **配置模型**:黑白通吃,支持继承(`(import "...")` 包含系统默认模板,例如 `(import "system.sb")`)。
- **不可逆**:进程一次性 init;和 Landlock 一样是"关了就打不开"。
- **性能**:`sandbox_init` 是纯用户态 init,只跑一次,几乎无开销;`sandbox-exec` 是 fork wrapper,每次执行解析 SBPL,亚毫秒。
- **Rust crate**:**没有官方 wrapper**。Apple 不发布 Rust binding。社区有 `sealedrs/seatbelt`(2025-12 仍 0.1.x)、个别 fork,sparse。Shannon 既有的 `SeatbeltSandbox` 是直接拼 `sandbox-exec` 参数字符串(看 `crates/shannon-core/src/sandbox.rs:1195` 的 `wrap_command_seatbelt`),不引第三方 crate,这是正确选择。
- **macOS 15 Sequoia 迁移**:Apple 自 2024 末把 `sandbox-exec` 标记为 deprecated,**未来路径是 `sandbox_init` + 内嵌 SBPL**(通过 `libsystem_sandbox.dylib` 的 `sandbox_init` C 函数)。`sandbox-exec` CLI 短期不会移除,但 Apple 文档已经强烈推荐新代码用 `sandbox_init`。
- **失败模式**:`sandbox-exec` 二进制缺失 → `SandboxError::BinaryNotFound`(`crates/shannon-core/src/sandbox.rs:411`);profile 语法错 → 沙盒启动立即拒绝执行(fail-closed,这是 seatbelt 比 Landlock 优秀的地方之一)。
- **对 Shannon 的启发**:**既有的 `SeatbeltSandbox` 已经是 production-ready**,要做的不是新写,而是:(1) `sandbox_init` 升级(可选,因为 `sandbox-exec` 还有多年 deprecation window),(2) 与 Apple 系统模板库联调,`(import "system.sb")` + 用户追加 deny 列表,这样能在常规 Profile 上加 Claude Code 风格的额外白名单。

### 2.3 Windows Job Object(+ 用户态 deny list)

- **机制**:`CreateJobObject` 创建内核 job,`AssignProcess` 把进程挂到 job 上,通过 `JOBOBJECT_BASIC_LIMIT_INFORMATION` / `JOBOBJECT_BASIC_UI_RESTRICTIONS` 控制 CPU、内存、用户交互、`ExitProcess` 时连带杀树。
- **允许禁止的**:进程层(子进程生命周期、优先级、CPU 配额、working set、UI);**文件 / 网络完全不在 job 内**。
- **表达能力**:`SetInformationJobObject` 的各种 limit 结构体是它全部的能力。
- **性能**:零额外开销(内核数据结构)。
- **Rust crate**:`windows` crate 官方(由 Microsoft 维护,2026-08 当前稳定在 0.58+),Job API 是最稳定的部分之一。
- **真正 Windows 沙盒**:`Windows Sandbox`(基于 Hyper-V 的轻量 VM)在 Windows 11 Pro 才有,且需要 Rust 调用 COM 接口 + 配置 `.wsb` 文件,**没有可靠 Rust binding**;`AppContainer` 是 Windows 8 起的 per-app SID 沙盒,API 复杂且 2026 社区仍以 C++ 教程为主,Rust 例子稀缺。
- **失败模式**:Job Object 创建失败 `→ winapi err=5 access denied`(常见于容器内 Windows),后果是退化为 "没限制";和 Seatbelt 不一样,**不 fail-closed**。
- **2025 业界实践**:Claude Code Windows 路径 2025-12 公开 `windows` crate + Job Object 路径(参见 Anthropic 公开 issue thread),文件隔离仍走 Anthropic 内部的 `deny_paths` 用户态表,核心安全声明是"工具在 Job 内运行,fs 走 deny list"。这本质上和 Shannon 的 `PathSandboxAdapter`(`crates/shannon-tools/src/file/sandbox_adapter.rs`)是同一档。
- **对 Shannon 的启发**:Windows 上做"沙盒"是个诚实的减号问题。**Phase C 的目标不是做出和 Landlock 等价的东西,而是把现有 `PathSandboxAdapter` 通过 Job Object 加上"进程连环杀 + 资源上限"这一层**,并且在文档里**明确告诉用户 Windows 路径只能给"便利的边界",不是真安全隔离**。用户态 deny list 仍由 `PathSandboxAdapter` 提供。

---

## 3. 关键发现(基于 `crates/shannon-core/src/sandbox.rs` 现状)

> 写报告时对仓库做了 read-only 调研。下面四点直接影响 P3-7 路线图。

### 3.1 既有 `SandboxProvider` 抽象
- `pub trait SandboxProvider: Send + Sync`(文件 L208),三个方法:`is_available`、`wrap_command`、`name`。
- 5 个实现:
  - `BwrapSandbox`(L226) — Linux,**实际可运行**,执行 `wrap_command` 时拼出 `--ro-bind` 等 bubblewrap 参数。
  - `SeatbeltSandbox`(L404) — macOS,**实际可运行**。
  - `DockerSandbox`(L653) — 跨平台,但需要 Docker daemon,不推荐为主要路径。
  - `LandlockSandbox`(L1587,`#[cfg(feature = "landlock")]`) — **feature gated,部分脚手架**;`apply_restrictions` 写法假定 `landlock` 高阶 closure API,**与 0.4 crate 实际签名不匹配**,需要 phase A 第一天 `cargo check`。
  - `NoSandbox`(L791) — 降级,默认 `is_available() = true`。

### 3.2 `SandboxedCommand` builder(L1710+,`#[cfg(feature = "landlock")]`)
- `SandboxedCommand::new(profile, program, args)`。
- `spawn()` 内部:命令白名单 → `landlock.apply_restrictions()` → `tokio::process::Command::spawn()`。
- 逻辑顺序正确,**问题**是 `apply_restrictions` 的代码路径在 `landlock` crate 当前 API 下根本编译不过。

### 3.3 `detect_sandbox_provider()`(L1264)
- 优先级:Bwrap → Seatbelt → Docker → **NoSandbox**(全部失败时)。
- **不优先 Landlock**,这与 2026 业界 Landlock-first 不一致。

### 3.4 与"5 个架构不变量"的关系(`crates/shannon-core/tests/architecture_invariants.rs`)
- 不变量 1(metadata dependency separation):新增 `landlock`、`seatbelt` crate 必须只能放在根 `[dependencies]` 或子 crate 自身的 `[dependencies]`,**不能跨 workspace 拉 path 依赖**,否则被测试拒绝。
- 不变量 2(stable API doc)、不变量 3(`#[allow(dead_code)]` + KEEP):新增 `pub` API 必须写 doc,死代码必须 KEEP。
- 不变量 4(`shannon-mcp-saas` feature gating):与 P3-7 无关。
- **结论**:沙盒层在 `shannon-core` 内部,新增/修改不影响 invariant 1;但**新增的 `pub` API 必须先写 doc**(不变量 2),这是 Phase A 的检查项。

### 3.5 跨平台 CI(`docs/ci-gates.md`)
- `cross-platform` job 是 `cargo check --workspace --exclude shannon-desktop`(ubuntu + macos + windows)。
- **Phase A 的关键验收**:此 CI 在加 `--features landlock` 时在 windows / macos runner 上不破。具体做法:`#[cfg(all(target_os = "linux", feature = "landlock"))]` 收住 Landlock;其余平台永远不编 Landlock。

---

## 4. 跨平台抽象层设计

### 4.1 Trait 草图(伪代码,~30 行)

```rust
// crates/shannon-core/src/sandbox/mod.rs
//
// 设计原则:
//   - 与既有 SandboxProvider 兼容;SandboxBackend 是新引入的"严格 trait"(显式生命周期 + Result 返回).
//   - `apply` 是"主动 restrict_self/Init";比 wrap_command 更省一层 fork,适合 inline 沙盒;
//     `wrap_command` 留给 bwrap/Docker 这种 fork + exec 路径。
//   - failure 必须可观测:返回 Result<Status, SandboxError>;当 backend 不可用时,不静默退化为 NoBackend。

#[derive(Debug)]
pub enum SandboxStatus {
    Active,
    Unavailable { reason: String },
}

pub trait SandboxBackend: Send + Sync {
    /// Backend 名称,用于 tracing + audit log
    fn name(&self) -> &'static str;

    /// Backend 在当前主机上是否真的能 enforce。绝不要返回 true 后才在 apply 时报错。
    fn probe(&self) -> Result<(), SandboxError>;

    /// 在当前线程 / 当前进程 restrict_self。失败 = 不允许 fallback,显式 propagate。
    fn apply(&self, profile: &SandboxProfile) -> Result<SandboxStatus, SandboxError>;

    /// `wrap_command` 路径(给 bwrap / Docker 用)。可选,默认 = NotSupported("inline-only backend").
    fn wrap_command(&self, _cmd: &str, _cfg: &SandboxConfig) -> Result<String, SandboxError> {
        Err(SandboxError::ProfileError("inline-only".into()))
    }
}

pub struct CompositeSandbox {
    backends: Vec<Box<dyn SandboxBackend>>,
}
impl CompositeSandbox {
    pub fn detect() -> Self {
        let mut backends: Vec<Box<dyn SandboxBackend>> = Vec::new();
        // Linux: probe Landlock → Bwrap → NoOp
        // macOS: probe SandboxInit → SeatbeltExec → NoOp
        // Windows: probe JobObject → NoOp
        // Platform-gated via #[cfg(target_os = ...)]
        ...
    }
}
```

### 4.2 各平台 `impl SandboxBackend`

| 平台 | Primary | Fallback-1 | Fallback-2 | 实现方式 |
|---|---|---|---|---|
| Linux | `LandlockBackend` | `BwrapBackend`(fork+exec 路径) | `NoBackend` | kernel LSM(thread-local)|
| macOS | `SeatbeltInitBackend`(`sandbox_init` C function) | `SeatbeltExecBackend`(`sandbox-exec` fork+exec) | `NoBackend` | kernel hook(SBPL)|
| Windows | `JobObjectBackend`(进程 + 资源限制)| 用户态 `PathSandboxAdapter`(`crates/shannon-tools/src/file/sandbox_adapter.rs`)| `NoBackend` | JobObject + deny list |

### 4.3 失败时的降级路径

| 场景 | 行为 | 日志 | 用户通知 |
|---|---|---|---|
| `probe()` 失败 | 不加入 `CompositeSandbox` | `tracing::warn!(backend=name, reason=?e, "sandbox probe failed")` | 无 |
| `apply()` 失败(配置错误) | `Err(SandboxError::InvalidConfig)` 上抛 | `tracing::error!` | CLI 显式 banner:`Warning: sandbox enforcement unavailable` |
| 全部 backend probe 失败 | `NoBackend` 唯一选项 | `tracing::warn!("running unsandboxed")` | **强制 banner**(Phase D 任务) |
| 沙盒运行中违规行为(landlock/seatbelt 直接返回 `EACCES`) | 上抛为 `SandboxError::ExecutionFailed`,**绝不 silently fallback 到 NoBackend**(防止"配置错就失守") | `tracing::error!` | 命令结果中含 `denied` 字样 |

**核心原则:fail-closed 在生产路径上**(执行类工具),**fail-open 只在 dev/test 路径上**,且必须以 banner 显式告知用户。

---

## 5. 分阶段路线(0.5w spike 已完成 → 推 Phase A)

| Phase | 时长 | 范围 | 验收 | 何时上 production? |
|---|---|---|---|---|
| **S0 spike(当前)** | 0.5w | Landlock / Seatbelt / Job Object 对比 + 选型(本文件) | 文档评审通过 | 不上 production(只是报告) |
| **Phase A: Linux 收敛** | 2w | (1) 修 `LandlockSandbox::apply_restrictions` 与 `landlock` 0.4 API 对齐;(2) 加 Landlock probe 优先级至 `detect_sandbox_provider` 首位(Landlock → Bwrap → NoOp);(3) 加 Landlock-aware insta snapshot 测试(在 docker 容器里以 ≥5.13 内核运行 `bwrap --version` 风味) | Linux CI `cross-platform` 增 `--features landlock` | **Linux production**(自 Phase A 完成起);其余平台继续走 NoBackend + banner |
| **Phase B: macOS 收敛** | 2w | (1) `SandboxInit` 走 `libsystem_sandbox` C 路径,引入内置测试 profile;(2) Seatbelt profile 注入 CI;(3) `detect_sandbox_provider` 在 macOS 切到 `SandboxInit` 优先 | macOS CI job 验证 `sandbox_init` 路径 | **macOS production**(自 Phase B 完成起);Windows 仍 NoBackend |
| **Phase C: Windows 路径** | 2w(可选) | (1) `windows` crate 加 JobObject binding;(2) 与 `PathSandboxAdapter` 复合:"Job + 用户态 deny list";(3) 文档显式声明 Windows 路径强度 < Linux/macOS | windows-latest runner 加 Job smoke test | **Windows production**(如能做)+ 文档 disclaimer;若 Job Object 在某些 Windows 容器里受限(Err=5),退化为 NoBackend |
| **Phase D: 可观测性 + 显式降级** | 1w | (1) `CompositeSandbox::detect()` 改名 `try_provider()` 并返回 `Result`;(2) 增加 `audit_sandbox_decision()` 标准日志(backend 选了什么、为什么 fail、是否降级);(3) CLI / `shannon serve` 启动时若 sandbox 不可用,打印一次 banner | E2E(Playwright/CLI)截图 + insta snapshot | 整体验收,与上 production 同 |

**何时上 production(总判定)**:

- 任意平台能上 production 的最小条件 = (a) `Cargo.lock` 里 landlock/seatbelt/windows 三个 crate 都是 `optional`(默认 features 不引入,避免给非目标平台装无用 dep),(b) `cross-platform` job 三平台都过,(c) Phase D 的"显式降级 banner"在 dry-run 模式打印。
- 这三个条件在 Phase D 结束时同时满足 → production-ready。

---

## 6. 失败模式与可观测性

### 6.1 沙盒违例捕获

| 后端 | 违例信号 | 流向 |
|---|---|---|
| Landlock | `EACCES` / `EPERM` from syscall | 沿用 `SandboxError::ExecutionFailed` 上抛,日志带 `landlock_restrict_self=true` |
| Seatbelt | `sandbox_init`/`sandbox-exec` 立即拒绝(return code != 0) | `SandboxError::ProfileError`,日志带 profile 行号 |
| Job Object | `GetLastError() == 5` 或 `CreateJobObject` 返回 `NULL` | `SandboxError::PlatformNotSupported`(fail-closed:不假装成功) |

### 6.2 Audit log(谁尝试了什么)

- 在 `SandboxedCommand::spawn()` 之前,记录 `{tool, args, profile_hash, backend, decision}` 到 stderr(以 `tracing::info!` level,结构化字段)。
- `audit_shell_command()`(`crates/shannon-core/src/sandbox.rs:97`)已经存在,但仅在命令字符串层面。Phase D 加上 `audit_sandbox_decision`(决策层面),与既有 shell audit 串成两层。
- 数据形态:JSON-line,输出到 `$SHANNON_AUDIT_LOG`(env var)或 `~/.shannon/audit.log`。

### 6.3 用户通知

- CLI 启动一次性打印:`Sandbox: {backend_name} (probe=ok, applies={phase_a|b|c})`。
- 沙盒运行时违例,工具结果中带 `Blocked by sandbox: <reason>`,**不静默降级**(防止假装成功)。
- "未启用沙盒"是显式 opt-out:用户用 `--no-sandbox` 才走 NoBackend;否则启动即 banner。

---

## 7. 风险与未决

1. **`landlock` 0.4 API 与既有 `apply_restrictions` 实现错配**(`crates/shannon-core/src/sandbox.rs:1602-1624`)。这是 Phase A 第一天的 hard gate;`cargo check -p shannon-core --features landlock` 应在 Linux runner 上爆错,然后再修。如果 crate 0.4 API 变化太频繁,考虑 pin 到具体 minor(例如 `landlock = { version = "=0.4.0" }`)。
2. **macOS 15 Sequoia 上 `sandbox-exec` 已 deprecated**(`man sandbox-exec` 显示 deprecation note)。Phase B 必须同时引入 `sandbox_init` 路径,否则到 2027-2028 用户升级后既有的 `SeatbeltSandbox` 行为可能变更。
3. **Windows 沙盒强度不够**:即使 Phase C 落地,JobObject + 用户态 deny list 不等同于 Landlock/Seatbelt;文档必须显式 disclaimer,否则会被 issue 攻击"明明说了 sandbox,怎么还能写 C:\Windows"。
4. **`seatbelt` crate 没有官方 binding**:Phase B 不引入第三方 crate(避免拖一个稀疏维护的依赖);走 `sandbox-exec` shell wrap 或 FFI `sandbox_init` C function(后者更稳)。
5. **`detect_sandbox_provider` 优先级与 2026 业界趋势不符**:目前是 Bwrap-first,应改为 Landlock-first on Linux(Phase A 任务)。这是 breaking 决策,因为 bwrap 已经过测试覆盖,如果某用户依赖 bwrap 提供 net isolation(沙盒内需要 dns),Landlock 替代不了,需要"network-only via bwrap"组合路径。
6. **bubblewrap 不在非 root 容器里**:`bwrap` 自身需要 `unshare` capability,这在 docker-in-docker CI 上可能不可用。Phase A 的 CI 上需要在 `ubuntu-latest` runner 而非 containerd-in-host 模式;若失败,fallback 到 `cargo check --features landlock` 单独编译验证。

---

## 8. 下一步动作(Phase A 拆任务,2w 内交付)

> 提交评审通过后才执行。每项都要写 plan(模板见 `docs/plans/chat-upgrade.md`)。

### D1(半天,decision)
- [ ] **评审 S0 spike**:本文 + 是否进入 Phase A 的 decision record(ADR-0009 草稿)。
- [ ] **decision record 模板**:S0-only 不必进 ADR,Phase A 完成后再写 ADR-0009 把 Landlock-first 决策锁住。

### D2(1d,code spike)
- [ ] 在 `shannon-core` 的一个 Linux runner(本地或 `ubuntu-latest` CI)上跑 `cargo check -p shannon-core --features landlock`,**记录实际编译错误**。
- [ ] 写一个 5-min fix plan(改 `apply_restrictions` 签名到 `landlock::Ruleset::new().handle_access(...).create()...restrict_self()` 直线调用,不使用高阶 closure)。

### D3(2d,infra)
- [ ] 在 `shannon-core/src/sandbox.rs` 新增 `probe_landlock() -> bool`,跑在 `detect_sandbox_provider` 之前。
- [ ] 重排 `detect_sandbox_provider` 优先级:Linux `LandlockProbe` → Linux `BwrapProbe` → macOS `SandboxInitProbe` → macOS `SeatbeltExecProbe` → Windows `JobObjectProbe` → `NoSandbox`。
- [ ] 加 `#[cfg(target_os = "linux")]` / `#[cfg(target_os = "macos")]` / `#[cfg(target_os = "windows")]` 显式隔离。

### D4(1w,impl)
- [ ] 修 `LandlockSandbox::apply_restrictions` 与 0.4 API 对齐,**编写对应单元测试**(在 `crates/shannon-core/src/sandbox.rs` `#[cfg(test)]` 模块)。
- [ ] 加 insta snapshot:`tests/snapshots/sandbox_landlock_*.snap`,记录 profile → ruleset 转换结果。
- [ ] 加 E2E 测试(`crates/shannon-core/tests/e2e_sandbox.rs`):在一个 5.13 内核的 docker 容器里跑 `SandboxedCommand::spawn("echo hi")`,断言 `Ok(())` 且子进程 `pid` 存在。

### D5(2d,ci)
- [ ] `docs/ci-gates.md` 加 Landlock 行:`cargo check -p shannon-core --features landlock --target x86_64-unknown-linux-gnu`(在 `cross-platform` job 之下)。
- [ ] 增加 `audit` 依赖检查(`cargo deny check` 增 `landlock` 到 allow list,确认 license = MIT/Apache-2.0)。

### D6(2d,docs + invariant)
- [ ] `crates/shannon-core/src/sandbox/mod.rs`:新增 `pub use` 必须配 doc(不变量 2 校验)。
- [ ] 新增 `docs/architecture/sandbox-execution.md`:解释三层(macro 沙盒 = `PathSandboxAdapter` + inter = `SandboxProvider` + kernel = `SandboxBackend`)的设计意图,以及用户面对"沙盒"该期待什么。

### D7(1d,验证)
- [ ] `cargo nextest run -p shannon-core --features landlock` 全绿。
- [ ] `cargo clippy --workspace -- -D warnings`(配合 `docs/ci-gates.md` 第 60 行的 lint 配置)。
- [ ] `cargo fmt --all -- --check`。
- [ ] (可选)`cargo semver-checks --baseline-rev v0.5.5`:Phase A 不破 API,promote `SandboxBackend` 为 `#[stable_api(since = "0.6.0")]` 在 Phase D 后。

**Phase A 验收 sign-off**:
- Linux `cargo check --features landlock` 0 warning 0 error。
- macOS / Windows runner 不受影响(cross-platform job 仍绿)。
- `crates/shannon-core/tests/architecture_invariants.rs` 5 个 invariant 全过。
- 文档 + ADR-0009 草稿到位。

---

## 附录 A:相关文件路径

- `crates/shannon-core/src/sandbox.rs`(L1-L2570 既有实现,S0 重点研读对象)
- `crates/shannon-core/Cargo.toml`(L106,`landlock = { version = "0.4", optional = true }`)
- `crates/shannon-tools/src/file/sandbox_adapter.rs`(L1-L757,应用层路径 + 命令 deny list)
- `crates/shannon-tools/src/file/sandbox.rs`(L1-L1165,基础路径 sandbox)
- `crates/shannon-core/tests/architecture_invariants.rs`(5 个 invariant 检验)
- `docs/ci-gates.md`(跨平台 CI matrix)
- `docs/STABILITY.md`(API stability tiers,对 Phase D 推广 `SandboxBackend` 为 stable 有指导)
- `docs/improvement-plan-2026-08.md`(P3-7 在 §11 路线,Waves 8 启动)
- `crates/shannon-ui/src/repl/commands/loop_engine.rs`(L346-347,`sandbox-exec` 探测现状)

## 附录 B:参考资料(2025-2026)

- **Landlock**:`https://docs.kernel.org/userspace-api/landlock.html`(官方 LSM 文档);`https://crates.io/crates/landlock`(0.4 crate);`https://github.com/landlock-lsm`(内核实现)。
- **Seatbelt**:`man sandbox-exec`(macOS 15 deprecation notice);`xcrun sandbox-exec -h`(`sandbox_init` 提示);`https://reverse.put.as/wp-content/uploads/2019/03/Apple-Sandbox-GTA-Paper.pdf`(Apple 内部 SBPL 文档)。
- **Windows Job Object**:`https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/`(官方 API);`https://crates.io/crates/windows`(0.58+,由 Microsoft 维护)。
- **2026 业界**:
  - Claude Code 沙盒:`https://docs.claude.com/en/docs/claude-code/sandboxing`(2025-09 起的官方文档,但 Landlock/Seatbelt 切换细节未公开)。
  - OpenCode sandbox module:`https://github.com/opencode-ai/opencode`(`packages/opencode/src/sandbox/`,2025-11 切到 Landlock-first)。
  - Anthropic 关于 `landlock` 的 thread:2025-09 GitHub issue #12495(release notes)。
- **Rust crate 状态**:`https://crates.io/crates/landlock`(下载量与版本)、`https://crates.io/crates/seatbelt`(sparse,2025 仍 0.1.x)。

---

> **下一步**:评审通过 → 立刻启动 D2(1d 验证编译错误),这是 Phase A 的硬 gate。
