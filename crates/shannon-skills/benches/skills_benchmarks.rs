//! Performance benchmarks for shannon-skills
//!
//! Benchmarks cover:
//! - load_skills_from_directory for 10, 50, 200 skill files
//! - Frontmatter parsing throughput

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use shannon_skills::definition::SkillSource;
use shannon_skills::frontmatter::parse_skill_frontmatter;
use shannon_skills::loader::load_skills_from_directory;
use std::fs;

// ============================================================================
// Helper: create a temp directory with N skill subdirectories
// ============================================================================

fn create_skill_tree(dir: &std::path::Path, count: usize) -> tempfile::TempDir {
    let tmp = tempfile::tempdir_in(dir).expect("create temp dir");
    for i in 0..count {
        let skill_dir = tmp.path().join(format!("skill-{i:04}"));
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(
            skill_dir.join("SKILL.md"),
            format!(
                r#"---
name: skill-{i:04}
description: Benchmark skill number {i}
alias:
  - s{i}
allowed-tools:
  - bash
  - read
argument-hint: "<files>"
---

# Skill {i:04}

This is the body of skill {i}. It contains instructions for benchmarking
the skill loading system. Multiple lines of content to simulate realistic
skill definitions.

## Usage

Run with `/{i:04}` command.

## Examples

```
/{i:04} src/main.rs
```
"#
            ),
        )
        .expect("write SKILL.md");
    }
    tmp
}

// ============================================================================
// load_skills_from_directory benchmarks
// ============================================================================

fn bench_load_skills_from_directory(c: &mut Criterion) {
    let mut group = c.benchmark_group("load_skills_from_directory");

    let bench_base = tempfile::tempdir().expect("create bench base dir");

    for &count in &[10usize, 50, 200] {
        let skill_tree = create_skill_tree(bench_base.path(), count);

        group.bench_with_input(BenchmarkId::new("skills", count), &count, |b, _| {
            b.iter(|| {
                let skills =
                    load_skills_from_directory(black_box(skill_tree.path()), SkillSource::User);
                let _ = black_box(skills);
            })
        });
    }

    group.finish();
}

// ============================================================================
// Frontmatter parsing throughput benchmarks
// ============================================================================

fn generate_skill_content(index: usize) -> String {
    format!(
        r#"---
name: skill-{index}
description: A benchmark skill for throughput testing
alias:
  - s{index}
  - bench{index}
allowed-tools:
  - bash
  - read
  - edit
  - write
argument-hint: "<files> <options>"
when_to_use: Use when benchmarking skill parsing
model: claude-sonnet-4-20250514
user-invocable: true
context: inline
agent: executor
version: "1.0"
---

# Skill {index}

This is a realistic skill body with multiple sections.

## Overview

Skill {index} is designed for benchmarking the frontmatter parser.
It contains typical markdown content that would be found in a real skill.

## Instructions

1. Read the target files
2. Analyze the content
3. Apply transformations
4. Write the results

## Examples

```bash
/skill-{index} src/lib.rs
```

## Notes

- This skill supports multiple file inputs
- It respects .gitignore patterns
- Output is formatted as markdown
"#
    )
}

fn bench_frontmatter_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("frontmatter_parsing");

    let content = generate_skill_content(0);
    group.bench_function("single_parse", |b| {
        b.iter(|| {
            let _ = parse_skill_frontmatter(black_box(&content), black_box("bench-skill"));
        })
    });

    for &batch_size in &[10usize, 50, 200] {
        let contents: Vec<String> = (0..batch_size).map(generate_skill_content).collect();

        group.bench_with_input(
            BenchmarkId::new("batch_parse", batch_size),
            &batch_size,
            |b, _| {
                b.iter(|| {
                    for (i, content) in contents.iter().enumerate() {
                        let source = format!("skill-{i}");
                        let _ = parse_skill_frontmatter(black_box(content), black_box(&source));
                    }
                })
            },
        );
    }

    let fm_only = "---\nname: minimal\ndescription: Minimal skill\n---\n";
    group.bench_function("minimal_frontmatter", |b| {
        b.iter(|| {
            let _ = parse_skill_frontmatter(black_box(fm_only), black_box("minimal"));
        })
    });

    let body_only = "# Just a Body\n\nSome instructions without any frontmatter.\n";
    group.bench_function("no_frontmatter", |b| {
        b.iter(|| {
            let _ = parse_skill_frontmatter(black_box(body_only), black_box("body-only"));
        })
    });

    let large_fm = r#"---
name: complex-skill
description: A skill with every possible frontmatter field
alias:
  - cs
  - complex
  - cskill
allowed-tools:
  - bash
  - read
  - edit
  - write
  - glob
  - grep
argument-hint: "<files> <options> [--verbose]"
when_to_use: Use for complex multi-step operations
model: claude-opus
disable-model-invocation: false
user-invocable: true
context: fork
agent: architect
paths:
  - "src/**/*.rs"
  - "tests/**/*.rs"
version: "2.3.1"
---

# Complex Skill

Very detailed skill body with many paragraphs.

## Section 1

Detailed instructions here.

## Section 2

More instructions and examples.

## Section 3

Edge cases and error handling.
"#;
    group.bench_function("large_frontmatter", |b| {
        b.iter(|| {
            let _ = parse_skill_frontmatter(black_box(large_fm), black_box("complex-skill"));
        })
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_load_skills_from_directory,
    bench_frontmatter_parsing
);
criterion_main!(benches);
