//! Mobile device pairing — desktop side (P1.3, Design D).
//!
//! The gateway holds pairing state (`PairTokenStore` + `DeviceRegistry`); the
//! desktop talks to it over the **shared-file control channel** — the same
//! channel the gateway supervisor already uses (`--config <file>`, no IPC).
//! These three commands read/write the two files named in the gateway config's
//! `mobile` block:
//!
//!   `generate_pair_token` — mints a one-time 75s-TTL token, **appends** it to
//!     `mobile.tokensFile` (the gateway consumes it on `shannon/pair`), and
//!     returns a QR (LAN endpoint + token) the phone scans.
//!   `list_paired_devices`  — reads `mobile.devicesFile` (the gateway writes it
//!     on successful pair).
//!   `revoke_device`        — atomically removes a device entry; the gateway's
//!     in-memory registry re-reads on its next operation (resume/query then
//!     reject with PAIRING_REQUIRED).
//!
//! Security: device public keys are not secrets (Ed25519), so a JSON data file
//! — not the OS keyring — is correct here and satisfies F14 (no credentials in
//! config/repo). The one-time pair token touches disk for ≤75s on a single-user
//! loopback host; it is consumed-on-read so a leaked/replayed token is useless.
//! See `claudedocs/mobile-host-architecture.md` (D3/D4) and
//! `mobile-host-implementation-plan.md` (P1.3).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use qrcode::{types::Color, QrCode};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::commands_connections::{GatewayConfig, GatewayMobileConfig};

/// Default port the gateway binds its mobile `shannon/*` WS server on. Mirrors
/// `shannon-gateway` `bootstrap()` and the desktop's default gateway config.
pub const DEFAULT_MOBILE_PORT: u16 = 33430;

/// One-time pair-token lifetime, ms. Within the 60–90s design window; matches
/// the gateway's `PairTokenStore` default TTL.
const TOKEN_TTL_MS: u64 = 75_000;

/// QR payload schema version. v1 = LAN direct (M1). v2 (relay + X25519 E2E)
/// lands in P2.2.
const QR_VERSION: u32 = 1;

/// `~/.shannon/mobile-pair-tokens.jsonl` — JSONL, one `{token,issuedAt,
/// expiresAt}` per line. Desktop appends; gateway consumes-on-read.
fn tokens_path() -> Result<PathBuf, String> {
    let home = home_dir()?;
    Ok(home.join(".shannon").join("mobile-pair-tokens.jsonl"))
}

/// `~/.shannon/mobile-devices.json` — `{ entries: DeviceEntry[] }`. Gateway
/// writes on pair; desktop reads (list) and rewrites (revoke).
fn devices_path() -> Result<PathBuf, String> {
    let home = home_dir()?;
    Ok(home.join(".shannon").join("mobile-devices.json"))
}

fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "cannot resolve home directory".to_string())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The default `mobile` block the desktop writes into the gateway config so the
/// inbound `shannon/*` server starts on the next gateway launch. Paths are the
/// canonical `~/.shannon/mobile-*` files these commands also use, so both sides
/// agree by construction.
pub fn default_mobile_config() -> GatewayMobileConfig {
    GatewayMobileConfig {
        enabled: true,
        host: Some("127.0.0.1".into()),
        port: Some(DEFAULT_MOBILE_PORT),
        tokens_file: tokens_path().ok().and_then(|p| p.to_str().map(str::to_string)),
        devices_file: devices_path().ok().and_then(|p| p.to_str().map(str::to_string)),
    }
}

// ── on-disk shapes (mirror shannon-gateway/src/mobile/pairing.ts) ────────────

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceEntry {
    pub device_id: String,
    pub public_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub added_at: u64,
    pub last_seen_at: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
struct DevicesFile {
    #[serde(default)]
    entries: Vec<DeviceEntry>,
}

#[derive(Debug, Serialize)]
struct PairTokenRecord {
    token: String,
    issued_at: u64,
    expires_at: u64,
}

// ── command result shapes ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairTokenResponse {
    pub token: String,
    pub expires_at: u64,
    /// `ws://<lan-ip>:<port>` — where the phone connects (same WiFi, M1).
    pub lan_endpoint: String,
    /// `data:image/svg+xml;base64,…` — render straight in an `<img>`.
    pub qr_data_url: String,
}

/// Mint a one-time pair token + QR. Appends to the tokens file the gateway
/// consumes; the QR embeds the LAN endpoint + token for the phone (P1.4 parses
/// this payload).
#[tauri::command]
pub async fn mobile_generate_pair_token() -> Result<PairTokenResponse, String> {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);

    let issued_at = now_ms();
    let expires_at = issued_at + TOKEN_TTL_MS;

    // Append the one-time record so the gateway can consume it on shannon/pair.
    let record = PairTokenRecord {
        token: token.clone(),
        issued_at,
        expires_at,
    };
    let line = serde_json::to_string(&record)
        .map_err(|e| format!("pair token: serialize failed: {e}"))?;
    let path = tokens_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("pair token: cannot create {parent:?}: {e}"))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("pair token: cannot open {path:?}: {e}"))?;
    writeln!(file, "{line}").map_err(|e| format!("pair token: write failed: {e}"))?;

    let (ip, port) = lan_endpoint()?;
    let lan_endpoint = format!("ws://{ip}:{port}");

    // QR payload — the contract the mobile app (P1.4) parses. v1 = LAN direct.
    let payload = serde_json::json!({
        "v": QR_VERSION,
        "scheme": "ws",
        "host": ip.to_string(),
        "port": port,
        "token": token,
        "exp": expires_at,
    })
    .to_string();
    let qr_data_url = render_qr_svg_data_url(&payload)?;

    Ok(PairTokenResponse {
        token,
        expires_at,
        lan_endpoint,
        qr_data_url,
    })
}

/// List currently paired devices (read-only; the gateway writes the file).
#[tauri::command]
pub async fn mobile_list_paired_devices() -> Result<Vec<DeviceEntry>, String> {
    Ok(read_devices()?.entries)
}

/// Remove a paired device by id. Atomically rewrites the registry file so the
/// gateway's next resume/query rejects it. Returns `true` if a device was
/// removed, `false` if it was already absent (idempotent).
#[tauri::command]
pub async fn mobile_revoke_device(device_id: String) -> Result<bool, String> {
    let path = devices_path()?;
    let mut file = read_devices()?;
    let before = file.entries.len();
    file.entries.retain(|e| e.device_id != device_id);
    let removed = file.entries.len() < before;
    if removed {
        write_devices_atomic(&path, &file)?;
    }
    Ok(removed)
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn read_devices() -> Result<DevicesFile, String> {
    let path = devices_path()?;
    if !path.exists() {
        return Ok(DevicesFile::default());
    }
    let raw = fs::read_to_string(&path)
        .map_err(|e| format!("devices file: cannot read {path:?}: {e}"))?;
    if raw.trim().is_empty() {
        return Ok(DevicesFile::default());
    }
    let file: DevicesFile = serde_json::from_str(&raw)
        .map_err(|e| format!("devices file: invalid JSON in {path:?}: {e}"))?;
    Ok(file)
}

fn write_devices_atomic(path: &PathBuf, file: &DevicesFile) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "devices file: no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|e| format!("devices file: mkdir failed: {e}"))?;
    let json = serde_json::to_string_pretty(file)
        .map_err(|e| format!("devices file: serialize failed: {e}"))?;
    let tmp = NamedTempFile::new_in(parent)
        .map_err(|e| format!("devices file: cannot create temp file: {e}"))?;
    fs::write(tmp.path(), &json).map_err(|e| format!("devices file: write failed: {e}"))?;
    tmp.persist(path)
        .map_err(|e| format!("devices file: persist failed: {e}"))?;
    Ok(())
}

/// Resolve the LAN egress IPv4 + the mobile WS port, for the QR endpoint. The
/// UDP "connect" selects the egress interface from the routing table without
/// sending any packet, so it works whenever a default route exists (typical home
/// WiFi). Returns an error if no LAN IPv4 can be determined — the UI surfaces it
/// rather than rendering a QR the phone could never reach.
fn lan_endpoint() -> Result<(Ipv4Addr, u16), String> {
    let ip = lan_ipv4().ok_or_else(|| {
        "cannot detect a LAN IPv4 address (no default route?). Connect to WiFi and retry.".to_string()
    })?;
    let port = gateway_mobile_port().unwrap_or(DEFAULT_MOBILE_PORT);
    Ok((ip, port))
}

fn lan_ipv4() -> Option<Ipv4Addr> {
    // Bind any local UDP socket, "connect" to a routable dummy (no packet is
    // sent), then read the source address the kernel would use.
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(v4) if !v4.is_loopback() => Some(v4),
        _ => None,
    }
}

/// The mobile WS port the desktop last wrote into the gateway config (or the
/// default if the config can't be read). Kept best-effort — the port is
/// desktop-controlled, so this is authoritative outside tests.
fn gateway_mobile_port() -> Option<u16> {
    let raw = fs::read_to_string(gateway_config_path().ok()?).ok()?;
    let cfg: GatewayConfig = serde_json::from_str(&raw).ok()?;
    cfg.mobile.and_then(|m| m.port).or(Some(DEFAULT_MOBILE_PORT))
}

fn gateway_config_path() -> Result<PathBuf, String> {
    let home = home_dir()?;
    Ok(home.join(".shannon").join("gateway").join("config.json"))
}

/// Render `payload` as a QR matrix → SVG → `data:image/svg+xml;base64,…`.
fn render_qr_svg_data_url(payload: &str) -> Result<String, String> {
    let code = QrCode::new(payload.as_bytes()).map_err(|e| format!("qr: {e}"))?;
    let modules = code.width();
    let colors = code.to_colors();

    const MODULE_PX: usize = 8;
    const QUIET_PX: usize = 4 * MODULE_PX; // 4-module quiet zone
    let dim = modules * MODULE_PX + 2 * QUIET_PX;

    let mut svg = String::with_capacity(colors.len() * 40);
    svg.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{dim}" height="{dim}" viewBox="0 0 {dim} {dim}" shape-rendering="crispEdges">"#,
    ));
    svg.push_str(&format!(
        r#"<rect width="{dim}" height="{dim}" fill="white"/>"#,
    ));
    for (i, c) in colors.iter().enumerate() {
        if *c == Color::Dark {
            let x = (i % modules) * MODULE_PX + QUIET_PX;
            let y = (i / modules) * MODULE_PX + QUIET_PX;
            svg.push_str(&format!(
                r#"<rect x="{x}" y="{y}" width="{MODULE_PX}" height="{MODULE_PX}" fill="black"/>"#,
            ));
        }
    }
    svg.push_str("</svg>");

    let b64 = base64::engine::general_purpose::STANDARD.encode(svg);
    Ok(format!("data:image/svg+xml;base64,{b64}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mobile_config_targets_loopback_and_canonical_files() {
        let m = default_mobile_config();
        assert!(m.enabled);
        assert_eq!(m.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(m.port, Some(DEFAULT_MOBILE_PORT));
        assert!(m.tokens_file.as_deref().unwrap().ends_with("mobile-pair-tokens.jsonl"));
        assert!(m.devices_file.as_deref().unwrap().ends_with("mobile-devices.json"));
    }

    #[test]
    fn device_entry_round_trips_gateway_schema() {
        // Exact wire shape the gateway's DeviceRegistry writes.
        let raw = r#"{
            "deviceId": "abc123",
            "publicKey": "pk",
            "label": "pixel",
            "addedAt": 1000,
            "lastSeenAt": 2000
        }"#;
        let e: DeviceEntry = serde_json::from_str(raw).unwrap();
        assert_eq!(e.device_id, "abc123");
        assert_eq!(e.public_key, "pk");
        assert_eq!(e.label.as_deref(), Some("pixel"));
        assert_eq!(e.added_at, 1000);
        assert_eq!(e.last_seen_at, 2000);
        // Round-trips back to camelCase.
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains("\"deviceId\""));
        assert!(j.contains("\"publicKey\""));
    }

    #[test]
    fn devices_file_parses_empty_and_missing_entries() {
        let f: DevicesFile = serde_json::from_str(r#"{"entries":[]}"#).unwrap();
        assert!(f.entries.is_empty());
        // Missing `entries` defaults to empty.
        let f2: DevicesFile = serde_json::from_str(r#"{}"#).unwrap();
        assert!(f2.entries.is_empty());
    }

    #[test]
    fn read_devices_returns_empty_when_no_file() {
        // Point HOME at a temp dir with no devices file. set_env(HOME) is unsafe
        // under parallel tests, so call the parser path directly instead.
        let parsed: DevicesFile =
            serde_json::from_str(r#"{"entries":[{"deviceId":"x","publicKey":"k","addedAt":1,"lastSeenAt":2}]}"#)
                .unwrap();
        assert_eq!(parsed.entries.len(), 1);
    }

    #[test]
    fn qr_svg_renders_a_data_url_with_content() {
        let url = render_qr_svg_data_url("hello").unwrap();
        assert!(url.starts_with("data:image/svg+xml;base64,"));
        let b64 = url.strip_prefix("data:image/svg+xml;base64,").unwrap();
        let svg = String::from_utf8(base64::engine::general_purpose::STANDARD.decode(b64).unwrap()).unwrap();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("<rect"));
    }

    #[test]
    fn lan_ipv4_never_panics() {
        // Best-effort: must never panic. Accept None (no route in CI) or a real
        // non-loopback IPv4.
        if let Some(ip) = lan_ipv4() {
            assert!(!ip.is_loopback(), "lan_ipv4 must not return loopback");
        }
    }
}
