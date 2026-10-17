# spikes/acp-client — ACP 子代理 spike(Task 4,PR-3)

> **原型代码,默认不合并**。不属于 workspace 成员、不接 CI;本目录的唯一产出是证据与报告:
> `docs/spikes/2026-10-acp-subagent-spike.md`

验证的问题:真 Claude Code 能否经 ACP(Agent Client Protocol)作为 Shannon 的子代理被编排——
spawn bridge → `initialize` → `session/new` → `session/prompt`(流式)→ `stopReason`,以及凭据隔离。

## 复现步骤

```bash
cd spikes/acp-client
npm install                    # 安装 ACP 桥接适配器 @agentclientprotocol/claude-agent-acp@0.93.0
cargo build
./target/debug/acp-client \
  --cwd "$PWD" \
  --prompt 'Reply with exactly one line of JSON: {"ok": true, "answer": <the answer to 7*6>} and nothing else.' \
  --log logs/my-run.jsonl \
  --record-env logs/my-child-env.txt \
  --poison                     # 在父进程注入伪造凭据,验证不会泄漏给子进程
```

预期:`stopReason=end_turn`,流式收到 `{"ok": true, "answer": 42}`;
`my-child-env.txt` 末行 `ISOLATION CHECK: PASS`(以 `/proc/<pid>/environ` 实测)。
`logs/` 被仓库根 `logs/` 规则忽略;若产生新的可评审取证,把裁剪后的副本放进 `evidence/`。

- 本机实测两次:run-1(首次,含 CLI 解压冷启动)与 run-2(热,墙钟 7.8s),均 `end_turn`。
- 已提交的取证文件:`evidence/e2e-run{,-2}-trimmed.jsonl`(超长行已截断,截断处有标记)、
  `evidence/child-env{,-2}.txt`。完整日志不入库(.gitignore)。

## 凭据隔离(实现方式)

`src/main.rs::scrubbed_env()`:子进程用 **env 清空 + 允许列表**(PATH/HOME/TERM 等 12 个)启动,
任何 `*KEY*/*TOKEN*/*SECRET*/*ANTHROPIC*/*SHANNON*/*OPENAI*` 形态的变量都不可能透传。
`--poison` 会先在父进程植入伪造凭据(`SHANNON_LLM_API_KEY`、`ANTHROPIC_API_KEY`、
`ANTHROPIC_AUTH_TOKEN`、`OPENAI_API_KEY`)作为对照。Claude Code 的鉴权来自它自己的
`~/.claude/settings.json`(`env.ANTHROPIC_AUTH_TOKEN`/`BASE_URL`)或 OS 钥匙串——provider 自持,
Shannon 凭据链零参与。

## 版本锚点(2026-10-17)

| 组件 | 版本 |
|---|---|
| `@agentclientprotocol/claude-agent-acp`(桥接适配器) | 0.93.0(npm,2026-10-16 发布;前身 `@zed-industries/claude-code-acp` 已弃用改名) |
| `@anthropic-ai/claude-agent-sdk`(适配器依赖) | 0.3.300 |
| ACP 协议版本(协商结果) | 1(v2 尚为实验,需 `CLAUDE_AGENT_ACP_EXPERIMENTAL_V2=1`) |
| `agent-client-protocol`(Zed 的 Rust crate) | 3.4.0(crates.io,2026-10-16 更新) |
| 本机 Claude Code CLI(独立存在,SDK 用自带 CLI) | 2.1.250 |
