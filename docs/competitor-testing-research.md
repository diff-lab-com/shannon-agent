# 竞品测试体系调研与 Shannon 测试改进建议

> 调研日期: 2026-05-31
> 覆盖竞品: DeepSeek-Reasonix, Claude Code, Codex CLI
> 跳过: OpenCode

---

## 1. 竞品测试体系概览

| 维度 | DeepSeek-Reasonix | Claude Code | Codex CLI | Shannon |
|------|-------------------|-------------|-----------|---------|
| 语言 | TypeScript | TypeScript (闭源) | Rust | Rust |
| 测试框架 | Vitest | 无传统测试 | cargo-nextest | cargo-nextest |
| 测试规模 | 2000-3000 | 未知 | 500+ | 9181 |
| HTTP Mock | fakeFetch | 无 (dogfooding) | Wiremock | mockito |
| Record/Replay | 无 | 无 | 无 | 有 (JSONL) |
| 快照测试 | 无 | 无 | insta | 无 |
| 变异测试 | Stryker | 无 | 无 | 无 |
| MCP Mock | FakeMcpTransport | 无 | 无 | 无 |

---

## 2. DeepSeek-Reasonix 测试体系

### 2.1 技术栈

- **框架**: Vitest (与 Vitest 深度集成)
- **语言**: TypeScript
- **测试规模**: 约 2000-3000 测试用例

### 2.2 核心 Mock 基础设施

#### fakeFetch — HTTP 请求 Mock

```typescript
// 典型用法: 拦截 LLM API 调用
vi.fn().mockResolvedValue({
  choices: [{ message: { content: "mocked response" } }]
})
```

- 替换全局 `fetch`，拦截所有 HTTP 请求
- 支持按 URL 模式匹配返回预设响应
- 支持流式响应模拟 (SSE)
- 轻量级，不需要启动真实 HTTP 服务器

#### FakeMcpTransport — MCP 协议 Mock

- 模拟 MCP (Model Context Protocol) 传输层
- 允许测试 MCP 工具发现、调用、通知
- 不需要启动真实 MCP 服务器进程
- 支持模拟工具 schema 返回和执行结果

### 2.3 测试分层

#### 单元测试 (最多)
- 纯函数测试 (token 计算、消息格式化)
- 工具执行逻辑 (文件操作、bash 命令构造)
- 状态管理 (对话历史、权限检查)
- 提示词构建 (system prompt 组装)

#### 集成测试
- API 适配器 (OpenAI/Anthropic 格式转换)
- 工具调用循环 (tool_call → execute → observe → respond)
- MCP 服务器生命周期
- 文件编辑冲突处理

#### 架构不变量测试 (特色)
- 验证系统架构约束不被破坏
- 例如: "所有工具必须声明 is_read_only"
- 例如: "消息流必须严格交替 user/assistant"
- 例如: "权限检查必须在工具执行前"

### 2.4 变异测试 (Stryker)

- 使用 Stryker 进行变异测试
- 自动修改源代码 (改运算符、删条件) 并检查测试是否捕获
- 目标: 变异存活率 < 5%
- 说明测试质量高，不是凑覆盖率

### 2.5 评价

**优点**:
- fakeFetch 比 mockito 更轻量 (不需要 TCP 端口)
- FakeMcpTransport 是 MCP 测试的最佳实践
- 架构不变量测试防止架构退化
- Stryker 变异测试保证测试质量

**不足**:
- 无 Record/Replay 机制，mock 数据全部手写
- 变异测试运行时间长 (全量 >30min)
- TypeScript 的类型安全不如 Rust 编译期保证

---

## 3. Claude Code 测试体系

### 3.1 特殊性

Claude Code 是 Anthropic 的闭源商业产品。无法直接查看源码和测试代码。
以下信息基于公开资料、社区报告和逆向分析。

### 3.2 测试哲学: AI-Native Dogfooding

Claude Code 的测试策略与传统的 "写测试用例" 完全不同:

- **AI 测试 AI**: 使用 Claude 自身来测试 Claude Code
- **内部 dogfooding**: Anthropic 员工日常使用 Claude Code 开发 Claude Code
- **高频发布**: 每天约 60-100 个内部版本，持续验证
- **A/B 测试**: 不同 prompt/模型版本在生产环境对比

### 3.3 Hook 系统作为测试基础设施

Claude Code 的 Hook 系统不仅是扩展机制，也是隐含的测试基础设施:

```json
{
  "hooks": {
    "PostToolUse": [{ "command": "test-validator.sh" }],
    "PostPrompt": [{ "command": "regression-check.sh" }]
  }
}
```

- `PostToolUse` hook 可在每次工具调用后执行验证
- `PostPrompt` hook 可在每个 prompt 周期后检查状态
- 等价于 "每次操作后的断言"
- 用户/团队可自定义测试逻辑

### 3.4 Zod Schema 验证

- 使用 Zod (TypeScript runtime schema validator)
- API 响应、工具输入/输出、配置文件全部有 Zod schema
- 等价于 Rust 的 serde 反序列化 + 手动验证
- 编译时类型 + 运行时 schema 双重保障

### 3.5 质量保证手段

| 手段 | 类型 | 说明 |
|------|------|------|
| Dogfooding | 手动 | 员工日常使用 |
| A/B testing | 统计 | 生产环境对比 |
| Hook 验证 | 自动 | 自定义 post-action 检查 |
| Zod schema | 自动 | 运行时类型验证 |
| 社区反馈 | 手动 | GitHub issues |

### 3.6 评价

**优点**:
- AI-native 测试理念领先
- 发布频率极高 (60-100次/天)
- Hook 系统让用户参与质量保证

**不足**:
- 闭源，无法学习具体实现
- 无公开的单元/集成测试
- 测试效果高度依赖使用密度
- 无法做回归测试的精准定位

---

## 4. Codex CLI 测试体系

### 4.1 技术栈

- **框架**: cargo-nextest (与 Shannon 相同)
- **语言**: Rust
- **HTTP Mock**: Wiremock-rs
- **快照测试**: insta

### 4.2 核心 Mock 基础设施

#### Wiremock — HTTP 服务 Mock

```rust
use wiremock::{Mock, MockServer, ResponseTemplate};
use wiremock::matchers::{method, path};

Mock::given(method("POST"))
    .and(path("/v1/chat/completions"))
    .respond_with(ResponseTemplate::new(200).set_body_string(mock_response))
    .mount(&mock_server)
    .await;
```

- 异步 HTTP mock 服务器
- 支持请求匹配 (method, path, header, body)
- 比 mockito-rs 更现代的 API
- 支持延迟响应模拟

#### StreamingSseServer — SSE 流 Mock

- 自定义的 SSE (Server-Sent Events) 测试服务器
- 模拟 LLM API 的流式响应
- 可控的 chunk 发送时序
- 测试流式解析和断线恢复

#### TestCodex — 测试 Harness

```rust
struct TestCodex {
    mock_server: MockServer,
    config: TestConfig,
    // 预配置的 LLM 响应
}
```

- 封装了 mock 服务器 + 配置 + 断言
- 简化测试编写
- 支持多轮对话模拟

### 4.3 快照测试 (insta)

```rust
#[test]
fn test_tool_call_format() {
    let result = format_tool_call(tool_input);
    insta::assert_snapshot!(result);
}
```

- 自动捕获输出作为快照
- 变更时生成 diff 供人工审核
- 用于: API 请求格式、提示词模板、错误消息
- 防止意外格式变化

### 4.4 测试结构

- 60+ 集成测试文件
- 每个 `src/` 模块对应 `tests/` 文件
- 大量 `#[cfg(test)] mod tests` 内联测试
- E2E 测试使用 `assert_cmd` crate

### 4.5 评价

**优点**:
- Wiremock 比 mockito-rs API 更友好
- StreamingSSEServer 直接测试流式解析
- insta 快照测试防止意外变化
- TestCodex harness 降低测试编写成本
- 技术栈与 Shannon 最接近，可直接借鉴

**不足**:
- 无 Record/Replay (Shannon 独有优势)
- 无变异测试
- 快照测试的 `.snapshot` 文件可能有 merge 冲突

---

## 5. Shannon 测试现状分析

### 5.1 当前测试分布

| Crate | 测试数 | 类型 |
|-------|--------|------|
| shannon-core | ~3370 | 单元 + 集成 |
| shannon-ui | ~1089 | 单元 + widget 测试 |
| shannon-tools | ~1111 | 单元 + 工具集成 |
| shannon-commands | ~335 | 命令测试 |
| shannon-agents | ~471 | agent 协调 |
| shannon-cli | ~191 | CLI E2E + record/replay |
| shannon-skills | ~171 | skill 执行 |
| shannon-mcp | ~373 | MCP 协议 |
| 其他 | ~270 | 类型、codegen 等 |
| **总计** | **~9181** | |

### 5.2 Shannon 独有优势

1. **Record/Replay**: 所有竞品都没有。录制真实 API 响应，CI 无需 API key 即可验证
2. **JSONL 会话格式**: 按 provider/model/session 命名，支持多 provider 录制
3. **Secret stripping**: 自动清理 fixture 中的 API key/token
4. **Per-session replay**: 自动发现并验证每个 session 文件

### 5.3 Shannon 相对不足

1. **无快照测试**: 没有 insta 类似的快照机制
2. **无变异测试**: 无 Stryker 类似工具验证测试质量
3. **无架构不变量测试**: 没有验证架构约束的专用测试
4. **mockito 限制**: 顺序依赖的 matcher 容易出错
5. **MCP 测试不足**: 缺少 FakeMcpTransport 类似的轻量 MCP mock

---

## 6. 改进建议

### 6.1 高优先级 (直接提升测试质量)

#### P1: 添加快照测试 (借鉴 Codex CLI)

使用 `insta` crate 对以下内容做快照:
- API 请求格式 (adapter 序列化输出)
- System prompt 模板
- 错误消息格式
- 工具 schema 定义

```rust
// 示例: 测试 system prompt 格式稳定性
#[test]
fn test_system_prompt_format() {
    let prompt = build_system_prompt(&config);
    insta::assert_snapshot!("system_prompt", prompt);
}
```

**收益**: 防止意外格式变化，快速发现 prompt/请求格式回退。

#### P2: 添加架构不变量测试 (借鉴 DeepSeek-Reasonix)

在 `shannon-core` 中添加不变量测试模块:

```rust
#[test]
fn all_tools_declare_readonly_flag() {
    // 验证每个 Tool 实现都正确声明了 is_read_only()
}

#[test]
fn message_flow_alternates_roles() {
    // 验证对话历史严格交替 user/assistant
}

#[test]
fn permission_check_before_tool_execution() {
    // 验证权限检查一定在工具执行之前
}
```

**收益**: 防止架构退化，这些是编译器无法检查的约束。

#### P3: 增强 MCP 测试 (借鉴 DeepSeek-Reasonix)

创建 `FakeMcpTransport` 类似的轻量 MCP mock:
- 不启动真实子进程
- 模拟 tool discovery、execution、notification
- 测试 MCP 生命周期管理

### 6.2 中优先级 (提升开发体验)

#### P4: 考虑 wiremock 替代 mockito

- wiremock-rs API 更现代，异步优先
- Matcher 不依赖顺序
- 但 mockito 已经广泛使用，迁移成本高
- **建议**: 新测试优先用 wiremock，旧测试暂不迁移

#### P5: StreamingSSE 测试 harness (借鉴 Codex CLI)

创建专用的 SSE 流测试工具:
- 可控的 chunk 发送时序
- 模拟断线/超时/格式错误
- 测试 `SseStream` 和 `MessageStream` 的边界情况

### 6.3 低优先级 (锦上添花)

#### P6: 变异测试实验

- 使用 `cargo-mutants` 对核心模块做变异测试
- 从 `shannon-core/src/api/` 和 `shannon-core/src/query/` 开始
- 评估测试质量，目标变异存活率 < 10%

#### P7: Hook 作为测试基础设施 (借鉴 Claude Code)

- 利用 Shannon 的 32 种 hook event
- 编写 hook 脚本作为回归测试
- 例如: PostToolUse hook 检查文件完整性

---

## 7. Record/Replay 测试评估

### 7.1 当前 record_task 测试清单

| # | 测试名 | 场景 | 覆盖能力 |
|---|--------|------|----------|
| 1 | bash_command | 执行简单命令 | bash 工具基本功能 |
| 2 | bash_verify | 执行命令并验证结果 | bash + 文件验证 |
| 3 | code_search | 代码搜索 | grep/search 工具 |
| 4 | create_file | 创建单个文件 | write_file 工具 |
| 5 | create_with_tests | 创建带测试的文件 | write_file + 内容生成 |
| 6 | edit_precise_match | 精确匹配编辑 | edit 工具基本功能 |
| 7 | error_recovery | 错误恢复 | 错误处理 + 重试 |
| 8 | glob_pattern | glob 文件搜索 | glob 工具 |
| 9 | json_schema_output | JSON Schema 结构化输出 | --schema 功能 |
| 10 | long_file_handling | 长文件处理 | 大文件读写 |
| 11 | multi_file_edit | 多文件编辑 | 跨文件 edit |
| 12 | multi_turn | 多轮对话 | 上下文保持 |
| 13 | read_and_edit | 读取后编辑 | read + edit 组合 |
| 14 | refactor_rename | 重构重命名 | 跨文件重命名 |
| 15 | search_read_edit | 搜索+读取+编辑 | 三步组合操作 |

### 7.2 覆盖评估

**已覆盖**:
- 单工具: bash, write, read, edit, grep, glob
- 组合: read+edit, search+read+edit, multi_file_edit
- 特殊: error_recovery, multi_turn, json_schema_output

**未覆盖的关键场景**:

| 缺失场景 | 重要性 | 说明 |
|----------|--------|------|
| MCP 工具调用 | 高 | 通过 MCP 调用外部工具 |
| 权限拒绝与确认 | 高 | PermissionManager 交互 |
| 上下文压缩 | 高 | 长对话后的 context compaction |
| 会话恢复 | 中 | 从 checkpoint 恢复会话 |
| 代码生成+执行 | 中 | 生成代码后执行验证 |
| 多 provider 切换 | 中 | 同一会话中切换 provider |
| 工具调用错误 | 中 | 工具返回错误后的处理 |
| 部分编辑冲突 | 中 | edit 工具的冲突检测 |
| 非 interactive 模式 | 中 | --prompt CI 模式完整流程 |
| Skill/Command 执行 | 中 | /help, /config 等命令 |

### 7.3 建议补充的 record_task 测试

#### 高优先级补充

```rust
// 1. MCP 工具调用
#[tokio::test]
#[ignore]
async fn record_task_mcp_tool_call() {
    // 配置 MCP server, 调用 MCP 工具, 验证结果
}

// 2. 权限交互
#[tokio::test]
#[ignore]
async fn record_task_permission_flow() {
    // 触发需要权限确认的操作 (如删除文件)
}

// 3. 上下文压缩
#[tokio::test]
#[ignore]
async fn record_task_context_compaction() {
    // 长对话 (10+ 轮), 触发自动压缩, 继续对话验证压缩效果
}
```

#### 中优先级补充

```rust
// 4. 非 interactive 模式
#[tokio::test]
#[ignore]
async fn record_task_prompt_mode() {
    // --prompt 模式, 验证 NDJSON 输出
}

// 5. 错误恢复 (工具级)
#[tokio::test]
#[ignore]
async fn record_task_tool_error_recovery() {
    // 工具返回错误, LLM 尝试替代方案
}

// 6. 会话恢复
#[tokio::test]
#[ignore]
async fn record_task_session_resume() {
    // 创建会话, 退出, 恢复, 继续对话
}
```

### 7.4 Record/Replay 改进建议

1. **Replay 时验证工具调用序列**: 不仅验证 HTTP mock 匹配，还验证工具调用顺序和参数
2. **多 provider 录制**: 对同一 session 分别录制 anthropic/openai/minimax，对比行为差异
3. **Replay 回归检测**: 每次录制新 fixture 时，运行旧 fixture 的 replay 确认无回退
4. **Fixture 最小化**: 定期清理过时的 fixture，保持 fixture 集与代码同步

---

## 8. 总结

Shannon 的测试体系 (9181 测试) 在数量上已超过竞品。核心差距不在测试数量，而在:

1. **测试质量验证**: 缺少变异测试，无法确认测试真正捕获了 bug
2. **架构保障**: 缺少不变量测试，架构约束依赖人工 review
3. **快照稳定性**: 缺少快照测试，格式变化需要手动发现
4. **Record/Replay 深度**: 有录制机制但 replay 验证不够深入 (只验证加载，不验证行为)

Record/Replay 是 Shannon 独有优势，建议:
- 补充 6 个缺失场景的录制测试 (MCP、权限、压缩、prompt模式、工具错误、会话恢复)
- 增强 replay 验证深度 (工具调用序列、参数匹配)
- 建立多 provider 录制对比机制
