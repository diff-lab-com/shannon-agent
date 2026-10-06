# Shannon Desktop UI 体系全面审查与 Liquid Glass 方向评估（2026-09-29）

> 审查范围：`desktop/ui`（主产品，React 19 + Vite + Tailwind v4 + shadcn/Base UI，221 个组件文件）为主，`website/`（Astro 官网）为辅。
> 方法：四路并行代码审查（主题令牌 / 页面导航 / 组件质量 / 设计文档考古）+ 关键发现逐条人工复核 + 现网截图走查（e2e 视觉基线 vs 9 月审计前后对比）。
> 本文回答四个问题：① 体系现状如何；② 设计视角的评价；③ 玻璃风格要不要做/怎么做得更好；④ 问题清单与实施方案。

---

## 0. TL;DR

1. **体系成熟度高于预期，是同类开源项目第一梯队**：颜色令牌化 100%（硬编码 0，CI 门禁 0 违规）、12 主题由单一来源生成且过 AA 对比度门禁、axe 全路由 e2e + 逐主题对比度专项 + 3 页视觉基线。骨架不用动。
2. **玻璃风格不需要"是否改进"的决策——2026-09 已正式转向并落地大半**（`docs/design/ui-audit-2026-09/UI-IMPROVEMENT-PLAN-2026-09.md` §6.5，Wave 1/2 完成）。方向判断正确（玻璃只上悬浮 chrome、内容保持实体、深色优先，符合 Apple HIG 精神与四个竞品的共同形态）。当前的问题是**完成度约七成 + 一致性失控**：同一材质在代码里有三种写法，最核心的侧栏/顶栏反而不走玻璃工具类。
3. **新的主要风险是"令牌漂移"而非"缺令牌"**：已核实 4 处多源定义（时长三源、海拔双源值不同、图标字号 12 vs 14px 冲突、图表色板 12 主题零覆盖），实际渲染值 ≠ 声明值。这类问题正在侵蚀这套优秀体系的可信度。
4. 实施方案分 4 批（止血 → 令牌执行 → 玻璃完成度 → 品牌统一），合计约 4-6 周，每批带验收标准和 CI 守卫，见 §5。

---

## 1. 体系现状：主题与设计令牌（Theme）

### 1.1 架构（强项，保持）

- **单一来源生成**：`scripts/theme-source.json` 是唯一手工色值处，`scripts/generate-themes.mjs` 派生 on-token 并做 AA 契约校验（21 对前景/背景，正文 4.5:1），不达标直接生成失败。产物三件：`src/theme/generated/themes.css`（每主题 `[data-theme]` 变量块）、`registry.ts`、回写 `index.css` 的 `GENERATED:THEME_BASE` 区域；`--check` 拦 CI 漂移。
- **运行时切换**：`ThemeContext.tsx` 写 `data-theme` / `data-theme-mode`，`@custom-variant dark` 由 registry 驱动；12 主题（6 深 6 浅）+ system 伪主题，深色默认 tokyo-night。另有 `fontScale`（0.85–1.3）与 `density`（compact）两个正交轴。
- **令牌清单**（`index.css` @theme）：MD3 语义色全量 + shadcn 别名、4px 间距刻度、MD3 角色制字号 10 档（11→48px，每档带行高/字距/字重）、海拔 e1–e5、z-index 语义刻度、时长四档、圆角从每主题 `--radius` 倍率派生。
- **CI 纪律**：`check-design-tokens.mjs` 禁裸调色板类与行内 hex，当前 0 违规；`i18n-check` 禁退役术语。

### 1.2 已核实的问题（漂移类，全部亲自验证）

| # | 问题 | 证据 | 影响 |
|---|------|------|------|
| T1 | `--duration-*` 三源定义、实际值 ≠ 声明值：@theme 声明 100/160/240/400ms，被同文件 `:root` 覆盖为 **120/200/280ms** 并新增 `--duration-base`；`tokens.css` 又镜像一套（100/160/240/400） | `index.css:195-198` vs `index.css:266-268` vs `styles/tokens.css:131-134`；README 记录的是声明值 | 文档与渲染不符；全站动效节奏与设计意图脱节且无人知情 |
| T2 | 海拔双定义且值不同：`var(--shadow-e1)`=黑 5%，类 `shadow-e1`=黑 18% | `index.css:172-176` vs `index.css:306-309` | 同名不同果；且 e 系列采纳率仅 11 处 vs 裸 `shadow-sm/md` 109 处——语义海拔体系**名存实亡** |
| T3 | `icon-xs` 双定义冲突：12px vs 14px（无 layer 的后者胜出） | `index.css:204` vs `styles/tokens.css:144` | 源码里写 12 渲染出 14；`tokens.css` 注释自己警告过的 "drift trap" 已复现 |
| T4 | `--chart-series-1..8` 只存在于 light 基色板，12 主题 **0 次覆盖** | `grep chart-series themes.css` = 0；`Chart.tsx:162-169` 直接引用 | 暗色主题下图表沿用浅色优化色板，对比度/和谐度未受控 |
| T5 | `tokens.css` 定位失真：README 称其 "inventory（不生效）"，实际被 import 生效（`:root` 重复声明全部值 + compact 密度覆盖 + 全局 reduce-motion） | `styles/README.md` vs `index.css` import 链 | 四源同步（theme-source.json → generated ×2 → index.css 基区 + tokens.css 手工镜像）中 tokens.css **无脚本绊线**，是漂移温床 |
| T6 | 死代码工具类：`press-scale`/`press-scale-active`、`animate-panel-in`、`glass-surface-dark`、`node-connector`、`thought-connector` 全部 tsx 0 引用；`custom-scrollbar` 在 CSS 里**无定义**却被 `KanbanBoard.tsx:100` 等引用 | grep 0 命中 | 维护噪音 + 假实现感 |
| T7 | 文档失真：README 列 8 主题（实际 12）、z-index 表写 `--z-*` 前缀（实际 `--z-index-*`） | `styles/README.md` | 新人按文档行事即踩坑 |

---

## 2. 体系现状：页面与导航

### 2.1 结构（强项）

- 13 个页面 + 2 个已退役为内联面板的页面（Editor/QuickFix），全部 lazy + Suspense；外壳与页面职责分离成熟：页面标题/模型切换/审批统一上收到 Header（TITLE_MAP），页面零标题代码；`--sidebar-w` 单写点（`Layout.tsx:124-130`）；路由级 ErrorBoundary 按 pathname 重挂 key。
- 导航哲学清晰：4 个 flat 入口（对话/收件箱/连接/记忆）+ simple/dev 双模式门控（`useSidebarMode`），Tasks/Settings/Extensions 三处消费同一 mode。
- 键盘体系完整：`Mod+K` 命令面板（cmdk，7 分组 + 同义词模糊）、`Mod+1..6` 直达、Triage `j/k/Enter`、Escape 的浮层归属仲裁（`useKeyboardShortcuts.ts:9-17`）、帮助浮层与实际绑定同步。
- 状态类组件统一度高：`empty-state` 18 处、`error-state` 14 处、`LoadingState`/`Skeleton` 各 14 处、sonner toast 唯一通道（~170 处调用）、双层 ErrorBoundary。

### 2.2 问题

| # | 问题 | 证据 |
|---|------|------|
| P1 | **内容容器宽度 6 种并存**：`max-w-[1200px]`（Triage）/`[1600px]`（OPC）/`max-w-6xl`（Usage/Editor）/`max-w-3xl`（TurnTimeline/QuickFix）/`max-w-[1000px]`（Settings）/无限制（Chat/Extensions）；令牌 `--spacing-max-content-width: 1200px` 定义了但 0 引用 | 各页面文件 |
| P2 | **StatCard 双实现**：`ui/stat-card.tsx` 与 `Usage.tsx:59` 自建同名组件，排版不同 | 两文件对比 |
| P3 | **错误反馈三通道无准则**：ErrorState / Banner tone=error / toast 混用（`Tasks.tsx:389-401`、`MessageArea.tsx:294` 用 Banner；Triage/TurnTimeline 用 ErrorState） | 三处对比 |
| P4 | **加载态三种形态**（spinner 块 / 骨架屏 / 页内手写 skeleton，`OPC.tsx:52-55`）缺选择准则 | — |
| P5 | **标题双轨**：Header TITLE_MAP 为主，但 Extensions 自带 H1（有意为之，注释说明）、TurnTimeline/Editor/QuickFix 自带标题头——两套标题来源并存 | `Header.tsx:19-38` vs `Extensions.tsx:74` |
| P6 | **移动适配遗留**：桌面应用保留完整 mobile 形态代码（`matchMedia(max-width:767px)` 侧栏抽屉、`md:hidden` 汉堡按钮），Tauri 最小窗宽 800px，`md:` 断点（768px）以下形态实际不可达却持续付维护成本 | `Layout.tsx:77-84,169`、`Header.tsx:145`、`tauri.conf.json:57-61` |

---

## 3. 体系现状：组件层

### 3.1 强项

- **颜色纪律近乎完美**：tsx 中裸 hex/rgb/调色板类 = 0；`dark:` 前缀仅 37 处（其余全靠语义令牌自动换肤——正确姿势）；z-index 裸值仅 4 处。
- **a11y 工程化**：aria 属性覆盖 150+ 文件、focus-visible 170 处/66 文件、装饰图标统一 `aria-hidden`、Playwright 全路由 axe + 逐主题 color-contrast + 视觉基线三页（5% 容差）。
- 图标单一通道（Material Symbols，169 文件）、lucide 残留 0、emoji 仅 3 处。

### 3.2 问题

| # | 问题 | 证据 |
|---|------|------|
| C1 | **字号旁路 667 处 `text-[Npx]`**（298 处图标 + ~359 处正文），绕过 10 档 type scale（高频：14px×152、18px×120、11px×107）。根因：官方包装器 `ui/icon.tsx` 有完整文档与映射表却**仅 4 个文件采用**，手写 `<span className="material-symbols-outlined text-[18px]">` 是事实惯例 | grep 统计；`ui/icon.tsx` |
| C2 | **裸 `<button>` 69 处**游离于 cva 体系外（a11y 纪律尚可但视觉细节靠人肉对齐），`buttonVariants` 导出业务层零复用；集中地 `SidebarSessions.tsx`(12)、`TerminalPanel.tsx`(7) | grep 统计 |
| C3 | **双间距刻度混用**：语义 `gap-sm` 247 次 vs Tailwind 默认 `gap-2` 49 次；裸 `rounded` 119 处不落派生刻度 | grep 统计 |
| C4 | **暗色模式真问题**：`HookTaskPipeline.tsx:182` 自绘开关滑块固定 `bg-white`，深色下刺眼（官方 `ui/switch.tsx` 走令牌）；`MobileDispatchCard.tsx:245` 二维码 `bg-white` 属合理但无注释 | 两文件 |
| C5 | 时长令牌旁路：Tailwind 原生 `duration-100/200/300/500/700` 23 处 | grep 统计 |

---

## 4. 设计视角评审与玻璃风格评估（问题 2、3）

### 4.1 总体设计评价

以高级 UI 设计师视角走查前后截图（9 月审计 18 页 vs 当前视觉基线）后的判断：

- **进步是实质性的**：从"浅色扁平白卡 + 单一紫罗兰"的通用 MD3 观感，转到"深色优先 + 材质层次"的开发者工具成熟形态。当前 tokyo-night 默认主题下，信息层级、工具卡折叠、composer 悬浮感都达到了竞品水准；浅色 material 主题的玻璃 composer 表现同样干净。
- **当前的风格人格**：接近"Codex/ZCode 的工作台克制"而不是"苹果发布会式的玻璃炫技"——这个定位对编程 Agent 桌面端是**正确的**（用户长时间凝视、高信息密度、深色为主）。
- **短板不在方向而在三处**：①材质语言不统一导致"玻璃感"时有时无；②排版层面字号旁路让 12 主题 × 双密度 × 字号缩放之外的第三套隐性字号体系存在，精致感被细节噪音稀释；③层次感偏平——深色下侧栏/内容区明度差微小，海拔语言（阴影/边框/材质）因 T2 失效而没有形成可感知的纵深。

### 4.2 玻璃风格评估：坚持，并从"用了"升级到"做成体系"

**结论：不需要再评估"是否转向"。2026-09 已转向（审计计划 §6.5 确立 Liquid Glass 三级材质），方向正确，应该坚持并补完。** 三个理由：

1. **竞品基线支持**。仓库自己的竞品画像（审计计划 §2 + `desktop/COMPETITIVE-ANALYSIS.md`）：Claude Code Desktop / Codex / ZCode / Hermes 的共同形态就是"深色优先 + 材质层次 + 克制用色"，Hermes 更被明确标注为"Apple 式精致 UI"。用户看到竞品的玻璃质感再回看 Shannon，感知差距主要来自完成度而非方向。
2. **Apple HIG 本身反对全面毛玻璃**。"Liquid Glass 只用于导航栏和标签栏"是苹果自己的规约；本项目"玻璃只承载悬浮 chrome（侧栏/顶栏/composer/浮层），内容卡保持实体"的三档材质设计（`index.css:251-296`）恰好是教科书式落地，且已带 `@supports` 降级、`prefers-reduced-transparency` 实心回退、同屏 ≤4 个 backdrop-filter 的性能预算。这套护栏让玻璃风格**可持续**，不必推翻。
3. **沉没成本与守卫已成体系**。12 主题 × AA 门禁 × 视觉基线全部兼容玻璃层；换方向成本远大于补完成本。

**与 Apple Liquid Glass 的真实差距（= 改进点）**：

| # | 差距 | 现状证据 | 改进方向 |
|---|------|---------|---------|
| G1 | **同一材质三种写法**：`glass-surface` 工具类（14 文件在用）、Header 手写 `bg-surface/80 [backdrop-filter:var(--glass-blur-surface)]`（绕过 inset 顶部高光/hairline/`contain:paint`）、25 处 ad-hoc `backdrop-blur-*`（Layout footer、dropdown-menu、side-panel、MessageBubble、ChatInput 等） | `Header.tsx:143`、`Layout.tsx:202` | 全部收编到 `glass-surface`/`glass-overlay` 两档工具类；`check-design-tokens` 加规则禁直接写 `backdrop-filter`/`backdrop-blur-*`（进 allowlist 审批） |
| G2 | **L0 窗口底座缺失**：审计计划的 `--material-base`（窗口底实体深色，让玻璃"有东西可透"）未落地；深色下侧栏与内容区背景几乎同色，玻璃的折射感弱、层次平 | 计划 §6.5.1 vs `index.css` 现状 | 在主题基区补 `--material-base`，侧栏/内容区拉开明度阶（surface-container 阶梯用足），让 blur 有可透内容 |
| G3 | **浮层全家桶未统一玻璃 overlay**：Dialog 用了 glass，但 CommandPalette、DropdownMenu、Tooltip、sonner Toast 各自为政 | `App.tsx:126`、`ui/command.tsx` 等 | 浮层原语统一 `glass-overlay` 材质——一次定义全站生效（审计计划 §6.5.7 的原意） |
| G4 | **Toast 游离于主题体系外**：sonner `theme="system"` 跟随 OS 而非 app 的 `data-theme-mode`（深色 OS + 用户选浅色主题时 toast 反色）；`richColors` 用内置色板而非语义令牌 | `App.tsx:126` | 按 `data-theme-mode` 驱动 sonner theme，`toastOptions` 接入语义令牌 |
| G5 | **动效语言半途而废**：`--ease-glass`（Apple sheet 曲线）已定义，但配套的 `press-scale`（按压 scale 0.98）和 `animate-panel-in`（玻璃贴附进场）是死代码 0 引用；玻璃层进场动效没有统一 | grep 0 命中 | 要么在全站交互点启用（按钮/卡片/tab），要么删除——当前是最差的"定义了没用"状态 |
| G6 | **性能预算无守卫**：≤4 个 backdrop-filter/屏 只写在注释里，无运行时断言 | `index.css:253-254` | e2e 增加断言脚本（遍历关键页统计 `getComputedStyle` 含 backdrop-filter 的可见元素数） |
| G7 | **浅色主题玻璃未专项走查**：12 主题玻璃参数全局统一（themes.css 0 覆盖，符合"公式统一"设计），但浅色 + 玻璃的组合只有 material 单一样本 | `THEME-GALLERY.md` | 逐主题截图走查浅色系（ember/slate/solarized-light/gruvbox-light/tokyo-night-light）下玻璃发灰/对比度问题 |

**明确不建议做的**：全窗口大面积毛玻璃（可读性与性能双输，苹果自己都不做）；模糊度动画（性能杀手，计划已禁，保持）；为玻璃质感牺牲 AA 对比度（现有门禁是对的，浅色主题尤其要顶住）。

---

## 5. 实施方案（4 批，约 4-6 周）

### Batch 0 — 止血：修漂移（1 天，1 个 PR）

1. **T1 时长统一**：删除 `index.css:266-268` 的 `:root` 覆盖块，以 @theme 的 100/160/240/400 为准（`--ease-glass` 移入 @theme）；`tokens.css` 的镜像段改为注释指针；`generate-themes.mjs` 的回写区加入 duration 校验。顺带把 23 处 `duration-N` 收编到 `duration-(--duration-*)` 任意值语法。
2. **T2 海拔二选一**：保留 @theme 变量为唯一真值，`@utility shadow-eN` 改为 `box-shadow: var(--shadow-eN)`；本轮先不动 109 处 `shadow-sm/md`（留给 Batch 1 codemod）。
3. **T3 icon-xs 归一**：两处定义合并为 index.css 单处（12px），删 tokens.css:144。
4. **T4 图表色主题化**：`theme-source.json` 每主题补 `--chart-series-1..8`（生成脚本已有派生与 AA 校验管线，成本低于预期）。
5. **T6 死代码清理**：删 `press-scale`/`animate-panel-in`/`glass-surface-dark`/`node-connector`/`thought-connector`，给 `custom-scrollbar` 补真实定义或删引用。

**验收**：`pnpm lint` + `test:ci` + `test:e2e`（含视觉基线——预期会触发基线更新，逐一人工确认后 accept）全绿；`tokens.css` 不再含任何生效规则之外的内容。

### Batch 1 — 令牌执行：收编旁路（约 1 周）

1. **C1 字号收编（最大单项）**：`ui/icon.tsx` 默认尺寸改为 `icon-md` 并全量推广（codemod：`material-symbols-outlined` + `text-[Npx]` → `<Icon size>`，机械替换约 298 处）；正文侧把 14/18/11/12px 高频值映射到 `text-body-sm`/`text-label-lg` 等，剩余特例允许 `text-(--text-*)` 任意值语法并加 lint 白名单逐步收紧。
2. **T2 后半 + C3**：codemod `shadow-sm/md/lg` → `shadow-e1/e2/e3`（按悬浮语义映射）；`gap-2/py-2/p-4` → 语义间距；裸 `rounded` → `rounded-sm`。
3. **P1 宽度收敛**：定 3 档内容宽度令牌（`--content-narrow: 48rem` 阅读 / `--content-medium: 62.5rem` 工作台 / `--content-wide: 100rem` 仪表盘），6 种 max-width 归入。
4. **P2 + P3**：Usage 自建 StatCard 合并进 `ui/stat-card`；写一页《状态反馈准则》（错误 = 页面级 ErrorState / 区块级 Banner / 操作级 toast），挂进 `desktop/CLAUDE.md`。
5. **守卫升级**：`check-design-tokens.mjs` 增加规则——禁 `duration-<数字>`、`shadow-sm|md`（allowlist 过渡期）、`text-[<数字>px]`（先 warn 两周再转 error）。

**验收**：`text-[Npx]` 从 667 → <50（图标 0）；`shadow-sm/md` 0；宽度档 3 种；lint 新规则全绿。

### Batch 2 — 玻璃完成度（1-2 周，§4.2 G1-G6 逐项）

1. **G1 材质收编**：Header/Footer 改用 `glass-surface`；25 处 ad-hoc `backdrop-blur-*` 逐个归入两档工具类（scrim 类除外，属遮罩不属玻璃）。
2. **G2 L0 底座**：`theme-source.json` 基区加 `--material-base`，侧栏与内容区拉开一档明度；同步更新 12 主题。
3. **G3 浮层统一**：CommandPalette/DropdownMenu/Tooltip/SidePanel/InlinePanelModal 统一 `glass-overlay`。
4. **G4 sonner 接入**：`theme` 由 `data-theme-mode` 驱动，`toastOptions.classNames` 接语义令牌。
5. **G5 动效**：启用 `press-scale`（Button/Card 可点区）与 `animate-panel-in`（浮层/抽屉进场），motion-reduce 降级已有全局兜底。
6. **G6 预算 CI 化**：`e2e/glass-budget.spec.ts`——chat/tasks/settings/extensions 四页断言同屏 backdrop-filter 元素 ≤4。
7. **G7 走查**：12 主题 × 深浅全量截图（复用 walkthrough 管线），浅色系玻璃专项确认；`THEME-GALLERY.md` 重拍。

**验收**：直写 `backdrop-filter` 的 tsx = 0；四页预算断言绿；12 主题走查报告归档；视觉基线更新。

### Batch 3 — 品牌统一与结构收尾（2 周，可与 Batch 2 并行）

1. **官网品牌统一**（E1）：官网 accent 橙 → 品牌紫 #6b38d4 体系，深色底 + 玻璃语言与桌面端同源（提取桌面端 `themes.css` 的 tokyo-night 基础色入官网 CSS 变量）；品牌规范 `BRAND-GUIDELINES-v0.1.md` 增补"官网章节"。
2. **P6 移动遗留**：删除 `md:` 以下桌面不可达形态（Tauri minWidth 800 > 768 断点），保留能力：若未来出 Web/移动端再按 surface 重建。
3. **P4/P5 小项**：加载态准则并入状态反馈文档；TurnTimeline/QuickFix 标题头收敛到 Header 体系（Extensions H1 保留，有注释论据）。
4. **C4**：HookTaskPipeline 自绘开关换 `ui/switch`；QR 白底加意图注释。
5. **C2（可选，量大）**：`SidebarSessions`/`TerminalPanel` 的裸按钮引入 `buttonVariants` 的 `icon` 系尺寸，先做两处试点再评估全量。

**验收**：官网与桌面端并排截图色彩一致；`md:hidden` 相关死代码 0；基线与 e2e 全绿。

### 度量

- 令牌旁路面：`text-[Npx]`、裸 `shadow-*`、裸 `duration-*`、直写 backdrop-filter 四个计数进 CI 报告，逐批下降。
- 玻璃质感：12 主题走查通过率、四页 backdrop 预算断言、axe 对比度 0 违规（现有门禁保持）。
- 回归守卫：视觉基线从 3 页扩到 8 页（补 triage/extensions/memory/usage）。

---

## 6. 附：本次审查的证据基线

- 代码审查：四路并行深查（主题令牌体系 / 13 页面+路由 / 221 组件 + grep 统计 / 文档考古），关键 8 项发现（T1-T4、G1、G4、C4、死代码）经人工逐条复核。
- 截图走查：`docs/design/ui-audit-2026-09/screenshots/`（18 页 + 12 主题）对比 `desktop/ui/e2e/visual-baseline.spec.ts-snapshots/`（当前 chat/tasks/settings）。
- 既有计划对齐：`UI-IMPROVEMENT-PLAN-2026-09.md`（Wave 1/2 已落地、遗留"逐页深色打磨/Extensions 目录化/多窗口"三条不在本方案重复，建议随后排期）；`2026-09-23-ui-nav-ia-redesign-proposal.md`、`2026-09-26-desktop-ui-pages-review.md` 各项已合入 dev，不重复立项。
- 仓库内 `desktop-screenshot.png`/`tauri-desktop-screenshot.png` 为 6 月旧 TUI 截图（内容与桌面 UI 无关），建议删除或更新，避免误导。
