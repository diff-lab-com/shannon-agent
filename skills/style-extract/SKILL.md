---
name: Style Extract
description: Extract reusable "brand style notes" markdown from an existing .pptx or .docx - theme fonts (majorFont/minorFont), the full theme color scheme (clrScheme hex palette), PPT slide-layout inventory or DOCX style-sheet essentials, and header/footer elements. Pure Python standard library (zipfile + xml.etree), read-only, with an honest manual-unzip fallback when no python3 exists. Extraction only - it does not restyle new files. 从已有 PPT/Word 提取品牌风格笔记（零依赖纯标准库方案）。
when_to_use: Use when the user hands you an existing .pptx or .docx (a corporate template or a previous file) and asks to extract its brand style, theme colors, fonts, layouts, or to make new decks/documents match the look of that file.
argument-hint: "[path-to-existing-pptx-or-docx]"
allowed-tools:
  - Bash
  - Read
  - Write
user-invocable: true
---

# Style Extract

Read a `.pptx` or `.docx` the user already has and produce **brand style
notes** on disk — a Markdown file with the theme fonts, the full color
palette as copy-paste-ready hex values, the layout/style inventory, and the
header/footer elements. The source file is opened read-only and is **never
modified**.

This skill is **extraction only**: it does not restyle newly generated files.
Applying an extracted theme to a fresh deck/document is future work; what the
notes *can* do today is steer later generation (Step 5).

## Step 1: Collect inputs

Ask the user (skip any question they already answered; if they said "just do
it", pick sensible defaults and state them):

1. **Source file** — the path to an existing `.pptx` or `.docx`. Verify it
   exists before anything else; legacy binary `.ppt`/`.doc` files are not
   supported (say so up front).
2. **What to capture** — default is all four sections: fonts, color palette,
   layouts/styles, header/footer. The user may ask for colors only.
3. **Output filename** — default
   `output/style-notes-<YYYYMMDD>-<short-slug>.md`.

If the user passed `${0}`, treat it as the source file path.

## Step 2: Probe the environment (choose the best available path)

Run these checks:

```bash
python3 --version
unzip -v 2>/dev/null | head -1
```

Branch rules:

- **python3 available** (the common case) → use the stdlib script in Step 3.
  It needs only `zipfile` + `xml.etree`, both built in.
- **No python3 but `unzip` exists** → extract the theme manually and compile
  the notes by hand:
  ```bash
  unzip -o template.pptx "ppt/theme/*" "ppt/slideLayouts/*" -d extracted/
  # or, for Word:
  unzip -o template.docx "word/theme/*" "word/styles.xml" "word/header*.xml" "word/footer*.xml" -d extracted/
  ```
  Then Read `extracted/ppt/theme/theme1.xml` (or
  `extracted/word/theme/theme1.xml`) with the Read tool, transcribe the
  `clrScheme` hex values and `majorFont`/`minorFont` typefaces into the notes,
  and say plainly that the notes were compiled via manual unzip.
- **Neither available** → ask the user to open the file in PowerPoint or Word
  (Design → Variants → Colors / Fonts) and paste the values; transcribe them
  into the notes format. Never invent hex values.

## Step 3: Extract with the stdlib script

Copy the script below into a scratch file (for example
`.shannon/tmp/extract_style.py`), then run
`python3 .shannon/tmp/extract_style.py <input.pptx|.docx> output/style-notes-<YYYYMMDD>-<slug>.md`.
No editing needed — the script takes paths as arguments.

```python
#!/usr/bin/env python3
"""Extract brand style notes (markdown) from a .pptx or .docx.

Pure Python standard library (zipfile + xml.etree). Read-only: the source
package is opened for reading and never modified.
Usage: python3 extract_style.py <file.pptx|file.docx> [output.md]
"""
import os
import re
import sys
import zipfile
import xml.etree.ElementTree as ET

HEX = re.compile(r"^[0-9A-Fa-f]{6}$")

# Suggested roles for the twelve theme color slots (display order matters).
SLOT_ROLES = [
    ("dk1", "dark 1 - body text"), ("lt1", "light 1 - background"),
    ("dk2", "dark 2 - headings"), ("lt2", "light 2 - alt background"),
    ("accent1", "accent 1 - primary"), ("accent2", "accent 2"),
    ("accent3", "accent 3"), ("accent4", "accent 4"),
    ("accent5", "accent 5"), ("accent6", "accent 6"),
    ("hlink", "hyperlink"), ("folHlink", "followed hyperlink"),
]


def local(tag):
    """Namespace-agnostic name: '{http://...}clrScheme' -> 'clrScheme'."""
    return tag.rsplit("}", 1)[-1]


def attr(elem, name):
    """Namespace-agnostic attribute lookup."""
    for k, v in elem.attrib.items():
        if k.rsplit("}", 1)[-1] == name:
            return v
    return ""


def hexv(elem):
    """First srgbClr val or sysClr lastClr under elem, as #RRGGBB, or ''."""
    for node in elem.iter():
        name = local(node.tag)
        if name == "srgbClr":
            v = attr(node, "val")
        elif name == "sysClr":
            v = attr(node, "lastClr")
        else:
            continue
        if HEX.match(v):
            return "#" + v.upper()
    return ""


def parse_theme(data):
    """Return (scheme_name, [(slot, hex)], {majorFont, minorFont})."""
    root = ET.fromstring(data)
    scheme, colors, fonts = "", [], {}
    for elem in root.iter():
        name = local(elem.tag)
        if name == "clrScheme":
            scheme = attr(elem, "name")
            for slot in elem:
                slot_name = local(slot.tag)
                v = hexv(slot)
                if slot_name and v:
                    colors.append((slot_name, v))
        elif name in ("majorFont", "minorFont"):
            for child in elem:
                if local(child.tag) == "latin":
                    fonts[name] = attr(child, "typeface") or "(not set)"
    return scheme, colors, fonts


def part_names(zf, pattern):
    return sorted(p for p in zf.namelist() if re.fullmatch(pattern, p))


def pptx_layouts(zf):
    """[(display name, part)] from ppt/slideLayouts/*.xml."""
    rows = []
    for part in part_names(zf, r"ppt/slideLayouts/slideLayout\d+\.xml"):
        try:
            root = ET.fromstring(zf.read(part))
        except ET.ParseError:
            rows.append((os.path.basename(part), part))
            continue
        label = os.path.basename(part)
        for elem in root.iter():
            if local(elem.tag) == "cSld" and attr(elem, "name"):
                label = attr(elem, "name")
                break
        rows.append((label, part))
    return rows


def pptx_master_hf(zf):
    """Header/footer placeholder flags per slide master (p:hf, if present)."""
    rows = []
    for part in part_names(zf, r"ppt/slideMasters/slideMaster\d+\.xml"):
        flags = "(no p:hf element - placeholders per slide)"
        try:
            root = ET.fromstring(zf.read(part))
        except ET.ParseError:
            flags = "(unparsable)"
        else:
            for elem in root.iter():
                if local(elem.tag) == "hf":
                    flags = ", ".join(
                        "%s=%s" % (k, attr(elem, k) or "-")
                        for k in ("hdr", "ftr", "dt", "sldNum")
                    )
                    break
        rows.append((part, flags))
    return rows


def docx_styles(zf, limit=30):
    """([(id, name, font, color)], total) for paragraph styles in word/styles.xml."""
    rows = []
    if "word/styles.xml" not in zf.namelist():
        return rows, 0
    try:
        root = ET.fromstring(zf.read("word/styles.xml"))
    except ET.ParseError:
        return rows, 0
    for style in root:
        if local(style.tag) != "style" or attr(style, "type") != "paragraph":
            continue
        name, font, color = "", "", ""
        for child in style:
            cname = local(child.tag)
            if cname == "name":
                name = attr(child, "val")
            elif cname == "rPr":
                for node in child.iter():
                    nname = local(node.tag)
                    if nname == "rFonts":
                        font = attr(node, "ascii") or attr(node, "hAnsi")
                    elif nname == "color" and HEX.match(attr(node, "val")):
                        color = "#" + attr(node, "val").upper()
        rows.append((attr(style, "styleId"), name, font or "-", color or "-"))
    return rows[:limit], len(rows)


def docx_hf_texts(zf, width=160):
    """[(part, first ~160 chars of text)] for word/header*.xml, word/footer*.xml."""
    rows = []
    for part in part_names(zf, r"word/(header|footer)\d+\.xml"):
        try:
            root = ET.fromstring(zf.read(part))
        except ET.ParseError:
            rows.append((part, "(unparsable)"))
            continue
        chunks = [n.text.strip() for n in root.iter()
                  if local(n.tag) == "t" and n.text and n.text.strip()]
        rows.append((part, " ".join(chunks)[:width] or "(empty)"))
    return rows


def md_escape(text):
    return str(text).replace("|", "\\|")


def main():
    if len(sys.argv) < 2:
        print("usage: extract_style.py <file.pptx|file.docx> [output.md]")
        return 2
    src = sys.argv[1]
    ext = os.path.splitext(src)[1].lower().lstrip(".")
    if ext not in ("pptx", "docx"):
        print("UNSUPPORTED %s: v1 reads .pptx and .docx only" % src)
        return 2
    out = sys.argv[2] if len(sys.argv) > 2 else "output/style-notes.md"
    parent = os.path.dirname(out)
    if parent:
        os.makedirs(parent, exist_ok=True)

    try:
        zf = zipfile.ZipFile(src)
    except (zipfile.BadZipFile, OSError) as exc:
        print("FAILED to open %s: %s" % (src, exc))
        return 1

    theme_part = {"pptx": "ppt/theme/theme1.xml", "docx": "word/theme/theme1.xml"}[ext]
    lines = ["# Style notes - %s" % os.path.basename(src), "",
             "Source: `%s` (read-only, never modified). Extracted by style-extract." % src, ""]

    # Theme fonts + color palette
    if theme_part in zf.namelist():
        try:
            scheme, colors, fonts = parse_theme(zf.read(theme_part))
        except ET.ParseError as exc:
            scheme, colors, fonts = "", [], {}
            lines += ["## Theme", "", "(theme part unparsable: %s)" % exc, ""]
        lines += ["## Theme fonts", "",
                  "| Slot | Typeface |", "|---|---|",
                  "| Major (headings) | %s |" % md_escape(fonts.get("majorFont", "(not found)")),
                  "| Minor (body) | %s |" % md_escape(fonts.get("minorFont", "(not found)")), ""]
        lines += ["## Color palette%s" % (" (scheme: %s)" % md_escape(scheme) if scheme else ""), "",
                  "| Slot | Hex | Suggested use |", "|---|---|---|"]
        roles = dict(SLOT_ROLES)
        for slot, v in colors:
            lines.append("| %s | %s | %s |" % (slot, v, roles.get(slot, "accent")))
        missing = [s for s, _ in SLOT_ROLES if s not in {c for c, _ in colors}]
        if missing:
            lines.append("")
            lines.append("Slots not found in the theme: %s." % ", ".join(missing))
        if colors:
            lines += ["", "Copy-paste palette: `%s`" % " ".join(v for _, v in colors)]
        lines.append("")
    else:
        lines += ["## Theme", "",
                  "Theme part `%s` not found in this package - fonts and "
                  "palette are not available." % theme_part, ""]

    # Layouts (pptx) or style essentials (docx)
    if ext == "pptx":
        layouts = pptx_layouts(zf)
        slides = part_names(zf, r"ppt/slides/slide\d+\.xml")
        lines += ["## Slide layouts (%d) - deck has %d slide(s)" % (len(layouts), len(slides)), "",
                  "| Layout | Part |", "|---|---|"]
        lines += ("| %s | `%s` |" % (md_escape(n), p) for n, p in layouts)
        lines += ["", "## Headers / footers (slide masters)", "",
                  "| Master | Placeholder flags |", "|---|---|"]
        lines += ("| `%s` | %s |" % (p, md_escape(f)) for p, f in pptx_master_hf(zf))
        lines.append("")
    else:
        rows, total = docx_styles(zf)
        title = ("## Style sheet essentials (%d of %d paragraph styles)"
                 % (len(rows), total)) if total > len(rows) else \
                ("## Style sheet essentials (%d paragraph styles)" % total)
        lines += [title, "",
                  "| Style ID | Name | Font | Color |", "|---|---|---|---|"]
        lines += ("| %s | %s | %s | %s |" % tuple(md_escape(c) for c in r) for r in rows)
        hf = docx_hf_texts(zf)
        lines += ["", "## Headers / footers", "", "| Part | Text (first 160 chars) |", "|---|---|"]
        lines += ("| `%s` | %s |" % (p, md_escape(t)) for p, t in hf)
        lines.append("")

    with open(out, "w", encoding="utf-8") as fh:
        fh.write("\n".join(lines) + "\n")
    zf.close()
    print("WROTE %s" % out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

## Step 4: Verify the artifact

Re-read the written notes with the Read tool and check:

- the **Theme fonts** and **Color palette** sections are present (or carry an
  explicit "not found" explanation);
- every palette value matches `#RRGGBB` (six hex digits);
- layout/style/header-footer rows exist for whatever the package actually
  contained, and anything missing is explained rather than silently absent;
- the source file still opens (`python3 -c "import zipfile,sys;
  zipfile.ZipFile(sys.argv[1])" <source>` should succeed) — proving it was
  never modified.

## Step 5: Put the notes to work

The notes are the input for later generation — say this when you deliver them:

- **`/ppt-outline`** — name the palette hexes and major/minor fonts in the
  deck request so slides use brand colors instead of defaults;
- **`/xlsx-table` and `write_xlsx`** — use accent hexes for header rows and
  the minor font for body styling where the tool supports it;
- **`/docx-report`** — pick heading colors (dk2/accent1) and body font
  (minorFont) for the generated document;
- **`/meeting-minutes`** output can be reformatted with the same palette.

Be honest about the limit: v1 extracts and advises; the generated files carry
these values as direct formatting, not as an injected theme. Restyling a new
file with the original `.thmx`/theme part is not implemented yet.

## Step 6: Report results honestly

Tell the user: the notes path, which sections were captured vs. not found,
any `sysClr` fallbacks (system colors resolved to their last-known hex), and
that the source file was not modified.

## Failure and retry rules

1. If the source file is missing, is not an OOXML zip (`BadZipFile`), or is a
   legacy binary `.ppt`/`.doc`, stop and say so — do not fabricate notes.
2. If the theme part is absent (some minimal files skip it), keep the notes
   with the explicit "not found" section and list what is missing.
3. If a layout/style part is unparsable, mark that row "(unparsable)" and
   continue with the rest — one bad part must not sink the extraction.
4. Retry at most **2** times. After the second failure, stop and report what
   failed and what you tried.
5. If Step 4 fails, do not ship the notes: fix or delete the broken file and
   say so. Never report success unless the verification passed.

## Not supported in v1

Honest scope — this skill extracts; it does **not** restyle. Not supported:
applying an extracted theme to newly generated files (future work, tracked
with the office Wave 3 template-system decision), writing `.thmx` files,
`.xlsx`/`.xltx` themes, per-slide or per-paragraph style overrides (only the
theme part plus layout/style names are read), images/logos in headers and
masters, `fmtScheme` fill/line/effect styles, and legacy binary `.ppt`/`.doc`
formats. Say so up front instead of promising "your template, applied".
