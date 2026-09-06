---
name: DOCX Report
description: Generate a real Microsoft Word (.docx) report on disk using only the Python standard library (zipfile + hand-written OOXML) - no python-docx or pandoc required. Probes the environment for richer libraries, verifies the artifact, and degrades honestly. 生成 Word 研报文档（零依赖纯标准库方案）。
when_to_use: Use when the user asks for a Word document, a .docx report, a research note, or any deliverable that must open in Microsoft Word.
argument-hint: "[report-topic-or-output-filename]"
allowed-tools:
  - Bash
  - Read
  - Write
user-invocable: true
---

# DOCX Report

Produce an actual `.docx` file on disk — a binary artifact the user can open in
Microsoft Word, not a description of one. The core path below uses **only the
Python standard library** (`zipfile` + hand-written OOXML), so it works on any
machine with `python3`, even without `python-docx` or `pandoc`.

Follow the steps in order. Never claim the file exists unless the Step 5
verification passed.

## Step 1: Collect inputs

Ask the user (skip any question they already answered; if they said "just do
it", pick sensible defaults and state them):

1. **Topic and audience** — what is the report about, and who reads it?
2. **Template choice** — pick one and say which:
   - `title + sections` (default): a title, then headed sections;
   - `summary + findings + recommendations`: three fixed `h1` sections;
   - `memo`: title, one `p` block, no headings.
3. **Data source** — pasted text, files in this repo (which paths?), or numbers
   you must compute first. Read source files with the Read tool before writing.
4. **Language** of the document (English / 中文 / ...).
5. **Output filename** — default `output/report-<YYYYMMDD>.docx`.

If the user passed `${0}`, treat it as the topic or output filename.

## Step 2: Probe the environment (choose the best available path)

Run these checks:

```bash
python3 --version
python3 -c "import docx; print('python-docx available')" 2>/dev/null
pandoc --version 2>/dev/null | head -1
```

Branch rules:

- **python-docx available** → prefer `python-docx` for the body generation
  (it supports styles, tables, and images). Keep Steps 3–5 unchanged.
- **pandoc available AND the content is already Markdown** → the fastest path
  is `pandoc <input>.md -o output/report.docx`. Keep Steps 3–5 unchanged.
- **Neither available** (the common case) → use the stdlib script in Step 4.
  It always works with bare `python3` (3.2+).

## Step 3: Choose the output path

- All artifacts go to the **`output/` directory at the project root** (create
  it if missing; the script below creates it for you).
- Default name: `output/report-<YYYYMMDD>.docx`.
- Never write outside the project root unless the user gives an absolute path.

## Step 4: Generate the file

Copy the script below into a scratch file (for example `.shannon/tmp/make_docx.py`),
edit **only the `DATA SECTION`** with the collected content, then run
`python3 .shannon/tmp/make_docx.py output/report-<YYYYMMDD>.docx`.

```python
#!/usr/bin/env python3
"""Generate a minimal but valid Microsoft Word .docx file.

Pure Python standard library (zipfile + hand-written OOXML).
Usage: python3 make_docx.py [output.docx]
"""
import os
import sys
import zipfile
from xml.sax.saxutils import escape

# --- DATA SECTION: edit this, nothing below needs to change ---------------
# Each block is ("title" | "h1" | "h2" | "p", text).
BLOCKS = [
    ("title", "Quarterly Report"),
    ("p", "Prepared by Shannon - 2026-09-05"),
    ("h1", "1. Executive Summary"),
    ("p", "Revenue grew 12% quarter over quarter."),
    ("h2", "1.1 Highlights"),
    ("p", "Server costs dropped after the migration."),
]
OUTPUT = "output/report.docx"
# --------------------------------------------------------------------------

CT_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"""

RELS_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"""

DOCUMENT_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>__BLOCKS__<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr></w:body></w:document>"""


def block_xml(kind, text):
    """Build one <w:p> run. Text is XML-escaped, so any content is safe."""
    t = escape(text)
    if kind == "title":
        return ('<w:p><w:pPr><w:spacing w:after="240"/></w:pPr><w:r><w:rPr><w:b/>'
                '<w:sz w:val="48"/><w:szCs w:val="48"/></w:rPr>'
                '<w:t xml:space="preserve">%s</w:t></w:r></w:p>' % t)
    if kind == "h1":
        return ('<w:p><w:pPr><w:spacing w:before="240" w:after="120"/></w:pPr><w:r><w:rPr><w:b/>'
                '<w:sz w:val="32"/><w:szCs w:val="32"/></w:rPr>'
                '<w:t xml:space="preserve">%s</w:t></w:r></w:p>' % t)
    if kind == "h2":
        return ('<w:p><w:pPr><w:spacing w:before="180" w:after="90"/></w:pPr><w:r><w:rPr><w:b/>'
                '<w:sz w:val="26"/><w:szCs w:val="26"/></w:rPr>'
                '<w:t xml:space="preserve">%s</w:t></w:r></w:p>' % t)
    return '<w:p><w:r><w:t xml:space="preserve">%s</w:t></w:r></w:p>' % t


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else OUTPUT
    parent = os.path.dirname(out)
    if parent:
        os.makedirs(parent, exist_ok=True)
    body = "".join(block_xml(kind, text) for kind, text in BLOCKS)
    doc = DOCUMENT_XML.replace("__BLOCKS__", body)
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", CT_XML)
        z.writestr("_rels/.rels", RELS_XML)
        z.writestr("word/document.xml", doc)
    print("wrote %s" % out)


if __name__ == "__main__":
    main()
```

## Step 5: Verify the artifact

Run the re-open check (re-unzips the package and parses every XML part, so a
truncated or malformed file is caught here, not by the user):

```bash
python3 -c '
import sys, zipfile, xml.dom.minidom
p = sys.argv[1]
z = zipfile.ZipFile(p)
assert z.testzip() is None, "corrupt zip member"
assert "word/document.xml" in z.namelist(), "missing document part"
for n in z.namelist():
    if n.endswith((".xml", ".rels")):
        xml.dom.minidom.parseString(z.read(n))
print("VERIFIED", p)
' output/report-<YYYYMMDD>.docx
```

A cheap extra signal: `file output/report-<YYYYMMDD>.docx` should print
`Microsoft Word 2007+`.

## Step 6: Report results honestly

Tell the user: the output path, the file size, which path was taken
(stdlib / python-docx / pandoc), and the v1 limitations below that matter for
their document.

## Failure and retry rules

1. If the script exits non-zero, read stderr. Data-shape mistakes (wrong tuple
   kind, missing comma) are the usual cause — fix the DATA SECTION and re-run.
2. Text with `<`, `>`, `&` is safe: the script escapes it automatically.
3. Retry at most **2** times. After the second failure, stop and report what
   failed and what you tried.
4. If Step 5 fails, do not ship the file: delete the broken artifact and say so.
5. Never report success unless the verification printed `VERIFIED`.

## Not supported in v1

Honest scope — this skill produces simple, valid documents. It does **not**
support: tables, images, charts, page headers/footers, table of contents,
named styles or style inheritance (headings are direct formatting), bullet or
numbered lists (rendered as plain paragraphs), footnotes, comments, or tracked
changes. If the user needs any of these, say so up front and route to
`python-docx` or `pandoc` when the environment has them.
