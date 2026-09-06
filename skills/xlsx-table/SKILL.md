---
name: XLSX Table
description: Generate a real Microsoft Excel (.xlsx) workbook on disk using only the Python standard library (zipfile + hand-written OOXML) - no openpyxl required. Probes the environment for richer libraries, verifies the artifact, and degrades honestly. 生成 Excel 表格（零依赖纯标准库方案）。
when_to_use: Use when the user asks for an Excel spreadsheet, an .xlsx table, or a data workbook deliverable.
argument-hint: "[table-topic-or-output-filename]"
allowed-tools:
  - Bash
  - Read
  - Write
user-invocable: true
---

# XLSX Table

Produce an actual `.xlsx` file on disk — a binary artifact the user can open in
Microsoft Excel. The core path below uses **only the Python standard library**
(`zipfile` + hand-written OOXML), so it works on any machine with `python3`,
even without `openpyxl`.

Follow the steps in order. Never claim the file exists unless the Step 5
verification passed.

## Step 1: Collect inputs

Ask the user (skip any question they already answered; if they said "just do
it", pick sensible defaults and state them):

1. **Columns** — which columns, in which order, and with what header names?
2. **Data source** — pasted rows, a CSV/JSON file in this repo (which path?
   read it with the Read tool first), or numbers you must compute first?
3. **Template choice** — pick one and say which:
   - `plain table` (default): header row + data rows;
   - `table + totals row`: same, plus a computed totals row at the bottom
     (compute the totals yourself and write them as literal numbers);
   - `wide summary`: one row per entity, one column per metric.
4. **Sheet name** — default `Data`.
5. **Output filename** — default `output/<topic>-table-<YYYYMMDD>.xlsx`.

If the user passed `${0}`, treat it as the topic or output filename.

## Step 2: Probe the environment (choose the best available path)

Run these checks:

```bash
python3 --version
python3 -c "import openpyxl; print('openpyxl available')" 2>/dev/null
```

Branch rules:

- **openpyxl available** → prefer `openpyxl` (it supports formulas, multiple
  sheets, cell styling, and charts). Keep Steps 3–5 unchanged.
- **Not available** (the common case) → use the stdlib script in Step 4.
  It always works with bare `python3` (3.2+).

## Step 3: Choose the output path

- All artifacts go to the **`output/` directory at the project root** (create
  it if missing; the script below creates it for you).
- Default name: `output/<topic>-table-<YYYYMMDD>.xlsx`.
- Never write outside the project root unless the user gives an absolute path.

## Step 4: Generate the file

Copy the script below into a scratch file (for example `.shannon/tmp/make_xlsx.py`),
edit **only the `DATA SECTION`** with the collected data, then run
`python3 .shannon/tmp/make_xlsx.py output/<topic>-table-<YYYYMMDD>.xlsx`.

```python
#!/usr/bin/env python3
"""Generate a minimal but valid Microsoft Excel .xlsx workbook.

Pure Python standard library (zipfile + hand-written OOXML).
Usage: python3 make_xlsx.py [output.xlsx]
"""
import os
import sys
import zipfile
from xml.sax.saxutils import escape

# --- DATA SECTION: edit this, nothing below needs to change ---------------
SHEET_NAME = "Data"
# First row is the header row. Numbers stay numbers; everything else is text.
ROWS = [
    ["Region", "Revenue", "Growth"],
    ["North", 125000, 0.12],
    ["South", 98000, -0.03],
    ["Total", 223000, 0.09],
]
OUTPUT = "output/table.xlsx"
# --------------------------------------------------------------------------

CT_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"""

RELS_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"""

WORKBOOK_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="__SHEET_NAME__" sheetId="1" r:id="rId1"/></sheets></workbook>"""

WORKBOOK_RELS_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"""

SHEET_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>__ROWS__</sheetData></worksheet>"""


def col_letter(idx):
    """0 -> A, 1 -> B, ... 26 -> AA"""
    s = ""
    idx += 1
    while idx > 0:
        idx, rem = divmod(idx - 1, 26)
        s = chr(65 + rem) + s
    return s


def cell_xml(col, row, value):
    """Numbers become numeric cells; everything else becomes an inline string."""
    ref = "%s%d" % (col_letter(col), row)
    if isinstance(value, (int, float)) and not isinstance(value, bool):
        return '<c r="%s"><v>%s</v></c>' % (ref, value)
    return '<c r="%s" t="inlineStr"><is><t xml:space="preserve">%s</t></is></c>' % (
        ref, escape(str(value)))


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else OUTPUT
    parent = os.path.dirname(out)
    if parent:
        os.makedirs(parent, exist_ok=True)
    rows = []
    for r, row in enumerate(ROWS, start=1):
        cells = "".join(cell_xml(c, r, v) for c, v in enumerate(row))
        rows.append('<row r="%d">%s</row>' % (r, cells))
    sheet = SHEET_XML.replace("__ROWS__", "".join(rows))
    workbook = WORKBOOK_XML.replace("__SHEET_NAME__", escape(SHEET_NAME))
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", CT_XML)
        z.writestr("_rels/.rels", RELS_XML)
        z.writestr("xl/workbook.xml", workbook)
        z.writestr("xl/_rels/workbook.xml.rels", WORKBOOK_RELS_XML)
        z.writestr("xl/worksheets/sheet1.xml", sheet)
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
assert "xl/workbook.xml" in z.namelist(), "missing workbook part"
for n in z.namelist():
    if n.endswith((".xml", ".rels")):
        xml.dom.minidom.parseString(z.read(n))
print("VERIFIED", p)
' output/<topic>-table-<YYYYMMDD>.xlsx
```

A cheap extra signal: `file output/<topic>-table-<YYYYMMDD>.xlsx` should print
`Microsoft Excel 2007+`.

## Step 6: Report results honestly

Tell the user: the output path, the row/column count, which path was taken
(stdlib / openpyxl), and the v1 limitations below that matter for their table.

## Failure and retry rules

1. If the script exits non-zero, read stderr. Ragged rows (fewer cells in one
   row) are fine, but a non-numeric value in a numeric column is the usual
   mistake — fix the DATA SECTION and re-run.
2. Text with `<`, `>`, `&` is safe: the script escapes it automatically.
3. Retry at most **2** times. After the second failure, stop and report what
   failed and what you tried.
4. If Step 5 fails, do not ship the file: delete the broken artifact and say so.
5. Never report success unless the verification printed `VERIFIED`.

## Not supported in v1

Honest scope — this skill produces simple, valid single-sheet workbooks. It
does **not** support: formulas (values are written as literals), multiple
sheets, cell formatting/styles, merged cells, charts, freeze panes, filters,
data validation, or shared-string tables (inline strings are used instead).
If the user needs any of these, say so up front and route to `openpyxl` when
the environment has it.
