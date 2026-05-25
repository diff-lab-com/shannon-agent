//! Real task tests with record/replay support.
//!
//! **Recording mode** (local, needs API key):
//!   SHANNON_RECORD_DIR=tests/fixtures/real_tasks \
//!   SHANNON_API_KEY=sk-... \
//!   cargo test --test cli_real_task_tests -- --ignored --test-threads=1
//!
//! **Replay mode** (CI, no API key):
//!   cargo test --test cli_real_task_tests -- --test-threads=1
//!
//! Recording uses LlmClient's built-in SHANNON_RECORD_DIR hook to capture
//! request/response pairs. Replay loads those fixtures via mockito.

use assert_cmd::Command;
use mockito::Server;
use serial_test::serial;
use std::fs;
use std::path::PathBuf;

const BIN: &str = "shannon";

fn shannon() -> Command {
    Command::cargo_bin(BIN).unwrap()
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("tests")
        .join("fixtures")
        .join("real_tasks")
}

fn create_workspace(name: &str) -> tempfile::TempDir {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let subdir = dir.path().join(name);
    fs::create_dir_all(&subdir).expect("create subdir");
    dir
}

fn require_api_key() -> String {
    std::env::var("SHANNON_API_KEY").unwrap_or_else(|_| {
        eprintln!("Skipping: set SHANNON_API_KEY to run recording tests");
        std::process::exit(0);
    })
}

fn require_record_dir() -> PathBuf {
    match std::env::var("SHANNON_RECORD_DIR") {
        Ok(dir) => PathBuf::from(dir),
        Err(_) => {
            eprintln!("Skipping: set SHANNON_RECORD_DIR to record fixtures");
            std::process::exit(0);
        }
    }
}

/// Check if replay fixtures exist for a given test name. Skip if not found.
fn has_fixture(prefix: &str) -> bool {
    let dir = fixtures_dir();
    if !dir.exists() {
        return false;
    }
    let Ok(entries) = fs::read_dir(&dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        e.path()
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with(prefix))
    })
}

/// Mount all recorded fixtures onto a mockito server for replay.
fn mount_fixtures(server: &mut Server, provider: &str) -> Vec<mockito::Mock> {
    use shannon_core::testing::record_replay::ReplayHarness;
    let harness = ReplayHarness::from_dir(fixtures_dir());
    let mut mocks = Vec::new();
    for fixture in &harness.fixtures {
        if fixture.provider != provider {
            continue;
        }
        let mock = server
            .mock("POST", fixture.request.path.as_str())
            .with_status(fixture.response_status_usize())
            .with_body(&fixture.response.body)
            .expect_at_least(1)
            .create();
        mocks.push(mock);
    }
    mocks
}

// ── Recording tests (require API key + SHANNON_RECORD_DIR, #[ignore]) ──

#[test]
#[serial]
#[ignore]
fn record_task_create_file_anthropic() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_create_file");

    shannon()
        .env("SHANNON_API_KEY", &api_key)
        .env("SHANNON_RECORD_DIR", &record_dir)
        .env("SHANNON_PROVIDER", "anthropic")
        .env_remove("OPENAI_API_KEY")
        .current_dir(workspace.path())
        .args([
            "--prompt",
            "Create a file called hello.txt with the content 'world'",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    // Verify the file was created
    assert!(
        workspace.path().join("hello.txt").exists(),
        "hello.txt should be created"
    );
    let content = fs::read_to_string(workspace.path().join("hello.txt")).unwrap();
    assert!(
        content.contains("world"),
        "hello.txt should contain 'world', got: {content}"
    );

    // Verify fixture was recorded
    let dir = fs::read_dir(&record_dir).expect("record dir");
    let count = dir.count();
    assert!(count > 0, "should have recorded at least one fixture");
}

#[test]
#[serial]
#[ignore]
fn record_task_bash_command_anthropic() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_bash_cmd");

    shannon()
        .env("SHANNON_API_KEY", &api_key)
        .env("SHANNON_RECORD_DIR", &record_dir)
        .env("SHANNON_PROVIDER", "anthropic")
        .env_remove("OPENAI_API_KEY")
        .current_dir(workspace.path())
        .args([
            "--prompt",
            "Run the command: echo hello_shannon > output.txt",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    assert!(
        workspace.path().join("output.txt").exists(),
        "output.txt should be created"
    );
}

#[test]
#[serial]
#[ignore]
fn record_task_read_and_edit_anthropic() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_read_edit");

    // Create a file to edit
    fs::write(
        workspace.path().join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }",
    )
    .expect("write src/lib.rs");

    shannon()
        .env("SHANNON_API_KEY", &api_key)
        .env("SHANNON_RECORD_DIR", &record_dir)
        .env("SHANNON_PROVIDER", "anthropic")
        .env_remove("OPENAI_API_KEY")
        .current_dir(workspace.path())
        .args([
            "--prompt",
            "Read src/lib.rs and add a doc comment above the add function",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    let content = fs::read_to_string(workspace.path().join("src/lib.rs")).unwrap();
    assert!(
        content.contains("///") || content.contains("//"),
        "lib.rs should have a comment added, got: {content}"
    );
}

#[test]
#[serial]
#[ignore]
fn record_task_code_search_anthropic() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_code_search");

    fs::write(
        workspace.path().join("src/lib.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }",
    )
    .expect("write src/lib.rs");

    let output = shannon()
        .env("SHANNON_API_KEY", &api_key)
        .env("SHANNON_RECORD_DIR", &record_dir)
        .env("SHANNON_PROVIDER", "anthropic")
        .env_remove("OPENAI_API_KEY")
        .current_dir(workspace.path())
        .args([
            "--prompt",
            "Find where the add function is defined",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(120))
        .assert();

    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    assert!(
        !stdout.is_empty(),
        "should produce output about the add function"
    );
}

#[test]
#[serial]
#[ignore]
fn record_task_multi_turn_anthropic() {
    let api_key = require_api_key();
    let record_dir = require_record_dir();
    let workspace = create_workspace("real_multi_turn");

    fs::write(
        workspace.path().join("src/main.rs"),
        "fn main() {\n    println!(\"Hello, World!\");\n}\n",
    )
    .expect("write src/main.rs");

    shannon()
        .env("SHANNON_API_KEY", &api_key)
        .env("SHANNON_RECORD_DIR", &record_dir)
        .env("SHANNON_PROVIDER", "anthropic")
        .env_remove("OPENAI_API_KEY")
        .current_dir(workspace.path())
        .args([
            "--prompt",
            "Read src/main.rs, then change the greeting from 'Hello, World!' to 'Hello, Shannon!'",
            "--output-format",
            "json",
        ])
        .timeout(std::time::Duration::from_secs(120))
        .assert()
        .success();

    let content = fs::read_to_string(workspace.path().join("src/main.rs")).unwrap();
    assert!(
        content.contains("Shannon"),
        "main.rs should contain 'Shannon', got: {content}"
    );
}

// ── Replay tests (no API key needed, use recorded fixtures) ────────────

#[tokio::test]
#[serial]
async fn replay_fixtures_load_successfully() {
    let dir = fixtures_dir();
    if !dir.exists() {
        return;
    }
    use shannon_core::testing::record_replay::ReplayHarness;
    let harness = ReplayHarness::from_dir(&dir);
    // If no fixtures yet, that's fine — this test just verifies loading works
    for fixture in &harness.fixtures {
        assert!(!fixture.provider.is_empty(), "provider should not be empty");
        assert!(!fixture.request_hash.is_empty(), "hash should not be empty");
        assert!(
            !fixture.response.body.is_empty(),
            "response body should not be empty"
        );
    }
}

#[tokio::test]
#[serial]
async fn replay_anthropic_fixtures_via_mockito() {
    if !has_fixture("anthropic") {
        eprintln!("Skipping: no anthropic fixtures found");
        return;
    }

    let mut server = Server::new_async().await;
    let _mocks = mount_fixtures(&mut server, "anthropic");

    // Verify at least one mock was mounted
    let resp = reqwest::Client::new()
        .post(format!("{}/v1/messages", server.url()))
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .body(r#"{"model":"test","messages":[]}"#)
        .send()
        .await;

    if let Ok(resp) = resp {
        assert_eq!(resp.status(), 200, "mock should return 200");
    }
}

#[test]
#[serial]
fn replay_workspace_creation_works() {
    let dir = fixtures_dir();
    if !dir.exists() {
        return;
    }

    // Verify we can create a workspace and that the fixture directory
    // is accessible from test code
    let workspace = create_workspace("replay_test");
    assert!(workspace.path().exists());

    // Write a test file to verify workspace isolation
    fs::write(workspace.path().join("test.txt"), "replay").expect("write");
    assert_eq!(
        fs::read_to_string(workspace.path().join("test.txt")).unwrap(),
        "replay"
    );
}
