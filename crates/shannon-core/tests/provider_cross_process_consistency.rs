//! Cross-writer consistency for `~/.shannon/providers.toml`
//! (ADR-0008 P2-5 S1-4).
//!
//! These tests pin the property the `ProviderConfigService` lock-then-reload
//! design is built to guarantee: **concurrent writers cannot lose each
//! other's updates.** Each writer — a separate `ProviderConfigService` over
//! the same file, modelling desktop + CLI or two CLI invocations in flight —
//! performs `lock → reload → mutate → save`, so it always composes on the
//! freshest committed state rather than the snapshot it loaded at
//! construction time.
//!
//! ## Why threads model processes
//!
//! `flock(2)` serializes by open-file-description, which is distinct per
//! `open()` call regardless of whether the callers share a process. So two
//! `ProviderConfigService` instances on the same thread (or two threads each
//! owning one) contend exactly the way two full processes would. The
//! thread-scoped tests here are therefore a faithful, fast, and non-flaky
//! stand-in for spawning real `shannon` child binaries.
//!
//! ## What is NOT tested here
//!
//! Atomic-rename integrity (no torn writes) is already guaranteed by
//! `ProviderConfigStore::save`'s temp-file + `rename` sequence and covered
//! by the unit tests in `provider_config_store`. This file covers the
//! higher-level R-M-W consistency that only lock-then-reload provides.

use std::collections::HashMap;

use shannon_core::provider_config_service::ProviderConfigService;
use shannon_core::provider_config_store::ProviderConfigStore;
use shannon_types::provider_config::{CredentialRef, ProviderKind, ProviderProfile, ProviderTiers};
use tempfile::TempDir;

/// Build a minimal, distinct `ProviderProfile` for writer `i`. Each writer
/// upserts its own id so the final file must hold all N ids when no updates
/// are lost.
fn profile_for(i: usize) -> ProviderProfile {
    ProviderProfile {
        id: format!("writer-{i}"),
        kind: ProviderKind::OpenAiCompatible,
        display_name: format!("writer {i}"),
        base_url: format!("https://example-{i}.invalid/v1"),
        models_url: None,
        credential: CredentialRef::Store {
            service: format!("writer-{i}"),
        },
        extra_headers: HashMap::new(),
        default_max_tokens: None,
        fallback_models: Vec::new(),
        quirks: Default::default(),
        tiers: ProviderTiers::default(),
    }
}

/// Count provider slots persisted in the `"default"` profile at `path`.
fn count_profiles(path: &std::path::Path) -> usize {
    ProviderConfigStore::load_or_default_at(path)
        .config()
        .profiles
        .get("default")
        .map(|p| p.providers.len())
        .unwrap_or(0)
}

/// The headline property: N writers each upserting a distinct profile
/// concurrently must all land on disk. Before lock-then-reload, each bare
/// mutation read its (stale) construction-time snapshot, added one profile,
/// and saved — so the last writer would win and clobber the rest, leaving a
/// single profile (a lost update). With `lock → reload → mutate → save`
/// inside every bare method, each writer re-reads inside the flock and the
/// upserts compose.
#[test]
fn concurrent_writers_do_not_lose_updates() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("providers.toml");
    const N: usize = 8;

    std::thread::scope(|s| {
        for i in 0..N {
            // Capture a shared ref so each scoped thread copies the ref
            // (shared refs are `Copy`) rather than moving the `PathBuf`,
            // leaving `path` owned by the test for the post-scope assert.
            let path_ref = &path;
            s.spawn(move || {
                let mut svc = ProviderConfigService::load_at(path_ref);
                svc.upsert(profile_for(i), &format!("model-{i}"), true)
                    .expect("upsert");
            });
        }
    });

    assert_eq!(
        count_profiles(&path),
        N,
        "lost update: expected {N} providers after {N} concurrent upserts"
    );
}

/// The reload mechanism in isolation. Writer B commits a profile while A's
/// in-memory snapshot is still the empty state A loaded before B wrote. A's
/// subsequent bare upsert must pick up B's commit (reload inside the flock)
/// and add its own profile on top — not overwrite B's with a stale empty
/// base.
#[test]
fn bare_upsert_picks_up_another_writers_commit() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("providers.toml");

    // A loads the empty file, then parks on a stale snapshot.
    let mut a = ProviderConfigService::load_at(&path);
    // B loads and commits profile 0 behind its own lock+reload.
    ProviderConfigService::load_at(&path)
        .upsert(profile_for(0), "model-0", true)
        .expect("B upsert");
    assert_eq!(count_profiles(&path), 1);

    // A's in-memory snapshot predates B's commit. A.upsert must reload
    // inside the flock and compose on B's state, not clobber it.
    a.upsert(profile_for(1), "model-1", true).expect("A upsert");

    assert_eq!(
        count_profiles(&path),
        2,
        "A's upsert lost B's commit — reload-inside-lock is broken"
    );
}

/// The explicit `LockedService` path (what the desktop's `configure()` arms
/// use): `lock` → `reload_locked` → mutate → drop. Must compose on fresh
/// state committed by a prior bare write.
#[test]
fn locked_service_path_composes_on_fresh_state() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("providers.toml");

    // Seed profile 0 via a bare upsert (its own lock+reload+save).
    ProviderConfigService::load_at(&path)
        .upsert(profile_for(0), "m0", true)
        .expect("seed upsert");

    let mut svc = ProviderConfigService::load_at(&path);
    {
        let mut locked = svc.lock().expect("lock");
        locked.reload_locked().expect("reload");
        locked
            .upsert(profile_for(1), "m1", true)
            .expect("locked upsert");
    }

    assert_eq!(count_profiles(&path), 2);
}

/// Dropping a `LockedService` must release the flock so the same service can
/// re-acquire it immediately — otherwise the RAII guard would self-deadlock
/// the second critical section.
#[test]
fn locked_service_drop_releases_the_flock() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("providers.toml");
    let mut svc = ProviderConfigService::load_at(&path);

    {
        let _locked = svc.lock().expect("first lock");
        // dropped here → flock released via the held `File`'s close.
    }

    // Re-acquire must succeed at once: no lingering lock, no deadlock.
    let _again = svc.lock().expect("second lock after drop");
}

// ────────────────────────────────────────────────────────────────────
// §C3 — CLI ↔ desktop surface parity matrix (ADR-0009 companion)
// ────────────────────────────────────────────────────────────────────
//
// The S1-4 tests above pin cross-writer *consistency* — N same-kind
// writers compose. This section pins **surface parity**: the CLI's bare
// `ProviderConfigService` write path (what `shannon providers …` and the
// REPL `/connect` drive) and the desktop's `LockedService` R-M-W path
// (what Tauri's `configure()` arms drive) produce **byte-identical
// on-disk state** for the same logical operation.
//
// Today both surfaces share one core mutation (`upsert_profile_with_active`,
// `disconnect_by_slug_unpersisted`, `ProviderConfigStore::set_active`), so
// parity is structural — the bare methods are literally `lock → reload →
// locked.<op>`. These tests guard against a future refactor that diverges
// the two compositions. See `docs/spikes/p2-2-c3-cli-desktop-parity-matrix.md`
// (rows 1-3 write parity + row 6 mixed-surface stress).

/// Assert two `providers.toml` files hold structurally identical state.
/// Used by every §C3 row to detect CLI↔desktop surface divergence.
fn assert_disk_state_eq(a: &std::path::Path, b: &std::path::Path) {
    let ca = ProviderConfigStore::load_or_default_at(a).config().clone();
    let cb = ProviderConfigStore::load_or_default_at(b).config().clone();
    assert_eq!(
        ca, cb,
        "CLI vs desktop surface divergence:\n  cli(a)     = {ca:#?}\n  desktop(b) = {cb:#?}"
    );
}

/// Row 1 — upsert (add provider). The CLI's bare `upsert` and the
/// desktop's `lock → reload_locked → LockedService::upsert` must persist
/// the same profile to the same on-disk shape.
#[test]
fn c3_upsert_cli_surface_matches_desktop_locked_surface() {
    let dir = TempDir::new().expect("temp dir");
    let cli_path = dir.path().join("cli.toml");
    let desk_path = dir.path().join("desk.toml");
    let profile = profile_for(0);

    // CLI surface: bare upsert (internally lock → reload → mutate → save).
    ProviderConfigService::load_at(&cli_path)
        .upsert(profile.clone(), "model-0", true)
        .expect("cli bare upsert");

    // Desktop surface: the explicit LockedService R-M-W that configure() arms use.
    {
        let mut svc = ProviderConfigService::load_at(&desk_path);
        let mut locked = svc.lock().expect("desktop lock");
        locked.reload_locked().expect("desktop reload");
        locked
            .upsert(profile.clone(), "model-0", true)
            .expect("desktop locked upsert");
    }

    assert_disk_state_eq(&cli_path, &desk_path);
    assert_eq!(count_profiles(&cli_path), 1);
}

/// Row 2 — disconnect (remove provider). After removing the same slot via
/// each surface, both files must agree (profile absent, active cleared).
#[test]
fn c3_disconnect_cli_surface_matches_desktop_locked_surface() {
    let dir = TempDir::new().expect("temp dir");
    let cli_path = dir.path().join("cli.toml");
    let desk_path = dir.path().join("desk.toml");
    let profile = profile_for(0);
    let slug = profile.id.clone();

    // Seed both files identically (seeding isn't under test — one surface for both).
    ProviderConfigService::load_at(&cli_path)
        .upsert(profile.clone(), "model-0", true)
        .expect("seed cli");
    ProviderConfigService::load_at(&desk_path)
        .upsert(profile.clone(), "model-0", true)
        .expect("seed desk");

    // CLI surface remove.
    ProviderConfigService::load_at(&cli_path)
        .disconnect_by_slug(&slug)
        .expect("cli disconnect");
    // Desktop surface remove.
    {
        let mut svc = ProviderConfigService::load_at(&desk_path);
        let mut locked = svc.lock().expect("desktop lock");
        locked.reload_locked().expect("desktop reload");
        locked
            .disconnect_by_slug(&slug)
            .expect("desktop disconnect");
    }

    assert_disk_state_eq(&cli_path, &desk_path);
    assert_eq!(count_profiles(&cli_path), 0);
    assert_eq!(count_profiles(&desk_path), 0);
}

/// Row 3 — set_active. Both surfaces must repoint `active_target` at the
/// same provider/model, leaving every other field untouched.
#[test]
fn c3_set_active_cli_surface_matches_desktop_locked_surface() {
    use shannon_core::LlmProvider;
    let dir = TempDir::new().expect("temp dir");
    let cli_path = dir.path().join("cli.toml");
    let desk_path = dir.path().join("desk.toml");

    // Seed both with two providers. The second connect (OpenAI) wins active,
    // so set_active(Anthropic) below actually moves the target — making the
    // parity assertion meaningful rather than a no-op.
    for path in [&cli_path, &desk_path] {
        let mut svc = ProviderConfigService::load_at(path);
        svc.connect(LlmProvider::Anthropic, None, None, true)
            .expect("seed anthropic");
        svc.connect(LlmProvider::OpenAI, None, None, true)
            .expect("seed openai");
    }

    // CLI surface: set active back to Anthropic.
    ProviderConfigService::load_at(&cli_path)
        .set_active(&LlmProvider::Anthropic, "claude-opus-4-8")
        .expect("cli set_active");
    // Desktop surface: same, via LockedService.
    {
        let mut svc = ProviderConfigService::load_at(&desk_path);
        let mut locked = svc.lock().expect("desktop lock");
        locked.reload_locked().expect("desktop reload");
        locked
            .set_active(&LlmProvider::Anthropic, "claude-opus-4-8")
            .expect("desktop set_active");
    }

    assert_disk_state_eq(&cli_path, &desk_path);
}

/// Row 6 — mixed-surface stress. Generalises `concurrent_writers_do_not_lose_updates`:
/// writers split across the bare-CLI and desktop-LockedService patterns must
/// all land on the shared file, proving the two surfaces compose under
/// contention (not just that each composes with itself).
#[test]
fn c3_mixed_surface_writers_do_not_lose_updates() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("providers.toml");
    const N: usize = 8;

    std::thread::scope(|s| {
        for i in 0..N {
            let path_ref = &path;
            s.spawn(move || {
                let mut svc = ProviderConfigService::load_at(path_ref);
                if i % 2 == 0 {
                    // Even writers — CLI bare surface.
                    svc.upsert(profile_for(i), &format!("model-{i}"), true)
                        .expect("cli bare upsert");
                } else {
                    // Odd writers — desktop LockedService surface.
                    let mut locked = svc.lock().expect("desktop lock");
                    locked.reload_locked().expect("desktop reload");
                    locked
                        .upsert(profile_for(i), &format!("model-{i}"), true)
                        .expect("desktop locked upsert");
                }
            });
        }
    });

    assert_eq!(
        count_profiles(&path),
        N,
        "mixed-surface lost update: expected {N} providers after {N} \
         writers split across CLI + desktop surfaces"
    );
}
