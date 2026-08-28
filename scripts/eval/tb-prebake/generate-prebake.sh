#!/usr/bin/env bash
# generate-prebake.sh — Terminal-Bench pin-slice prebaked-image generator
# (§4.13 companion to the t15 probe findings).
#
# PROBLEM (measured by t15, 2026-08-28): the full TB loop is GREEN but every
# verdict pays a cold-provision tax inside the task container —
# run-tests.sh re-installs uv + pytest from the network on EVERY repetition
# (apt-get update + curl | sh + uv pip install), rate-limited at
# ~936 s/rep. That tax is infrastructure, not capability; baking it out is
# the difference between a usable batch and a rate-limited crawl.
#
# WHAT THIS GENERATOR EMITS (per pinned task, no docker required):
#   <out>/<id>/Dockerfile            FROM base image + prebake layer
#   <out>/<id>/prebake-context.json  tags + corpus paths for the runner
# and a global <out>/build-order.txt. Pass --build to ALSO docker-build the
# base and derived images (NOT executed by the batch-1 batch — TB is the
# successor batch's job).
#
# DERIVED IMAGE DESIGN (the part that kills the 936 s):
#   1. FROM <base> — the task's own image built from its own Dockerfile via
#      its own docker-compose.yaml. No task file is edited.
#   2. Prebake layer installs the EXACT toolchain run-tests.sh expects —
#      uv 0.7.13 (pinned by the corpus's own scripts) and pytest 8.4.1 —
#      at the canonical locations ($HOME/.local/bin/uv, warm UV_CACHE_DIR,
#      /opt/tb-test-venv) so the "setup uv and pytest" section of
#      run-tests.sh degrades to cache hits instead of cold network fetches.
#   3. apt state is baked and lists cleaned; curl stays installed because
#      several run-tests.sh scripts apt-install it per verdict.
#   4. UV_CACHE_DIR/UV_PYTHON_INSTALL_DIR point INSIDE the image so `uv`
#      never needs the network for the pinned versions.
#
# RUNTIME CONTRACT for the successor harness (tb-harness.sh v2):
#   - set T_BENCH_TASK_DOCKER_CLIENT_IMAGE_NAME=shannon-tb-prebake/prebaked:<id>
#   - `docker compose ... up -d --no-build` — the image exists, compose must
#     NOT rebuild; the task's own compose file is otherwise used verbatim.
#   - verdict STILL comes only from the task's own run-tests.sh + tests/,
#     copied in exactly as t15 did. The prebake changes provisioning cost,
#     never verification semantics.
#   - the container agent MUST receive the key via -e SHANNON_API_KEY (the
#     CLI key chain does not read ZHIPU_API_KEY — measured t15 gotcha).
#
# Usage:
#   generate-prebake.sh [--out DIR] [--corpus DIR] [--pins FILE] [--build]
set -u

SELF_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SELF_DIR/../../.." && pwd)"

OUT="/tmp/tb-prebake"
CORPUS="${SHANNON_TB_TASKS_DIR:-/home/ed/datasets/terminal-bench/repo/original-tasks}"
PINS="$REPO_ROOT/tests/eval/benchmarks/terminalbench_tasks.txt"
BUILD=0
BASE_REPO="shannon-tb-prebake"
UV_VERSION="0.7.13"      # the version the corpus's own run-tests.sh pin
PYTEST_VERSION="8.4.1"   # ditto

while [ $# -gt 0 ]; do
  case "$1" in
    --out) OUT="${2:?}"; shift 2 ;;
    --corpus) CORPUS="${2:?}"; shift 2 ;;
    --pins) PINS="${2:?}"; shift 2 ;;
    --build) BUILD=1; shift ;;
    -h|--help) grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "unknown flag '$1'" >&2; exit 2 ;;
  esac
done

mkdir -p "$OUT"
: > "$OUT/build-order.txt"

pin_count=0
while IFS= read -r line; do
  id="${line%%#*}"
  id="$(echo "$id" | tr -d '[:space:]')"
  [ -n "$id" ] || continue
  task_dir="$CORPUS/$id"
  if [ ! -f "$task_dir/docker-compose.yaml" ]; then
    echo "generate-prebake: SKIP $id (missing docker-compose.yaml in $task_dir)" >&2
    continue
  fi

  # Resolve the CLIENT service's build (context + dockerfile) from the task's
  # own compose file — most tasks are single-service with build context = the
  # task dir, but e.g. simple-sheets-put builds the client from client/.
  build_info="$(python3 - "$task_dir/docker-compose.yaml" <<'PYEOF'
import sys, yaml
compose = yaml.safe_load(open(sys.argv[1]))
for name, svc in (compose.get("services") or {}).items():
    image = svc.get("image") or ""
    if "T_BENCH_TASK_DOCKER_CLIENT_IMAGE_NAME" in image:
        build = svc.get("build") or {}
        if isinstance(build, str):
            print(f"{build}\nDockerfile")
        else:
            ctx = build.get("context") or "."
            df = build.get("dockerfile") or "Dockerfile"
            print(f"{ctx}\n{df}")
        break
else:
    sys.exit(3)
PYEOF
)" || { echo "generate-prebake: SKIP $id (no client service with T_BENCH_TASK_DOCKER_CLIENT_IMAGE_NAME)" >&2; continue; }
  build_ctx="$(echo "$build_info" | sed -n 1p)"
  build_df="$(echo "$build_info" | sed -n 2p)"
  ctx_dir="$task_dir/$build_ctx"
  [ -f "$ctx_dir/$build_df" ] || { echo "generate-prebake: SKIP $id (client Dockerfile not found: $ctx_dir/$build_df)" >&2; continue; }
  pin_count=$((pin_count + 1))

  base_tag="$BASE_REPO/base:$id"
  derived_tag="$BASE_REPO/prebaked:$id"
  dest="$OUT/$id"
  mkdir -p "$dest"

  # 1. build the base image from the task's OWN Dockerfile (unmodified).
  cat > "$dest/build-base.sh" <<EOF
#!/usr/bin/env bash
# Base image = the task's own client-service Dockerfile, unmodified
# (context: $ctx_dir · dockerfile: $build_df · resolved from the task's
# own docker-compose.yaml client service).
set -euo pipefail
docker build -f "$ctx_dir/$build_df" -t "$base_tag" "$ctx_dir"
EOF
  chmod +x "$dest/build-base.sh"

  # 2. derived image: prebake layer on top.
  cat > "$dest/Dockerfile" <<EOF
# Prebaked Terminal-Bench task image — GENERATED by scripts/eval/tb-prebake.
# Base: the task's own image ($base_tag). Verification semantics unchanged:
# the task's own run-tests.sh + tests/ still deliver the verdict (see
# tb-prebake/README.md). Only provisioning cost is baked out.
FROM $base_tag

# Pin the toolchain to exactly what the corpus's run-tests.sh installs.
ENV UV_CACHE_DIR=/opt/tb-uv-cache \\
    UV_PYTHON_INSTALL_DIR=/opt/tb-uv-python \\
    TB_PREBAKED=1

RUN set -eux; \\
    apt-get update; \\
    apt-get install -y --no-install-recommends curl ca-certificates; \\
    rm -rf /var/lib/apt/lists/*; \\
    curl -LsSf https://astral.sh/uv/$UV_VERSION/install.sh | sh; \\
    ln -sf "\$HOME/.local/bin/uv" /usr/local/bin/uv; \\
    uv venv /opt/tb-test-venv; \\
    uv pip install --python /opt/tb-test-venv/bin/python pytest==$PYTEST_VERSION; \\
    ln -sf /opt/tb-test-venv/bin/pytest /usr/local/bin/pytest; \\
    uv cache prune
EOF

  # 3. machine-readable context for the successor runner.
  cat > "$dest/prebake-context.json" <<EOF
{
  "task_id": "$id",
  "corpus_dir": "$task_dir",
  "base_image": "$base_tag",
  "derived_image": "$derived_tag",
  "uv_version": "$UV_VERSION",
  "pytest_version": "$PYTEST_VERSION",
  "compose_runtime": {
    "env": { "T_BENCH_TASK_DOCKER_CLIENT_IMAGE_NAME": "$derived_tag" },
    "up_flags": ["--no-build"],
    "notes": "compose file used verbatim from the task dir; --no-build is mandatory (image already exists)"
  }
}
EOF

  echo "$id" >> "$OUT/build-order.txt"

  if [ "$BUILD" -eq 1 ]; then
    echo "[prebake] building base + derived images for $id"
    "$dest/build-base.sh"
    docker build -t "$derived_tag" "$dest"
  fi
done < "$PINS"

echo "generate-prebake: $pin_count task(s) generated under $OUT"
[ "$BUILD" -eq 1 ] && echo "generate-prebake: images built (verify with: docker images 'shannon-tb-prebake/*')"
echo "generate-prebake: next (successor batch) — wire tb-harness.sh v2 to the derived tags in prebake-context.json"
