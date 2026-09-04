# Shannon Desktop UI 现代化实施方案

> 状态:全部 12 任务已落地(基线 `dev` @ `6cc658fc` 2026-08-25);吸收 / 修正细节见二期审计文档 `docs/plans/desktop-ui-modernization-phase2.md`
> 范围:`desktop/ui/`(包 `shannon-desktop-ui` v0.6.0)纯前端;不涉及 Rust 侧(`shannon-desktop` crate)与 tauri.conf
> 性质:增量收敛,**不是重写**。现有 MD3 主题体系、a11y、测试设施全部保留
> 参照实现:`shannon-service/shannon-go/stack-shannon-go/apps/web`(shadcn base-nova,已验证可行)、`desktop/ui/design/`(同配方原型)

## 0. 背景与目标

代码库现状是"手工移植的半个 shadcn":19 个原语按 shadcn 约定写(cva + data-slot),但没接 CLI(components.json 缺失、`@import "shadcn/tailwind.css"` 被注释为 broken);原语采用率低(208 个裸 `<button>` vs 111 处 `<Button>`);24 个文件各自手写 `fixed inset-0` 弹窗;cn() 基本只在 ui/ 内使用。

本方案四个目标:

1. **接通 shadcn CLI**(base-nova 样式,与 apps/web 同配方),让原语可生成、可维护
2. **收敛重复造轮子**:弹窗/抽屉/命令面板统一到标准原语
3. **有限引入 ReactBits**:Welcome/空状态等第一印象面,2–4 个组件,禁 WebGL
4. **结构卫生**:拆超大文件、补 ESLint、清死依赖

## 总原则

- 单 PR 单主题(按 feature 分批),每 PR 必须全绿:`pnpm lint` + vitest + Playwright 4 spec
- 迁移保持对外 props/行为兼容:组件内部换实现,调用点小改或零改
- 每完成一个任务,在本文档"任务状态"表打勾并更新实测计数(验收 grep 命令见附录 B)
- 提交信息走 conventional commits:`feat(desktop-ui):` / `refactor(desktop-ui):` / `chore(desktop-ui):`
- 文档本身留在本地工作区,不进仓库(仓库惯例)

## 任务总览

| ID | 任务 | 阶段 | 规模 | 依赖 | 建议粒度 |
|----|------|------|------|------|----------|
| T0.1 | shadcn 接线(components.json + import + 变量扩充) | P0 | S | 无 | 1 PR |
| T0.2 | 图标政策定案(Material Symbols 唯一 + Icon 封装) | P0 | S | 无 | 1 PR |
| T0.3 | motion 依赖决策 | P0 | XS | 无 | 并入 T0.1 PR |
| T0.4 | ESLint 引入 | P0 | S–M | 无 | 1 PR |
| T1.1 | 13 个手写原语替换为 CLI 生成版 | P1 | M | T0.1, T0.2 | 2–3 PR(分批) |
| T1.2 | 弹窗收敛(24 个 fixed inset-0 → Dialog/Sheet/AlertDialog) | P1 | L | T1.1 | 4–6 PR(按 feature) |
| T1.3 | CommandPalette → cmdk | P1 | S–M | T1.1 | 1 PR |
| T1.4 | Button / cn() 采用率收敛 | P1 | M(滚动) | T1.1 | 随 T1.2 各 PR 顺手做 |
| T2.1 | 动画守卫基础设施(reduced-motion + 失焦暂停) | P2 | S | 无(可与 P1 并行) | 1 PR |
| T2.2 | ReactBits Welcome 试点(2–4 组件) | P2 | S–M | T0.1, T2.1 | 1–2 PR |
| T3.1 | 大文件拆分(8 个 >500 行) | P3 | M | 建议在 T1.2 之后 | 随各 feature PR |
| T3.2 | lucide-react 依赖移除 | P3 | XS | T0.2, T1.1 完成 | 并入收尾 PR |

里程碑:**M1 = P0 全部** → **M2 = T1.1** → **M3 = T1.2 + T1.3** → **M4 = T1.4 滚动** → **M5 = P2** → **M6 = P3 收尾**。T2.1 可与 P1 并行开发。

---

## P0 卫生

### T0.1 shadcn 接线

**目标**:让 `desktop/ui` 成为可被 shadcn CLI 管理的项目,恢复被注释的样式入口,补齐 base-nova 所需的 CSS 变量——完成"已开始的迁移"的第一步。

**内容**:
1. 确认 `shadcn` 运行时包:对照 `stack-shannon-go/apps/web/package.json` 里承载 `@import "shadcn/tailwind.css"` 的依赖名与版本,在 desktop/ui 安装同样依赖
2. 新建 `desktop/ui/components.json`:
   - `style: "base-nova"`(与 apps/web、design/ 原型一致)
   - `rsc: false`、`tsx: true`
   - tailwind 指向 `src/index.css`(CSS-first v4,无 config 文件)
   - aliases:`src/components`、`src/lib`、`src/hooks`
3. 恢复 `src/index.css:3` 的 `@import "shadcn/tailwind.css"`(删注释与 "broken" 备注),验证与 `@theme` MD3 token(13–115 行)无冲突
4. 变量对齐:拉一个参考组件(diff 生成代码里的 `var(--popover)`、`--card`、`--dialog` 等),对照现有别名层(`index.css:290-337`)缺哪些,补进**全部 13 个 `[data-theme]` 块**(321-915 行)——漏块会导致该主题下组件无色
5. 试拉一个低风险组件验证全链路:`pnpm dlx shadcn@latest add separator`,确认落盘 `src/components/ui/separator.tsx` 且类型通过

**验收标准**:
- [ ] `components.json` 存在,style 为 base-nova
- [ ] `index.css` 无注释残留的 broken import;`grep -n "shadcn/tailwind.css" src/index.css` 显示生效的 import
- [ ] `pnpm dlx shadcn@latest add separator` 成功,tsc 通过
- [ ] `pnpm lint` + `pnpm test` 全绿;Playwright smoke 通过
- [ ] 至少 3 个主题(material 浅色 + dracula + tokyo-night)下目检无变量缺失(新拉组件颜色正常)

**实施方案**:先读 apps/web 的 `components.json` 与 `index.css` 头部作模板;变量对齐以"生成组件实际引用的 var() 清单"为核对表,逐主题块过;separator 作为接线验证留在 ui/ 目录即可(后续自然会用)。

**涉及文件与模块**:`desktop/ui/package.json`、`desktop/ui/components.json`(新)、`desktop/ui/src/index.css`、`desktop/ui/src/components/ui/separator.tsx`(新);参照:`stack-shannon-go/apps/web/components.json`、`apps/web/src/index.css`

### T0.2 图标政策定案

**目标**:确立 Material Symbols 为唯一图标体系,建立生成组件的图标替换流程,终结 lucide/Material 双轨。

**内容**:
1. 定案记录:图标一律用 Material Symbols(`material-symbols-outlined` class,105 文件现状),lucide 仅允许作为 shadcn 生成代码的中间态,落地前替换
2. 新建统一封装 `src/components/ui/icon.tsx`:渲染 `<span class="material-symbols-outlined" aria-hidden>{name}</span>`,统一 font-variation-settings 与尺寸 class——后续替换生成组件的 lucide import 时机械换成 `<Icon name="..."/>`
3. 替换现有 3 个使用 lucide-react 的文件(定位:`grep -rl "lucide-react" src`),视觉语义对齐(lucide `X` → Material `close` 等)
4. 图标规范写入 `desktop/ui/README.md`(无则新建)或 `shannon-mono/CLAUDE.md` Desktop 段落:一节"Icon policy"

**验收标准**:
- [ ] `grep -rl "lucide-react" desktop/ui/src` 结果为空(依赖本身留到 T3.2 再从 package.json 移除)
- [ ] `icon.tsx` 存在且有单测(渲染 name、aria-hidden)
- [ ] 3 个替换文件的图标视觉等效(目检)
- [ ] 图标规范文档落地

**实施方案**:lucide 图标名 → Material Symbols 名做一张小映射表放进 icon.tsx 注释(close, check, chevron_right, search, settings, delete, add, remove, warning, error, info…),生成组件替换时查表。

**涉及文件与模块**:`desktop/ui/src/components/ui/icon.tsx`(新)、3 个 lucide 使用文件(以 grep 定位)、`desktop/ui/README.md` 或 `shannon-mono/CLAUDE.md`

### T0.3 motion 依赖决策

**目标**:消除 package.json 里的悬空状态(motion 12.40 当前零 import)。

**决策**:**保留**,理由:P2 ReactBits 试点大概率用到 motion 变体组件(Animated Content 等),保留可让 P2 直接可用;若 P2 结束后确认走纯 CSS 路线,再删。决策记录写入本节,不再反复。

**验收标准**:
- [ ] 本节含决策与理由(即本文档即记录,无代码改动)

**涉及文件与模块**:无(纯决策;若选删除则动 `package.json`)

### T0.4 ESLint 引入

**目标**:补 lint 缺口——现状 `pnpm lint` 仅为 `tsc --noEmit`,react-hooks 规则等完全缺失。

**内容**:
1. ESLint 9 flat config(`eslint.config.js`):`typescript-eslint`(recommended)+ `eslint-plugin-react-hooks`;不引入 prettier/格式化(避免风格之争)
2. script 改造:`"lint": "tsc --noEmit && eslint src"`(保持一键)
3. 首跑清零存量违规:能修则修,确属误报的用行内注释豁免并注明原因;**不许**降低规则回避问题
4. 接入现有 CI/lint 入口:若 justfile/CI 已有 desktop 前端检查步骤则挂入,没有则确认 CI 里 desktop 构建路径并补(以实际 recipe 为准,先 `grep -n "desktop" justfile`)

**验收标准**:
- [x] `pnpm lint` 双通道通过(tsc + eslint 零错误)→ 0 errors, 70 warnings (consistent-type-imports / exhaustive-deps)
- [x] 存量违规清零或显式豁免清单(数量与理由记录在本文档任务状态表)→ 22 errors 全修(19 no-useless-escape + 2 no-unused-expressions + 1 真 hook 误命名)
- [x] CI 中该检查生效(以 CI 实际配置为准)→ `desktop/scripts/local-check.sh:30` 自动 pickup `pnpm lint` 变更

**实施方案**:eslint 配置从最小集起步(recommended),跑一遍看存量规模再决定是否需要分批清;react-hooks 的 `exhaustive-deps` 警告级、error 级规则按 recommended 默认。

**涉及文件与模块**:`desktop/ui/eslint.config.js`(新)、`desktop/ui/package.json`、`pnpm-lock.yaml`、`justfile` / `.github/workflows/*`(以 grep 定位)

---

## P1 shadcn 收敛

### T1.1 手写原语替换为 CLI 生成版

**目标**:`src/components/ui/` 的 13 个纯手写原语统一为 CLI 生成的 base-nova 版本(底层 Base UI,与现有 6 个 Base UI 原语同源),消除自维护负担、统一 focus/Esc/滚动锁定行为。

**内容与分批**(按被引用数与风险排批):

| 批次 | 组件(引用数) | 风险 | 状态 |
|------|----------------|------|------|
| A1 | badge(3)、card(1)、tooltip(0)、pagination(2) | 低:引用少 | ✅ done (2026-08-23) |
| A2 | dropdown-menu(0) | 中:Base UI Menu 新依赖 | ✅ done (2026-08-23,compat shim) |
| B | textarea(3)、modal(5)、confirm-dialog(12)、drawer(以 grep 复测) | 中:引用多,涉及弹窗行为 | ⏳ |
| C(保留改造) | empty-state(10)、error-state(4)、loading-state(8)、banner(2) | 非纯原语:保留,内部改用 base-nova 原语组合,不换名不换 API | banner 已迁入 |

**每组件的替换步骤**:
1. `pnpm dlx shadcn@latest add <name>`(生成到 `src/components/ui/`)
2. diff 生成版与现有版的 props API;若调用点会大面积变,写薄适配层保持旧 props(过渡期),收敛完调用点再删
3. 按 T0.2 流程替换生成代码里的 lucide import
4. 生成代码引用的新 CSS 变量按 T0.1 的核对表补进 13 个主题块
5. 更新全部调用点,`grep -rl "from.*components/ui/<name>"` 清零旧路径
6. 跑 lint + vitest;弹窗类(dialog/drawer/confirm)手工抽检 focus trap、Esc、滚动锁定

**验收标准**:
- [ ] 批次 A、B 组件均为 CLI 生成版(文件头可辨识,base-nova 结构),旧手写实现删除
- [ ] 批次 C 三件保留且内部基于标准原语,对外 API 不变
- [ ] 全部调用点更新,`pnpm lint` + vitest + Playwright 全绿
- [ ] confirm-dialog 的 12 处调用逐一冒烟(确认弹窗是危险操作入口,重点回归)
- [ ] 现有 a11y 测试(`__tests__/Accessibility.test.tsx`)不回退

**涉及文件与模块**:`desktop/ui/src/components/ui/`(badge、banner、card、confirm-dialog、drawer、dropdown-menu、empty-state、error-state、loading-state、modal、pagination、textarea、tooltip)+ 全部调用点(tasks/ 26、extensions/ 14、settings/ 10、chat/ 8 等分布)

### T1.2 弹窗收敛

**目标**:24 个手写 `fixed inset-0` 遮罩弹窗/抽屉迁移到 Dialog/AlertDialog/Sheet 标准原语,删除各自手写的 focus/退出/过渡逻辑。

**内容**:
1. 建清单:`grep -rl "fixed inset-0" src --include="*.tsx" | grep -v __tests__`,逐个分类:
   - 模态型 → Dialog(经 T1.1 的 base-nova 版)
   - 确认型 → AlertDialog(或继续走已有 ConfirmDialog 封装)
   - 抽屉型 → Sheet/Drawer
   - **非弹窗的全屏视图**(如全屏编辑器)→ 列入白名单记录在本文档,不强迁
2. 迁移模式:各组件**对外 props 不变**,内部 render 换原语结构;手写的焦点管理/滚动锁定/Esc 处理删除(原语接管);过渡动画用 tw-animate-css 的 animate-in/out 对齐现有 `shannon-dialog-in` 观感
3. 按 feature 分 PR 迁移(顺序按风险从低到高):
   1. `diff/DiffDialog`、`chat/ResearchReportModal` 等单点弹窗
   2. `tasks/`(CancelTaskModal、TaskDetailDrawer 等)
   3. `extensions/`(含 762 行的 McpAddServerDialog——先迁 Dialog,拆分留给 T3.1,两件事分 PR)
   4. `KeyboardShortcutsHelp` 及其余
   5. `CommandPalette` 不在本任务(见 T1.3)
4. 每批 PR:lint + vitest + Playwright;弹窗行为抽检(Esc 关闭、焦点回到触发元素、背景滚动锁定)

**验收标准**:
- [ ] `grep -rl "fixed inset-0" src --include="*.tsx" | grep -v __tests__ | wc -l` 归零,或仅剩白名单(白名单逐条列在本文档并说明为何不是弹窗)
- [ ] 每个迁移弹窗的现有功能测试通过;抽检 focus/Esc/滚动锁定行为不回退
- [ ] 各 PR 全绿;迁移期间无 behavior 变更类 bug 遗留(以测试与抽检为准)

**涉及文件与模块**:24 个文件(已知含 `tasks/CancelTaskModal`、`tasks/TaskDetailDrawer`、`diff/DiffDialog`、`chat/ResearchReportModal`、`extensions/McpAddServerDialog`、`KeyboardShortcutsHelp`、`CommandPalette` 等,完整清单以 grep 为准)、`src/components/ui/dialog.tsx`、`alert-dialog`、`sheet`(T1.1 产出)

### T1.3 CommandPalette → cmdk

**目标**:手写命令面板换成 cmdk(经 shadcn `command` 组件包装),获得成熟的过滤/分组/键盘导航。

**内容**:
1. `pnpm dlx shadcn@latest add command`(即 cmdk + 包装)
2. 迁移 `CommandPalette.tsx`:现有命令注册数据源不动,渲染与过滤/上下键逻辑换成 Command/CommandInput/CommandList/CommandGroup/CommandItem
3. 保留现有快捷键绑定(打开键等)与 `data-theme` 下的样式观感(样式变量自然继承,必要时微调)
4. lucide 替换按 T0.2 流程

**验收标准**:
- [ ] 打开/输入过滤/分组导航/上下键/回车执行/Esc 关闭全部等价
- [ ] Playwright smoke 增加或更新一条"打开面板并执行一条命令"的用例
- [ ] 现有快捷键行为(KeyboardShortcutsHelp 所载)不回退

**涉及文件与模块**:`desktop/ui/src/components/CommandPalette.tsx`、`src/components/ui/command.tsx`(新)、相关 e2e spec

### T1.4 Button / cn() 采用率收敛(滚动任务)

**目标**:裸 `<button>` 从基线 208 显著收敛;feature 代码 className 拼接统一经 cn()。

**内容与优先级**:
1. **危险操作**(destructive 语义)裸 button 全部换 `<Button variant="destructive">` 或 ConfirmDialog 流程——最高优先,涉及取消任务/删除等
2. primary CTA(提交/保存)
3. settings/extensions 表单区
4. 其余顺手替换
- cn() 推广与 button 替换同文件同步做(模板字符串拼接 → cn())
- 不追求 100%:一次性/实验性 UI 允许裸写,但新增代码的规范靠 T0.4 的 ESLint + 文档约定

**验收标准**:
- [ ] `grep -rc "<button" src --include="*.tsx"`(排除 __tests__ 与 ui/)从 208 降到 ≤ 60,余量逐类说明(或全部清完)
- [ ] 危险操作类裸 button 清零(grep destructive/delete/cancel 相关文件复测)
- [ ] cn() 使用文件数显著上升(基线 15,其中 13 在 ui/;目标:ui/ 外 ≥ 40)
- [ ] 每批替换后视觉抽检:尺寸/圆角/焦点环与原样式一致

**涉及文件与模块**:分布广——tasks/、extensions/、settings/、chat/、artifact/、opc/ 等;`src/components/ui/button.tsx`、`src/lib/utils.ts`(cn)

---

## P2 ReactBits 试点

### T2.1 动画守卫基础设施

**目标**:为一切 JS 动画提供统一的 reduced-motion 与失焦暂停守卫(ReactBits 组件不自带)。

**内容**:
1. `src/hooks/useReducedMotion.ts`:`matchMedia('(prefers-reduced-motion: reduce)')` 订阅式 hook,含单测
2. 窗口失焦暂停:Tauri window focus/blur 事件(复用现有 `useTauriEvent` 模式),输出 `paused` 状态供动画组件条件暂停 rAF
3. 测试基建:vitest setup 里 mock `matchMedia`(jsdom 不实现),默认 reduced-motion 让动画测试稳定
4. 可选薄封装 `<MotionGuard paused fallback>`:reduced-motion 或 paused 时直接渲染静态子树

**验收标准**:
- [ ] hook 单测通过(变化时重渲染)
- [ ] 任一试点组件在 reduced-motion 下无动画但内容完整渲染
- [ ] vitest 中动画相关测试无 flake(跑 3 遍验证)

**涉及文件与模块**:`desktop/ui/src/hooks/useReducedMotion.ts`(新)、`src/hooks/useTauriEvent.ts`(参照)、`vitest.setup.ts`(文件名以现有为准)、试点组件

### T2.2 ReactBits Welcome 试点

**目标**:Welcome/空状态引入 2–4 个 ReactBits 组件,验证观感收益与维护成本,为是否扩大使用提供依据。

**候选组件**(CSS-only 优先;最终由实施时视觉评审定,数量控制在 4 以内):
- Gradient Text / Shiny Text——Welcome 主标题强调
- Count Up——欢迎页统计数字(如有)
- Text Loop——副标语轮播

**硬性边界**:
- 只取 **TS-TW** 变体;安装走 `pnpm dlx shadcn@latest add @react-bits/<Name>-TS-TW`(与 T0.1 同链路)
- **禁止引入任何 WebGL/OGL 依赖**:验收 grep `ogl|three` 为零——桌面工具常驻 + Linux WebKitGTK GPU 加速弱,rAF 背景(粒子/silk/aurora 类)一律不用
- 改造落位 `src/components/reactbits/`:替换硬编码渐变色为 `data-theme` CSS 变量(现有 token 体系),接 T2.1 守卫
- **不用 Pro 组件**(付费层;免费层 MIT + Commons Clause,商用免费,仅限制"售卖"该软件本身——Shannon 开源桌面 + 云端服务收费模式不触及)

**验收标准**:
- [ ] 2–4 个组件上线 `pages/Welcome.tsx`,经视觉评审通过
- [ ] `grep -rE "from ['\"](ogl|three)" src` 为零;package.json 无新增 WebGL 依赖
- [ ] reduced-motion 下优雅降级;窗口失焦不持续耗 CPU(任务管理器抽检)
- [ ] 至少 3 个主题(material 浅色 + dracula + tokyo-night)下颜色正确
- [ ] Playwright Welcome 页 smoke 通过
- [ ] 试点结论(是否扩大、扩到哪些面)回写本文档"任务状态"

**涉及文件与模块**:`desktop/ui/src/pages/Welcome.tsx`(724 行,T3.1 拆分前先小步改)、`src/components/reactbits/`(新)、`src/components/WelcomeState.tsx`

---

## P3 结构收尾

### T3.1 大文件拆分

**目标**:8 个 >500 行文件拆分到 <500(理想 <300),降低维护与评审成本。

**对象与拆分方向**(按行数排序;具体切口实施时定):

| 文件 | 行数 | 拆分方向 |
|------|------|----------|
| settings/ModelsSettings.tsx | 834 | 按 tab/section 抽子组件到 settings/models/ |
| pages/Chat.tsx | 783 | 消息列表/输入区/工具调用渲染分块(注意:不动 ChatV2Spike,那是独立 spike) |
| extensions/McpAddServerDialog.tsx | 762 | 表单步骤/校验/预览分块(在 T1.2 迁 Dialog 之后做,分 PR) |
| pages/Welcome.tsx | 724 | 分步内容抽组件(与 T2.2 协调顺序) |
| settings/ConnectionsSettings.tsx | 668 | 同 ModelsSettings 模式 |
| 其余 3 个 >500 行 | — | 以 grep 复测清单为准 |

**验收标准**:
- [ ] `find src -name "*.tsx" | xargs wc -l | sort -rn | head -10` 中无 >500 行非测试文件
- [ ] 拆分纯移动+重组,行为不变:lint + vitest + Playwright 全绿
- [ ] 拆出的子组件落位对应 feature 子目录,命名沿用现有约定

**涉及文件与模块**:上表 8 文件 + 新建各 feature 子目录组件

### T3.2 lucide-react 依赖移除

**目标**:图标单轨收尾。

**内容**:T0.2 与 T1.1 全部完成、生成组件的 lucide 替换流程稳定后,`pnpm remove lucide-react`,更新 lockfile。

**验收标准**:
- [ ] package.json 无 lucide-react;`pnpm lint` + build 通过
- [ ] `grep -r "lucide" src` 为零

**涉及文件与模块**:`desktop/ui/package.json`、`pnpm-lock.yaml`

---

## 每阶段验证命令

```bash
cd desktop/ui
pnpm lint                     # tsc --noEmit(T0.4 后含 eslint)
pnpm test                     # vitest(单线程,慢但稳)
pnpm exec playwright test     # 4 个 spec(webServer 自动起 pnpm demo / VITE_MOCK_MODE=1)
```

Playwright 浏览器安装遇阻时用 npmmirror 镜像(既有经验)。Rust 侧不受影响,无需 cargo 检查(不动 `desktop/src-tauri`)。

## 风险与回滚

| 风险 | 缓解 |
|------|------|
| 变量别名漏补某主题块 → 该主题下组件无色 | T0.1 建立"生成代码引用的 var() 清单"核对表;每批替换后 ≥3 主题目检 |
| Base UI 版本与 shadcn 生成代码 API 漂移 | 现状 1.5.0 与 apps/web(TW 4.1/React 19)同配方已验证;生成后 tsc 即时暴露 |
| confirm-dialog(12 调用)迁移引入危险操作行为回退 | 单独成批 + 逐一冒烟 + 保留对外 API |
| "fixed inset-0"误伤全屏视图 | 白名单机制,逐条确认归类 |
| vitest 单线程随测试增多变慢 | 本期接受;后续按目录分组并行另立项 |
| ReactBits 许可证(Commons Clause) | 免费层商用免费已由官方声明;桌面端若转付费产品前复审一次 |
| 每步回滚 | 单 PR 单主题 + conventional commits,git revert 即可;无数据/协议层改动 |

## 附录 A:基线数据(2026-08-23 实测,验收时复测对比)

- 313 个 ts/tsx;非测试 tsx ≈ 27,517 行,非测试 ts ≈ 6,213 行;123 个测试文件(112 用 RTL)
- ui/ 19 原语 = 6 Base UI(button/select/input/tabs/switch/scroll-area)+ 13 手写
- 裸 `<button>` 208 vs `<Button>` 111(34 文件 import)
- 原语引用数:ConfirmDialog 12、EmptyState 10、Input 9、LoadingState 8、Modal 5、Select 4、Badge 4、Switch 4、ErrorState 4、Banner 3、Pagination 3、ScrollArea 3、Textarea 3、Card 2、DropdownMenu 1、Tooltip 1、Tabs 0
- `fixed inset-0` 手写弹窗:24 个非测试文件
- cn():15 文件(13 在 ui/);硬编码 hex 12 处(11 在 ScheduleDAGView)
- 主题:13 个 `[data-theme]` 块(1 默认浅 + 8 深 + 3 浅变体)
- 动画:全 CSS(239 transition-*、46 animate-*、2 自定义 keyframes);motion 12.40 为零引用死依赖
- 图标:Material Symbols 105 文件 vs lucide 3 文件
- >500 行文件 8 个(最大 ModelsSettings 834)
- lint 现状:仅 tsc,无 ESLint

## 附录 B:验收复测命令

```bash
cd desktop/ui
grep -rc "<button" src --include="*.tsx" | grep -v __tests__ | awk -F: '{s+=$2} END {print s}'   # 裸 button 计数(基线 208)
grep -rl "fixed inset-0" src --include="*.tsx" | grep -v __tests__ | wc -l                        # 手写弹窗计数(基线 24)
grep -rl "from.*lib/utils" src --include="*.tsx" | wc -l                                          # cn() 使用文件数(基线 15)
grep -rl "lucide-react" src | wc -l                                                               # lucide 残留(基线 3)
grep -rn "var(--" src/components/ui/dialog.tsx 2>/dev/null | wc -l                                 # 新原语变量引用(对核对表)
find src -name "*.tsx" -not -path "*__tests__*" | xargs wc -l | sort -rn | head -12                # 大文件复测
```

## 任务状态

| ID | 状态 | 备注 |
|----|------|------|
| T0.1 | ✅ 完成 | shadcn 4.19.0 装妥;components.json (base-nova);separator.tsx 落地;tsc/vite build 通过;vitest 慢为已知非本次引发 |
| T0.2 | ✅ 完成 | <Icon> 封装带映射表+单测;3 个 lucide 文件(modal/select/SessionsPanel)已替;grep lucide-react=0;policy 写进 desktop/CLAUDE.md;tsc/build 通过 |
| T0.3 | ✅ 决策完成 → **已删除** | 原决策"保留 motion"在二期被推翻:R6 删包,落地于 `feature/desktop-ui-r4-textloop-cleanup` (R4+R6 同 PR,合入 `31d3abc7`) |
| T0.4 | ✅ 完成 | ESLint 9 flat config + react-hooks plugin 落地;lint 脚本 = tsc + eslint;首跑 22 errors 全修(19 no-useless-escape + 2 no-unused-expressions + 1 真 hook 误命名);72 warnings 冻结线已由二期 R3 固化为 `--max-warnings 72` |
| T1.1 | ✅ 完成(A1/A2/B/C 全闭合) | A1/A2 在一期 `feat/desktop-ui-t1.1a-primitives` 合入(`cd03a40e`);B 批(drawer/textarea/confirm-dialog/modal)由二期 `SidePanel`/`Modal` 迁移吸收(R1b/R1c/R2);C 批(empty/error/loading-state)在一期合并线由 R1c "C 批核对"收口 |
| T1.2 | ✅ 完成(残尾由 R2 收口) | 20 Modal + 3 SidePanel 主线在一期 `feat/desktop-ui-t1.2-modals-batch1` 合入(`d3d69972`);QuickFixDrawer + 4 文件头注释 + 自研 useModalFocus + 白名单脚本由二期 `feat/desktop-ui-r2-quickfix-overlay-cleanup` 收口(`e6a8b906`) |
| T1.3 | ✅ 完成 | CommandPalette → cmdk(shadcn `command` 包装),在一期合并线落地(`d3d69972`);playwright 用例随之扩充 |
| T1.4 | ✅ 完成 | cn() 采用率滚动任务,T1.4a/b/c/d 在一期合并线全部完成(特性代码模板字面量清零);T1.4 验收 grep 复测见附录 B |
| T2.1 | ✅ 完成 | `useReducedMotion` + `useWindowBlur` 双 hook 在一期合并线落地(`d3d69972`);二期 R4 进一步把 `__tests__/setup.ts` 改成 query-aware(reduced-motion 测试命中静态分支) |
| T2.2 | ✅ 完成(混合方案定案) | CSS-only ReactBits 等价物(`GradientText`)上线 Welcome;`TextLoop` 在二期 R4 接线到 WelcomeState 副标语(`20d451bc`);CountUp 一期试点后无家,二期 R4 删除 |
| T3.1 | ✅ 完成 | Editor/InstallDialog/McpAddServerDialog/ConnectionsSettings/AddProviderModal/Chat/ModelsSettings/MemoryPanel/Welcome 共 9 个 >500 行文件拆分(orchestrator + 子组件),覆盖原计划 8 个目标 + 额外 1 个 |
| T3.2 | ✅ 完成 | lucide-react 引用清零(`5689c4ee` T3.2);包本身随 R6 motion 同期移除 |

> **一期基线已于 2026-08-25 全部落地**;合并拓扑、审计问题清单与 R1–R7 收尾细节见二期文档 `docs/plans/desktop-ui-modernization-phase2.md`(本文档的姊妹)。
