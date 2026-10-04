# Permissions

Shannon controls what tools may do without asking through **approval modes** (a 4+3 model) plus an **allow/ask/deny rule layer** that is effective in every mode.

## Approval modes (4 + 3)

Autonomy ladder — `Shift+Tab` in the terminal cycles the three stops; the desktop composer pill mirrors them:

| Mode | Behavior |
|------|----------|
| `ask` | Reads run freely; every other tool asks first. |
| `auto-edit` | File edits run without asking; commands still ask. **Engine default.** |
| `full-auto` | Everything below critical risk runs automatically. |

Workflow tier — entered via `/plan`, never a cycle stop:

| Mode | Behavior |
|------|----------|
| `plan` | Read-only until the plan is approved; approval unlocks plan-scoped auto-run (deny rules and critical-risk denial still bind). Exiting restores the previous ladder mode. |

Expert modes — explicit `/mode` in the terminal, Settings → General → "Advanced" in the desktop:

| Mode | Behavior |
|------|----------|
| `readonly` | Read-only analysis; writes and bash are denied. |
| `dontAsk` | Never waits: allow-listed tools and reads pass, the rest is **denied** (CI posture). |
| `bypassPermissions` | Skips all checks except deny rules. Guardrails: refused as root, `SHANNON_DISABLE_BYPASS=1` kill switch, first-use confirmation. |

## Rule layer (settings.json)

`permissions.allow` / `permissions.ask` / `permissions.deny` are effective in **every mode**:

- `deny` blocks even under `bypassPermissions`;
- `ask` forces a prompt even in `full-auto` (and is denied under `dontAsk`);
- `allow` pre-approves but never overrides critical-risk denial.

`permissions.defaultMode` seeds the startup mode; project-level files may not set bypass/dontAsk.

## Permission classifier

The rule-based `PermissionClassifier` (5 risk levels, bash command analysis, MCP verb classification) is the decision engine inside the auto modes. An optional LLM hardening layer (`permissions.llm_fallback`) can escalate ambiguous medium+ risk cases — it may only tighten, never loosen, a verdict.

## Permission profiles

`permission_profile` in `config.toml` (or `SHANNON_PERMISSION_PROFILE`): `strict` / `balanced` / `permissive` / `custom:<name>` from `.shannon/profiles/*.toml`. A profile applies on startup; an explicit mode choice (CLI flag, `/mode`, desktop tier) wins.

## CI / headless

Headless (`--prompt`) defaults to `full-auto`; `--yes` requests `bypassPermissions` (subject to the guardrails above). `--permission-mode <token>` overrides on every headless path. The `permissions.max_auto_approvals` breaker (default off, `--max-auto-approvals N`) forces exit code 8 when the budget is exhausted.
