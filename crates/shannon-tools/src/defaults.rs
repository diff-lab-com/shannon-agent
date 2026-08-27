//! Default execution-world handles for tools that are constructed without an
//! explicit provider injection (§4.11 W3-3a).
//!
//! Tool constructors keep their historical signatures and fall back to the
//! local world (`shannon_core::providers::LocalFs` / `LocalProcess`). Whole
//! execution worlds (sandbox, remote) are swapped at assembly time via
//! `register_default_tools_with_providers`, which threads injected providers
//! through every tool instead of these defaults.

use shannon_tool_interface::{FileSystemProvider, ProcessProvider};
use std::sync::Arc;

/// Default filesystem world handle.
pub fn fs() -> Arc<dyn FileSystemProvider> {
    shannon_core::providers::LocalFs::shared()
}

/// Default process world handle.
pub fn process() -> Arc<dyn ProcessProvider> {
    Arc::new(shannon_core::providers::LocalProcess::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The defaults must be real `FileSystemProvider`/`ProcessProvider`
    /// trait objects so every tool field initializes identically whether the
    /// provider came from here or from assembly-time injection.
    #[test]
    fn defaults_are_boxed_providers() {
        let fs = fs();
        // Spot-check a harmless read: a directory listing of `/` may vary,
        // so only assert the call itself succeeds or fails with io error —
        // never panics.
        let _ = fs.list_dir_blocking(std::path::Path::new("/"));

        let proc = process();
        assert_eq!(
            std::mem::size_of_val(proc.as_ref()),
            std::mem::size_of::<shannon_core::providers::LocalProcess>(),
            "default process provider is LocalProcess"
        );
    }
}
