use scratch_lib::{add, is_positive, mean, multiply, shout};

#[test]
fn add_works() {
    assert_eq!(add(2, 3), 5);
    assert_eq!(add(-1, 1), 0);
}

#[test]
fn multiply_works() {
    assert_eq!(multiply(2, 3), 6);
    assert_eq!(multiply(-2, 3), -6);
}

#[test]
fn is_positive_works() {
    assert!(is_positive(1));
    assert!(!is_positive(0));
    assert!(!is_positive(-1));
}

#[test]
fn mean_works() {
    assert_eq!(mean(&[1, 2]), 1.5);
    assert_eq!(mean(&[2, 2]), 2.0);
}

#[test]
fn shout_works() {
    assert_eq!(shout("hi"), "HI!");
}
