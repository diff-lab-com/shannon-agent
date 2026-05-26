# Shannon 测试命令速查
#
# 日常开发:   just dev          (check + lint + test, 提交前跑一次)
# 云端 CI:    just test         (全量测试)
# 回归/调优:  just bench        (微基准测试)
# 录制:       just record       (真实 API → fixture, 需要 SHANNON_API_KEY)
# 发布:       just build        (编译 release binary)
#
# 安装: cargo install just

# 日常开发：提交前跑一次 (check + lint + test)
dev:
    cargo check --workspace
    cargo clippy --workspace
    cargo test --workspace -- --test-threads=1

# 全量测试 (CI 也用这个)
test:
    cargo test --workspace -- --test-threads=1

# 快速类型检查
check:
    cargo check --workspace

# Lint
lint:
    cargo clippy --workspace

# 录制真实 API fixture (需要 SHANNON_API_KEY)
record:
    #!/usr/bin/bash
    if [ -z "$SHANNON_API_KEY" ]; then echo "Set SHANNON_API_KEY first"; exit 1; fi
    SHANNON_RECORD_DIR=tests/fixtures/real_tasks \
    cargo test --test live_tests -- --ignored --test-threads=1

# 回放录制的 fixture (不需要 API key)
replay:
    cargo test --test live_tests -- --test-threads=1

# 微基准测试
bench:
    cargo bench

# 编译 release binary
build:
    cargo build --release -p shannon-cli
