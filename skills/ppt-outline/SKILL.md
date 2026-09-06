---
name: PPT Outline
description: Build a presentation outline as Markdown, then optionally render a minimal but valid .pptx deck with pure-stdlib OOXML (plain text-box slides). Explicit degradation rules - if the environment cannot produce a PPTX, deliver the Markdown outline and say so. 生成 PPT 大纲，可选零依赖极简 PPTX，明确降级规则。
when_to_use: Use when the user asks for slides, a deck, a presentation outline, or a .pptx deliverable.
argument-hint: "[deck-topic-or-output-filename]"
allowed-tools:
  - Bash
  - Read
  - Write
user-invocable: true
---

# PPT Outline

Two deliverables, in priority order:

1. **Markdown outline** (always produced, never skipped).
2. **Minimal `.pptx` deck** (best effort) — only if the environment can render
   it. If it cannot, degrade to deliverable 1 and say so plainly.

The PPTX path uses **only the Python standard library** (`zipfile` +
hand-written OOXML): one plain text-box layout, title + bullets per slide.

## Step 1: Collect inputs

Ask the user (skip any question they already answered; if they said "just do
it", pick sensible defaults and state them):

1. **Topic, audience, and duration** — e.g. a 10-minute review vs a 45-minute
   lecture (drives slide count; default ~1 slide per 2-3 minutes).
2. **Template choice** — pick one and say which:
   - `narrative` (default): context → findings → recommendation → next steps;
   - `status review`: done / in progress / blocked / risks;
   - `pitch`: problem → solution → proof → ask.
3. **Data source** — pasted notes, files in this repo (which paths?), or
   material you must gather first. Read sources with the Read tool.
4. **Slide count** and approximate bullets per slide (default 3-5).
5. **Output filenames** — defaults `output/<topic>-outline-<YYYYMMDD>.md` and
   `output/<topic>-deck-<YYYYMMDD>.pptx`.

If the user passed `${0}`, treat it as the topic or output filename.

## Step 2: Write the Markdown outline (deliverable 1 — always)

Write `output/<topic>-outline-<YYYYMMDD>.md` with the Write tool:

```markdown
# <Deck title>

## Slide 1 — <Title>
- bullet
- bullet
  - sub-bullet

## Slide 2 — <Title>
- ...
```

Rules: one `## Slide N — Title` heading per slide; top-level bullets are
level 0, indented (two spaces + `- `) bullets become level 1 sub-bullets;
3-5 bullets per slide; no empty slides. This file is a deliverable — it must
be complete and self-sufficient even if Step 3-5 never run.

## Step 3: Probe the environment (choose the best available path)

Run these checks:

```bash
python3 --version
python3 -c "import pptx; print('python-pptx available')" 2>/dev/null
pandoc --version 2>/dev/null | head -1
```

Branch rules:

- **python-pptx available** → prefer `python-pptx` (rich layouts, images,
  notes). Still verify the result in Step 5.
- **pandoc available** → `pandoc <outline>.md -t pptx -o <deck>.pptx` works
  if the outline maps to pandoc's slide model (headers = slides). Verify it.
- **Bare `python3` only** → use the stdlib script in Step 4.
- **No `python3` at all** → degrade now: deliverable is the Markdown outline
  only. Tell the user: "PPTX not generated: python3 unavailable."

## Step 4: Generate the PPTX (deliverable 2 — best effort)

Copy the script below into a scratch file (for example
`.shannon/tmp/make_pptx.py`), edit **only the `DATA SECTION`** so `SLIDES`
mirrors the Markdown outline, then run
`python3 .shannon/tmp/make_pptx.py output/<topic>-deck-<YYYYMMDD>.pptx`.

```python
#!/usr/bin/env python3
"""Generate a minimal but valid Microsoft PowerPoint .pptx deck.

Pure Python standard library (zipfile + hand-written OOXML).
One text-box slide per SLIDES entry (title + bullets).
Usage: python3 make_pptx.py [output.pptx]
"""
import os
import sys
import zipfile
from xml.sax.saxutils import escape

# --- DATA SECTION: edit this, nothing below needs to change ---------------
# One dict per slide. Bullets: plain strings are level 0; (text, level)
# tuples are indented sub-bullets.
SLIDES = [
    {
        "title": "Q3 Review",
        "bullets": [
            "Revenue grew 12% QoQ",
            "Server costs down after migration",
            ("Next: expand to EMEA", 1),
        ],
    },
    {
        "title": "Risks",
        "bullets": ["Churn in SMB segment", "Hiring pipeline is thin"],
    },
]
OUTPUT = "output/deck.pptx"
# --------------------------------------------------------------------------

CT_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/>__SLIDE_OVERRIDES__<Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/></Types>"""

RELS_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"""

PRESENTATION_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" saveSubsetFonts="1"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>__SLD_IDS__</p:sldIdLst><p:sldSz cx="9144000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"""

PRESENTATION_RELS_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/>__SLIDE_RELS__</Relationships>"""

SLIDE_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/><p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="838200" y="365125"/><a:ext cx="7467625" cy="1325563"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr wrap="square" rtlCol="0"/><a:lstStyle/><a:p><a:r><a:rPr lang="en-US" sz="3600" b="1" dirty="0"/><a:t>__TITLE__</a:t></a:r></a:p></p:txBody></p:sp><p:sp><p:nvSpPr><p:cNvPr id="3" name="Content"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="838200" y="1825625"/><a:ext cx="7467625" cy="4351338"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr wrap="square" rtlCol="0"/><a:lstStyle/>__PARAS__</p:txBody></p:sp></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"""

SLIDE_RELS_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"""

SLIDE_MASTER_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"""

SLIDE_MASTER_RELS_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/></Relationships>"""

SLIDE_LAYOUT_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="obj" preserve="1"><p:cSld name="Title and Content"><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"""

SLIDE_LAYOUT_RELS_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"""

THEME_XML = r"""<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Shannon Minimal"><a:themeElements><a:clrScheme name="Minimal"><a:dk1><a:srgbClr val="000000"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink></a:clrScheme><a:fontScheme name="Minimal"><a:majorFont><a:latin typeface="Calibri Light"/><a:ea typeface=""/><a:cs typeface=""/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface=""/></a:minorFont></a:fontScheme><a:fmtScheme name="Minimal"><a:fillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:fillStyleLst><a:lnStyleLst><a:ln w="6350" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash solid="solid"/><a:miter lim="800000"/></a:ln><a:ln w="12700" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash solid="solid"/><a:miter lim="800000"/></a:ln><a:ln w="19050" cap="flat" cmpd="sng" algn="ctr"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:prstDash solid="solid"/><a:miter lim="800000"/></a:ln></a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>"""


def bullet_para(item):
    """One <a:p>. Plain strings are level 0; (text, level) tuples indent."""
    if isinstance(item, tuple):
        text, level = item
    else:
        text, level = item, 0
    return ('<a:p><a:pPr lvl="%d"/><a:r><a:rPr lang="en-US" sz="2000" dirty="0"/>'
            '<a:t>%s</a:t></a:r></a:p>' % (level, escape(text)))


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else OUTPUT
    parent = os.path.dirname(out)
    if parent:
        os.makedirs(parent, exist_ok=True)
    n = len(SLIDES)
    if n < 1:
        raise SystemExit("SLIDES must contain at least one slide")

    overrides, slide_rels, sld_ids, slide_files = [], [], [], []
    for i in range(n):
        overrides.append('<Override PartName="/ppt/slides/slide%d.xml" '
                         'ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>' % (i + 1))
        slide_rels.append('<Relationship Id="rId%d" Type="http://schemas.openxmlformats.org/'
                          'officeDocument/2006/relationships/slide" Target="slides/slide%d.xml"/>' % (i + 2, i + 1))
        sld_ids.append('<p:sldId id="%d" r:id="rId%d"/>' % (256 + i, i + 2))
        slide_files.append(("ppt/slides/slide%d.xml" % (i + 1), SLIDE_XML
                            .replace("__TITLE__", escape(SLIDES[i]["title"]))
                            .replace("__PARAS__", "".join(bullet_para(b) for b in SLIDES[i]["bullets"]))))

    ct = CT_XML.replace("__SLIDE_OVERRIDES__", "".join(overrides))
    presentation = PRESENTATION_XML.replace("__SLD_IDS__", "".join(sld_ids))
    pres_rels = PRESENTATION_RELS_XML.replace("__SLIDE_RELS__", "".join(slide_rels))

    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("[Content_Types].xml", ct)
        z.writestr("_rels/.rels", RELS_XML)
        z.writestr("ppt/presentation.xml", presentation)
        z.writestr("ppt/_rels/presentation.xml.rels", pres_rels)
        z.writestr("ppt/slideMasters/slideMaster1.xml", SLIDE_MASTER_XML)
        z.writestr("ppt/slideMasters/_rels/slideMaster1.xml.rels", SLIDE_MASTER_RELS_XML)
        z.writestr("ppt/slideLayouts/slideLayout1.xml", SLIDE_LAYOUT_XML)
        z.writestr("ppt/slideLayouts/_rels/slideLayout1.xml.rels", SLIDE_LAYOUT_RELS_XML)
        z.writestr("ppt/theme/theme1.xml", THEME_XML)
        for i, (name, xml_text) in enumerate(slide_files, start=1):
            z.writestr(name, xml_text)
            z.writestr("ppt/slides/_rels/slide%d.xml.rels" % i, SLIDE_RELS_XML)
    print("wrote %s (%d slides)" % (out, n))


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
names = z.namelist()
for required in ("ppt/presentation.xml", "ppt/theme/theme1.xml", "ppt/slides/slide1.xml"):
    assert required in names, "missing part: " + required
for n in names:
    if n.endswith((".xml", ".rels")):
        xml.dom.minidom.parseString(z.read(n))
print("VERIFIED", p)
' output/<topic>-deck-<YYYYMMDD>.pptx
```

A cheap extra signal: `file output/<topic>-deck-<YYYYMMDD>.pptx` should print
`Microsoft PowerPoint 2007+`.

## Step 6: Report results honestly (degradation rules)

- Both deliverables produced → report both paths and the slide count.
- PPTX attempted but failed after the retries below → deliverable is the
  **Markdown outline**; tell the user: `PPTX not generated: <reason>. The
  Markdown outline at <path> is the deliverable.` Delete any broken artifact.
- Environment could not attempt the PPTX (no python3) → same message with the
  reason `python3 unavailable`.
- Never fabricate a PPTX, never claim success without `VERIFIED`, and never
  leave the Markdown outline unwritten — it is the primary deliverable.

## Failure and retry rules

1. If the script exits non-zero, read stderr. Shape mistakes in `SLIDES`
   (missing `title`/`bullets` keys) are the usual cause — fix and re-run.
2. Text with `<`, `>`, `&` is safe: the script escapes it automatically.
3. Retry at most **2** times. After the second failure, degrade per Step 6.
4. If Step 5 fails, degrade per Step 6 and delete the broken artifact.

## Not supported in v1

Honest scope — the stdlib deck is deliberately minimal: plain text boxes on a
blank layout (no placeholder inheritance), no images, charts, or tables, no
speaker notes, no transitions or animations, no custom theme beyond the
built-in minimal one, and no per-slide layout variations. For any of these,
say so up front and route to `python-pptx` or `pandoc` when the environment
has them.
