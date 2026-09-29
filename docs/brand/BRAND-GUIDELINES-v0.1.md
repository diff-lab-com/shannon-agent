# Shannon 品牌规范（初稿 v0.1）

> 状态：工程侧初稿（2026-09-18，PR fix/select-commit 批次 D8）。视觉稿与 logo 字形仍需设计输入；
> 本文先把**已代码化的品牌事实**（tokens.css / 主题系统 / 文案口径）沉淀为单一参考。

## 1. 名称与口号

| 项 | 内容 | 出处 |
|---|---|---|
| 产品名 | **Shannon**（应用内大写首字母；crate/命令行统一小写 `shannon`） | README |
| 副标题 | Your AI Workspace | Sidebar 品牌块 |
| 主口号（EN） | Open source. Total control. Keys never leave your machine. | marketing plan |
| 主口号（CN） | 完全开源，尽在掌控；密钥不出门，数据不搬家。 | README.zh-CN |
| 屋顶信息 | Agent 的下一个战场不是更大的模型，而是谁控制 agent 的运行时。 | marketing plan |

## 2. 色彩

唯一权威来源：`desktop/ui/src/styles/tokens.css`（改动须经 `check-design-tokens` 与 AA 对比度门禁）。

| 角色 | Token | 值 / 规则 |
|---|---|---|
| 品牌主色 | `--color-primary` | **紫 #6b38d4**（"Brand purple — primary actions, focus ring"） |
| 暗色默认主题 | tokyo-night | 深色优先（对齐 Codex/ZCode 开箱深色） |
| 主题族 | 12 套 | material / tokyo-night(+light) / catppuccin / nord / ember / slate / solarized(+light) / dracula / gruvbox(+light) |
| 材质 | Liquid Glass | 三级 surface token（base/surface/overlay），`--glass-tint-alpha` 深/浅 0.62/0.48 |
| 语义色 | success/warning/error/secondary/tertiary | MD3 角色；**禁止**在组件里使用裸调色板类（lint 强制） |

对比度纪律：所有主题组合必须过 axe AA（e2e/themes.spec.ts 全主题扫描）。

## 3. 字体与图标

| 项 | 值 |
|---|---|
| 正文/标签 | **Inter Variable**（`@fontsource-variable/inter`，全站默认） |
| 图标 | **Material Symbols Outlined**（`@fontsource-variable/material-symbols-outlined`） |
| 图标规则 | 装饰性图标必须 `aria-hidden="true"`（typeahead/读屏纯净——2026-09 Select 事件教训） |
| 等宽 | 按调用点 opt-in（token/代码/耗时等数据型文本用 `font-mono`） |
| 密度 | Comfortable（默认）/ Compact（`html[data-density]` 缩放共享字号与间距 token） |

## 4. 形状与间距

- 圆角：卡片 `rounded-2xl`、控件 `rounded-lg/xl`、chips `rounded-full`（气泡 `rounded-tr-none / rounded-tl-none` 表示对话方向）。
- 间距：仅用 `--spacing-{xs..xl}` 缩放族（compact 档整体 ~10% 收紧）。
- 玻璃拟态：`glass-panel` / `glass-surface`，hover 微位移 + `shadow-primary/30` 发光仅限主 CTA。

## 5. 术语表（文案口径，lint 部分强制）

| 用 | 不用 | 说明 |
|---|---|---|
| 会话 / Chat | 任务（指聊天时） | 知识工作者心智；"运行/Goal"语义另用 |
| 例行任务 / Routines | ~~定时任务~~ / 已排程 | 退役术语（lint 黑名单） |
| 收件箱 / Inbox | ~~分流队列~~ / Triage（对用户） | |
| 多方案对比 / Best-of-N | ~~并行方案~~ | |
| 对话 / Diff / 预览 | ~~聚焦聊天~~ | 视图 tab 命名 |
| 上下文 / 计划 / 产物 / Diff | — | 右侧 dock 四 tab |

## 6. Logo

**待设计**。当前仓库仅有应用图标 `desktop/icons/`（icon.png / tray-icon.png，无矢量源、无使用规范）。
建议下一步：以 `cognitive`（现 Sidebar 品牌图标）为字形基础做矢量化和留白/最小尺寸/单色规范。

## 7. Website / 官网（2026-09-29 增补）

官网（`website/`，Astro 5 + React islands）的 accent 自橙色 `#f97316` 体系**统一回品牌紫 #6b38d4 体系**（UI 审查计划 Batch 3.1 / E1），深色底与玻璃材质语言与桌面端同源。落地文件：`website/src/styles/global.css`（token 定义）+ 各 landing 组件。

| 项 | 值 / 规则 |
|---|---|
| 深色底 | `--bg: #0f0f14`（不变；官网自有中性阶，不随桌面主题切换） |
| 品牌 accent | `--accent: #6b38d4`（与桌面 `--color-primary` 同值；仅用于填充：按钮、渐变、选中态） |
| accent hover | `--accent-hover: #8455ef`（取自桌面 `--color-primary-container`） |
| accent 文字态 | `--accent-bright: #a78bfa`，hover `--accent-bright-hover: #c4b5fd`（深底文字对比度 ~7:1，过 AA；链接/序号/表头等**文字一律用亮档**，禁用 #6b38d4 直接做深底文字色） |
| accent 弱化底 | `--accent-soft: rgba(139, 92, 246, 0.14)`（chips、侧栏激活项背景） |
| 品牌渐变 | `linear-gradient(135deg, #8b5cf6, #6b38d4 50%, #4f46e5)`（紫主导 → 辅助冷色 indigo；logo 块与 favicon.svg 同源） |
| 主 CTA 光晕 | hover 时 `box-shadow: 0 8px 24px rgba(107, 56, 212, 0.35)`，仅限 `.btn-primary`（对齐 §4 "发光仅限主 CTA"） |
| 终端拟物装饰 | macOS 三色灯与 prompt/tool 语法色取 Tokyo Night（桌面默认主题）语义色：红 `#f7768e` / 琥珀 `#e0af68` / 绿 `#9ece6a` / 蓝 `#7aa2f7` / 紫 `#9d7cd8`，不得回退到通用 macOS 糖果色 |

**官网玻璃配方**（营销页档，对齐桌面 `glass-surface`；不受桌面"同屏 ≤4 backdrop"预算约束，但克制在导航栏 + hero + 主要卡片）：

```css
background: rgba(26, 26, 36, 0.55);            /* --paper @ 55%（--glass-bg） */
backdrop-filter: blur(20px) saturate(1.4);
border: 1px solid rgba(255, 255, 255, 0.08);   /* hairline（--glass-border） */
box-shadow: inset 0 1px 0 rgba(255,255,255,0.06),  /* 顶部内高光（--glass-highlight） */
            0 8px 32px rgba(0, 0, 0, 0.35);        /*（--glass-shadow） */
```

变体：`.glass-cta`（CTA banner 叠加紫晕渐变）、`.glass-terminal`（终端窗用 `rgba(17,17,27,0.68)` 深代码底）。两者均带 `@supports not (backdrop-filter)` 与 `prefers-reduced-transparency` 实心回退（`--paper` / `--code-bg`）。

**权威来源不变**：桌面端 token（`desktop/ui/src/index.css` + `desktop/ui/src/theme/generated/themes.css`，经 `desktop/ui/src/styles/tokens.css` 沉淀）仍是全局唯一权威来源；官网 CSS 变量是**派生镜像**，改动官网色值须同步核对桌面端 token，反之桌面端改动（尤其 `--color-primary`）须回写官网并过对比度检查。
