# Pier × Shannon adapter 前置研读笔记（P0-4，2026-09-18）

对象：`datacurve-pier==0.3.1`（uv tool 安装，源码位于
`~/.local/share/uv/tools/datacurve-pier/lib/python3.13/site-packages/pier/`）。
语料：`~/eval-corpora/deep-swe` @ main `0b9fabb`（**语料 anchor：无 v1.1 tag，仅 v1.0.0；
main HEAD 即榜上 v1.1 配置，超时已调 10800s**）。任务数核实：`tasks/` 下 **113 个任务目录**
与 `PROVENANCE.md` 113 行清单完全一致（多出的 4 项是 `dataset.toml`/`manifest*.json`/`README.md`
文件，非任务）。

## 一、四问答案

### Q-a 自定义 agent 注册路径：`--agent-import-path module:Class`，无需注册

- `pier/cli/jobs.py`：`pier run` 提供 `--agent-import-path`（"Import path for custom agent"）、
  `--ak/--agent-kwarg key=value`（透传 agent `__init__` kwargs）、`--ae/--agent-env KEY=VALUE`。
- `pier/agents/factory.py:71` `create_agent_from_import_path`：`importlib.import_module` +
  `getattr`，类只需在 `PYTHONPATH` 可导入。
- 基类 `pier/agents/installed/base.py` `BaseInstalledAgent`（与 harbor 同源 fork，shannon 的
  `shannon_harbor_agent.py` 用到的 `exec_as_root/exec_as_agent/NonZeroAgentExitCodeError/
  CliFlag/with_prompt_template` 全部同名存在，平移成本低）。
- 必须实现：`name()`（staticmethod）、`install_spec()`、`populate_context_post_run(context)`、
  `run(instruction, environment, context)`。`setup()` 有默认实现（建 `/installed-agent` →
  `install()` → version 探测），**覆写 `install()` 即可**（默认实现遍历 install_spec 步骤）。
- **坑**：`AgentInstallSpec` 的 validator 强制 `steps` 非空——`install_spec()` 需返回一个占位
  步骤（如 `mkdir -p /usr/local/bin`），真实安装逻辑走覆写的 `install()`（docker 环境
  `environment.agent_install_spec` 为 None，不会走预构建路径）。
- `SUPPORTS_ATIF` 默认 False：轨迹留 shannon 自有 NDJSON，pier 只存不渲染——可接受。

### Q-b commit 契约：`git diff <base> HEAD`，**adapter 不代 commit**

- `task.toml`：`[[verifier.collect]]` = `cd /app && git diff --binary <base_commit> HEAD >
  /logs/artifacts/model.patch`；`artifacts = ["/logs/artifacts/model.patch"]`。只有**已提交**
  内容进入判分。
- `instruction.md` 末尾明文："IMPORTANT: Please work on this in a new branch from main and
  **commit everything when you are done**." ——指令对所有 agent 一视同仁，mini-swe-agent 亦无
  adapter 兜底。
- **裁定：adapter 不做任何自动 commit**。shannon 不 commit = 真实的指令遵循/收工信号问题，
  属 P3 分析对象，不是 harness 该补的洞。（与 harbor adapter 不代改任务仓库同一原则。）
- 其他判分参数：`[agent] timeout_sec=10800`（3h/题）；`[verifier] separate 容器、1800s、
  no-network`；预构建镜像 `public.ecr.aws/d3j8x8q7/swe-bench-202605:<ext_id>-v1.1`（docker
  pull 免本地构建）。

### Q-c 网络白名单：覆写 `network_allowlist()`，放行 `open.bigmodel.cn`

- `pier/agents/base.py`：`network_allowlist() -> NetworkAllowlist`，默认空实现；
  `pier/models/agent/network.py`：domains 为精确域名或前导点后缀（如 `.anthropic.com`）。
- 任务侧 `[agent] network_mode = "no-network"`：pier 用 per-agent 白名单放行必要出网
  （这是 pier 相对 harbor 的核心差异，README 明示）。
- zhipu-coding-plan 端点 = `open.bigmodel.cn/api/coding/paas/v4` → 白名单
  `["open.bigmodel.cn"]`。API key→JWT 签名（`generate_zhipu_jwt`）是本地计算，无额外 host。
- 参考实现 mini_swe_agent.py:703 从 env/config 提取 base_url 域名；shannon 直接静态返回。
- **watch item**：shannon 启动期的 models.dev 动态目录合并若在容器内发起请求，冒烟时验证
  是否阻塞/可降级（`glm-5.3-flash` 已入静态 catalog（C4），动态目录非必需）。若确实必需，
  再议是否加白（mini-swe 也放行其 litellm cost-map 域名，有先例）。

### Q-d 续跑：`pier job resume` 原生支持

- `pier job --help`：`resume <job-dir>` 从 job 目录恢复；`pier run` 另有
  `-n/--n-concurrent`、`-k/--n-attempts`、`-r/--max-retries` + `--retry-include/--retry-exclude`
  （异常类型过滤）、`--timeout-multiplier` 系列。
- 任务过滤：`--task-name`（glob）、`--exclude-task-name`、`--n-tasks` + `--sample-seed`
  （确定性抽样，官方口径）。
- 结论：**不需要**自写 run-batch 式 job 包装；预算闸/发车门禁仍用仓库既有
  preflight-network.sh + 手工分波（`--task-name` glob 分片）实现。

## 二、adapter 设计契约（P1 实现蓝本）

```
scripts/eval/pier-adapter/shannon_pier_agent.py
class Shannon(pier.agents.installed.base.BaseInstalledAgent)
```

| 成员 | 实现 |
|---|---|
| `name()` | `"shannon"` |
| `install_spec()` | 占位：`InstallStep(run="mkdir -p /usr/local/bin", user="root")` |
| `install(env)` | 覆写：libc 探测（`/lib/ld-musl-x86_64.so.1`）→ musl/glibc 本地二进制 → `environment.upload_file` → `/usr/local/bin/shannon` → `shannon --version`（照抄 harbor adapter；`SHANNON_HARBOR_BIN` 环境变量指认 dev 构建，禁 release fallback） |
| `network_allowlist()` | `NetworkAllowlist(domains=["open.bigmodel.cn"])` |
| `run()` | env：`build_process_env` + `SHANNON_API_KEY` + host 侧 `SHANNON_*` 透传 + `SHANNON_STREAM_IDLE_SECS` 默认 `420`；prompt upload 文件 + 附着式 `--prompt="$(cat /tmp/shannon_prompt.txt)"`（L3）；命令尾部 `> /logs/agent/shannon.ndjson 2> /logs/agent/shannon.stderr`；rc=4 → 60s 后重试一次 |
| `populate_context_post_run()` | 解析 NDJSON `done` 事件：`tokens_in/out` → `n_input/n_output_tokens`；`tool_call` 计数 → `n_agent_steps`；`metadata` 带 exit_code/turns |
| 模型约定 | `-m zhipu-coding-plan/glm-5.3-flash`（provider/model 拆分同 harbor adapter） |

调用模板（P1 冒烟即用）：

```bash
PYTHONPATH=scripts/eval/pier-adapter \
SHANNON_HARBOR_BIN=/path/to/shannon \
pier run -p ~/eval-corpora/deep-swe/tasks/<task-id> \
  --agent-import-path shannon_pier_agent:Shannon \
  -m zhipu-coding-plan/glm-5.3-flash \
  --ae SHANNON_API_KEY=$SHANNON_API_KEY \
  -n 1
```

## 三、公平性裁定（反过拟合，与 backlog §五同源）

1. **不代 commit**：commit 指令在题面，人人可见；shannon 的执行情况是产品信号。
2. **不碰 verifier/镜像**：`environment/`、`tests/`、`solution/` 只读。
3. **白名单最小化**：仅放行 LLM 端点（+冒烟后证明确需的目录源）；不放行通用出网。
4. **prompt 原样透传**：`instruction.md` 逐字节进 `--prompt`（附着式传参保证字节级一致，
   L3 验证协议）；不用 prompt_template 注入任何 DeepSWE 特化措辞。
5. 与官方榜可比性：官方 = pier + mini-swe-agent + glm-5.3-flash [max]；我们 = pier +
   shannon + glm-5.3-flash（默认 thinking=max 同档）；对比差值即 shannon scaffolding 损益。

## 四、环境噪声备忘（T1 教训呼应）

- 本机对 `raw.githubusercontent.com` egress 劣化（pier 启动时 LiteLLM cost-map 拉取 3 连
  超时后回退本地，非致命）——**每波发车前跑 preflight-network.sh 四点探针**（GLM API、
  docker hub、ECR public、ECR 镜像拉取）。
- 预构建镜像在 `public.ecr.aws`：P1 冒烟前手动 `docker pull` 一枚验证可达性（ECR 匿名
  限流是 T1 已知 infra 噪声源）。实测：镜像 ~2.6GB/题（含共享基础层），ECR 匿名拉取正常。

## 五、冒烟 RCA 记录（smoke-1/2，2026-09-18）

1. **smoke-1 exit 127（shannon: command not found）**：pier 把 `install_spec()` 步骤按指纹
   内联进派生镜像（`agent-build-context/Dockerfile` FROM 任务镜像 + RUN 步骤，指纹
   `d7b2941ec377796c`），随后 `environment.agent_install_spec` 非空 → 基类 `setup()` 判定
   「已预装」→ 跳过 `install()` → 二进制从未进容器。**修复**：adapter 覆写 `setup()`
   无条件执行真实安装（upload + install + version 探测）。
2. **prompt 字节级核验（L3）**：start 事件 prompt 与 `instruction.md` 逐字节一致，仅
   **末尾换行被 pier 剥掉**（1845 vs 1846 字符）——harness 层行为，对所有 agent 一致，
   不做补偿。
3. **容器内 LLM 出网**：`network_allowlist()=["open.bigmodel.cn"]` 生效，no-network 任务
   中 GLM 调用正常（pier 经 `pier-egress-proxy` sidecar 实现白名单代理）；shannon 的
   bwrap 沙箱在容器内缺失时按设计降级 NoSandbox（TB 已验证的同一路径）。
4. **smoke-2（1h0m，rc=3 死于第 8 轮）**：引擎默认单请求**总**超时 300s
   （`unified_config.rs:607` `unwrap_or(300)` → `client.rs:903` request.timeout）撞上
   GLM 思考型长调用（已录得 312s+ 静默）→ "Request timed out" → rc=3 → 42 次工具调用的
   工作未 commit → `git diff base..HEAD` 空 patch → 判分 F2P 0/88、P2P 275/275。
   **adapter 修复**：`SHANNON_TIMEOUT` 默认 1800s（>420s 看门狗，任务级 3h 不受影响）。
   **P3 产品候选（附证据）**：看门狗默认 420s 与总超时默认 300s 自相矛盾——对 GLM 类
   长思考模型，默认配置会把健康流当超时杀死；「总超时应让位于无字节进度判据」是通用
   改进项，非 DeepSWE 特化。
5. 冒烟早期实测：2.5 分钟推进到 turn 3 / ~30k tokens；smoke-2 全程 1h 推进 8 轮 /
   ~54k tokens / 42 工具调用——GLM 单轮 3-8 分钟（思考延迟主导），单题期望 1-2.5h，
   3 并发全量约 2-4 天，与方案预估一致。另观察到 stderr turn 计数 7→5 回退（疑压缩后
   显示口径），列入 P3 观测性核对。
