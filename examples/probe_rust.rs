//! Dev tool: dump the tree-sitter node types a Rust sample produces.
//!
//! Used to build the Rust `LanguageSpec` from what the pinned grammar actually emits,
//! rather than from what its node names are assumed to be. Every other language spec
//! in this crate was derived the same way.

use tree_sitter::{Node, Parser};

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

fn main() {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("grammar loads");
    let tree = parser.parse(SAMPLE, None).expect("parses");

    let mut kinds: Vec<String> = Vec::new();
    let mut stack: Vec<Node> = vec![tree.root_node()];
    let mut errors = 0;
    while let Some(node) = stack.pop() {
        if node.kind() == "ERROR" || node.is_missing() {
            errors += 1;
        }
        kinds.push(node.kind().to_string());
        let mut cursor = node.walk();
        stack.extend(node.children(&mut cursor));
    }
    kinds.sort();
    kinds.dedup();

    // Structure of the pieces the detectors depend on.
    fn dump(node: Node, src: &str, depth: usize, want: &[&str]) {
        if want.contains(&node.kind()) {
            let text = node.utf8_text(src.as_bytes()).unwrap_or("");
            let first: String = text.lines().next().unwrap_or("").chars().take(70).collect();
            println!("  {}{}  fields: name={:?} type={:?} pattern={:?} condition={:?} body={:?} trait={:?}",
                "  ".repeat(depth), node.kind(),
                node.child_by_field_name("name").map(|n| n.utf8_text(src.as_bytes()).unwrap_or("")),
                node.child_by_field_name("type").map(|n| n.utf8_text(src.as_bytes()).unwrap_or("")),
                node.child_by_field_name("pattern").map(|n| n.utf8_text(src.as_bytes()).unwrap_or("")),
                node.child_by_field_name("condition").map(|n| n.utf8_text(src.as_bytes()).unwrap_or("")),
                node.child_by_field_name("body").is_some(),
                node.child_by_field_name("trait").map(|n| n.utf8_text(src.as_bytes()).unwrap_or("")));
            let _ = first;
        }
        let mut c = node.walk();
        for ch in node.children(&mut c) { dump(ch, src, depth + 1, want); }
    }
    println!("--- structure ---");
    dump(tree.root_node(), SAMPLE, 0, &["impl_item","trait_item","function_item","parameter","self_parameter","closure_expression","if_expression","macro_invocation","struct_item","type_item","enum_item"]);
    println!("parse errors: {errors}");
    println!("root: {}", tree.root_node().kind());
    for kind in kinds
        .iter()
        .filter(|k| k.chars().all(|c| c.is_alphanumeric() || c == '_') && k.len() > 2)
    {
        println!("  {kind}");
    }
}
