# Shannon 测试命令速查
#
# 日常开发:   just dev          (check + lint + test, 提交前跑一次)
# 云端 CI:    just ci           (全量测试 + doctests, 带 retry/timeout)
# 回归/调优:  just bench        (微基准测试, 与基线比较)
# 性能回归:   just perf         (性能阈值测试)
# 场景测试:   just scenarios    (YAML 声明式场景测试)
# 录制:       just record       (真实 API → fixture, 需要 SHANNON_API_KEY)
# 回放:       just replay       (回放录制 fixture, 不需要 key)
# 发布:       just build        (编译 release binary)
#
# 安装: cargo install just cargo-nextest

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

# 录制真实 API fixture (需要 SHANNON_API_KEY)
# 默认 anthropic，可改用其他 provider:
#   SHANNON_RECORD_PROVIDER=minimax just record
#   SHANNON_RECORD_PROVIDER=openai just record
record:
    #!/usr/bin/bash
    if [ -z "$SHANNON_API_KEY" ]; then echo "Set SHANNON_API_KEY first"; exit 1; fi
    SHANNON_RECORD_DIR=tests/fixtures/real_tasks \
    SHANNON_RECORD_PROVIDER=${SHANNON_RECORD_PROVIDER:-anthropic} \
    SHANNON_MODEL=${SHANNON_MODEL:-unknown} \
    cargo nextest run --test live_tests -p shannon-cli --config-file .config/nextest.toml \
        -E 'test(record_task_)'

# 回放录制的 fixture (不需要 API key)
replay:
    cargo nextest run --test live_tests -p shannon-cli --config-file .config/nextest.toml \
        -E 'test(replay_) + test(test_write_file) + test(test_record_provider) + test(test_create_workspace) + test(test_provider_key_env) + test(test_all_nested)'

# 微基准测试 (与基线比较)
bench:
    cargo bench

# 编译 release binary
build:
    cargo build --release -p shannon-cli
