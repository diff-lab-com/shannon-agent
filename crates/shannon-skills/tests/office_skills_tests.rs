//! Tests for the bundled office skills (P2-3): `docx-report`, `xlsx-table`,
//! `ppt-outline`.
//!
//! Coverage:
//! - bundled registration, id uniqueness, and frontmatter validity
//! - on-disk `skills/*/SKILL.md` parse via the same loader frontmatter parser
//! - static well-formedness of every OOXML XML document template embedded in
//!   the runbook scripts (dependency-free mini checker, no new crates)
//! - python3-gated smoke test: actually runs each runbook script and verifies
//!   the produced package. Skipped (with a note) when `python3` is missing.

use shannon_skills::bundled::{BundledSkills, init_bundled_skills};
use shannon_skills::definition::Skill;
use shannon_skills::frontmatter::parse_skill_frontmatter;

const DOCX_MD: &str = include_str!("../../../skills/docx-report/SKILL.md");
const XLSX_MD: &str = include_str!("../../../skills/xlsx-table/SKILL.md");
const PPT_MD: &str = include_str!("../../../skills/ppt-outline/SKILL.md");

const OFFICE_SKILLS: [(&str, &str); 3] = [
    ("docx-report", DOCX_MD),
    ("xlsx-table", XLSX_MD),
    ("ppt-outline", PPT_MD),
];

fn bundled_registry() -> BundledSkills {
    let registry = BundledSkills::new();
    init_bundled_skills(&registry).expect("bundled skills must initialize");
    registry
}

fn office_skill(id: &str) -> Skill {
    let registry = bundled_registry();
    registry
        .list()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("office skill {id} must be registered"))
}

/// Extract fenced ```python code blocks from markdown.
fn fenced_python_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut lines = markdown.lines();
    while let Some(line) = lines.next() {
        if line.trim_start().starts_with("```python") {
            let mut body = Vec::new();
            for l in lines.by_ref() {
                if l.trim_start().starts_with("```") {
                    break;
                }
                body.push(l);
            }
            blocks.push(body.join("\n"));
        }
    }
    blocks
}

/// Extract every `r"""<?xml ..."""` triple-quoted XML document from a Python
/// script. Only complete documents (starting with the XML declaration) are
/// matched, so partial fragments (e.g. paragraph templates) are not checked.
fn xml_document_constants(python_src: &str) -> Vec<String> {
    let re = regex::Regex::new(r#"(?s)"""(\s*<\?xml.*?)""""#).expect("static regex");
    re.captures_iter(python_src)
        .map(|c| c[1].to_string())
        .collect()
}

fn find_seq(chars: &[char], from: usize, pat: &[char]) -> Option<usize> {
    if chars.len() < pat.len() {
        return None;
    }
    (from..=chars.len() - pat.len()).find(|&i| &chars[i..i + pat.len()] == pat)
}

/// Dependency-free XML well-formedness checker for the static templates:
/// single root element, balanced start/end tags, quoted attributes, closed
/// comments and processing instructions. Not a schema validator - it asserts
/// the same class of guarantee as `xml.dom.minidom.parseString`.
fn check_xml_well_formed(xml: &str) -> Result<(), String> {
    fn name_char(c: char) -> bool {
        c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':')
    }

    let chars: Vec<char> = xml.chars().collect();
    let mut i = 0usize;
    let mut stack: Vec<String> = Vec::new();
    let mut root_seen = false;
    let mut root_closed = false;

    while i < chars.len() {
        if chars[i] != '<' {
            if root_closed && !chars[i].is_whitespace() {
                return Err(format!("text after root element at offset {i}"));
            }
            i += 1;
            continue;
        }
        i += 1;
        match chars.get(i) {
            None => return Err("unterminated '<'".to_string()),
            Some('?') => {
                let end = find_seq(&chars, i, &['?', '>'])
                    .ok_or("unterminated processing instruction")?;
                i = end + 2;
            }
            Some('!') => {
                if chars.get(i + 1) == Some(&'-') && chars.get(i + 2) == Some(&'-') {
                    let end = find_seq(&chars, i + 2, &['-', '-']).ok_or("unterminated comment")?;
                    i = end + 2;
                } else {
                    return Err("DOCTYPE/CDATA are not used in these templates".to_string());
                }
            }
            Some('/') => {
                i += 1;
                let start = i;
                while i < chars.len() && name_char(chars[i]) {
                    i += 1;
                }
                if chars.get(i) != Some(&'>') {
                    return Err("malformed closing tag".to_string());
                }
                let name: String = chars[start..i].iter().collect();
                match stack.pop() {
                    Some(open) if open == name => {}
                    Some(open) => {
                        return Err(format!(
                            "mismatched closing tag </{name}>, open element is <{open}>"
                        ));
                    }
                    None => return Err(format!("closing tag </{name}> without open element")),
                }
                i += 1;
                if stack.is_empty() {
                    root_closed = true;
                }
            }
            Some(_) => {
                if root_closed {
                    return Err("second root element".to_string());
                }
                let start = i;
                while i < chars.len() && name_char(chars[i]) {
                    i += 1;
                }
                if i == start {
                    return Err("element without a name".to_string());
                }
                let name: String = chars[start..i].iter().collect();
                let mut self_closing = false;
                loop {
                    while i < chars.len() && chars[i].is_whitespace() {
                        i += 1;
                    }
                    match chars.get(i) {
                        None => return Err(format!("unterminated <{name}>")),
                        Some('>') => {
                            i += 1;
                            break;
                        }
                        Some('/') => {
                            self_closing = true;
                            i += 1;
                            if chars.get(i) != Some(&'>') {
                                return Err(format!("malformed self-closing <{name}/>"));
                            }
                            i += 1;
                            break;
                        }
                        Some(_) => {
                            let astart = i;
                            while i < chars.len()
                                && chars[i] != '='
                                && chars[i] != '>'
                                && !chars[i].is_whitespace()
                            {
                                i += 1;
                            }
                            if i == astart {
                                return Err(format!("bad attribute in <{name}> at offset {i}"));
                            }
                            while i < chars.len() && chars[i].is_whitespace() {
                                i += 1;
                            }
                            if chars.get(i) != Some(&'=') {
                                return Err(format!("attribute without '=' in <{name}>"));
                            }
                            i += 1;
                            while i < chars.len() && chars[i].is_whitespace() {
                                i += 1;
                            }
                            let quote = match chars.get(i) {
                                Some(&q) if q == '"' || q == '\'' => q,
                                _ => {
                                    return Err(format!("attribute value not quoted in <{name}>"));
                                }
                            };
                            i += 1;
                            while i < chars.len() && chars[i] != quote {
                                if chars[i] == '<' {
                                    return Err(format!("'<' inside attribute value in <{name}>"));
                                }
                                i += 1;
                            }
                            if i >= chars.len() {
                                return Err(format!("unterminated attribute value in <{name}>"));
                            }
                            i += 1;
                        }
                    }
                }
                if !self_closing {
                    stack.push(name);
                }
                root_seen = true;
            }
        }
    }
    if !root_seen {
        return Err("no root element".to_string());
    }
    if let Some(open) = stack.last() {
        return Err(format!("unclosed element <{open}>"));
    }
    Ok(())
}

#[test]
fn test_office_skills_registered_with_unique_ids() {
    let registry = bundled_registry();
    // 5 core bundled skills + 3 office skills
    assert_eq!(registry.len(), 8);
    let skills = registry.list();
    let mut ids: Vec<&str> = skills.iter().map(|s| s.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 8, "skill ids must be unique");
    for (id, _) in OFFICE_SKILLS {
        assert!(ids.contains(&id), "missing office skill {id}");
    }
}

#[test]
fn test_office_skills_allowed_tools_and_flags() {
    let expected_tools = ["Bash".to_string(), "Read".to_string(), "Write".to_string()];
    for (id, _) in OFFICE_SKILLS {
        let skill = office_skill(id);
        assert_eq!(skill.allowed_tools, expected_tools, "{id}");
        assert!(skill.user_invocable, "{id} must be user-invocable");
        assert_eq!(
            skill.source,
            shannon_skills::definition::SkillSource::Bundled
        );
        assert!(!skill.content.is_empty(), "{id} must have a runbook body");
    }
}

#[test]
fn test_disk_skill_files_parse_with_loader_frontmatter() {
    for (id, raw) in OFFICE_SKILLS {
        let parsed = parse_skill_frontmatter(raw, id)
            .unwrap_or_else(|e| panic!("{id}: frontmatter must parse: {e}"));
        assert!(parsed.frontmatter.name.is_some(), "{id}: name");
        assert!(
            parsed.frontmatter.description.is_some(),
            "{id}: description"
        );
        assert!(
            parsed.frontmatter.when_to_use.is_some(),
            "{id}: when_to_use"
        );
        assert!(
            parsed.frontmatter.argument_hint.is_some(),
            "{id}: argument-hint"
        );
        assert_eq!(
            parsed.frontmatter.allowed_tools,
            Some(vec![
                "Bash".to_string(),
                "Read".to_string(),
                "Write".to_string()
            ]),
            "{id}: allowed-tools"
        );
        assert_eq!(parsed.frontmatter.user_invocable, Some(true), "{id}");
        // Body must not leak frontmatter markers.
        assert!(!parsed.body.starts_with("---"), "{id}: body");
        // Bundled registration must mirror the parsed frontmatter.
        let bundled = office_skill(id);
        assert_eq!(bundled.name, parsed.frontmatter.name.unwrap(), "{id}");
        assert_eq!(bundled.allowed_tools.len(), 3, "{id}");
    }
}

#[test]
fn test_descriptions_carry_the_one_chinese_line() {
    for (id, _) in OFFICE_SKILLS {
        let skill = office_skill(id);
        assert!(
            skill
                .description
                .chars()
                .any(|c| ('\u{4E00}'..='\u{9FFF}').contains(&c)),
            "{id}: description should carry one Chinese summary line"
        );
    }
}

#[test]
fn test_runbooks_have_required_sections() {
    for (id, raw) in OFFICE_SKILLS {
        for marker in [
            "Collect inputs",
            "output/",
            "Verify the artifact",
            "Failure and retry rules",
            "Not supported in v1",
        ] {
            assert!(raw.contains(marker), "{id}: runbook missing `{marker}`");
        }
    }
    // ppt-outline has the explicit degradation contract.
    assert!(PPT_MD.contains("degrade"), "ppt-outline: degradation rules");
}

#[test]
fn test_ooxml_xml_templates_are_well_formed() {
    // Minimum number of complete XML documents per script.
    let expected_docs: [(&str, usize); 3] =
        [("docx-report", 3), ("xlsx-table", 5), ("ppt-outline", 11)];

    for (id, min_docs) in expected_docs {
        let raw = OFFICE_SKILLS.iter().find(|(k, _)| *k == id).unwrap().1;
        let blocks = fenced_python_blocks(raw);
        assert_eq!(
            blocks.len(),
            1,
            "{id}: runbook must contain exactly one ```python script block"
        );
        let script = &blocks[0];
        assert!(script.contains("import zipfile"), "{id}: stdlib zipfile");
        assert!(
            script.contains("[Content_Types].xml"),
            "{id}: OOXML package parts"
        );

        let docs = xml_document_constants(script);
        assert!(
            docs.len() >= min_docs,
            "{id}: expected >= {min_docs} XML documents, found {}",
            docs.len()
        );
        for doc in &docs {
            check_xml_well_formed(doc)
                .unwrap_or_else(|e| panic!("{id}: XML template not well-formed: {e}\n{doc}"));
        }
    }
}

#[test]
fn test_xml_checker_rejects_broken_documents() {
    // Sanity-check the checker itself so the assertions above mean something.
    assert!(check_xml_well_formed("<a><b/></a>").is_ok());
    assert!(check_xml_well_formed("<?xml version=\"1.0\"?><a x=\"1\">t</a>").is_ok());
    assert!(check_xml_well_formed("<!-- c --><a/>").is_ok());
    assert!(
        check_xml_well_formed("<a><b></a></b>").is_err(),
        "misnested"
    );
    assert!(check_xml_well_formed("<a></b>").is_err(), "stray close");
    assert!(check_xml_well_formed("<a>").is_err(), "unclosed");
    assert!(check_xml_well_formed("<a x=1/>").is_err(), "unquoted attr");
    assert!(check_xml_well_formed("<a></a><b/>").is_err(), "two roots");
    assert!(check_xml_well_formed("<a attr=nope/>").is_err());
}

#[test]
fn test_python3_smoke_generates_valid_office_files() {
    // Env-gated: skip (with a note) when python3 is not on PATH.
    let probe = std::process::Command::new("python3")
        .arg("--version")
        .output();
    if probe.is_err() || !probe.as_ref().unwrap().status.success() {
        eprintln!("SKIPPED office skill smoke test: python3 unavailable");
        return;
    }

    // (skill id, script fence source, output extension, required package part)
    let cases: [(&str, &str, &str); 3] = [
        ("docx-report", DOCX_MD, "docx"),
        ("xlsx-table", XLSX_MD, "xlsx"),
        ("ppt-outline", PPT_MD, "pptx"),
    ];

    let validate = r#"
import sys, zipfile, xml.dom.minidom
p, required = sys.argv[1], sys.argv[2]
z = zipfile.ZipFile(p)
assert z.testzip() is None, "corrupt zip member"
assert required in z.namelist(), "missing part: " + required
for n in z.namelist():
    if n.endswith((".xml", ".rels")):
        xml.dom.minidom.parseString(z.read(n))
print("VERIFIED", p)
"#;

    for (id, raw, ext) in cases {
        let blocks = fenced_python_blocks(raw);
        assert_eq!(blocks.len(), 1, "{id}");
        let tmp = tempfile::tempdir().expect("tempdir");
        let script_path = tmp.path().join("make.py");
        std::fs::write(&script_path, blocks[0].as_bytes()).expect("write script");
        let out_path = tmp.path().join(format!("smoke.{ext}"));

        let run = std::process::Command::new("python3")
            .arg(&script_path)
            .arg(&out_path)
            .current_dir(tmp.path())
            .output()
            .expect("spawn python3 (probed above)");
        assert!(
            run.status.success(),
            "{id}: script failed: {}",
            String::from_utf8_lossy(&run.stderr)
        );
        assert!(out_path.exists(), "{id}: no output file produced");

        let check = std::process::Command::new("python3")
            .arg("-c")
            .arg(validate)
            .arg(&out_path)
            .arg(required_part(ext))
            .output()
            .expect("spawn python3 validator");
        assert!(
            check.status.success(),
            "{id}: package validation failed: {}",
            String::from_utf8_lossy(&check.stderr)
        );
        assert!(
            String::from_utf8_lossy(&check.stdout).contains("VERIFIED"),
            "{id}: validator did not confirm the package"
        );
    }
}

fn required_part(ext: &str) -> &'static str {
    match ext {
        "docx" => "word/document.xml",
        "xlsx" => "xl/workbook.xml",
        "pptx" => "ppt/slides/slide1.xml",
        other => panic!("unexpected extension {other}"),
    }
}
