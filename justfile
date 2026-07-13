# Shannon 测试命令速查
#
# 日常开发:   just dev          (check + lint + test, 提交前跑一次)
# 云端 CI:    just ci           (全量测试 + doctests, 带 retry/timeout)
# 回归/调优:  just bench        (微基准测试, 与基线比较)
# 性能回归:   just perf         (性能阈值测试)
# 场景测试:   just scenarios    (YAML 声明式场景测试)
# 录制:       just record       (默认 anthropic)
#             just record-deepseek / record-openai / record-minimax ...
# 回放:       just replay       (回放所有已录制的 fixture)
# 发布:       just build        (编译 release binary)
#
# 安装: cargo install just cargo-nextest

# 录制时使用的模型名 (默认由各 recipe 自行设定)
shannon_model := env_var_or_default("SHANNON_MODEL", "")

# 日常开发：提交前跑一次 (check + lint + test)
dev:
    cargo check --workspace
    cargo clippy --workspace
    cargo nextest run --workspace --config-file .config/nextest.toml

# 全量测试 (本地快速验证)
test:
    cargo nextest run --workspace --config-file .config/nextest.toml

# 完整测试 (nextest + doctests)
test-all: test
    cargo test --workspace --doc

# CI 完整流程 (nextest CI profile + doctests + lint)
ci:
    cargo nextest run --workspace --config-file .config/nextest.toml --profile ci
    cargo test --workspace --doc
    cargo clippy --workspace -- -D warnings

# 快速类型检查
check:
    cargo check --workspace

# Lint (CI 严格模式: warning 视为 error)
lint:
    cargo clippy --workspace -- -D warnings

# 性能阈值测试 (检测明显性能退化)
perf:
    cargo nextest run --workspace --config-file .config/nextest.toml -E 'test(compaction_100_turns) + test(session_load) + test(tool_chain) + test(streaming_parse) + test(snapshot_render) + test(token_estimation) + test(cache_hit_rate) + test(cache_accumulation) + test(single_turn) + test(five_turn) + test(sse_round_trip) + test(message_serialization)'

# YAML 场景测试
scenarios:
    cargo nextest run --workspace --config-file .config/nextest.toml -E 'test(scenario_)'

# ── 录制/回放 ──────────────────────────────────────────────────────────────
#
# 录制用真实 API 请求生成 fixture 文件，回放用 fixture 驱动 mockito mock。
#
# fixture 命名: {provider}_{model}_{session_name}.jsonl
# 例如: anthropic_unknown_create_file.jsonl
#       deepseek_deepseek-chat_create_file.jsonl
#
# 录制示例:
#   SHANNON_API_KEY=sk-ant-... just record
#   SHANNON_API_KEY=sk-... just record-deepseek
#   SHANNON_API_KEY=sk-... just record-openai gpt-4o-mini
#   SHANNON_API_KEY=sk-... SHANNON_MODEL=claude-sonnet-4 just record
#
# 自定义 provider (需手动设 base URL):
#   SHANNON_API_KEY=... SHANNON_BASE_URL=https://api.myprovider.com/v1 \
#     SHANNON_RECORD_PROVIDER=myprovider just record
#
# 回放所有已录制的 fixture:
#   just replay

# 录制: Anthropic (默认)
record: (_build) (_check-api-key)
    @echo "Recording with provider=anthropic, model={{ if shannon_model != "" { shannon_model } else { "unknown" } }}..."
    SHANNON_RECORD_DIR=tests/fixtures/real_tasks \
    SHANNON_RECORD_PROVIDER=anthropic \
    SHANNON_MODEL={{ if shannon_model != "" { shannon_model } else { "unknown" } }} \
    cargo nextest run --test live_tests -p shannon-cli --config-file .config/nextest.toml \
        --run-ignored ignored-only --test-threads=1 --no-fail-fast -E 'test(record_task_)'

# 录制: DeepSeek
record-deepseek: (_build) (_check-api-key)
    @echo "Recording with provider=deepseek, model={{ if shannon_model != "" { shannon_model } else { "deepseek-chat" } }}..."
    SHANNON_RECORD_DIR=tests/fixtures/real_tasks \
    SHANNON_RECORD_PROVIDER=deepseek \
    SHANNON_MODEL={{ if shannon_model != "" { shannon_model } else { "deepseek-chat" } }} \
    cargo nextest run --test live_tests -p shannon-cli --config-file .config/nextest.toml \
        --run-ignored ignored-only --test-threads=1 --no-fail-fast -E 'test(record_task_)'

# 录制: OpenAI (可选参数: model, 默认 gpt-4o)
record-openai model="gpt-4o": (_build) (_check-api-key)
    @echo "Recording with provider=openai, model={{ model }}..."
    SHANNON_RECORD_DIR=tests/fixtures/real_tasks \
    SHANNON_RECORD_PROVIDER=openai \
    SHANNON_MODEL={{ model }} \
    cargo nextest run --test live_tests -p shannon-cli --config-file .config/nextest.toml \
        --run-ignored ignored-only --test-threads=1 --no-fail-fast -E 'test(record_task_)'

# 录制: MiniMax
record-minimax: (_build) (_check-api-key)
    @echo "Recording with provider=minimax, model={{ if shannon_model != "" { shannon_model } else { "MiniMax-Text-01" } }}..."
    SHANNON_RECORD_DIR=tests/fixtures/real_tasks \
    SHANNON_RECORD_PROVIDER=minimax \
    SHANNON_MODEL={{ if shannon_model != "" { shannon_model } else { "MiniMax-Text-01" } }} \
    cargo nextest run --test live_tests -p shannon-cli --config-file .config/nextest.toml \
        --run-ignored ignored-only --test-threads=1 --no-fail-fast -E 'test(record_task_)'

# 录制: Moonshot/Kimi
record-moonshot: (_build) (_check-api-key)
    @echo "Recording with provider=moonshot, model={{ if shannon_model != "" { shannon_model } else { "moonshot-v1-8k" } }}..."
    SHANNON_RECORD_DIR=tests/fixtures/real_tasks \
    SHANNON_RECORD_PROVIDER=moonshot \
    SHANNON_MODEL={{ if shannon_model != "" { shannon_model } else { "moonshot-v1-8k" } }} \
    cargo nextest run --test live_tests -p shannon-cli --config-file .config/nextest.toml \
        --run-ignored ignored-only --test-threads=1 --no-fail-fast -E 'test(record_task_)'

# 录制: Zhipu/GLM (标准 OpenAI 兼容 API)
record-zhipu: (_build) (_check-api-key)
    @echo "Recording with provider=zhipu, model={{ if shannon_model != "" { shannon_model } else { "glm-4-flash" } }}..."
    SHANNON_RECORD_DIR=tests/fixtures/real_tasks \
    SHANNON_RECORD_PROVIDER=zhipu \
    SHANNON_MODEL={{ if shannon_model != "" { shannon_model } else { "glm-4-flash" } }} \
    cargo nextest run --test live_tests -p shannon-cli --config-file .config/nextest.toml \
        --run-ignored ignored-only --test-threads=1 --no-fail-fast -E 'test(record_task_)'

# 录制: Zhipu/GLM Coding Plan (Anthropic 兼容 API)
record-zhipu-coding: (_build) (_check-api-key)
    @echo "Recording with provider=zhipu-coding, model={{ if shannon_model != "" { shannon_model } else { "glm-5.1" } }}..."
    SHANNON_RECORD_DIR=tests/fixtures/real_tasks \
    SHANNON_RECORD_PROVIDER=zhipu-coding \
    SHANNON_MODEL={{ if shannon_model != "" { shannon_model } else { "glm-5.1" } }} \
    cargo nextest run --test live_tests -p shannon-cli --config-file .config/nextest.toml \
        --run-ignored ignored-only --test-threads=1 --no-fail-fast -E 'test(record_task_)'

# 录制: 任意 provider + model
# 用法: just record-with <provider> <model>
# 示例: just record-with zhipu glm-5.1
#       just record-with dashscope qwen-plus
#       just record-with myprovider mymodel
record-with provider model: (_build) (_check-api-key)
    @echo "Recording with provider={{ provider }}, model={{ model }}..."
    SHANNON_RECORD_DIR=tests/fixtures/real_tasks \
    SHANNON_RECORD_PROVIDER={{ provider }} \
    SHANNON_MODEL={{ model }} \
    cargo nextest run --test live_tests -p shannon-cli --config-file .config/nextest.toml \
        --run-ignored ignored-only --test-threads=1 --no-fail-fast -E 'test(record_task_)'

# 回放录制的 fixture (不需要 API key)
replay:
    cargo nextest run --test live_tests -p shannon-cli --config-file .config/nextest.toml \
        -E 'test(replay_) + test(test_write_file) + test(test_record_provider) + test(test_create_workspace) + test(test_provider_key_env) + test(test_all_nested)'

# 回放: 用 fixture 驱动 agent 做端到端 CI 回归 (ADR 0003 Phase 1, 不需要 API key)
# 与 `replay` (只做 fixture 结构校验) 区分开 —— 这里真正重跑 agent + 工具 + 工作区副作用。
replay-agent:
    cargo nextest run --test live_tests -p shannon-cli --config-file .config/nextest.toml \
        -E 'test(replay_agent_)'

# 分析录制 fixture 中的缓存命中统计
cache-stats:
    @echo "Cache statistics from recorded fixtures:"
    @for f in tests/fixtures/real_tasks/*.jsonl; do \
        [ -f "$f" ] || continue; \
        echo ""; \
        echo "=== $f ==="; \
        jq -s 'map(select(.cache_read_input_tokens != null)) | {total: length, cache_hits: map(select(.cache_read_input_tokens > 0)) | length, total_created: map(.cache_creation_input_tokens // 0) | add, total_read: map(.cache_read_input_tokens // 0) | add, hit_rate: ((map(select(.cache_read_input_tokens > 0)) | length) / length * 100 | tostring + "%")}' "$f" 2>/dev/null || echo "(no cache data)"; \
    done

# ── 内部 helper ──────────────────────────────────────────────────────────────

[private]
_build:
    cargo build -p shannon-cli

[private]
_check-api-key:
    @if [ -z "${SHANNON_API_KEY:-}" ]; then echo "Set SHANNON_API_KEY first"; exit 1; fi

# ── 基准测试 ──────────────────────────────────────────────────────────────

# 微基准测试
bench:
    cargo bench

# 编译 release binary
build:
    cargo build --release -p shannon-cli

# ── Desktop ──────────────────────────────────────────────────────────────
# shannon-desktop was extracted to its own repo at ../shannon-desktop.
# Run these recipes from the shannon-code checkout; they shell into the
# sibling repo. The desktop Cargo.toml has a [patch] override that points
# shannon-* deps at this checkout, so engine changes are picked up locally.

DESKTOP_DIR := "../shannon-desktop"

# Desktop app (dev build, needs Tauri system deps)
desktop:
    cd {{DESKTOP_DIR}} && cargo build --features tauri

# Desktop app (release build)
desktop-release:
    cd {{DESKTOP_DIR}} && cargo build --release --features tauri

# Desktop crate tests (no system deps needed)
desktop-test:
    cd {{DESKTOP_DIR}} && cargo test

# Desktop UI development and build
desktop-ui:
    cd {{DESKTOP_DIR}}/ui && pnpm install && pnpm dev

desktop-build:
    cd {{DESKTOP_DIR}}/ui && pnpm build && cd {{DESKTOP_DIR}} && cargo tauri build
