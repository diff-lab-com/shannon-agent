//! C3-d — real-binary CLI provider-config smoke (ADR-0009 companion to the
//! C3 parity matrix). See `docs/spikes/p2-2-c3-cli-desktop-parity-matrix.md`
//! §3.1 ("In-process first, real-binary later").
//!
//! The in-process matrix (`crates/shannon-core/tests/provider_cross_process_
//! consistency.rs`, rows 1-6) pins CLI↔desktop on-disk parity by driving the
//! Rust API directly. These tests cover the wiring layer the matrix cannot
//! reach: **clap arg parse → `run_providers_add` →
//! `ProviderConfigService::upsert` → disk**, through the REAL `shannon`
//! binary. They prove a provider added at the CLI surface lands as a profile
//! the desktop's `ProviderConfigStore` can read back, with exactly the fields
//! the command-line flags imply.
//!
//! Hermetic: each test points the spawned binary at its own tempdir `HOME`,
//! so no test touches the real `~/.shannon/`. The spawned command itself
//! returns in well under a second — no network, and no MCP/skills/hooks init
//! (the `providers` subcommand takes the `CliConfig::default()` fast path in
//! `main.rs`, which short-circuits heavy app-state loading).

use assert_cmd::Command;
use shannon_core::provider_config_store::ProviderConfigStore;
use shannon_types::provider_config::{CredentialRef, ProviderKind, ProviderTiers};
use tempfile::TempDir;

const BIN: &str = "shannon";

/// `~/.shannon/providers.toml` for the given isolated HOME.
fn providers_toml(home: &std::path::Path) -> std::path::PathBuf {
    home.join(".shannon").join("providers.toml")
}

/// Read the binary's on-disk write back through the SAME store the desktop +
/// engine read — the cross-surface contract under test (CLI binary write must
/// be legible to the desktop's store). Returns the cloned top-level config.
fn load_written_config(
    home: &std::path::Path,
) -> shannon_types::provider_config::ProviderModelConfig {
    ProviderConfigStore::load_or_default_at(&providers_toml(home))
        .config()
        .clone()
}

/// Row C3-d (1/2) — minimal happy path. `shannon providers add` with just
/// `--kind` + `--model` must boot the real binary, default the anthropic
/// `base_url`, persist a `CredentialRef::Store` keyed by the provider id, and
/// pin the new provider as active — all legible to the desktop's store.
#[test]
fn providers_add_anthropic_defaults_to_desktop_readable_profile() {
    let home = TempDir::new().expect("temp HOME");
    Command::cargo_bin(BIN)
        .unwrap()
        .args([
            "providers",
            "add",
            "smoke-cli",
            "--kind",
            "anthropic",
            "--model",
            "claude-opus-4-8",
        ])
        .env("HOME", home.path())
        .assert()
        .success();

    let cfg = load_written_config(home.path());
    let default = cfg
        .profiles
        .get("default")
        .expect("default profile persisted");
    assert_eq!(default.providers.len(), 1, "exactly one provider persisted");
    let p = &default.providers[0];
    assert_eq!(p.id, "smoke-cli");
    assert_eq!(p.kind, ProviderKind::Anthropic, "--kind wired to disk");
    assert_eq!(
        p.base_url, "https://api.anthropic.com",
        "canonical anthropic base_url defaulted (no --base-url supplied)"
    );
    assert_eq!(
        p.credential,
        CredentialRef::Store {
            service: "smoke-cli".into()
        },
        "credential service defaults to the provider id"
    );
    assert_eq!(
        default.active_target.provider_id, "smoke-cli",
        "new provider always becomes active (make_active = true)"
    );
    assert_eq!(
        default.active_target.model_id, "claude-opus-4-8",
        "--model wired to the active target"
    );
}

/// Row C3-d (2/2) — full flag surface. Every `providers add` flag must land
/// in the right on-disk field: explicit `--base-url`, `--api-key-ref`
/// overriding the id-derived service, `--extra-header KEY=VALUE`, `--tier pro`
/// writing the model into the `pro` slot, and `--set-active` parsing without
/// error (a documented no-op since the new provider is always made active).
#[test]
fn providers_add_openai_compatible_wires_every_flag_to_disk() {
    let home = TempDir::new().expect("temp HOME");
    Command::cargo_bin(BIN)
        .unwrap()
        .args([
            "providers",
            "add",
            "wire-all",
            "--kind",
            "openai-compatible",
            "--base-url",
            "https://example.invalid/v1",
            "--model",
            "m1",
            "--api-key-ref",
            "mysvc",
            "--tier",
            "pro",
            "--extra-header",
            "X-Team=alpha",
            "--set-active",
        ])
        .env("HOME", home.path())
        .assert()
        .success();

    let cfg = load_written_config(home.path());
    let default = cfg
        .profiles
        .get("default")
        .expect("default profile persisted");
    let p = default
        .providers
        .iter()
        .find(|p| p.id == "wire-all")
        .unwrap_or_else(|| {
            panic!(
                "wire-all provider persisted, got ids {:?}",
                default
                    .providers
                    .iter()
                    .map(|p| p.id.as_str())
                    .collect::<Vec<_>>()
            )
        });

    assert_eq!(
        p.kind,
        ProviderKind::OpenAiCompatible,
        "--kind openai-compatible wired"
    );
    assert_eq!(
        p.base_url, "https://example.invalid/v1",
        "--base-url wired verbatim"
    );
    assert_eq!(
        p.credential,
        CredentialRef::Store {
            service: "mysvc".into()
        },
        "--api-key-ref overrides the id-derived service"
    );
    assert_eq!(
        p.extra_headers.get("X-Team").map(String::as_str),
        Some("alpha"),
        "--extra-header KEY=VALUE wired (value trimmed)"
    );
    assert_eq!(
        p.tiers,
        ProviderTiers {
            pro: Some("m1".into()),
            ..Default::default()
        },
        "--tier pro writes the model into the pro slot only (others stay None)"
    );
    assert_eq!(
        p.display_name, "wire-all",
        "display_name defaults to the provider id"
    );
    assert_eq!(
        default.active_target.provider_id, "wire-all",
        "active pinned to the new provider"
    );
    assert_eq!(
        default.active_target.model_id, "m1",
        "--model wired to the active target"
    );
}
