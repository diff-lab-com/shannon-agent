//! Gateway pairing-approval RPC — the desktop approval entry for IM access
//! control (T9, the "full form" of review F42).
//!
//! An unpaired IM sender who DMs the gateway's bot gets a 6-digit pairing
//! challenge (B6). Until now the only approval channel was replying
//! `approve <code>` from an already-allowed sender. These commands let the
//! owner see the pending challenges and approve them right here:
//!
//!   `gateway_pairing_pending` — lists the pending codes
//!   `gateway_pairing_approve` — consumes one code and allowlists the sender
//!
//! ## How the desktop reaches a running gateway
//!
//! Over the gateway's mobile listener (the same `shannon/*` server the phone
//! dials, `gateway/config.json` → `mobile.host`/`mobile.port`). The gateway
//! serves the two operations as `shannon/pairing.pending` /
//! `shannon/pairing.approve` JSON-RPC over its WebSocket dispatch AND over the
//! narrow HTTP POST skin `/rpc/pairing/pending` + `/rpc/pairing/approve` —
//! this module speaks the HTTP skin, because the desktop has an HTTP client
//! but no WS client. Both skins share ONE approval implementation with the IM
//! `approve <code>` reply (`approvePairingCode` in `gateway/src/access`),
//! so the two channels can never drift apart.
//!
//! ## Auth
//!
//! Every call mints a fresh one-time pair token (the Design-D control channel
//! this desktop already uses for the QR flow — `mobile_generate_pair_token`
//! appends it to `~/.shannon/mobile-pair-tokens.jsonl`, the gateway verifies
//! or consumes it). The list call only verifies the token (reads stay cheap);
//! approval consumes it (single-use, so a replayed token approves nothing
//! twice). This is the same credential bar `shannon/pair` applies, and the
//! token file is 0600 user-owned — possession IS the owner.
//!
//! ## TLS
//!
//! When `mobile.tls` is on (the desktop-written default) the gateway serves a
//! SELF-SIGNED certificate whose SHA-256 fingerprint is published in
//! `~/.shannon/mobile-tls/tls-info.json` — the same file the QR flow reads,
//! so the desktop and the phone pin one value. Chain validation is
//! impossible against a self-signed CA, so this client PINS the fingerprint
//! instead: the presented end-entity cert must hash to the published value
//! or the request fails with a loud error. A machine in the middle can no
//! longer terminate the session (and relaying a sniffed approve call to the
//! real gateway dies with the interception). Handshake signatures are still
//! verified against the presented key, so a stolen fingerprint alone is
//! useless without the gateway's private key.
//!
//! When the fingerprint file is absent (a manually configured gateway, or a
//! deleted/regenerating file) the client falls back to the pre-pinning
//! behavior — accept any cert — with a one-time warning. Auth remains the
//! single-use pair token carried in the body.

use std::sync::OnceLock;
use std::time::Duration;

use sha2::Digest as _;

use serde::{Deserialize, Serialize};

use crate::commands_connections::{GatewayConfig, gateway_read_config};
use crate::commands_mobile_pairing::{DEFAULT_MOBILE_PORT, mint_pair_token};

/// Gateway HTTP skin paths (gateway/src/mobile/accessRpc.ts). One place so
/// desktop and gateway agree.
pub(crate) const PAIRING_PENDING_PATH: &str = "/rpc/pairing/pending";
pub(crate) const PAIRING_APPROVE_PATH: &str = "/rpc/pairing/approve";

/// One pending (or just-approved) IM pairing request. Wire shape mirrors the
/// gateway's `PairingRequestRecord` (protocol.ts) in camelCase.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GatewayPairingRequest {
    /// The 6-digit code shown in the IM challenge.
    pub code: String,
    /// Chat platform the requester came from (slack/telegram/…).
    pub platform: String,
    /// Platform sender id the allowlist entry carries.
    pub sender_id: String,
    /// Epoch ms when the challenge was issued.
    pub requested_at: u64,
    /// Epoch ms after which the code expires (issue + 5 min).
    pub expires_at: u64,
}

/// Map the gateway config to the dialable base URL of the mobile listener.
/// The gateway binds `0.0.0.0`/`::` for LAN phones; from the same machine the
/// loopback form is the address that actually connects. TLS-on configs get an
/// `https` URL (the self-signed cert is accepted — see module docs). Errors
/// when the mobile server is disabled: there is no RPC surface to talk to.
pub(crate) fn pairing_rpc_base_url(config: &GatewayConfig) -> Result<String, String> {
    let mobile = config.mobile.as_ref().ok_or_else(|| {
        "gateway mobile server is not configured — enable Mobile dispatch in Connections settings"
            .to_string()
    })?;
    if !mobile.enabled {
        return Err(
            "gateway mobile server is disabled — enable Mobile dispatch in Connections settings"
                .to_string(),
        );
    }
    let host = match mobile.host.as_deref() {
        None | Some("") | Some("0.0.0.0") | Some("::") => "127.0.0.1",
        Some(h) => h,
    };
    let port = mobile.port.unwrap_or(DEFAULT_MOBILE_PORT);
    let scheme = if mobile.tls.as_ref().is_some_and(|t| t.enabled) {
        "https"
    } else {
        "http"
    };
    Ok(format!("{scheme}://{host}:{port}"))
}

#[derive(Debug, Deserialize)]
struct PairingRpcError {
    #[serde(default)]
    code: i64,
    message: String,
}

// ── TLS fingerprint pinning ────────────────────────────────────────────────

/// The client for one pairing call: pinned TLS when the URL is https and the
/// gateway's published fingerprint is readable; the legacy accept-any-cert
/// client (with a one-time warning) otherwise. See the module TLS docs.
fn pairing_client_for(base_url: &str) -> Result<reqwest::Client, String> {
    if !base_url.starts_with("https://") {
        return legacy_client();
    }
    match pinned_mobile_fingerprint() {
        Some(pin) => pinned_https_client(&pin),
        None => {
            static WARNED: OnceLock<()> = OnceLock::new();
            if WARNED.set(()).is_ok() {
                tracing::warn!(
                    "pairing RPC: gateway serves https but \
                     ~/.shannon/mobile-tls/tls-info.json carries no usable \
                     fingerprint — accepting any certificate until the file \
                     exists (restart the gateway with mobile.tls on to mint it)"
                );
            }
            legacy_client()
        }
    }
}

fn legacy_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        // No pin on file: nothing to compare, so chain checking is skipped
        // (the gateway intentionally serves a self-signed cert).
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| format!("pairing RPC: client build failed: {e}"))
}

/// Normalized (lowercase, colon-less) pinned fingerprint from the gateway's
/// `tls-info.json`, or `None` when the file is absent/unreadable/malformed —
/// the same source the QR flow reads, so desktop and phones pin one value.
fn pinned_mobile_fingerprint() -> Option<String> {
    let info = crate::commands_mobile_pairing::read_tls_info()?;
    let normalized = normalize_fingerprint(&info.fingerprint);
    parse_pinned_fingerprint(&normalized)?;
    Some(normalized)
}

/// Lowercase hex without separators — the tls-info.json/QR form. Tolerates
/// the colon-separated OpenSSL form for hand-edited files.
fn normalize_fingerprint(raw: &str) -> String {
    raw.trim().to_ascii_lowercase().replace(':', "")
}

/// Decode a normalized fingerprint into its 32 raw bytes; `None` when it is
/// not exactly 64 hex chars.
fn parse_pinned_fingerprint(normalized: &str) -> Option<[u8; 32]> {
    if normalized.len() != 64 {
        return None;
    }
    let hex_digit = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = [0u8; 32];
    for (i, pair) in normalized.as_bytes().chunks(2).enumerate() {
        out[i] = (hex_digit(pair[0])? << 4) | hex_digit(pair[1])?;
    }
    Some(out)
}

/// HTTPS client whose TLS layer pins the gateway cert by fingerprint.
fn pinned_https_client(pinned_hex: &str) -> Result<reqwest::Client, String> {
    let pinned = parse_pinned_fingerprint(pinned_hex).ok_or_else(|| {
        format!("pairing RPC: pinned fingerprint is not 64 hex chars: {pinned_hex}")
    })?;
    let provider = rustls::crypto::ring::default_provider();
    let mut config = rustls::ClientConfig::builder_with_provider(provider.clone().into())
        .with_safe_default_protocol_versions()
        .map_err(|e| format!("pairing RPC: TLS config error: {e}"))?
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(FingerprintVerifier {
            pinned,
            provider,
        }))
        .with_no_client_auth();
    // The gateway's axum skin is plain HTTP/1.1; advertise it so ALPN cannot
    // negotiate anything else.
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .use_preconfigured_tls(Some(config))
        .build()
        .map_err(|e| format!("pairing RPC: client build failed: {e}"))
}

/// rustls server-cert verifier that replaces chain validation with the
/// published fingerprint: the end-entity cert's SHA-256 must equal the pin.
/// Handshake signatures are still checked against the presented key (proof
/// of possession), so a peer without the gateway's private key cannot pass
/// even a fingerprint it observed.
#[derive(Debug)]
struct FingerprintVerifier {
    pinned: [u8; 32],
    provider: rustls::crypto::CryptoProvider,
}

impl rustls::client::danger::ServerCertVerifier for FingerprintVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let digest = sha2::Sha256::digest(end_entity.as_ref());
        if digest.as_slice() == self.pinned {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General(
                "gateway TLS certificate does not match the pinned fingerprint \
                 (~/.shannon/mobile-tls/tls-info.json). If the gateway cert was \
                 regenerated, restart the gateway so the file refreshes; \
                 otherwise this may be a machine in the middle."
                    .to_string(),
            ))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[derive(Debug, Deserialize)]
struct PairingRpcErrorBody {
    error: PairingRpcError,
}

#[derive(Debug, Deserialize)]
struct PendingBody {
    #[serde(default)]
    result: PendingResult,
}

#[derive(Debug, Default, Deserialize)]
struct PendingResult {
    #[serde(default)]
    pending: Vec<GatewayPairingRequest>,
}

#[derive(Debug, Deserialize)]
struct ApproveBody {
    #[serde(default)]
    result: ApproveResult,
}

#[derive(Debug, Default, Deserialize)]
struct ApproveResult {
    #[serde(default)]
    ok: bool,
    #[serde(default)]
    record: Option<GatewayPairingRequest>,
}

/// POST one pairing-access call with a minted token and unwrap the JSON-RPC
/// outcome: `200 {result}` → `Ok(result)`, anything else → `Err` carrying the
/// gateway's message (unknown/expired code, bad token, …).
pub(crate) async fn pairing_rpc_post(
    base_url: &str,
    path: &str,
    body: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let client = pairing_client_for(base_url)?;
    let url = format!("{}{}", base_url.trim_end_matches('/'), path);
    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("pairing RPC: cannot reach the gateway at {base_url}: {e}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|e| format!("pairing RPC: gateway response unreadable: {e}"))?;
    if !status.is_success() {
        if let Ok(parsed) = serde_json::from_str::<PairingRpcErrorBody>(&text) {
            return Err(format!(
                "pairing RPC: gateway rejected the request: {} (code {})",
                parsed.error.message, parsed.error.code
            ));
        }
        return Err(format!("pairing RPC: gateway returned HTTP {status}"));
    }
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("pairing RPC: gateway response is not JSON: {e}"))?;
    Ok(value)
}

/// Parse a `pending` result. Pure so tests cover the wire contract without
/// spinning a server.
pub(crate) fn parse_pending_result(
    value: serde_json::Value,
) -> Result<Vec<GatewayPairingRequest>, String> {
    let body: PendingBody = serde_json::from_value(value)
        .map_err(|e| format!("pairing RPC: unexpected pending response shape: {e}"))?;
    Ok(body.result.pending)
}

/// Parse an `approve` result into the approved record. Pure likewise.
pub(crate) fn parse_approve_result(
    value: serde_json::Value,
) -> Result<GatewayPairingRequest, String> {
    let body: ApproveBody = serde_json::from_value(value)
        .map_err(|e| format!("pairing RPC: unexpected approve response shape: {e}"))?;
    if !body.result.ok {
        return Err("pairing RPC: gateway did not approve the request".to_string());
    }
    body.result
        .record
        .ok_or_else(|| "pairing RPC: approve response carried no record".to_string())
}

/// Pending IM pairing requests on the running gateway (mint-verify-list).
#[tauri::command]
pub async fn gateway_pairing_pending() -> Result<Vec<GatewayPairingRequest>, String> {
    let config: GatewayConfig = gateway_read_config().await?;
    let base = pairing_rpc_base_url(&config)?;
    let token = mint_pair_token()?.token;
    let response = pairing_rpc_post(
        &base,
        PAIRING_PENDING_PATH,
        serde_json::json!({ "token": token }),
    )
    .await?;
    parse_pending_result(response)
}

/// Approve one pending pairing by code. The code is the one shown in the IM
/// challenge / listed by [`gateway_pairing_pending`]. Unknown or expired
/// codes come back as an `Err` carrying the gateway's reason.
#[tauri::command]
pub async fn gateway_pairing_approve(code: String) -> Result<GatewayPairingRequest, String> {
    if code.trim().is_empty() {
        return Err("pairing RPC: code is required".into());
    }
    let config: GatewayConfig = gateway_read_config().await?;
    let base = pairing_rpc_base_url(&config)?;
    let token = mint_pair_token()?.token;
    let response = pairing_rpc_post(
        &base,
        PAIRING_APPROVE_PATH,
        serde_json::json!({ "token": token, "code": code.trim() }),
    )
    .await?;
    parse_approve_result(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_mobile(
        mobile: Option<crate::commands_connections::GatewayMobileConfig>,
    ) -> GatewayConfig {
        GatewayConfig {
            engine: crate::commands_connections::GatewayEngineConfig {
                ws_url: "ws://127.0.0.1:33420/api/ws".into(),
                http_base_url: "http://127.0.0.1:33420".into(),
                model: None,
            },
            adapters: vec![],
            log_level: None,
            mobile,
        }
    }

    fn mobile_block(
        enabled: bool,
        host: Option<&str>,
        port: Option<u16>,
        tls: bool,
    ) -> crate::commands_connections::GatewayMobileConfig {
        crate::commands_connections::GatewayMobileConfig {
            enabled,
            host: host.map(str::to_string),
            port,
            tokens_file: None,
            devices_file: None,
            tls: Some(crate::commands_connections::GatewayMobileTlsConfig { enabled: tls }),
        }
    }

    #[test]
    fn base_url_maps_wildcard_binds_to_loopback() {
        // The desktop-written default binds 0.0.0.0 with TLS on — dialing
        // 0.0.0.0 never works; loopback is the same-machine address.
        let cfg = config_with_mobile(Some(mobile_block(true, Some("0.0.0.0"), None, true)));
        assert_eq!(
            pairing_rpc_base_url(&cfg).expect("base url"),
            "https://127.0.0.1:33430"
        );
        // IPv6 wildcard behaves the same.
        let cfg = config_with_mobile(Some(mobile_block(true, Some("::"), Some(3400), false)));
        assert_eq!(
            pairing_rpc_base_url(&cfg).expect("base url"),
            "http://127.0.0.1:3400"
        );
    }

    #[test]
    fn base_url_keeps_explicit_hosts_and_honors_tls_off() {
        let cfg = config_with_mobile(Some(mobile_block(
            true,
            Some("192.168.1.10"),
            Some(3399),
            false,
        )));
        assert_eq!(
            pairing_rpc_base_url(&cfg).expect("base url"),
            "http://192.168.1.10:3399"
        );
    }

    #[test]
    fn base_url_errors_when_mobile_disabled_or_missing() {
        let cfg = config_with_mobile(Some(mobile_block(false, None, None, false)));
        let err = pairing_rpc_base_url(&cfg).expect_err("must error");
        assert!(err.contains("disabled"), "got: {err}");
        let cfg = config_with_mobile(None);
        assert!(pairing_rpc_base_url(&cfg).is_err());
    }

    #[test]
    fn parses_pending_wire_shape() {
        let value = serde_json::json!({
            "result": { "pending": [
                { "code": "012345", "platform": "slack", "senderId": "U123",
                  "requestedAt": 1000, "expiresAt": 1300 }
            ]}
        });
        let pending = parse_pending_result(value).expect("parse");
        assert_eq!(pending.len(), 1);
        assert_eq!(
            pending[0],
            GatewayPairingRequest {
                code: "012345".into(),
                platform: "slack".into(),
                sender_id: "U123".into(),
                requested_at: 1000,
                expires_at: 1300,
            }
        );
        // camelCase serializes back for the UI contract.
        let json = serde_json::to_string(&pending[0]).expect("serialize");
        assert!(json.contains("\"senderId\""));
        assert!(json.contains("\"requestedAt\""));
    }

    #[test]
    fn parses_pending_empty_and_rejects_garbage() {
        let empty = parse_pending_result(serde_json::json!({ "result": {} })).expect("parse");
        assert!(empty.is_empty());
        assert!(
            parse_pending_result(serde_json::json!({ "result": { "pending": "nope" } })).is_err()
        );
    }

    #[test]
    fn parses_approve_record_and_flags_missing_one() {
        let ok = parse_approve_result(serde_json::json!({
            "result": { "ok": true, "record": {
                "code": "654321", "platform": "telegram", "senderId": "U9",
                "requestedAt": 5, "expiresAt": 10 } }
        }))
        .expect("parse");
        assert_eq!(ok.sender_id, "U9");

        let no_record = serde_json::json!({ "result": { "ok": true } });
        assert!(parse_approve_result(no_record).is_err());
        let not_ok = serde_json::json!({ "result": { "ok": false } });
        assert!(parse_approve_result(not_ok).is_err());
    }

    /// Local axum stub speaking the gateway's HTTP skin contract
    /// (`{result}` on 200, `{error:{code,message}}` on 4xx), so the reqwest
    /// path — URL building, error mapping, parsing — is exercised for real.
    #[tokio::test]
    async fn pairing_rpc_post_maps_success_and_gateway_errors() {
        use axum::Json;
        use axum::routing::{get, post};

        async fn pending() -> Json<serde_json::Value> {
            Json(serde_json::json!({
                "result": { "pending": [ { "code": "111111", "platform": "slack",
                    "senderId": "U1", "requestedAt": 1, "expiresAt": 2 } ] }
            }))
        }

        async fn approve() -> (axum::http::StatusCode, Json<serde_json::Value>) {
            (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": { "code": -32001, "message": "Unknown or expired pairing code" }
                })),
            )
        }

        async fn health() -> &'static str {
            "ok"
        }

        let app = axum::Router::new()
            .route("/rpc/pairing/pending", post(pending))
            .route("/rpc/pairing/approve", post(approve))
            .route("/health", get(health));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let addr = listener.local_addr().expect("local addr");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        let base = format!("http://{addr}");
        let ok = pairing_rpc_post(&base, PAIRING_PENDING_PATH, serde_json::json!({}))
            .await
            .expect("pending succeeds");
        let pending = parse_pending_result(ok).expect("parse pending");
        assert_eq!(pending[0].code, "111111");

        let err = pairing_rpc_post(
            &base,
            PAIRING_APPROVE_PATH,
            serde_json::json!({ "token": "t", "code": "000000" }),
        )
        .await
        .expect_err("approve error maps to Err");
        assert!(
            err.contains("Unknown or expired pairing code"),
            "got: {err}"
        );
        assert!(
            err.contains("-32001"),
            "error carries the gateway code: {err}"
        );

        // Unreachable base URL → Err mentioning the gateway.
        let down = pairing_rpc_post(
            "http://127.0.0.1:1",
            PAIRING_PENDING_PATH,
            serde_json::json!({}),
        )
        .await
        .expect_err("unreachable gateway is an Err");
        assert!(down.contains("cannot reach the gateway"), "got: {down}");

        server.abort();
    }

    // ── TLS fingerprint pinning ────────────────────────────────────────

    #[test]
    fn fingerprint_normalization_and_parse() {
        // The gateway publishes lowercase colon-less hex (mobileTls.ts);
        // the colon-separated OpenSSL form is tolerated too.
        assert_eq!(normalize_fingerprint("AA:BB:0C:0D:0E:0F"), "aabb0c0d0e0f");
        let normalized = normalize_fingerprint(
            "3A:F9:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:01:23:45:67:89:AB:CD:EF:FE:DC:BA:98:76:54:32",
        );
        let parsed = parse_pinned_fingerprint(&normalized).expect("64 hex chars parse");
        assert_eq!(parsed.len(), 32);
        assert_eq!(parsed[0], 0x3a);
        assert_eq!(parsed[31], 0x32);

        assert!(parse_pinned_fingerprint("aabb").is_none(), "too short");
        assert!(
            parse_pinned_fingerprint(&"a".repeat(63)).is_none(),
            "odd/short length rejected"
        );
        assert!(
            parse_pinned_fingerprint(&format!("{}zz", "a".repeat(62))).is_none(),
            "non-hex rejected"
        );
    }

    /// The verifier's whole contract: hash the presented end-entity DER,
    /// compare to the pin. Synthetic bytes suffice — it never parses the
    /// cert. Signature checks delegate to the stock provider (not re-tested).
    #[test]
    fn fingerprint_verifier_accepts_pinned_and_rejects_other() {
        use rustls::client::danger::ServerCertVerifier as _;

        let presented = vec![0xABu8; 512];
        let pinned: [u8; 32] = sha2::Sha256::digest(&presented).into();
        let provider = rustls::crypto::ring::default_provider();
        let verifier = FingerprintVerifier { pinned, provider };
        let cert = rustls::pki_types::CertificateDer::from(presented);
        let name = rustls::pki_types::ServerName::try_from("127.0.0.1".to_string())
            .expect("ip server name");

        assert!(
            verifier
                .verify_server_cert(&cert, &[], &name, &[], rustls::pki_types::UnixTime::now())
                .is_ok()
        );

        let other = rustls::pki_types::CertificateDer::from(vec![0xCDu8; 512]);
        let err = verifier
            .verify_server_cert(&other, &[], &name, &[], rustls::pki_types::UnixTime::now())
            .expect_err("mismatching cert must fail");
        let text = match err {
            rustls::Error::General(text) => text,
            other => panic!("unexpected error shape: {other:?}"),
        };
        assert!(text.contains("pinned fingerprint"), "got: {text}");
    }
}
