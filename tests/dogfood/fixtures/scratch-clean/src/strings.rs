//! String helpers.

/// Upper-cases `s` and appends an exclamation mark.
pub fn shout(s: &str) -> String {
    format!("{}!", s.to_uppercase())
}
