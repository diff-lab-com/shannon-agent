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
//! SELF-SIGNED certificate with the fingerprint published in
//! `~/.shannon/mobile-tls/tls-info.json` for phones to pin. This client
//! accepts any cert (`danger_accept_invalid_certs`) — chain validation is
//! impossible against a self-signed CA, and the request is already
//! authenticated by the single-use pair token carried in the body. That
//! matches the QR threat model: a network observer who can see the token is
//! out of the established trust boundary (the same token also rides the QR
//! and the plaintext-fallback `ws://` flow).

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::commands_connections::{gateway_read_config, GatewayConfig};
use crate::commands_mobile_pairing::{mint_pair_token, DEFAULT_MOBILE_PORT};

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
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        // See module docs: the gateway intentionally serves a self-signed
        // cert; auth is the single-use pair token, not the TLS chain.
        .danger_accept_invalid_certs(true)
        .build()
        .map_err(|e| format!("pairing RPC: client build failed: {e}"))?;
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
pub(crate) fn parse_pending_result(value: serde_json::Value) -> Result<Vec<GatewayPairingRequest>, String> {
    let body: PendingBody = serde_json::from_value(value)
        .map_err(|e| format!("pairing RPC: unexpected pending response shape: {e}"))?;
    Ok(body.result.pending)
}

/// Parse an `approve` result into the approved record. Pure likewise.
pub(crate) fn parse_approve_result(value: serde_json::Value) -> Result<GatewayPairingRequest, String> {
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
    let response = pairing_rpc_post(&base, PAIRING_PENDING_PATH, serde_json::json!({ "token": token }))
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

    fn config_with_mobile(mobile: Option<crate::commands_connections::GatewayMobileConfig>) -> GatewayConfig {
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
        let cfg = config_with_mobile(Some(mobile_block(true, Some("192.168.1.10"), Some(3399), false)));
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
        assert_eq!(pending[0], GatewayPairingRequest {
            code: "012345".into(),
            platform: "slack".into(),
            sender_id: "U123".into(),
            requested_at: 1000,
            expires_at: 1300,
        });
        // camelCase serializes back for the UI contract.
        let json = serde_json::to_string(&pending[0]).expect("serialize");
        assert!(json.contains("\"senderId\""));
        assert!(json.contains("\"requestedAt\""));
    }

    #[test]
    fn parses_pending_empty_and_rejects_garbage() {
        let empty = parse_pending_result(serde_json::json!({ "result": {} })).expect("parse");
        assert!(empty.is_empty());
        assert!(parse_pending_result(serde_json::json!({ "result": { "pending": "nope" } })).is_err());
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
        use axum::routing::{get, post};
        use axum::Json;

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
        assert!(err.contains("Unknown or expired pairing code"), "got: {err}");
        assert!(err.contains("-32001"), "error carries the gateway code: {err}");

        // Unreachable base URL → Err mentioning the gateway.
        let down = pairing_rpc_post("http://127.0.0.1:1", PAIRING_PENDING_PATH, serde_json::json!({}))
            .await
            .expect_err("unreachable gateway is an Err");
        assert!(down.contains("cannot reach the gateway"), "got: {down}");

        server.abort();
    }
}
