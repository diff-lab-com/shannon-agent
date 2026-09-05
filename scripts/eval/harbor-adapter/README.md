# harbor adapter — Shannon Code agent

Drives the **local dev build** of Shannon inside harbor task containers
(Terminal-Bench 2.x etc.). Verdicts always come from the task's own tests;
this adapter only replaces the "which agent runs in the container" half.

## Prerequisites

- harbor CLI in a venv: `python3 -m venv ~/.venvs/harbor && ~/.venvs/harbor/bin/pip install harbor`
- built engine binary (default `/home/ed/workspace/app/work/shannon/shannon-mono/target/debug/shannon`)
- zhipu credential exported for this run: `SHANNON_API_KEY`

## Invocation (P1d — terminal-bench 2.1, 89 tasks, n=1)

```bash
export SHANNON_API_KEY="$(python3 -c "import json;print(json.JSONDecoder().raw_decode(open('$HOME/.shannon/credentials/zhipu.json').read())[0]['value'])")"
export SHANNON_HARBOR_BIN=/home/ed/workspace/app/work/shannon/shannon-mono/target/debug/shannon
ADAPTER=/home/ed/workspace/app/work/shannon/shannon-eval/scripts/eval/harbor-adapter

~/.venvs/harbor/bin/harbor run \
  -d /home/ed/datasets/tb21/terminal-bench-2-1 \
  -a shannon_harbor_agent:Shannon \
  -m glm-5.3-flash \
  --ae SHANNON_PROVIDER=zhipu-coding-plan \
  --ak max_turns=80 \
  -n 1 \
  --output-path ~/.shannon/eval/v2-glm-tb21-harbor \
  --agent-include-logs 'shannon.*' \
  PYTHONPATH=$ADAPTER ...   # see note below
```

Notes:
- `PYTHONPATH=$ADAPTER` must prefix the `harbor` invocation (env form:
  `PYTHONPATH=$ADAPTER ~/.venvs/harbor/bin/harbor run ...`); `-a` resolves
  `module.path:ClassName` through it.
- Prefer the **bare** model id + `--ae SHANNON_PROVIDER=...` (as shown); the
  `provider/model` form also works but exercises harbor's provider registry,
  which does not know `zhipu-coding-plan`.
- Validation gate before the full run: 2-3 tasks via `-t <task-id>` (or a
  short dataset dir) must pass end-to-end including task-native grading.
- Anchor rules (plan §三) apply unchanged: glm-5.3-flash @ zhipu-coding-plan,
  n / date / anchor triple on any cited number.
