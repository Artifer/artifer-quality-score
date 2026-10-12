//! The body every `probe_*` dev tool shares.
//!
//! A probe is a sample of one language plus the node kinds and fields worth looking at;
//! parsing it, walking it and printing what the pinned grammar emitted is the same for
//! all of them, so it lives here once.

use tree_sitter::{Language, Node, Parser, Tree};

/// Parse `sample`, print the structure of the `wanted` node kinds, then every node kind
/// the sample produced.
///
/// Each entry of `fields` names a field to show per wanted node. A bare name prints the
/// field's source text, `name:kind` prints the node kind found there, and `name?` prints
/// only whether the field is present.
pub fn run(language: Language, sample: &str, wanted: &[&str], fields: &[&str]) {
    let mut parser = Parser::new();
    parser.set_language(&language).expect("grammar loads");
    let tree = parser.parse(sample, None).expect("parses");

    println!("--- structure ---");
    dump(tree.root_node(), sample, 0, &Probe { wanted, fields });

    let (kinds, errors) = kinds_and_errors(&tree);
    println!("parse errors: {errors}");
    println!("root: {}", tree.root_node().kind());
    for kind in kinds.iter().filter(|kind| is_named_kind(kind)) {
        println!("  {kind}");
    }
}

/// What a probe asks to see.
struct Probe<'a> {
    wanted: &'a [&'a str],
    fields: &'a [&'a str],
}

fn dump(node: Node, source: &str, depth: usize, probe: &Probe) {
    if probe.wanted.contains(&node.kind()) {
        let shown: Vec<String> = probe
            .fields
            .iter()
            .map(|field| describe(&node, source, field))
            .collect();
        println!(
            "  {}{}  {}",
            "  ".repeat(depth),
            node.kind(),
            shown.join(" ")
        );
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        dump(child, source, depth + 1, probe);
    }
}

/// One `field=value` entry of a structure line.
fn describe(node: &Node, source: &str, field: &str) -> String {
    if let Some(name) = field.strip_suffix('?') {
        return format!("{name}={:?}", node.child_by_field_name(name).is_some());
    }
    if let Some(name) = field.strip_suffix(":kind") {
        let kind = node.child_by_field_name(name).map(|child| child.kind());
        return format!("{name}={kind:?}");
    }
    let text = node
        .child_by_field_name(field)
        .map(|child| child.utf8_text(source.as_bytes()).unwrap_or(""));
    format!("{field}={text:?}")
}

/// Every distinct node kind in the tree, sorted, and how many nodes failed to parse.
fn kinds_and_errors(tree: &Tree) -> (Vec<String>, usize) {
    let mut kinds: Vec<String> = Vec::new();
    let mut errors = 0;
    let mut stack: Vec<Node> = vec![tree.root_node()];
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
    (kinds, errors)
}

/// Punctuation and operators are node kinds too; only the named ones are worth listing.
fn is_named_kind(kind: &str) -> bool {
    kind.len() > 2 && kind.chars().all(|c| c.is_alphanumeric() || c == '_')
}
