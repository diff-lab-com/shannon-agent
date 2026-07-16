# Automation / Hook Surface Audit (C2)

**Scope:** `src/automation_commands.rs` and any frontend surface that
writes hook-related state. Engine-side hook firing lives in
`shannon-core` and is out of scope for this audit.

## Commands surveyed

| Command | Surface | Writes to | Threat |
|---|---|---|---|
| `list_hook_events` | read-only | — | none — returns static catalog |
| `list_permission_profiles` | read-only | — | reads `.shannon/profiles/` + `.claude/profiles/`; no write |
| `save_custom_profile` | write | `.shannon/profiles/<name>.toml` in cwd | see below |
| `delete_custom_profile` | delete | `.shannon/profiles/<name>.toml` or `.claude/profiles/<name>.toml` | see below |

## Findings

### `save_custom_profile` — SAFE

- **Name validation** at lines 365–370 rejects any char outside
  `[A-Za-z0-9-_]`. This blocks `..`, `/`, `\`, and any shell metachar.
- **Path is constructed by format string** (`dir.join(format!("{trimmed}.toml"))`)
  so the validated name cannot escape `.shannon/profiles/`.
- **Description / auto_approve / confirm / deny arrays** are TOML-escaped
  by `toml_basic_string` (escapes `"`, `\`, `\n`, `\r`, `\t`). Control
  characters outside that set are NOT escaped, which violates TOML 1.0.
  The engine's strict parser would reject the file — no code execution,
  but the user gets a confusing error.

  **Severity:** informational. Future hardening: reject or escape
  control chars in `description` before serialization.

### `delete_custom_profile` — SAFE

- Same name validation as `save_custom_profile`.
- Only deletes within `.shannon/profiles/` or `.claude/profiles/` under
  cwd. No path expansion, no symlink-following (uses `path.is_file()`).
- Returns the list of removed paths; nothing is executed.

### `list_hook_events` — SAFE

- Returns a static in-memory catalog. No disk, no IPC, no eval.

### `list_permission_profiles` — SAFE

- Read-only delegation to `CustomProfileRegistry::load_from_dirs()`.

## Engine-side gaps (not in this repo)

The actual hook firing surface (30 events cataloged here) lives in
`shannon-core::hooks`. Hook scripts are executed by the engine, not by
the desktop shell. Any audit of hook script sandboxing / resource limits
must happen against `shannon-code`, not this repo.

## Conclusion

No exploitable surface found in `automation_commands.rs`. The one
informational finding (control-char escaping in `toml_basic_string`) is
a robustness issue, not a security issue — the engine's parser rejects
the malformed TOML rather than executing anything.
