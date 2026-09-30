//! R4-2 — real-binary smoke for `shannon providers export|import`.
//!
//! Companion to `providers_binary_smoke.rs` (which covers `providers add`):
//! these tests drive the actual `shannon` binary through the portability
//! story — export a snapshot of `~/.shannon/providers.toml`, wipe it, import
//! on a "fresh machine" (a new providers.toml), and verify the resolution is
//! reproduced through the same store surface the desktop reads. The
//! redaction guarantees (`--redact` masks credential references to
//! `"<redacted>"`; no secret values anywhere, ever) are pinned at grep level.
//!
//! Hermetic: every invocation points the spawned binary at its own tempdir
//! `HOME`; no test touches the real `~/.shannon/`.

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;

const BIN: &str = "shannon";

fn shannon(home: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin(BIN).unwrap();
    cmd.env("HOME", home);
    cmd
}

fn providers_toml(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".shannon").join("providers.toml")
}

/// `shannon list-providers --json` read back as `serde_json::Value` — the
/// same `{active, providers}` surface the desktop and scripts consume.
fn list_providers_json(home: &std::path::Path) -> serde_json::Value {
    let output = shannon(home)
        .args(["list-providers", "--json"])
        .output()
        .expect("list-providers runs");
    assert!(output.status.success(), "list-providers must succeed");
    serde_json::from_slice(&output.stdout).expect("valid JSON")
}

#[test]
fn export_wipe_import_round_trips_resolution() {
    let home = TempDir::new().expect("temp HOME");

    // 1. Add two providers with distinct credential service names.
    shannon(home.path())
        .args([
            "providers",
            "add",
            "glm",
            "--kind",
            "openai-compatible",
            "--base-url",
            "https://open.bigmodel.cn/v1",
            "--model",
            "glm-4.6",
            "--api-key-ref",
            "glm-custom-svc",
            "--tier",
            "standard",
        ])
        .assert()
        .success();
    shannon(home.path())
        .args([
            "providers",
            "add",
            "fallback-net",
            "--kind",
            "openai",
            "--model",
            "gpt-5-mini",
        ])
        .assert()
        .success();

    // 2. Export to a file. The snapshot is TOML, carries the schema marker,
    //    and never contains a secret-shaped value.
    let snap = home.path().join("providers-snapshot.toml");
    shannon(home.path())
        .args(["providers", "export", "--out"])
        .arg(&snap)
        .assert()
        .success();
    let text = std::fs::read_to_string(&snap).expect("snapshot written");
    assert!(
        text.contains("schema = \"shannon-providers-export/v1\""),
        "schema marker present:\n{text}"
    );
    assert!(
        text.contains("shannon providers import"),
        "header explains the import path:\n{text}"
    );
    assert!(
        !text.contains("sk-"),
        "no secret-shaped value anywhere in an export:\n{text}"
    );
    assert!(
        text.contains("service = \"glm-custom-svc\""),
        "credential REFERENCES travel (names, not values):\n{text}"
    );

    // 3. Wipe: the "fresh machine" has no providers.toml at all.
    std::fs::remove_file(providers_toml(home.path())).expect("wipe");

    // 4. Import reproduces the setup; the checklist flags the store
    //    credential that does not exist in this hermetic HOME.
    shannon(home.path())
        .args(["providers", "import"])
        .arg(&snap)
        .assert()
        .success()
        .stdout(contains("providers: 2 added, 0 replaced"))
        .stdout(contains(
            "active profile: default (from the snapshot (fresh machine))",
        ))
        .stdout(contains("[missing] default/glm"))
        .stdout(contains("store:glm-custom-svc"))
        .stdout(contains("[missing] default/fallback-net"))
        .stdout(contains("credential reference(s) need attention"));

    // 5. Same resolution, read through the store surface the desktop uses.
    let json = list_providers_json(home.path());
    let providers = json["providers"].as_array().expect("providers array");
    assert_eq!(providers.len(), 2, "both providers restored: {json}");
    let glm = providers
        .iter()
        .find(|p| p["id"] == "glm")
        .expect("glm restored");
    assert_eq!(glm["kind"], "OpenAICompat");
    assert_eq!(glm["base_url"], "https://open.bigmodel.cn/v1");
    assert_eq!(glm["credential_service"], "glm-custom-svc");
    assert_eq!(
        json["active"]["provider_id"], "fallback-net",
        "the last-added provider was active pre-wipe; the snapshot restores it"
    );
    assert_eq!(json["active"]["model_id"], "gpt-5-mini");

    // 6. Re-exporting the imported state produces the same payload modulo
    //    nothing — a true TOML-in/TOML-out round trip.
    let snap2 = home.path().join("providers-snapshot-2.toml");
    shannon(home.path())
        .args(["providers", "export", "--out"])
        .arg(&snap2)
        .assert()
        .success();
    let text2 = std::fs::read_to_string(&snap2).expect("second snapshot");
    assert_eq!(
        text, text2,
        "export(import(export(x))) must be a fixed point"
    );
}

#[test]
fn import_refuses_conflicts_then_force_replaces() {
    let home = TempDir::new().expect("temp HOME");

    shannon(home.path())
        .args([
            "providers",
            "add",
            "glm",
            "--kind",
            "openai-compatible",
            "--base-url",
            "https://old.example.com/v1",
            "--model",
            "old-model",
        ])
        .assert()
        .success();
    let snap = home.path().join("snap.toml");
    shannon(home.path())
        .args(["providers", "export", "--out"])
        .arg(&snap)
        .assert()
        .success();

    // Change the live provider so the snapshot conflicts with it.
    shannon(home.path())
        .args([
            "providers",
            "add",
            "glm",
            "--kind",
            "openai-compatible",
            "--base-url",
            "https://changed.example.com/v1",
            "--model",
            "changed-model",
        ])
        .assert()
        .success();

    // Default import: refuse and list the conflicting id.
    shannon(home.path())
        .args(["providers", "import"])
        .arg(&snap)
        .assert()
        .failure()
        .stderr(contains("already exist"))
        .stderr(contains("default/glm"))
        .stderr(contains("--force"));
    let json = list_providers_json(home.path());
    assert_eq!(
        json["providers"][0]["base_url"], "https://changed.example.com/v1",
        "refused import must not touch the live file: {json}"
    );

    // --force: the conflicting slot is replaced by the snapshot's version.
    shannon(home.path())
        .args(["providers", "import", "--force"])
        .arg(&snap)
        .assert()
        .success()
        .stdout(contains("providers: 0 added, 1 replaced"));
    let json = list_providers_json(home.path());
    assert_eq!(
        json["providers"][0]["base_url"],
        "https://old.example.com/v1"
    );
    assert_eq!(json["active"]["model_id"], "old-model");
}

#[test]
fn redacted_export_masks_refs_and_cannot_be_imported() {
    let home = TempDir::new().expect("temp HOME");

    shannon(home.path())
        .args([
            "providers",
            "add",
            "glm",
            "--kind",
            "openai-compatible",
            "--base-url",
            "https://open.bigmodel.cn/v1",
            "--model",
            "glm-4.6",
            "--api-key-ref",
            "glm-custom-svc",
        ])
        .assert()
        .success();

    let redacted_snap = home.path().join("redacted.toml");
    shannon(home.path())
        .args(["providers", "export", "--redact", "--out"])
        .arg(&redacted_snap)
        .assert()
        .success();
    let text = std::fs::read_to_string(&redacted_snap).expect("redacted snapshot");
    assert!(text.contains("redacted = true"), "{text}");
    assert!(text.contains("<redacted>"), "{text}");
    assert!(
        !text.contains("glm-custom-svc"),
        "credential reference details are masked:\n{text}"
    );
    assert!(
        text.contains("base_url = \"https://open.bigmodel.cn/v1\""),
        "non-credential fields still travel:\n{text}"
    );

    // A redacted snapshot is refused on import with the actionable reason.
    shannon(home.path())
        .args(["providers", "import"])
        .arg(&redacted_snap)
        .assert()
        .failure()
        .stderr(contains("--redact"))
        .stderr(contains("<redacted>"));
}

#[test]
fn import_rejects_foreign_files_with_actionable_errors() {
    let home = TempDir::new().expect("temp HOME");

    // A raw providers.toml copy is not an export (no schema marker).
    let raw = home.path().join("raw-providers.toml");
    std::fs::write(
        &raw,
        "version = 2\n\n[profiles.default]\nname = \"default\"\n",
    )
    .expect("write raw file");
    shannon(home.path())
        .args(["providers", "import"])
        .arg(&raw)
        .assert()
        .failure()
        .stderr(contains("not a usable Shannon provider export"))
        .stderr(contains("schema"));

    // Garbage is not TOML.
    let garbage = home.path().join("garbage.toml");
    std::fs::write(&garbage, "this is = not = toml ==").expect("write garbage");
    shannon(home.path())
        .args(["providers", "import"])
        .arg(&garbage)
        .assert()
        .failure()
        .stderr(contains("not a usable Shannon provider export"));

    // A missing file names the path.
    shannon(home.path())
        .args(["providers", "import", "/nonexistent/snapshot.toml"])
        .assert()
        .failure()
        .stderr(contains("cannot read '/nonexistent/snapshot.toml'"));
}
