// Greets the user.
use std::fmt;

/// A greeter.
pub struct Greeter {
    name: String,
}

impl fmt::Display for Greeter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Hello, {}", self.name)
    }
}

fn main() {
    let g = Greeter { name: "vr".to_string() };
    println!("{g} {}", 42);
}
