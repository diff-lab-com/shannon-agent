# Terminal-Bench prebake (design notes, delivered 2026-08-29)

Status: **design + generator only — not executed.** Executing the prebake and
running the TB n=3 batch is the successor batch's job. Everything below was
learned from the t15 probe (`/tmp/tb-probe/`) on 2026-08-28.

## Why prebake exists

The t15 probe proved the full Terminal-Bench loop GREEN end-to-end (compose
build/up → engine provisioned into the client container → agent headless run
→ the task's own `run-tests.sh` decides the verdict), but every repetition
pays a **cold-provision tax inside the task container**: `run-tests.sh`
re-installs uv + pytest from the network on every verdict
(`apt-get update` + `curl | sh` + `uv pip install`), which got rate-limited
at ~936 s/rep. Nine pins × 3 reps of that is infrastructure cost, not model
capability. The fix is a derived image per pinned task with the toolchain
preinstalled at the exact locations/pins the corpus scripts expect.

## Deliverable

`generate-prebake.sh` reads the pin list
(`tests/eval/benchmarks/terminalbench_tasks.txt`, 9 tasks after the
intro-sudo removal) and for each pin emits:

- `<out>/<id>/build-base.sh` — builds `shannon-tb-prebake/base:<id>` from the
  task's **own** Dockerfile (context = corpus task dir, unmodified).
- `<out>/<id>/Dockerfile` — `FROM` the base + prebake layer: apt state baked,
  uv 0.7.13 at `$HOME/.local/bin` (+ `/usr/local/bin` symlink), pytest 8.4.1
  in `/opt/tb-test-venv` (+ `pytest` on PATH), warm `UV_CACHE_DIR` /
  `UV_PYTHON_INSTALL_DIR` inside the image.
- `<out>/<id>/prebake-context.json` — machine-readable tags + the compose
  runtime contract for the successor harness.
- `build-order.txt` — the pin order.

`--build` additionally docker-builds base+derived per task (not run yet).

## Runtime contract for the successor harness (tb-harness.sh v2)

1. Start from the t15-proven `/tmp/tb-probe/tb-harness.sh` and change ONLY
   the image source: set
   `T_BENCH_TASK_DOCKER_CLIENT_IMAGE_NAME=shannon-tb-prebake/prebaked:<id>`
   (read it from `prebake-context.json`) and run
   `docker compose ... up -d --no-build`. `--no-build` is mandatory — the
   image already exists; compose would otherwise rebuild from the task
   Dockerfile and silently erase the prebake.
2. Provision the engine into the client container exactly as t15 did
   (`docker cp` the binary, `chmod +x`, cwd `/app`).
3. **The container agent MUST receive `-e SHANNON_API_KEY`.** The CLI key
   chain does not read `ZHIPU_API_KEY` — measured the hard way in t15.
4. The verdict still comes ONLY from the task's own `run-tests.sh` + `tests/`,
   copied into the container exactly as t15 did. Prebake changes provisioning
   cost, never verification semantics. If a future task's `run-tests.sh`
   pins different tool versions, regenerate — never reuse a stale derived
   image across pin/corpus changes.
5. Emit `verdict.json` (SHANNON_BENCH_VERDICT_FILE) with the native
   run-tests exit code and honest token/cost sums from the session
   `events.jsonl` (the t15 harness already implements this correctly).

## Known risks / open items for the successor

- **Installer fast-path**: astral's `uv` installer may still phone home even
  when uv is already installed. If verdict latency still shows network
  stalls, wrap the "Setup uv and pytest" section of the *baked copy* of
  `run-tests.sh` in an idempotence guard
  (`command -v uv && pytest -V || <original block>`) at DERIVED-IMAGE BUILD
  time. This preserves the verification semantics (the pytest invocation
  line stays byte-identical) but MUST be called out in the batch report as a
  provisioning-only deviation from the corpus script.
- **Non-root task users**: some task images switch USER before the CMD. The
  `RUN` layer runs as root and bakes `/usr/local/bin` symlinks, so PATH
  resolution works for any user; `$HOME/.local/bin` cache warm only helps
  the root user — that is fine because `UV_CACHE_DIR` is absolute.
- **Image size**: ~9 derived images each carrying a uv cache + venv
  (~150–250 MB each). Prune with `uv cache prune` (the Dockerfile does) and
  budget disk before `--build`.
- **Pin drift gate**: run
  `bench_runner validate-pins --suite terminal_bench --tb-tasks <corpus>`
  before citing any score (already GREEN against this corpus on 2026-08-28;
  re-run after any corpus update).
