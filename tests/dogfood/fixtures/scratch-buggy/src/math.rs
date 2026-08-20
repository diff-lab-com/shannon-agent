//! Integer math helpers. (Dogfood fixture: contains two PRESET BUGS that the
//! s3/m5 tasks must fix; every workspace copy starts broken on purpose.)

/// Adds two integers.
pub fn add(a: i32, b: i32) -> i32 {
    a.wrapping_add(b)
}

/// Multiplies two integers.
pub fn multiply(a: i32, b: i32) -> i32 {
    a.wrapping_mul(b)
}

/// BUG: `n >= 0` treats zero as positive; tests require strictly `n > 0`.
pub fn is_positive(n: i32) -> bool {
    n >= 0
}

/// BUG: integer division truncates; mean(&[1,2]) yields 0.5 less than the
/// tests' expected 1.5.
pub fn mean(values: &[i32]) -> f64 {
    assert!(!values.is_empty(), "mean of empty slice");
    let sum: i64 = values.iter().map(|&v| v as i64).sum();
    (sum / values.len() as i64) as f64
}
