# 新版设计稿(Aurora Glass)逐页对比审查 R3 —— 收口走查报告

**日期**:2026-10-15 · **分支**:`feat/files-library-batch`(基线 dev @ `69286b288`)
**上轮**:[R2(2026-10-14)](./2026-10-14-design-parity-audit-r2.md)(整体 8.5/10) · **方法**:4 个并行走查代理(对话域/任务收件箱用量域/连接记忆指挥台时间线文件域/设置伴随窗面板横切域),基准为 **D1-D8 回改后的设计稿 HTML**,逐域核验 B 批新特性 + 全页快扫 + 回归检查。

---

## 0. TL;DR

1. **整体保真度 9/10**(R1 7/10 → R2 8.5/10 → **R3 9/10**),四域全部 9/10,**零阻断缺陷**。B 批全部特性(B1 预览面板/B2 提取反写/补1 回合 meta/补2 折算参数化/补3 错误锚点/存1 多窗口核验/存2 浅色走查)逐项验收通过,测试三层齐备。
2. 走查发现的存量缺陷已当场修复:**BarChart/Donut SVG 色类错误导致柱图全主题渲染黑色**(bg-* 对 SVG 无效 + Tailwind scanner 看不见运行时替换,改字面量 fill-/stroke- 映射);**工具成功对勾穿琥珀最后一处残留**;浅色主题 3 个 outline 对比度不达 3:1(含门禁补位);turn-meta 浅色 marginal 对比度。
3. 收官微批(本 PR 内):命令面板「任务」术语终检(最后一处「自动化」)+ 补 /usage 入口;General 链接卡标题对齐「默认执行模式」;死键 `palette.action.newRoutine` 清理;Triage/Usage/OPC 补 sr-only h1(Tasks 模式)。
4. **结论:R3 通过——「Aurora Glass 14 页」落地周期收官**。剩余差异全部为形态级/有裁决注记的可改进项,已登记 §4 遗留清单,不阻断收官。

---

## 1. 逐域结论

| 域 | 评分 | 要点 |
|---|---|---|
| 对话(02/02c/03) | **9/10** | 回合 meta 与设计稿逐字同形+四象限测试;composer 五控件/权限词汇 v2/effort gauge/Shift+Tab/状态条两段无回归;dock 徽章/极光线/条件默认 tab 无回归;高危审批全局 Modal 追认维持 |
| 任务/收件箱/用量 | **9/10** | 折算参数化前后端双层校验(1-120 拒绝越界)+行内编辑;focusError 滚动+2s 高亮三层测试;柱图/环图配色+滚动容器修复;失败例行/指挥台入口/NL 卡/审批内置/运行号无回归 |
| 连接/记忆/指挥台/时间线/文件 | **9/10** | B1 七项+B2 六项全落(含诚实缺席正反例+线形测试);注入横幅 3chips+N tooltip 逐字;OPC hero 合卡/blocked 原因/时间线预算 chip 无回归;07 安全话术三处逐字 |
| 设置/伴随窗/面板/横切 | **9/10** | 出站防线诚实卡/默认执行模式实体卡/工作目录可写/旁路默认六项无回归;manual_minutes_per_item allowlist 质量高;浅色令牌修复 dark 零波及(diff 证明);i18n 十语言全绿+ru 复数四类完整 |

## 2. 本批验收明细(摘要,证据见走查代理报告)

- **B1 预览面板**:320px 右栏/fileKind 11 组图标/选中-Esc-关闭/来源回跳(session_id 门控)/提取徽章(extracted_segments 门控)/操作回撤(≥1280 结构性)/<1280 单栏回退;15 测试用例。
- **B2 提取反写**:落点 `commands.rs:1787`(send 管线,PDF/office 臂后);`extracted==false` 不反写、未注册路径跳过、存储失败仅 warn 不 fail send;`Option<u32>` 线上缺席语义有线形测试;89 Rust 测试+fmt+clippy 绿。
- **补1**:turn-meta 工具数+时长(花费经调查无逐回合归属数据源,诚实缺席,结论入码注);18 用例四象限。
- **补2**:`manual_minutes_per_item`(allowlist 校验 1-120 拒绝不 clamp,空回落 5)+Triage 行内编辑(Enter/blur 防双发、失败回退);Rust 2 测+前端 6 测。
- **补3**:`{openRoutineId, focusError}` 路由状态→抽屉 rAF 滚动+`ring-error` 2s 高亮;Tasks/Drawer/Triage 三层测试。
- **存1**:核验发现两处多窗口瑕疵**已由历史提交 c1c5e78a8 完整修复**(BroadcastChannel toast 去重 F3 模式、subagent 事件 session 归属+isEventForCurrentWindow);本批补上缺失的 AppContext 级集成测试(6 用例)。
- **存2**:3 浅色主题 × 7 页走查(46 张截图):solarized-light outline 2.48:1 不达 3:1(613 处边框)→ 修 3 主题 outline 源值 + contrast 脚本补 `LIGHT_ONLY_PAIRS` 门禁(原门禁自 token 规范化起休眠);dark 9 主题 diff 零变化。

## 3. 收官微批(随本 PR)

1. BarChart/Donut SVG fill/stroke 字面量映射(**存量缺陷:柱图全主题黑柱**——`bg-*` 对 SVG 无效,且运行时 replace 的类名 Tailwind scanner 不可见);
2. turn-meta `text-tertiary`→`text-on-surface-variant`(浅色对比 marginal 消除,数值复核全主题 ≥4.5:1);
3. TaskCard 取消按钮 default 变体 primary 底压 error 图标 → bg-transparent;Usage 页根补滚动容器(溢出 637px 不可达);
4. 命令面板 p-tasks 标签「自动化」→「任务」(IA-T1 半截迁移的最后一处)+ 补 /usage 入口;General 链接卡标题对齐「默认执行模式」;死键 `palette.action.newRoutine` 全 locale 清理;
5. Triage/Usage/OPC 补 sr-only h1(Tasks 模式对齐)。

## 4. 遗留清单(R3 可改进项,全部非阻断;接替 R2 §7 成为跟踪正本)

**需要后端/契约**:
- MCP「去认证」动作(设计 07:337-371,前后端均无 deauth IPC);
- 文件预览卡来源会话无可读标题(`FileIndexEntry` 缺 session title);FilesPage 搜索框+页副标题;
- 02 回合 meta 花费段(需 per-turn 成本归属,建议 MessageArea 侧派生 turn→cost 下发)。

**纯前端小项(建议随下批顺手)**:
- msg-meta 补「完成」chip(成功态语义对称)并裁决头部行放置;Reasoning 头分态(「已思考 N 秒」vs「思考中」);dock tab 顺序注记或 Diff 居前;03 主区整宽评审形态(挂账);blocked 卡琥珀边框(现为 error 红);已丢失卡整卡虚线形态;Triage 节省卡订阅 CONFIG_UPDATED(跨窗回显);Usage 峰值卡百分比形态/按会话模型列/今日较昨日副行/默认成本口径;companion 文案微差;ask 档描述两侧(设计/实现)二选一收口。

**设计稿侧(下次重绘顺笔)**:
- 02/03 页 F-9 注记句式(与 02c D3 口径);07 卡足「注入扫描 ✓ 已签名 ✓」措辞诚实化回改与「去授权」文案;安全白皮书按钮注记;用户气泡圆角/头像形态;06「每日成本」首屏口径。

## 5. 门禁

lint 全绿(tsc/eslint/tokens/i18n 10-locale 4473 keys/import-collisions)· vitest **3783/3783** · e2e 17 项通过(themes/theme-gallery/glass-budget/attachments/cross-page/dock/model-mode,含 12 主题 axe 全扫)· cargo fmt --check + clippy -D warnings + 89 Rust 测试 · mock 实跑视觉复验(柱图双系列配色/浅色三主题截图)。

---

*四份走查代理报告全文已核校合并;行号以 `feat/files-library-batch` @ 09a59a823 工作树为准。*
