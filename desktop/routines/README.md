# Routine Templates

Each `.toml` file in this directory is a pre-made routine that users can
instantiate from the Routines page (`Browse templates →`). Instantiation
copies the template into the user's scheduled-task store; the source file
stays read-only.

## Schema

Required keys:

| key              | type   | notes                                                    |
|------------------|--------|----------------------------------------------------------|
| `id`             | string | kebab-case; must match the filename (minus `.toml`)      |
| `name`           | string | display name                                             |
| `description`    | string | one-line summary shown in the browser                    |
| `category`       | string | `engineering` / `security` / `productivity` / `finops` / `documentation` / `operations` (free-form, but keep it short) |
| `prompt`         | string | the prompt the routine runs                              |
| `trigger_type`   | string | `cron` or `interval`                                     |

Trigger-specific keys:

- `trigger_type = "cron"` → `cron_expr` (string, required), `timezone` (string, optional)
- `trigger_type = "interval"` → `interval_secs` (integer, required)

Optional keys: none yet. Future fields (max_fires, expires_at, depends_on)
will be added as the schema grows.

## Adding a template

1. Pick a unique kebab-case `id`.
2. Drop a new `<id>.toml` in this directory using the schema above.
3. Run `cargo test routine_templates` — the bundled test validates every
   `.toml` parses and that `id` matches the filename.

The runtime reader walks the directory at startup and on every
`list_routine_templates` call, so new files appear without a recompile.
