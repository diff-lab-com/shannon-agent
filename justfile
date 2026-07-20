# Shannon-Agent monorepo task runner.
# ACTIVATED IN PHASE 5 (see ../MIGRATION.md): copy to repo root.
# `just` discovers this at the repo root. Requires: cargo, pnpm, bun.
#
# Note: desktop/ui and gateway are INDEPENDENT TS packages (no pnpm workspace).
# Each is installed/built in its own directory — see `install` below.

# shannon-model env override; consumed by record/record-with recipes
shannon_model := env_var_or_default("SHANNON_MODEL", "")

# ─────────────────────────────────────────────────────────────────────
# Fixture 治理规则(2026-07-21 制定,详见 plan:
#   docs/superpowers/plans/2026-07-21-justfile-record-replay-restore.md)
# ─────────────────────────────────────────────────────────────────────
# tests/fixtures/real_tasks/ 下的 .jsonl 是真实 LLM 录制产物。
# 默认:不 commit (.gitignore 拦截);精选 commit 走 PR 评审。
# 现有 4 个 force-tracked:bash_command / create_file /
# overwrite_existing_file / read_and_edit (Phase 1 确定集)。
# 重新评估 CI 集成触发条件:精选 fixture ≥ 10 或 Phase 2 ADR 落地。
# ─────────────────────────────────────────────────────────────────────

default:
    @just --list

# ---------- Install (TS packages are independent — no `pnpm install -r`) ----------

install:
    cd desktop/ui && pnpm install
    cd gateway && pnpm install

# ---------- Products (each independently buildable) ----------

# shannon-code product: the CLI/TUI coding agent.
build-code:
    cargo build --release -p shannon-cli

# shannon-desktop product: Tauri desktop app (member `desktop`).
build-desktop:
    cargo build --release -p shannon-desktop

# shannon-gateway product: TS platform bridge, compiled to a standalone binary.
build-gateway:
    cd gateway && pnpm build:binary

# ---------- Protocol codegen (Phase A) ----------

# Regenerate gateway TS types + OpenAPI from crates/shannon-api-protocol.
gen-protocol:
    cargo run -p shannon-api-protocol --bin gen-ts
    cd gateway && pnpm typecheck

# ---------- Lint / fmt ----------

fmt:
    cargo fmt --all

# Note: clippy runs against the workspace library + bin targets only (matches
# the original shannon-code CI gate). Test targets are intentionally NOT
# linted here -- the upstream test code uses `unwrap()` extensively and was
# never subject to `clippy --all-targets` in the original justfile; re-linting
# it would block CI for pre-existing patterns the migration does not own.
lint:
    cargo clippy --workspace -- -D warnings
    cd desktop/ui && pnpm lint
    cd gateway && pnpm typecheck

# ---------- Test ----------

test-rust:
    cargo nextest run --workspace || cargo test --workspace -- --test-threads=1

test-ui:
    cd desktop/ui && pnpm test:ci

test-gateway:
    cd gateway && pnpm test

test: test-rust test-ui test-gateway

# ---------- Supply chain ----------

deny:
    cargo deny check

# ---------- Full CI gate ----------

ci: fmt lint deny gen-protocol test
    @echo "✅ all gates green"

# ── Recording / Replay (ADR 0003, Phase 1 本地 harness) ──
#
# 录制需要:SHANNON_API_KEY + 网络访问 provider + 选定 model
# 回放不需要:API key,只跑本地 fixture 加载与离线校验
# fixture 路径:tests/fixtures/real_tasks/{provider}_{model}_{session}.jsonl
#
# 录制示例:
#   SHANNON_API_KEY=sk-... just record
#   SHANNON_API_KEY=sk-... SHANNON_MODEL=claude-sonnet-4 just record
#   SHANNON_API_KEY=sk-... just record-with anthropic claude-sonnet-4
#   SHANNON_API_KEY=sk-... just record-with minimax MiniMax-M3
#
# 回放示例:
#   just replay         # 离线结构校验
#   just replay-agent   # 端到端 agent 回放(本地,ADR 0003 Phase 1 范围)

# 默认录制(anthropic,model 走 SHANNON_MODEL 或 "unknown")
record: (_build-cli) (_check-api-key)
    @echo "Recording with provider=anthropic, model={{ if shannon_model != "" { shannon_model } else { "unknown" } }}..."
    SHANNON_RECORD_DIR=tests/fixtures/real_tasks \
    SHANNON_RECORD_PROVIDER=anthropic \
    SHANNON_MODEL={{ if shannon_model != "" { shannon_model } else { "unknown" } }} \
    cargo nextest run --test live_tests -p shannon-cli -- \
        --ignored --test-threads=1 --no-fail-fast \
        -E 'test(record_task_)'

# 任意 provider + model
record-with provider model: (_build-cli) (_check-api-key)
    @echo "Recording with provider={{ provider }}, model={{ model }}..."
    SHANNON_RECORD_DIR=tests/fixtures/real_tasks \
    SHANNON_RECORD_PROVIDER={{ provider }} \
    SHANNON_MODEL={{ model }} \
    cargo nextest run --test live_tests -p shannon-cli -- \
        --ignored --test-threads=1 --no-fail-fast \
        -E 'test(record_task_)'

# 离线结构校验(no API key)
replay:
    cargo test --test live_tests -p shannon-cli -- --test-threads=1

# 端到端 agent 回放(ADR 0003 Phase 1 本地 harness,CI 不跑)
# replay_agent_* tests are #[ignore]'d in live_tests.rs; need --ignored flag.
# Nextest arg layout: nextest flags (-j1, -E) BEFORE --, test-binary args (--ignored) AFTER --.
replay-agent:
    cargo nextest run -p shannon-cli --test live_tests -j1 -E 'test(replay_agent_)' -- --ignored

# fixture 缓存命中率统计(jq + awk,按 fixture + provider 聚合)
# 注意:需要 jq;macOS/Linux 都有。Windows 不支持。
cache-stats:
    @GREEN='\033[32m'; \
    YELLOW='\033[33m'; \
    RED='\033[31m'; \
    NC='\033[0m'; \
    { \
        printf "%-58s  %6s  %12s  %12s  %9s  %9s\n" "fixture" "exch" "cache_read" "prompt_tot" "rate" "warm_rate"; \
        printf -- '-%.0s' {1..126}; echo; \
        for f in tests/fixtures/real_tasks/*.jsonl; do \
            [ -f "$f" ] || continue; \
            base=$(basename "$f"); \
            jq -rs --arg n "$base" '($n | split("_")[0]) as $provider | ([.[] | ((.cache_read_input_tokens // 0) // 0) as $cr | ((.cache_creation_input_tokens // 0) // 0) as $cc | (try (.response.body | scan("\"(prompt_tokens|input_tokens)\":\\s*([0-9]+)") | last | tonumber) catch 0) as $pf | (if $pf > ($cr + $cc) then $pf else ($pf + $cr + $cc) end) as $total | {cr: $cr, total: $total, is_warm: ($total > 0 and $cr * 20 >= $total)}]) as $stats | ($stats | map(.cr) | add) as $tcr | ($stats | map(.total) | add) as $tp | ($stats | map(select(.is_warm) | .cr) | add // 0) as $wcr | ($stats | map(select(.is_warm) | .total) | add // 0) as $wtp | ($stats | length) as $exch | "\($n)\t\($provider)\t\($exch)\t\($tcr)\t\($tp)\t\($wcr)\t\($wtp)"' "$f" 2>/dev/null; \
        done; \
    } | awk -F'\t' -v green="$GREEN" -v yellow="$YELLOW" -v red="$RED" -v nc="$NC" \
        'NR==1 {print; next} NR==2 {print; next} { \
            rate = ($5 > 0 ? $4 * 100 / $5 : 0); \
            warm_rate = ($7 > 0 ? $6 * 100 / $7 : -1); \
            color = (rate >= 70 ? green : (rate >= 40 ? yellow : red)); \
            warm_str = (warm_rate < 0 ? "N/A" : sprintf("%5.1f%%", warm_rate)); \
            warm_color = (warm_rate < 0 ? nc : (warm_rate >= 70 ? green : (warm_rate >= 40 ? yellow : red))); \
            printf "%-58s  %6s  %12s  %12s  %s%8.1f%%%s  %s%9s%s\n", $1, $3, $4, $5, color, rate, nc, warm_color, warm_str, nc; \
        }'

# ── Local fixture migration (dev convenience, no commit) ──
#
# 从同级 stale pre-migration 副本(.../../shannon-agent/shannon-code/...)
# 把 55 个老 fixture 拷到 monorepo 同名目录,本地用,不 commit。
#
# 55 个 fixture schema 与新 repo 兼容(同 JSONL keys,同加载器),
# 见 docs/superpowers/plans/2026-07-21-justfile-record-replay-restore.md Q1 取舍。
#
# 注意:依赖 stale 副本(非 git)存在。如果清理掉了,本 recipe 报错。

migrate-fixtures:
    #!/usr/bin/env bash
    set -euo pipefail
    SRC="../../shannon-agent/shannon-code/tests/fixtures/real_tasks"
    DST="tests/fixtures/real_tasks"
    if [ ! -d "$SRC" ]; then
        echo "ERROR: source dir not found: $SRC"
        echo "Expected location of pre-migration shannon-code fixture cache."
        echo "If this stale copy has been cleaned up, fixtures must be re-recorded via 'just record'."
        exit 1
    fi
    mkdir -p "$DST"
    copied=0
    skipped=0
    for f in "$SRC"/*.jsonl; do
        [ -f "$f" ] || continue
        name=$(basename "$f")
        if [ -f "$DST/$name" ]; then
            skipped=$((skipped + 1))
        else
            cp "$f" "$DST/$name"
            copied=$((copied + 1))
        fi
    done
    total=$(ls "$DST"/*.jsonl | wc -l)
    echo "✅ migrate-fixtures: copied=$copied skipped=$skipped total=$total"
    echo "   source: $SRC"
    echo "   dest:   $DST"
    echo "   (none committed — see justfile header for curated-commit rule)"

# ── private helpers ──

[private]
_build-cli:
    cargo build -p shannon-cli

[private]
_check-api-key:
    @if [ -z "${SHANNON_API_KEY:-}" ]; then echo "Set SHANNON_API_KEY first"; exit 1; fi

# ---------- Release helpers (Phase 6) ----------

# Verify a clean-clone desktop build (the Phase 2 KPI), runnable anytime.
kpi-clean-build:
    cargo build -p shannon-desktop

# ---------- Release prep: bump every version source, commit, tag ----------
# Usage: just release-prep 0.7.0
#   then: git push && git push origin v0.7.0   (triggers release.yml)
# Bumps the 4 independent version sources so cargo-dist + tauri + gateway
# + `shannon --version` all agree with the tag:
#   1) Cargo.toml workspace.package.version  (crates with version.workspace=true inherit)
#   2) desktop/tauri.conf.json  "version"  (Tauri does NOT read the cargo workspace)
#   3) gateway/package.json        "version"
#   4) `shannon --version` display value is tied to the workspace version
#      automatically via clap::crate_version!() in shannon-cli (see task C).
release-prep version:
    #!/usr/bin/env bash
    set -euo pipefail
    # Derive the CURRENT workspace version — every release-version source that
    # tracks it is bumped from this value, so they stay in lockstep.
    OLD="$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
    echo "release-prep: ${OLD} -> {{version}}"
    # 1) root Cargo.toml — BOTH workspace.package.version AND every
    #    [workspace.dependencies] internal path-dep pin. These pins MUST track
    #    the workspace version: a stale `version = "<OLD>"` makes the requirement
    #    `^<OLD>` fail to resolve against the newly-versioned crate (this broke
    #    v0.7.0-rc1: pins stayed 0.6.0 while the crate bumped to rc1).
    sed -i "s/version = \"${OLD}\"/version = \"{{version}}\"/g" Cargo.toml
    # 2) desktop has its OWN [package] version (NOT workspace-inherited)
    sed -i "s/^version = \"${OLD}\"/version = \"{{version}}\"/" desktop/Cargo.toml
    # 3) Tauri (separate hardcoded version; Tauri does not read the cargo workspace)
    sed -i 's/^    "version": ".*"/    "version": "{{version}}"/' desktop/tauri.conf.json
    # 4) gateway (independent TS package)
    sed -i 's/^  "version": ".*"/  "version": "{{version}}"/' gateway/package.json
    # `shannon --version` is tied to the workspace version automatically via
    # env!("CARGO_PKG_VERSION") in shannon-cli (version.workspace=true) — no sed.
    git add Cargo.toml desktop/Cargo.toml desktop/tauri.conf.json gateway/package.json
    git commit -m "chore(release): v{{version}}"
    git tag v{{version}}
    echo "✅ tagged v{{version}} — run: git push origin dev && git push origin v{{version}}"
