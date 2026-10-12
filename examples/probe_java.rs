//! Dev tool: dump the tree-sitter node types a Java sample produces.

mod support;

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

/// The node kinds the detectors depend on.
const WANTED: &[&str] = &[
    "class_declaration",
    "interface_declaration",
    "enum_declaration",
    "record_declaration",
    "constructor_declaration",
    "method_declaration",
    "lambda_expression",
    "formal_parameters",
    "formal_parameter",
    "if_statement",
    "switch_expression",
    "switch_block_statement_group",
    "switch_rule",
    "try_statement",
    "catch_clause",
    "ternary_expression",
    "field_declaration",
    "modifiers",
    "marker_annotation",
    "annotation",
    "class_body",
    "block",
    "local_variable_declaration",
    "field_access",
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
    support::run(tree_sitter_java::LANGUAGE.into(), SAMPLE, WANTED, FIELDS);
}
