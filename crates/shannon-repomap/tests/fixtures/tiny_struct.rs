// Fixture: tiny_struct.rs
// Smallest possible Rust file with multiple top-level declarations
// so the parser tests can assert each kind surfaces correctly.

pub struct Point {
    pub x: f64,
    pub y: f64,
}

pub enum Color {
    Red,
    Green,
    Blue,
}

pub type Alias = Point;

pub const ORIGIN: Point = Point { x: 0.0, y: 0.0 };