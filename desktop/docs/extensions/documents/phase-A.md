# Documents Extension — Phase A Plan

**Status:** Draft, awaiting user sign-off on scope
**Decision basis:** `feedback_extension_first_architecture.md` + 2026-06-22 user directive
**Target:** Q4 2026, ~3 weeks
**Distribution target:** Extensions Hub catalog entry in Shannon Desktop

## 1. Objective

Ship a Shannon Extension that lets a user create / edit / beautify office
documents (DOCX, PPTX, XLSX, MD, PDF) from the Shannon Desktop chat by
orchestrating host-installed CLI tools. **No document logic lands in the
Shannon Desktop core** — only the extension, plus minimal host-requirements
detection in the core.

## 2. Repository layout

Separate repo `shannon-documents-ext` (mirrors `shannon-skills` pattern):

```
shannon-documents-ext/
├── README.md
├── .claude-plugin/
│   └── marketplace.json          # entry point for MarketplacePluginInstaller
├── SKILL.md                      # bundle-level overview, loaded by Shannon
├── skills/
│   ├── create-doc/
│   │   ├── SKILL.md              # skill-scoped instructions
│   │   ├── create-doc.sh         # pandoc invocation
│   │   └── templates/
│   │       ├── meeting-notes.md
│   │       ├── one-pager.md
│   │       └── report.md
│   ├── edit-doc/
│   │   ├── SKILL.md
│   │   └── edit-doc.py           # python-docx runner
│   ├── beautify-doc/
│   │   ├── SKILL.md
│   │   └── beautify-doc.py       # reference-docx injection via pandoc
│   └── convert-doc/
│       ├── SKILL.md
│       └── convert-doc.sh        # pandoc cross-format
├── host-requirements.yaml        # parsed by core (section 4)
└── tests/
    ├── fixtures/
    │   └── ref.docx              # reference docx for styling
    └── golden/                   # expected outputs for snapshot tests
```

The `MarketplacePluginInstaller` already handles cloning this repo into
`~/.shannon/skills/shannon-documents-ext/`. No new installer code needed.

## 3. SKILL.md drafts

### 3.1 Bundle-level `SKILL.md`

```markdown
---
name: shannon-documents
description: Create, edit, and beautify DOCX/PPTX/XLSX/MD/PDF documents via host-installed pandoc, libreoffice, and python-docx.
version: 0.1.0
author: shannon-agent
host_requirements:
  pandoc: ">=2.19"
  libreoffice: optional
  python_docx: optional
  python_pptx: optional
  openpyxl: optional
---

# Shannon Documents Extension

When the user asks to create, edit, convert, or beautify a document,
use one of these skills:

- **create-doc** — generate a DOCX/MD/PDF from a template or natural-language outline
- **edit-doc** — apply text edits to an existing DOCX (python-docx)
- **beautify-doc** — restyle a DOCX using a reference template
- **convert-doc** — cross-format conversion via pandoc (md↔docx↔pdf↔html)

Each skill checks host-requirements on first run and prints install hints
if a tool is missing.
```

### 3.2 Skill-level `skills/create-doc/SKILL.md` (representative)

```markdown
---
name: create-doc
description: Generate a new document from a template or natural-language outline.
inputs:
  - name: format
    enum: [docx, md, pdf]
    default: docx
  - name: template
    enum: [meeting-notes, one-pager, report, blank]
    default: blank
  - name: outline
    type: string
    description: Markdown outline or natural-language description
---

# create-doc

## When to use
- "Draft a meeting-notes doc for today's standup"
- "Create a one-pager for project Shannon"
- "Generate a quarterly report template"

## Steps
1. If `template != blank`, copy `templates/${template}.md` to a working file
2. Fill in `${outline}` placeholders (LLM-driven)
3. Run: `pandoc working.md -o output.${format} ${"--reference-doc=ref.docx if format==docx"}`
4. Return the output file path

## Host requirements
- pandoc >= 2.19
- (for PDF) a LaTeX engine or `--pdf-engine=weasyprint`
```

## 4. Host-requirements detection

**Decision needed (flag for user):** do we add a generic `host_requirements`
parser to the Shannon Desktop core, or keep detection inside each skill's
first-run script?

**Option A — Core parser (recommended):**
- Add `src/extensions/host_requirements.rs` that parses the
  `host_requirements:` block from any skill's SKILL.md
- Expose `check_host_requirements(skill_id) -> HostReport` Tauri command
- UI surfaces missing tools in the install dialog (Extensions Hub)
- Pros: consistent UX across all extensions, one place to maintain
- Cons: small core footprint (~150 LOC + tests)

**Option B — Per-skill detection script:**
- Each skill ships a `check-host.sh` that prints JSON
- Skill SKILL.md documents the contract
- Pros: zero core changes
- Cons: inconsistent UX, every extension reinvents the wheel

**Recommendation:** Option A. The core addition is small and directly
supports the "extension-first but core-aware" principle in
`feedback_extension_first_architecture.md`.

### `host-requirements.yaml` schema (Option A)

```yaml
# Parsed by src/extensions/host_requirements.rs
tools:
  - name: pandoc
    binary: pandoc
    min_version: "2.19"
    check_args: ["--version"]
    install_hint:
      debian: "apt install pandoc"
      macos: "brew install pandoc"
      windows: "winget install --id JohnMacFarlane.Pandoc"
  - name: libreoffice
    binary: soffice
    required: false  # optional tool
    install_hint:
      debian: "apt install libreoffice"
      macos: "brew install --cask libreoffice"
  - name: python_docx
    python_module: docx
    required: false
    install_hint: "pip install python-docx"
```

## 5. Five template list (Phase A scope)

Templates ship in `skills/create-doc/templates/`:

| Template | Format | Source | Purpose |
|----------|--------|--------|---------|
| `meeting-notes.md` | DOCX out | Standup, 1:1, project sync | Most common first-use case |
| `one-pager.md` | DOCX out | Project summary, product brief | Exec-friendly short doc |
| `report.md` | DOCX + PDF out | Quarterly report, incident postmortem | Showcases PDF generation |
| `design-review.md` | DOCX out | Architecture decision record | Engineering audience |
| `blank.md` | MD out | Free-form | Escape hatch for custom docs |

Each template uses YAML front-matter + Markdown body so pandoc can
convert to DOCX with `--reference-doc=ref.docx` for consistent styling.

## 6. Pandoc generation path

### 6.1 DOCX
```
template.md → fill placeholders → working.md
pandoc working.md -o output.docx --reference-doc=tests/fixtures/ref.docx
```

### 6.2 PDF (via LaTeX or weasyprint)
```
pandoc working.md -o output.pdf --pdf-engine=xelatex
# fallback for hosts without LaTeX:
pandoc working.md -o output.pdf --pdf-engine=weasyprint
```

### 6.3 Reference docx
`tests/fixtures/ref.docx` is a checked-in Word file with paragraph + character
styles. Pandoc uses it as a style template. Updated by the maintainer; users
can override by placing their own `ref.docx` in the skill directory.

## 7. Distribution

The extension repo gets a catalog entry in Shannon Desktop's Extensions Hub
Featured tab. The catalog entry is a `CatalogEntry` row in
`src/extensions/catalog.rs` (or upstream JSON if we externalize):

```rust
CatalogEntry {
    id: "shannon.documents".into(),
    name: "Documents".into(),
    kind: AddonKind::Skill,
    source: CatalogSource::GitHubRepo {
        repo: "shannon-agent/shannon-documents-ext".into(),
        ref_: "main".into(),
    },
    trust_level: TrustLevel::Official,
    description: "Create, edit, and beautify DOCX/PPTX/XLSX/MD/PDF via host tools.".into(),
    homepage: Some("https://github.com/shannon-agent/shannon-documents-ext".into()),
    // ...
}
```

## 8. Phase A deliverables (3 weeks)

**Week 1 — Skeleton + detection**
- Day 1-2: Repo scaffold, marketplace.json, top-level SKILL.md
- Day 3-4: `host_requirements.rs` core module + Tauri command + tests
- Day 5: UI hook in Extensions Hub install dialog

**Week 2 — Templates + pandoc**
- Day 6-7: Five templates with placeholder contract
- Day 8-9: `create-doc` skill end-to-end (MD → DOCX → PDF)
- Day 10: `convert-doc` skill (md↔docx↔pdf↔html)

**Week 3 — Polish + ship**
- Day 11-12: `beautify-doc` skill (reference-docx injection)
- Day 13: Snapshot tests for all five templates
- Day 14: README, install-hint translations (en + zh-CN), catalog PR
- Day 15: Buffer / smoke testing

## 9. Open decisions (need user input)

| # | Question | Default if no answer |
|---|----------|---------------------|
| 1 | Option A vs B for host-requirements detection? | **A** (core parser) |
| 2 | Should beautify-doc ship in Phase A, or defer to Phase B? | **Ship Phase A** — it's a headline feature |
| 3 | Do we support PPTX in Phase A? | **No** — Phase B per `project_documents_strategy.md` |
| 4 | Where does `ref.docx` live — extension repo or user dir? | **Extension repo** (`tests/fixtures/ref.docx`), user can override |
| 5 | Min pandoc version (2.19 vs 3.x)? | **2.19** — wider compat; revisit in Phase B |
| 6 | PDF engine: xelatex (heavy) or weasyprint (light)? | **Try xelatex, fall back to weasyprint** |
| 7 | Should detection auto-install missing tools? | **No** — only print install hints; user installs |

## 10. Out of scope (deferred)

- DOCX editing via python-docx (Phase B)
- PPTX generation (Phase B)
- XLSX (Phase C)
- Template marketplace (Phase C)
- WPS / Office Online integrations (post-C)
- InlinePreview component in Shannon Desktop core (Phase A uses file path only)

## 11. References

- Strategy memory: `project_documents_strategy.md` (2026-06-22 decision)
- Architecture principle: `feedback_extension_first_architecture.md`
- Skill installer source: `src/extensions/skill_installers.rs` (`MarketplacePluginInstaller`)
- Catalog type: `src/extensions/types.rs::CatalogEntry`
