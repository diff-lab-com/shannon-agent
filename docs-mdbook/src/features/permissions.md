# Permissions

Shannon Code provides a configurable permission system to control what tools can do.

## Approval Modes

| Mode | Behavior |
|------|----------|
| `Strict` | Require approval for every tool call |
| `Balanced` | Auto-approve reads, require approval for writes |
| `Permissive` | Auto-approve non-destructive, deny destructive |
| `FullAuto` | Auto-approve everything except critical operations |
| `BypassPermissions` | Approve all (only with explicit `--yes` flag) |

## Permission Classifier

The `PermissionClassifier` evaluates each tool call:

1. **Rule-based classification** — Fast, deterministic
2. **LLM fallback** — For ambiguous cases (confidence < 0.7, Medium+ risk)

Classification tiers:
- `allow` — Auto-approve
- `soft_deny` — Prompt user
- `hard_deny` — Block execution
- Explicit user intent overrides classification

## Permission Profiles

Pre-configured profiles for common workflows:

| Profile | Reads | Writes | Destructive |
|---------|-------|--------|-------------|
| `strict` | approve | approve | deny |
| `balanced` | auto | approve | deny |
| `permissive` | auto | auto | deny |
| `custom` | configurable | configurable | configurable |

Set via config or `SHANNON_PERMISSION_PROFILE` env var.

## CI/Headless Mode

In headless mode (`--prompt`), `FullAuto` is the default. `BypassPermissions` requires explicit `--yes` flag.
