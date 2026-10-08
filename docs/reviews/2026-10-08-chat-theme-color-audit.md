# Chat 区域 × 12 主题 颜色适配审查(2026-10-08)

**触发**:2026-10-08 桌面审查第 1 轮问题 4(「AI 聊天区域不同 theme 下各组件颜色适配审查」)。
**范围**:`desktop/ui/src/pages/chat/`、`components/chat/`、`components/terminal/`、`components/diff/`;主题源 `desktop/ui/scripts/theme-source.json`(6 dark:tokyo-night/catppuccin/nord/solarized/dracula/gruvbox;6 light:material/tokyo-night-light/ember/slate/solarized-light/gruvbox-light)。
**方法**:源码扫描(硬编码色值 / `dark:` 变体 / scrim 用法)+ 玻璃与极光线 CSS 审读 + xterm 主题映射审读。

## 结论(TL;DR)

**chat 组件层的 token 纪律非常好**——12 个主题下无 `dark:` 变体依赖(0 处),无组件级硬编码主题色;颜色全部走 `--color-*` 语义 token,随 `[data-theme]` 块整体切换。发现的问题集中在**三处边缘**:终端 ANSI 调色板、scrim 策略不统一、以及一个历史遗留的用法漂移。无 P0。

## 发现

### A · P1 · 终端 ANSI-16 是「两套固定调色板」,不跟随主题身份

`components/terminal/xtermTheme.ts`:background/foreground/cursor/selection 读 live token(✅ 跟主题走),但 **ANSI 16 色**只有 `ANSI_LIGHT`/`ANSI_DARK` 两套(暗色套就是 tokyo-night 的 ANSI)。于是 **6 个暗色主题共用同一套 tokyo-night ANSI**:gruvbox 主题下终端里 `ls --color`/git diff 的蓝紫绿全是 tokyo-night 味,和周边的暖棕 gruvbox UI 明显「两张皮」;dracula/nord/catppuccin 同理。

**改进方案**:在 `theme-source.json` 每主题加可选 `ansi` 块(16 色 + foreground),`generate-themes.mjs` 同步生成 `src/theme/generated/xterm-palettes.ts`;`xtermTheme()` 按主题 id 取专属调色板,取不到时回退现有两套 floor。工作量 S(生成器 + 6 组色值),一次性把终端从「跟随明暗」升级为「跟随主题身份」。

### B · P2 · scrim 遮罩两套哲学并存

- 图片灯箱(`MessageBubble.tsx:244`):`!bg-black/70` 固定黑,任意主题下不变(图片阅读惯例,可接受);
- 会话切换遮罩(`MessageArea.tsx`):`bg-surface-container-lowest/60 + backdrop-blur`(token 化)。

同一应用两种 scrim 语义。建议定一个 `--scrim` token(暗主题=近黑、亮主题=深灰蓝),灯箱与切换遮罩都走它;至少在 `styles/README.md` 把「scrim 何时用黑、何时用 token」写成规则,防止第三个 scrim 出现时再漂移。

### C · P2 · `text-outline` 徽标在亮主题的对比度存疑

TerminalPanel 关闭 tab 的「已退出」后缀用 `text-outline`(最淡的前景色档);全局同类用法 **14 处/8 文件**(empty-state/KeyboardShortcutsHelp/Layout/SidebarSessions/ContextBreakdownCard/DataSourcesQuery/TerminalPanel/AdvancedSettings)。outline token 本意是描边不是正文;6 个亮主题下 outline 亮度接近 surface,弱视力用户可能读不到。建议改 `text-on-surface-variant`,或给「已退出」补一个 `opacity` 语义档。

### D · 无问题(审查过、可放心的)

- **极光发丝线**(`.aurora-line`):固定品牌渐变(violet→cyan),亮主题有专门加强档(`index.css:392`)——设计稿裁定「全站唯一装饰、跨主题恒定」,实现与设计一致;
- **玻璃三档**(glass-surface/overlay/panel/card)在 `prefers-reduced-transparency`/`prefers-reduced-motion` 下都有 solid 回退,亮主题无玻璃白屏风险;
- **diff/Markdown/InlineDiffCard/QueueChips/ChatStatusBar**:未发现硬编码色值,全部走 token;
- **chat 组件 0 处 `dark:`**:说明没有绕过 token 的主题特判,这是好事(有 `dark:` 才意味着双轨维护)。

## 改进清单(按序)

1. 终端 ANSI 主题化(发现 A,S);
2. scrim token 统一 + 规则入 `styles/README.md`(发现 B,S);
3. `text-outline` 正文级用法复查与替换(发现 C,14 处/8 文件,S);
4. 12 主题 × chat 页截图回归:改稿后用 `assets/shot.mjs` 的路子对 6+6 主题各截一张 chat 页(欢迎态+有消息态),人工扫一眼——目前没有这层回归,发现 A/C 这类问题只能靠肉眼撞见(M)。

## 遗留相关项(非本轮范围)

- R3-V-06:usage 页图表柱体近黑、轴刻度混用数制——用量页,不属 chat 区域,已在 R3 报告挂账;
- R3-V-02:执行模式四处四套称呼(严格·平衡·宽松 vs Strict/Balanced/Permissive vs ask/auto-edit/full-auto)——词汇层问题,颜色审查中发现顶栏 pill 与 composer chip 用词已统一走 `execMode.tier.*` i18n,设置页三档卡标题仍是英文(R2-P2-14 原账)。
