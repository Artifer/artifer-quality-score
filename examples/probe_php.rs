//! Dev tool: dump the tree-sitter node types a PHP sample produces.

mod support;

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

/// The node kinds the detectors depend on.
const WANTED: &[&str] = &[
    "class_declaration",
    "interface_declaration",
    "trait_declaration",
    "enum_declaration",
    "method_declaration",
    "function_definition",
    "anonymous_function",
    "anonymous_function_creation_expression",
    "arrow_function",
    "formal_parameters",
    "simple_parameter",
    "property_declaration",
    "property_element",
    "const_declaration",
    "if_statement",
    "else_if_clause",
    "elseif_clause",
    "else_clause",
    "switch_statement",
    "case_statement",
    "default_statement",
    "match_expression",
    "match_conditional_expression",
    "match_default_expression",
    "try_statement",
    "catch_clause",
    "conditional_expression",
    "member_access_expression",
    "compound_statement",
    "text",
    "text_interpolation",
];

/// The fields shown for each of them; see `support::run` for the suffixes.
const FIELDS: &[&str] = &[
    "name",
    "type",
    "parameters:kind",
    "body:kind",
    "condition",
    "alternative:kind",
];

fn main() {
    support::run(tree_sitter_php::LANGUAGE_PHP.into(), SAMPLE, WANTED, FIELDS);
}
