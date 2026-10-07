# secret-guard 默认 `redact` 翻转 —— dogfood 判据与落地记录

- 日期：2026-10-07
- 决策：D1（PR #335 遗留清单第 1 项）——默认 `audit` → `redact` 翻转，**分两步**：先 dogfood，过线后随下一个 minor（v0.13.0）合并本 PR。
- 本 PR：`feat/secret-guard-redact-default`（**draft，dogfood 过线前不合**）。
- 关联：PR #335（折中方案已落地：audit 态警示纠偏 + 落盘无明文回归锁定）；本分支栈叠其上，#335 合并后把本 PR base 改回 `dev`。

## 一、dogfood 安排

- 环境：开发机 `~/.shannon/config.toml` 已写 `[secret_guard] mode = "redact"`（2026-10-07 起）。
- 观察信号（每次真实会话后 ≤1 分钟）：
  1. `grep -c "unresolved placeholder" ~/.shannon/sessions/*/events.jsonl` —— F3/F4（占位符流进执行面）信号；
  2. 转录/截图中出现裸 `SG1:` token —— 显示面还原缺口信号；
  3. 模型对代理 token 的异常行为（反复追问、试图"执行"占位符）。

## 二、过关判据（预注册，满足即合本 PR）

- ≥ 5 个真实会话在 redact 下完整跑过，或 7 天自然到期（先到为准）；
- 期间**零次**真实 F3/F4；
- 无显示面泄漏；
- 无可疑模型行为。

## 三、止损判据（出现即暂停合并）

- 任一真实工作流中占位符进入工具调用且造成实际失败 → 记 defect、修复后**重计观察期**。

## 四、回退预案

- 翻转后若出现系统性干扰：补丁版本把 `resolve_mode_with_default` 的默认值拨回 `Audit`（一行）+ 文案回退；用户已有配置不受影响。
- 已知且接受的局限：内建形状层保守（词边界 + 知名前缀 + 显式声明值），完整语料检测在 external `secret-guard-plugin`；漏检（false negative）不阻断本翻转。

## 五、本 PR 内容清单

1. `resolve_mode_with_default`：release 默认 `Audit` → `Redact`（含 doc）。
2. T5 锁存器改双模式：audit 命中 → "开 redact" 建议；redact 命中 → 信息性通知（含两个逃生口）。**文案归 core 所有**（`take_redaction_suggestion` 返回 `Option<String>`），TUI/CLI 宿主只渲染——顺带修掉宿主残留的旧错误断言（"written to the session log"）。
3. 文档：`secret_guard.rs` 三处 doc、README 两语言第 39 行。
4. CHANGELOG：Unreleased 顶部 Breaking 小节（含全部逃生口）。
5. 测试：默认值断言翻转；T5 三测重写（audit 建议 / redact 通知 / external 静默）。

## 六、状态

- [x] 分支创建（栈叠 #335）
- [x] 实现完成
- [ ] dogfood 判据满足（观察中，2026-10-07 起）
- [ ] 合并（先 retarget base → dev）
