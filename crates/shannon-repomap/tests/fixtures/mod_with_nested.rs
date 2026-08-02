// Fixture: mod_with_nested.rs
// Exercises nested modules, traits, and impls — the test asserts the
// children get attached under their enclosing item rather than promoted
// to the file root.

pub mod inner {
    pub fn helper(x: i32) -> i32 {
        x + 1
    }

    pub struct Holder {
        pub value: i32,
    }

    impl Holder {
        pub fn new(v: i32) -> Self {
            Self { value: v }
        }

        pub fn doubled(&self) -> i32 {
            self.value * 2
        }
    }
}

pub trait Greeter {
    fn greet(&self) -> String;
}

pub struct Person {
    pub name: String,
}

impl Greeter for Person {
    fn greet(&self) -> String {
        format!("hello, {}", self.name)
    }
}