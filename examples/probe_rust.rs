//! Dev tool: dump the tree-sitter node types a Rust sample produces.
//!
//! Used to build the Rust `LanguageSpec` from what the pinned grammar actually emits,
//! rather than from what its node names are assumed to be. Every other language spec
//! in this crate was derived the same way.

mod support;

const SAMPLE: &str = r####"
pub struct Money { amount: i64, currency: String }

pub trait Shape { fn area(&self) -> f64; fn name(&self) -> &str; }

impl Money {
    pub fn new(amount: i64, currency: String) -> Self { Self { amount, currency } }
    pub fn doubled(&self) -> i64 { self.amount * 2 }
    pub fn classify(&self, a: bool, b: bool, c: &str, d: u32, e: f64) -> Result<u8, Error> {
        let v = compute()?;
        if a && b || c.is_empty() {
            for i in 0..d {
                while i > 0 {
                    match v {
                        1 => return Ok(1),
                        2 if a => return Ok(2),
                        _ => {}
                    }
                    if let Some(x) = maybe() { let _ = x; } else { break }
                }
            }
        } else if b { return Ok(3) } else { return Ok(4) }
        let closure = |z: u32| if z > 1 { 1 } else { 2 };
        loop { break }
        assert!(a);
        assert_eq!(v, 1);
        Ok(closure(d) as u8)
    }
}

impl Shape for Money {
    fn area(&self) -> f64 { 0.0 }
    fn name(&self) -> &str { "money" }
}

enum Kind { A, B }
type Alias = Money;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() { assert_eq!(1, 1); assert!(true); }
}
"####;

/// The node kinds the detectors depend on.
const WANTED: &[&str] = &[
    "impl_item",
    "trait_item",
    "function_item",
    "parameter",
    "self_parameter",
    "closure_expression",
    "if_expression",
    "macro_invocation",
    "struct_item",
    "type_item",
    "enum_item",
];

/// The fields shown for each of them; see `support::run` for the suffixes.
const FIELDS: &[&str] = &["name", "type", "pattern", "condition", "body?", "trait"];

fn main() {
    support::run(tree_sitter_rust::LANGUAGE.into(), SAMPLE, WANTED, FIELDS);
}
