//! `shannon self-update` — D1/D2 of the extension-security revision
//! (approved 2026-10-11): an explicit-command update flow over a
//! signature-verified release channel.
//!
//! Trust model, honest about its anchors:
//! - `latest.json` (the update channel) carries an Ed25519 signature over its
//!   canonical form (compact JSON, alphabetically sorted keys — exactly
//!   `serde_json::Value::to_string` output) made by the release pipeline
//!   (`shannon self-update --emit-channel` in the publish job, private key in
//!   an Actions secret). The public key is pinned below.
//! - The signature pins the channel's **per-asset SHA-256 hashes**; the
//!   downloaded tarball is checked against the channel's hash. So one
//!   signature covers the whole artifact chain.
//! - What this does NOT claim: TLS to GitHub/R2 is still the transport
//!   anchor, and the pinned key rotates only by shipping a new release.
//!
//! Windows is out of scope for v1 (a running exe cannot be replaced; the
//! command says so and points at install.ps1).

use anyhow::{Context, Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde_json::{Map, Value, json};

/// The release channel signing key (Ed25519 public key, base64 raw 32 bytes).
/// Generated 2026-10-12; private half lives in the `SHANNON_LATEST_SIGNING_KEY`
/// Actions secret (backup: workspace secrets-backup). Rotation = generate a
/// new pair, ship it here, then flip the secret.
pub const LATEST_CHANNEL_PUBKEY_B64: &str = "d+51dj8SsH2s9gZWwhoC3qPxuLwuU2YvbjxmouYh+VI=";

/// Channel schema version — bump on incompatible changes; verifiers refuse
/// schemas they do not know.
pub const CHANNEL_SCHEMA: u32 = 1;

/// Default channel location: the GitHub "latest" download surface (the
/// publish job attaches latest.json as a release asset). `SHANNON_UPDATE_URL`
/// overrides the whole URL (tests / R2 mirror).
pub const DEFAULT_CHANNEL_URL: &str =
    "https://github.com/diff-lab-com/shannon-agent/releases/latest/download/latest.json";

/// CLI release asset for this platform — full rustc triple, as the release
/// matrix publishes (`shannon-<triple>.tar.gz`). Windows is excluded (see
/// module docs).
fn cli_target_triple() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Some("x86_64-apple-darwin"),
        ("macos", "aarch64") => Some("aarch64-apple-darwin"),
        _ => None,
    }
}

/// A verified update channel.
#[derive(Debug, Clone)]
pub struct Channel {
    pub tag: String,
    /// rustc triple → release asset for the CLI tarball.
    pub cli: Vec<(String, String, String)>, // (triple, url, sha256)
}

fn verifying_key(pubkey_b64: &str) -> Result<VerifyingKey> {
    let bytes = B64
        .decode(pubkey_b64)
        .context("channel pubkey is not valid base64")?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow!("channel pubkey must be 32 bytes"))?;
    VerifyingKey::from_bytes(&arr).map_err(|e| anyhow!("channel pubkey rejected: {e}"))
}

/// Parse + signature-verify a channel document against the pinned release
/// key. Signature failure is ALWAYS fatal — there is no skip-and-continue
/// path (that would make the signature decorative).
pub fn verify_channel(raw: &str) -> Result<Channel> {
    verify_channel_with(raw, LATEST_CHANNEL_PUBKEY_B64)
}

/// [`verify_channel`] against an explicit key (tests / rotation rehearsal).
pub fn verify_channel_with(raw: &str, pubkey_b64: &str) -> Result<Channel> {
    let value: Value = serde_json::from_str(raw).context("latest.json is not valid JSON")?;
    let schema = value.get("schema").and_then(|s| s.as_u64()).unwrap_or(0);
    if schema != CHANNEL_SCHEMA as u64 {
        return Err(anyhow!(
            "unsupported update channel schema {schema} (want {CHANNEL_SCHEMA})"
        ));
    }
    let sig_b64 = value
        .get("signature")
        .and_then(|s| s.as_str())
        .ok_or_else(|| anyhow!("latest.json carries no signature — refusing"))?;
    let sig_bytes = B64
        .decode(sig_b64)
        .context("channel signature is not valid base64")?;
    let arr: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| anyhow!("channel signature must be 64 bytes"))?;

    // Canonical form = the document without its signature, compact-serialized
    // (serde_json sorts object keys alphabetically — deterministic on both
    // sides of the pipeline, which is why emit and verify agree byte-for-byte).
    let mut payload = value.clone();
    payload
        .as_object_mut()
        .expect("channel document is an object")
        .remove("signature");
    let canonical = payload.to_string();

    let vk = verifying_key(pubkey_b64)?;
    vk.verify(canonical.as_bytes(), &Signature::from_bytes(&arr))
        .map_err(|_| {
            anyhow!("channel signature INVALID — refusing to update (possible tampering)")
        })?;

    let tag = payload
        .get("tag")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("channel missing \"tag\""))?
        .to_string();
    let cli_obj = payload
        .get("cli")
        .and_then(|c| c.as_object())
        .ok_or_else(|| anyhow!("channel missing \"cli\" assets"))?;
    let mut cli = Vec::new();
    for (triple, asset) in cli_obj {
        let url = asset
            .get("url")
            .and_then(|u| u.as_str())
            .ok_or_else(|| anyhow!("cli.{triple} missing url"))?;
        let sha256 = asset
            .get("sha256")
            .and_then(|u| u.as_str())
            .ok_or_else(|| anyhow!("cli.{triple} missing sha256"))?;
        cli.push((triple.clone(), url.to_string(), sha256.to_string()));
    }
    if cli.is_empty() {
        return Err(anyhow!("channel carries no cli assets"));
    }
    Ok(Channel { tag, cli })
}

/// Emit a signed channel document from a release's SHA256SUMS — the publish
/// job runs this with the release binary itself, so canonicalization and
/// verification can never drift apart (same code both sides).
///
/// `sums_text` is the release-wide SHA256SUMS; every `shannon-<triple>.tar.gz`
/// line becomes a cli entry at `{url_base}/<name>`.
pub fn emit_channel(
    tag: &str,
    url_base: &str,
    sums_text: &str,
    signing_key_b64: &str,
) -> Result<String> {
    let seed = B64
        .decode(signing_key_b64)
        .context("signing key is not valid base64")?;
    let arr: [u8; 32] = seed
        .try_into()
        .map_err(|_| anyhow!("signing key must be 32 bytes"))?;
    let key = SigningKey::from_bytes(&arr);

    let mut cli = Map::new();
    for line in sums_text.lines() {
        let mut parts = line.split_whitespace();
        let (Some(hash), Some(name)) = (parts.next(), parts.next()) else {
            continue;
        };
        let triple = name
            .strip_prefix("shannon-")
            .and_then(|n| n.strip_suffix(".tar.gz"));
        let Some(triple) = triple else { continue };
        cli.insert(
            triple.to_string(),
            json!({
                "url": format!("{url_base}/{name}"),
                "sha256": hash.to_lowercase(),
            }),
        );
    }
    if cli.is_empty() {
        return Err(anyhow!(
            "no shannon-<triple>.tar.gz entries found in SHA256SUMS"
        ));
    }

    let payload = json!({ "schema": CHANNEL_SCHEMA, "tag": tag, "cli": cli });
    let canonical = payload.to_string();
    let signature = key.sign(canonical.as_bytes());

    let mut doc = payload;
    doc.as_object_mut().expect("payload is an object").insert(
        "signature".to_string(),
        json!(B64.encode(signature.to_bytes())),
    );
    Ok(serde_json::to_string_pretty(&doc)?)
}

/// `shannon self-update`: fetch the channel, verify, download, hash-check,
/// atomically replace this binary. Explicit user command — this complements
/// (never replaces) the desktop's honest "notify only" promise.
pub fn run(url_override: Option<&str>, yes: bool) -> Result<()> {
    if std::env::consts::OS == "windows" {
        return Err(anyhow!(
            "self-update is not supported on Windows yet — re-run install.ps1 to update"
        ));
    }
    let current = env!("CARGO_PKG_VERSION");
    let url = url_override.unwrap_or(DEFAULT_CHANNEL_URL);
    println!("Fetching update channel: {url}");
    let raw = crate::curl_to_string(url)?;
    let channel = verify_channel(&raw)?;
    let latest = channel.tag.trim_start_matches('v').to_string();

    let triple = cli_target_triple()
        .ok_or_else(|| anyhow!("self-update has no build mapping for this platform"))?;
    let (_, asset_url, expected_sha) = channel
        .cli
        .iter()
        .find(|(t, _, _)| t == triple)
        .ok_or_else(|| anyhow!("channel {tag} has no build for {triple}", tag = channel.tag))?;

    if !crate::version_is_newer(current, &latest) {
        println!(
            "shannon {current} is up to date (channel: {}).",
            channel.tag
        );
        return Ok(());
    }
    println!("shannon {current} → {latest}");
    if !yes {
        print!("Update now? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            println!("Aborted — nothing changed.");
            return Ok(());
        }
    }

    let exe = std::env::current_exe().context("cannot locate the running binary")?;
    let target = exe.canonicalize().unwrap_or(exe.clone());
    let parent = target
        .parent()
        .ok_or_else(|| anyhow!("binary has no parent directory"))?
        .to_path_buf();

    // Stage INSIDE the target's directory so the final swap is a
    // same-filesystem rename (atomic, and works while this process runs —
    // the old inode stays alive for it).
    let staging = parent.join(format!(".shannon-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging)
        .with_context(|| format!("cannot write to {} — is shannon installed by a system package? Re-run install.sh with sudo or use your package manager", parent.display()))?;

    let result = (|| -> Result<()> {
        let archive = staging.join("shannon.tar.gz");
        println!("Downloading {asset_url}");
        crate::curl_to_file(asset_url, &archive)?;

        let actual = sha256_file(&archive)?;
        if actual != expected_sha.to_lowercase() {
            return Err(anyhow!(
                "sha256 MISMATCH: channel says {expected_sha}, downloaded archive is {actual} — refusing to install"
            ));
        }

        let status = std::process::Command::new("tar")
            .arg("-xzf")
            .arg(&archive)
            .current_dir(&staging)
            .status()
            .context("spawn tar")?;
        if !status.success() {
            return Err(anyhow!("tar extraction failed"));
        }
        let extracted = staging.join("shannon");
        if !extracted.exists() {
            return Err(anyhow!("archive does not contain a `shannon` binary"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&extracted, std::fs::Permissions::from_mode(0o755))?;
        }
        std::fs::rename(&extracted, &target).with_context(|| {
            format!(
                "cannot replace {} — installed by a system package? Re-run install.sh with sudo",
                target.display()
            )
        })?;
        Ok(())
    })();

    let _ = std::fs::remove_dir_all(&staging);
    result?;

    println!("Updated shannon → {latest} at {}", target.display());
    println!(
        "Running sessions keep the old binary until they exit; new invocations use the new version."
    );
    Ok(())
}

fn sha256_file(path: &std::path::Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

// ---------------------------------------------------------------------------
// Tests. A dedicated test keypair (NOT the release key) — the cross-check
// against an independent implementation (python cryptography) lives in
// `emit_matches_independent_python_implementation`.
#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD as B64;

    const TEST_SEED_B64: &str = "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc=";
    // VerifyingKey::from_bytes(&[7u8; 32]) — derived once, pinned as bytes.
    fn test_pubkey_b64() -> String {
        let vk = VerifyingKey::from(&SigningKey::from_bytes(&[7u8; 32]));
        B64.encode(vk.to_bytes())
    }

    const SUMS: &str = "\
1111111111111111111111111111111111111111111111111111111111111111  shannon-x86_64-unknown-linux-gnu.tar.gz
2222222222222222222222222222222222222222222222222222222222222222  shannon-aarch64-apple-darwin.tar.gz
3333333333333333333333333333333333333333333333333333333333333333  install.sh
";

    #[test]
    fn emit_then_verify_roundtrip() {
        let doc = emit_channel(
            "v9.9.9",
            "https://example.test/download/v9.9.9",
            SUMS,
            TEST_SEED_B64,
        )
        .expect("emit");
        let channel = verify_channel_with(&doc, &test_pubkey_b64()).expect("verify");
        assert_eq!(channel.tag, "v9.9.9");
        let (_, url, sha) = channel
            .cli
            .iter()
            .find(|(t, _, _)| t == "x86_64-unknown-linux-gnu")
            .expect("linux asset");
        assert_eq!(
            url,
            "https://example.test/download/v9.9.9/shannon-x86_64-unknown-linux-gnu.tar.gz"
        );
        // Sums are lowercased into the channel.
        assert_eq!(sha.as_str(), "1".repeat(64).as_str());
        // Non-CLI lines (install.sh) are excluded.
        assert!(channel.cli.iter().all(|(t, _, _)| t != "install.sh"));
    }

    #[test]
    fn emit_is_deterministic() {
        let a = emit_channel("v1.0.0", "https://x.test", SUMS, TEST_SEED_B64).unwrap();
        let b = emit_channel("v1.0.0", "https://x.test", SUMS, TEST_SEED_B64).unwrap();
        assert_eq!(a, b, "same inputs must produce byte-identical channels");
    }

    #[test]
    fn tampered_payload_is_refused() {
        let doc = emit_channel("v1.0.0", "https://x.test", SUMS, TEST_SEED_B64).unwrap();
        // Flip the tag AFTER signing.
        let evil = doc.replace("v1.0.0", "v2.0.0");
        assert!(verify_channel_with(&evil, &test_pubkey_b64()).is_err());
    }

    #[test]
    fn wrong_key_is_refused() {
        let doc = emit_channel("v1.0.0", "https://x.test", SUMS, TEST_SEED_B64).unwrap();
        assert!(verify_channel_with(&doc, LATEST_CHANNEL_PUBKEY_B64).is_err());
    }

    #[test]
    fn missing_signature_is_refused() {
        let doc = emit_channel("v1.0.0", "https://x.test", SUMS, TEST_SEED_B64).unwrap();
        let value: Value = serde_json::from_str(&doc).unwrap();
        let stripped =
            json!({ "schema": value["schema"], "tag": value["tag"], "cli": value["cli"] });
        assert!(verify_channel_with(&stripped.to_string(), &test_pubkey_b64()).is_err());
    }

    #[test]
    fn unknown_schema_is_refused() {
        assert!(
            verify_channel_with(
                r#"{"schema":99,"tag":"v1","cli":{},"signature":"AAAA"}"#,
                &test_pubkey_b64()
            )
            .is_err()
        );
    }

    /// Independent-implementation cross-check: sign the canonical bytes with
    /// python `cryptography` (RFC 8032, unrelated codebase) and assert the
    /// Rust verifier accepts it — this pins the canonicalization contract
    /// against drift in `Value::to_string` key ordering or escaping.
    #[test]
    fn emit_matches_independent_python_implementation() {
        // Produced (2026-10-12) by python cryptography with the SAME test
        // seed [7u8;32] over
        //   {"cli":{"x86_64-unknown-linux-gnu":{"sha256":"11…","url":"https://x.test/shannon-x86_64-unknown-linux-gnu.tar.gz"}},"schema":1,"tag":"v1.0.0"}
        // (json.dumps(sort_keys=True, separators=(',',':'), ensure_ascii=False)).
        let python_signature: &str = include_str!("self_update_test_sig.b64");
        let payload = json!({
            "schema": CHANNEL_SCHEMA,
            "tag": "v1.0.0",
            "cli": {
                "x86_64-unknown-linux-gnu": {
                    "url": "https://x.test/shannon-x86_64-unknown-linux-gnu.tar.gz",
                    "sha256": "1".repeat(64),
                }
            },
        });
        let canonical = payload.to_string();
        let vk = verifying_key(&test_pubkey_b64()).unwrap();
        let sig: [u8; 64] = B64
            .decode(python_signature.trim())
            .expect("fixture is valid b64")
            .try_into()
            .expect("signature is 64 bytes");
        vk.verify(canonical.as_bytes(), &Signature::from_bytes(&sig))
            .expect("python-signed canonical payload must verify in Rust");
    }
}
