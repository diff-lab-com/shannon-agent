# Dangerous 安装确认门 · 设计方案（待审核，2026-10-10）

**状态**：草案，等你拍板 D-A/D-B/D-C/D-D 四个决策项后实施（M，一个 worktree）。
**背景**：批 2 实查推翻了"ConfirmationLevel 管线现成"的调研——`requires_confirmation()` 无任何调用方、`ConfirmationLevel` 无 Dangerous 变体、安装命令（`install_mcp_stdio` / `install_mcp_mcpb` / `install_skill_from_repo` / `install_native_skill` / `install_agent_from_repo`）一律无条件放行；`InjectionRisk::Dangerous` 的文档契约写明"Block install unless the user explicitly overrides"（security.rs:33）但仅止于 advisory 扫描。当前扩展安全 = 诚实徽章说"我们不拦"。

## 现状盘点（代码事实）

| 组件 | 现状 |
|---|---|
| `InjectionRisk`（security.rs） | Clean / Suspicious / Dangerous；~22 静态模式，README+描述扫描；**仅暴露为 `scan_prompt_injection*` 独立命令** |
| `ConfirmationLevel`（types.rs:238） | None / Review / TypeToConfirm——**语义现成**（注释明写 TypeToConfirm = "Must type entry name to confirm"）但零消费方 |
| `AddonInstaller::requires_confirmation()` | trait 方法 + 单测，**安装链无人调用** |
| 安装命令 ×5 | 无确认参数，收到即装 |
| UI（批 2 后） | 徽章诚实化（"安装时扫描 · 发布者自声明"），安装按钮无门槛 |

## 设计

### 1. 安装时重扫（D-B，建议采纳）

安装命令**内部**对即将安装的内容（README/描述/manifest）重跑同一 advisory 扫描，**不信任 UI 侧早前的扫描结果**——消除"扫描后、安装前内容被换"的窗口；UI 侧扫描保留为预览用途。

### 2. 风险 → 确认级别映射（让 ConfirmationLevel 第一次有消费方）

```
Clean      → ConfirmationLevel::None           （静默安装）
Suspicious → ConfirmationLevel::Review         （现 UI 警告样式，单击可装——D-C 默认不变）
Dangerous  → ConfirmationLevel::TypeToConfirm  （见 3）
```

`requires_confirmation()` 的实现改为 `max(来源信任级别基线, 扫描派生级别)`。

### 3. Dangerous 门（D-A：输入确认词，建议采纳）

- 命令签名（additive）：五个安装命令各加末位可选参数
  `confirmation: Option<InstallConfirmation>`，
  `InstallConfirmation { acknowledged_risk: InjectionRisk, typed_name: String }`。
- **后端拒绝规则**：扫描判 Dangerous 且 `confirmation` 缺失 / `acknowledged_risk != Dangerous` / `typed_name != 条目名`（后端精确比对，trim 后大小写敏感）→ 拒绝并返回**结构化错误载荷**（非裸字符串）：
  `confirmation_required { risk, matches[], match_count, required: "type_to_confirm", name }`——UI 据此渲染"为什么拦你"。
- 通过后照常安装；审计行记录 "dangerous install confirmed (typed name)"。

### 4. UI

- 扫描预览 Dangerous 的条目：安装按钮替换为确认抽屉——逐条列出 matches（pattern/category/substring），用户**输入条目名**后按钮才可用；输入值进 `typed_name`。
- Review 级：保留现有警告样式与单击流（不新增门槛）。
- 批 2 徽章文案升级：从"安装时扫描 · 发布者自声明"追加一句——Dangerous 判定需输入名称确认后才会安装（诚实告知新门槛）。
- mock/demo：demo 目录种一个 Dangerous 条目走完整流程。

### 5. 测试与验收

- Rust：无确认/错名/错 risk 拒绝；Suspicious 带警告可装；Clean 静默；安装时重扫生效（构造扫描后内容变化用例如可行）。
- UI vitest：抽屉门槛、输入名流、Review 不受影响。
- e2e：mock 种 Dangerous 条目 → 被拦 → 输入名 → 装上。
- CHANGELOG + 徽章文案更新。

### 6. 范围（D-D，建议 v1 就此打住）

v1 只门上述五个安装命令；OAuth 授权流（`install_mcp_oauth_*`）不涉及仓库内容拉取，不进 v1。真·出站扫描与密码学签名仍是独立 L 级项，本方案不碰。

## 决策项

| 项 | 建议 | 备选 |
|---|---|---|
| D-A 确认手势 | 输入条目名（TypeToConfirm，枚举语义现成） | 简单二次开关确认（弱，可被惯性点掉） |
| D-B 扫描时点 | 安装命令内部重扫（关掉 TOCTOU 窗口） | 信任 UI 预扫结果（实现更省，留窗口） |
| D-C Suspicious | 维持非阻断警告（现行为） | 升级为需 Review 抽屉确认 |
| D-D 范围 | v1 五个安装命令，OAuth 排除 | 全部安装面 |

确认 D-A/D-B/D-C/D-D 后即开工；与批 3 一起发 v0.14.0。
