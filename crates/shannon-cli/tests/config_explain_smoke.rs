//! R4-4a — real-binary smoke for `shannon config --explain <key>`.
//!
//! Drives the actual `shannon` binary against hermetic layered config files
//! (a tempdir `HOME` holding `~/.shannon/config.toml` + a connected
//! `~/.shannon/providers.toml`, a fresh cwd for the project layer, and
//! `SHANNON_*` vars set per-invocation) and asserts the human-readable
//! output names the right winning layer, lists the right losers, and lists
//! the known keys for an unknown key.

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::TempDir;

const BIN: &str = "shannon";

/// A shannon invocation pinned to a hermetic `HOME` + empty cwd, with the
/// `SHANNON_*` vars these tests care about cleared so the machine's
/// environment cannot leak into the layer ladder.
fn shannon(home: &std::path::Path, cwd: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin(BIN).unwrap();
    cmd.env("HOME", home)
        .current_dir(cwd)
        .env_remove("SHANNON_MAX_TOKENS")
        .env_remove("SHANNON_TEMPERATURE")
        .env_remove("SHANNON_TIMEOUT")
        .env_remove("SHANNON_DEBUG")
        .env_remove("SHANNON_MODEL")
        .env_remove("SHANNON_PROVIDER")
        .env_remove("SHANNON_BASE_URL")
        .env_remove("SHANNON_ENABLE_TOOLS")
        .env_remove("SHANNON_MAX_CONTEXT_TOKENS")
        .env_remove("SHANNON_PERMISSION_PROFILE");
    cmd
}

fn write_global_config(home: &std::path::Path, body: &str) {
    let dir = home.join(".shannon");
    std::fs::create_dir_all(&dir).expect("mkdir ~/.shannon");
    std::fs::write(dir.join("config.toml"), body).expect("write config.toml");
}

/// Minimal connected providers.toml (the shape the engine/store write).
fn write_connected(home: &std::path::Path) {
    let dir = home.join(".shannon");
    std::fs::create_dir_all(&dir).expect("mkdir ~/.shannon");
    std::fs::write(
        dir.join("providers.toml"),
        r#"version = 2

[profiles.default]
name = "default"

[profiles.default.active_target]
provider_id = "glm"
model_id = "glm-4.6"
scope = "global"

[[profiles.default.providers]]
id = "glm"
kind = "openai-compatible"
display_name = "glm"
base_url = "https://open.bigmodel.cn/v1"

[profiles.default.providers.credential]
backend = "store"
service = "glm"
"#,
    )
    .expect("write providers.toml");
}

#[test]
fn global_layer_wins_when_nothing_higher_defines_the_key() {
    let home = TempDir::new().expect("temp HOME");
    let cwd = TempDir::new().expect("temp cwd");
    write_global_config(home.path(), "max_tokens = 1234\n");

    shannon(home.path(), cwd.path())
        .args(["config", "--explain", "max_tokens"])
        .assert()
        .success()
        .stdout(contains("key: max_tokens"))
        .stdout(contains("user-global — "))
        .stdout(contains(".shannon/config.toml"))
        .stdout(contains(": 1234"))
        .stdout(contains("winner: user-global"))
        .stdout(contains("change it:"))
        .stdout(contains("shannon config max_tokens="));
}

#[test]
fn env_layer_beats_global_and_the_loser_is_still_listed() {
    let home = TempDir::new().expect("temp HOME");
    let cwd = TempDir::new().expect("temp cwd");
    write_global_config(home.path(), "max_tokens = 1234\n");

    shannon(home.path(), cwd.path())
        .args(["config", "--explain", "max_tokens"])
        .env("SHANNON_MAX_TOKENS", "8192")
        .assert()
        .success()
        .stdout(contains("user-global — "))
        .stdout(contains(": 1234"))
        .stdout(contains("env-vars — SHANNON_MAX_TOKENS: 8192"))
        .stdout(contains("winner: env-vars (SHANNON_MAX_TOKENS) = 8192"));
}

#[test]
fn connected_layer_answers_model_and_provider() {
    let home = TempDir::new().expect("temp HOME");
    let cwd = TempDir::new().expect("temp cwd");
    write_connected(home.path());

    shannon(home.path(), cwd.path())
        .args(["config", "--explain", "model"])
        .assert()
        .success()
        .stdout(contains("connected — "))
        .stdout(contains(".shannon/providers.toml"))
        .stdout(contains("glm-4.6"))
        .stdout(contains("winner: connected"))
        .stdout(contains("/model"));

    shannon(home.path(), cwd.path())
        .args(["config", "--explain", "provider"])
        .assert()
        .success()
        .stdout(contains("winner: connected"))
        .stdout(contains(": glm"));

    shannon(home.path(), cwd.path())
        .args(["config", "--explain", "base_url"])
        .assert()
        .success()
        .stdout(contains("https://open.bigmodel.cn/v1"));
}

#[test]
fn connected_layer_beats_env_model_the_connect_contract() {
    let home = TempDir::new().expect("temp HOME");
    let cwd = TempDir::new().expect("temp cwd");
    write_connected(home.path());

    // Engine precedence: connected (providers.toml) > SHANNON_* env —
    // `/connect` works without env vars. The env value is still listed as a
    // losing layer.
    shannon(home.path(), cwd.path())
        .args(["config", "--explain", "model"])
        .env("SHANNON_MODEL", "gpt-4o")
        .assert()
        .success()
        .stdout(contains("env-vars — SHANNON_MODEL: gpt-4o"))
        .stdout(contains("connected — "))
        .stdout(contains("glm-4.6"))
        .stdout(contains("winner: connected"));
}

#[test]
fn unset_key_reports_the_runtime_default_and_still_hints() {
    let home = TempDir::new().expect("temp HOME");
    let cwd = TempDir::new().expect("temp cwd");

    shannon(home.path(), cwd.path())
        .args(["config", "--explain", "temperature"])
        .assert()
        .success()
        .stdout(contains("no config layer defines temperature"))
        .stdout(contains("SHANNON_TEMPERATURE"));
}

#[test]
fn unknown_key_lists_every_known_key() {
    let home = TempDir::new().expect("temp HOME");
    let cwd = TempDir::new().expect("temp cwd");

    shannon(home.path(), cwd.path())
        .args(["config", "--explain", "modle"])
        .assert()
        .success()
        .stdout(contains("unknown config key: 'modle'"))
        .stdout(contains("max_tokens"))
        .stdout(contains("temperature"))
        .stdout(contains("provider_model"))
        .stdout(contains("--dump-config"));
}

#[test]
fn secret_shaped_key_gets_the_credentials_answer() {
    let home = TempDir::new().expect("temp HOME");
    let cwd = TempDir::new().expect("temp cwd");

    shannon(home.path(), cwd.path())
        .args(["config", "--explain", "anthropic_api_key"])
        .assert()
        .success()
        .stdout(contains("decision A1"))
        .stdout(contains("credential store"))
        .stdout(contains("/connect"));
}

#[test]
fn explain_conflicts_with_setting_at_parse_time() {
    let home = TempDir::new().expect("temp HOME");
    let cwd = TempDir::new().expect("temp cwd");

    shannon(home.path(), cwd.path())
        .args(["config", "--explain", "model", "--setting", "model"])
        .assert()
        .failure();
}
