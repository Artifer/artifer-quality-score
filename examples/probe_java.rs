//! Dev tool: dump the tree-sitter node types a Java sample produces.

use tree_sitter::{Node, Parser};

const SAMPLE: &str = r####"
package billing;

import java.util.List;

public interface Shape {
    double area();
    String name();
}

public abstract class Base {
    protected abstract void run();
}

public class Invoice extends Base implements Shape {
    private final long amount;
    private String currency;

    public Invoice(long amount, String currency, int a, boolean b, double c, String d) {
        this.amount = amount;
        this.currency = currency;
    }

    public long getAmount() {
        return this.amount;
    }

    @Override
    public double area() { return 0.0; }

    public String classify(String kind, String mode, boolean flag, int other, long extra) {
        var value = lookup(kind);
        if (kind.equals("a") && mode.equals("x") || flag) {
            for (int i = 0; i < 10; i++) {
                for (String item : value) {
                    switch (item) {
                        case "one":
                            if (other != 0 && extra != 0 && flag) {
                                return "one";
                            }
                            break;
                        case "two":
                            return "two";
                        default:
                            break;
                    }
                }
            }
        } else if (mode.equals("y")) {
            return "y";
        } else {
            try {
                risky();
            } catch (RuntimeException e) {
                throw e;
            } finally {
                cleanup();
            }
        }
        Runnable inner = () -> { System.out.println("x"); };
        var ternary = flag ? 1 : 2;
        List<String> mapped = value.stream().map(s -> s.trim()).toList();
        while (other > 0) { other--; }
        do { other++; } while (other < 0);
        return ternary > 1 ? "big" : "small";
    }

    private List<String> lookup(String kind) { return List.of(kind); }
    private void risky() {}
    private void cleanup() {}
    void run() {}
}

enum Kind { A, B }

record Money(long amount, String currency) {}
"####;

fn main() {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_java::LANGUAGE.into())
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

    fn dump(node: Node, src: &str, depth: usize, want: &[&str]) {
        if want.contains(&node.kind()) {
            println!(
                "  {}{}  name={:?} type={:?} params={:?} body={:?} cond={:?} alt={:?}",
                "  ".repeat(depth),
                node.kind(),
                node.child_by_field_name("name").map(|n| n.utf8_text(src.as_bytes()).unwrap_or("")),
                node.child_by_field_name("type").map(|n| n.utf8_text(src.as_bytes()).unwrap_or("")),
                node.child_by_field_name("parameters").map(|n| n.kind()),
                node.child_by_field_name("body").map(|n| n.kind()),
                node.child_by_field_name("condition").map(|n| n.utf8_text(src.as_bytes()).unwrap_or("")),
                node.child_by_field_name("alternative").map(|n| n.kind()),
            );
        }
        let mut c = node.walk();
        for ch in node.children(&mut c) {
            dump(ch, src, depth + 1, want);
        }
    }
    println!("--- structure ---");
    dump(
        tree.root_node(),
        SAMPLE,
        0,
        &[
            "class_declaration", "interface_declaration", "enum_declaration",
            "record_declaration", "constructor_declaration", "method_declaration",
            "lambda_expression", "formal_parameters", "formal_parameter",
            "if_statement", "switch_expression", "switch_block_statement_group",
            "switch_rule", "try_statement", "catch_clause", "ternary_expression",
            "field_declaration", "modifiers", "marker_annotation", "annotation",
            "class_body", "block", "local_variable_declaration", "field_access",
        ],
    );
    println!("parse errors: {errors}");
    println!("root: {}", tree.root_node().kind());
    for kind in kinds
        .iter()
        .filter(|k| k.chars().all(|c| c.is_alphanumeric() || c == '_') && k.len() > 2)
    {
        println!("  {kind}");
    }
}
