# Shannon Agent — 安装 / 发布架构方案（高级架构师版）

> 状态：**待审核**（2026-07-18）
> 对标：[hermes-agent](https://github.com/NousResearch/hermes-agent)（统一 `hermes` 命令 + `serve`/`desktop`/`gateway` 子命令 + systemd/launchd 服务）
> 已确认决策（用户拍板）：
> 1. **伞命令归属**：`shannon`(Rust) 壳到 `shannon-gateway` 二进制做 gateway 控制（gateway 保持独立进程，契合 README 架构）。
> 2. **发布模型**：合并成**单一** `release.yml`，一个 tag 一个 release。
> 3. **productName 改名**：`Shannon Agent` → `shannon-desktop`（产物名统一为 `shannon-*` 体系）。
> 4. **gateway 服务化（阶段 D）**：现在做。
> 5. **Nice-to-have**（Homebrew cask / Scoop / Winget / AUR / `update` / `doctor`）：纳入本方案。

---

## 0. 目标态（Grafana 式一句话）

```
用户:  curl -fsSL https://get.shannon.ai/install.sh | sh
      → 装好 shannon(CLI) + shannon-desktop + shannon-gateway，gateway 注册为后台服务

shannon                # REPL（默认）            [已有]
shannon query …       # 非交互                    [已有]
shannon serve …       # 无头 api_server :33420  [已有]
shannon desktop [--no-build] [--foreground]   # 构建/启动桌面端     [新增]
shannon gateway <sub> # run/start/stop/status/install/… [新增，壳到二进制]
shannon update        # 自更新                    [新增]
shannon doctor        # 环境/端口诊断           [新增]
```

---

## 1. 现状核对（已读代码确认）

| 项 | 现状 | 问题 |
|---|---|---|
| `shannon` CLI (`crates/shannon-cli`) | 已有 `repl/version/config/query/serve(:33420)/screenshot/mcp` | 缺 `desktop`/`gateway`/`update`/`doctor` |
| 发布 workflow | `Release`(cargo-dist) **与** `release-desktop-gateway` **都**触发 `v*` 各建一个 release | 同一 tag 两个 release **冲突** |
| cargo-dist 实际状态 | **失败**：crate 版本 `0.6.0` ≠ tag `0.6.3` | `shannon`(code 二进制) **从未发布** |
| `scripts/install.sh` / `.ps1` | 指向 `shannon-code` 仓库 + `shannon-cli-*.tar.gz` | 资产名对不上真实 release，**已失效** |
| `desktop/tauri.conf.json` `productName` | `"Shannon Agent"` | 产物 `Shannon.Agent_0.6.0_amd64.deb`，与 `shannon-*` 不一致 |
| gateway 服务管理 | 仅裸二进制 `bun run dev` / 直接跑 | 无 `install`/`start`/`stop`/`status` |
| `dist-workspace.toml` | `installers=["shell","powershell","homebrew"]`，`tap="shannon-agent/homebrew-shannon"` | cargo-dist 跑不起来，配置闲置 |
| R2 CDN 镜像 | `Release` 里已写（Cloudflare R2 `$VERSION/` `latest/`） | 因 cargo-dist 失败未生效 |

**版本来源有 4 处独立硬编码**（release-prep 必须全改）：
- `Cargo.toml` `workspace.package.version` （crate 多数为 `version.workspace = true`，继承它）
- `desktop/tauri.conf.json` → `"version": "0.6.0"`（**Tauri 不读 cargo workspace**，单独硬编码）
- `gateway/package.json` → `"version": "0.6.0"`
- `crates/shannon-cli/src/main.rs` clap `#[command(version="0.1.0")]`（`shannon --version` 展示值）

---

## 2. 架构决策与理由

| 决策 | 选择 | 理由 |
|---|---|---|
| 伞命令 | Rust `shannon` 壳到 `shannon-gateway` | gateway 是 TS/Bun、host-agnostic、经 WS 连 engine；用 Rust 重写协议客户端是重复。壳调用保留单一 gateway 代码库（决策 #1）。 |
| 单一 release | 一个 `release.yml` 编排三产品 → 一个 draft release → published | 避免同 tag 两个 release、单一 `latest` 真相源、install 脚本解析更简单（决策 #2）。 |
| cargo-dist 仍用于 CLI | `release=false` 仅上传到既有 release | 白捡 homebrew formula + shell/powershell 安装器 + 校验和；跨目标编译。风险见 §7，有 fallback。 |
| 服务单元级别 | **用户级**（systemd `--user` / `~/Library/LaunchAgents`） | 免 sudo，solo dev 友好，契合 hermes 默认。 |
| productName 改名 | `shannon-desktop` | 产物名统一 `shannon-*`，与 gateway 资产一致（决策 #3）。 |

---

## 3. 阶段 A — 版本对齐（先解锁 CLI 发布）

**新增 `just release-prep <version>`**（命令梳理自现状）：
```just
release-prep version:
    # 1) cargo workspace 版本
    sed -i 's/^version = ".*"/version = "{{version}}"/' Cargo.toml   # workspace.package
    # 2) Tauri（不读 workspace，必须单独改）
    #    改 desktop/tauri.conf.json 顶层 "version"
    # 3) gateway
    #    sed package.json "version"
    # 4) shannon --version 展示值（clap attr，可选）
    # 5) 提交 + 打 tag
    git commit -am "chore(release): v{{version}}"
    git tag v{{version}}
```
- CI `prep` job 守卫：**`workspace.package.version` ≠ tag 则失败**（对齐 publish-crates 已有的校验逻辑）。
- `dist-workspace.toml` 加 `release = false`、`pr-release = false`（cargo-dist 不再自建 release，改为往既有 tag release 上传）。

**验证门**：对 `v0.7.0-rc` 打 tag，`shannon` CLI 二进制 + homebrew formula 成功产出。

---

## 4. 阶段 B — 单一 `release.yml` 编排

```
name: release
on:  push tags 'v[0-9]+.[0-9]+.[0-9]+*'   (+ workflow_dispatch)
permissions: contents: write
env: FORCE_JAVASCRIPT_ACTIONS_TO_NODE24: true

jobs:
  prep       # 版本守卫；计算矩阵；生成 SHA256SUMS 骨架
  cli        # cargo-dist build shannon 跨目标 → 上传到 scratch(release=false)
  desktop    # tauri-action@v0 (tagName+releaseName+releaseDraft:true)
              #   → 直接上传 bundle 到(draft) release；productName 已改名 shannon-desktop
  gateway    # bun build --compile ×4 → softprops 上传到同一 release(draft)
  publish    # 依赖 cli+desktop+gateway(success)
              #   softprops draft:false,prerelease:false
              #   files: scripts/install.sh scripts/install.ps1 SHA256SUMS
              #   镜像 Cloudflare R2: $VERSION/ , latest/ , install.sh/.ps1
```

- **desktop job**：沿用已验证的 `tauri-action@v0` 矩阵（5 入口：deb/rpm/x64-dmg/aarch64-dmg/nsis），无需重写——只改 `productName` 后即产出 `shannon-desktop_*`。
- **gateway job**：沿用已验证的 `bun build --compile --target --outfile` 矩阵（4 目标）。
- **cli job**：`dist build --tag=vX --output-format=json` 后 `dist host --steps=upload --tag=vX`（因 `release=false`，只传资产不建 release）。
- **废弃** `release-desktop-gateway.yml`（合并进来）；`Release.yml`（cargo-dist 自动生成）由编排器取代，删 `allow-dirty=["ci"]` 或折叠。
- **R2 镜像**：从旧 `Release` 搬过来（`wrangler r2 object put`，需 `CLOUDFLARE_API_TOKEN`/`CLOUDFLARE_ACCOUNT_ID`/`R2_BUCKET`）。

**产物资产名（改名后，install 脚本必须对齐）**：
```
CLI:      shannon-x86_64-unknown-linux-gnu.tar.gz (+.sha256)
          shannon-x86_64-apple-darwin.tar.gz
          shannon-aarch64-apple-darwin.tar.gz
          shannon-x86_64-pc-windows-msvc.zip
Desktop:  shannon-desktop_0.7.0_amd64.deb
          shannon-desktop-0.7.0-1.x86_64.rpm
          shannon-desktop_0.7.0_x64.dmg
          shannon-desktop_0.7.0_aarch64.dmg
          shannon-desktop_0.7.0_x64-setup.exe
Gateway:  shannon-gateway-linux-x64
          shannon-gateway-linux-arm64
          shannon-gateway-darwin-x64
          shannon-gateway-darwin-arm64
+         install.sh  install.ps1  SHA256SUMS
```

---

## 5. 阶段 C — 统一 `shannon` 命令（`crates/shannon-cli`）

在现有 `Commands` enum 追加（Rust/clap）：

```rust
/// Build & launch the Shannon desktop app (à la `hermes desktop`)
Desktop {
    /// Skip cargo tauri build; launch already-installed app
    #[arg(long)] no_build: bool,
    /// Run in foreground (don't detach)
    #[arg(long)] foreground: bool,
},

/// Control the Shannon gateway (delegates to shannon-gateway binary)
Gateway {
    #[command(subcommand)] command: GatewaySubcommand,
},

/// Self-update from the latest release
Update,

/// Diagnose environment, ports, and connectivity
Doctor,
```

- **`shannon desktop`**：解析桌面二进制（顺序：PATH `shannon-desktop` → 已知安装目录 → 若无且非 `--no-build` 则 `cargo tauri build`）。启动之。桌面端**保持进程内嵌 engine**（与现状一致，不依赖独立 `serve`）。
- **`shannon gateway <sub>`**：构造 argv 并 `spawn` 外部 `shannon-gateway` 二进制（决策 #1）。`GatewaySubcommand` 镜像 hermes：`Run`/`Start`/`Stop`/`Restart`/`Status`/`List`/`Install`/`Uninstall`/`Setup`/`MigrateLegacy`/`Enroll`。
- **`shannon update`**：`GET api.github.com/repos/shannon-agent/shannon-agent/releases/latest` → 比对 tag 与本地 `shannon --version` → 下载平台资产并重装（或重跑 install.sh）。
- **`shannon doctor`**：检查清单（Rust/node/bun 存在性、config 合法性、`:33420` 端口占用、engine 连通性、gateway 可达、TLS/证书）。

---

## 6. 阶段 D — gateway 服务化（`gateway/src`，现在做）

新增 `gateway/src/service/`，由 `gateway install` 写单元、`shannon gateway install` 调用：

**Linux (systemd --user)** `~/.config/systemd/user/shannon-gateway.service`：
```
[Unit] Description=Shannon Gateway After=network.target
[Service] Type=simple
ExecStart=%h/.local/bin/shannon-gateway run
Restart=on-failure
[Install] WantedBy=default.target
```
→ `systemctl --user daemon-reload && systemctl --user enable --now shannon-gateway`

**macOS (launchd)** `~/Library/LaunchAgents/com.shannon-agent.gateway.plist`：
```
Label=com.shannon-agent.gateway
ProgramArguments=(/usr/local/bin/shannon-gateway, run)
RunAtLoad=true  KeepAlive=true
```
→ `launchctl load ...`

**Windows (MVP)** `schtasks` 注册后台任务（foreground `run` 由任务调度拉起）；NSSM 作为可选进阶。

子命令行为：
| sub | 行为 |
|---|---|
| `install` | 写用户级单元 + enable/start |
| `uninstall` | stop + disable + 删单元 |
| `start`/`stop`/`restart` | 调对应服务管理器 |
| `status` | 查服务管理器状态 + 探活 gateway 健康端点（WS/HTTP），报 PID + 可达性 |
| `list` | 枚举 profiles（config 目录）及各自运行状态 |
| `setup` | 交互式平台鉴权（复用 `config/loader.ts` + `secrets/`） |
| `migrate-legacy` | 删改名前旧 `shannon.service` 单元（防御） |
| `enroll` | 实验性 relay connector（stub/延后） |

**权限**：全用**用户级**单元（免 sudo）。若系统级安装，root 单元为可选。

---

## 7. 阶段 E — 分发渠道（Nice-to-have，纳入本方案）

| 渠道 | 状态 | 动作 |
|---|---|---|
| GitHub Releases（三产品 + install 脚本 + SHA256） | 阶段 B 做 | 单一 release |
| `curl get.shannon.ai/install.sh \| sh` | 阶段 B 重写 | 对齐真实资产名 + R2 镜像 |
| Homebrew tap `shannon-agent/homebrew-shannon` | cargo-dist 已配 | CLI formula 自动；**新增** desktop cask + gateway formula（手写） |
| Scoop (Win) | 缺 | bucket + `shannon.json` manifest（cli+gateway zip） |
| Winget (Win) | 缺 | `shannon-agent.shannon{,-desktop,-gateway}.yaml` |
| AUR (Linux) | 缺 | `shannon-desktop-bin` / `shannon-gateway-bin` PKGBUILD（`-bin` 指 GitHub release） |

- **Homebrew**：
  - CLI：`brew install shannon-agent/homebrew-shannon/shannon`（cargo-dist 生成）
  - Desktop：`brew install --cask shannon-agent/homebrew-shannon/shannon-desktop`（手写 cask）
  - Gateway：`brew install shannon-agent/homebrew-shannon/shannon-gateway`（手写 formula）
- 这些作为 **post-release 自动化**（`publish-channels` job 或手动）逐步实现，优先级低于 A–D。

---

## 8. `install.sh` / `install.ps1` 重写（阶段 B 交付物）

新逻辑（Linux 为例，macOS/Win 类比）：
```sh
CDN="${SHANNON_CDN_URL:-https://get.shannon.ai}"   # R2 镜像，回退 GitHub latest
# 1) CLI: shannon-<target>.tar.gz (+.sha256) → /usr/local/bin (fallback ~/.local/bin)
# 2) Gateway: shannon-gateway-<target> (+.sha256) → 同上
# 3) Desktop: 按 OS 下对应 bundle 并装
#      Linux:   shannon-desktop_*.deb  → dpkg -i  (或 .rpm → dnf/yum)
#      macOS:   shannon-desktop_*.dmg → 挂载装 / brew --cask
#      Win:     shannon-desktop_*setup.exe → 静默安装
# 4) 校验: shannon --version && shannon-gateway --version
# 5) 提示: shannon gateway install  (注册服务)
```
**硬约束**：资产名必须与 §4 产物清单**逐字一致**（旧脚本引用 `shannon-cli-*` 是错的，必须改）。

---

## 9. 风险登记

| 风险 | 影响 | 缓解 |
|---|---|---|
| cargo-dist `release=false` + 上传到既有 release 行为刁钻 | CLI 资产传不到单一 release | **fallback**：弃 cargo-dist，编排器内 `cargo build --target` 直编 CLI，手写 homebrew formula |
| aarch64-linux desktop 缺失 | Linux ARM 桌面无包 | 已知限制；待自托管 aarch64 runner或 GitHub GA |
| macOS 签名/公证缺失 | 非开发机 Gatekeeper 拦截 | 独立任务（阶段外）；先 dev 签名 |
| NSIS `.exe` / RPM 命名与预期不符 | install 解析失败 | §4 资产名以 `tauri-action` 实际产出为准，发布前在 rc tag 核对 |
| gateway 服务跨 OS 行为差异（尤其 Win） | `install`/`status` 不稳 | 阶段 D 先 Linux/macOS，Win 用 schtasks MVP，NSSM 延后 |
| 用户级 systemd 在 headless/容器失效 | `start` 无效 | `run` 前台模式兜底（WSL/Docker/Termux）|

---

## 10. 实施顺序 / 里程碑

1. **A 版本对齐** → `v0.7.0-rc1` 验证 CLI 二进制 + homebrew。
2. **B 单一 release.yml** → 切 `v0.7.0`（一个 release，三产品全资产 + install 脚本 + R2）。
3. **C 统一 `shannon` 命令**（`desktop`/`gateway`/`update`/`doctor`）→ `v0.7.x`。
4. **D gateway 服务化** → `v0.8.0`。
5. **E 分发渠道**（Homebrew cask/Scoop/Winget/AUR）→ `v0.8.x` 增量。

---

## 11. 验证门（每个里程碑）

- [ ] `just ci` 全绿（fmt + clippy + deny + test）。
- [ ] `shannon --version` / `serve --help` / `desktop --help` / `gateway --help` 均能解析。
- [ ] `v0.7.0-rc`：确认**仅一个** release，三产品资产齐全，install.sh 解析成功，homebrew formula 构建通过。
- [ ] 干净容器（Linux）/ VM（macOS/Win）实测 install 脚本全流程。
- [ ] `shannon gateway install` → `status` 在 Linux + macOS 实测可启停。
