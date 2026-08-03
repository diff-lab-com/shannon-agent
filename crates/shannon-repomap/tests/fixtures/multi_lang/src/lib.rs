//! Multi-language fixture root: Rust crate with TS + Python + Go siblings.
//!
//! The accompanying integration tests use this directory as a fixture
//! exercising the incremental update path. Files deliberately contain
//! a representative spread of symbol kinds so the budget trim has
//! something to chew on.

pub mod auth;
pub mod db;

pub fn service_entry(name: &str) -> String {
    format!("hello, {name}")
}

pub struct Config {
    pub host: String,
    pub port: u32,
}

pub enum Mode {
    Dev,
    Staging,
    Prod,
}
