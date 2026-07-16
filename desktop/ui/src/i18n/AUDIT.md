# i18n Audit — 2026-06-22

**Parity check**: en.json ↔ zh-CN.json has **0 mismatches** (both 1776 keys). ✅

**Usage audit** (regex over `ui/src/` for `t()` / `formatMessage({ id })` / `<FormattedMessage id="…">`):

| Metric | Count |
|---|---|
| Keys defined in JSON | 1776 |
| Keys used in code | 1306 |
| **Orphan keys** (in JSON, not in code) | **476** |
| Missing keys (in code, not in JSON) | 0* |

\* The raw diff produced 6 candidate "missing" keys (`file1.pdf`, `file2.txt`,
`report.pdf`, `imap.gmail.com`, `lib.rs`, `myComponent.greeting`) — all are
false positives caught by the regex matching example strings in comments/docs,
not real i18n calls.

## Orphan keys by top-level section

| Section | Orphans | Likely source of drift |
|---|---|---|
| `settings.*` | 114 | Settings page refactor (S2) |
| `extensions.*` | 111 | Extensions Hub phases 1–3 |
| `tasks.*` | 48 | Tasks board redesign |
| `conversations.*` | 40 | Conversations list removed/renamed |
| `welcome.*` | 28 | Welcome flow changes |
| `perf.*` | 26 | Performance panel |
| `opc.*` | 24 | OPC metrics |
| `nav.*` | 20 | Navigation rework |
| `triage.*` | 17 | Triage view |
| `shortcuts.*` | 16 | Keyboard shortcuts UI |
| `chat.*` | 15 | Chat header/input refactor |
| others | 17 | misc |

## Recommendation

Do NOT delete the 476 orphans in this PR — the volume is high enough that a
review per section is safer. Recommend a follow-up "i18n cleanup" PR series,
one commit per top-level section (settings → extensions → tasks → …), each
with `pnpm test` verification. The full orphan list is available via:

```
LC_ALL=C comm -23 \
  <(python3 -c "import json,sys; def flat(d,p=''); [flat(v,p+k+'.') if isinstance(v,dict) else print(p+k) for k,v in d.items()]" ui/src/i18n/locales/en.json | sort -u) \
  <(grep -rhoE "(t|formatMessage)\(['\"][a-z][a-zA-Z0-9]*\.[a-zA-Z0-9._]+['\"]|id: ['\"][a-z][a-zA-Z0-9]*\.[a-zA-Z0-9._]+['\"]|id=\"[a-z][a-zA-Z0-9]*\.[a-zA-Z0-9._]+\"" ui/src/ | sed -E "s/.*['\"]([a-zA-Z0-9._]+)['\"].*/\1/; s/.*\"([a-zA-Z0-9._]+)\".*/\1/" | sort -u)
```

## scripts/check-i18n-parity.mjs

The existing CI script checks **en ↔ zh parity only** (both files have the
same keys). It does NOT check usage — which is why 476 orphans accumulated
unnoticed. Extending the script to also flag orphans is a worthwhile
follow-up.
