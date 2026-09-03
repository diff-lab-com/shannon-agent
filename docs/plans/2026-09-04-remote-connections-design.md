# 远程目标连接（SSH / Docker 容器）设计方案

- 日期：2026-09-04（v3，纳入两轮设计评审意见）
- 分支：`feat/remote-machine-connections`（基于 `dev`）
- 状态：评审通过（第二轮 5 项意见已全部吸收，见 §11；第一轮 12 项亦全数关闭）
- 依据：[远程连接竞品调研报告](../research/remote-connections-competitive-research.md)

---

## 1. 背景与问题

Shannon 当前的所有工具执行（Bash / Read / Write / Edit / Grep / Glob / Notebook…）只能作用于**本地机器**。竞品（VS Code Remote-SSH、Zed、JetBrains Gateway、Cursor、Warp、DevPod/Coder/Codespaces）均已支持"连接远程机器（SSH 主机 / Docker 容器）并在其上运行全部开发工具"，这是 AI coding agent 的行业标配能力。用户的典型诉求：

- 代码在远程开发机 / 云主机 / GPU 服务器上，本地只跑 UI；
- 依赖环境在 Docker 容器里，希望 agent 直接"进入"容器工作；
- 团队共享的构建机、CI runner 等无法本地复现的环境。

## 2. 现状盘点（Gap 分析）

| 能力 | 竞品水准 | Shannon 现状 | 结论 |
|---|---|---|---|
| SSH 远程执行 | 全部主流产品 | **无任何 SSH 客户端代码**（Cargo.lock 无 russh/ssh2/libssh2） | 缺失 |
| Docker 容器目标 | Dev Containers attach / devcontainer.json / Codex 容器沙箱 | 仅有**本地** `DockerSandbox`（`docker run` argv 包装，用于命令沙箱化，非工作目标）；无 `docker exec` 目标、无 devcontainer 支持 | 缺失 |
| 主机/目标管理 UI | Remote Explorer、Gateway 启动器、Termius Hosts | 无 | 缺失 |
| `~/.ssh/config` 复用 | 行业统一做法 | 无 | 缺失 |
| 首连信任（TOFU） | known_hosts + 首连提示 | 无 | 缺失 |
| 全局远程指示器 | VS Code `><`、Zed/Warp 状态徽标 | 无 | 缺失 |
| 目标一等实体模型 | DevPod/Coder/Codespaces workspace | 配置中无 host/target 概念 | 缺失 |
| **执行世界抽象** | — | ✅ `ProcessProvider`/`FileSystemProvider` trait（`shannon-tool-interface/src/providers.rs`）+ 组装接缝 | **已有的决定性优势** |

**结论：功能层面完全不支持，架构层面已预留标准扩展点。** 文件工具、grep 内容读取、LSP 工具、FileHistory（`with_fs`）均已走 provider 接缝；但目录**遍历**（Grep/Glob 的 `ignore::WalkBuilder`）、**路径校验**（`PathSandbox` 以本地 project_dir 为根）与 **REPL 会话层**（/rewind、@ 附件、diff 视图）仍是本地硬编码——这三处是本设计必须一并解决的（§6.4–§6.6），否则"远程世界"只对恰好路径重合的场景成立。

## 3. 备选方案

### 方案 A：argv 改写（否决）

`SpawnRewrite` 把每个命令改写为 `ssh <host> -- <cmd>` / `docker exec <c> <cmd>`。
工作量最小，但**文件工具仍操作本地盘**——Read/Edit 与 Bash 看到的是两个世界，语义割裂；竞品无此做法。

### 方案 B：Provider 执行世界（推荐，MVP 采用）

新增 `shannon-remote` crate，实现两套新执行世界并注入现有接缝：

- **SSH 世界**：进程 = 系统 `ssh` 客户端（`openssh` crate；Unix 走 ControlMaster 多路复用，Windows 逐命令连接）；文件 = SFTP（**专用 `ssh -s sftp` 子进程**承载 `openssh-sftp-client`，不依赖 mux，Windows 亦可用）。
- **Docker 世界**：进程 = `docker exec`（复用现有 `DockerSandbox` 的 docker CLI 依赖模式）；文件 = `docker exec` 内 `cat/tee/find/stat/mv` 组合。
- **DynamicWorld 可切换装饰器**：同时实现两个 provider trait，内部 `RwLock` 指向当前世界（默认 `LocalFs`/`LocalProcess`），`/remote use` 运行时热切换，无需重建 ToolRegistry。

选择系统 `ssh` 二进制而非内嵌 russh 的理由（竞品佐证）：VS Code Remote-SSH、Zed、Warp、JetBrains 均复用系统 ssh / `~/.ssh/config`——一次性免费获得 IdentityFile、ProxyJump、ssh-agent、FIDO2 key、known_hosts 管理；不重复造密钥管理轮子，且与"密码不落盘"的行业标准一致。

- 优点：全工具覆盖；架构意图（§4.11 注释）的正统实现；依赖极少（两个纯 Rust crate + 系统 ssh/docker）。
- 边界：每命令一条 SSH channel（mux 后延迟低）；PTY 交互式远程命令不支持（§6.7）；远程目标需有 `bash`（`run_shell_captured` 硬编码 bash，健康检查探测）。

### 方案 C：远端 server 二进制（VS Code/Zed 范式，Phase 3 演进）

远端自动安装 `shannon-remote-server` 守护进程，JSON-RPC over SSH stdio。上限最高但工程量数周。**`ExecutionWorld` 抽象保持不变，届时仅替换传输实现。**

**决策：MVP 采用方案 B。**

## 4. 目标实体模型

持久化于 `~/.shannon/remotes.toml`（0600，先例：`~/.shannon/providers.toml`）：

```toml
default_target = "build-box"        # 可选：新会话默认目标

[[targets]]
name = "build-box"
kind = "ssh"
host = "build-box"                  # 直接使用 ~/.ssh/config 的 Host 别名，或 user@host
port = 22                           # 可选；缺省走 ssh config
workspace_dir = "/home/ed/proj"     # **必填**：远程工作区根（沙箱根 + 默认 cwd，见 §6.4）
# 认证不落盘：完全交给系统 ssh（config / agent / known_hosts）。

[[targets]]
name = "ci-runner"
kind = "docker"
container = "shannon-ci"            # 运行中容器名/ID
shell = "bash"                      # 容器内 shell，默认 bash（退化 sh）
workspace_dir = "/workspace"        # **必填**：容器内工作区根
# ssh_target = "build-box"          # 可选：经 SSH 到远端机器执行 docker exec（远程 Docker）
```

活动目标解析优先级：`--target <name>`（CLI）> `SHANNON_TARGET` > REPL `/remote use`（会话内）> `default_target` > 本地。

`~/.ssh/config` 主机**自动发现**为只读候选（`ssh -G <host>` 解析生效配置）；添加为 Shannon 目标时仅引用别名，不复制敏感字段。

## 5. User Journey 设计（对齐竞品三件套：连接管理器 + 主机选择器 + 全局远程指示器）

### 5.1 TUI / CLI（主力旅程）

1. **发现**：`/remote` 仪表盘——SSH 候选（`~/.ssh/config`）+ Docker 运行中容器（`docker ps`）+ 已保存目标及连通状态。
2. **连接**：`/remote use build-box`。健康检查探测：连接建立（BatchMode，10s 超时防 passphrase 挂起）、`uname` 平台、`$HOME`、`bash` 可用性、workspace_dir 存在性。首连信任由 known_hosts + `StrictHostKeyChecking=accept-new`（TOFU）完成；**主机密钥变更时 ssh 直接失败并原样透出错误**（不静默降级）。成功后状态栏出现 `[⇅ build-box]` 目标徽标（对标 VS Code `><`）。
3. **工作**：所有工具调用透明路由到远端；沙箱根 = workspace_dir（§6.4）；Grep/Glob 远程遍历（§6.6）；文件快照照常工作（§6.5，/rewind 已修正为共享 provider 化管理器）。
4. **断连/故障**：传输层错误（exit 255 / channel closed）→ 世界标记为不健康 → **透明重连一次**（新建 Session，状态保留在本地）；重连失败 → 状态栏徽标转为降级态 + 行内提示 `/remote reconnect`；`/remote disconnect` 显式回本地。
5. **Headless**：`shannon --target build-box -p "..."` / `SHANNON_TARGET=build-box shannon ...`。

### 5.2 Desktop（管理面）

新设置页 **Settings → Remotes**（`/settings/remotes`，沿用 ConnectionsSettings 卡片模式）：

- **SSH Hosts Card**：候选列表（`~/.ssh/config` 自动发现 + 手动添加 name/host/port/workspace_dir），每行 Test（延迟/平台/指纹）、设为默认、删除（仅删 Shannon 引用，不动 ssh config）。
- **Docker Containers Card**：本机 `docker ps` 运行中容器列表 + Attach；已保存容器目标及健康状态。
- **Add Remote Dialog**（Modal）：name/kind/host/port/workspace_dir/container 字段，**无任何密码/私钥字段**；提交即写 `remotes.toml`。
- **全局指示**：聊天页头部显示默认目标徽标（会话级路由的深度集成为 Phase 2）。

### 5.3 明确不做（MVP 非目标）

devcontainer.json 声明式容器、远端 server 二进制（方案 C）、密码/口令认证 UI、远程 LSP/MCP stdio 的**特殊处理**（LSP 随世界自然路由到远端，见 §6.7）、远程 git worktree、PTY 交互式远程命令、`/api/remote/*` 网络端点、远程 Windows 目标（cmd.exe 远端 shell 与 `sh -c` 语义不兼容，明示不支持）。

## 6. 架构设计

### 6.1 新 crate `shannon-remote`

```
crates/shannon-remote/
├── src/
│   ├── lib.rs            # 导出 + assemble_providers(target) -> Assembly { providers, world_state }
│   ├── target.rs         # RemoteTarget 模型、remotes.toml 读写、ssh config 发现、解析优先级
│   ├── ssh/
│   │   ├── session.rs    # SshRuntime：专属 tokio runtime 线程 + mux Session 管理 + 重连
│   │   ├── process.rs    # SshProcess: ProcessProvider
│   │   └── fs.rs         # SshFs: FileSystemProvider（专用 `ssh -s sftp` 子进程）
│   ├── docker/
│   │   ├── process.rs    # DockerExecProcess: ProcessProvider（本地 docker exec）
│   │   └── fs.rs         # DockerExecFs: FileSystemProvider
│   └── dynamic.rs        # DynamicWorld: 可热切换装饰器 + WorldState（活动目标/沙箱根共享单元）
# 注：目录遍历的 gitignore 匹配器与 walk 默认实现归 shannon-tool-interface（§6.6），
# Local/Ssh/Docker 三个世界统一继承，shannon-remote 不另设 walk 模块。
└── Cargo.toml            # deps: openssh, openssh-sftp-client, tokio, async-trait, serde, toml
```

### 6.2 关键实现语义

**运行时所有权（评审意见 #4）**：`SshProcess`/`SshFs` 的所有 openssh/sftp IO 对象只在 crate 内部的**专属后台 tokio runtime**（独立线程）上创建与驱动，杜绝跨 runtime 驱动导致的 panic/挂起：

- `run_blocking` → `handle.block_on(...)`；
- `run_async` → `runtime.spawn(..)` + oneshot 回传（调用方 runtime 只等 oneshot）；
- `spawn_piped` → `tokio::io::duplex` 桥（duplex 两半与 runtime 解耦）：专属 runtime 泵 RemoteChild 流 ↔ 调用方 runtime 轮询 duplex 半端；`wait/kill` 经 oneshot 往返。三个入口均有跨 runtime 测试。

**命令组合（评审意见 #5，无手工转义）**：argv 安全完全依赖 openssh crate 的逐参数引号；`cwd`/`env` 组合用固定字面量脚本，不含任何用户数据：

```text
env 'K=V' 'K2=V2' sh -c 'cd "$1" && shift && exec "$@"' sh <cwd> <program> <args...>
```

`shift` 不可省略：`sh -c` 后首参数落到 `$0`（占位 `sh`），`$1`=cwd、`$@` 自 `<program>` 起；缺 `shift` 会把 cwd 当作命令执行（exit 126）。测试断言该脚本以一个 cwd + 一个 program 端到端执行成功。

Docker 更简单——`docker exec` 原生支持：`docker exec -w <cwd> -e K=V -i <container> <program> <args...>`。

**Docker 文件操作（评审意见 #9）**：`list_dir` = `docker exec <c> find <dir> -mindepth 1 -maxdepth 1 -type d -print0` 与 `-type f -print0` 两次调用合并（`-mindepth 1` 必须有，否则目录会列出自身导致 walk 无限递归；NUL 分割规避换行文件名/locale 问题；`len=0` 容忍并列为文档化限制）；`metadata` = `stat -c '%s %Y'`（失败退 len=0，秒级精度）；`rename` = `mv -f`；读写 = `cat` / `tee`（二进制安全，经 stdin/stdout）。M1 首日探针对 debian 与 alpine（busybox stat）双镜像验证。

**SFTP（评审意见 #10）**：专用 `ssh -s sftp` 子进程承载 `SftpSession`（不依赖 mux，Windows 可用）；`rename` 优先 `posix-rename@openssh.com` 扩展（Write/Edit 的 tmp→rename 提交依赖覆盖语义），M1 探针验证，缺失时退化为 unlink+rename（tmp 后缀保证最终一致）。`canonicalize`→realpath；`read_prefix_blocking`→分段读。

**健康与重连（评审意见 #7）**：`SshRuntime` 维护 Session 生命周期；探测通道 + 命令失败模式识别（exit 255 / channel closed）；不健康 → 一次透明重连（新 Session，BatchMode + 10s 超时）→ 仍失败则置 degraded 并向 UI 发状态事件（`WorldState::status: Local|Connected|Degraded`，`tokio::sync::watch` 广播）。

### 6.3 引擎接线（注册点，已按评审修正）

| 注册点 | 位置 | 现状 | 改动 |
|---|---|---|---|
| REPL | `crates/shannon-ui/src/repl/mod.rs:416` | `register_default_tools_with_project_dir_ex`（本地 current_dir） | 换 DynamicWorld 装配 + 共享 WorldState（沙箱根可切换） |
| CLI headless ×4 | `crates/shannon-cli/src/main.rs:1232/1673/2364/2482` | 本地装配 | 解析 `--target`/`SHANNON_TARGET` → 静态世界装配（project_dir = workspace_dir） |
| Desktop 会话 | `desktop/src/commands.rs:318` | `register_default_tools`（无 provider 变体） | 换 DynamicWorld 装配（默认本地；会话级切换 Phase 2） |
| Desktop loopback | `desktop/src/loopback_api.rs:34` | `register_default_tools`（同上） | 同上 |

组装入口 `shannon_remote::assemble_providers(target) -> Assembly`：静态目标 → 对应世界 + project_dir=workspace_dir；None → DynamicWorld(本地)。CLAMP：`register_all_tools` 内部以传入 project_dir 构建 `PathSandbox`（见 §6.4），装配层不改动该函数签名以外的东西。

### 6.4 世界感知的沙箱根（评审 BLOCKER #1）

`PathSandbox` 以 `allowed_roots=[project_dir]` + `strict_mode` 克隆进每个工具，注册后不可变；`validate()` 同时对照**本地** home_dir。远程目标下远程路径必然被拒。修复：

1. `PathSandbox` 内部改持 `Arc<RwLock<RootsConfig>>`（新增 `PathSandbox::shared()` 构造与 `set_roots()`；克隆语义不变——共享句柄），**工具代码零改动**；
2. 装配 REPL 时把共享句柄交给 `WorldState`；`/remote use <t>` 原子地：切换 DynamicWorld 内部世界 + `set_roots([t.workspace_dir])` + 更新 REPL `working_directory` 上下文为 workspace_dir；`/remote disconnect` 反向恢复；
3. home 边界检查改为世界感知：RootsConfig 携带 `home_dir`（本地世界=本地 home，远程世界=远端 `$HOME`，来自健康检查），validate 一并读取；
4. 静态装配（CLI `--target`）直接以 workspace_dir 为 project_dir 构建沙箱，无需切换能力。

### 6.5 /rewind 与共享 FileHistory（评审 MAJOR #3）

REPL 回滚处理器现构建**无 provider 的** `FileHistoryManager::new(cfg)`（`session.rs:452-471`，本地 std::fs 读写），远程会话中会静默读不到快照。修复：`ToolRegistrationResult` 增加 `file_history: Arc<FileHistoryManager>` 字段，REPL 侧 /rewind 复用同一 Arc（provider 化、与工具快照同源），恢复路径全部经 `fs` 世界。

### 6.6 Grep/Glob 的 provider 化遍历（评审 BLOCKER #2）

`GrepTool`/`GlobTool` 的遍历与存在性检查走本地（`ignore::WalkBuilder`、`std::env::current_dir()`、`path.is_dir()`），provider 只接了内容/元数据读取。修复（归属地声明：gitignore 匹配器与 walk 默认实现放 `shannon-tool-interface/src/walk.rs`——trait 默认实现无法引用下游 crate，Local/Ssh/Docker 三个世界统一继承；shannon-remote 不设 walk 模块）：

- `FileSystemProvider` 新增带默认实现的方法 `walk_blocking(root, &mut dyn FnMut(DirEntryInfo) -> bool)`：默认实现基于 `list_dir_blocking` 递归 + 简化 .gitignore 匹配（字面量/`*`/`**`/`!` 反选/目录后缀，内置排除 `.git`/`node_modules`/`target` 等）；**`LocalFs` 覆盖实现**，内部继续用 `ignore::WalkBuilder`（本地行为逐字节不变、性能不回退）；
- `GrepTool`/`GlobTool` 改调 `fs.walk_blocking` 并把 `exists`/`is_dir` 改为 provider 调用；`GlobTool` 中与 `std::env::current_dir()` 的父目录比较（`glob.rs:141-152`）改为与**活动 `RootsConfig` 根**（§6.4 共享单元）比较——`FileSystemProvider` 没有 current_dir 概念，远端世界下"项目根"即 workspace_dir（工具内小改动，行为对本地透明）；
- 遍历并发/排序语义与现实现保持一致（确定顺序）。

### 6.7 PTY 门控、LSP 与其余边界（评审 #6/#8/#11/#12）

- **PTY 与本地沙箱分支门控（第二轮评审 BLOCKER）**：`ProcessProvider` 新增默认方法 `capabilities(&self) -> ExecCaps { is_remote: bool }`（Local=false；Ssh/Docker/DynamicWorld 透传内部世界）。`BashTool::execute` 的分支序为 `use_pty` → `sandbox`（legacy DockerSandbox）→ `process_sandbox`（bwrap/Seatbelt argv 包装）→ `direct_process`（`system.rs:1039-1084`）；其中 `sandbox`/`process_sandbox` 持有的是**本地** provider（`with_process_sandbox` 不经 `with_worlds`，`system.rs:866-899`），在装有 docker/bwrap 的机器上会**遮蔽注入的远程世界**——Bash 静默留在本地执行而文件工具走远端，即"语义割裂"。修复：三个本地分支（`use_pty`、`sandbox`、`process_sandbox`）统一加 `!caps.is_remote` 门控，远程世界一律经 `direct_process`（注入世界）路由；本地行为逐字节不变。M2 验收增加断言：远程 Bash 输出（`uname -s`）与健康检查平台一致，证明 Bash 确实落在目标上。
- **LSP**：8 个 LSP 工具经 `with_worlds` 注册、`spawn_piped` 启动 server——远程世界下 rust-analyzer 等将**在目标上启动**，这是设计决定（保持"完整 workspace 在远端"的一致语义，与竞品一致），非缺陷；server 缺失时透出明确错误。文档明示"远程 LSP 需目标机安装语言服务器"。
- **WorktreeTool**：远程会话中拒绝执行（`std::env::set_current_dir` 为本地副作用），提示仅本地支持。
- **REPL 会话层本地读取**（@ 附件 `at_reference.rs`、diff 视图 `diff_viewer.rs`、状态卡分支、loop_engine 的 git/which）：MVP 保持本地行为，列为已知限制（M6 文档 + /remote 仪表盘提示），Phase 2 逐项迁移。
- **bash 依赖**：`run_shell_captured` 硬编码 bash——健康检查探测 `command -v bash`，缺失时在 /remote use 结果与 Desktop Test 结果中显式告警。

### 6.8 安全

- 不新增密钥/口令存储面：认证完全委托系统 ssh；SSH 连接使用 `BatchMode`（防交互 passphrase 挂起）+ 10s 超时；desktop 表单无密码/私钥字段。
- TOFU：`StrictHostKeyChecking=accept-new`，密钥变更直接失败并透出 ssh 原始错误；Desktop Test 连接可选展示 `ssh-keyscan` + `ssh-keygen -l` 指纹（增强项）。
- 注入面：ssh argv 由 openssh 逐参数引号 + 固定字面量 `sh -c` 脚本（§6.2），docker exec 数组传参，无字符串拼接 shell。
- `remotes.toml` 0600；删除目标只移除 Shannon 引用。

## 7. UI/交互设计

### 7.1 Desktop（React）

```
/settings/remotes            → RemotesSettings.tsx
  ├─ DefaultTargetCard       # 当前默认目标徽标 + 清除
  ├─ SshHostsCard            # ~/.ssh/config 发现 + 已保存目标（Test/设默认/删除）
  ├─ DockerContainersCard    # docker ps 列表 + Attach + 已保存容器目标
  └─ AddRemoteDialog         # Modal 表单（含必填 workspace_dir）
```

- 组件全部使用设计系统原语：Card / Badge / EmptyState / LoadingState / ErrorState / Modal / ConfirmDialog / Icon(Material Symbols) / sonner toast；`data-testid` 齐备（沿用 MobilePairingCard 模式）。
- Tauri 命令（新模块 `desktop/src/commands_remote.rs`）：`remote_list_targets`、`remote_discover_ssh_hosts`、`remote_list_docker_containers`、`remote_add_target`、`remote_remove_target`、`remote_test_target`、`remote_set_default_target`；前端手写 `tauri-api.ts` 包装 + `types/index.ts` 类型（仓库现行模式）。
- i18n：`en.json` + `zh-CN.json` 同步新增 `settings.remotes.*` / `nav.remotes`。

### 7.2 TUI

- `/remote`（无参）仪表盘：目标列表（名称/类型/主机/状态徽标）+ 候选提示；子命令 `use <name>`、`add ssh <user@host> <name> <workspace_dir>`、`add docker <container> <name> <workspace_dir>`、`test <name>`、`disconnect`、`reconnect`、`remove <name>`。
- 状态栏目标 pill：`[⇅ build-box]`（Connected）/ `[⇅⚠ build-box]`（Degraded）；`status_card` 首屏命令提示追加 `/remote`。
- i18n：`commands.remote.*` 全部 10 个 `locales/*.yml`。

## 8. 测试策略

- **单元**（每文件 `#[cfg(test)]`）：target 模型 TOML 往返、解析优先级、ssh config 发现解析（fixture）、docker/ssh 命令组合（含 cwd/env/无注入用例 + `sh -c 'cd "$1" && shift && exec "$@"'` 脚本以一个 cwd + 一个 program 端到端执行成功）、PathSandbox 共享根切换、DynamicWorld 切换委派与 watch 状态、capabilities 门控（use_pty/sandbox/process_sandbox 三分支 + Worktree）、walk 默认实现与 LocalFs 覆盖等价性。
- **跨 runtime**：SshProcess 三入口的 runtime 桥接（用内存 mock channel 驱动）。
- **集成（ignored，需环境，CI 跳过）**：对本机 sshd 的 SshProcess/SshFs 全 trait 往返（含 posix-rename 覆盖语义探针）；对 docker 的 DockerExec 世界往返（debian + alpine 双镜像）。
- **前端**：RemotesSettings 组件测试（vitest，mock tauri-api）。
- **验证门槛**：`just dev`（fmt + clippy -D warnings + nextest 全绿）；桌面 UI 页面截图走视觉评审。

## 9. 里程碑（已按评审扩充工具层工作量）

| 阶段 | 内容 | 验收 |
|---|---|---|
| M1 | `shannon-remote` crate：target 模型/持久化/发现 + SshWorld + DockerWorld + **首日探针**（openssh 系编译兼容、posix-rename、busybox stat、list 语义）+ localhost 集成测试（ignored） | 单测绿 + 探针结论落档 |
| M2 | 引擎接线：DynamicWorld + 4 注册点 + `--target`/`SHANNON_TARGET` + PathSandbox 共享根 + home 世界感知 + capabilities() 门控（PTY/sandbox/process_sandbox/Worktree） | headless 对本机 sshd 跑通；**断言远程 Bash `uname` 输出=健康检查平台**（防本地沙箱分支遮蔽） |
| M3 | 工具层正确性：FileSystemProvider::walk 默认实现（tool-interface）+ LocalFs 覆盖 + Grep/Glob provider 化 + /rewind 共享 FileHistory | 远程 Grep/Glob/rewind 行为正确 |
| M4 | TUI `/remote` + 状态栏 pill + i18n（10 locale） | REPL 手动旅程 |
| M5 | Desktop `commands_remote.rs` + Remotes 设置页 + i18n | 页面测试 + 视觉评审 |
| M6 | 文档（CLAUDE.md 差距表/README/已知限制）+ `just dev` 全绿 | 提交 |

## 10. 风险与缓解

| 风险 | 缓解 |
|---|---|
| `openssh`/`openssh-sftp-client` 与 workspace（edition 2024、rust 1.85+）兼容性 | M1 首日探针；不兼容则降级 russh（世界接口不变，隔离在 ssh/ 模块内） |
| SFTP rename 覆盖语义 / busybox stat 差异 | M1 首日探针双镜像；posix-rename 缺失时 unlink+rename 兜底 |
| Windows 无 ControlMaster | 进程世界逐命令连接（慢但可用）；文件世界走专用 sftp 子进程（不依赖 mux）；远程 **Windows 目标**整体不支持（明示） |
| 会话中切换目标导致进行中调用世界不一致 | DynamicWorld 切换取 RwLock 读快照，单次工具调用内一致；切换发生在命令间隙 |
| 无 bash 的最小容器（alpine sh only） | 健康检查显式告警；文档要求 bash |
| ssh passphrase / 交互提示挂起 | BatchMode + 连接超时；文档要求 agent/免交互密钥 |

## 11. 评审意见吸收记录（v1 → v2）

| # | 级别 | 意见 | 处理 |
|---|---|---|---|
| 1 | BLOCKER | 无远程工作区根，沙箱拒远程路径 | §4 workspace_dir 必填 + §6.4 PathSandbox 共享根/home 世界感知 |
| 2 | BLOCKER | Grep/Glob 本地遍历 | §6.6 provider 化 walk（LocalFs 覆盖保本地语义） |
| 3 | MAJOR | /rewind 用无 provider 管理器 | §6.5 ToolRegistrationResult 传递共享 Arc |
| 4 | MAJOR | 内部 runtime 须覆盖 run_async/spawn_piped | §6.2 专属 runtime + duplex 桥 + 三入口测试 |
| 5 | MAJOR | `cd '<escaped>' &&` 转义不健全 | §6.2 固定字面量 `sh -c 'cd "$1" && exec "$@"'`；docker 用 `-w/-e` 原生参数 |
| 6 | MAJOR | LSP 实际经 provider 会路由远端 | §6.7 明确为设计决定并文档化 |
| 7 | MAJOR | 无重连/不健康策略 | §6.2 健康监测 + 透明重连 + Degraded 状态 + /remote reconnect |
| 8 | MINOR | PTY 门控机制未指定 | §6.7 capabilities() 默认方法 |
| 9 | MINOR | ls/stat 解析脆弱 | §6.2 find 双调用 + stat -c + 双镜像探针 |
| 10 | MINOR | SFTP rename 覆盖 / Windows 无 FS | §6.2 posix-rename 探针 + 专用 sftp 子进程（不依赖 mux）+ 远程 Windows 明示不支持 |
| 11 | MINOR | 额外绕过路径（worktree/REPL 层） | §6.7 worktree 拒绝 + REPL 层列已知限制（M6） |
| 12 | MINOR | TOFU 文案夸大 / bash 硬编码 | §5.1/§6.8 文案修正 + §6.7 bash 健康检查探测 |

**第二轮（v2 → v3）：**

| # | 级别 | 意见 | 处理 |
|---|---|---|---|
| 13 | BLOCKER | `process_sandbox`/`sandbox` 本地分支遮蔽注入世界，Bash 静默留在本地 | §6.7 三分支统一 `!caps.is_remote` 门控 + M2 验收平台断言 |
| 14 | MAJOR | `sh -c` 组合脚本缺 `shift`（cwd 被当命令执行） | §6.2 加 `shift &&` + §8 端到端脚本测试 |
| 15 | MINOR | docker list_dir 缺 `-mindepth 1`（walk 无限递归）、行分割仍惧换行 | §6.2 `-mindepth 1` + `-print0` NUL 分割 |
| 16 | MINOR | walk 归属地冲突（trait 默认实现不能引用 shannon-remote） | §6.1/§6.6 归属 shannon-tool-interface，删除 shannon-remote walk 模块 |
| 17 | MINOR | Glob 的 current_dir 比较未指明替代 | §6.6 与活动 RootsConfig 根（workspace_dir）比较 |
