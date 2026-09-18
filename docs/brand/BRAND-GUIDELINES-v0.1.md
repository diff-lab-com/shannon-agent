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
