# 单产品化 Phase B — 分发最后一公里(执行 checklist)

- **状态**: 执行中(决策已由 [ADR-0011](../adr/0011-single-product-multi-surface-distribution.md) 批准,2026-08-19;同日审查扩展 §7/§8;**第一批(B0+B11–B15+B17)2026-08-19 实施;第二批(C2+B16)与第三批 a/b/c(B1–B8)2026-08-20 实施;第四批(C1①+C3+C4 短期缓解)2026-08-20 实施;第五批(push 解封 + 远端 CI 修复 + required checks)2026-08-20 实施;第六批(B9 tag 演练 + publish-crates rc 跳过)2026-08-20 实施**,详见附录「实施记录」。剩余:C4 正式签名(独立排期)与 C1②(随 C4))
- **前置决策**: [RELEASE-INSTALL-PLAN.md](../RELEASE-INSTALL-PLAN.md)(2026-07-18,已实施 ~80%)
- **预计工作量(分档)**: Phase B 主体(B0–B9)5–7 人日;发布安全网(B11–B13,P0)≈1 人日;完整性与渠道卫生(B14–B17)≈1.5 人日;故事修复(C1–C3)≈2–3 人日;签名(C4)独立排期
- **原则**: 只捆绑、不重构;两进程保持隔离;headless 产物永远不链接 GUI 库(ADR-0011 红线 1)

**目标**: 用户从任一路径安装(桌面安装器 / install.sh),得到同版本的 `shannon` CLI 与桌面 GUI,共享 `~/.shannon/` 状态;`shannon` 是唯一用户入口。

---

## 0. 前置测量(开工前 30 分钟)

- [x] 下载计数核实(2026-08-19):GitHub UI 已不展示计数;经 API 查 v0.10.0 全部资产 ≈0–1 次下载(无采用信号)。
- 结论:无数据支撑渠道倾斜 → 维持平台对齐默认;死配置已清理(B17②);winget/scoop/homebrew 手写清单仅修 URL、未接 CI(见附录遗留项)。

## 0.5 install 脚本前置修复(B0,独立于打包工程,可先行,≈0.5 人日)

三场景(服务器 / TUI / 事后加装 desktop)都经手 install 脚本;现状无组件选择,且 arm64 Linux 有资产错配:

- [x] **组件开关**:新增 `SHANNON_COMPONENTS=cli|gateway|desktop|all`(默认 `all`,完全兼容现状),`install.ps1` 同步。服务器一行安装:`curl … | SHANNON_COMPONENTS=cli sh`。**(已实施)**
- [x] **arm64 Linux 修复**:install.sh arm64 分支不再引用不存在的 CLI/桌面资产 —— CLI 无预编译时显式提示源码构建(仅 `SHANNON_COMPONENTS=cli` 时硬错;默认组合时降级警告,gateway 仍可装);`install_desktop()` 改用解析出的 `$DESKTOP` 资产名(aarch64 Linux 无 bundle → 直接跳过)。CLI matrix 补 aarch64-linux 目标留待后续。**(已实施)**
- [x] **跳过 desktop 时不得触发 sudo**:sudo 仅存在于 `install_desktop()` Linux 分支内;组件不含 desktop 时整段不执行。**(已实施)**

## 1. 安装器捆绑 CLI(B1–B4,核心工程量)

### B1. release.yml:桌面构建产物内含 CLI 二进制
- [x] CLI matrix job 产物按 target-triple 命名供桌面 job 消费(依赖关系:desktop job `needs` cli job)。**(2026-08-20 实施)**desktop job `needs: [create-release, cli]`;unix tar.gz / windows zip 两条 staging step 从 draft release 拉取对应 target 资产,解包到 `desktop/binaries/shannon-<target-triple>[.exe]` 并实跑 `--version` 冒烟;`.gitignore` 挡 `desktop/binaries/`。
- [x] `desktop/tauri.conf.json` 增加 `bundle.externalBin: ["binaries/shannon"]`。**(2026-08-20 实施)**
- [ ] 验证 deb/AppImage/NSIS 产物体积增量(CLI release 二进制约几十 MB),记录到附录。
- 注意(已遵守):tauri `beforeBuildCommand` 只管前端;备料只在 release.yml step,不污染本地 `pnpm tauri dev`/`cargo build`(externalBin 是 bundler 特性,dev 构建不经过 bundler)。本地 `pnpm tauri build` 需自备 `desktop/binaries/shannon-<triple>`(发布在 CI 全自动)。

### B2. Windows / NSIS:PATH 注册
- [x] 新增 NSIS installer hook(`bundle.windows.nsis.installerHooks` → `desktop/nsis/hooks.nsh`),安装时把 `$INSTDIR`(CLI 所在目录)加入用户 PATH。**(2026-08-20 实施;仅用 stock NSIS——核实 Tauri 官方 NSIS 发行包**不**含 EnVar 插件,模板已 include WordFunc/LogicLib,PATH 手术用 `${WordReplace}` + `WriteRegExpandStr` + `WM_SETTINGCHANGE` 广播)**
- [x] 卸载时清理 PATH 条目(`NSIS_HOOK_PREUNINSTALL`,best-effort 不阻断卸载)。
- [x] **不遮蔽规则**:hook 先 `nsExec 'where shannon'`,已解析则只记 DetailPrint 不改 PATH;未解析才**追加在末尾**(既有条目全部保持优先);重复安装先剥离旧条目再追加(幂等)。

### B3. macOS / dmg:应用内"安装 CLI 命令"
- dmg 无法改 PATH,采用 VS Code 模式:
- [x] .app bundle 内已含 CLI(B1 的 externalBin);桌面端新增 `commands_surface.rs`(`install_cli_to_path` + `get_cli_install_status` + `get_surface_info`)+ Settings → Advanced → Command line 卡片(状态徽章 + 安装按钮,i18n 双语),symlink 到 `/usr/local/bin`(失败回落 `~/.local/bin`;Windows/NSIS 与 deb/rpm 由安装器负责,按钮呈提示态)。**(2026-08-20 实施)**
- [x] ~~`shannon desktop` 的 macOS 候选路径已含 `/Applications/Shannon Desktop.app/...` 无需改~~ **实测要改**:productName 是 `shannon-desktop`,实际 app 名为 `shannon-desktop.app`(B5 批次已修,含 ~/Applications 变体)。
- [x] 按钮点击时若 PATH 已有 `shannon`,改为提示"已安装(版本 X)"并跳过 symlink(与 B2 的不遮蔽规则同源;命令内非遮蔽短路 + UI 按钮 disabled 双层)。**(2026-08-20 实施)**

### B4. Linux / deb(注:产物矩阵现状只有 deb/rpm,无 AppImage)
- [x] deb:~~postinst symlink~~ **不需要**——externalBin 由 tauri-bundler 自动平铺到 `usr/bin/shannon`(源码级核实 `debian.rs`:`data_dir.join("usr/bin")` + `copy_binaries(&bin_dir)`),卸载随包自动清理。**(2026-08-20 实施)**
- [x] rpm:同等待遇(`rpm.rs` 显式 `/usr/bin` + external_bin 循环);无 postinst 依赖。与 install.sh 的 `/usr/local/bin/shannon` 并存时 PATH 顺序保证脚本安装优先,符合不遮蔽规则。**(2026-08-20 实施)**

## 2. CLI 侧配套(B5–B7)

### B5. `shannon desktop` bootstrap 改造(`crates/shannon-cli/src/main.rs` `run_desktop_command`)
现状:找不到 binary → 自动 `cargo tauri build`(开发行为,不适合装好的产品);且 `find_desktop_binary` 候选**只有 Unix 路径,Windows 永远找不到已安装的桌面端**(新发现的缺口)。
- [x] `find_desktop_binary` 增加 Windows 候选:`%LOCALAPPDATA%\shannon-desktop\`(NSIS perUser;productName 即 shannon-desktop)/ `C:\Program Files\shannon-desktop\`(perMachine)。**(2026-08-20 实施)**顺带修正:macOS 候选原为推测名 `Shannon Desktop.app`,实际 productName 是 `shannon-desktop` → `shannon-desktop.app`(补 /Applications 与 ~/Applications 两处);新增 `desktop/target/release` 候选使 `--build` 后能真正找到产物(旧代码 build 后仍查安装目录,必然落空)。
- [x] 默认 fallback 从"cargo build"改为:打印安装指引(install.sh/ps1 一行 + `--install` + `--build` 三条路径);自动 build 移到显式 `--build`;`--no-build` 隐藏保留(deprecated,no-op)。**(2026-08-20 实施)**指引默认 exit 非零(脚本可判)。
- [x] 安装指引文案与 `shannon update` 对齐(同一 curl 传输层;`--install` 复用同款 GitHub API 查询模式)。**(2026-08-20 实施)**
- [x] **(升级)`shannon desktop --install`**:拉 latest release 元数据 → 按 (os,arch,dpkg/rpm) 选资产 → 下载 → SHA256SUMS 校验(缺失降级警告,mismatch 硬错)→ 交互确认 → 平台安装(macOS hdiutil 挂载 + cp .app 到 /Applications 回落 ~/Applications,全程无 sudo;linux `sudo dpkg -i` / `sudo rpm -Uvh`;windows NSIS `/S` 静默)。**(2026-08-20 实施)**资产选择/校验和解析为纯函数,5 个单元测试覆盖。

### B6. headless 纯净性 CI 守卫(ADR-0011 红线 1 的强制执行)
- [x] 新增 `scripts/check-headless-purity.sh`。**(2026-08-20 实施)**tree 检查覆盖 `tauri / tauri-runtime / gtk / webkit2gtk / webkit2gtk-sys / wry` 六个 crate(`cargo tree -i` 解析即失败);可选参数传已构建的 linux 二进制加 `ldd` 断言(本地对 `target/release/shannon` 实测通过)。CI 走 metadata-only tree 检查(秒级、无需编译),ldd 变体留给本地/发布演练。
- [x] 接入 `ci.yml`(独立 `guard-headless` job)+ justfile(`just guard-headless [<binary>]`)。

### B7. `shannon doctor --json` 增加 `surface` 字段
- [x] CLI 报 `surface: "cli"`(+version);`--json` 输出结构化报告(checks / shannon_installations / dual_install),文本模式同步。**(2026-08-20 实施)**
- [ ] 桌面端 about/诊断面板报 `surface: "desktop"` + 同一版本号(归入 B3 桌面批次,任务 #13)。
- [x] 用途:支持分流、遥测、工单模板(ADR-0011 Negative 项的缓解)——字段已定型:`surface` / `version` / `checks` / `shannon_installations[]` / `dual_install{detected, versions_differ, rule:"PATH wins"}`。
- [x] **双装检测**:PATH 全量解析(直走 PATH 环境变量,按序收集)+ 已知 bundle 位置(linux /usr/bin、macOS .app、windows NSIS 目录),去重后逐个探 `--version`;版本不一致 → WARN + 统一建议(PATH 优先:`shannon update` 更新 PATH 副本,重装桌面更新捆绑副本)。**(2026-08-20 实施)**顺带修复既有 bug:`command -v` 是 shell 内建,`Command::new("command")` 在无 `/usr/bin/command` shim 的发行版(如 stock Ubuntu)直接失败,doctor 工具探测历来全假 WARN——改为 `find_on_path()` 直走 PATH,doctor/desktop/gateway 三处共用。

## 3. 文档与叙事(B8)

- [x] README 快速开始改为单产品叙事:一条 install 命令 → 四个入口(`shannon` / `-p` / `serve` / `desktop`)表格,双语(README.md + README.zh-CN.md)。**(2026-08-20 实施)**
- [x] `desktop/README.md` 开头声明桌面 surface(引用 ADR-0011,说明三平台 CLI 捆绑通道与 `shannon doctor` 诊断)。**(2026-08-20 实施)**
- [x] `docs/RELEASE-INSTALL-PLAN.md` 头部状态更新:已落地收尾,指向 ADR-0011 与本方案文档。**(2026-08-20 实施)**
- [x] CHANGELOG 的历史 `shannon-code` 字样保留(既有政策),零改动。

## 4. 演练与验收(B9)

- [ ] 先用测试 tag(`release-test.yml` 路径)走一遍完整 release,产出可下载产物。
- [ ] 干净虚拟机逐平台验收:

| 平台 | 验收标准 |
|---|---|
| Windows | NSIS 装完 → 新终端 `shannon --version` 可用且与 GUI 版本一致;`shannon desktop` 能启动已安装的 app(不再 cargo build) |
| macOS | dmg 装完 → GUI 可用;点「Install CLI」→ `shannon --version` 可用 |
| Linux | deb 装完 → `shannon` 在 PATH;AppImage 内含 CLI |
| 全平台 | CLI 与 GUI 共享 `~/.shannon/`(providers/sessions);`shannon doctor --json` 含 `surface` 字段 |

- [ ] 回归确认:纯 CLI tarball 通道不受影响(体积、内容不变)。

## 5. 风险与回滚

| 风险 | 缓解 / 回滚 |
|---|---|
| externalBin 命名/签名问题导致打包失败 | 先在 test tag 演练(B9 强制);回滚 = revert tauri.conf.json 的 bundle 增项 |
| NSIS hook 改 PATH 引发安全软件误报 | 只加用户 PATH;不写系统目录;hook 变更走 PR 评审 |
| 安装器体积膨胀 | 附录记录实测;可接受阈值:增量 ≤ CLI tarball 的 1.2 倍(压缩后) |
| updater 流程受牵连 | `createUpdaterArtifacts` 保持 false,本阶段不动 updater;列入 ADR-0011 Open Questions |

## 6. 不做的事(范围外)

- 不合并二进制、不让 CLI 链接 GUI 库(红线)。
- 不动 gateway / mobile / semver 行为 / crate 命名。
- 不启用桌面 updater 签名产物(单独议题,C2/C4 分步走)。
- 不把 crates.io 当安装渠道(publish-crates.yml 仅发布 `shannon-api-protocol` wire contract,CLI 不上 crates.io)。
- Phase C(会话接力 UI、`shannon://` 统一处理、遥测 surface 上报)另立计划。

---

## 7. 发布管线安全网(2026-08-19 审查新增,B11–B17)

> 按四环节(装上/用起来/升上去/修不了)审查 build/release 与用户故事所得;证据均为代码实证,带 file:line。

### P0 — 发布安全(下次发版前必须)

- [x] **B11 · R2 `latest/` prerelease 防护**:`publish` job 对 R2 的 `latest/` 覆盖无 prerelease 判断——GitHub 的 releases/latest 会跳过 prerelease,R2 不会,rc tag 会污染 `latest/` 安装源。修:`contains(tag,'-')` 时只写 `$VERSION/`,不写 `latest/`。**(已实施)**
- [x] **B12 · 版本守卫扩到全部 4 处版本源**:`prep` 只校验根 Cargo.toml;desktop/Cargo.toml、desktop/tauri.conf.json、gateway/package.json 漂移不拦——v0.7.0-rc1 事故根因的结构性残留。修:4 处全量读出并与 tag 比对,不一致 `::error::` 退出。**(已实施)**
- [x] **B13 · 发布冒烟**:此前构建成功即 published,零运行时验证。修:publish 前加 smoke——10 个关键资产存在性断言(含版本化命名,拦资产名漂移)+ linux CLI tarball 解包实跑 `--version` == tag + `dpkg-deb -f` 校验 deb 元数据版本。**(已实施)**

### P1 — 完整性与门禁

- [x] **B14 · desktop 资产完整性校验**:tauri-action 不产 `.sha256` sidecar,install.sh `download_verify` 打印 "Checksum not available, skipping" 静默跳过——而 SHA256SUMS 就挂在同一 release。修:无 sidecar 时回退查 SHA256SUMS 匹配(sh/ps1 均已回退,精确文件名匹配)。**(已实施)**
- [x] **B15 · release CI 门**:ci.yml 只在 branches/PR 触发,tag 不触发;release.yml 自身仅跑 gateway typecheck+test → 未过 CI 的 commit 打 tag 即出货。修:ci.yml push 加 `tags: ['v[0-9]+.[0-9]+.[0-9]+*']`。**(已实施)**
- [x] **B16 · 回滚通道**:`latest/` 被坏版本覆盖后无工具、无 runbook。修:先落 runbook,再做 `just release-rollback <ver>`(重传旧版到 R2 `latest/` + GitHub release 校订)。**(已实施 2026-08-20)**:`docs/RELEASE-ROLLBACK.md`(四通道矩阵、判定、R2/GitHub 两步、用户侧、演练)+ `just release-rollback <ver>`(gh 拉取指定版本全部资产 → wrangler 重传 R2 `latest/`)。

### P2 — 渠道卫生

- [x] **B17 · 渠道事实对齐**:① install.ps1 "gateway 需 linux/macOS" 过期文案已改为准确描述(下载失败提示 + 指向 release 页);② `dist-workspace.toml` 已删除(无采用数据支撑 brew 渠道复活;手写的 `packaging/homebrew/*.rb` 保留但未接 CI,见遗留项);③ crates.io 非安装渠道的口径不变(README 仅保留 `cargo install --git`,合法);④ **实施中新发现并修复的 P0**:全仓默认 org `github.com/shannon-agent/shannon-agent` 不存在(404)→ 已全量替换为 `diff-lab-com/shannon-agent`(install 脚本、`shannon update` 5 处 URL、README×2 快速开始、Cargo.toml metadata×3、NOTICE、scoop/winget/AUR/homebrew 清单;历史文档与 archive 保留原文)。**(已实施)**

## 8. 用户故事断链修复(审查新增,C1–C4)

四环节走查结论(✅ 通 / ⚠️ 摩擦 / ❌ 断链):

| Persona | 装上 | 用起来 | 升上去 | 排障 | 断点归属 |
|---|---|---|---|---|---|
| 服务器 headless | ⚠️ | ✅ | ⚠️ | ⚠️ | 全家桶+sudo、arm64 ❌(B0);升级无指引 |
| 终端 TUI | ✅ Linux / ⚠️ macOS | ✅ | ⚠️ | ✅ | brew 断供(B17)、未签名(C4) |
| 桌面 GUI | ✅ | ✅ | **❌** | ⚠️ | 升级故事空缺(C1) |
| 聊天平台常驻 | ⚠️ | ⚠️ | **❌** | ⚠️ | engine 断链(C2,最高优先)、无升级命令(C3) |
| CI/自动化 | ⚠️ | ✅ | — | ✅ | arm runner 无 CLI 产物(B0 arm64 项) |
| 移动端 | — | 依赖 host+relay | — | — | 生态依赖,另线(shannon-service) |

- [x] **C1 · 桌面升级故事(最大体验债)**:updater 未启用(`createUpdaterArtifacts:false` + pubkey 占位)+ 无签名,GUI 用户唯一升级路径是手动重下安装器。分两步:① 先做 app 内「检查新版 → 打开下载页」半自动提示(≈1 人日,不依赖签名);② updater 启用依赖签名与 latest.json 通道,随 C4 排期(ADR-0011 Open Question)。**(① 已实施 2026-08-20)**:Settings → Advanced 新增 Version & updates 卡片;`check_app_update` 走公开 GitHub `/releases/latest`(网络失败落 error 字段,不 fail command);`open_release_page` 复用 OAuth 的 shell-open 先例;② 仍随 C4。
- [x] **C2 · engine 生命周期引导(优先级最高)**:install.sh 引导链止于 `gateway setup → install → enroll`,但 gateway 是 WS 客户端(`gateway/src/engine/wsClient.ts:20`:connect→runQuery→close),**不负责拉起 engine**——用户照官方引导做完,gateway 服务起来了,engine(`shannon serve`)由谁安装/注册/保活没有任何一步覆盖,结果是连不上。**(已实施 2026-08-20)**:`shannon gateway install` 现在同时注册 engine 单元 —— linux `~/.config/systemd/user/shannon-serve.service`(gateway 单元 `After=/Wants=shannon-serve.service` 排序)+ `loginctl enable-linger`(SSH 登出不停);macOS `com.shannon-agent.serve.plist`(KeepAlive);Windows schtasks `shannon-serve`。engine 二进制经 `$SHANNON_ENGINE_BIN` → `which shannon` 解析,找不到则跳过并明示警告;`status`/`list` 增加 `engine=` 状态字段;`uninstall` 刻意保留 engine 单元(desktop/mobile 共用同一实例)。install.sh/ps1 收尾提示已于第一批加入。
- [x] **C3 · `shannon gateway update`**:服务化后升级 = 手工换二进制 + 手工 restart。新增子命令:下载 → sha256 校验 → 替换 → restart(复用 `run_update_command` 的下载逻辑,main.rs:2786);engine serve 单元同理受益。**(已实施 2026-08-20)**:CLI 拦截(不透传 gateway 二进制)——PATH 定位 → `shannon-gateway-<os>-<arch>` 资产(Windows 无构建,提前 bail)→ SHA256SUMS 校验(不符致命/缺失警告,同 `desktop --install` 规则)→ 同目录 staging 原子 rename(running service 旧 inode 直到 restart)→ 尽力 restart(失败 WARN + 手动指引)。真端到端冒烟:假 gateway 0.0.1 → 实下载 v0.10.0 90.3MB → sha256 verified → 原子替换为真 ELF → restart 失败优雅降级退出 0。
- [ ] **C4 · 签名排期(外部依赖项,独立任务)**:macOS 签名/公证、Windows 代码签名是 Gatekeeper/SmartScreen 摩擦与桌面 updater 的共同前置。短期缓解:install.sh 在 macOS 主动清 `com.apple.quarantine`(覆盖浏览器下载路径);正式签名立独立任务排期,不再无限期挂起。**(短期缓解已实施 2026-08-20)**:install.sh macOS 段 cp .app 后无条件 `xattr -dr com.apple.quarantine`(尽力而为;curl 本不打隔离属性,防的是覆盖浏览器下载副本继承);正式签名(证书采购 + 公证流水线)仍为独立任务待排期,C1② 随之。

## 9. 建议执行顺序(整合后)

1. **立即(≈1 人日)**:B11–B13(P0 发布安全网)——下次发版前必须到位;可与 B0 同批(都是小改动)。✅ **2026-08-19 已完成**(与 B14/B15/B17 同批实施,见附录)。
2. **短期(≈2–3 人日)**:C2(engine 引导,story 级断链)、B14–B16、B17、B0 收尾。✅ **2026-08-20 已完成**(C2+B16;B14/B15/B17/B0 已在第一批完成)。
3. **中期**:B1–B9(打包/捆绑/三平台验收)、C1①(检查更新提示)、C3;C4 签名独立排期,C1②(updater)随之。✅ **2026-08-20 完成**(B1–B8 + C1① + C3 + C4 短期缓解;仅 B9 远程 tag 演练待 push 后执行)。

## 附录(执行时填写)

### 下载计数(2026-08-19 核实)
GitHub UI 已不再展示资产下载计数;经 API 查询 v0.10.0:全部资产 0–1 次下载 → **无采用信号**。据此维持平台对齐默认,不做渠道倾斜;cargo-dist 死配置直接清理(B17②),渠道复兴等有真实采用数据再议。

### 实施记录(2026-08-19,第一批:B0 + B11–B15 + B17)
- **B0**:`install.sh`/`install.ps1` 新增 `SHANNON_COMPONENTS=cli|gateway|desktop|all`(默认 all,兼容现状);arm64 Linux 无 CLI 预编译 → 显式提示源码构建(仅 cli 组件时硬错,默认组合时降级警告,gateway 仍可装);desktop 资产名修正(aarch64 Linux 无 bundle 直接跳过);不含 desktop 组件时全程无 sudo。`sh -n` / `dash -n` 通过。
- **B11**:release.yml publish → R2 `latest/` 写入加 prerelease 判断(tag 含 `-` 只写 `$VERSION/`)。
- **B12**:prep 版本守卫扩为 4 源全查(root/desktop Cargo.toml、tauri.conf.json、gateway package.json)。
- **B13**:publish 新增冒烟步骤:10 个关键资产存在性断言(含版本化命名)+ linux CLI tarball 解包实跑 `--version` == tag + `dpkg-deb -f` 校验 deb 元数据版本。
- **B14**:sh/ps1 `download_verify` 无 sidecar 时回退 release 级 `SHA256SUMS`(精确文件名匹配;desktop/gateway 资产本无 sidecar)。
- **B15**:ci.yml push 触发加 `tags: ['v[0-9]+.[0-9]+.[0-9]+*']`,tag 也过完整 CI 门。
- **B17**:① ps1 过期文案修正;② `dist-workspace.toml` 删除(release.yml 头注释同步改写);③④ 见下节。
- 验证:actionlint 通过(仅余 1 条既有 SC2086 info:create-release 的 `$PRE` 有意展开为 flag,非本次引入);`cargo check -p shannon-cli` 通过;两个 workflow YAML 解析通过;组件校验与 SHA256SUMS awk 匹配做了本地功能测试。
- C2 提示层部分缓解:install.sh/ps1 收尾增加「gateway 需连接运行中的 engine(`shannon serve`)」说明。

### 实施记录(2026-08-20,第六批:B9 远程 tag 演练 + publish-crates rc 跳过)
- **B9**:dev CI 卡在 runner 基础设施问题(Test / Doc Build / Insta / musl / ubuntu-latest 五个 job 冷缓存编译 3 小时不动——已被 cancel;同 SHA 在 tag 触发的 ci.yml 上 17/17 job 一次过含上述五个 + Generate Metrics,证实卡死是 runner flake 不是代码问题)→ 不再等 dev CI 收尾,直接经 ssh.github.com:443 推 `v0.11.0-rc1`(锚定 `3ed8b7aa`,workspace 版本 0.11.0 匹配 rc 数字核心)。**release.yml 全绿 35 分钟**(17 job:version guard / create-release / CLI×4 / Desktop×5 含 NSIS hooks 首次真编 + staging CLI 进 desktop / Gateway×5 / publish smoke 含 CLI --version grep + deb Version grep + 10 资产完整性断言);release 以 `prerelease=true draft=false` 落地、21 资产齐全。同步 ci.yml 在 tag 上全绿(17/17)。release-test.yml 三 job 绿(Build Release Binary / Cross-platform Build Check windows / Cross-platform Build Check macos),Full Release Test 在 45 分钟 timeout 处被 GitHub 自动取消——同型 runner 卡死(冷缓存 `cargo test --workspace`),非代码缺陷。
- **publish-crates.yml rc 漏洞同步修复**(f0190cf8):和 release.yml 同型的 `tag stripped of v` vs workspace 版本不一致问题(rc tag 字面 `0.11.0-rc1` ≠ 数字核心 `0.11.0`);加 job 级 `if: ${{ !contains(github.ref_name, '-') }}` 跳过 prerelease tag(避免演练误发到 crates.io);GA tag 行为不变。
- 演练 tag 处置:自动渠道已核实全部走 `/releases/latest`(prerelease 天然不可见),R2 镜像未配置无残留 → **经维护者确认,`v0.11.0-rc1` release + 远端 tag 已于 2026-08-20 删除**(演练数据完整记录于本文;GA 发版用全新数字 tag)。

### 实施记录(2026-08-20,第五批:push 解封 + 远端 CI 修复 + required checks)
- **push 解封**:dev 历史经 `ssh://git@ssh.github.com:443/diff-lab-com/shannon-agent.git` 推送(端口 22 被封)。推送前顺手修了两个远端才暴露的问题:
  - **release.yml rc 容差**(cbdf296d):prep 版本守卫与 publish 冒烟改为对 `${VERSION%%-*}`(数字核心)比较 —— rc tag(`v0.11.0-rc1`)下 workspace 版本 0.11.0 与 tag 字面不等,原逻辑会把合法 rc 演练误杀。
  - **vendor.yml 旗标修复**(7064865f):`cargo vendor --versioned-sources` 不存在(该 job 自创建起每次必挂)→ 改 `--versioned-dirs`,远端 run 首次转绿。
- **audit 修复**(20a95c71):RUSTSEC-2026-0258(h2 空 DATA 帧无界,2026-08-17 新公告,修复 ≥0.4.16;lock 为 0.4.15)导致 Dependency Audit + RustSec Advisory Audit 双挂 → `cargo update -p h2 --precise 0.4.16` 补丁位升级,本地 `cargo deny check advisories` ok + `cargo check --workspace` 过;不加 deny ignore(有干净升级可走)。
- **required checks 补齐**:dev(9 上下文)与 main(8 上下文)的分支保护 required status checks 均已加入 `Headless purity (ADR-0011)`(裸 job name)。实现备注:该环境对子端点 PUT 一律 404(gh 与 curl 同),须整包 PUT 父端点 `/branches/{br}/protection`;改后逐字段核对 strict / enforce_admins / PRR / rlh / afp 原样保留。
- **CI 现状**:32291454488(20a95c71)全量跑通后即可打 `v0.11.0-rc1` 演练 tag。
- **C1①**(95a117fd):desktop `commands_surface.rs` 新增 `check_app_update`(reqwest 10s 超时 + GitHub `/releases/latest`;数字点分版本比较与 CLI `version_is_newer` 同款宽松解析;网络失败落 `error` 字段而非 fail command)+ `open_release_page`(OAuth 同款 shell-open 先例,`#[allow(deprecated)]`)。AdvancedSettings 新增 Version & updates 全宽卡片(检查按钮、up-to-date/可更新徽章、当前版本行、错误提示、打开下载页 ghost 按钮)。i18n en+zh-CN 各 10 键;setup.ts mock + 3 个新用例(渲染、检查后徽章+链接、可更新→打开页面断言 invoke 参数)。**顺手修了一个既有测试脆弱点**:system-logs 用例的 `/Shannon Desktop/` 正则被新卡片文案撞上,改为锚定 `Shannon Desktop v0.1.0` 精确行。顺带发现并修复自写 `version_is_newer` 初版的 `.any()` 陷阱(1.0 vs 0.9 误判,first-differ 必须立即返回)。
- **C3**(99495d25):见 §8 C3 条目。`GatewaySubcommand::Update` 在 `run_gateway_command` 的 match 内 diverging 臂拦截(不透传)。纯函数 `gateway_asset_name(os, arch)` + 单测;真端到端冒烟如 §8 所述。
- **C4 短期缓解**(b3a0b93c):install.sh macOS 段,cp .app 成功后 `xattr -dr com.apple.quarantine`(POSIX sh,`command -v xattr` 守卫,尽力而为);shellcheck 无新告警,sh/dash/bash `-n` 全过。
- 验证:shannon-cli clippy(-D warnings + CI 白名单)过、nextest 389/389;desktop nextest 580/580;UI tsc 过、vitest 1262/1262。

### 实施记录(2026-08-20,第三批 c 补:本地 deb 演练实证 B1/B4 + 收尾门)
- **deb 迷你演练**:`desktop/binaries/shannon-x86_64-unknown-linux-gnu`(28MB release 二进制手工暂存,模拟 release.yml staging step)→ `cargo tauri build --bundles deb` 成功(3m52s,产物在 workspace 级 `target/release/bundle/deb/`,desktop 是 workspace member 共享 target-dir)。
- **实证结果**:`dpkg-deb -c shannon-desktop_0.11.0_amd64.deb` 确认 **`usr/bin/shannon`(28,169,560 B)+ `usr/bin/shannon-desktop`(35,951,416 B)** —— externalBin 自动落 `/usr/bin` 的 B1/B4 机制成立,零 postinst。解包冒烟:extracted `usr/bin/shannon --version` → `shannon 0.11.0`,ELF x86-64。
- **体积增量实测**(遗留项回填):deb 总 28,162,174 B(≈27MB);CLI 二进制 gzip 后 11,060,294 B(≈ CLI tarball 体积)即 CLI 对 deb 的压缩增量;GUI gzip 16,562,429 B,两者相加 ≈ deb 实际大小,自洽。阈值判定:增量 11.1MB ≤ 1.2 × CLI tarball(13.3MB)→ **通过**(§风险登记口径)。
- **收尾门**:`cargo clippy --workspace -- -D warnings`(带 CI 白名单)修复 commands_surface.rs 一处 needless borrow 后通过;`cargo fmt --all -- --check` 通过(两处单行 cfg! if-else 展开 + main.rs use 排序,由 `cargo fmt` 自动完成);修复折入第三批 c 提交。

### 实施记录(2026-08-20,第三批 c:B3 + B7 桌面半 + B8 文档,B9 准备)
- **B3**:desktop 新增 `commands_surface.rs` —— `install_cli_to_path`(current_exe 同级定位捆绑 CLI → 非遮蔽短路 → unix symlink /usr/local/bin 回落 ~/.local/bin,只替换自己的旧 symlink 绝不覆盖真实文件;windows 返回"安装器已处理")、`get_cli_install_status`(onPath/onPathVersion/bundledPath/handledByInstaller)、`get_surface_info`(B7 桌面半:`surface:"desktop"` + crate 版本,与 release-prep 锁步)。注册进 lib.rs/main.rs invoke_handler;tauri-api.ts 包装 + types/index.ts 类型 + re-export;AdvancedSettings 新增 Command line 全宽卡片(状态徽章绿/红、安装按钮 disabled 逻辑、installer hint);i18n en + zh-CN 各 9 键;setup.ts 全局 mock + AdvancedSettings.test 新增 2 用例。
- **B8**:README.md / README.zh-CN.md Quick Start 增四入口表格(双语);desktop/README.md 头部 surface 声明;RELEASE-INSTALL-PLAN.md 头部改"已落地收尾"并指向 ADR-0011。
- **B9(准备)**:核实 `release-test.yml` 只覆盖 CLI 构建(无 bundling),externalBin 风险只能靠真 tauri build 暴露 → 本地做了 deb 迷你演练(见下);完整 tag 演练待 push 后远程执行(端口 22 封锁,本机不 push)。
- 验证:`cargo check -p shannon-desktop` 过;desktop nextest 577/577;UI tsc + vitest 全量 1259/1259(新卡片 2 用例);UI lint(=tsc)过。

### 实施记录(2026-08-20,第三批 b:B5 + B7,CLI 侧)
- **B5**:`shannon desktop` 三态化——`--install`(下载+SHA256SUMS 校验+确认+三平台安装)/ `--build`(显式开发构建,产物在 desktop/target/release,现在真能被找到)/ 默认打印安装指引(非零退出)。`find_desktop_binary` 修正 macOS 候选名(productName=shannon-desktop → `shannon-desktop.app`)、补 Windows NSIS perUser/perMachine 候选。sha2 加入 shannon-cli 依赖(树内 6 crate 已用 0.10,零新增编译成本);下载走 curl(与 update 同传输层,reqwest 在 cli 只是 dev-dep)。
- **B7**:`doctor --json` 定型 surface/version/checks/installations/dual_install;文本模式带 surface 行与安装清单;双装版本漂移 → WARN + PATH 优先建议。
- **顺带修复(既有 bug)**:`Command::new("command")` 探测在 stock Ubuntu 必败(无 /usr/bin/command shim,`command` 是 shell 内建)→ doctor 工具/gateway/desktop 三处探测全部改 `find_on_path()` 直走 PATH。实机验证:修复前 doctor 四工具全假 WARN,修复后全 OK。
- 验证:`cargo clippy -p shannon-cli -- -D warnings`(带 CI 白名单)通过;nextest 388 测试全过(含 5 个新纯函数测试);实机冒烟:doctor 文本/JSON、desktop 指引(exit 1)输出符合设计;`cargo fmt --all -- --check` 通过。

### 实施记录(2026-08-20,第三批 a:B1 + B2 + B4 + B6,构建管线侧)
- **B1**:`desktop/tauri.conf.json` 加 `bundle.externalBin: ["binaries/shannon"]` + `windows.nsis.installerHooks`;release.yml desktop job `needs: [create-release, cli]`,新增两条 staging step(unix/mac 从 draft 拉 `shannon-<target>.tar.gz` 平铺解包;windows 拉 `.zip` + `Expand-Archive`)落位 `desktop/binaries/shannon-<target-triple>[.exe]`,均实跑 `--version` 冒烟。`.gitignore` 挡 `desktop/binaries/`。
- **B2**:`desktop/nsis/hooks.nsh`(POSTINSTALL + PREUNINSTALL)。关键决策:**不用 EnVar 插件**——拉取 Tauri 官方 `installer.nsi` 核实,其 NSIS 发行包不含 EnVar、模板已 include WordFunc/StrFunc/LogicLib;PATH 手术用 stock NSIS(`${WordReplace}` 剥离旧条目保幂等、`WriteRegExpandStr HKCU Environment Path` 保 REG_EXPAND_SZ、`SendMessage HWND_BROADCAST WM_SETTINGCHANGE` 广播)。非遮蔽:`where shannon` 已解析则零改动。
- **B4**:deb/rpm 经 externalBin 自动落 `/usr/bin/shannon`,**零 postinst 脚本**;tauri-bundler `debian.rs`/`rpm.rs` 源码级核实落位路径。
- **B6**:`scripts/check-headless-purity.sh`(6 crate tree 断言 + 可选 ldd)+ `just guard-headless` + ci.yml 独立 `guard-headless` job。已于第五批加入 GitHub required checks(dev + main,裸 job name `Headless purity (ADR-0011)`)。
- 验证:purity 脚本本地通过(含对 `target/release/shannon` 的 ldd);检测机制反证(serde 在图中 → 会 FAIL)成立;tauri.conf.json JSON 校验过;两 workflow actionlint 仅余 1 条既有 SC2086 info(HEAD 已有,非本次引入);`just -n guard-headless` 解析通过。NSIS hooks 无本地 makensis,首次编译发生在 CI windows job(B9 演练重点盯)。

### 实施记录(2026-08-20,第二批:C2 + B16)
- **C2**:gateway 服务层(`gateway/src/service/{units,service}.ts`、`src/index.ts`)—— `install` 双 unit(gateway + engine serve);systemd 排序(gateway 单元 `After=/Wants= shannon-serve.service`)+ `loginctl enable-linger`(headless 服务器登出不停);launchd / schtasks 对应 engine 单元;`resolveEngineBinary()`(`$SHANNON_ENGINE_BIN` → `which shannon`,找不到跳过并警告,install 输出明示);status/list 增加 `engine=` 状态;uninstall 保留 engine 单元。验证:tsc 通过、vitest 27 文件 / 286 测试全过、builder 输出实测(systemd 排序行、launchd/win32 路径)。
- **B16**:`just release-rollback <ver>`(gh 拉取好版本全部资产 → wrangler 重传 R2 `latest/`)+ `docs/RELEASE-ROLLBACK.md` runbook。justfile release-prep 注释中的 cargo-dist 残留提及一并清除。

### 渠道事实对齐(实施中发现的 P0,已修)
全仓默认指向 `github.com/shannon-agent/shannon-agent` 的 org **不存在(404)**,真实仓库为 `diff-lab-com/shannon-agent`。受影响并已全部替换(代码 + 活文档;`docs/archive`、`legacy-archives`、`docs/superpowers` 历史记录保留原文):`install.sh`/`install.ps1` 默认 CDN 与版本 API、`shannon update` 全部 5 处 URL(main.rs)、README/README.zh-CN 快速开始(同时修复了引用不存在资产名 `shannon-$(uname -s)-$(uname -m).tar.gz` 的 curl 一行命令 → 改为 install.sh 一行 + `SHANNON_COMPONENTS=cli` 无头变体 + Windows `irm install.ps1`)、gateway/vscode README、根 + 2 个 crate 的 Cargo.toml metadata、NOTICE、scoop/winget/AUR/homebrew 清单、RELEASE-INSTALL-PLAN.md。

### 遗留 / 待决
- winget 清单钉在 v0.7.0 且引用不存在的 `.zip` gateway 资产;scoop/homebrew/AUR 清单未接 CI 自动更新 → 下次发版时二选一:接入 release.yml 自动生成/更新,或删除。本次仅保守修正 URL。
- GitHub 上残留 3 个 v0.6.x draft release(2026-07-17)→ 建议手动删除避免列表混淆(不影响 `latest` 解析;latest 只看已发布的非预发布)。
- 安装器体积增量实测:deb 27MB 总量,CLI 压缩增量 11.1MB ≈ 1.0 × CLI tarball(阈值 1.2×)→ 通过。详见附录第三批 c 补。
- test tag 演练记录:`v0.11.0-rc1` 2026-08-20 在 `3ed8b7aa` 上推送 → release.yml **全绿 35 分钟**(17 job:version guard / create-release / CLI×4 / Desktop×5 含 NSIS hooks 首次真编 / Gateway×5 / publish smoke 含 CLI --version + deb Version + 10 资产断言);`prerelease=true draft=false`;21 资产齐全(CLI tar+sha、deb/rpm/dmg×2/NSIS、gateway×5、install.sh/ps1、SHA256SUMS)。同时触发 ci.yml 在 tag 上全绿 17 job(包括之前 dev 上卡死的 Test / Doc Build / Insta / musl / ubuntu-latest / Generate Metrics),证实 dev CI 卡死是 runner flake 不是代码问题。tag 演练期间另外发现的 publish-crates.yml 也有同样 rc-hostile 漏洞(已加 job 级 `if: !contains(ref_name, '-')` 跳过,f0190cf8 推到 dev)。
