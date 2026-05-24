//! Three-way merge and conflict detection for file editing.
//!
//! When the Edit tool's `old_string` is not found (because the file was changed
//! externally), this module provides a three-way merge fallback that attempts to
//! combine the external changes with the intended edit.
//!
//! ## Algorithm
//!
//! Line-based three-way merge using LCS (Longest Common Subsequence):
//! 1. Compute the diff between base and ours (current file on disk)
//! 2. Compute the diff between base and theirs (intended edit applied to base)
//! 3. Walk through the diff hunks:
//!    - Unchanged region → keep as-is
//!    - Changed only in ours → use ours
//!    - Changed only in theirs → use theirs
//!    - Same change on both sides → use either
//!    - Conflicting changes → generate conflict markers

// ---------------------------------------------------------------------------
// ConflictRegion
// ---------------------------------------------------------------------------

/// A region of conflict between two versions of a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictRegion {
    /// 1-based line number where the conflict starts in the merged output.
    pub start_line: usize,
    /// Content from "ours" (current file on disk).
    pub ours_content: String,
    /// Content from "theirs" (intended edit).
    pub theirs_content: String,
    /// Content from "base" (original version before either change).
    pub base_content: String,
}

// ---------------------------------------------------------------------------
// MergeResult
// ---------------------------------------------------------------------------

/// Result of a three-way merge.
#[derive(Debug, Clone)]
pub enum MergeResult {
    /// Merge succeeded cleanly — no conflicts.
    Clean(String),
    /// Merge completed but has conflicts that need manual resolution.
    Conflicted {
        merged: String,
        conflicts: Vec<ConflictRegion>,
    },
}

impl MergeResult {
    /// Returns the merged content regardless of whether conflicts exist.
    pub fn into_content(self) -> String {
        match self {
            MergeResult::Clean(content) => content,
            MergeResult::Conflicted { merged, .. } => merged,
        }
    }

    /// Returns `true` if the merge is clean (no conflicts).
    pub fn is_clean(&self) -> bool {
        matches!(self, MergeResult::Clean(_))
    }
}

// ---------------------------------------------------------------------------
// DiffHunk (internal)
// ---------------------------------------------------------------------------

/// A contiguous region of change between base and a modified version.
#[derive(Debug, Clone)]
#[allow(dead_code)] // changed_start/changed_end used by LCS merge algorithm
struct DiffHunk {
    /// Start line in base (0-based).
    base_start: usize,
    /// End line in base (exclusive, 0-based).
    base_end: usize,
    /// Start line in the changed version (0-based).
    changed_start: usize,
    /// End line in the changed version (exclusive, 0-based).
    changed_end: usize,
    /// Lines from the changed version in this region.
    changed_lines: Vec<String>,
}

// ---------------------------------------------------------------------------
// LCS diff computation
// ---------------------------------------------------------------------------

/// Build the LCS dynamic-programming table for two line sequences.
fn lcs_table(a: &[&str], b: &[&str]) -> Vec<Vec<usize>> {
    let m = a.len();
    let n = b.len();
    let mut dp = vec![vec![0usize; n + 1]; m + 1];
    for i in 1..=m {
        for j in 1..=n {
            if a[i - 1] == b[j - 1] {
                dp[i][j] = dp[i - 1][j - 1] + 1;
            } else {
                dp[i][j] = dp[i - 1][j].max(dp[i][j - 1]);
            }
        }
    }
    dp
}

/// Backtrack through the LCS table to produce an edit script.
/// Each element is ('=', line), ('-', line), or ('+', line).
fn backtrack_lcs<'a>(dp: &[Vec<usize>], a: &[&'a str], b: &[&'a str]) -> Vec<(char, &'a str)> {
    let mut edits = Vec::new();
    let (mut i, mut j) = (a.len(), b.len());
    while i > 0 || j > 0 {
        if i > 0 && j > 0 && a[i - 1] == b[j - 1] {
            edits.push(('=', a[i - 1]));
            i -= 1;
            j -= 1;
        } else if j > 0 && (i == 0 || dp[i][j - 1] >= dp[i - 1][j]) {
            edits.push(('+', b[j - 1]));
            j -= 1;
        } else {
            edits.push(('-', a[i - 1]));
            i -= 1;
        }
    }
    edits.reverse();
    edits
}

/// Compute diff hunks between a base and a changed version.
/// Returns a list of hunks describing regions that differ.
fn compute_hunks(base: &[&str], changed: &[&str]) -> Vec<DiffHunk> {
    // Guard against very large files
    const MAX_LINES_FOR_LCS: usize = 50_000;
    if base.len() > MAX_LINES_FOR_LCS || changed.len() > MAX_LINES_FOR_LCS {
        // Fall back: treat the entire file as one big hunk
        return vec![DiffHunk {
            base_start: 0,
            base_end: base.len(),
            changed_start: 0,
            changed_end: changed.len(),
            changed_lines: changed.iter().map(|s| s.to_string()).collect(),
        }];
    }

    let dp = lcs_table(base, changed);
    let edits = backtrack_lcs(&dp, base, changed);

    // Convert edit script into hunks
    let mut hunks: Vec<DiffHunk> = Vec::new();
    let mut base_pos = 0usize;
    let mut changed_pos = 0usize;
    let mut in_hunk = false;
    let mut hunk_base_start = 0usize;
    let mut hunk_changed_start = 0usize;
    let mut hunk_changed_lines: Vec<String> = Vec::new();
    let mut hunk_base_end = 0usize;

    for edit in &edits {
        match edit.0 {
            '=' => {
                if in_hunk {
                    // Close the current hunk
                    hunks.push(DiffHunk {
                        base_start: hunk_base_start,
                        base_end: hunk_base_end,
                        changed_start: hunk_changed_start,
                        changed_end: changed_pos,
                        changed_lines: hunk_changed_lines.clone(),
                    });
                    in_hunk = false;
                    hunk_changed_lines.clear();
                }
                base_pos += 1;
                changed_pos += 1;
            }
            '-' => {
                if !in_hunk {
                    hunk_base_start = base_pos;
                    hunk_changed_start = changed_pos;
                    hunk_changed_lines.clear();
                    in_hunk = true;
                }
                hunk_base_end = base_pos + 1;
                base_pos += 1;
            }
            '+' => {
                if !in_hunk {
                    hunk_base_start = base_pos;
                    hunk_changed_start = changed_pos;
                    hunk_changed_lines.clear();
                    in_hunk = true;
                }
                hunk_changed_lines.push(edit.1.to_string());
                hunk_base_end = base_pos;
                changed_pos += 1;
            }
            _ => {}
        }
    }

    // Flush remaining hunk
    if in_hunk {
        hunks.push(DiffHunk {
            base_start: hunk_base_start,
            base_end: hunk_base_end,
            changed_start: hunk_changed_start,
            changed_end: changed_pos,
            changed_lines: hunk_changed_lines,
        });
    }

    hunks
}

// ---------------------------------------------------------------------------
// Three-way merge
// ---------------------------------------------------------------------------

/// Perform a three-way merge.
///
/// - `base`: the original content (from git HEAD or checkpoint)
/// - `ours`: the current file content (what's on disk)
/// - `theirs`: what the file would look like if we applied the edit to base
///
/// Returns `MergeResult::Clean` if there are no conflicts, or
/// `MergeResult::Conflicted` with conflict markers if changes collide.
pub fn three_way_merge(base: &str, ours: &str, theirs: &str) -> MergeResult {
    // Fast path: all identical
    if base == ours && base == theirs {
        return MergeResult::Clean(base.to_string());
    }
    // Fast path: only ours changed
    if base == theirs {
        return MergeResult::Clean(ours.to_string());
    }
    // Fast path: only theirs changed
    if base == ours {
        return MergeResult::Clean(theirs.to_string());
    }
    // Fast path: ours and theirs converged to the same result
    if ours == theirs {
        return MergeResult::Clean(ours.to_string());
    }

    // Split into lines (keeping line endings for reconstruction)
    let base_lines: Vec<&str> = base.lines().collect();
    let ours_lines: Vec<&str> = ours.lines().collect();
    let theirs_lines: Vec<&str> = theirs.lines().collect();

    let ours_hunks = compute_hunks(&base_lines, &ours_lines);
    let theirs_hunks = compute_hunks(&base_lines, &theirs_lines);

    // Build a map from base line ranges to hunks
    // Walk through base lines, checking if each region is touched by ours, theirs, or both

    let mut merged_lines: Vec<String> = Vec::new();
    let mut conflicts: Vec<ConflictRegion> = Vec::new();

    // Build interval lookups for hunks
    let ours_intervals: Vec<(usize, usize, &DiffHunk)> = ours_hunks
        .iter()
        .map(|h| (h.base_start, h.base_end, h))
        .collect();
    let theirs_intervals: Vec<(usize, usize, &DiffHunk)> = theirs_hunks
        .iter()
        .map(|h| (h.base_start, h.base_end, h))
        .collect();

    // Walk through base, handling each region
    let mut base_pos = 0usize;
    let mut ours_idx = 0usize;
    let mut theirs_idx = 0usize;

    while base_pos < base_lines.len() || ours_idx < ours_intervals.len() || theirs_idx < theirs_intervals.len() {
        // Find the next event (hunk start or base end)
        let ours_next = ours_intervals.get(ours_idx).map(|(s, _, _)| *s);
        let theirs_next = theirs_intervals.get(theirs_idx).map(|(s, _, _)| *s);

        // Determine the next boundary
        let next_hunk_start = match (ours_next, theirs_next) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => base_lines.len(),
        };

        // Emit unchanged base lines up to the next hunk start
        let unchanged_end = next_hunk_start.min(base_lines.len());
        while base_pos < unchanged_end {
            merged_lines.push(base_lines[base_pos].to_string());
            base_pos += 1;
        }

        // Check if ours has a hunk starting here
        let ours_hunk = if ours_idx < ours_intervals.len() && ours_intervals[ours_idx].0 == next_hunk_start {
            Some(ours_intervals[ours_idx].2)
        } else {
            None
        };

        // Check if theirs has a hunk starting here
        let theirs_hunk = if theirs_idx < theirs_intervals.len() && theirs_intervals[theirs_idx].0 == next_hunk_start {
            Some(theirs_intervals[theirs_idx].2)
        } else {
            None
        };

        match (ours_hunk, theirs_hunk) {
            (Some(oh), None) => {
                // Only ours changed this region
                for line in &oh.changed_lines {
                    merged_lines.push(line.clone());
                }
                base_pos = oh.base_end;
                ours_idx += 1;
            }
            (None, Some(th)) => {
                // Only theirs changed this region
                for line in &th.changed_lines {
                    merged_lines.push(line.clone());
                }
                base_pos = th.base_end;
                theirs_idx += 1;
            }
            (Some(oh), Some(th)) => {
                // Both sides changed this region
                let base_region: Vec<&str> = base_lines[oh.base_start.min(th.base_start)..oh.base_end.max(th.base_end)].to_vec();
                let base_region_str = base_region.join("\n");
                let ours_str = oh.changed_lines.join("\n");
                let theirs_str = th.changed_lines.join("\n");

                if ours_str == theirs_str {
                    // Same change — no conflict
                    for line in &oh.changed_lines {
                        merged_lines.push(line.clone());
                    }
                } else {
                    // Conflict — generate conflict markers
                    let conflict_start = merged_lines.len() + 1;
                    merged_lines.push("<<<<<<< ours (current file)".to_string());
                    for line in &oh.changed_lines {
                        merged_lines.push(line.clone());
                    }
                    merged_lines.push("=======".to_string());
                    for line in &th.changed_lines {
                        merged_lines.push(line.clone());
                    }
                    merged_lines.push(">>>>>>> theirs (intended edit)".to_string());

                    conflicts.push(ConflictRegion {
                        start_line: conflict_start,
                        ours_content: ours_str.clone(),
                        theirs_content: theirs_str.clone(),
                        base_content: base_region_str,
                    });
                }

                base_pos = oh.base_end.max(th.base_end);
                ours_idx += 1;
                theirs_idx += 1;
            }
            (None, None) => {
                // No more hunks — we're done with base lines (handled above)
                break;
            }
        }
    }

    // Emit any remaining base lines
    while base_pos < base_lines.len() {
        merged_lines.push(base_lines[base_pos].to_string());
        base_pos += 1;
    }

    // If the original content ended with a newline, preserve it
    let trailing_newline = base.ends_with('\n') || ours.ends_with('\n') || theirs.ends_with('\n');

    let mut merged = merged_lines.join("\n");
    if trailing_newline {
        merged.push('\n');
    }

    if conflicts.is_empty() {
        MergeResult::Clean(merged)
    } else {
        MergeResult::Conflicted {
            merged,
            conflicts,
        }
    }
}

// ---------------------------------------------------------------------------
// Conflict marker parsing
// ---------------------------------------------------------------------------

/// Parse conflict markers from a string.
///
/// Returns a list of conflict regions found in the content.
/// Conflict markers have the form:
/// ```text
/// <<<<<<< ours (current file)
/// ... ours content ...
/// =======
/// ... theirs content ...
/// >>>>>>> theirs (intended edit)
/// ```
///
/// The marker labels after `<<<<<<<` and `>>>>>>>` are optional and ignored.
pub fn parse_conflict_markers(content: &str) -> Vec<ConflictRegion> {
    let mut conflicts = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim_start();
        if line.starts_with("<<<<<<<") {
            let start_line = i + 1; // 1-based
            let mut ours_lines: Vec<String> = Vec::new();
            let mut theirs_lines: Vec<String> = Vec::new();
            let base_lines: Vec<String> = Vec::new();
            let mut in_ours = true;
            let mut found_separator = false;
            let mut found_closing = false;
            i += 1;

            while i < lines.len() {
                let inner = lines[i].trim_start();
                if inner == "=======" && in_ours {
                    in_ours = false;
                    found_separator = true;
                    i += 1;
                    continue;
                }
                if inner.starts_with(">>>>>>>") {
                    found_closing = true;
                    i += 1;
                    break;
                }
                if in_ours {
                    ours_lines.push(lines[i].to_string());
                } else {
                    theirs_lines.push(lines[i].to_string());
                }
                i += 1;
            }

            // Only record a conflict if we found both separator and closing marker
            if found_separator && found_closing {
                conflicts.push(ConflictRegion {
                    start_line,
                    ours_content: ours_lines.join("\n"),
                    theirs_content: theirs_lines.join("\n"),
                    base_content: base_lines.join("\n"),
                });
            }
        } else {
            i += 1;
        }
    }

    conflicts
}

/// Resolve conflicts in a file using the given resolution strategy.
///
/// `resolutions` must have one entry per conflict, either `"ours"` or `"theirs"`.
/// Returns the resolved content with all conflicts handled.
pub fn resolve_conflicts(content: &str, resolutions: &[String]) -> Result<String, String> {
    let conflicts = parse_conflict_markers(content);
    if conflicts.len() != resolutions.len() {
        return Err(format!(
            "Number of resolutions ({}) does not match number of conflicts ({})",
            resolutions.len(),
            conflicts.len()
        ));
    }

    let lines: Vec<&str> = content.lines().collect();
    let mut result: Vec<String> = Vec::new();
    let mut conflict_idx = 0;
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim_start();
        if line.starts_with("<<<<<<<") {
            let resolution = &resolutions[conflict_idx];
            conflict_idx += 1;

            // Skip <<<<<<< line
            i += 1;

            // Collect ours content
            let mut ours_lines: Vec<String> = Vec::new();
            while i < lines.len() && !lines[i].trim_start().starts_with("=======") {
                ours_lines.push(lines[i].to_string());
                i += 1;
            }

            // Skip ======= line
            i += 1;

            // Collect theirs content
            let mut theirs_lines: Vec<String> = Vec::new();
            while i < lines.len() && !lines[i].trim_start().starts_with(">>>>>>>") {
                theirs_lines.push(lines[i].to_string());
                i += 1;
            }

            // Skip >>>>>>> line
            i += 1;

            // Apply resolution
            match resolution.as_str() {
                "ours" => {
                    for l in ours_lines {
                        result.push(l);
                    }
                }
                "theirs" => {
                    for l in theirs_lines {
                        result.push(l);
                    }
                }
                _ => {
                    return Err(format!(
                        "Invalid resolution '{resolution}': must be 'ours' or 'theirs'",
                    ));
                }
            }
        } else {
            result.push(lines[i].to_string());
            i += 1;
        }
    }

    let mut output = result.join("\n");
    if content.ends_with('\n') {
        output.push('\n');
    }
    Ok(output)
}

// ---------------------------------------------------------------------------
// Get base version from git
// ---------------------------------------------------------------------------

/// Get the git HEAD version of a file.
///
/// Returns `None` if git is not available or the file is not tracked.
pub async fn get_git_head_version(file_path: &str) -> Option<String> {
    use tokio::process::Command;

    // Use git show HEAD:<path> to get the committed version
    let result = Command::new("git")
        .args(["show", &format!("HEAD:{file_path}")])
        .output()
        .await
        .ok()?;

    if !result.status.success() {
        return None;
    }

    String::from_utf8(result.stdout).ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_conflict_markers ─────────────────────────────────────────

    #[test]
    fn test_parse_no_conflicts() {
        let content = "line1\nline2\nline3\n";
        let conflicts = parse_conflict_markers(content);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_parse_single_conflict() {
        let content = "\
line1
<<<<<<< ours
ours line
=======
theirs line
>>>>>>> theirs
line3
";
        let conflicts = parse_conflict_markers(content);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].start_line, 2);
        assert_eq!(conflicts[0].ours_content, "ours line");
        assert_eq!(conflicts[0].theirs_content, "theirs line");
    }

    #[test]
    fn test_parse_multiple_conflicts() {
        let content = "\
line1
<<<<<<< ours
ours1
=======
theirs1
>>>>>>> theirs
middle
<<<<<<< ours
ours2
=======
theirs2
>>>>>>> theirs
end
";
        let conflicts = parse_conflict_markers(content);
        assert_eq!(conflicts.len(), 2);
        assert_eq!(conflicts[0].ours_content, "ours1");
        assert_eq!(conflicts[0].theirs_content, "theirs1");
        assert_eq!(conflicts[1].ours_content, "ours2");
        assert_eq!(conflicts[1].theirs_content, "theirs2");
    }

    #[test]
    fn test_parse_conflict_multiline_content() {
        let content = "\
<<<<<<< ours
ours line 1
ours line 2
=======
theirs line 1
theirs line 2
theirs line 3
>>>>>>> theirs
";
        let conflicts = parse_conflict_markers(content);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].ours_content, "ours line 1\nours line 2");
        assert_eq!(
            conflicts[0].theirs_content,
            "theirs line 1\ntheirs line 2\ntheirs line 3"
        );
    }

    #[test]
    fn test_parse_unclosed_conflict_ignored() {
        // Missing >>>>>>> marker — should not produce a valid conflict
        let content = "\
<<<<<<< ours
some content
=======
other content
";
        let conflicts = parse_conflict_markers(content);
        assert!(conflicts.is_empty());
    }

    #[test]
    fn test_parse_conflict_with_custom_labels() {
        let content = "\
<<<<<<< HEAD
head content
=======
branch content
>>>>>>> feature-branch
";
        let conflicts = parse_conflict_markers(content);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].ours_content, "head content");
        assert_eq!(conflicts[0].theirs_content, "branch content");
    }

    // ── three_way_merge ────────────────────────────────────────────────

    #[test]
    fn test_merge_no_changes() {
        let base = "line1\nline2\nline3\n";
        let result = three_way_merge(base, base, base);
        assert!(result.is_clean());
        assert_eq!(result.into_content(), base);
    }

    #[test]
    fn test_merge_only_ours_changed() {
        let base = "line1\nline2\nline3\n";
        let ours = "line1\nLINE2_CHANGED\nline3\n";
        let theirs = base;
        let result = three_way_merge(base, ours, theirs);
        assert!(result.is_clean());
        assert_eq!(result.into_content(), ours);
    }

    #[test]
    fn test_merge_only_theirs_changed() {
        let base = "line1\nline2\nline3\n";
        let theirs = "line1\nTHEIRS\nline3\n";
        let result = three_way_merge(base, base, theirs);
        assert!(result.is_clean());
        assert_eq!(result.into_content(), theirs);
    }

    #[test]
    fn test_merge_both_same() {
        let base = "line1\nline2\nline3\n";
        let both = "line1\nCHANGED\nline3\n";
        let result = three_way_merge(base, both, both);
        assert!(result.is_clean());
        assert_eq!(result.into_content(), both);
    }

    #[test]
    fn test_merge_conflict() {
        let base = "line1\nline2\nline3\n";
        let ours = "line1\nOURS\nline3\n";
        let theirs = "line1\nTHEIRS\nline3\n";
        let result = three_way_merge(base, ours, theirs);
        match result {
            MergeResult::Conflicted { merged, conflicts } => {
                assert_eq!(conflicts.len(), 1);
                assert!(merged.contains("<<<<<<<"));
                assert!(merged.contains("======="));
                assert!(merged.contains(">>>>>>>"));
                assert!(merged.contains("OURS"));
                assert!(merged.contains("THEIRS"));
                assert_eq!(conflicts[0].ours_content, "OURS");
                assert_eq!(conflicts[0].theirs_content, "THEIRS");
            }
            MergeResult::Clean(_) => panic!("Expected conflict, got clean merge"),
        }
    }

    #[test]
    fn test_merge_non_overlapping_changes() {
        // Both sides change different parts — should merge cleanly
        let base = "line1\nline2\nline3\nline4\n";
        let ours = "OURS1\nline2\nline3\nline4\n";
        let theirs = "line1\nline2\nline3\nTHEIRS4\n";
        let result = three_way_merge(base, ours, theirs);
        assert!(result.is_clean());
        let content = result.into_content();
        assert!(content.contains("OURS1"));
        assert!(content.contains("THEIRS4"));
    }

    #[test]
    fn test_merge_insertions_different_spots() {
        let base = "a\nb\nc\n";
        let ours = "a\nX\nb\nc\n"; // inserted X after a
        let theirs = "a\nb\nY\nc\n"; // inserted Y after b
        let result = three_way_merge(base, ours, theirs);
        assert!(result.is_clean());
        let content = result.into_content();
        assert!(content.contains("X"));
        assert!(content.contains("Y"));
    }

    #[test]
    fn test_merge_empty_base() {
        let base = "";
        let ours = "ours content\n";
        let theirs = "theirs content\n";
        let result = three_way_merge(base, ours, theirs);
        match result {
            MergeResult::Conflicted { conflicts, .. } => {
                assert_eq!(conflicts.len(), 1);
            }
            MergeResult::Clean(_) => {
                // If both add the same content it's clean
            }
        }
    }

    #[test]
    fn test_merge_one_side_empty() {
        let base = "line1\nline2\n";
        let ours = "line1\nline2\n";
        let theirs = "line1\nline2\nextra\n";
        let result = three_way_merge(base, ours, theirs);
        assert!(result.is_clean());
        assert_eq!(result.into_content(), "line1\nline2\nextra\n");
    }

    #[test]
    fn test_merge_deletion_in_ours() {
        let base = "line1\nline2\nline3\n";
        let ours = "line1\nline3\n"; // deleted line2
        let theirs = "line1\nline2\nTHEIRS3\n"; // changed line3
        let result = three_way_merge(base, ours, theirs);
        assert!(result.is_clean());
        let content = result.into_content();
        assert!(content.contains("THEIRS3"));
        assert!(!content.contains("line2"));
    }

    // ── resolve_conflicts ──────────────────────────────────────────────

    #[test]
    fn test_resolve_ours() {
        let content = "\
line1
<<<<<<< ours
OURS
=======
THEIRS
>>>>>>> theirs
line3
";
        let resolved = resolve_conflicts(content, &["ours".to_string()]).unwrap();
        assert_eq!(resolved, "line1\nOURS\nline3\n");
    }

    #[test]
    fn test_resolve_theirs() {
        let content = "\
line1
<<<<<<< ours
OURS
=======
THEIRS
>>>>>>> theirs
line3
";
        let resolved = resolve_conflicts(content, &["theirs".to_string()]).unwrap();
        assert_eq!(resolved, "line1\nTHEIRS\nline3\n");
    }

    #[test]
    fn test_resolve_wrong_count() {
        let content = "\
<<<<<<< ours
a
=======
b
>>>>>>> theirs
";
        let result = resolve_conflicts(content, &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not match"));
    }

    #[test]
    fn test_resolve_invalid_strategy() {
        let content = "\
<<<<<<< ours
a
=======
b
>>>>>>> theirs
";
        let result = resolve_conflicts(content, &["invalid".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid resolution"));
    }

    #[test]
    fn test_resolve_multiple_conflicts() {
        let content = "\
start
<<<<<<< ours
A1
=======
B1
>>>>>>> theirs
mid
<<<<<<< ours
A2
=======
B2
>>>>>>> theirs
end
";
        let resolved =
            resolve_conflicts(content, &["ours".to_string(), "theirs".to_string()]).unwrap();
        assert_eq!(resolved, "start\nA1\nmid\nB2\nend\n");
    }

    // ── MergeResult helpers ────────────────────────────────────────────

    #[test]
    fn test_merge_result_is_clean() {
        let clean = MergeResult::Clean("ok".to_string());
        assert!(clean.is_clean());

        let conflicted = MergeResult::Conflicted {
            merged: "conflict".to_string(),
            conflicts: vec![],
        };
        assert!(!conflicted.is_clean());
    }

    #[test]
    fn test_merge_result_into_content() {
        let clean = MergeResult::Clean("clean content".to_string());
        assert_eq!(clean.into_content(), "clean content");

        let conflicted = MergeResult::Conflicted {
            merged: "conflicted content".to_string(),
            conflicts: vec![],
        };
        assert_eq!(conflicted.into_content(), "conflicted content");
    }

    // ── LCS internals ──────────────────────────────────────────────────

    #[test]
    fn test_lcs_table_identical() {
        let a = vec!["a", "b", "c"];
        let b = vec!["a", "b", "c"];
        let dp = lcs_table(&a, &b);
        assert_eq!(dp[3][3], 3);
    }

    #[test]
    fn test_lcs_table_different() {
        let a = vec!["a", "b", "c"];
        let b = vec!["x", "b", "y"];
        let dp = lcs_table(&a, &b);
        assert_eq!(dp[3][3], 1); // only "b" is common
    }

    #[test]
    fn test_lcs_table_empty() {
        let a: Vec<&str> = vec![];
        let b: Vec<&str> = vec![];
        let dp = lcs_table(&a, &b);
        assert_eq!(dp[0][0], 0);
    }

    #[test]
    fn test_backtrack_produces_correct_edits() {
        let a = vec!["a", "b", "c"];
        let b = vec!["a", "x", "c"];
        let dp = lcs_table(&a, &b);
        let edits = backtrack_lcs(&dp, &a, &b);
        let has_delete_b = edits.iter().any(|(op, line)| *op == '-' && *line == "b");
        let has_insert_x = edits.iter().any(|(op, line)| *op == '+' && *line == "x");
        assert!(has_delete_b);
        assert!(has_insert_x);
    }

    #[test]
    fn test_compute_hunks_no_change() {
        let base = vec!["a", "b", "c"];
        let changed = vec!["a", "b", "c"];
        let hunks = compute_hunks(&base, &changed);
        assert!(hunks.is_empty());
    }

    #[test]
    fn test_compute_hunks_single_change() {
        let base = vec!["a", "b", "c"];
        let changed = vec!["a", "X", "c"];
        let hunks = compute_hunks(&base, &changed);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].changed_lines, vec!["X".to_string()]);
    }

    #[test]
    fn test_compute_hunks_insertion() {
        let base = vec!["a", "c"];
        let changed = vec!["a", "b", "c"];
        let hunks = compute_hunks(&base, &changed);
        assert_eq!(hunks.len(), 1);
        assert!(hunks[0].changed_lines.contains(&"b".to_string()));
    }

    // ── parse_conflict_markers roundtrip with merge ────────────────────

    #[test]
    fn test_conflict_markers_from_merge_are_parseable() {
        let base = "line1\nline2\nline3\n";
        let ours = "line1\nOURS\nline3\n";
        let theirs = "line1\nTHEIRS\nline3\n";
        let result = three_way_merge(base, ours, theirs);
        match result {
            MergeResult::Conflicted { merged, .. } => {
                let conflicts = parse_conflict_markers(&merged);
                assert_eq!(conflicts.len(), 1);
                assert_eq!(conflicts[0].ours_content, "OURS");
                assert_eq!(conflicts[0].theirs_content, "THEIRS");
            }
            MergeResult::Clean(_) => panic!("Expected conflict"),
        }
    }
}
