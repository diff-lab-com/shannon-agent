# 清理计划 + 缓期项实施调研(待审核,2026-10-09)

**基线**: dev @ `d83123f88`(= origin/dev,PR #348 合并后)。盘点与调研均为只读,本文档两项内容(清理计划、实施建议)**均待你审核后才执行**。

---

## 第一部分:PR 合并结论

**无需开新 PR。** 全量盘点结果:所有含独有内容的本地分支均已合并进 origin/dev(21 个本地分支中 19 个已合并,含本轮全部 wave/fix/feat 分支;远端 GitHub 侧 14 个已删分支的 tracking ref 已随 fetch --prune 清理)。

唯一的图论未合并分支是 **`main` ≡ `origin/main` ≡ `feat/trust-kind-scope` @ `54ab39680`**:经树级 diff 验证,main 相对 merge-base **零独有内容**(三个 dot-diff 全空,`chat-nightly.yml` 在 dev 上原样存在)——它只是一根落后 dev 91 个提交的**过期 release 指针**。

> ⚠️ 决策项 D-1(需要你拍板):是否开一个 `dev → main` 的 PR 刷新 release 指针?这属于发版流程决策(仓库惯例是 main 承接发版),未获授权前不动。

---

## 第二部分:清理计划(审核后执行)

### C 组 · 建议清理(零风险,已合并 + worktree 干净)

| 对象 | 动作 |
|---|---|
| 5 个 worktree:`browser-computer-use`、`full-review-improvements`、`mobile-batch-b`、`secret-guard-flip`、`ui-redesign-v2` | `git worktree remove`(全部干净、分支已合并) |
| 13 个已合并本地分支:`fix/api-protocol-codegen-drift`、`chore/full-review-improvements`、`feat/agent-state-push`、`feat/secret-guard-redact-default`、`feat/d1-kit-and-followups`、`design/ui-redesign-v2-followup`、`design/ui-redesign-v2-mockups`、`feat/computer-use-browser-remote-improvements`、`feat/inbox-approval-and-statusbar`、`feat/mobile-batch-b`、`feat/r1-followup-batch`、`feat/ui-v2-backlog`、`fix/ui-review-theme-parity` | `git branch -d`(全部已合并,-d 安全) |
| 5 个 wave/* 分支(`wave/chat-parity` 等,从未推远端) | `git branch -d`(已通过 #345 合并) |
| `fix/ui-review-theme-parity` 若远端残留 | 远端已删,无需处理 |

### K 组 · 保留(有真实未提交工作,不在本次清理范围)

| 对象 | 内容 | 建议 |
|---|---|---|
| `.worktrees/trust-kind-scope`(脏) | **未提交 WIP:+1105/−42,15 个文件 + 2 个新文件**(shannon-engine trust/guard/agent_loop、api-protocol、gateway/mobile protocol/crypto + `trust.rs`/`trust.test.ts`)——是某个进行中的 trust 机制工作,**不属于任何分支** | 决策项 D-2:让原会话继续,或先把 WIP 提交到 `wip/trust-kind-scope` 分支保全后再决定 |
| `stash@{0}`(主工作区) | protocol-asset 实验改动(上轮同步 dev 前 stash) | 决策项 D-3:drop,或继续保留 |
| 主工作区 12 个 untracked 文档 | 6 份 `docs/research/*.md` 竞品/走查文档、2 份 `docs/plans/*.md`、`task_plan.md`、`findings-journey.md`、`gateway/cross-repo-check.mts`、`.zcodeignore`、空文件 `0` | 决策项 D-4:调研类文档建议择优入库(docs/research 有先例),会话产物(task_plan/findings-journey/空文件 `0`)建议删 |
| `main` 过期指针 | 见第一部分 | 决策项 D-1 |

执行方式(审核通过后):一条脚本顺序跑,`-d` 只删已合并分支(有任何未合并会被 git 拒绝,天然防呆);删前再跑一次 merge 校验并输出清单留档。

---

## 第三部分:缓期项实施调研(8 项,按可实施性重排)

### 🔴 先修(发现的一致性 bug,建议插入下一批最前)

**R-0 · secret_guard D1 翻转"半落地"**(S,一行修复+钉住测试)
PR #342 宣称并已在 CHANGELOG 向用户宣告"unset 默认 redact",但实际:`secret_guard.rs:905` 的 `is_some()` 真空臂使 (None,None) 配置落到 `:914 install_mode(Audit)`;`unified_config.rs:963-964` 钉住测试仍 pin Audit(D1 kit §2.4 明确要求同步翻转,被漏掉)。**当前 unset 用户的实际生效模式与文档相悖。**落地出站防线卡之前必须先修。

### 🟢 S 级(各 ≤1 天,可打包成一批)

| 项 | 结论(证据) | 建议形态 |
|---|---|---|
| 启动恢复会话开关 | 能力已存在且**默认 always-on**(main.rs:718-721 + `config.open_session_windows` 持久化 + 重开自愈),只缺用户可见开关 | 真开关 gate main.rs:721 + config 字段 |
| 启动时更新检查 | check-only 能力两套现成(commands_surface.rs:206-310 GitHub Releases比对;core updater.rs 24h 节流),缺启动触发 | 真开关,诚实命名「仅提示,不自动安装」;自动安装=签名基建,不做 |
| 注入扫描徽章 | 现状仅安装前目录/README advisory 扫描(~22 静态模式),**出站维度后端完全不存在**(出站版=L,不做) | 开关改名「安装时注入扫描」+说明;可选 M:把 Dangerous 升级为安装确认门(ConfirmationLevel 管线现成) |
| 签名校验徽章 | 现状是**自声明发布者标识非密码学校验**(代码内自认,Ed25519 是独立 P6 工作),且未接安装链 | 诚实徽章说明现状;确认门强化 M;真签名 L 不做 |
| 使命实体(config 键版) | `strategic_focus` 已有 config 键先例(strategic_focus→configure 专用臂→config-updated);runs 已带 cost_usd;任务已有 dueDate | 新键 `mission{name,budget_usd,deadline_ts,task_ids}`(松耦合);正式表 M-L 后置,可无损升表 |
| 任务看板金额 | **join 免费拿**:agent 会话产的任务目录名= session uuid,成本=`spent_for_session(team)`(ledger 现成) | `list_tasks` 投影 cost_usd;手建任务(`<adhoc>/`)不显示(绝不估);tooltip 注明 ledger 轮转口径 |
| 上下文峰值 | 线上 `max_tokens` 从未填充(百分比**永不显示**);turn/end 已有逐 turn 用量 | 两字段:UsagePayload 加 `context_total`(两发射点)+SessionIndex 加 `max_context_tokens`;UI 换算 % 并 Math.max 峰值 |

### 🟡 M 级

| 项 | 结论 | 方案 |
|---|---|---|
| 全量会话导出 | zip 2 已在树内 + 诊断包打 zip 先例(commands_diagnostics.rs:106-132);权威数据=`sessions/<uuid>/events.jsonl`+meta/index | 新命令 `export_all_sessions(dest)`:原样打包容器+manifest(时间/版本/会话清单);markdown 全量版后置 |
| 开机自启 | 完全没有(tauri-plugin-autostart 全套缺失) | 插件接入+ACL+config 开关,M |
| 执行模式收敛(缓期 #11) | **真合并不可行**:profile→approval_mode 非单射(strict/balanced 同映 ask),合并必有信息损失 | 双控件保留+按命名 doc §7.2 收敛:预设改名「规则预设」移设置页/带 tooltip、解除 activate 时的 approval_mode 静默覆写(automation_commands.rs:650-652)、词表统一收口 approvalModes.ts。S~M |

### ⚪ 决策后动

- **正式 missions 表**(config 键版的升级路径,多使命/历史归档需求出现时)
- **真·出站注入扫描** / **真密码学签名**(Ed25519+密钥固定+安装链强制,均 L,独立立项)
- **自动安装更新**(签名基建,ADR-0011/C4,L)

### 建议的下一批打包(待你圈选)

**批 1(建议 immediately)**:R-0 secret_guard 一行修复 + 启动恢复开关 + 更新检查开关 + 任务看板金额 + 上下文峰值 —— 全 S,含一个用户告知级 bug 修复
**批 2**:使命实体 config 键版 + 注入扫描/签名校验诚实徽章(权限页出站防线卡落地)+ 全量导出
**批 3**:执行模式收敛(S~M,牵 composer/设置/e2e)+ 开机自启(M)

---

*证据明细:三个调研代理报告全文(引擎能力对账 / 数据契约 / 合并状态盘点)已核校;行号以 dev @ d83123f88 为准。*

---

## 执行记录(2026-10-09,C 组经用户批准后执行)

**已执行**:5 个 worktree 全部 `git worktree remove` 成功(browser-computer-use / full-review-improvements / mobile-batch-b / secret-guard-flip / ui-redesign-v2,无一脏拒);13 个已合并分支 + 5 个 wave/* 分支全部 `git branch -d` 成功(chat-parity / gallery-parity / onboard-parity / settings-parity / workbench-parity,无一拒绝、未用 `-D`、零强删)。**现状**:worktree 仅剩主工作区 + `.worktrees/trust-kind-scope`;本地分支仅剩 `dev` / `main` / `feat/trust-kind-scope`;dev 仍 @ `d83123f88`(= origin/dev),全程零提交。

**K 组前提修正(执行时逐项盘点发现,建议以本表为准重新决策)**:

| 项 | 原前提 | 修正后事实 |
|---|---|---|
| D-1 main | 过期指针,零独有内容 | 提交图上有 4 个 dev 没有的提交(树内容仍零独有,chat-nightly.yml 已在 dev 原样存在);本地 main ≡ origin/main,当前**无需刷新也不可 fast-forward**。发版为 **tag 驱动**(release.yml / publish-crates.yml 监听 `v*` tag),推 main 只触发 ci.yml / deploy-website.yml / vendor.yml,**不会误触发布** |
| D-2 trust WIP | 未提交 +1105/−42、15 文件 | 真实脏 WIP 仅 **2 文件 +9/−2**(desktop/Cargo.toml 给 tauri 可选依赖加 `protocol-asset` feature + Cargo.lock 相应 7 行);+1105/−42 是分支 HEAD(18be45c50)对 dev 的差异——trust 机制内容**已提交**在 `feat/trust-kind-scope` 上,无丢失风险 |
| D-3 stash@{0} | protocol-asset 实验改动 | **已不存在**(`git stash list` 为空);内容疑即 D-2 的 2 个文件(曾被 pop 进 trust worktree),此项可关闭 |
| D-4 untracked | 12 项 | 实为 13 项(含本文档自身)。建议删 4:`0`(0 字节空文件)、`task_plan.md`、`findings-journey.md`(均为 2026-10-01 走查的根目录草稿/索引,正本已在 docs/research)、`gateway/cross-repo-check.mts`(一次性 O3 live-check 脚本,绝对路径导入、不可移植);建议入库 9:docs/research ×5、docs/plans ×2、docs/reviews/本文档、`.zcodeignore`(仓库级工具配置,性质同 .gitignore) |

## 执行记录(二)(2026-10-09,K 组决策 + 缓期批 1 落地)

- **D-1 ✅**:main 发现已被 PR #351(N3 类别信任)直合(dev 侧缺),D-1 由「指针刷新」升级为三步:#352 批 1 合并 → #354 回灌(冲突并集解:CHANGELOG/gen_ts/协议 schema/engineBridge,本地 5585 nextest + clippy + gateway 677 vitest 实测)→ #355 dev→main 刷新。现 main @ 7b1396394 ⊇ dev,恢复「dev 集成、main 发版」单向流。发版仍 tag 驱动,未触发。
- **D-2 ✅**:worktree 脏 WIP(2 文件 +9/−2,protocol-asset feature)以 `a6a8ea3ca` 提交到 `feat/trust-kind-scope` 后移除 worktree;其 trust 主体已随 #351 进 main、随 #354 进 dev,分支仅多这一个 feature 开关提交,留待 trust 后续工作圈定。
- **D-3 ✅(关闭)**:stash 确认为空,内容即 D-2 的 2 文件。
- **D-4 ✅**:PR #350 合并——9 项入库(docs/research ×5、docs/plans ×2、本文档、.zcodeignore),4 项删除(空文件 `0`、task_plan.md、findings-journey.md、gateway/cross-repo-check.mts)。
- **缓期批 1 ✅**:PR #352 合并——R-0 secret_guard unset→redact 安装位收口、启动恢复/启动检查更新双开关(设置→通用→启动)、上下文峰值(QUERY_USAGE `context_total` + SessionIndex `max_context_tokens`,真实来源=resolved context window)、任务看板金额(`list_tasks.cost_usd` 台账 join)。CI 二轮全绿(首轮:config.rs fmt + E2E mock 模式下启动探针 console.error 触发 watchdog,均修)。
- **本地部署 ✅**:dev @ 88235d821 构建(custom-protocol,前端 dist 已重建),DISPLAY=:1 运行中。
- **待圈选**:批 2(使命实体 config 键版 + 注入/签名诚实徽章 + 全量导出)、批 3(执行模式收敛 + 开机自启)、R4 复审时机。

## 执行记录(三)(2026-10-10,疑似真 bug 修复 + 收尾清洁 + 文档注记)

- **发现即修复 ✅(PR #388)**:执行记录(二)「留待圈定」的 `feat/trust-kind-scope` 停放提交 `a6a8ea3ca`(protocol-asset feature)经查证**不是废弃 WIP 而是真 bug 修复**——tauri 2.x 把 asset 协议处理器整体门在该 Cargo 特性后(`src/protocol/mod.rs` 顶格 `#[cfg(feature = "protocol-asset")]`),未开启时 `tauri.conf.json` 的 assetProtocol 配置被无视,UI 4 处 `convertFileSrc()`(MessageBubble 图片附件 ×2 / AttachmentChip 缩略图 / PdfPreview)在一切构建版(**含 v0.14.0 发布包**)全部加载失败。证据链:编译期(tauri 2.12.2 源码 cfg 门 + `cargo tree -e features` 零路径 + 修复前二进制 strings 检索 asset-protocol 字面量 0 命中)+ 运行时(隔离 Xvfb:99 + 临时 HOME + XTEST 驱动真实 UI 贴图流程:修复前 chip 渲染 webkit 破图问号图标,修复后蓝红渐变测试图真实渲染,并排对比图 `/tmp/asset-attach/zoom-before-after.png`)。cherry-pick 重走正式合并,原分支已删。
- **零风险清洁 ✅**:发版链遗留 `/tmp/backfill-v14` + `/tmp/r4-walk` 两 worktree 移除;6 个已并入分支 `git branch -d` 删除(chore/backfill-v14、docs/dangerous-gate-design、feat/dangerous-install-gate、fix/autostart-i18n-keys、fix/release-smoke-syntax、feat/tool-result-wire);另一会话活跃 worktree(b6-zero / wire-batch)未触碰。dev 对齐历经 ff(至 #386 合并点)→ #388 合并后拓扑分叉 → 内容 diff 验证(仅 #388 增量)后 reset 对齐,零丢失。
- **文档注记 ✅(PR #389)**:Linux 桌面包 PipeWire 版本要求落三处——README 安装段、website getting-started Linux 段、install.sh 桌面安装提示行。措辞以已验证事实为界(22.04 原版 libpipewire 0.3.48 过旧;本机 22.04 + backport 1.0.7 可用;24.04+ 开箱即用),不虚构精确版本下限。
- **R4 非阻塞备忘 ✅**:handlers.ts mission_progress 注释明写 MissionCard 并标注与 /opc 常驻 `opc-mission-progress` hero 是不同组件(搭 #388 顺风车)。
- **门禁**:desktop nextest 1780/1780 + clippy/fmt 干净;tsc 0 错;mock 相关 vitest 91/91;CI 全绿(#388:19 pass + website build 按 path 过滤跳过;#389:20 pass)。
- **本地部署 ✅**:dev @ 981f42146(#388 合并点,含 #389 前端无关),二进制已含修复,DISPLAY=:1 运行中(pid 1993406)。前端 dist 沿用 v0.14.0 构建产物——与本轮 UI 源码差异仅 handlers.ts 注释(mock 专用,零渲染影响),故未重建。
- **遗留建议**:① **v0.14.1 patch 发版待圈选**——修复仅在 dev,v0.14.0 发布包用户仍遇破图;② install.sh 可升级为 pipewire 版本探测(本轮仅提示行,逻辑探测属行为变更未纳入);③ 扩展安全专项(签名/出站扫描/自动更新)仍在决策后动清单。
