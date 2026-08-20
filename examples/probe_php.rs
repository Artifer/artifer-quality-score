//! Dev tool: dump the tree-sitter node types a PHP sample produces.

use tree_sitter::{Node, Parser};

const SAMPLE: &str = r####"<?php

namespace Billing;

interface Shape {
    public function area(): float;
}

trait Loggable {
    public function log(string $m): void {}
}

class Invoice implements Shape {
    private int $amount;
    private string $currency;
    public array $lines = [];
    private const TAX = 0.21;

    public function __construct(int $amount, string $currency, bool $a, float $b, int $c, string $d) {
        $this->amount = $amount;
        $this->currency = $currency;
    }

    public function getAmount(): int {
        return $this->amount;
    }

    public static function build(int $amount): self {
        return new self($amount, "EUR", true, 1.0, 1, "x");
    }

    public function classify(string $kind, string $mode, bool $flag, int $other): string {
        $value = $this->lookup($kind);
        if ($kind === "a" && $mode === "x" || $flag) {
            foreach ($value as $item) {
                for ($i = 0; $i < 10; $i++) {
                    switch ($item) {
                        case "one":
                            if ($other !== 0 && $flag) {
                                return "one";
                            }
                            break;
                        default:
                            break;
                    }
                }
            }
        } elseif ($mode === "y") {
            return "y";
        } else {
            try {
                $this->risky();
            } catch (\RuntimeException $e) {
                throw $e;
            } finally {
                $this->cleanup();
            }
        }
        $fn = function ($z) { return $z + 1; };
        $arrow = fn($z) => $z * 2;
        $t = $flag ? 1 : 2;
        $m = match ($t) { 1 => "a", default => "b" };
        while ($other > 0) { $other--; }
        do { $other++; } while ($other < 0);
        return $this->currency;
    }

    private function lookup(string $k): array { return [$k]; }
    private function risky(): void {}
    private function cleanup(): void {}
    public function area(): float { return 0.0; }
}

enum Kind { case A; case B; }
?>
<p>markup line</p>
<?php echo "tail"; ?>
"####;

fn main() {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
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
            "class_declaration", "interface_declaration", "trait_declaration",
            "enum_declaration", "method_declaration", "function_definition",
            "anonymous_function", "anonymous_function_creation_expression",
            "arrow_function", "formal_parameters", "simple_parameter",
            "property_declaration", "property_element", "const_declaration",
            "if_statement", "else_if_clause", "elseif_clause", "else_clause",
            "switch_statement", "case_statement", "default_statement",
            "match_expression", "match_conditional_expression", "match_default_expression",
            "try_statement", "catch_clause", "conditional_expression",
            "member_access_expression", "compound_statement", "text", "text_interpolation",
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
