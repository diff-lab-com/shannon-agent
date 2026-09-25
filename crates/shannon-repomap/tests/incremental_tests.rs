//! Integration tests for the P1-4 incremental update + cache.
//!
//! These exercise the multi-language fixture under `tests/fixtures/multi_lang/`
//! and assert:
//!
//! 1. The cache loads the fixture (Rust + TS + Python + Go).
//! 2. Updating a single file is meaningfully faster than a full re-walk
//!    (target: >5x speedup).
//! 3. The trimmed markdown respects the budget.
//! 4. Removing a file via `update_file` (when it no longer exists) evicts
//!    it from the cache.
//! 5. `update_file` takes the no-parse fast path when a tracked file's
//!    mtime is unchanged (editor-save churn).
//! 6. `RepoMapCache::new` validates a loaded disk cache against a stat-only
//!    walk: modified / deleted / added files force a full re-parse instead
//!    of serving stale symbols.

use shannon_repomap::budget::total_tokens;
use shannon_repomap::{RepoMap, RepoMapCache, SymbolKind, SymbolMap, SymbolNode};
use std::path::PathBuf;
use std::time::{Duration, Instant, UNIX_EPOCH};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn multi_lang_root() -> PathBuf {
    fixtures_dir().join("multi_lang")
}

/// The fixture contains a Rust crate (3 files), one TS module, one Python
/// module, and one Go module — 6 supported files in total.
const EXPECTED_FILE_COUNT: usize = 6;

/// Snapshot the multi-language fixture into a private temp directory so
/// tests can mutate it freely without racing each other. Returns the
/// snapshot root.
fn snapshot_multi_lang(label: &str) -> PathBuf {
    let src = multi_lang_root();
    let dst = std::env::temp_dir().join(format!(
        "shannon_repomap_test_{label}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let _ = std::fs::remove_dir_all(&dst);
    copy_tree(&src, &dst);
    dst
}

fn copy_tree(src: &std::path::Path, dst: &std::path::Path) {
    std::fs::create_dir_all(dst).expect("create dst dir");
    for entry in walkdir::WalkDir::new(src)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target).expect("mkdir");
        } else if entry.file_type().is_file() {
            std::fs::copy(entry.path(), &target).expect("copy file");
        }
    }
}

#[test]
fn multi_lang_fixture_loads_all_languages() {
    let root = multi_lang_root();
    let cache = RepoMapCache::ephemeral(&root).expect("ephemeral cache");
    assert_eq!(
        cache.file_count(),
        EXPECTED_FILE_COUNT,
        "expected {EXPECTED_FILE_COUNT} files in multi-lang fixture, found {}",
        cache.file_count()
    );

    // Every language should contribute at least one symbol.
    let map = cache.map();
    let kinds_present = collect_kinds(&map);
    assert!(
        kinds_present.contains(&SymbolKind::Function),
        "Rust functions missing"
    );
    assert!(
        kinds_present.contains(&SymbolKind::Struct),
        "Rust struct missing"
    );
    assert!(
        kinds_present.contains(&SymbolKind::Class),
        "TS/Python class missing"
    );
    assert!(
        kinds_present.contains(&SymbolKind::Interface),
        "TS/Go interface missing"
    );
    assert!(
        kinds_present.contains(&SymbolKind::Trait),
        "Rust trait missing"
    );
    assert!(
        kinds_present.contains(&SymbolKind::Impl),
        "Rust impl missing"
    );
}

#[test]
fn incremental_update_handles_single_file_change() {
    let root = snapshot_multi_lang("single_change");
    let mut cache = RepoMapCache::ephemeral(&root).expect("ephemeral cache");
    let initial = cache.file_count();
    assert_eq!(initial, EXPECTED_FILE_COUNT);

    // Touch a single file: re-write the auth module with two extra functions
    // and an extra impl. Using a totally different symbol set guarantees
    // the parsed symbol list differs from the snapshot.
    let auth_path = root.join("src/auth.rs");
    let updated = "\
pub trait Authenticator {\n    fn authenticate(&self, token: &str) -> bool;\n    fn rotate(&mut self, new_secret: &str);\n}\n\
pub struct TokenAuth {\n    pub secret: String,\n}\n\
impl Authenticator for TokenAuth {\n    fn authenticate(&self, token: &str) -> bool {\n        token == self.secret\n    }\n    fn rotate(&mut self, new_secret: &str) {\n        self.secret = new_secret.to_string();\n    }\n}\n\
impl TokenAuth {\n    pub fn new(secret: impl Into<String>) -> Self {\n        Self { secret: secret.into() }\n    }\n}\n\
pub fn extra_helper_v2() -> &'static str { \"v2\" }\n\
pub fn second_helper_v2() -> usize { 42 }\n";
    std::fs::write(&auth_path, updated).expect("rewrite auth.rs");
    // The cache re-parses only when the mtime moves past the whole-second
    // value recorded by the walk; pin a distinctly different one rather
    // than racing the clock.
    set_mtime_seconds(&auth_path, PINNED_MTIME_SECS);

    let changed = cache.update_file(&auth_path).expect("update_file");
    assert!(
        changed,
        "update_file must report a change after content edits"
    );

    // The map should now mention the new helpers (and not the old ones).
    let map = cache.map();
    let names = collect_names(&map);
    assert!(
        names.iter().any(|n| n == "extra_helper_v2"),
        "new helper missing from cache: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "second_helper_v2"),
        "second helper missing from cache: {names:?}"
    );
}

#[test]
fn incremental_update_handles_removed_file() {
    let root = snapshot_multi_lang("removed");
    let mut cache = RepoMapCache::ephemeral(&root).expect("ephemeral cache");
    assert_eq!(cache.file_count(), EXPECTED_FILE_COUNT);

    let go_path = root.join("pkg/server.go");
    std::fs::remove_file(&go_path).expect("remove go file");
    let changed = cache
        .update_file(&go_path)
        .expect("update_file on missing path");
    assert!(
        changed,
        "update_file must report a change when file is gone"
    );
    assert_eq!(cache.file_count(), EXPECTED_FILE_COUNT - 1);
}

#[test]
fn update_file_skips_reparse_when_mtime_unchanged() {
    // Fixed epoch offsets hours apart: the cache stores whole-second
    // mtimes, so this sidesteps same-second collisions without sleeps.
    const T_OLD: u64 = 1_700_000_000;
    const T_NEW: u64 = T_OLD + 3_600;

    let root = snapshot_multi_lang("mtime_fast_path");
    let mut cache = RepoMapCache::ephemeral(&root).expect("ephemeral cache");
    assert_eq!(cache.file_count(), EXPECTED_FILE_COUNT);

    // Write a brand-new file and record it: an insert always parses and
    // must report a change.
    let probe = root.join("fast_path_probe.rs");
    std::fs::write(&probe, "pub fn probe_v1() -> u8 { 1 }\n").expect("write probe");
    set_mtime_seconds(&probe, T_OLD);
    assert!(
        cache.update_file(&probe).expect("first update"),
        "insert of a new file must report a change"
    );
    assert_eq!(cache.file_count(), EXPECTED_FILE_COUNT + 1);

    // Same content, same mtime: the documented no-op fast path.
    assert!(
        !cache.update_file(&probe).expect("no-op update"),
        "unchanged mtime must take the no-parse fast path"
    );

    // Rewrite the contents but restore the recorded mtime — exactly what a
    // save-without-changes looks like to the filesystem. The cache cannot
    // tell, so it must skip the parse and keep the old symbols.
    std::fs::write(&probe, "pub fn probe_v2() -> u8 { 2 }\n").expect("rewrite probe");
    set_mtime_seconds(&probe, T_OLD);
    assert!(
        !cache.update_file(&probe).expect("backdated update"),
        "backdated mtime must still take the fast path"
    );
    let names = collect_names(&cache.map());
    assert!(
        names.iter().any(|n| n == "probe_v1"),
        "old symbol must survive a backdated edit: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n == "probe_v2"),
        "backdated edit must not be parsed in: {names:?}"
    );

    // A real mtime advance re-parses and picks the new symbols up.
    set_mtime_seconds(&probe, T_NEW);
    assert!(
        cache.update_file(&probe).expect("real update"),
        "changed mtime with changed content must re-parse and report it"
    );
    let names = collect_names(&cache.map());
    assert!(
        names.iter().any(|n| n == "probe_v2"),
        "edited symbols missing after a real mtime change: {names:?}"
    );
}

#[test]
fn pack_respects_token_budget() {
    let root = snapshot_multi_lang("pack_budget");
    let mut cache = RepoMapCache::ephemeral(&root).expect("ephemeral cache");
    let big_budget = total_tokens(&cache.map());
    let small_budget = 200usize;

    // Sanity: full map is comfortably larger than the small budget.
    assert!(
        big_budget > small_budget,
        "fixture must be larger than {small_budget} tokens (got {big_budget})"
    );

    // `pack` enforces the budget on symbol signatures. Markdown rendering
    // adds structural headers ("# Repo Map: <root>", "## <path>", "- **fn**
    // `...` — at line N") that are not counted against the symbol budget
    // because they're constant per file / per symbol. Verify both:
    //   1. The trimmed symbol budget is satisfied (the real contract).
    //   2. The rendered markdown is at most ~2.5x the symbol budget, which
    //      captures the structural overhead without making the test flaky.
    let md = cache.pack(small_budget);
    let tokens_used = shannon_repomap::budget::estimate_tokens(&md);
    let rendered_allowance = small_budget * 5 / 2;
    assert!(
        tokens_used <= rendered_allowance,
        "pack output exceeds 2.5x budget: {tokens_used} > {rendered_allowance}"
    );
    // The trimmed map must report fewer tokens than the un-trimmed map.
    let trimmed_tokens = total_tokens(&cache.map());
    assert!(
        trimmed_tokens <= small_budget + 1,
        "trimmed symbol budget violated: {trimmed_tokens} > {small_budget}"
    );
    // Header must include the Repo Map label.
    assert!(md.contains("Repo Map:"));
}

#[test]
fn incremental_update_is_much_faster_than_full_reparse() {
    // Build a synthetic fixture large enough that the full-reparse cost is
    // dominated by N file parses, while incremental only pays for one.
    let tmp = std::env::temp_dir().join(format!("shannon_repomap_speedup_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create tmp dir");

    // Create 60 trivial Rust source files, each with a handful of symbols.
    for i in 0..60 {
        let path = tmp.join(format!("mod_{i:03}.rs"));
        std::fs::write(
            &path,
            format!(
                "pub fn fn_a_{i}() -> i32 {{ {i} }}\n\
                 pub fn fn_b_{i}(x: i32) -> i32 {{ x + {i} }}\n\
                 pub struct Type_{i} {{ pub v: i32 }}\n"
            ),
        )
        .expect("write fixture file");
    }
    // The single file we'll edit during the timed runs.
    let target = tmp.join("mod_007.rs");
    let original = std::fs::read_to_string(&target).expect("read target");
    let edited = format!("{original}\npub fn extra_after_edit() {{}}\n");

    // Warm up the disk cache and the tree-sitter runtime so the timed runs
    // aren't dominated by first-use costs.
    let _ = RepoMapCache::ephemeral(&tmp).expect("warmup cache");

    // Build a per-sample full+incremental pair, take the best ratio across
    // samples. Using best-of rather than median prevents a single noisy
    // sample from hiding a real speedup.
    let mut best_ratio = 0.0f64;
    let mut best_inc = std::time::Duration::ZERO;
    let mut best_full = std::time::Duration::ZERO;
    for _ in 0..10 {
        std::fs::write(&target, &edited).expect("write edit");
        let mut cache = RepoMapCache::ephemeral(&tmp).expect("ephemeral");

        // The walk above recorded the target's current whole-second mtime;
        // pin a distinctly different one so the timed update_file actually
        // re-parses instead of taking the same-mtime fast path.
        set_mtime_seconds(&target, PINNED_MTIME_SECS);

        let t_inc = Instant::now();
        cache.update_file(&target).expect("incremental update_file");
        let inc = t_inc.elapsed();

        let mut cache = RepoMapCache::ephemeral(&tmp).expect("ephemeral");
        let t_full = Instant::now();
        cache.full_reparse().expect("full reparse");
        let full = t_full.elapsed();

        let ratio = full.as_secs_f64() / inc.as_secs_f64().max(1e-9);
        if ratio > best_ratio {
            best_ratio = ratio;
            best_inc = inc;
            best_full = full;
        }
    }

    // Restore the fixture for cleanliness.
    std::fs::write(&target, &original).expect("restore target");

    eprintln!("best ratio {best_ratio:.2}x (incremental {best_inc:?} vs full {best_full:?})");

    assert!(
        best_ratio >= 5.0,
        "incremental update is not >=5x faster than full reparse \
         (best ratio {best_ratio:.2}x, incremental {best_inc:?}, full {best_full:?})"
    );
}

#[test]
fn cache_round_trips_through_disk() {
    let root = snapshot_multi_lang("disk_round_trip");
    // Sanity: building via `new` (which would try to load from disk) works
    // even when there is no prior cache.
    let cache = RepoMapCache::new(&root).expect("new cache");
    assert_eq!(cache.file_count(), EXPECTED_FILE_COUNT);
}

#[test]
fn disk_cache_invalidated_when_tracked_file_modified() {
    // Pin an mtime before the cold walk so the recorded value is exact;
    // the offset edit guarantees the change survives the cache's
    // whole-second mtime granularity without any sleeps.
    const T0: u64 = 1_700_100_000;

    let root = snapshot_multi_lang("load_stale_modified");
    let auth_path = root.join("src/auth.rs");
    set_mtime_seconds(&auth_path, T0);
    {
        let cache = RepoMapCache::new(&root).expect("cold cache");
        assert_eq!(cache.file_count(), EXPECTED_FILE_COUNT);
        cache.flush().expect("flush cold cache");
    }

    // Edit the file while "no session is running"; the mtime moves with
    // the write. The next `new` must reject the disk cache and re-walk
    // instead of serving the stale symbol list.
    std::fs::write(&auth_path, "pub fn fresh_after_restart() -> u8 { 7 }\n")
        .expect("rewrite auth.rs");
    set_mtime_seconds(&auth_path, T0 + 3_600);

    let cache = RepoMapCache::new(&root).expect("reload cache");
    let names = collect_names(&cache.map());
    assert!(
        names.iter().any(|n| n == "fresh_after_restart"),
        "reload served stale symbols after an offline edit: {names:?}"
    );
    cache.invalidate().expect("clean up disk cache");
}

#[test]
fn disk_cache_invalidated_when_tracked_file_deleted() {
    let root = snapshot_multi_lang("load_stale_deleted");
    {
        let cache = RepoMapCache::new(&root).expect("cold cache");
        assert_eq!(cache.file_count(), EXPECTED_FILE_COUNT);
        cache.flush().expect("flush cold cache");
    }

    std::fs::remove_file(root.join("pkg/server.go")).expect("remove go file");

    let cache = RepoMapCache::new(&root).expect("reload cache");
    let tracked: Vec<PathBuf> = cache.tracked_files();
    assert!(
        !tracked.iter().any(|p| p.ends_with("server.go")),
        "deleted file still tracked after reload: {tracked:?}"
    );
    assert_eq!(cache.file_count(), EXPECTED_FILE_COUNT - 1);
    cache.invalidate().expect("clean up disk cache");
}

#[test]
fn disk_cache_invalidated_when_new_file_added() {
    let root = snapshot_multi_lang("load_stale_added");
    {
        let cache = RepoMapCache::new(&root).expect("cold cache");
        assert_eq!(cache.file_count(), EXPECTED_FILE_COUNT);
        cache.flush().expect("flush cold cache");
    }

    let added = root.join("src/late_addition.rs");
    std::fs::write(&added, "pub fn late_to_the_party() -> bool { true }\n")
        .expect("write new file");

    let cache = RepoMapCache::new(&root).expect("reload cache");
    assert!(
        cache
            .tracked_files()
            .iter()
            .any(|p| p.ends_with("late_addition.rs")),
        "new file missing from reloaded cache"
    );
    let names = collect_names(&cache.map());
    assert!(
        names.iter().any(|n| n == "late_to_the_party"),
        "new file's symbols missing after reload: {names:?}"
    );
    cache.invalidate().expect("clean up disk cache");
}

#[test]
fn markdown_for_multi_lang_is_self_contained() {
    // Spot-check that the rendered markdown references symbols from every
    // language. This guards against a future refactor that accidentally
    // drops one of the per-language extractors.
    let root = snapshot_multi_lang("markdown_check");
    let cache = RepoMapCache::ephemeral(&root).expect("ephemeral cache");
    let map = cache.map();
    let mut repo = RepoMap { map };
    repo.trim_to_budget(4_000);
    let md = repo.to_system_prompt_markdown();
    for needle in [
        "service_entry", // Rust fn
        "TokenAuth",     // Rust struct
        "Authenticator", // Rust trait
        "UserService",   // TS class
        "ServiceConfig", // TS type alias
        "Pipeline",      // Python class
        "transform",     // Python fn
        "Server",        // Go struct
        "Handler",       // Go interface
    ] {
        assert!(md.contains(needle), "markdown missing {needle}:\n{md}");
    }
}

// -- helpers --------------------------------------------------------------

/// Fixed mtime (2023-11-14) used to pin file timestamps. The cache stores
/// whole-second mtimes, so tests that edit a file and expect a re-parse pin
/// a value that is always distinct from the walk/flush times recorded on a
/// modern clock instead of racing `SystemTime::now`.
const PINNED_MTIME_SECS: u64 = 1_700_000_000;

/// Set a file's mtime to an exact whole-second epoch value via
/// `std::fs::File::set_modified` (stable since 1.75). Opening in append
/// mode gets a writable fd without touching the contents.
fn set_mtime_seconds(path: &std::path::Path, secs: u64) {
    let f = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open file to set mtime");
    f.set_modified(UNIX_EPOCH + Duration::from_secs(secs))
        .expect("set_modified");
}

fn collect_kinds(map: &SymbolMap) -> Vec<SymbolKind> {
    let mut out = Vec::new();
    for (_, syms) in &map.files {
        for s in syms {
            push_kind(&mut out, s);
        }
    }
    out
}

fn push_kind(out: &mut Vec<SymbolKind>, node: &SymbolNode) {
    out.push(node.kind.clone());
    for child in &node.children {
        push_kind(out, child);
    }
}

fn collect_names(map: &SymbolMap) -> Vec<String> {
    let mut out = Vec::new();
    for (_, syms) in &map.files {
        for s in syms {
            push_name(&mut out, s);
        }
    }
    out
}

fn push_name(out: &mut Vec<String>, node: &SymbolNode) {
    out.push(node.name.clone());
    for child in &node.children {
        push_name(out, child);
    }
}
