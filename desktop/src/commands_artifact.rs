//! `artifact://` custom protocol — interactive HTML artifacts (design doc
//! docs/plans/2026-09-26-desktop-chat-ui-round2-design.md §5-1, decision A).
//!
//! Chat-fence HTML used to render in a srcdoc iframe with `sandbox=""`:
//! honest static, because the production CSP (`script-src 'self'`) kills all
//! scripts in srcdoc children (multiple policies intersect). Decision A
//! serves the artifact document from this custom URI scheme protocol so it
//! gets its **own response-header CSP** and can really run scripts — inside
//! a strictly sandboxed iframe (`allow-scripts allow-forms` only; the
//! document stays opaque-origin with no host DOM/storage access).
//!
//! Security posture (non-negotiable, enforced in [`serve_artifact_request`]):
//! * every artifact response carries a single strict CSP —
//!   `default-src 'none'; script-src 'unsafe-inline'; style-src
//!   'unsafe-inline'; img-src data:; font-src data:; connect-src 'none';
//!   form-action 'none';` — no network, no subresource/frame loading at all;
//! * `Cache-Control: no-store` + `X-Content-Type-Options: nosniff`;
//! * ids are unguessable tokens (128 random bits, hex-encoded); lookups
//!   reject anything outside `^[A-Za-z0-9_-]{1,64}$`
//!   (traversal, query strings, extra segments, unicode → 404);
//! * content is capped at 2 MiB (same cap as `open_artifact_externally`),
//!   the registry is capped at 64 entries with oldest-eviction, and
//!   unregister removes the content.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;
use tauri::http::header::{
    CACHE_CONTROL, CONTENT_SECURITY_POLICY, CONTENT_TYPE, X_CONTENT_TYPE_OPTIONS,
};
use tauri::http::{Request, Response, StatusCode};

/// Byte cap for one interactive artifact — same as
/// `commands_surface::MAX_ARTIFACT_EXPORT_BYTES` (open_artifact_externally).
pub const MAX_ARTIFACT_BYTES: usize = 2 * 1024 * 1024;

/// Registry entry cap; registering past it evicts the oldest entry.
pub const MAX_REGISTRY_ENTRIES: usize = 64;

/// The custom URI scheme name. Registered in `main.rs`
/// (`register_uri_scheme_protocol`); the main-app CSP frame-src must list
/// this scheme (plus the Windows host form) or the webview blocks the frame.
pub const ARTIFACT_SCHEME: &str = "artifact";

/// The one CSP the artifact document ever sees. Sent on every response; see
/// the module docs for why each directive is closed.
const ARTIFACT_CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; font-src data:; connect-src 'none'; form-action 'none';";

/// Result of [`register_interactive_artifact`]: the id (for later
/// unregister) and the ready-to-load iframe URL (platform-shaped, computed
/// Rust-side so the frontend never guesses).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactRegistration {
    pub id: String,
    pub url: String,
}

/// Managed state: id → HTML for the currently registered interactive
/// artifacts, in insertion order. Every accessor takes the full lock; the
/// critical sections are memcpy-scale, so a std Mutex is plenty (and the
/// commands below never hold it across an await).
#[derive(Debug, Default)]
pub struct InteractiveArtifactRegistry {
    /// `(id, html)` pairs, oldest first (the front of the queue).
    entries: Mutex<VecDeque<(String, String)>>,
}

/// `^[A-Za-z0-9_-]{1,64}$` — one unambiguous path segment, nothing else.
/// Rejects traversal (`..`), separators, percent-escapes, unicode and
/// oversized ids before any lookup happens.
fn is_valid_artifact_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// `a` + 32 hex chars (128 random bits): an id an attacker cannot guess or
/// enumerate. A wall-clock+counter scheme would leave only ~1e6·counter
/// candidates per millisecond to anyone who can issue requests.
fn fresh_artifact_id() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!("a{hex}")
}

impl InteractiveArtifactRegistry {
    /// Store `html` under a fresh unguessable id and return it. Evicts the
    /// oldest entry when the registry is over its cap.
    pub fn register(&self, html: String) -> String {
        let id = fresh_artifact_id();

        let mut entries = self.entries.lock().expect("artifact registry poisoned");
        entries.push_back((id.clone(), html));
        while entries.len() > MAX_REGISTRY_ENTRIES {
            let _ = entries.pop_front();
        }
        id
    }

    /// Remove an id. Missing ids are not an error (the UI may unregister a
    /// stale id after an eviction race) — returns whether it existed.
    pub fn unregister(&self, id: &str) -> bool {
        let mut entries = self.entries.lock().expect("artifact registry poisoned");
        match entries.iter().position(|(k, _)| k == id) {
            Some(index) => {
                entries.remove(index);
                true
            }
            None => false,
        }
    }

    /// Look up the HTML for a validated id.
    pub fn lookup(&self, id: &str) -> Option<String> {
        if !is_valid_artifact_id(id) {
            return None;
        }
        let entries = self.entries.lock().expect("artifact registry poisoned");
        entries
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, html)| html.clone())
    }
}

/// The URL shape the platform webview serves custom schemes under (tauri
/// 2.11.x `register_uri_scheme_protocol` docs):
/// * Windows/Android: `http(s)://<scheme>.localhost/<path>` — `http` unless
///   the window opts into `useHttpsScheme`; this app does not, so the CSP
///   frame-src lists `http://artifact.localhost` explicitly;
/// * macOS/Linux: `<scheme>://localhost/<path>`.
///
/// Computed here (not in the frontend) so the URL is always consistent with
/// the scheme this binary actually registered.
fn artifact_url(id: &str) -> String {
    debug_assert!(is_valid_artifact_id(id));
    #[cfg(windows)]
    {
        format!("http://{ARTIFACT_SCHEME}.localhost/{id}")
    }
    #[cfg(not(windows))]
    {
        format!("{ARTIFACT_SCHEME}://localhost/{id}")
    }
}

/// Extract the artifact id from a protocol request. Accepts exactly one
/// path segment matching [`is_valid_artifact_id`] — anything else (empty,
/// extra segments, query string, traversal, percent-escapes) is None.
fn artifact_id_from_request(request: &Request<Vec<u8>>) -> Option<String> {
    if request.uri().query().is_some() {
        return None;
    }
    let segment = request.uri().path().strip_prefix('/')?;
    if segment.contains('/') || !is_valid_artifact_id(segment) {
        return None;
    }
    Some(segment.to_string())
}

/// A 404 with an empty body — used for unknown/expired ids and for
/// malformed requests (they must be indistinguishable). Also the fallback
/// when the managed registry is not (yet) installed.
pub fn not_found_response() -> Response<Vec<u8>> {
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(CACHE_CONTROL, "no-store")
        .body(Vec::new())
        .expect("static 404 response parts are valid")
}

fn artifact_response(status: StatusCode, body: Vec<u8>) -> Response<Vec<u8>> {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .header(CONTENT_SECURITY_POLICY, ARTIFACT_CSP)
        .header(CACHE_CONTROL, "no-store")
        .header(X_CONTENT_TYPE_OPTIONS, "nosniff")
        .body(body)
        .expect("static artifact response parts are valid")
}

/// Pure request → response handler, split from the tauri closure in `main.rs`
/// so tests can drive it without an app runtime.
pub fn serve_artifact_request(
    registry: &InteractiveArtifactRegistry,
    request: Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    match artifact_id_from_request(&request)
        .and_then(|id| registry.lookup(&id).map(|html| (id, html)))
    {
        Some((_, html)) => artifact_response(StatusCode::OK, html.into_bytes()),
        None => not_found_response(),
    }
}

/// §5-1 A: store an interactive HTML artifact and return its sandboxed URL.
///
/// Sync (no await inside) on purpose — the registry lock is held for one
/// VecDeque push; making this async would only add a runtime hop.
#[tauri::command]
pub fn register_interactive_artifact(
    state: tauri::State<'_, InteractiveArtifactRegistry>,
    html: String,
) -> Result<ArtifactRegistration, String> {
    if html.is_empty() {
        return Err("interactive artifact HTML is empty".to_string());
    }
    if html.len() > MAX_ARTIFACT_BYTES {
        return Err(format!(
            "interactive artifact too large: {} bytes (max {MAX_ARTIFACT_BYTES})",
            html.len()
        ));
    }
    let id = state.register(html);
    Ok(ArtifactRegistration {
        url: artifact_url(&id),
        id,
    })
}

/// §5-1 A: drop a registered artifact (its content is removed immediately).
/// Unregistering an unknown/expired id is not an error.
#[tauri::command]
pub fn unregister_interactive_artifact(
    state: tauri::State<'_, InteractiveArtifactRegistry>,
    id: String,
) -> Result<(), String> {
    state.unregister(&id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        InteractiveArtifactRegistry, MAX_ARTIFACT_BYTES, MAX_REGISTRY_ENTRIES,
        artifact_id_from_request, artifact_url, is_valid_artifact_id, not_found_response,
        serve_artifact_request,
    };
    use tauri::http::{Request, StatusCode};

    fn request_for(uri: &str) -> Request<Vec<u8>> {
        Request::builder().uri(uri).body(Vec::new()).unwrap()
    }

    // -- id validation -------------------------------------------------------

    #[test]
    fn ids_accept_only_one_safe_segment() {
        assert!(is_valid_artifact_id("a"));
        assert!(is_valid_artifact_id("aB_9-x"));
        assert!(is_valid_artifact_id(&"x".repeat(64)));
        assert!(!is_valid_artifact_id(&"x".repeat(65)));
        assert!(!is_valid_artifact_id(""));
        // traversal / separators / escapes / unicode / dots / spaces
        assert!(!is_valid_artifact_id(".."));
        assert!(!is_valid_artifact_id("a/b"));
        assert!(!is_valid_artifact_id("a%2Fb"));
        assert!(!is_valid_artifact_id("ä"));
        assert!(!is_valid_artifact_id("a.b"));
        assert!(!is_valid_artifact_id("a b"));
        assert!(!is_valid_artifact_id("a\nb"));
    }

    // -- registry ------------------------------------------------------------

    #[test]
    fn register_lookup_unregister_roundtrip() {
        let registry = InteractiveArtifactRegistry::default();
        let id = registry.register("<p>hi</p>".to_string());
        assert!(is_valid_artifact_id(&id));
        assert_eq!(registry.lookup(&id).as_deref(), Some("<p>hi</p>"));

        assert!(registry.unregister(&id));
        assert_eq!(registry.lookup(&id), None);
        // unregistering again is a silent no-op
        assert!(!registry.unregister(&id));
    }

    #[test]
    fn lookup_rejects_malformed_ids_before_touching_the_registry() {
        let registry = InteractiveArtifactRegistry::default();
        let id = registry.register("x".to_string());
        for bad in ["", "../etc/passwd", "a/b", &"x".repeat(65)] {
            assert_eq!(registry.lookup(bad), None, "lookup must reject {bad:?}");
        }
        assert!(registry.lookup(&id).is_some());
    }

    #[test]
    fn ids_are_unguessable_and_unique() {
        let registry = InteractiveArtifactRegistry::default();
        let ids: Vec<String> = (0..256).map(|_| registry.register("x".into())).collect();
        let unique: std::collections::BTreeSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), ids.len());
        for id in &ids {
            assert!(id.len() <= 64, "generated id {id} must satisfy its own cap");
            assert!(is_valid_artifact_id(id));
        }
    }

    #[test]
    fn registry_cap_evicts_oldest() {
        let registry = InteractiveArtifactRegistry::default();
        let first = registry.register("first".to_string());
        let mut ids = Vec::new();
        for i in 0..MAX_REGISTRY_ENTRIES {
            ids.push(registry.register(format!("n{i}")));
        }
        // now at the cap — registering again must evict the oldest entry.
        let last = registry.register("last".to_string());
        assert!(registry.lookup(&last).is_some());
        assert!(
            registry.lookup(ids.last().unwrap()).is_some(),
            "newest pre-cap entry survives"
        );
        assert_eq!(registry.lookup(&first), None, "oldest entry evicted");
        assert_eq!(
            registry.lookup(&ids[0]),
            None,
            "eviction works in insertion order"
        );
        let count = registry.entries.lock().expect("registry poisoned").len();
        assert_eq!(count, MAX_REGISTRY_ENTRIES);
    }

    #[test]
    fn size_cap_constant_matches_export_cap() {
        assert_eq!(MAX_ARTIFACT_BYTES, 2 * 1024 * 1024);
    }

    // -- protocol handler ----------------------------------------------------

    #[test]
    fn registered_id_serves_200_with_security_headers_and_body() {
        let registry = InteractiveArtifactRegistry::default();
        let id = registry.register("<html><body>x</body></html>".to_string());
        let resp = serve_artifact_request(&registry, request_for(&format!("/{id}")));
        assert_eq!(resp.status(), StatusCode::OK);
        let headers = resp.headers();
        assert_eq!(
            headers.get("Content-Security-Policy").unwrap(),
            "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; \
             img-src data:; font-src data:; connect-src 'none'; form-action 'none';"
        );
        assert_eq!(headers.get("Cache-Control").unwrap(), "no-store");
        assert_eq!(headers.get("X-Content-Type-Options").unwrap(), "nosniff");
        assert_eq!(
            headers.get("Content-Type").unwrap(),
            "text/html; charset=utf-8"
        );
        assert_eq!(resp.into_body(), b"<html><body>x</body></html>".to_vec());
    }

    #[test]
    fn unknown_and_expired_ids_serve_404() {
        let registry = InteractiveArtifactRegistry::default();
        let resp = serve_artifact_request(&registry, request_for("/a123-zzz"));
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert!(resp.into_body().is_empty());

        let id = registry.register("gone".to_string());
        registry.unregister(&id);
        let resp = serve_artifact_request(&registry, request_for(&format!("/{id}")));
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn malformed_requests_serve_404() {
        let registry = InteractiveArtifactRegistry::default();
        registry.register("x".to_string());
        let overlong = format!("/{}", "x".repeat(65));
        let bad = [
            "/",               // empty segment
            "/a/b",            // extra segment
            "/../etc/passwd",  // traversal
            overlong.as_str(), // over-long id
            "/a%2Fb",          // percent-escape
            "/ä",              // unicode
        ];
        for uri in bad {
            let resp = serve_artifact_request(&registry, request_for(uri));
            assert_eq!(resp.status(), StatusCode::NOT_FOUND, "uri {uri:?}");
            assert!(resp.into_body().is_empty(), "uri {uri:?}");
        }
        // query strings are rejected too
        let resp = serve_artifact_request(&registry, request_for("/a1?x=1"));
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn request_parser_accepts_only_the_single_valid_segment_form() {
        assert_eq!(
            artifact_id_from_request(&request_for("/abc_XY-9")).as_deref(),
            Some("abc_XY-9")
        );
        assert_eq!(artifact_id_from_request(&request_for("/abc?x")), None);
        assert_eq!(artifact_id_from_request(&request_for("/abc/de")), None);
        // Both real-world request shapes resolve to the same path segment:
        // `artifact://localhost/<id>` (Linux/macOS) and
        // `http://artifact.localhost/<id>` (Windows) — host never matters.
        assert_eq!(
            artifact_id_from_request(&request_for("artifact://localhost/abc")).as_deref(),
            Some("abc")
        );
        assert_eq!(
            artifact_id_from_request(&request_for("http://artifact.localhost/abc")).as_deref(),
            Some("abc")
        );
    }

    #[test]
    fn not_found_response_has_no_store() {
        let resp = not_found_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(resp.headers().get("Cache-Control").unwrap(), "no-store");
    }

    // -- URL shapes ----------------------------------------------------------

    #[test]
    #[cfg(windows)]
    fn artifact_url_uses_windows_host_form() {
        assert_eq!(artifact_url("a1"), "http://artifact.localhost/a1");
    }

    #[test]
    #[cfg(not(windows))]
    fn artifact_url_uses_custom_scheme_form() {
        assert_eq!(artifact_url("a1"), "artifact://localhost/a1");
    }
}
