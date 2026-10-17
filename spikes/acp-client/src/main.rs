//! ACP subagent spike client (Task 4, docs/plans/2026-10-17-followup-implementation-tasks.md PR-3).
//!
//! Minimal ACP (Agent Client Protocol) client over stdio:
//!   spawn bridge -> initialize -> session/new -> session/prompt -> stream session/update -> stopReason.
//!
//! Credential isolation: the bridge subprocess is spawned with a scrubbed
//! (allow-list) environment. Claude Code authenticates from its OWN config
//! (~/.claude/settings.json / OS keychain); Shannon's credential chain is
//! neither passed down nor readable by the child. `--poison` plants fake
//! credentials in the parent process to prove they are stripped; with
//! `--record-env` we also read /proc/<pid>/environ of the bridge to prove the
//! child never saw them.
//!
//! Code quality is irrelevant (spike); the report is the deliverable.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::Write as _;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

/// Environment variables the bridge subprocess is allowed to inherit.
/// Deliberately tight: NOTHING matching *KEY*/*TOKEN*/*SECRET*/*ANTHROPIC*/*SHANNON*
/// can pass, because it is not in the allow-list.
const ENV_ALLOWLIST: &[&str] = &[
    "PATH", "HOME", "SHELL", "USER", "LOGNAME", "LANG", "LC_ALL", "TERM", "TMPDIR",
    "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME",
];

fn scrubbed_env() -> HashMap<String, String> {
    let mut env = HashMap::new();
    for k in ENV_ALLOWLIST {
        if let Ok(v) = std::env::var(k) {
            env.insert((*k).to_string(), v);
        }
    }
    env
}

struct Logger(Arc<Mutex<std::io::BufWriter<std::fs::File>>>);

impl Logger {
    fn log(&self, dir: &str, line: &str) {
        if let Ok(mut w) = self.0.lock() {
            let _ = writeln!(w, "{dir}{line}");
            let _ = w.flush();
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut adapter = "./node_modules/.bin/claude-agent-acp".to_string();
    let mut cwd = std::env::var("PWD").unwrap_or_else(|_| ".".into());
    let mut prompt = "Reply with exactly one line of JSON: {\"ok\": true, \"answer\": <the answer to 2+2>} and nothing else.".to_string();
    let mut log_path = String::from("logs/run.jsonl");
    let mut env_record = String::from("logs/child-env.txt");
    let mut poison = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--adapter" => adapter = args.next().unwrap(),
            "--cwd" => cwd = args.next().unwrap(),
            "--prompt" => prompt = args.next().unwrap(),
            "--log" => log_path = args.next().unwrap(),
            "--record-env" => env_record = args.next().unwrap(),
            "--poison" => poison = true,
            other => bail!("unknown arg {other}"),
        }
    }

    std::fs::create_dir_all("logs")?;

    if poison {
        // Simulate Shannon's own credential chain being present in the parent
        // process. The scrubber must ensure none of these reach the bridge.
        // SAFETY: single-threaded setup phase of a spike binary.
        unsafe {
            std::env::set_var("SHANNON_LLM_API_KEY", "sk-LEAK-shannon-credential");
            std::env::set_var("ANTHROPIC_API_KEY", "sk-LEAK-shannon-injected");
            std::env::set_var("ANTHROPIC_AUTH_TOKEN", "sk-LEAK-shannon-injected");
            std::env::set_var("OPENAI_API_KEY", "sk-LEAK-shannon-injected");
        }
    }

    let env = scrubbed_env();

    println!("[client] spawning adapter: {adapter}");
    let mut child = Command::new(&adapter)
        .env_clear()
        .envs(&env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("spawn {adapter}"))?;
    let pid = child.id();

    // ---- evidence: dump the env we handed to the child ----
    {
        let mut f = std::io::BufWriter::new(std::fs::File::create(&env_record)?);
        writeln!(f, "# env passed to adapter subprocess (keys only unless *_LEAK marker)")?;
        let mut keys: Vec<&String> = env.keys().collect();
        keys.sort();
        for k in keys {
            writeln!(f, "{k}")?;
        }
        // Stronger proof on Linux: the child's actual /proc environ.
        if let Some(p) = pid {
            let path = format!("/proc/{p}/environ");
            if let Ok(raw) = std::fs::read(&path) {
                let vars: Vec<String> = raw
                    .split(|b| *b == 0)
                    .filter_map(|s| String::from_utf8(s.to_vec()).ok())
                    .collect();
                writeln!(f, "\n# /proc/{p}/environ of the LIVE bridge process (keys only)")?;
                let mut live: Vec<String> = vars
                    .iter()
                    .map(|v| v.split('=').next().unwrap_or("").to_string())
                    .collect();
                live.sort();
                for k in &live {
                    writeln!(f, "{k}")?;
                }
                let cred_like: Vec<String> = live
                    .iter()
                    .filter(|k| {
                        let ku = k.to_uppercase();
                        ku.contains("KEY") || ku.contains("TOKEN") || ku.contains("SECRET")
                            || ku.starts_with("ANTHROPIC")
                            || ku.starts_with("SHANNON")
                            || ku.starts_with("OPENAI")
                    })
                    .cloned()
                    .collect();
                if cred_like.is_empty() {
                    writeln!(f, "\n# ISOLATION CHECK: PASS — no credential-like variable in live child env")?;
                } else {
                    writeln!(f, "\n# ISOLATION CHECK: FAIL — credential-like vars present: {cred_like:?}")?;
                }
            } else {
                writeln!(f, "\n# /proc/{p}/environ not readable (non-Linux or race)")?;
            }
        }
    }
    println!("[client] child env recorded to {env_record}");

    let log = Arc::new(Logger(Arc::new(Mutex::new(std::io::BufWriter::new(
        std::fs::File::create(&log_path)?,
    )))));

    // ---- stdout reader: routes responses by id, streams notifications ----
    let (resp_tx, mut resp_rx) = mpsc::unbounded_channel::<(u64, Value)>();
    let chunks: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let stdout = child.stdout.take().unwrap();
    let reader_log = log.clone();
    let chunk_sink = chunks.clone();
    let reader = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            reader_log.log("<-- ", &line);
            let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
            if let Some(id) = msg.get("id").and_then(Value::as_u64) {
                // JSON-RPC response (has result or error)
                if msg.get("result").is_some() || msg.get("error").is_some() {
                    let _ = resp_tx.send((id, msg));
                }
                continue;
            }
            if msg.get("method").and_then(Value::as_str) == Some("session/update") {
                let upd = &msg["params"]["update"];
                if upd["sessionUpdate"] == "agent_message_chunk" {
                    if let Some(t) = upd["content"]["text"].as_str() {
                        chunk_sink.lock().unwrap().push_str(t);
                        print!("{}", t);
                        let _ = std::io::stdout().flush();
                    }
                } else {
                    eprintln!(
                        "\n[update] {}",
                        upd["sessionUpdate"].as_str().unwrap_or("?")
                    );
                }
            } else if let Some(m) = msg.get("method").and_then(Value::as_str) {
                if m != "session/update" {
                    eprintln!("\n[notify] {m}");
                }
            }
        }
    });

    // ---- stderr reader (adapter/CLI logs) ----
    let stderr = child.stderr.take().unwrap();
    let stderr_log = log.clone();
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            stderr_log.log("ERR ", &line);
        }
    });

    let stdin = child.stdin.take().unwrap();
    let stdin = Arc::new(Mutex::new(stdin));
    let next_id = AtomicU64::new(1);

    let send = {
        let stdin = stdin.clone();
        let log = log.clone();
        move |v: Value| {
            let stdin = stdin.clone();
            let log = log.clone();
            async move {
                let line = serde_json::to_string(&v)?;
                log.log("--> ", &line);
                stdin.lock().unwrap().write_all(line.as_bytes()).await?;
                stdin.lock().unwrap().write_all(b"\n").await?;
                stdin.lock().unwrap().flush().await?;
                Ok::<_, anyhow::Error>(())
            }
        }
    };

    // ---- 1. initialize ----
    let id = next_id.fetch_add(1, Ordering::SeqCst);
    send(json!({
        "jsonrpc": "2.0", "id": id, "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientCapabilities": {
                "fs": {"readTextFile": false, "writeTextFile": false},
                "terminal": false
            }
        }
    }))
    .await?;
    let resp = tokio::time::timeout(Duration::from_secs(30), resp_rx.recv())
        .await?
        .context("adapter closed before initialize response")?;
    anyhow::ensure!(resp.0 == id, "out-of-order response");
    let init = resp.1;
    if let Some(e) = init.get("error") {
        bail!("initialize failed: {e}");
    }
    println!(
        "\n[client] initialize ok: protocolVersion={} agent={:?} authMethods={}",
        init["result"]["protocolVersion"],
        init["result"]["agentInfo"]["name"].as_str().unwrap_or("?"),
        init["result"]["authMethods"]
    );

    // ---- 2. session/new ----
    let id = next_id.fetch_add(1, Ordering::SeqCst);
    send(json!({
        "jsonrpc": "2.0", "id": id, "method": "session/new",
        "params": {"cwd": cwd, "mcpServers": []}
    }))
    .await?;
    let resp = tokio::time::timeout(Duration::from_secs(30), resp_rx.recv())
        .await?
        .context("adapter closed before session/new response")?;
    anyhow::ensure!(resp.0 == id, "out-of-order response");
    let created = resp.1;
    if let Some(e) = created.get("error") {
        bail!("session/new failed: {e}");
    }
    let session_id = created["result"]["sessionId"]
        .as_str()
        .context("missing sessionId")?
        .to_string();
    println!("[client] session created: {session_id}");

    // ---- 3. session/prompt (streamed) ----
    let id = next_id.fetch_add(1, Ordering::SeqCst);
    send(json!({
        "jsonrpc": "2.0", "id": id, "method": "session/prompt",
        "params": {"sessionId": session_id, "prompt": [{"type": "text", "text": prompt}]}
    }))
    .await?;
    println!("\n[client] prompt sent; streaming agent_message_chunk:");
    let resp = tokio::time::timeout(Duration::from_secs(300), resp_rx.recv())
        .await?
        .context("adapter closed before session/prompt response")?;
    anyhow::ensure!(resp.0 == id, "out-of-order response");
    let done = resp.1;
    println!();
    if let Some(e) = done.get("error") {
        bail!("session/prompt failed: {e}");
    }
    let stop = done["result"]["stopReason"]
        .as_str()
        .unwrap_or("(none)")
        .to_string();
    let text = chunks.lock().unwrap().clone();
    println!("[client] stopReason={stop}");
    println!("[client] assembled agent text: {text}");

    // ---- teardown ----
    let _ = child.kill().await;
    let _ = reader.await;
    let _ = stderr_task.await;
    println!("[client] evidence log: {log_path}");
    Ok(())
}
