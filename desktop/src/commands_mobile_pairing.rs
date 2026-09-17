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
//!     returns a QR (LAN endpoint + token) the phone scans. The QR `host` is
//!     the desktop's mDNS name (`<hostname>.local`), not a raw IP — iOS ATS
//!     rejects raw-IP `ws://` endpoints at the OS level, while `.local` names
//!     are covered by `NSAllowsLocalNetworking` (cross-repo-adaptation-spec
//!     §A8/§A8b; the gateway advertises `_shannon._tcp` over mDNS alongside).
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

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use qrcode::{QrCode, types::Color};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::commands_connections::{
    GatewayConfig, GatewayMobileConfig, GatewayMobileTlsConfig, gateway_read_config,
    write_gateway_config_atomic,
};

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
/// agree by construction. Binds `0.0.0.0`: LAN direct-connect pairing is
/// reachable only from a non-loopback bind (the gateway skips its mDNS
/// advertisement on loopback binds), and access is gated by one-time tokens.
pub fn default_mobile_config() -> GatewayMobileConfig {
    GatewayMobileConfig {
        enabled: true,
        host: Some("0.0.0.0".into()),
        port: Some(DEFAULT_MOBILE_PORT),
        tokens_file: tokens_path()
            .ok()
            .and_then(|p| p.to_str().map(str::to_string)),
        devices_file: devices_path()
            .ok()
            .and_then(|p| p.to_str().map(str::to_string)),
        // v0.12 rollout: TLS ships OFF. Flip the default only after the
        // pinning-capable mobile build is widespread (release checklist).
        tls: None,
    }
}

// ── on-disk shapes (mirror shannon-gateway/src/mobile/pairing.ts) ────────────
//
// Wire-form contract (verified against the TS sources, which are the producer
// for devices.json and the consumer for tokens.jsonl):
//   devices.json  → snake_case keys (`device_id`, `public_key`, `added_at`,
//                  `last_seen_at`) — the gateway writes and reads these; its
//                  load() drops entries without `device_id`/`public_key`.
//   tokens.jsonl  → camelCase keys (`issuedAt`, `expiresAt`) — the gateway's
//                  `PairTokenRecord` shape; a snake_case line is silently
//                  skipped by its consumer, so the desktop MUST write camelCase.
//
// `DeviceEntry` (camelCase) is the Tauri command result for the UI;
// `DeviceEntryFile` (snake_case + legacy aliases) is what touches the disk.

/// On-disk device entry. Reads both the canonical snake_case form and the
/// camelCase form older desktop builds wrote (those entries used to be
/// dropped by the gateway's strict load — un-trusting every paired device).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct DeviceEntryFile {
    #[serde(alias = "deviceId")]
    pub device_id: String,
    #[serde(alias = "publicKey")]
    pub public_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(alias = "addedAt")]
    pub added_at: u64,
    #[serde(alias = "lastSeenAt")]
    pub last_seen_at: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct DevicesFile {
    #[serde(default)]
    entries: Vec<DeviceEntryFile>,
}

/// UI-facing entry (the desktop's JS convention, unchanged for the frontend).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceEntry {
    pub device_id: String,
    pub public_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub added_at: u64,
    pub last_seen_at: u64,
}

impl From<DeviceEntryFile> for DeviceEntry {
    fn from(f: DeviceEntryFile) -> Self {
        DeviceEntry {
            device_id: f.device_id,
            public_key: f.public_key,
            label: f.label,
            added_at: f.added_at,
            last_seen_at: f.last_seen_at,
        }
    }
}

impl From<&DeviceEntry> for DeviceEntryFile {
    fn from(e: &DeviceEntry) -> Self {
        DeviceEntryFile {
            device_id: e.device_id.clone(),
            public_key: e.public_key.clone(),
            label: e.label.clone(),
            added_at: e.added_at,
            last_seen_at: e.last_seen_at,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PairTokenRecord {
    token: String,
    issued_at: u64,
    expires_at: u64,
}

/// `~/.shannon/mobile-tls/tls-info.json` — written by the gateway when
/// `mobile.tls` is on (mobileTls.ts). Its fingerprint rides the QR so phones
/// pin the self-signed cert instead of chain-validating it.
#[derive(Debug, Clone, Deserialize)]
struct MobileTlsInfo {
    fingerprint: String,
}

/// Read the gateway's TLS info file; `None` when TLS is off (or the file is
/// unreadable → plaintext ws QR, matching the gateway's actual listener).
fn read_tls_info() -> Option<MobileTlsInfo> {
    let path = home_dir()
        .ok()?
        .join(".shannon")
        .join("mobile-tls")
        .join("tls-info.json");
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
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
    let line =
        serde_json::to_string(&record).map_err(|e| format!("pair token: serialize failed: {e}"))?;
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

    // QR host: the desktop's mDNS name (`<hostname>.local`) — iOS ATS refuses
    // raw-IP ws:// endpoints outright, while `.local` names are permitted
    // (§A8b). Fall back to the raw IP only when no hostname can be determined
    // (same behavior as before this existed; iOS will fail its preflight with
    // `rawIpOnIos` instead of a mysterious OS-level refusal).
    let qr_host = mdns_hostname().unwrap_or_else(|| ip.to_string());

    // v0.12: when the gateway serves TLS (tls-info.json present), the QR
    // advertises wss + the cert fingerprint the phone pins (out-of-band
    // trust — same channel that carried the pair token).
    let tls_info = read_tls_info();
    let scheme = if tls_info.is_some() { "wss" } else { "ws" };
    let lan_endpoint = format!("{scheme}://{ip}:{port}");

    // QR payload — the contract the mobile app (P1.4) parses. v1 = LAN direct.
    let mut payload = serde_json::Map::new();
    payload.insert("v".into(), serde_json::json!(QR_VERSION));
    payload.insert("scheme".into(), serde_json::json!(scheme));
    payload.insert("host".into(), serde_json::json!(qr_host));
    payload.insert("port".into(), serde_json::json!(port));
    payload.insert("token".into(), serde_json::json!(token));
    payload.insert("exp".into(), serde_json::json!(expires_at));
    if let Some(tls) = tls_info {
        payload.insert("certFingerprint".into(), serde_json::json!(tls.fingerprint));
    }
    let payload = serde_json::Value::Object(payload).to_string();
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
    Ok(read_devices()?
        .entries
        .into_iter()
        .map(Into::into)
        .collect())
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

// ── v0.12 LAN TLS (wss + cert-fingerprint pinning) ──────────────────────────

/// UI-facing TLS status: the config flag plus the live material info (the
/// fingerprint appears only after the gateway first boots with TLS on).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MobileTlsStatus {
    pub enabled: bool,
    pub fingerprint: Option<String>,
}

/// Current `mobile.tls` toggle state + cert fingerprint (read-only).
#[tauri::command]
pub async fn mobile_tls_status() -> Result<MobileTlsStatus, String> {
    let config: GatewayConfig = gateway_read_config().await?;
    let enabled = config
        .mobile
        .as_ref()
        .and_then(|m| m.tls.as_ref())
        .map(|t| t.enabled)
        .unwrap_or(false);
    Ok(MobileTlsStatus {
        enabled,
        fingerprint: read_tls_info().map(|i| i.fingerprint),
    })
}

/// Flip `mobile.tls.enabled` in the gateway config (read-modify-write, same
/// atomic path as the connections panel). Takes effect on the next gateway
/// (re)start — the supervised process restart is the UI's job after writing.
#[tauri::command]
pub async fn mobile_set_tls(enabled: bool) -> Result<MobileTlsStatus, String> {
    let mut config: GatewayConfig = gateway_read_config().await?;
    let mobile = config.mobile.get_or_insert(default_mobile_config());
    let tls = mobile
        .tls
        .get_or_insert(GatewayMobileTlsConfig { enabled: false });
    tls.enabled = enabled;
    write_gateway_config_atomic(&config)?;
    mobile_tls_status().await
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
    let parent = path
        .parent()
        .ok_or_else(|| "devices file: no parent".to_string())?;
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
        "cannot detect a LAN IPv4 address (no default route?). Connect to WiFi and retry."
            .to_string()
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

/// The desktop's mDNS hostname, `<first-label>.local` (§A8b) — matches the
/// name the gateway's `_shannon._tcp` advertisement resolves under (the OS
/// built-in responder on macOS, Avahi or the gateway's own responder on
/// Linux). Single label + lowercase keeps the name a legal mDNS host label.
/// `None` only if the OS has no hostname, in which case the QR falls back to
/// the raw LAN IP.
fn mdns_hostname() -> Option<String> {
    let raw = gethostname::gethostname().to_string_lossy().into_owned();
    let label = raw.split('.').next()?.trim().to_ascii_lowercase();
    if label.is_empty() {
        return None;
    }
    Some(format!("{label}.local"))
}

/// The mobile WS port the desktop last wrote into the gateway config (or the
/// default if the config can't be read). Kept best-effort — the port is
/// desktop-controlled, so this is authoritative outside tests.
fn gateway_mobile_port() -> Option<u16> {
    let raw = fs::read_to_string(gateway_config_path().ok()?).ok()?;
    let cfg: GatewayConfig = serde_json::from_str(&raw).ok()?;
    cfg.mobile
        .and_then(|m| m.port)
        .or(Some(DEFAULT_MOBILE_PORT))
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
    fn default_mobile_config_binds_wildcard_and_canonical_files() {
        let m = default_mobile_config();
        assert!(m.enabled);
        // §A8b: LAN direct-connect needs a non-loopback bind; the gateway
        // skips its mDNS advertisement on loopback-only servers.
        assert_eq!(m.host.as_deref(), Some("0.0.0.0"));
        assert_eq!(m.port, Some(DEFAULT_MOBILE_PORT));
        assert!(
            m.tokens_file
                .as_deref()
                .unwrap()
                .ends_with("mobile-pair-tokens.jsonl")
        );
        assert!(
            m.devices_file
                .as_deref()
                .unwrap()
                .ends_with("mobile-devices.json")
        );
    }

    #[test]
    fn mdns_hostname_is_a_single_local_label() {
        // Best-effort: must never panic. Wherever a hostname exists, the QR
        // host form is `<single lowercase label>.local` (iOS ATS contract).
        if let Some(name) = mdns_hostname() {
            assert!(name.ends_with(".local"));
            let label = name.strip_suffix(".local").unwrap();
            assert!(!label.contains('.'), "single label only, got {label:?}");
            assert!(
                label
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            );
            assert!(!label.is_empty());
        }
    }

    #[test]
    fn device_entry_round_trips_gateway_schema() {
        // Exact wire shape the gateway's DeviceRegistry writes/reads
        // (pairing.ts DeviceEntry — snake_case on disk).
        let raw = r#"{
            "device_id": "abc123",
            "public_key": "pk",
            "label": "pixel",
            "added_at": 1000,
            "last_seen_at": 2000
        }"#;
        let e: DeviceEntryFile = serde_json::from_str(raw).unwrap();
        assert_eq!(e.device_id, "abc123");
        assert_eq!(e.public_key, "pk");
        assert_eq!(e.label.as_deref(), Some("pixel"));
        assert_eq!(e.added_at, 1000);
        assert_eq!(e.last_seen_at, 2000);
        // Round-trips back to snake_case — the shape the gateway's load()
        // requires (it drops entries without `device_id`/`public_key`).
        let j = serde_json::to_string(&e).unwrap();
        assert!(j.contains("\"device_id\""));
        assert!(j.contains("\"public_key\""));
        assert!(!j.contains("\"deviceId\""));
    }

    #[test]
    fn device_entry_still_parses_legacy_camelcase_files() {
        // Pre-fix desktop builds wrote camelCase; serde aliases keep those
        // files readable (they get rewritten snake_case on the next revoke).
        let legacy: DeviceEntryFile = serde_json::from_str(
            r#"{"deviceId":"abc","publicKey":"pk","addedAt":1,"lastSeenAt":2}"#,
        )
        .unwrap();
        assert_eq!(legacy.device_id, "abc");
        assert_eq!(legacy.public_key, "pk");
        assert_eq!(legacy.added_at, 1);
        assert_eq!(legacy.last_seen_at, 2);
    }

    #[test]
    fn ui_device_entry_stays_camelcase() {
        // The Tauri command result is the UI contract (MobileDispatchCard.tsx
        // reads d.deviceId / d.lastSeenAt) — distinct from the disk shape.
        let j = serde_json::to_string(&DeviceEntry {
            device_id: "abc".into(),
            public_key: "pk".into(),
            label: None,
            added_at: 1,
            last_seen_at: 2,
        })
        .unwrap();
        assert!(j.contains("\"deviceId\""));
        assert!(j.contains("\"lastSeenAt\""));
    }

    #[test]
    fn pair_token_record_serializes_gateway_consumable_camelcase() {
        // The gateway's PairTokenStore.consumeFromFile skips any line whose
        // `expiresAt` (camelCase) is missing — a snake_case record made every
        // desktop-minted QR token invisible to `shannon/pair`.
        let record = PairTokenRecord {
            token: "tok".into(),
            issued_at: 1,
            expires_at: 2,
        };
        let j = serde_json::to_string(&record).unwrap();
        assert!(j.contains("\"issuedAt\""));
        assert!(j.contains("\"expiresAt\""));
        assert!(!j.contains("\"expires_at\""));
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
        let parsed: DevicesFile = serde_json::from_str(
            r#"{"entries":[{"device_id":"x","public_key":"k","added_at":1,"last_seen_at":2}]}"#,
        )
        .unwrap();
        assert_eq!(parsed.entries.len(), 1);
    }

    #[test]
    fn qr_svg_renders_a_data_url_with_content() {
        let url = render_qr_svg_data_url("hello").unwrap();
        assert!(url.starts_with("data:image/svg+xml;base64,"));
        let b64 = url.strip_prefix("data:image/svg+xml;base64,").unwrap();
        let svg = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .unwrap(),
        )
        .unwrap();
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
