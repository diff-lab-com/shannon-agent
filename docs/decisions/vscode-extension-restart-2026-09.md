# 决策记录：VS Code 扩展（I-6）重启评估

**日期**: 2026-09-14 ｜ **状态**: 建议（Decision: Defer → 条件重启）
**背景**: 竞研 I-6 指出 Claude Code / Codex / ZCode 均有 IDE 扩展而 Shannon 缺位； abandoned 的 VS Code 扩展已于 `d0d655e2` 移除（CI job 一并删除，档案见 `legacy-archives/`）。

## 1. 事实

- 旧扩展停滞于 Wave 3 的 7/8（improvement-plan-2026-08 P2-8），2026-09-05 评审决议放弃（`26613904`）。
- 竞品格局（2026-09 竞研）：Codex 桌面端**与 VS Code 扩展共享 App Server 代码**；ZCode 无 CLI 但桌面为唯一形态；Claude Code 以 CLI + 桌面 + 扩展三线并行。
- Shannon 现有对位资产：`shannon serve` loopback API（`ws://127.0.0.1:33420/api/ws`）、CLI、桌面端工作区（可拖拽面板/终端/diff）。
- IDE 扩展的用户价值在于「编辑器内上下文」：选区/打开文件/diff 内联——这些是桌面端无法触达的。

## 2. 选项评估

| 选项 | 成本 | 收益 | 判定 |
|---|---|---|---|
| A. 原样重启旧扩展 | 低 | 旧代码基于已废弃形态，欠 1/8 未完成 | ❌ 沉没成本，不重启 |
| B. 全新最小扩展（webview 嵌 `shannon serve` 会话列表 + 选区上下文发送） | 中（1 人月） | 编辑器内可达性；复用 loopback WS API，无新后端 | ⭕ **条件重启触发器见 §3** |
| C. 不做，桌面端加「IDE 感知」命令（`shannon open --editor`） | 低 | 仅部分场景 | 保留为 B 的降级路径 |
| D. 永久放弃 | 零 | 放弃 IDE 入口赛道 | ❌ 竞品三面压进，放弃过早 |

## 3. 决策

**Defer（不立即投入），设定重启触发器**——当以下任一信号出现即启动选项 B：

1. 桌面端 Wave 1–3 遗留清偿完毕（当前已基本达成 ✅）；
2. 用户调研/issue 中「IDE 内使用」请求 ≥ 10 例；
3. `shannon serve` 的 WS API 稳定性标记 stable（当前随桌面迭代中）。

**重启时的技术路线（选项 B 要点）**：

- VS Code webview 直接加载 `shannon serve` 的会话视图（复用桌面 React 组件库，Base UI + token 体系与桌面一致，组件库选型见 [COMPONENT-LIBRARY-RESEARCH.md](../design/ui-audit-2026-09/COMPONENT-LIBRARY-RESEARCH.md)）；
- 编辑器集成点：活动选区 → composer 预填、活动文件路径 → working_dir 建议、diff 视图 → `SessionDiffPanel` 复用；
- **不复制** App Server：与 Codex 不同，Shannon 的引擎/IPC 面已在 `shannon serve`，扩展保持薄壳。

## 4. 影响与跟进

- 本决策关闭 I-6 的「未决策」状态；季度竞研复查时复核触发器。
- 若触发，先出 spike：webview 加载 loopback 页面 + 选区桥接（约 3 天）。
