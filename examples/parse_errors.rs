//! Dev tool: print the source text tree-sitter failed to parse.
//!
//! When `aqs scan` reports syntax errors, the score for those files is unreliable and
//! the cause is usually a language construct newer than the pinned grammar. This shows
//! exactly which text the parser rejected, so you can tell "our grammar is too old"
//! apart from "this file is genuinely broken".
//!
//! ```text
//! cargo run --example parse_errors -- path/to/file.tsx [more.ts ...]
//! ```

use aqs::languages::spec_for_path;
use aqs::metrics::all_nodes;
use tree_sitter::Parser;

fn main() {
    for path in std::env::args().skip(1) {
        let Some(spec) = spec_for_path(&path) else {
            eprintln!("{path}: unsupported file type");
            continue;
        };
        let Ok(source) = std::fs::read(&path) else {
            eprintln!("{path}: cannot read");
            continue;
        };

        let mut parser = Parser::new();
        parser
            .set_language(&spec.language())
            .expect("grammar loads");
        let Some(tree) = parser.parse(&source, None) else {
            eprintln!("{path}: parser returned nothing");
            continue;
        };

        for node in all_nodes(&tree.root_node()) {
            if node.kind() != "ERROR" && !node.is_missing() {
                continue;
            }
            let line = node.start_position().row + 1;
            let text = node.utf8_text(&source).unwrap_or("<binary>");
            let shown: String = text.chars().take(120).collect();
            let label = if node.is_missing() {
                "MISSING"
            } else {
                "ERROR"
            };
            println!("{path}:{line}: {label} {:?}", shown.trim());
        }
    }
}
