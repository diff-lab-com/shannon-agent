# 远程目标连接（SSH / Docker 容器）实施方案

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 Shannon 的全部工具（Bash/Read/Write/Edit/Grep/Glob/…）可透明地在远程 SSH 主机或 Docker 容器内执行，并提供 TUI `/remote` 命令、CLI `--target` 与 Desktop Settings→Remotes 管理 UI。

**Architecture:** 新 crate `shannon-remote` 以系统 `ssh`（openssh crate）+ `docker exec` 实现 `ProcessProvider`/`FileSystemProvider` 两个既有 trait；`DynamicWorld` 装饰器运行时热切换本地/远程世界；`PathSandbox` 共享根 + `FileSystemProvider::walk` 使沙箱与遍历跟随世界切换。

**Tech Stack:** Rust workspace（edition 2024）、`openssh` + `openssh-sftp-client`、tokio、Tauri v2 + React 19 + react-intl、vitest。

**Spec:** [docs/plans/2026-09-04-remote-connections-design.md](./2026-09-04-remote-connections-design.md)（v2，评审通过）

## Global Constraints

- Rust 1.85+ / edition 2024；`cargo clippy --workspace -- -D warnings` 零警告；每个新源文件至少一个 `#[test]`。
- 测试命令一律 `cargo nextest run -p <crate>`（仓库规范 `just test` 全量）。
- 不新增密钥/口令存储；SSH 认证完全委托系统 ssh；`remotes.toml` 权限 0600。
- 本地行为零回退：`LocalFs` 的 walk 覆盖实现必须复用 `ignore::WalkBuilder`；既有 Grep/Glob/文件工具测试必须全绿。
- i18n：TUI 改 10 个 `locales/*.yml`；Desktop 改 `en.json` + `zh-CN.json` 两份且同 PR。
- 提交遵循仓库 conventional commits（`feat:`/`test:`/`docs:`）。
- 每个任务结束独立提交；集成测试（需本机 sshd/docker）一律 `#[ignore]`。

---

### Task 1: crate 脚手架 + 首日探针

**Files:**
- Create: `crates/shannon-remote/Cargo.toml`, `crates/shannon-remote/src/lib.rs`
- Modify: `Cargo.toml`（workspace members + workspace.dependencies）

**Interfaces:**
- Produces: crate `shannon-remote`（lib name 同名），空导出 + `#[cfg(test)]` 冒烟测试。

- [ ] **Step 1: workspace 注册**。根 `Cargo.toml`：`members` 追加 `crates/shannon-remote`；`[workspace.dependencies]` 追加：

```toml
openssh = "0.11"
openssh-sftp-client = "0.15"
async-trait = { workspace = true }   # 已有，确认键名
toml = { workspace = true }
```

crate 自身 `Cargo.toml`（对齐 `crates/shannon-core/Cargo.toml` 的写法，版本走 workspace 继承）：

```toml
[package]
name = "shannon-remote"
version.workspace = true
edition.workspace = true

[dependencies]
openssh = { workspace = true }
openssh-sftp-client = { workspace = true }
tokio = { workspace = true, features = ["rt", "process", "io-util", "sync", "time", "macros"] }
async-trait.workspace = true
serde.workspace = true
toml.workspace = true
thiserror.workspace = true
tracing.workspace = true
dirs.workspace = true
shannon-tool-interface = { path = "../shannon-tool-interface" }

[dev-dependencies]
tempfile.workspace = true
```

- [ ] **Step 2: lib.rs 骨架 + 模块声明**（模块文件后续任务填充，先全部建空文件带 `//!` 文档与一个占位 test）：

```rust
//! Remote execution worlds (SSH hosts, Docker containers) for Shannon tools.
//! Implements `ProcessProvider`/`FileSystemProvider` over system ssh + docker exec.
pub mod target;
pub mod ssh;
pub mod docker;
pub mod dynamic;
// 注：目录遍历归 shannon-tool-interface（trait 默认实现），本 crate 不设 walk 模块。

#[cfg(test)]
mod smoke {
    #[test]
    fn crate_links() { assert!(true); }
}
```

- [ ] **Step 3: 探针**。`cargo check -p shannon-remote`。若 `openssh`/`openssh-sftp-client` 与 edition 2024 冲突：记录替代版本；彻底不兼容则触发设计 §10 降级（russh）——把结论写进本文件 Task 1 复选框备注。
- [ ] **Step 4: Commit** `feat(remote): scaffold shannon-remote crate`

### Task 2: RemoteTarget 模型 + remotes.toml 持久化

**Files:**
- Create: `crates/shannon-remote/src/target.rs`
- Test: 同文件 `#[cfg(test)]`

**Interfaces:**
- Produces:
  - `pub enum TargetKind { Ssh, Docker }`
  - `pub struct RemoteTarget { pub name: String, pub kind: TargetKind, pub host: Option<String>, pub port: Option<u16>, pub user: Option<String>, pub container: Option<String>, pub shell: Option<String>, pub ssh_target: Option<String>, pub workspace_dir: PathBuf }`
  - `pub struct RemotesFile { pub default_target: Option<String>, pub targets: Vec<RemoteTarget> }`
  - `impl RemotesFile { pub fn load(path: &Path) -> io::Result<Self>; pub fn load_default() -> Self; pub fn save(&self, path: &Path) -> io::Result<()>; /* 0600 */ pub fn resolve(&self, name: &str) -> Option<&RemoteTarget>; pub fn resolve_active(cli: Option<&str>, env: Option<&str>, file: &Self) -> Option<RemoteTarget> /* CLI > env > default */ }`
  - `pub fn remotes_path() -> PathBuf  // ~/.shannon/remotes.toml`
  - `pub struct ValidationError(&'static str)` —— `RemoteTarget::validate()`：name 非空唯一、workspace_dir 必须绝对路径、Ssh 必须有 host、Docker 必须有 container。

- [ ] **Step 1: 失败测试**（TOML 往返 + 优先级 + 校验 + 0600）：

```rust
#[test] fn toml_roundtrip_preserves_all_fields() { /* serde Roundtrip on fixture with ssh+docker target */ }
#[test] fn resolve_active_priority_cli_over_env_over_default() { /* 三级断言，None→None */ }
#[test] fn docker_target_requires_container_and_absolute_workspace() { /* validate 拒绝用例 */ }
#[test] fn save_sets_0600_on_unix() { /* tempdir + std::os::unix::fs::PermissionsExt */ }
```

- [ ] **Step 2:** `cargo nextest run -p shannon-remote` 确认 FAIL（模块未实现）。
- [ ] **Step 3: 实现**：serde `#[serde(rename_all = "snake_case")]`、`#[serde(deny_unknown_fields)]`；`save` 用 `std::fs::OpenOptions` + `set_permissions(0o600)`（`#[cfg(unix)]`）。序列化形状严格等于设计 §4 的 TOML 示例。
- [ ] **Step 4:** 测试 PASS。**Step 5: Commit** `feat(remote): target model and remotes.toml persistence`

### Task 3: ~/.ssh/config 主机发现

**Files:**
- Create: `crates/shannon-remote/src/ssh/discover.rs`（`ssh/mod.rs` 同步建立）

**Interfaces:**
- Produces: `pub fn discover_ssh_hosts() -> Vec<SshHostCandidate>`，`pub struct SshHostCandidate { pub alias: String, pub user: Option<String>, pub hostname: Option<String>, pub port: Option<u16> }`。实现：`ssh -G -F <config?> <alias>` 逐别名探测太慢——直接**解析** `~/.ssh/config` 的 `Host <pattern>` 块（跳过含 `*`/`?` 的 pattern），再对每个别名跑一次 `ssh -G <alias>`（超时 2s，失败跳过）取生效 user/port。`include` 指令忽略并记录 tracing 警告。
- [ ] Step 1: 失败测试（fixture config 文本 → 解析出 3 别名、通配符跳过、ssh -G 用 mock：将解析与子进程探测拆成 `parse_ssh_config(text)` 纯函数 + `probe_effective()`，单测只测 parse）。
- [ ] Step 2: 实现 parse（状态机：按行去注释、`key value` 分词、块缩进无关）。
- [ ] Step 3: PASS + Commit `feat(remote): ssh config host discovery`

### Task 4: SshRuntime + SshProcess（ProcessProvider）

**Files:**
- Create: `crates/shannon-remote/src/ssh/session.rs`, `crates/shannon-remote/src/ssh/process.rs`

**Interfaces:**
- Produces:
  - `pub struct SshRuntime`：内部 `std::thread` + `tokio::runtime::Runtime`（current_thread），`mpsc` 命令队列；`pub async fn connect(target: &RemoteTarget) -> io::Result<Arc<SshRuntime>>`。构建参数：`SessionBuilder::new().batch_mode(true)`? —— 以 openssh 实际 API 为准：`openssh::SessionBuilder` 设 `.inactivity_timeout`（不做）、连接侧 `-o ConnectTimeout=10 -o StrictHostKeyChecking=accept-new -o BatchMode=yes`（经 `SessionBuilder` 的 `.key_option`/raw option 能力；若 crate 不暴露则 `connect()` 自行拼装 argv 用 `openssh::known_hosts::parse_known_hosts` 不涉及——**以 Task 4 实测 API 为准，备选方案是绕开 openssh 直接 `tokio::process::Command("ssh")` + ControlMaster 参数**，接口不变）。
  - `impl ProcessProvider for SshProcess`：`run_blocking`/`run_async`/`spawn_piped`/`capabilities()`。
  - `pub fn compose_command(req: &ProcessRequest, default_cwd: &Path) -> Vec<String>`（纯函数，单测核心）：

```rust
// 返回 argv：env 'K=V'... sh -c 'cd "$1" && shift && exec "$@"' sh <cwd> <program> <args...>
// req.cwd 覆盖 default_cwd；env 逐项 'K=V'；脚本体是常量字面量，无注入面。
// shift 不可省略：$1=cwd、$@ 自 program 起，缺 shift 会把 cwd 当命令执行（exit 126）。
```

  - `pub struct ExecCaps { pub is_remote: bool }`（放进 `shannon-tool-interface::ExecCaps`，`ProcessProvider::capabilities()` 默认 `ExecCaps { is_remote: false }`）。
  - 健康检查：`pub async fn health(&self) -> HealthReport { platform, home, bash_available, workspace_exists, latency_ms }`（`uname -s`、`echo $HOME`、`command -v bash`、`test -d <workspace>` 一次合并执行）。
  - 不健康判定：`exit.code == Some(255)` 或 channel 关闭错误 → `SshRuntime::mark_unhealthy()` → 一次重建；`pub fn status(&self) -> WorldStatus`。

- [ ] Step 1: 失败测试：`compose_command` 用例（cwd 覆盖、env 多项、args 原样、默认 cwd、`program` 含空格不特殊化、`shift` 端到端：用真实 `sh -c 'cd "$1" && shift && exec "$@"' sh <tmpdir> pwd` 断言输出 tmpdir 且退出码 0）；exit-255 分类；duplex 桥（用一个真实 `tokio::process::Command("cat")` 假装远程，验证 spawn_piped stdin→stdout 往返 + `wait`）。
- [ ] Step 2: 实现（见设计 §6.2 专属 runtime 语义：`run_blocking`→`block_on`；`run_async`→`spawn`+oneshot；`spawn_piped`→RemoteChild 流 ↔ `tokio::io::duplex` 泵）。
- [ ] Step 3: PASS；`#[ignore]` 集成测试 `ssh_localhost_roundtrip`（本机 sshd 存在才手动跑）。
- [ ] Step 4: Commit `feat(remote): ssh process world over system ssh`

### Task 5: SshFs（FileSystemProvider via SFTP）

**Files:**
- Create: `crates/shannon-remote/src/ssh/fs.rs`

**Interfaces:**
- Produces: `impl FileSystemProvider for SshFs`。SFTP 会话：M1 探针确认 `openssh::Session` 获取 sftp 子系统通道的方式（优先 crate 原生；否则 `ssh -S <control_socket> <host> -s sftp` 子进程），统一封装 `SshRuntime::sftp() -> SftpHandle`（内部 Mutex，操作间复用；跨 runtime 同样经专属 runtime 泵——SFTP 对象只在专属 runtime 创建，`*_blocking` 方法走 `block_on`）。
- 语义映射：`read_text/read_bytes`→`sftp.read`；`write_bytes`→`create+write+flush+close`；`metadata`→`sftp.metadata`→`FileMeta{len,is_dir,modified:mtime→SystemTime}`；`create_dir_all`→逐级 `create_dir`（忽略 AlreadyExists）；`rename`→优先 `posix_rename`（探针确认 crate 是否暴露；无则 `remove+rename` 兜底）；`canonicalize`→`canonicalize`；`remove_file_blocking`→`remove_file`；`list_dir_blocking`→`read_dir`→`DirEntryInfo`；`read_prefix_blocking`→`open+前 N 字节 read`。
- [ ] Step 1: 失败测试：路径工具函数（`remote_join`）单测 + `#[ignore]` 全 trait localhost 往返（tmp 目录：写→读→prefix→list→rename 覆盖→remove→canonicalize→metadata）。
- [ ] Step 2: 实现。Step 3: 单测 PASS（ignored 集成在有 sshd 的机器上跑一次并在 Task 1 备注）。Step 4: Commit `feat(remote): sftp-backed filesystem world`

### Task 6: Docker 世界

**Files:**
- Create: `crates/shannon-remote/src/docker/process.rs`, `crates/shannon-remote/src/docker/fs.rs`, `crates/shannon-remote/src/docker/mod.rs`

**Interfaces:**
- Produces:
  - `pub fn compose_docker_exec(req: &ProcessRequest, container: &str, shell: &str, default_cwd: &Path) -> ProcessRequest`（纯函数 → 输出本地 argv：`docker exec -w <cwd> [-e K=V]... -i <container> <program> <args...>`；`ssh_target` 存在时外层再套 `ssh <ssh_target> -- docker ...`）。
  - `impl ProcessProvider for DockerExecProcess`（经 `LocalProcess` 执行改写后的请求；capabilities: is_remote=true）。
  - `impl FileSystemProvider for DockerExecFs`：`list_dir_blocking`= `docker exec <c> find <dir> -mindepth 1 -maxdepth 1 -type d -print0` 与 `-type f -print0` 两次调用合并（`-mindepth 1` 必须有，否则目录列出自身导致 walk 无限递归；NUL 分割；dir 条目 `len=0` 并文档化为限制）；`metadata_blocking`= `docker exec <c> stat -c '%s %Y' <path>`（busybox 兼容；失败退 `len=0`）；`read*`=`cat`；`write_bytes*`=`docker exec -i <c> sh -c 'cat > "$1"' sh <path>` + stdin_data；`rename`=`mv -f`；`canonicalize`=`readlink -f`（失败返回原路径的 io::Error）；`create_dir_all`=`mkdir -p`；`remove_file_blocking`=`rm -f`。
  - `pub async fn list_running_containers(docker_host_ssh: Option<&SshTarget>) -> io::Result<Vec<ContainerInfo>>`：`docker ps --format '{{json .}}'` → `{id, names, image, status}`。
- [ ] Step 1: 失败测试：`compose_docker_exec` 全用例（-w/-e/-i/顺序/ssh 嵌套）；`stat` 输出解析（GNU 与 busybox 两种 fixture）；`find` 输出解析（含空格路径）。
- [ ] Step 2: 实现。Step 3: PASS + `#[ignore]` docker 集成（debian + alpine 镜像探针）。Step 4: Commit `feat(remote): docker exec execution world`

### Task 7: DynamicWorld + WorldState + assemble_providers

**Files:**
- Create: `crates/shannon-remote/src/dynamic.rs`, 更新 `lib.rs`

**Interfaces:**
- Produces:
  - `pub enum WorldStatus { Local, Connected, Degraded }`
  - `pub struct WorldState { /* watch channel + sandbox handle，见 Task 8 */ pub fn status(&self) -> WorldStatus; pub fn subscribe(&self) -> tokio::sync::watch::Receiver<WorldStatus>; pub fn active_target(&self) -> Option<String> }`
  - `pub struct DynamicWorld { inner: RwLock<WorldHandle> }`，`WorldHandle { fs: Arc<dyn FileSystemProvider>, process: Arc<dyn ProcessProvider>, caps: ExecCaps, target: Option<RemoteTarget>, runtime: Option<Arc<SshRuntime>> }`；两个 trait impl 委派（每次调用取读快照）；`pub async fn use_target(self: &Arc<Self>, t: RemoteTarget) -> io::Result<()>`（连 SshRuntime → 换 inner → 状态 Connected；失败保留旧世界并报错）；`pub fn disconnect(&self)`；`pub fn reconnect(&self) -> impl Future`。
  - `pub struct Assembly { pub providers: shannon_tools::ToolProviders, pub state: Option<Arc<WorldState>>, pub dynamic: Option<Arc<DynamicWorld>> }`
  - `pub fn assemble_providers(target: Option<&RemoteTarget>) -> Assembly`（None→DynamicWorld+本地；Some(Ssh)→静态 SshProcess/SshFs（同步接口内不能连接——返回 `Assembly::pending_ssh(t)` 变体，调用方在 async 上下文 `connect()` 后 `build()`；CLI 路径是 async OK）。
  - **依赖方向**：shannon-tools 不依赖 shannon-remote；`Assembly` 用 `shannon_tools::ToolProviders` 需要 dev-facing crate `shannon-remote` 依赖 `shannon-tools`（types only）。确认无环：shannon-tools 不依赖 shannon-remote ✅。
- [ ] Step 1: 失败测试：`FakeWorld`（两个 trait 的可编程假实现，计数调用）→ 切换后新调用走新世界、进行中的旧 `Arc` 克隆仍工作；watch 状态流转 Local→Connected→Degraded。
- [ ] Step 2: 实现。Step 3: PASS。Step 4: Commit `feat(remote): hot-swappable dynamic world`

### Task 8: PathSandbox 共享根 + home 世界感知

**Files:**
- Modify: `crates/shannon-tools/src/file/sandbox.rs`（`PathSandbox`）
- Modify: `crates/shannon-tools/src/lib.rs`（`ToolProviders` + `register_all_tools`）
- Modify: `crates/shannon-tools/src/sandbox/mod.rs:495,521`（字面量构造点补字段）

**Interfaces:**
- Produces:
  - `pub struct WorldSandboxHandle(std::sync::RwLock<WorldRoots>)`，`pub struct WorldRoots { pub allowed_roots: Vec<PathBuf>, pub home_dir: Option<PathBuf> }`；`pub fn set(&self, roots: WorldRoots)`。
  - `PathSandbox::with_world_sandbox(handle: Arc<WorldSandboxHandle>)`：设置后 `check_allowed_roots`/`check_home_boundary` 优先读 handle（`strict_mode` 与 denied patterns 仍来自 config）。
  - `ToolProviders { fs, process, denial_classifier, pub world_sandbox: Option<Arc<WorldSandboxHandle>> }`（Default→None；`register_all_tools` 在 `with_config` 后链 `.with_world_sandbox`；`ToolRegistrationResult` 增加 `pub file_history: Option<Arc<Mutex<FileHistoryManager>>>`——Task 12 用）。
- [ ] Step 1: 失败测试：`with_world_sandbox` 后 `set` 新根 → `validate_sync` 立即按新根判过/拒；home 边界随 `home_dir` 切换；未设 handle 时行为与现状逐字节一致（回归断言）。
- [ ] Step 2: 实现（最小 diff：两个字段读取点改走 `self.world.as_ref().map_or(config/home_dir, ...)`）。
- [ ] Step 3: 全量跑 `cargo nextest run -p shannon-tools`（sandbox 相关既有测试必须全绿）。Step 4: Commit `feat(tools): world-aware sandbox roots`

### Task 9: FileSystemProvider::walk + LocalFs 覆盖

**Files:**
- Modify: `crates/shannon-tool-interface/src/providers.rs`（trait 新方法）
- Modify: `crates/shannon-core/src/providers.rs`（LocalFs 覆盖）

**Interfaces:**
- Produces:
  - trait 新增（带默认实现，不破坏第三方 impl）：

```rust
/// Depth-first recursive walk used by Grep/Glob. Return `false` from the
/// callback to prune the entry's subtree (directories only).
fn walk_blocking(&self, root: &Path, skip_gitignore: bool, cb: &mut dyn FnMut(&DirEntryInfo) -> bool) -> io::Result<()> {
    // 默认实现：list_dir_blocking 递归 + 简化 .gitignore 匹配
    // （每层读 .gitignore，支持字面量/*/‌**/!反选/尾随/；内置排除 .git, node_modules, target, dist, .venv, __pycache__）
}
```

  - `LocalFs::walk_blocking` 覆盖：`ignore::WalkBuilder`（`hidden(false)` 与现 Grep/Glob 参数一致），行为等价今天的本地遍历。
- [ ] Step 1: 失败测试：接口层——临时目录树（含 .gitignore `*.log`、`!keep.log`、node_modules）下默认实现产出行；LocalFs 覆盖实现与默认实现对同一棵“无 gitignore”树产出集合相等；gitignore 树上 LocalFs 产出行与 `ignore` crate 语义一致（`keep.log` 在、`a.log` 不在）。
- [ ] Step 2: 实现 gitignore 简化匹配器（独立 `crates/shannon-tool-interface/src/walk.rs`，纯函数可测）。
- [ ] Step 3: PASS（跑 `-p shannon-tool-interface -p shannon-core`）。Step 4: Commit `feat(interface): provider-aware directory walk`

### Task 10: Grep/Glob provider 化

**Files:**
- Modify: `crates/shannon-tools/src/grep.rs:373-428`（`WalkBuilder`→`fs.walk_blocking`；`search_root.exists()`→`fs.exists_blocking`；`path.is_dir()`→`fs.metadata_blocking`）
- Modify: `crates/shannon-tools/src/file/glob.rs:142,158,194-238`（同上三处：`current_dir` 由入参传入的 base 承担、`base.exists()`→provider、遍历→walk）

**Interfaces:** 无新增公共接口；`GrepTool`/`GlobTool` 内部改走 `self.fs`。

- [ ] Step 1: 先跑并记录既有 grep/glob 测试基线数量。
- [ ] Step 2: 改造（保持排序语义：walk 回调按字典序 push）。新增 1 个测试：`FakeFs`（shannon-core 测试模块已有 fake？复用；否则最小 fake）上的 grep 命中远程语义路径。
- [ ] Step 3: 既有测试全绿（数量不少于基线）+ Commit `feat(tools): grep/glob traverse via FileSystemProvider`

### Task 11: capabilities 门控（PTY / 本地沙箱分支 / Worktree）

**Files:**
- Modify: `crates/shannon-tool-interface/src/providers.rs`（`ExecCaps` + `ProcessProvider::capabilities`）
- Modify: `crates/shannon-tools/src/system.rs:1039-1084`（三个本地分支统一门控：`use_pty`、`sandbox`（legacy DockerSandbox）、`process_sandbox`（bwrap/Seatbelt）——后两者持有本地 provider，会遮蔽注入的远程世界；`!caps.is_remote` 才可进入，远程一律走 `direct_process`（注入世界）+ 一次性提示行）
- Modify: `crates/shannon-tools/src/worktree.rs`（`execute` 开头：caps.is_remote → 拒绝并提示 "git worktrees are local-only"）

- [ ] Step 1: 失败测试：`FakeProcess(caps: remote)` 的 BashTool `use_pty=true` 输入 → 不进 PTY 分支；`with_process_sandbox` 装配 + remote 世界 → bash 输出**不**含 bwrap/docker 包装痕迹、由注入世界执行（断言 FakeProcess 收到调用）；WorktreeTool remote → 明确错误文案。
- [ ] Step 2: 实现。Step 3: PASS。Step 4: Commit `feat(tools): gate local-only paths on remote worlds`

### Task 12: /rewind 共享 FileHistory

**Files:**
- Modify: `crates/shannon-tools/src/lib.rs`（`ToolRegistrationResult.file_history`，Task 8 已加字段 → 此处填充值）
- Modify: `crates/shannon-tools/src/file_history.rs`（新增 `pub fn restore(&self, path: &Path, id: &str) -> Result<String, FileHistoryError>`：rollback + 经自身 fs 写回）
- Modify: `crates/shannon-ui/src/repl/commands/session.rs:444-471`（`apply_file_rewind`/`run_file_rewind` 增加参数 `history: Option<Arc<Mutex<FileHistoryManager>>>`，来自 Repl 持有的 `ToolRegistrationResult`；None 时回退旧路径）

- [ ] Step 1: 失败测试：`FakeFs` 注入 manager → restore 写回经 fs（fake 收到 write 调用）；session.rs 回退路径行为不变（现有 session 测试全绿）。
- [ ] Step 2: 实现 + REPL 侧 `Repl` 存 `reg_result`。Step 3: PASS。Step 4: Commit `fix(rewind): reuse provider-wired file history manager`

### Task 13: 引擎接线（CLI --target / SHANNON_TARGET / REPL 动态装配）

**Files:**
- Modify: `crates/shannon-cli/src/main.rs`（clap 增 `--target <NAME>`；4 处 `register_default_tools_with_project_dir_ex`（:1232/:1673/:2364/:2482）前解析 `resolve_active`；有目标 → `Assembly::pending_ssh(t)` 在既有 async 块 `connect` 后 `build`，project_dir 传 `t.workspace_dir`；`SHANNON_TARGET` env 在 `resolve_active` 内读）
- Modify: `crates/shannon-ui/src/repl/mod.rs:413-420`（装配换 `shannon_remote::assemble_providers(None)` 的 DynamicWorld providers；`Repl` 存 `dynamic: Option<Arc<DynamicWorld>>`、`world_state`、并把 `world_sandbox` 句柄随 reg_result 保存）
- Modify: `crates/shannon-cli/src/main.rs` 的 clap 结构测试文件（`cli_args_tests.rs`）加 `--target` 解析用例

- [ ] Step 1: 失败测试：`cli_args_tests` 中 `--target build-box` 解析断言；`resolve_active` env 分支（已在 Task 2）。
- [ ] Step 2: 实现。REPL 装配点改动必须保持 `cfg!(test)` 最小初始化路径不受影响。
- [ ] Step 3: `cargo nextest run -p shannon-cli -p shannon-ui` 全绿。Step 4: Commit `feat(cli): --target/SHANNON_TARGET remote assembly`

### Task 14: TUI /remote 命令 + 状态栏

**Files:**
- Create: `crates/shannon-ui/src/repl/commands/remote.rs`
- Modify: `crates/shannon-ui/src/repl/commands/mod.rs`（`repl_only_commands` 加 `remote`；`handle_command` match 加臂，:462 附近）
- Modify: `crates/shannon-ui/src/widgets/status_bar.rs`（pill：`world_state.status()` 非 Local 时渲染 `[⇅ name]` / Degraded 加 `⚠`）
- Modify: `crates/shannon-ui/src/widgets/status_card.rs:60,198`（footer 命令行加 `/remote`）
- Modify: `locales/*.yml` ×10（`commands.remote.*`：dashboard_title/list_status/add_usage/use_usage/connected/disconnected/degraded/test_ok/test_failed/platform/bash_missing/workspace_missing/remove_done/not_found/reconnect_ok）

**Interfaces:**
- 子命令：`/remote`（仪表盘）、`/remote list`、`/remote add ssh <user@host> <name> <workspace_dir>`、`/remote add docker <container> <name> <workspace_dir>`、`/remote use <name>`（async：`dynamic.use_target` → `world_sandbox.set(roots=[workspace_dir], home=health.home)` → REPL `working_directory` 更新）、`/remote test <name>`、`/remote disconnect`、`/remote reconnect`、`/remote remove <name>`（Confirm 一次）。
- 输出走 `repl.chat.add_message(ChatRole::System, t!("commands.remote.xxx"))`（对齐 connect.rs 现有模式）。
- [ ] Step 1: 失败测试：`parse_remote_args` 纯函数用例（每子命令 + 错误参数 → 用法文案 key）；dashboard 渲染函数给定状态输出含目标名。i18n：en.yml 先行，其余 9 语言同键机械翻译。
- [ ] Step 2: 实现 + 状态栏 pill（渲染测试：state 注入 WorldStatus）。Step 3: PASS。Step 4: Commit `feat(tui): /remote command and target status pill`

### Task 15: Desktop Tauri 命令

**Files:**
- Create: `desktop/src/commands_remote.rs`
- Modify: `desktop/src/lib.rs`（mod 声明）、`desktop/src/main.rs:75-115`（invoke_handler 注册）

**Interfaces:**
- 命令（全部 `#[tauri::command]`，重 IO 用 `tokio::task::spawn_blocking`/`tauri::async_runtime`）：
  - `remote_list_targets() -> Vec<RemoteTargetDto>`（读 remotes.toml）
  - `remote_discover_ssh_hosts() -> Vec<SshHostCandidateDto>`
  - `remote_list_docker_containers() -> Vec<ContainerInfoDto>`
  - `remote_add_target(dto) -> Result<(), String>`（validate + save 0600）
  - `remote_remove_target(name) -> Result<(), String>`
  - `remote_set_default_target(Option<String>) -> Result<(), String>`
  - `remote_test_target(name) -> RemoteHealthDto`（ssh/docker health + 指纹可选：`ssh-keyscan -T 3 <host> | ssh-keygen -l -f -`）
- DTO 一律 serde camelCase，对齐 `desktop/src/commands_connections.rs` 现有风格。
- [ ] Step 1: 失败测试（desktop crate 已有测试模式——纯逻辑函数如 DTO↔model 映射、validate 透传）。
- [ ] Step 2: 实现 + 注册。Step 3: `cargo check -p shannon-desktop`。Step 4: Commit `feat(desktop): remote target management commands`

### Task 16: Desktop Remotes 设置页

**Files:**
- Create: `desktop/ui/src/components/settings/RemotesSettings.tsx` + `remotes-settings/`（`SshHostsCard.tsx`、`DockerContainersCard.tsx`、`AddRemoteDialog.tsx`、`types.ts`）
- Modify: `desktop/ui/src/App.tsx`（lazy import + `/settings/remotes` 路由）、`desktop/ui/src/components/Sidebar.tsx:459-469`（`SubNavLink to="/settings/remotes" labelId="nav.remotes"`）
- Modify: `desktop/ui/src/lib/tauri-api.ts`（7 个 invoke 包装）、`desktop/ui/src/types/index.ts`（DTO 类型）
- Modify: `desktop/ui/src/i18n/locales/en.json` + `zh-CN.json`（`nav.remotes`、`settings.remotes.*`：title/description/ssh/docker/add/test/default/remove/empty*/dialog 字段/错误文案，≈40 键）
- Test: `desktop/ui/src/__tests__/RemotesSettings.test.tsx`

**Interfaces:** 组件仅经 `tauri-api` 取数；EmptyState（无目标）/LoadingState/ErrorState/ConfirmDialog（删除）/Modal（Add）/Badge（Connected|Degraded|未测）/sonner toast；`data-testid="remotes-*"`。参照 `MobilePairingCard.tsx` 全套模式。
- [ ] Step 1: 失败测试：渲染 EmptyState（mock api 返回 []）、列表渲染、Add 对话框提交调用 `remoteAddTarget`、删除走 ConfirmDialog、test 按钮显示 health badge。
- [ ] Step 2: 实现组件 + 路由 + 侧边栏 + i18n 双语。
- [ ] Step 3: `pnpm -C desktop/ui test` 绿 + `pnpm -C desktop/ui build` 绿。Step 4: Commit `feat(desktop-ui): remotes settings page`

### Task 17: 文档

**Files:**
- Modify: `CLAUDE.md`（Architecture 表加 `shannon-remote` 行；Known Gaps 增加“远程目标（SSH/Docker）”条目指向设计文档）
- Modify: `README.md`（Features 增小节 + `/remote` 命令行）
- Modify: `docs/plans/2026-09-04-remote-connections-design.md`（状态改“已实现”，附已知限制清单：PTY/LSP/MCP stdio/worktree/REPL 层本地读取/远程 Windows/alpine bash）

- [ ] Step 1: 更新三处。Step 2: Commit `docs: remote targets feature`

### Task 18: 全量验证

#### 集成测试环境变量（追加于实现期）

`#[ignore]` 集成测试通过环境变量指向任意可达 sshd（默认 localhost:22）：

| 变量 | 含义 | 示例（容器化 sshd） |
|---|---|---|
| `SHANNON_TEST_SSH_HOST` | 目标主机 | `localhost` |
| `SHANNON_TEST_SSH_PORT` | 端口 | `2222` |
| `SHANNON_TEST_SSH_USER` | 用户 | `ed` |
| `SHANNON_TEST_SSH_WORKSPACE` | 远程工作区 | `/config` |

运行方式：`cargo test -p shannon-remote --lib -- --ignored`（需 ssh-agent 已加载密钥）。
Docker 集成测试需要本地守护进程与名为 `shannon-it` 的运行中容器（alpine 即可）。

#### 真实环境测试结论（2026-09-04 追加）

对本机容器化 sshd + alpine 容器执行全部集成测试：**4/4 通过**。执行前发现并修复 4 个单测无法暴露的真实缺陷：

1. **SshRuntime 缺失驱动线程**——`current_thread` Runtime 无人 block_on，所有 spawn 的任务（含第一次 connect）永不执行，整个远程功能上线即挂死；
2. **target.port 未传入 SessionBuilder**——自定义端口的主机全部拨向 22；
3. **DockerExecProcess 的 argv 重写丢弃 stdin_data**——容器内所有文件写入均为 0 字节；
4. **健康探针字段错位**——`uname` 尾部换行挤乱 NUL 分隔字段，platform/home 解析出错。

另含 SFTP 握手 15s 超时 + 子进程 stderr 诊断（防呆化：配置错误报错而非挂起）。

- [ ] `cargo fmt --check && cargo clippy --workspace -- -D warnings`
- [ ] `just test`（全绿；`#[ignore]` 集成测试若本机有 sshd/docker 则手动跑一轮并在 PR 描述记录结果）
- [ ] `pnpm -C desktop/ui test && pnpm -C desktop/ui build`
- [ ] 桌面 UI `/settings/remotes` 截图 → 视觉评审（judge）
- [ ] 手动旅程冒烟：`shannon --target <localhost 别名> -p "run ls"`；REPL `/remote use` → 状态栏徽标 → Read/Edit 远端文件 → `/rewind` → `/remote disconnect`
- [ ] Commit（如有修复）+ 分支整理
