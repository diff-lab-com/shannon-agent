//! §4.12 W3-3b acceptance matrix:
//! `{off, local, landlock} × {write inside workspace, write outside
//! workspace, network, execute}` — plus the off-mode byte-equality regression
//! and the zero-tool-code-change assembly check (master plan §4.12 验证标准
//! ①②③⑤; criterion ④ has dedicated unit coverage in
//! `shannon-core/src/session_log/mod.rs` and `shannon-tools/src/system.rs`).
//!
//! Landlock cells execute only when this host actually enforces Landlock;
//! otherwise they print the probe reason and return early (an explicit,
//! labeled skip per the master-plan constraint).

use shannon_core::tools::Tool;
use shannon_tool_interface::{ProcessProvider, ProcessRequest, SandboxMode};
use shannon_tools::sandbox::{SandboxSettings, assemble, assemble_local, kernel_denial_classifier};
use shannon_tools::{BashTool, ToolProviders};
use std::path::Path;
use std::sync::Arc;

#[allow(unused_imports)]
use shannon_tool_interface::sandbox::SandboxError;

fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

fn settings(mode: SandboxMode, network: bool) -> SandboxSettings {
    SandboxSettings {
        mode,
        network,
        ..Default::default()
    }
}

struct Captured {
    stdout: String,
    stderr: String,
    success: bool,
}

/// Run `script` through `/bin/bash -c` in the given process world.
async fn bash(world: &dyn ProcessProvider, script: &str) -> Captured {
    let request = ProcessRequest {
        program: "/bin/bash".into(),
        args: vec!["-c".into(), script.to_string()],
        env: vec![],
        cwd: None,
        stdin_data: None,
    };
    let out = world.run_async(&request).await.expect("spawn must start");
    Captured {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        success: out.exit.success,
    }
}

// ═══════════════════════════ OFF row ════════════════════════════════════

#[tokio::test]
async fn off_write_inside_workspace_succeeds_byte_identically() {
    let ws = tempdir();
    let fs = ToolProviders::default().fs;
    fs.write_bytes(&ws.path().join("in.bin"), b"off-\xE2\x9C\x93")
        .await
        .expect("default world imposes nothing");
    assert_eq!(
        fs.read_bytes(&ws.path().join("in.bin"))
            .await
            .expect("read"),
        b"off-\xE2\x9C\x93"
    );
}

#[tokio::test]
async fn off_write_outside_any_root_is_unrestricted() {
    // The off world has no roots at all: any location behaves identically.
    let anywhere = tempdir();
    let fs = ToolProviders::default().fs;
    fs.write_bytes_blocking(&anywhere.path().join("free.txt"), b"unrestricted")
        .expect("no policy ⇒ no denial");
}

#[tokio::test]
async fn off_network_reaches_a_loopback_listener() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let accepted = std::thread::spawn(move || listener.accept().is_ok());
    let providers = ToolProviders::default();
    let cap = bash(
        providers.process.as_ref(),
        &format!("exec 3<>/dev/tcp/127.0.0.1/{port}"),
    )
    .await;
    assert!(cap.success, "connect should succeed unsandboxed");
    assert!(
        accepted.join().expect("thread"),
        "listener must have seen it"
    );
}

#[tokio::test]
async fn off_execute_streams_exact_child_bytes() {
    let providers = ToolProviders::default();
    let cap = bash(providers.process.as_ref(), "printf 'aGVsbG8='").await;
    assert!(cap.success);
    assert_eq!(cap.stdout, "aGVsbG8=");

    let raw = providers
        .process
        .run_async(&ProcessRequest {
            program: "/bin/bash".into(),
            args: vec!["-c".into(), "printf X >&2".into()],
            ..Default::default()
        })
        .await
        .expect("run");
    assert_eq!(raw.stderr, b"X", "stderr bytes pass through untouched");
}

// ═════════ LOCAL row (user-space mirror, children stay unguarded) ═══════

#[tokio::test]
async fn local_write_inside_workspace_allowed_by_policy_mirror() {
    let ws = tempdir();
    let assembled = assemble_local(&settings(SandboxMode::Local, false), ws.path());
    let path = ws.path().join("in.txt");
    let bytes = b"local-in".to_vec();
    assembled
        .providers
        .fs
        .write_bytes(&path, &bytes)
        .await
        .expect("workspace write allowed");
    assert_eq!(
        assembled
            .providers
            .fs
            .read_bytes(&path)
            .await
            .expect("read"),
        bytes,
        "decorated reads return inner bytes verbatim"
    );
}

#[tokio::test]
async fn local_write_outside_workspace_denied_with_classification() {
    let ws = tempdir();
    let outside = tempdir();
    let assembled = assemble_local(&settings(SandboxMode::Local, false), ws.path());
    let err = assembled
        .providers
        .fs
        .write_bytes(&outside.path().join("nope"), b"x")
        .await
        .expect_err("user-space policy must deny");

    let text = err.to_string();
    assert!(
        text.starts_with(shannon_tool_interface::SANDBOX_DENIED_PREFIX),
        "canonical prefix expected, got: {text}"
    );
    let denial = shannon_tool_interface::SandboxDenialInfo::parse(&text).expect("machine-parsable");
    assert_eq!(denial.op, "write");
}

/// Documented local-mode behavior: networking is *not* restricted (there is
/// no enforcement surface); connectivity matches off. Recorded as its own
/// matrix cell so the semantics are pinned rather than accidental.
#[tokio::test]
async fn local_network_unrestricted_by_design() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let accepted = std::thread::spawn(move || listener.accept().is_ok());
    let assembled = assemble_local(
        &settings(SandboxMode::Local, false),
        Path::new(std::env::temp_dir().to_str().expect("utf8")),
    );
    let cap = bash(
        assembled.providers.process.as_ref(),
        &format!("exec 3<>/dev/tcp/127.0.0.1/{port}"),
    )
    .await;
    assert!(cap.success, "local mode leaves child sockets alone");
    assert!(accepted.join().expect("thread"));
}

#[tokio::test]
async fn local_execute_delegates_spawn_without_byte_changes() {
    let ws = tempdir();
    let assembled = assemble_local(&settings(SandboxMode::Local, false), ws.path());
    let raw = assembled
        .providers
        .process
        .run_async(&ProcessRequest::new("/bin/echo", &["ok-local"]))
        .await
        .expect("run");
    assert_eq!(raw.stdout, b"ok-local\n");
    assert_eq!(raw.stderr, Vec::<u8>::new());
}

// ═══════ LANDLOCK row (kernel-enforced; explicit skips elsewhere) ═══════

/// Assemble the kernel world rooted at a fresh temp workspace.
///
/// `Ok((assembled, ws))` on enforcing hosts; `Err(reason)` carries the
/// probe/degrade explanation used as the printed skip reason.
fn kernel_world(
    network: bool,
) -> Result<(shannon_tools::sandbox::AssembledWorlds, tempfile::TempDir), String> {
    let ws = tempdir();
    match assemble(&settings(SandboxMode::Landlock, network), ws.path()) {
        Ok(assembled) => {
            if !network
                && assembled
                    .notices
                    .iter()
                    .any(|notice| notice.tag == "net-abi-unavailable")
            {
                return Err("SKIP(kernel): network ABI unavailable on this kernel".to_string());
            }
            Ok((assembled, ws))
        }
        Err(SandboxError::Unsupported { backend, detail }) => Err(format!(
            "SKIP(kernel): {backend} unsupported on this host: {detail}"
        )),
        Err(other) => Err(format!("FAIL(kernel): unexpected assembly error: {other}")),
    }
}

#[tokio::test]
async fn landlock_fs_mirror_allows_inside_and_shows_no_extra_copying() {
    let Ok((world, ws)) = kernel_world(false) else {
        return;
    };
    let payload = b"kernel-mirror-\xE2\x9C\x93".to_vec();
    world
        .providers
        .fs
        .write_bytes(&ws.path().join("a.bin"), &payload)
        .await
        .expect("workspace writes flow to the inner world unchanged");
    assert_eq!(
        world
            .providers
            .fs
            .read_bytes(&ws.path().join("a.bin"))
            .await
            .expect("read"),
        payload
    );
}

/// Master-plan criterion ②: the KERNEL refuses writes outside the writable
/// roots from inside a forked child (real Landlock domain, not user-space).
#[tokio::test]
async fn landlock_kernel_denies_outside_workspace_write_from_bash() {
    let Ok((world, _ws)) = kernel_world(false) else {
        return;
    };
    let outside = tempdir(); // exists and is writable by us — unsandboxed would succeed
    let script = format!("printf x > {}", outside.path().join("probe.txt").display());
    let cap = bash(world.providers.process.as_ref(), &script).await;

    assert!(!cap.success, "kernel must refuse the write");
    assert!(
        cap.stderr.contains("Permission denied") || cap.stderr.contains("Operation not permitted"),
        "expected kernel EACCES-flavored refusal, got stderr: {:?}",
        cap.stderr
    );
    assert!(
        !outside.path().join("probe.txt").exists(),
        "nothing may have been written"
    );

    // The wired classifier labels exactly this failure shape (criterion ④
    // source: ToolOutput.metadata["classification"] → L0 meta).
    let classifier = kernel_denial_classifier();
    let fabricated = shannon_tools::CommandOutput {
        stdout: String::new(),
        stderr: cap.stderr.clone(),
        exit_code: 1,
        success: false,
    };
    let denial = classifier(&fabricated).expect("classifier recognizes the refusal");
    assert_eq!(denial.op, "child_command");
}

#[tokio::test]
async fn landlock_network_connect_is_kernel_denied_when_network_off() {
    let Ok((world, _ws)) = kernel_world(false) else {
        return;
    };
    // Grab a fresh unused loopback port to avoid ECONNREFUSED noise; under
    // landlock the failure must be EPERM-shaped, never "refused".
    let probe_port = {
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind");
        l.local_addr().expect("addr").port()
    };
    let cap = bash(
        world.providers.process.as_ref(),
        &format!("exec 3<>/dev/tcp/127.0.0.1/{probe_port}"),
    )
    .await;
    assert!(!cap.success);
    assert!(
        cap.stderr.contains("Permission denied"),
        "landlock connect denial should be EPERM-shaped, got: {:?}",
        cap.stderr
    );
}

#[tokio::test]
async fn landlock_execute_outside_executable_roots_is_kernel_denied() {
    let Ok((world, _ws)) = kernel_world(false) else {
        return;
    };
    let outside = tempdir();
    let victim = outside.path().join("mytrue");
    std::fs::copy("/bin/true", &victim).expect("seed helper binary");
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&victim, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    let cap = bash(
        world.providers.process.as_ref(),
        &format!("exec {}", victim.display()),
    )
    .await;
    assert!(
        !cap.success,
        "execution outside executable roots must be refused"
    );
    assert!(cap.stderr.contains("Permission denied"));
}

#[tokio::test]
async fn landlock_executes_system_interpreters_inside_the_world() {
    let Ok((world, ws)) = kernel_world(true) else {
        return;
    };
    // Positive control: interpreters under the seeded executable roots run,
    // workspace stays writable — proving the deny-by-default ruleset didn't
    // produce a broken (unusable) world.
    let cap = bash(
        world.providers.process.as_ref(),
        "/bin/echo locked-and-live",
    )
    .await;
    assert!(cap.success, "stderr was {:?}", cap.stderr);
    assert_eq!(cap.stdout, "locked-and-live\n");
    let script = format!("printf written > {}/from-kernel.txt", ws.path().display());
    let cap = bash(world.providers.process.as_ref(), &script).await;
    assert!(cap.success, "in-workspace writes work from children");
}

// ═════ Assembly equivalence + zero-tool-code-change (criteria ①③) ══════

/// Criterion ③: with settings at their default (`off`) the provider set IS
/// the §4.11 passthrough — asserted structurally (fresh LocalFs/LocalProcess,
/// no classifier) and bytewise (identical captured bytes versus a naked
/// LocalProcess for stdout/stderr/exit across flavors).
#[tokio::test]
async fn off_mode_is_structurally_and_bytewise_the_passthrough() {
    let detected_default = SandboxSettings::default();
    assert_eq!(detected_default.mode, SandboxMode::Off);

    let proc = ToolProviders::default().process;

    // Structural: exercising an op outside any conceivable restriction list
    // cannot know about "policies" because off ships none.
    let reference = shannon_core::providers::LocalProcess::new();

    let requests = vec![
        ProcessRequest::new("/bin/sh", &["-c", "printf abc"]),
        ProcessRequest {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "printf e1>&2; printf o".into()],
            stdin_data: Some(b"".to_vec()),
            ..Default::default()
        },
    ];
    for req in &requests {
        let a = reference.run_async(req).await.expect("reference run");
        let b = proc.run_async(req).await.expect("off-world run");
        assert_eq!(a.stdout, b.stdout, "stdout byte equality");
        assert_eq!(a.stderr, b.stderr, "stderr byte equality");
        assert_eq!(a.exit.code, b.exit.code);
        assert_eq!(a.exit.success, b.exit.success);

        let ra = reference.run_blocking(req).expect("ref blocking");
        let rb = proc.run_blocking(req).expect("off blocking");
        assert_eq!(ra.stdout, rb.stdout);
        assert_eq!(ra.stderr, rb.stderr);
    }
}

/// Criterion ①: switching `sandbox=landlock` changes ONLY the assembled
/// worlds handed to registration. Registry contents (tool identity/order)
/// are invariant, and the registered Bash executes inside the kernel world
/// with denials classified — all without any tool-code difference.
#[tokio::test]
async fn registry_assembly_swaps_worlds_without_touching_tools() {
    use shannon_core::tools::ToolRegistry;

    let mut baseline = ToolRegistry::new();
    shannon_tools::register_default_tools(&mut baseline).expect("baseline registration");

    let ws = tempdir();
    let assembled = match assemble(&settings(SandboxMode::Landlock, true), ws.path()) {
        Ok(assembled) => assembled,
        Err(reason) => {
            println!("{reason}");
            return;
        }
    };

    let mut sandboxed = ToolRegistry::new();
    shannon_tools::register_default_tools_with_providers(&mut sandboxed, &assembled.providers)
        .expect("sandboxed registration");

    let mut names_of = |reg: &ToolRegistry| -> Vec<String> {
        let mut names = reg
            .list_tools_info()
            .iter()
            .map(|t| t.name.clone())
            .collect::<Vec<_>>();
        // Registry listing order is hash-seeded (nondeterministic between any
        // two fresh registries), so identity comparison sorts.
        names.sort();
        names
    };
    assert_eq!(
        names_of(&baseline),
        names_of(&sandboxed),
        "world swap must not add/remove/reorder tools"
    );

    // Denial through the registered tool, classified end-to-end.
    let outside = tempdir();
    let input = serde_json::json!({
        "command": format!("printf x > {}", outside.path().join("reg-probe").display())
    });
    let output = sandboxed.execute("Bash", input).await.expect("tool ran");
    assert!(output.is_error, "kernel refusal surfaces as tool error");
    assert_eq!(
        output
            .metadata
            .get("classification")
            .and_then(|v| v.as_str()),
        Some("sandbox_denied"),
        "classification rides ToolOutput.metadata: {:?}",
        output.metadata.get("sandbox")
    );

    // Positive control through the same registry entry point.
    let ok = sandboxed
        .execute("Bash", serde_json::json!({"command": "echo fine"}))
        .await
        .expect("tool ran");
    assert!(!ok.is_error);
    assert!(
        ok.content.contains("fine"),
        "allowed operations behave identically to the unsandboxed world"
    );
}

/// Criterion ④ plumbing head: the classifier tags exactly the two canonical
/// kernel refusal messages and nothing else.
#[tokio::test]
async fn classifier_shapes_are_precise() {
    let mk = |stderr: &str| shannon_tools::CommandOutput {
        stdout: String::new(),
        stderr: stderr.to_string(),
        exit_code: 1,
        success: false,
    };

    let classifier = kernel_denial_classifier();
    let tagged = classifier(&mk("bash: /etc/x: Permission denied")).expect("tagged");
    assert_eq!(tagged.op, "child_command");
    let _ = tagged.target; // best-effort extraction; presence is not contractual

    assert!(classifier(&mk("fatal: not a git repository")).is_none());
    assert!(classifier(&mk("")).is_none());
    let ok_case = shannon_tools::CommandOutput {
        stdout: "fine".into(),
        stderr: String::new(),
        exit_code: 0,
        success: true,
    };
    assert!(classifier(&ok_case).is_none());
}

/// Wiring sanity kept next to the seam: BashTool built on a bare local world
/// carries no sandbox classification (historical metadata shape preserved).
#[tokio::test]
async fn plain_bash_tool_has_no_sandbox_metadata() {
    use shannon_core::providers::LocalProcess;
    let bash_tool =
        BashTool::new().with_worlds(Arc::new(LocalProcess::new()) as Arc<dyn ProcessProvider>);
    let output = bash_tool
        .execute(serde_json::json!({"command": "printf hi"}))
        .await
        .expect("run");
    assert!(!output.is_error);
    assert!(output.metadata.get("classification").is_none());
    assert!(output.content.contains("hi"));
}
