//! Shared reqwest builder for the desktop's own outbound HTTP clients.
//!
//! Settings R3 T4 (B1): the workspace locks reqwest on `rustls-tls` +
//! webpki-roots — it never reads the system certificate store — so a
//! corporate MITM / inspection CA must be added explicitly or every
//! desktop-side HTTPS call fails its handshake behind such a proxy. This
//! module hands out a [`reqwest::ClientBuilder`] pre-loaded with the custom
//! roots named by `SHANNON_CA_BUNDLE` (the same env var — and the same
//! parsing / warn-and-skip semantics — the engine's LLM client uses, via
//! `shannon_engine::api::client::apply_custom_root_certificates`).
//!
//! Scope (settings R3 plan): wired into the provider probing /
//! model-listing clients and the models.dev catalog refresh (which reads the
//! same helper from shannon-core). Other one-off desktop clients (extension
//! catalogs, voice models, gateway pairing) are intentionally NOT migrated
//! in this round — they are listed in the PR description as follow-ups.

/// A `reqwest::ClientBuilder` with the `SHANNON_CA_BUNDLE` roots (if any)
/// already added. Chain the call-specific tuning (timeouts, headers) on top
/// as usual:
///
/// ```ignore
/// let client = crate::desktop_http::builder()
///     .timeout(Duration::from_secs(15))
///     .build()?;
/// ```
pub fn builder() -> reqwest::ClientBuilder {
    shannon_engine::api::client::apply_custom_root_certificates(reqwest::Client::builder())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unset env → the helper is a plain `Client::builder()` equivalent:
    /// builders always construct, so this asserts no panic wiring.
    #[test]
    fn builder_constructs_without_env() {
        unsafe { std::env::remove_var("SHANNON_CA_BUNDLE") };
        let built = builder().timeout(std::time::Duration::from_secs(1)).build();
        assert!(built.is_ok());
    }

    #[test]
    fn builder_with_missing_bundle_file_still_builds() {
        // Warn-and-skip contract: a dangling path never blocks the client.
        unsafe { std::env::set_var("SHANNON_CA_BUNDLE", "/nonexistent/desktop-http-ca.pem") };
        let built = builder().build();
        unsafe { std::env::remove_var("SHANNON_CA_BUNDLE") };
        assert!(built.is_ok());
    }
}
