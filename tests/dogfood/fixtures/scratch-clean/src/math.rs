//! Integer math helpers.

/// Adds two integers.
pub fn add(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

/// Multiplies two integers.
pub fn multiply(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b)
}

/// Returns `true` when `n` is strictly greater than zero.
pub fn is_positive(n: i32) -> bool {
    n > 0
}

/// Arithmetic mean of a slice, as `f64`. Panics on empty input.
pub fn mean(values: &[i32]) -> f64 {
    assert!(!values.is_empty(), "mean of empty slice");
    let sum: i64 = values.iter().map(|&v| v as i64).sum();
    sum as f64 / values.len() as f64
}
