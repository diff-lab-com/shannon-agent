# Shannon 测试命令速查
#
# 日常开发:   just dev          (check + lint + test, 提交前跑一次)
# 云端 CI:    just test         (全量测试)
# 回归/调优:  just bench        (微基准测试)
# 录制:       just record       (真实 API → fixture, 需要 SHANNON_API_KEY)
# 发布:       just build        (编译 release binary)
#
# 安装: cargo install just cargo-nextest

# 日常开发：提交前跑一次 (check + lint + test)
dev:
    cargo check --workspace
    cargo clippy --workspace
    cargo nextest run --workspace --config-file .config/nextest.toml

# 全量测试 (CI 也用这个)
test:
    cargo nextest run --workspace --config-file .config/nextest.toml

# 完整测试 (nextest + doctests)
test-all: test
    cargo test --workspace --doc

# 快速类型检查
check:
    cargo check --workspace

# Lint
lint:
    cargo clippy --workspace

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
    cargo test --test live_tests -- --ignored --test-threads=1 record_task

# 回放录制的 fixture (不需要 API key)
replay:
    cargo test --test live_tests -- --test-threads=1 replay_ test_write_file test_record_provider test_create_workspace test_provider_key_env test_all_nested

# 微基准测试
bench:
    cargo bench

# 编译 release binary
build:
    cargo build --release -p shannon-cli
