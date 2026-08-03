//! DB helpers — exercises structs + a few top-level fns.

use std::collections::HashMap;

pub struct Connection {
    pub url: String,
}

#[derive(Clone)]
pub struct QueryResult {
    pub rows: Vec<HashMap<String, String>>,
}

impl Connection {
    pub fn open(url: impl Into<String>) -> Self {
        Self { url: url.into() }
    }

    pub fn query(&self, sql: &str) -> QueryResult {
        QueryResult { rows: Vec::new() }
    }
}

pub fn transaction<F: FnOnce() -> R, R>(f: F) -> R {
    f()
}
