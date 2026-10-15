# 视觉收敛 R4 走查 —— 调查 spike 记录

**日期**:2026-10-15 · **分支**:`feat/visual-convergence`(基线 origin/dev) · **方法**:并行调查 spike(各节由独立代理写入)。

---

## S1 · 出站防线 tripwires 对账

**裁决:B —— 需要新增 Rust 面,本期不接线,卡片保持诚实静态**。引擎侧确有真实运行态(且比卡片文案所述更进一步:两道防线今天就真实在跑),但**没有任何既有 Tauri 命令能把它读到 UI**;接线必须新增命令 + ACL 授予 + mock 层登记,超出本 spike 的授权范围。后端工单见 §S1.6。

### S1.1 卡片现状(UI 侧)

- 组件:`desktop/ui/src/components/settings/PermissionsSettings.tsx:525-558`(`data-testid="permissions-outbound-card"`)。纯静态渲染:标题/描述 + 三行条目(`key-scan` / `injection-scan` / `signature-check`),每行右侧一枚琥珀虚线占位徽章(`hourglass_top` + `settings.permissions.outbound.pendingBadge`),无任何 switch/button/query。
- i18n 键:`settings.permissions.outbound.*`(`desktop/ui/src/i18n/locales/zh-CN.json:3373-3380`,en 同构)。`reconcileNote` 文案:「三道防线均处规划阶段——尚无引擎能力支撑,数据源就位前不提供任何开关。」
- 行为被测试钉死:`desktop/ui/src/__tests__/SettingsPanes.test.tsx:131-152`(断言 3 枚占位徽章、`queryByRole('switch')`/`('button')` 均为 null)。
- 出处:design-parity R2 审计缓期项 C1(audit §3.2 M2)。

### S1.2 引擎侧暴露盘点(真实运行态 vs 静态配置)

桌面进程**内嵌** QueryEngine(`desktop/src/session_registry.rs:27` `use shannon_core::query_engine::QueryEngine;`,:145 `pub query_engine: tokio::sync::Mutex<Option<QueryEngine>>`),因此下列进程内计数器/模式锁存与 Tauri 命令同进程,**不跨进程、无 IPC 代价**——但没有任何命令去读它们。

**① 出站密钥扫描(卡片 key-scan)→ `shannon_core::secret_guard` —— 真实且默认已武装**

| 项 | 位置 | 形态 |
|---|---|---|
| 发送边界审计 | `crates/shannon-core/src/query_engine/engine/agent_loop.rs:974,1648` — `secret_guard::audit_wire_and_log(wire)` | 运行时:每次出站 LLM 请求过一遍序列化 wire |
| 引擎入口自动安装 | `agent_loop.rs:537` — `secret_guard::init_from_env_or_config()`(注释明言 CLI/desktop/server 三宿主共用) | 运行时:桌面进程同样生效 |
| 发现计数器 | `crates/shannon-core/src/secret_guard.rs:88,100,117` — `pub fn audit_hits() -> u64`(进程级 `AtomicU64`) | 运行时状态 ✅ |
| 生效模式 | `secret_guard.rs:780-791` `static ENABLED: AtomicU8` + **私有** `fn enabled_mode()`;模式枚举 `SecretGuardMode{Audit,Redact}`(:462-468,off = 未安装/None) | 运行时状态,**无公开 getter** |
| 模式来源 | env `$SHANNON_SECRET_GUARD` > `[secret_guard] mode`(`~/.shannon/config.toml`/`.shannon.toml`,经 `unified_config.rs:89-138` `SecretGuardSection::load()`)> 缺省 **redact**(v0.13.0 起,`secret_guard.rs:826-844`) | 配置态 |
| 一次性通知闩 | `secret_guard.rs:150-160` `take_redaction_suggestion() -> Option<String>`(CLI/TUI 在消费,桌面侧无人消费) | 运行时状态 ✅ |

**② 提示注入扫描(卡片 injection-scan)→ `shannon_core::injection_guard` —— 真实,audit-only(设计如此)**

| 项 | 位置 | 形态 |
|---|---|---|
| 扫描点 | `crates/shannon-tools/src/web.rs:397`(web fetch 页面文本)、`:798`(web search 结果) | 运行时:不可信网络内容入对话时 |
| 命中计数器 | `crates/shannon-core/src/injection_guard.rs:162,167` — `pub fn audit_hits() -> u64` | 运行时状态 ✅ |
| 生效模式 | `injection_guard.rs:149-160` `OnceLock` + **私有** `fn enabled_mode()`;`InjectionGuardMode{Audit,Off}`(:110-113),env `$SHANNON_INJECTION_GUARD` > `[injection_guard] mode`(`unified_config.rs:140-175`)> 缺省 **audit**(:142-147) | 配置态,**无公开 getter** |
| 语义边界 | 只扫网络 ingress,不扫本地工具输出;只记录不改写(audit-only,`injection_guard.rs:1-19` 模块文档) | — |

**③ 技能与 MCP 签名校验(卡片 signature-check)→ 安装期门禁,无运行态**

- `desktop/src/extensions/security.rs:233` `verify_signature()`:`.mcpb` 包安装时检查签名文件,但**诚实声明非密码学验证**——`SelfDeclared` 仅代表「声明中的签名者在我们认识的名单里」(:177-220,KNOWN_SIGNERS 两个名字)。
- `crates/shannon-skills/src/security.rs` `scan_for_injection`:技能体标记扫描,listing 期门禁,与签名无关。
- 结论:这一道**没有可暴露的持续运行态**(没有 armed 状态、没有计数器),只有逐包的安装期判定。诚实 UI 表达只能是「能力说明」而非「状态」。

**静态配置区分**:上述模式配置(`[secret_guard]`/`[injection_guard]`)属引擎级 unified_config(引擎自己的 `~/.shannon/config.toml`),**不在**桌面 `DesktopConfig`(`desktop/src/config.rs:50`)里,也不在 `configure` 命令的键白名单里——即使只读模式也拿不到现成通道。

### S1.3 既有 UI 通道核查(为何裁决 B)

逐一排除了所有既有只读命令:

- `get_status`(`desktop/src/commands_chat.rs:368`)→ `StatusResponse{model, provider, querying, message_count, working_dir, active_profile}` — 无防线字段。
- `get_config`(`desktop/src/commands_config.rs:1629`)→ `DesktopConfig`(config.rs:50,含 `sandbox: Option<SandboxConfig>` :225,但那是命令沙箱、另一回事)— 无 guard 字段。
- `configure`(`commands_config.rs`,`match key` 白名单)→ 纯 setter,键集无 `secret_guard.*`/`injection_guard.*`。
- `request_permission`/`respond_permission`(`commands_permissions.rs:167,185`):工具审批流,无关。
- `export_diagnostics`(`commands_diagnostics.rs:178`):写 zip 包,非可查询状态。
- 决定性反证:`grep -rn "audit_hits|secret_guard|injection_guard" desktop/src --include="*.rs"` 仅命中 `extensions/security.rs:59` 的 PATTERNS 复用注释;`desktop/src/main.rs:163` `generate_handler!` 全清单无任何防线相关命令;`desktop/src/loopback_api.rs` 亦无。

接线成本(若未来接线,是三道门,不是一道):
1. `desktop/tests/app_command_acl_coverage.rs` — manifest↔handler 双向 diff tripwire:新命令必须同步在 `acl/app-permissions.json` 加 `allow-<command>` 授予并进 capability set,否则 CI 挂。
2. `desktop/ui/src/__tests__/mock-handlers-coverage.test.ts` — 新 invoke 字面量必须有 mock handler 或进 `UNMOCKED_ALLOWLIST`。
3. 引擎侧还需补两个**公开**模式 getter(`secret_guard::enabled_mode` :782 与 `injection_guard::enabled_mode` :151 均为私有)——这本身就是 shannon-core 的新面。

### S1.4 裁决:B

按 spike 裁决规则:「无既有只读命令可供给真实状态 → 不实现,登记后端工单,卡片保持诚实静态」。卡片原样保留(含其占位徽章与 `reconcileNote`),本次未改动任何代码。

**一处文案精度注记(供后端批次参考,不属本 PR)**:`reconcileNote` 的「三道防线均处规划阶段——尚无引擎能力支撑」对①②是**过谦**的——secret_guard(redact 缺省)与 injection_guard(audit 缺省)今天就在桌面进程内真实运行,缺的只是「暴露给 UI」。接线落地时应同步把该行改为「引擎已在运行;此卡显示实时状态」一类,避免反向失真。

### S1.5 诚实契约核对

维持:状态未知 → 不渲染确定值;无数据源 → 保持琥珀虚线占位(MemoryPanel injection-preview-pending 同形)。当前卡片满足该契约,裁决 B 下无需变更。

### S1.6 后端工单(建议登记为独立 ticket)

**标题**:出站防线卡接真实引擎状态 —— 新增只读命令 `get_outbound_defense_status`

**范围**:
- shannon-core(小):`secret_guard` 与 `injection_guard` 各补一个 `pub fn active_mode() -> Option<XxxGuardMode>`(读既有 `ENABLED`/`OnceLock` 锁存,不新增状态);`SecretGuardMode`/`InjectionGuardMode` 加 `serde::Serialize`。
- desktop(主):新命令

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct OutboundDefenseStatus {
    /// 出站密钥扫描:redact | audit | off(未安装 = None)。
    secret_guard: Option<GuardModeStatus>,   // { mode, audit_hits: u64 }
    /// 提示注入扫描:audit | off;计数器为进程启动以来累计命中。
    injection_guard: Option<GuardModeStatus>,
    /// 签名校验:无运行态,恒为能力说明(install_gate,非密码学验证)。
    signature_check: SignatureCapabilityNote,
}

#[tauri::command]
pub async fn get_outbound_defense_status() -> Result<OutboundDefenseStatus, String>
```

计数器直接调 `shannon_core::secret_guard::audit_hits()` / `shannon_core::injection_guard::audit_hits()`(同进程,零 IPC);模式取新 getter。
- 注册四件套:`main.rs` `generate_handler!` + `acl/app-permissions.json` 授予 + capability set + UI mock handler(两道 tripwire 见 §S1.3)。
- UI:卡片三条目改为按字段渲染(状态未知/命令失败 → 维持现有琥珀虚线占位;`signature_check` 恒为说明性条目),并按 §S1.4 注记修正 `reconcileNote`。
- i18n:新增状态徽章键,zh-CN/en 先行,十语言传播,`node scripts/i18n-check.mjs` 过闸。

**预估**:shannon-core getter ~0.5h(含测试);desktop 命令 + ACL + mock ~2-3h;UI 卡片 + i18n + 三层测试(组件/ACL/mock coverage)~0.5-1 天。合计 **~1-1.5 天**,一个独立小 PR。

**依赖/风险**:计数器为进程生命周期累计值(重启归零),UI 文案需标注「本次运行以来」;`audit` 模式下 secret_guard 计数器包含「已转发原值」的发现,展示时不得暗示「已拦截」——诚实红线。

---

## W4 · 逐页微打磨

**日期**:2026-10-15 · **分支**:`feat/visual-convergence`(在 W1-W3/T1/T2 之上) · **方法**:设计稿截图(docs/design/ui-redesign-2026-10/screenshots)× 应用截图(`VITE_MOCK_MODE=1` 生产构建,vite preview :1423,Playwright,1440×900,tokyo-night 全量 + tokyo-night-light 抽查;截图仅存 /tmp,未入库)逐页并排比对,按批准的微打磨词表(极光发丝线/chip 圆角内距/阴影档/大写微标签/tabular-nums/cmTheme 兜底)收口。

### W4.1 cmTheme 暗色兜底紫化

`desktop/ui/src/components/editor/cmTheme.ts`:jsdom/SSR-only 的暗色兜底仍持旧 ANSI 蓝 —— `DARK_FLOOR.cursor` 与 `DARK_SYNTAX_FLOOR.keyword/attribute` 的 `#7aa2f7` → **`#8b7cf7`**(W3 tokyo-night 主色同族;兜底不再是活主题读不到时的「错误预测」)。对比度复核:`#8b7cf7` 对 `#1a1b26` = **5.15:1**(AA ≥4.5 通过;原蓝 6.79:1),文件内注释同步改写;`CodeEditor.test.tsx` 两处字面量断言同步(其余 string/comment/constant/tag/heading 不动)。xterm ANSI 蓝按 W3 裁决保持不变。

### W4.2 逐页判定表

| 设计页 ↔ 路由 | 命中项 | 改动 | 遗留登记 |
|---|---|---|---|
| 01-welcome ↔ `/welcome` | hero composer 极光线已在位(W4 前) | 无 | 品牌区结构差异(设计:居中 hero+步进 pill;应用:顶部品牌行+圆点步进)——既有 IA,不重建 |
| 01b-welcome-done ↔ `/welcome`(完成态) | done-card `.glasscard aurora-line` | **+aurora**:`WelcomeCard` 增 `aurora?: boolean` prop,仅 `DoneStep` 传入(设计只在完成态画线,中间步不画);组件测试既有 DoneStep 用例覆盖渲染 | — |
| 02/02c/03-chat ↔ `/chat` | composer 极光线、diffcard 极光线(激活文件)、chip 形制均在位 | 无 | 底部状态条无会话统计数字 —— 02c 收敛裁决 D5 有意移入 SessionUsageDialog,非缺口 |
| 04-tasks ↔ `/tasks` | NL 快建卡极光线已在位;**运行/历史 下划线 tab** | **+aurora-tab**:激活 tab 由 `border-primary`(基类按钮四边框全染色,读作描边框)改为新 `aurora-tab` 工具类 —— 设计 `.tab.on::after` 的 2px 紫→青渐变下划线,坐于行发丝线上 | 日历部件在设计稿为主列伴卡;应用挂在侧栏位(Tasks.tsx:605),呈现位不同,不重建 |
| 04b-best-of-n ↔ `/tasks` | 分支 chip 金额已 tabular | 无 | — |
| 05-inbox ↔ `/triage` | 筛选 pill/行条目形制对齐 | 无 | 设计顶部三张 stat 大卡 vs 应用头部紧凑 chips(「3 条待处理/今天 1 条/本周约省」)——呈现层差异,数据同源,不重建 |
| 06-usage ↔ `/usage` | 表头大写+字距、表格 `.num` mono 已对齐 | **+tabular-nums**:`StatCard` 值行(设计 `.stat .v`)——覆盖 KPI 八卡 | KPI 卡解剖(应用横排 icon+值,设计纵排)为 2026-09-29 共享组件裁决,登记不回改 |
| 07-connectors ↔ `/extensions/*` | 卡片/扫描 crumb 对齐;数据源 tab、MCP 添加弹窗 tab | **+aurora-tab**(DataSources 两 tab、McpAddServerDialog 三 tab) | 主 tab(精选/已安装/待处理)为容器 chip 形态 —— design-parity R2 IA 裁决(M1/X4),登记不回改 |
| 08-memory ↔ `/memory` | **注入预览横幅** | **+aurora-line**:设计 08:146 在该横幅画 `.glasscard aurora-line`;移除早前「memory 不属四极光面」的过时注释(W4 以「设计画了就上」为准) | 分类分布 ring vs 应用五枚小 stat 卡:呈现差异,不重建 |
| 09-mission-control ↔ `/opc` | mission hero/激活卡极光线已在位 | StatCard tabular-nums 同步受益 | — |
| 10-timeline ↔ `/timeline/:id` | 头部统计 chips 已 mono 形制 | 无 | **结构登记**:设计为工具级瀑布(4 stat 卡+tl-row 瀑布+双累计图);应用为回合卡+累计曲线,信息架构不同代 —— 不在本批重建 |
| 11-files ↔ `/files` | 计数/meta 已 tabular | 无 | — |
| 12-settings-general/appearance ↔ `/settings/*` | 分段控件/卡片形制对齐 | 无 | — |
| 12-settings-models ↔ `/settings/models` | provider 筛选 tab | **+aurora-tab**(同 04;设计 12-models 未画 tab——应用自有 IA,统一到同一 tab 语言避免双语并存) | 设计为供应商卡列表;应用为性能策略+指位 IA,既有裁决,不重建 |
| 12-settings-connections ↔ `/settings/connections` | **配对请求条目** | **+aurora-line**:设计 12:208 在待配对 callout 画线;应用为列表行,每行一条极光线(mock 模式 mobile 停用不渲染,类级验证) | 设计的 callout 卡解剖(ico32+批准/拒绝同排)vs 应用行式列表 —— 不重建 |
| 13-companion ↔ `/companion` | **窗体顶缘极光线** | **+aurora-line**(页面根 = 356px 伴随窗内容,真实窗尺寸下即设计框光;浏览器 1440 预览会拉伸) | — |
| 14-command-palette ↔ ⌘K | 应用已有极光线(设计稿未画) | 无 | 保留现状(更早批次裁决,不回删) |

### W4.3 新增 CSS 面(index.css)

- `@utility aurora-tab` —— 设计 `.tab.on::after` 的移植:2px、左右 inset 10px、bottom:-1px、圆角 2px、`linear-gradient(93deg,#8b7cf7,#6fa8e8 55%,#56c8dd)`(全饱和 --grad-aurora 三站,固定品牌色、非主题 token,12 主题同读;纯静态 `::after` 装饰,`pointer-events:none`,选中态仍由文字色+`aria-selected` 承载,reduced-motion/reduce-glass 无需回退)。应用四处:Tasks、DataSources、McpAddServerDialog、ModelsSettings(均为既有下划线 tab,无 DOM 改动)。
- `aurora-line` 新落点三处(见上表)复用既有工具类,含 light 主题自动提档,无新增 blur 层、无动画。

### W4.4 tabular-nums 补齐

- `StatCard`(shared;Usage KPI/Memory 统计/OPC 分析三处消费)值行 `+tabular-nums`。
- `SidebarUsageMeter` 标签行 `+tabular-nums`(「月预算已用 62%」,对应设计 `.meter-row b`)。
- 其余页面排查:SessionsPanel/BatchDiffCompare/FilesPage/Usage 表格/BarChart 均已有 tabular/mono,未动。

### W4.5 刻意不改

- 阴影档:玻璃面暗色档由 `glass-surface`/`glass-overlay` 承载(≈设计 sh-1/sh-2),实体卡亮色档走 `shadow-e1..e5`;并排比对未见可视偏差,不动全局 token。
- chip 圆角/内距:并排细察(composer 三要素、模型 chip、筛选 pill)与设计 `.chip`(h28/p9/r8)/`.pill`(h26/p11/full)一致,未动。
- 大写微标签:侧栏分组/表头/图卡标题均已 `uppercase tracking-wider`,未动。
- i18n:零文案改动(en=4489 键,10 locale 校验通过)。

### W4.6 验证

- vitest 全量:**3825 passed / 9 skipped(328 文件)**,与基线一致(CodeEditor 两处字面量断言随 cmTheme 值同步更新,属「字面量值期望」许可)。
- `node scripts/contrast-audit.mjs`(cwd desktop/ui):**All theme token pairs pass AA**,exit 0。
- `npx tsc --noEmit`:0 错;改动文件 `eslint --max-warnings 0`:0;`check-design-tokens.mjs`:0 error(13 条既有文档化 warning,无新增);`i18n-check.mjs`:OK。
- 极光面上机验证(Playwright computedStyle):companion 根、memory 横幅、tasks 激活 tab、chat composer 的 `::before/::after` 渐变均实测在位;tokyo-night-light 抽查通过。
