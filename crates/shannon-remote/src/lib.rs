//! Remote execution worlds (SSH hosts, Docker containers) for Shannon tools.
//!
//! Implements [`shannon_tool_interface::ProcessProvider`] and
//! [`shannon_tool_interface::FileSystemProvider`] over the system `ssh`
//! client and the `docker` CLI so every registered tool transparently runs
//! against a remote target. See
//! `docs/plans/2026-09-04-remote-connections-design.md`.
//!
//! Note: directory walking (gitignore matcher + default `walk_blocking`)
//! lives in `shannon-tool-interface` so all worlds inherit it; this crate
//! deliberately has no walk module.

pub mod assembly;
pub mod docker;
pub mod dynamic;
pub mod ssh;
pub mod target;

#[cfg(test)]
mod smoke {
    /// The public surface the rest of the workspace relies on must stay
    /// importable; this also keeps the crate meaningful when modules change.
    #[test]
    fn public_surface_links() {
        use shannon_tool_interface::{FileSystemProvider, ProcessProvider};
        fn assert_providers<F: FileSystemProvider, P: ProcessProvider>() {}
        assert_providers::<crate::ssh::SshFs, crate::ssh::SshProcess>();
        let _ = std::mem::size_of::<crate::target::RemotesFile>;
    }
}
