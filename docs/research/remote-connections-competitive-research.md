# 远程连接（SSH / Docker）竞品调研报告

- 日期：2026-09-04
- 任务：调研竞品如何设计"连接远程机器（SSH 主机 / Docker 容器）运行 AI coding agent 会话"的能力，为 Shannon 新增远程目标（remote targets）功能提供设计依据
- 输入：VS Code Remote、Claude Code、OpenAI Codex、Cursor、Windsurf、Zed、JetBrains Gateway、DevPod / Coder / Daytona / Codespaces / Gitpod、Warp / Termius 的官方文档、工程博客与社区反馈（截至 2026-09-04）
- 结论速览：**行业已收敛出一条成熟范式——「本地瘦客户端 + 首连自动安装的远程 server 进程 + 复用 `~/.ssh/config` + 显式的连接管理 UI + 状态栏远程指示器」。** 工具执行全部路由到远程（整个 workspace 在远端），UI/密钥留在本地；密码一律不落盘，走 ssh-agent / key。对 Shannon 最值得借鉴的是：①把"远程目标"做成一等实体（DevPod / Coder / Codespaces 的 workspace 模式）；②首连信任提示（host key TOFU）；③`devcontainer.json` 兼容作为容器目标的定义格式；④会话可迁移（Claude Code 的 teleport）。

---

## 1. 竞品横评总表

| 产品 | 远程目标 | 设置 UX（零到连通） | 工具/agent 执行模型 | 主机管理 UI | 认证 |
|---|---|---|---|---|---|
| **VS Code Remote-SSH** | SSH 主机（VM/物理机/容器内 sshd） | 装扩展 → `~/.ssh/config` 添加 Host（命令面板 "Add New SSH Host"）→ 命令面板/Remote Explorer 选主机 → 自动装 VS Code Server | 全部 workspace 操作（文件、终端、扩展、调试）在远端；本地只跑 UI 类扩展 | Remote Explorer 侧栏（SSH Targets 列表）+ 状态栏 `><` 远程指示器 + 命令面板 | 复用 `~/.ssh/config` 全套（IdentityFile/ProxyJump）；不保存密码；不支持 PuTTY |
| **VS Code Dev Containers** | Docker 容器（新建 or attach 已运行） | 项目内 `.devcontainer/devcontainer.json` → Rebuild Container；或 "Attach to Running Container..." 选容器 | workspace 挂载进容器，扩展/终端在容器内运行 | Docker 容器视图（Containers 面板可 attach） | 复用 Docker daemon 凭证；容器内认证用挂载卷 + 环境变量 |
| **VS Code Remote Tunnels** | 任意可装 VS Code 的机器（反向出站连接） | 远端 `code tunnel` 或 Account 菜单开启 → 本地 vscode.dev / 命令面板 "Connect to Tunnel" 选机器 | 同 Remote-SSH（server 在远端） | 命令面板隧道列表（按 GitHub/MS 账号罗列） | Microsoft/GitHub 账号 OAuth；经 MS 中继，无需开入站端口 |
| **Claude Code** | 本地 + devcontainer + 云端沙箱 VM（无官方 SSH 远程） | devcontainer：`devcontainer.json` 加官方 feature → rebuild → 容器内 `claude` 登录；云端：网页发起，会话跑在托管 VM | 容器/VM 内全套 shell+文件工具；本地模式可开内置 bash 沙箱 | 无连接管理器；云端会话列表在网页/App；`claude --teleport` 拉回本地 | OAuth 登录存 `~/.claude`（卷持久化）；建议不挂载 `~/.ssh`；容器内禁 root 跑 bypass |
| **OpenAI Codex** | 本地沙箱 + Docker 容器 + 云端容器（每任务一个） | CLI 零配置即用（内置 Landlock/Seatbelt 沙箱）；容器方案 = 官方文档"在容器里跑 + 关沙箱"；云端选 repo 配 setup script | CLI 沙箱三级（read-only / workspace-write / danger-full-access）；云端 setup 阶段有网、执行阶段默认断网 | 无主机管理；云端环境配置在 ChatGPT UI/IDE 扩展里 | ChatGPT 登录；容器内常见 `--dangerously-bypass-approvals-and-sandbox`（YOLO in a box） |
| **Cursor** | SSH 主机 + 云端 VM（Background Agents）+ 自托管 worker 机器 | 同 VS Code（fork 自带 Remote-SSH）→ 自动装 cursor-server；自托管：装 Cursor CLI → `agent login` → `agent worker start` | SSH 窗口内 agent 跑在远端 workspace；Background Agent 跑在 Cursor 云 VM；自托管 worker 经出站长连接接单 | 无独立连接管理器（沿用 VS Code Remote Explorer）；自托管有 My Machines / Team Pools 概念 | `~/.ssh/config` 复用；自托管走 API key/service account，仅出站 HTTPS |
| **Windsurf** | SSH 主机 | 装 "Open Remote - SSH" 扩展 → 命令面板 "Remote-SSH: Connect to SSH Host" → 自动装远端 server | Cascade（agent）跑在远端 workspace（社区确认可用，部分插件有坑） | 沿用 VS Code 式 Remote Explorer | `~/.ssh/config` 复用（open-remote-ssh 实现） |
| **Zed** | SSH 主机（+WSL） | `ctrl-cmd-shift-o` 打开 Remote Projects 对话框 → "Connect New Server" 输入 ssh 命令 → 自动下载 Rust 版 remote server → 选路径 | 语言服务器/搜索/任务/终端全在远端；本地跑 UI、Tree-sitter、LLM 通信；Agent Panel 远程可用 | Remote Projects 对话框（最近项目 + 新建连接）；CLI `zed ssh://host/path` | 用系统 `ssh` 二进制（继承 `~/.ssh/config` 与 agent）；设置文件禁止存密码；SSH ControlMaster 多路复用 |
| **JetBrains Gateway** | SSH 主机 + Dev Container | Gateway/欢迎屏 → Remote Development → 配 SSH → 远端下载 headless 后端 IDE → 打开 JetBrains Client | 整个 IDE 后端在远端（索引/运行/调试），瘦客户端只渲染 | Gateway 启动器：最近主机 + 最近项目列表；欢迎屏 Dev Containers 区 | `~/.ssh/config` 复用；密码/密钥均可选存于 IDE 凭证库 |
| **DevPod** | 声明式 workspace：provider = docker / ssh / kubernetes / cloud | `devpod up <repo>` 按 provider 建 workspace（devcontainer 标准）| workspace 内跑 devcontainer；agent 进容器做端口/凭证转发；IDE 经 workspace 的 SSH server 接入 | workspace 是一等实体：list/stop/delete；provider 配置文件化（yaml） | 前置 provider 认证（如 ssh provider 用你的 key）；agent 做凭证转发 |
| **Coder** | 平台化 workspace（Terraform 模板定义，跑在哪都行） | 装 coderd → 写模板 → `coder create` → `coder config-ssh` 把每个 workspace 写成 `~/.ssh/config` Host 项 → 任意 SSH 客户端/IDE 直连 | workspace 内完整开发环境；官方支持把 Claude Code 等 AI agent 跑进 workspace | Web 控制台：workspace 生命周期（create/start/stop/delete）+ 状态；VS Code 扩展列 workspace | Coder 为每用户生成 SSH keypair；模板可注入云凭证 |
| **GitHub Codespaces** | 云端容器（每 codespace 一台托管 VM） | repo 里放 `devcontainer.json`（可选）→ GitHub UI/CLI 建 codespace → 浏览器或桌面 IDE 直开 | 容器内完整环境；生命周期钩子 onCreate→postCreate→postStart；prebuild 缓存 | github.com/codespaces 列表页 + `gh cs` CLI：list/stop/rebuild/delete | GitHub 凭证自动注入（git 认证）、secrets、转发端口需授权 |
| **Gitpod（Ona）** | 云端环境 + Flex 自托管环境（自有 AWS/GCP/裸机） | repo URL 即建环境（Classic）；Flex 在自有主机装 agent 后注册为环境 | 环境内完整 devcontainer；桌面 IDE 经 SSH/Gateway 接入 | 环境列表 + 生命周期管理；`gp` CLI | GitHub/GitLab OAuth；SSH key 绑定账号 |
| **Daytona** | Sandbox（AI 代码执行基础设施方向） | SDK/CLI `daytona.create()` / `daytona create --name` → `sandbox.process.exec()` | sandbox 即实体：快照、secrets、进程执行 API | CLI + 控制台管理 sandbox 生命周期 | API token；secrets 按创建注入 |
| **Warp** | SSH 会话（Warpify）+ 云端 agent | 终端里输 `ssh ...`，Warp 检测后提示"安装 SSH extension"→ 远端 `~/.warp*/remote-server` | 终端原样透传；装了扩展后：文件树/编辑/索引/agent diff 在远端；Agent CLI 不装远端组件，本地 agent 经 pty 驱动远端 | 无主机管理器（依赖 `~/.ssh/config`）；有 `ssh_hosts_denylist` | 复用 `~/.ssh/config` 与 ssh-agent；扩展只写 home、不开端口 |
| **Termius** | SSH 主机（管理型客户端，非 agent） | 手动/批量建 Host → 分组打标签 → 点连 | 不涉及 agent；提供 SFTP/端口转发/代码片段 | 主机管理黄金标准：Hosts/Groups/Tags/Snippets/Port Forwarding 规则/Host Chains，端到端加密云同步，known_hosts 导入 | 密钥托管（加密同步）+ ssh-agent 转发；可导入 known_hosts |

---

## 2. 分产品详述

### 2.1 VS Code Remote-SSH / Dev Containers / Remote Tunnels（原型范本）

**功能名**：Remote - SSH 扩展（`ms-vscode-remote.remote-ssh`）、Dev Containers 扩展、Remote Tunnels（`ms-vscode.remote-server`）。

**用户旅程（SSH，零到连通）**：
1. 前置：本地有 OpenSSH 客户端，远端跑 sshd（Ubuntu 16.04+ / RHEL / Windows 10+ / macOS 10.14+，glibc ≥ 2.17，1GB 内存起步，需出站 443 下载 server）。
2. 官方建议先在终端验证 `ssh user@host` 可通，再进 VS Code。
3. 配置主机：命令面板 `Remote-SSH: Add New SSH Host...`（可粘整条 `ssh user@host -p port`，扩展自动生成 Host/HostName/User/IdentityFile 条目写进 `~/.ssh/config`），或直接编辑 config（`remote.SSH.configFile` 可换文件）。支持 ProxyJump 堡垒机、LocalForward 持久端口转发。
4. 连接：命令面板 `Remote-SSH: Connect to Host...` 或 Remote Explorer 视图点主机 → 新窗口。
5. 首连：扩展检测远端平台（识别失败会让你手选，结果存 `remote.SSH.remotePlatform`）→ 下载并启动 **VS Code Server**（与远端已装的 VS Code 无关）→ 打开文件夹即工作。断开用 `File > Close Remote Connection`。

**UI 面**：
- **Remote Explorer** 侧栏：SSH Targets 树，展开主机可"新窗口打开"或"重开上次文件夹"。
- **状态栏远程指示器**（`><` 图标 + 主机名）：点击出现全部远程命令；这是"当前在远端"的全局可视化。
- 扩展视图按"远端已装 / 本地已装"分组，一键 "Install Local Extensions in SSH: {host}" 批量装到远端。

**执行模型**：完整 workspace 在远端——文件、终端、调试、绝大多数扩展都在远端跑；只有主题/按键等 UI 扩展在本地。扩展作者可用 `remote.extensionKind` 强制 ui/workspace 归属。设置优先级 Workspace > Remote > User（"Preferences: Open Remote Settings" 编辑远端层）。

**认证**：完全复用 `~/.ssh/config`；不保存密码/令牌；Windows 不支持 PuTTY；推荐 key-based。

**限制**：Alpine/非 glibc 不支持；passphrase 保护的 key 下 VS Code 内 git pull 可能挂起；本地代理设置不会带到远端；纯 SFTP/FTP 主机不可用。

**Dev Containers**：项目里 `.devcontainer/devcontainer.json` 声明 image/Dockerfile/docker-compose + features + `postCreateCommand` 等生命周期钩子 → "Rebuild Container" 构建并进入；**也支持 attach**：命令面板 "Dev Containers: Attach to Running Container..." 选运行中容器（可绑定 workspace 让下次自动重连）。devcontainer 规范（containers.dev）已成行业通用格式，Codespaces / JetBrains / Cursor / DevPod 全部兼容。

**Remote Tunnels**：反向连接方案——远端跑 `code tunnel`（或 VS Code Account 菜单 "Turn on Remote Tunnel Access"），经微软中继（类 TURN）出站长连接；本地用 vscode.dev 或 "Remote Tunnels: Connect to Tunnel" 选机器；用 GitHub/Microsoft 账号鉴权，**无需 SSH、无需开入站端口/VPN**，适合 NAT 后和 headless 机器。

来源：<https://code.visualstudio.com/docs/remote/ssh> · <https://code.visualstudio.com/docs/remote/ssh-tutorial> · <https://code.visualstudio.com/docs/remote/vscode-server> · <https://code.visualstudio.com/docs/remote/tunnels> · <https://code.visualstudio.com/docs/devcontainers/containers> · <https://code.visualstudio.com/blogs/2019/10/03/remote-ssh-tips-and-tricks>

### 2.2 Claude Code（devcontainer / 云端沙箱 / 远程故事）

**devcontainer 支持**：官方提供参考实现（anthropics/claude-code 仓库的 `.devcontainer/`：devcontainer.json + Dockerfile + `init-firewall.sh`），并发布 devcontainer **feature** `ghcr.io/anthropics/devcontainer-features/claude-code:1.0` 一行接入。要点：
- 参考容器带**出站防火墙白名单**（iptables），需要 `NET_ADMIN`/`NET_RAW` capability；防火墙可选，可替换为自己的网络管控。
- 认证持久化：named volume 挂到 `~/.claude` + 设 `CLAUDE_CONFIG_DIR` 指向同路径；OAuth 与 per-project trust 分别存不同文件。
- 策略：Dockerfile 把 `managed-settings.json` 拷到 `/etc/claude-code/`（Linux 最高优先级）；官方提醒"仓库内 Dockerfile 可被有写权限的人改掉"，要强制策略需服务端下发或 MDM。
- `--dangerously-skip-permissions` 以 root 运行会被拒绝，所以 `remoteUser` 必须非 root；可用 `permissions.disableBypassPermissionsMode: "disable"` 组织级封禁。
- 官方明确限制：**容器不是绝对免疫**——bypass 模式下恶意项目仍可外泄容器内可达的一切（包括 `~/.claude` 里的凭证）；建议不要挂载宿主机 `~/.ssh`/云凭证。

**沙箱谱系**（`sandbox-environments` 文档）：内置 bash 沙箱（文件系统 + 网络隔离，自动放行安全操作）→ 自定义容器 → dev container → 云端托管 VM。云端（Claude Code on the web）：每个会话一个隔离的 Anthropic 托管 VM + 网络代理默认出站白名单；用户在网页/App 实时观察并可中途纠偏。

**会话迁移（teleport）**：网页会话点 "Open in CLI" 得到 `claude --teleport session_XXXX`，在本地 repo checkout 里执行即把云端会话拉回本地继续（自动切分支、stash 本地改动）；**单向** cloud→local。社区已有 issue 请求"通过 SSH teleport 到远程机器"（#14666）——说明官方目前**没有** SSH 远程执行故事，远程 = devcontainer（自建）或云端沙箱（托管）。

**企业侧**：Claude Platform 另有 "self-hosted sandboxes"——企业可在自己的 AWS 账号里跑 agent 会话，与云端沙箱互达（经 tunnel 访问私有 MCP）。

来源：<https://code.claude.com/docs/en/devcontainer> · <https://code.claude.com/docs/en/sandbox-environments> · <https://code.claude.com/docs/en/sandboxing> · <https://www.anthropic.com/engineering/claude-code-sandboxing> · <https://claude.com/blog/claude-code-on-the-web> · <https://platform.claude.com/docs/en/managed-agents/self-hosted-sandboxes> · <https://github.com/anthropics/claude-code/issues/14666>

### 2.3 OpenAI Codex（CLI 沙箱 + 云端容器）

**CLI 沙箱**（工具调用级，不是把自己整个沙箱起来）：主进程不设防，对每条执行的命令按平台套沙箱——Linux 用 Landlock（文件系统）+ seccomp（系统调用/断网），macOS 用 Seatbelt，Windows 用受限 token。三档 `--sandbox`：`read-only` / `workspace-write`（默认，可写 workspace+`/tmp`，默认断网）/ `danger-full-access`（全放行）；配合 `--ask-for-approval` 系列审批策略。配置在 `config.toml`。

**容器方案**（官方文档认可的模式）：把 Docker 当安全边界——容器内挂一个 workspace，跑 `codex --dangerously-bypass-approvals-and-sandbox`（"YOLO in a box"，Docker Docs 有官方指南）。已知坑：容器内默认连不上宿主 Docker daemon（unix socket 不可见），挂 `/var/run/docker.sock` 又是新的安全权衡；Linux 下 bubblewrap 缺失会报 "retry without sandbox"。

**Codex cloud**：每个任务一个一次性容器；流程 = 建容器 → checkout 指定 branch/SHA → 跑用户的 **setup script**（此阶段有互联网，结果被缓存复用，缓存命中后直接 checkout 任务分支）→ agent 执行阶段默认**断网**（可配置 agent internet access）。基础镜像 `openai/codex-universal`（可本地拉取调试自定义环境）；secrets 注入式提供；用户在 ChatGPT 网页/IDE 扩展里配置环境、发起任务、看 diff、开 PR。

来源：<https://learn.chatgpt.com/docs/environments/cloud-environment> · <https://learn.chatgpt.com/docs/cloud> · <https://github.com/openai/codex> · <https://docs.docker.com/ai/sandboxes/agents/codex/> · <https://github.com/openai/codex-universal> · <https://www.openai.com/index/building-codex-windows-sandbox/>

### 2.4 Cursor（SSH 远程 + Background Agents + 自托管机器）

**SSH 远程**：作为 VS Code fork 原生支持 Remote-SSH（Anysphere 维护的分支扩展），流程与 VS Code 一致（命令面板 Connect to Host → 自动部署 cursor-server，部署方式是经 SSH 传 base64 脚本 bash 解码执行）；复用 `~/.ssh/config`；首连有 host fingerprint 确认。agent 在 SSH 远程窗口里可正常工作（跑在远端 workspace 上）。

**关键缺口**：SSH 远程窗口里**用不了 Background/Cloud Agents**（它们绑定本地桌面客户端连接），论坛有 "durable agent runtime on Remote SSH" 的长贴请求——这正是"agent 与连接解耦"的需求信号。常见故障：更新后连接挂、agent 在大会话开始时无响应。

**自托管机器（Self-Hosted Machines）**：把自家机器注册成 Cloud Agent 的**执行 worker**——本地装 Cursor CLI → `agent login` → `agent worker start`，worker 向 Cursor 后端开**长连出站 HTTPS**（无需入站端口/公网 IP/VPN），Cursor 把工具调用推过来执行（文件编辑、终端、computer-use、本地 MCP）。两种形态：My Machines（个人机器，多 agent 共享）/ Team Pools（企业池，service account，一机一 agent，controller 扩缩容）。规模上限 200 worker/用户、1000/团队。**"注册机器为 worker" 是与 SSH 并列的另一条远程路径**，值得 Shannon 关注。

来源：<https://cursor.com/docs/cloud-agent/self-hosted> · <https://forum.cursor.com/t/does-cursor-support-remote-ssh/7620> · <https://forum.cursor.com/t/durable-agent-runtime-on-remote-ssh-cloud-workstations/159513> · <https://docs.rc.fas.harvard.edu/kb/cursor-remote-development-via-ssh-and-tunnel/>

### 2.5 Windsurf

官方商城提供 **"Open Remote - SSH"** 扩展（jeanp413 的 open-remote-ssh，即开源 OSS 版 Remote-SSH 移植），用法与 VS Code 相同：命令面板 `Remote-SSH: Connect to SSH Host` → 输入主机 → 自动装远端 server。Cascade（agent）在远程 workspace 可用（早期有插件/远程兼容问题，社区逐步修复）。无独立的主机管理面板，完全依赖 `~/.ssh/config`。

来源：<https://marketplace.windsurf.com/extension/jeanp413/open-remote-ssh> · <https://everything.intellectronica.net/p/windsurfing-the-codespaces> · <https://www.reddit.com/r/Codeium/comments/1j156z6/>

### 2.6 Zed（Rust 同源的远程架构，对 Shannon 参考价值最高）

**架构**：本地跑 Zed UI + Tree-sitter 高亮 + LLM 通信 + 未保存缓冲；远端跑 headless 的 **zed-remote-server**（Rust 二进制），负责源码、语言服务器、任务、终端。SSH 是唯一通道（曾有的经 Zed 服务器中继模式已在 v0.157 移除）。AI（Agent Panel、Inline Assistant）在远程会话中可用。

**用户旅程**：
1. `ctrl-cmd-shift-o` 打开 **Remote Projects** 对话框 → "Connect New Server" → 输入 ssh 命令（支持 `-p -l -L -R -i -o -J -F` 等参数与 bash 引号规则）。
2. Zed 用系统 `ssh` 二进制拨号（因此**自动继承 `~/.ssh/config`**、agent、Known Hosts 交互——host key/密码短语提示直接浮在 UI 里），建立 **ControlMaster 多路复用**（每主机一条复用连接，开新项目免重认证）。
3. 检查远端 `~/.zed_server/zed-remote-server-{channel}-{version}`，缺失/版本不匹配则从 zed.dev 下载；`upload_binary_over_ssh: true` 可改为"本地下载再经 SSH 上传"，专供无外网服务器；也支持手动放二进制（版本必须精确匹配）。
4. 选路径打开项目。CLI 亦通：`zed ssh://user@host:port/path`、`zed://ssh/...`。

**认证**：明确**禁止在设置文件里存密码**（"we do not support writing a password to your settings file"），只支持 key-based；Windows 下给出 askpass 图形对话框与 credential-manager 冲突排查指引。

**重连**：每条连接走"代理模式"——掉线后自动重启/复用 daemon 续连；失败则不复用 daemon，但未保存修改留在本地、重连后恢复。连接级 `port_forwards` 配置（底层 ssh -L）。

**限制**：远端终端里敲 `zed` 打不开文件；超大目录（>10 万文件）劝退；远端不支持 FreeBSD；无 Windows 远端（WSL 是本地特例）。

来源：<https://zed.dev/docs/remote-development> · <https://zed.dev/blog/remote-development> · <https://zed.dev/remote-development>

### 2.7 JetBrains（Remote Development / Gateway）

**架构**：**整个 IDE 后端**（含索引、运行、调试）在远端主机跑 headless；本地只跑 **JetBrains Client** 瘦客户端；两者经 TLS socket + SSH 隧道**直连**（无中继）。Gateway 是独立启动器，也可从 IDE 欢迎屏 File > Remote Development 进入。

**用户旅程**：欢迎屏选 Remote Development → SSH → 填主机（读 `~/.ssh/config`，可选存密码/密钥）→ 选远端项目路径 + 选后端 IDE 版本 → Gateway 下载并启动后端 → 打开 Client。Gateway 界面维护"最近 SSH 主机 + 最近项目"两个列表（即它的主机管理 UI）。

**Dev Containers**：Gateway 与 IDE 欢迎屏均有 Dev Containers 区——"New Dev Container"（读 `devcontainer.json` 建）或在远端项目上按其 json 起容器后连接，即"SSH 主机 × 容器"两级嵌套官方支持。

**限制**：社区反馈延迟与稳定性问题较多（索引/同步在弱网下体验差）；远端需下载完整后端 IDE（磁盘/内存开销大）。

来源：<https://www.jetbrains.com/help/idea/remote-development-a.html> · <https://www.jetbrains.com/help/idea/remote-development-overview.html> · <https://www.jetbrains.com/help/idea/start-dev-container-for-a-remote-project.html> · <https://blog.jetbrains.com/idea/2024/07/using-dev-containers-in-jetbrains-ides-part-1/> · <https://wyh.life/article/2022/11/13/how-jetbrains-gateway-works>

### 2.8 Workspace-on-demand 阵营（环境即实体）

这组产品的共同点：**把"环境/workspace"建模为一等实体**，有 ID、状态机（creating/running/stopped/deleted）、生命周期操作（create/start/stop/rebuild/delete）、描述配置（devcontainer/模板）和多客户端接入（Web/桌面 IDE/SSH/CLI）。Shannon 若引入"远程目标"实体，最值得抄这套语义。

**DevPod**（开源，loft-loft）：
- 客户端-代理架构：本地 client 经 provider（docker / ssh / kubernetes / 任一云）把 **agent** 注入目标机与容器，agent 起 gRPC + SSH server，承担端口转发、**凭证转发**、日志流——形成"横跨开发环境的控制平面"。
- 核心命令 `devpod up <repo>`：按上下文选 provider → 起 devcontainer → 本地 IDE 经 workspace 内 SSH server 自动接入。workspace 可 list/stop/delete；配置 yaml 化（provider + workspace 两层）。
- ssh provider = "把已有主机变成 workspace"，正好是 Shannon 要的"容器/主机即目标"语义。

**Coder**（自托管平台）：
- workspace 由 **Terraform 模板**定义（可跑在任何云/本地）；Web 控制台管理生命周期与状态。
- **`coder config-ssh`**：为每个 workspace 在 `~/.ssh/config` 生成 Host 条目（或通配块），此后**任何** SSH 客户端/IDE 零改造直连——"平台实体 → 标准配置文件"的反向输出思路很妙。
- Coder 为每用户生成 SSH keypair；官方主打 "AI coding agents hosted in workspaces"（把 Claude Code 等跑进 workspace），VS Code 扩展/JetBrains Gateway 均可列 workspace 接入。

**GitHub Codespaces**：
- codespace = 托管 VM 上的容器；`devcontainer.json` + 生命周期钩子（onCreateCommand → updateContentCommand → postCreateCommand → postStartCommand → postAttachCommand）+ **prebuild** 缓存。
- 管理面：github.com/codespaces 列表 + `gh cs` CLI（list/stop/rebuild/delete/ssh）；浏览器 IDE 或桌面 IDE（经 SSH）接入；端口转发需用户授权、secrets 集中管理、GitHub 凭证自动注入 git。

**Gitpod（现 Ona）**：
- Classic：repo URL 即建环境，`gitpod.yml` 声明配置；浏览器/桌面 IDE（SSH 底座）/JetBrains Gateway 接入；SSH 设置页可取每环境连接命令。
- Flex：环境跑在**自有基础设施**（AWS/GCP/Azure/裸机 + Docker），主机装 agent 后注册为环境——同样"注册主机"模型。

**Daytona**：已转型为"跑 AI 生成代码的安全基础设施"：sandbox 是一等实体（create/exec/snapshot/secrets，SDK 化）。旧版 v1 的 dev environment manager（`daytona create` + SSH + provider）路线已被这个方向取代——对 Shannon 的启示是"目标实体"既可服务开发也可以服务 agent 执行。

来源：<https://devpod.sh/docs/how-it-works/overview> · <https://coder.com/docs/user-guides/workspace-access> · <https://coder.com/docs/reference/cli/config-ssh> · <https://docs.github.com/en/codespaces/about-codespaces/deep-dive> · <https://ona.com/docs/classic/user/configure/user-settings/ssh> · <https://www.daytona.io/docs/en/>

### 2.9 Warp 与 Termius（主机管理 / 终端侧模式）

**Warp**：
- **Warpify SSH**：终端里敲 `ssh ...`，Warp 检测认证成功后弹内联提示"是否安装 SSH extension"→ 往远端 `~/.warp*/remote-server` 装伴随进程（以你的用户身份、不开网络端口、只写 home、随客户端版本自更新）。装上后获得：真·远端文件树、远端文件编辑与 code review、远端代码库索引、agent 以原生 diff 应用编辑（而不是退化成 sed）。设置项 `warpify.ssh.ssh_extension_install_mode`（Always ask/Always/Never）+ `ssh_hosts_denylist`。
- **Agent CLI 的反方向**：本地一个二进制，用 pty 多路复用驱动远端 shell，"SSH into a remote host and use the agent **without installing a remote binary**"——即轻路径（无远端组件）与重路径（remote-server）双轨并存，按功能分级。
- 限制：远端仅 Linux/macOS；glibc ≥ 2.31，不支持 Alpine/musl 与老发行版（静默降级为传统透传）；需 bash/zsh、可写 home；Windows 客户端装不了扩展。

**Termius**（SSH 客户端 UX 黄金标准，供 Shannon 主机管理 UI 参考）：
- 实体分层：**Hosts → Groups（+Tags）→ Identities（凭证）**；Host 是一等对象，SFTP、Port Forwarding 规则、Host Chains（跳板链）都引用它。
- **Snippets**：可参数化的常用命令模板，一键在目标主机执行。
- **Port Forwarding**：local/remote/dynamic 三种规则持久化管理（用户抱怨点：规则不支持分组——反过来说明"规则也要可组织"）。
- **信任与同步**：端到端加密云同步（主机/密钥/known_hosts 跨设备）；可从 `~/.ssh/known_hosts` 导入；付费档含 agent 转发与团队共享。
- 关键 UX 特征：侧栏主机树 + 双击即连 + 移动端同构——"连接器"本身被产品化为持久资产而不是临时输入。

来源：<https://docs.warp.dev/terminal/warpify/ssh/> · <https://www.warp.dev/agent-cli> · <https://docs.termius.com/> · <https://docs.termius.com/organize-and-connect-to-hosts/port-forwarding-and-tunneling>

---

## 3. 跨产品模式综合

1. **`~/.ssh/config` 是事实标准**。VS Code / Cursor / Windsurf / Zed / JetBrains / Warp 全部直接消费它；Coder 反向把平台实体**写回**它（`coder config-ssh`）。Shannon 不应发明私有主机格式，而是"读取 + 增强"（在自有库里存别名/标签/备注，底层仍指向 ssh config 条目）。
2. **瘦客户端 + 首连自动安装远程组件** 是万能架构（VS Code Server、cursor-server、zed-remote-server、JetBrains 后端、Warp remote-server、DevPod/Coder agent 全是此模式）。组件以当前用户身份跑、放 `~/.xxx`、按客户端版本精确匹配、支持**经 SSH 上传**以适配无外网主机（Zed/Warp 都做了这个 fallback）。
3. **首连信任与平台检测是固定仪式**：host key 指纹确认（TOFU）、远端 OS/arch 探测（识别不了让用户手选并记住）、server 下载进度通知、日志输出通道（Remote-SSH Output）。
4. **认证红线**：没有一家在配置文件里存密码；全部走 key + ssh-agent，提示交互（passphrase/fingerprint）浮到自家 UI。容器场景则**不挂载宿主敏感目录**（Claude Code 明确反对挂 `~/.ssh`），改用短命凭证注入。
5. **远程可见性三件套**：连接管理器（面板/对话框）、主机/目标选择器（命令面板 + 列表）、**全局远程指示器**（状态栏主机名徽标，点开是远程命令菜单）。掉线统一为"自动重连 + 保留本地未保存状态 + 显式断开入口"。
6. **容器目标双入口**：①声明式（`devcontainer.json`：build/run + 生命周期钩子 + features）；②命令式（attach 运行中容器）。行业用 containers.dev 规范做互操作底座，JetBrains/Codespaces/Cursor/DevPod 全兼容。
7. **执行路由只有两种流派**：全量远置（编辑器系：workspace 完全在远端，本地纯 UI）与本地代理（Warp Agent CLI：agent 在本地，pty 驱动远端；功能受限但零安装）。AI agent 产品（Cursor 自托管、Claude 云沙箱、Codex cloud）额外有第三种：**出站长连接 worker**——目标机主动注册到控制面，控制面推任务（对 NAT/防火墙最友好）。
8. **环境即实体**（workspace-on-demand 阵营）：目标 = {标识、连接信息、状态机、生命周期操作、描述配置、接入通道}；实体可被 Web/CLI/IDE 多端消费；"停止/重建/删除"是标配操作而非一次性连接。
9. **会话可迁移**成为新趋势：Claude Code teleport（cloud→local，单向）、Warp cloud agent handoff。会话与目标解耦后，"在哪接着跑"变成用户选择。
10. **分级降级**：Warp 无扩展时退化为透传终端；Codex 沙箱失败提示"retry without sandbox"。远程能力应按"有远端组件 > 纯透传"两档设计，而不是硬依赖。

---

## 4. 给 Shannon 的功能模型建议

### 4.1 实体模型

```rust
// 统一目标抽象：一切 agent 会话都绑定一个 target
enum Target {
    Local,
    Ssh(RemoteHost),
    Docker(ContainerTarget),
}

struct RemoteHost {
    id: Uuid,
    label: String,                  // 展示名（默认 user@host）
    ssh_ref: SshRef,                // 底层指向 ~/.ssh/config 的 Host 条目或显式参数
    // SshRef = ConfigEntry("my-alias") | Explicit { host, port, user, identity_files, jump }
    groups: Vec<String>,            // 分组/标签（Termius 式组织）
    trust: HostKeyTrust,            // 首连 TOFU 记录：指纹 + 首次时间（决定告警）
    platform: Option<PlatformInfo>, // 首连探测到的 os/arch（缓存，避免重复探测）
    remote_agent: AgentRuntimeInfo, // 已装版本 / 目标版本 / 上次心跳
    notes: String,
    default_workspace: Option<PathBuf>,
}

struct ContainerTarget {
    id: Uuid,
    runtime: DockerEndpoint,        // local daemon | ssh://host 的 docker context
    container: ContainerRef,        // 运行中容器 attach
    // 或声明式：
    definition: Option<DevcontainerRef>, // devcontainer.json 路径（兼容 containers.dev 规范）
    mounts: Vec<Mount>,
    state: ContainerState,          // created/running/stopped/exited（复用 docker 状态机）
}
```

要点：
- **不发明私有 SSH 格式**：`ssh_ref` 优先引用 `~/.ssh/config` 条目；Shannon 库只存增量（label/分组/trust/备注）。提供 "从 ssh config 导入" 的一键迁移。
- **trust（host key 指纹）必须持久化**：首连弹指纹确认框；后续变更强告警——这是竞品公认的信任仪式。
- **容器目标同时支持 attach（运行中容器）与 devcontainer.json（声明式创建）**，后者直接兼容生态。

### 4.2 远程执行组件（shannon-server）

仿 zed-remote-server / VS Code Server 模式：
- 远端二进制 `~/.shannon/bin/shannon-server`，以登录用户运行，unix socket 监听，**不开入站网络端口**；
- 连接首查远端 `~/.shannon/`：版本匹配直接复用，否则下载；提供 `upload_binary_over_ssh` 式 fallback（本地下载经 SSH 上传，兼容无外网主机）；
- 全部工具路由到远端：文件读写/glob/grep/shell/进程管理在远端执行（workspace 全量远置）；会话记录与 UI 状态留在本地；LLM 流量始终从本地出（密钥不落远端）；
- 重连：SSH ControlMaster 多路复用 + server 常驻，掉线自动重连，未落盘会话状态本地保留。

### 4.3 用户旅程

**桌面 UI（Tauri）**：
1. 连接管理器面板：目标列表（本地/SSH/容器分组），状态点（最近可达性、远端组件版本），顶部"新建目标"（导入 ssh config / 手填 / 新建容器）。
2. 新建 SSH 目标向导：粘贴 `ssh user@host -p 2222` 或选 config 条目 → 测试连接 → **指纹确认框**（显示 key type + SHA256 指纹）→ 探测平台 → 自动装 shannon-server → 完成。
3. 新建容器目标：选 docker context → 列运行中容器 attach；或选 repo 的 devcontainer.json → 构建/启动 → 进入。
4. 会话发起：新建会话时选目标（默认本地，记住上次目标）；会话窗口顶栏常驻**目标徽标**（如 `ssh: build-server` / `docker: rust-dev`），点开菜单 = 断开 / 重连 / 查看日志 / 打开远端终端。
5. 状态可见性：目标详情页显示探测到的平台、组件版本、最近心跳、信任记录。

**CLI / TUI**：
```
shannon target list                     # 列出目标（含健康状态）
shannon target add ssh [--from-config]  # 导入或新建（--from-config 交互式选 ssh config 条目）
shannon target add docker [--attach <name>] [--devcontainer <path>]
shannon target test <id>                # 连通性 + 平台探测 + 组件安装（dry-run 可选）
shannon --target <id>                   # 本会话后续命令均指向该目标
shannon chat --target <id> "修复 failing tests"   # 一步直达
shannon target remove <id>
```
TUI 内：会话顶部显示目标徽标；`/target` 斜杠命令热切换；首次连上远端时 TUI 内嵌指纹确认与安装进度条。

### 4.4 必备 vs 锦上添花

**Must-have（MVP 对齐行业基线）**：
1. SSH 目标一等实体 + `~/.ssh/config` 导入与复用（key/agent 认证，不存密码）；
2. 首连 host key 指纹确认（TOFU 持久化 + 变更告警）；
3. shannon-server 自动安装/版本匹配/经 SSH 上传 fallback；
4. 工具全量路由远端（workspace 在远端），本地保留 UI 与凭证；
5. 目标列表 UI + 状态徽标 + 命令面板/CLI 目标选择；
6. Docker attach（连本机 daemon 的运行中容器）；
7. 断线自动重连 + 会话恢复（本地保留会话日志）。

**Nice-to-have（二阶段）**：
1. `devcontainer.json` 声明式容器创建（兼容 containers.dev，含生命周期钩子）；
2. 远程主机上的 devcontainer（SSH × 容器嵌套，JetBrains 模式）；
3. 出站 worker 模式（目标机主动注册，穿越 NAT；Cursor self-hosted / VS Code Tunnels 模式）；
4. 端口转发持久规则（Termius 式规则管理 + `LocalForward` 复用）；
5. 会话迁移/接管（teleport：桌面会话 ←→ CLI 会话 ←→ 远端目标，Claude Code 模式）；
6. 主机分组/标签/Snippets（Termius 式组织）；跳板链（ProxyJump 已由 ssh config 免费获得，UI 显性化即可）；
7. 容器目标快照/重建（Daytona/Codespaces 模式）；
8. 组织级策略下发（禁 bypass、网络白名单模板，参考 Claude Code managed-settings + 防火墙脚本）。

### 4.5 风险与注意

- **沙箱语义变化**：本地 bash 沙箱（namespace/seccomp）在远端未必可用（老内核/非 Linux 远端），需探测降级并在 UI 标注远端沙箱等级；
- **凭证不外溢**：API key、git 凭证留在本地/由用户在远端自行配置；绝不随 server 安装包上传（Claude Code 教训：容器内凭证可被恶意项目外泄）；
- **平台矩阵**：远端先支持 Linux x86_64/aarch64（竞品均如此），macOS 次之，Windows 远端可明确列为不支持；
- **老发行版**：glibc 版本探测 + 明确报错（Warp 静默降级被诟病，不如 Zed 显式失败）。

---

## 5. 来源索引

- VS Code Remote-SSH: <https://code.visualstudio.com/docs/remote/ssh> · <https://code.visualstudio.com/docs/remote/vscode-server> · <https://code.visualstudio.com/blogs/2019/10/03/remote-ssh-tips-and-tricks>
- VS Code Tunnels: <https://code.visualstudio.com/docs/remote/tunnels>
- VS Code Dev Containers: <https://code.visualstudio.com/docs/devcontainers/containers> · <https://devcontainers.github.io/implementors/json_reference/>
- Claude Code devcontainer/sandbox: <https://code.claude.com/docs/en/devcontainer> · <https://code.claude.com/docs/en/sandbox-environments> · <https://code.claude.com/docs/en/sandboxing> · <https://www.anthropic.com/engineering/claude-code-sandboxing> · <https://platform.claude.com/docs/en/managed-agents/self-hosted-sandboxes>
- Claude Code web/teleport: <https://claude.com/blog/claude-code-on-the-web> · <https://news.ycombinator.com/item?id=45647166> · <https://github.com/anthropics/claude-code/issues/14666>
- Codex: <https://learn.chatgpt.com/docs/environments/cloud-environment> · <https://learn.chatgpt.com/docs/cloud> · <https://github.com/openai/codex> · <https://docs.docker.com/ai/sandboxes/agents/codex/> · <https://github.com/openai/codex-universal>
- Cursor: <https://cursor.com/docs/cloud-agent/self-hosted> · <https://forum.cursor.com/t/durable-agent-runtime-on-remote-ssh-cloud-workstations/159513> · <https://forum.cursor.com/t/does-cursor-support-remote-ssh/7620>
- Windsurf: <https://marketplace.windsurf.com/extension/jeanp413/open-remote-ssh>
- Zed: <https://zed.dev/docs/remote-development> · <https://zed.dev/blog/remote-development>
- JetBrains: <https://www.jetbrains.com/help/idea/remote-development-a.html> · <https://www.jetbrains.com/help/idea/start-dev-container-for-a-remote-project.html> · <https://blog.jetbrains.com/idea/2024/07/using-dev-containers-in-jetbrains-ides-part-1/>
- DevPod: <https://devpod.sh/docs/how-it-works/overview> · <https://github.com/loft-sh/devpod>
- Coder: <https://coder.com/docs/user-guides/workspace-access> · <https://coder.com/docs/reference/cli/config-ssh>
- Codespaces: <https://docs.github.com/en/codespaces/about-codespaces/deep-dive>
- Gitpod/Ona: <https://ona.com/docs/classic/user/configure/user-settings/ssh>
- Daytona: <https://www.daytona.io/docs/en/>
- Warp: <https://docs.warp.dev/terminal/warpify/ssh/> · <https://www.warp.dev/agent-cli>
- Termius: <https://docs.termius.com/> · <https://docs.termius.com/organize-and-connect-to-hosts/port-forwarding-and-tunneling>
