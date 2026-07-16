# Shannon Desktop — 综合 PM/架构审核 + 改进方案

**日期**: 2026-06-29 · **审核人**: 高级 PM + 架构师视角 · **状态**: 待用户审核
**分支**: 基于 `dev` + PR #96(tasks zh-CN)

## 审核范围与方法

- **全路由清单**(App.tsx):12 个页面 + Extensions 7 子页 + Settings 6 子页;5 条 legacy 重定向。
- **逐行深读**(普通用户核心路径):Welcome、Chat、Tasks、TasksHeader、ModelsSettings、Sidebar、Extensions shell。
- **自动化全量扫描**:i18n(missing key / 未翻译 / 硬编码 JSX 属性串)、空 catch、TODO/FIXME、console、native dialog、loading/empty-state 覆盖。
- **未逐行深读**(已由 grep + 历史审核轮次覆盖):OPC/OPCTask(dev 门控,普通用户不可见)、Editor、QuickFix、Memory、Triage、Usage、各 settings 子页(除 Models)。

> 深度说明:核心编排体验(chat/agent/scheduled)与普通用户首次门槛(onboarding/模型配置/导航)已逐行覆盖;dev-only 与辅助页面为抽样 + 自动化扫描。如需某页逐行深读可指定。

---

## 一、健康面(无需改动)

| 页面/模块 | 评价 |
|---|---|
| **Chat.tsx** | 功能完整:空态 WelcomeState、缺 Key 时 Banner+CTA、虚拟化列表(>30 条)、流式响应、错误重试、会话搜索/置顶/导出/打印/删除。i18n 规范,XSS 安全(textContent)。 |
| **Tasks.tsx** | 干净的编排器(active/history/worktrees),所有交互均有 toast 反馈,i18n 完整。 |
| **Welcome.tsx** | 4 步向导,连接测试带细分错误(invalid_key/rate_limited/network_unreachable),环境自动探测,focus-visible,aria-label。(除 P1-1 外优秀) |
| **ModelsSettings.tsx** | 托管 Provider(P2):测试/激活/编辑/删除,快捷填充(Anthropic/OpenAI/DeepSeek/GLM/Kimi/MiniMax/Ollama),masked-key 处理,校验完整。(除 P2-4、P3-7 外优秀) |
| **Sidebar.tsx** | Simple/Dev 双模式,会话拖拽排序+搜索+上限,快捷键提示,Triage 未读徽标。 |
| **Extensions.tsx** | 干净的 tab shell + 共享搜索(outlet context)。 |
| **i18n 体系** | PR #96 后**完整**:2285=2285 parity,0 missing key,**0 硬编码 JSX 属性串**,0 空 catch,0 TODO。剩余 46 个 zh===en 全为合理保留(品牌名/代码占位符/纯变量)。 |

---

## 二、发现的问题(按严重度)

### 🔴 P1 — 面向普通用户的实质缺陷(建议优先修)

**P1-1 · 首次运行 Documents 技能安装必失败**
- 位置:`ui/src/pages/Welcome.tsx` `DOCUMENTS_SKILLS`(L104-129)+ `installDocumentsSkill`(L286)
- 现状:三个一键安装按钮指向 `shannon-agent/shannon-skills-docs`,代码注释自认是**占位仓库**("Repos are placeholders until … published; the install UI gracefully reports failure")。
- 影响:每个新普通用户在 onboarding 点这些按钮都收到失败 toast —— 首次体验即踩坑。
- 方案(需产品决策):① 发布对应 repo;② 安装前探测 repo 是否存在,不存在则隐藏整段;③ 从 Welcome 移除,仅保留在 Extensions Hub。

**P1-2 · Simple 模式下 Extensions Hub 不可达**
- 位置:`Sidebar.tsx`(Extensions/OPC 均在 `mode === 'dev'` 门内,L342-389)
- 现状:Welcome 引导用户安装技能并链接到 `/extensions/featured`,但 onboarding 之后普通用户(Simple 模式)在导航里**找不到 Extensions 入口** —— IA 自相矛盾。
- 影响:普通用户日后想增删 MCP/技能,只能开 Dev 模式(门槛过高)。
- 方案:Simple 模式导航增加 Extensions 入口(或"管理扩展");或把 Welcome 的技能区改为纯展示、引导去 Dev。

**P1-3 · "New Worktree" 按钮对普通用户可见**
- 位置:`Sidebar.tsx` L284-293(`createSessionInWorktree` 未受 mode 门控,Simple/Dev 都显示)
- 现状:"worktree" 是 git 开发者概念,对普通用户无意义,却以一级按钮呈现。
- 影响:术语困惑;普通用户误点进入陌生流程。
- 方案:门控到 Dev 模式(与 OPC/Extensions 一致),或重命名+降级为"高级"二级入口。

### 🟡 P2 — 一致性 / 打磨

**P2-4 · 删除 Provider 用原生 window.confirm**
- 位置:`ModelsSettings.tsx:341`(全仓唯一一处 native dialog)
- 现状:与 App 自有 Modal 设计系统不一致(别处用 CancelTaskModal / ProviderModal);OS 弹窗不保证跟随 App 语言。
- 方案:改用 styled ConfirmDialog(复用 CancelTaskModal 模式)。

**P2-5 · 页面标题双显模式不统一**
- Editor/QuickFix/Triage/Usage:页面内 h1 与 Header 可见标题同值重复。历史决策=保持(Z)。
- 建议:确认是否要统一为单一标题源(全仓清理,需设计决策)。**默认 defer。**

**P2-6 · Triage 统计每 30s 轮询**
- 位置:`Sidebar.tsx:198-203`(setInterval 30s)
- 现状:轻量但属轮询;可改为 Tauri 事件驱动。(注:历史 memory 记"无轮询",实有此 30s 轮询。)
- 建议:低优先,事件化。

### 🟢 P3 — 细节

- **P3-7 · 硬编码 "context" 串**:`ModelsSettings.tsx:180` `` `· {n}k context` `` —— 应走 i18n key。(全仓唯一遗漏的可见英文)
- **P3-8 · 47 处 console.\*** (非测试 src):审查哪些是刻意日志 vs 残留调试。
- **P3-9 · 版本号过期**:`settings.advanced.logsDesc1` = "Shannon Desktop v0.1.0",现版本 v0.3.x。

### ⚪ Deferred(历史决策,列出备查)
OPC 改名(dev-only,ROI≈0)、标题统一(Z keep)、skill loop 产品化、P3-2 隐藏 demo Billing。

---

## 三、i18n 结论(用户重点关注项)

**已完整,无需进一步修复**(PR #96 已闭环):
- 0 missing key(无会显示成 key 本身的缺口)。
- 0 硬编码 JSX 属性串(placeholder/title/aria-label 全走 t())。
- tasks/agent/scheduled 模块 234 key 中文已在 #96 补齐。
- 剩余 46 个 zh===en 全为合理保留(品牌/代码占位符/纯变量/性能阈值/脱敏)。

唯一遗漏 = P3-7 的 "context" 单词(1 处)。可随 Phase 2 一并修。

---

## 四、综合改进方案(分阶段,待审核)

| 阶段 | 内容 | 估时 | 产出 |
|---|---|---|---|
| **Phase 1 — 普通用户信任** | P1-1(repo 决策)、P1-2(Extensions Simple 入口)、P1-3(Worktree 门控) | ~1 天 | 1-2 PR,base=dev |
| **Phase 2 — 一致性打磨** | P2-4(ConfirmDialog)、P3-7(context i18n)、P3-9(版本号) | ~0.5 天 | 1 PR |
| **Phase 3 — 效率/卫生** | P2-6(事件化 triage)、P3-8(console 审查) | ~0.5 天 | 1 PR |
| **Deferred** | P2-5 标题统一、OPC、skill loop、demo Billing | 待定 | 视决策 |

**推荐先做 Phase 1**:这三项是唯一直击普通用户的核心缺陷;P1-1 尤其需要在首次体验就修好(否则每个新用户踩坑)。其余为打磨,可排期。

---

## 五、需用户拍板的决策点

1. **P1-1**:Documents 技能 repo —— ① 发布 repo / ② 探测后隐藏 / ③ 移出 Welcome?(推荐 ②,最稳)
2. **P1-2**:Simple 模式导航是否加 Extensions 入口?(推荐:加)
3. **P1-3**:Worktree 按钮 —— ① 门控到 Dev / ② 重命名降级?(推荐:①)
4. **P2-4**:是否本期把 native confirm 换 styled dialog?(推荐:是,顺手)
5. **P2-5**:标题统一是否启动?(推荐:否,defer)

确认后我按 Phase 1 → 2 → 3 实施,每阶段独立 PR base=dev,沿用"检查后再实施"纪律。
