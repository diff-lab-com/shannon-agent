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

pub mod target;
pub mod ssh;
pub mod docker;
pub mod dynamic;
pub mod assembly;

#[cfg(test)]
mod smoke {
    #[test]
    fn crate_links() {
        assert!(true);
    }
}
