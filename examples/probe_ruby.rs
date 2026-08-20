//! Dev tool: dump the tree-sitter node types a Ruby sample produces.
//!
//! The companion of `probe_rust` and `probe_go`. Every language spec in this crate was
//! derived from what the pinned grammar actually emits, rather than from what its node
//! names are assumed to be.

use tree_sitter::{Node, Parser};

const SAMPLE: &str = r####"
module Billing
  class Invoice < Record
    TAX = 0.21

    def initialize(amount, currency)
      @amount = amount
      @currency = currency
      @lines = []
    end

    def total
      @amount
    end

    def self.build(amount)
      new(amount, "EUR")
    end

    def classify(a, b, c = 1, *rest, key: nil, **opts, &blk)
      value, err = compute
      if a && b || c.empty?
        (0..10).each do |i|
          rest.each { |item| puts item }
          case value
          when 1 then return 1
          when 2 then return 2
          else
            nil
          end
          while i > 0
            i -= 1
          end
          until i > 5
            i += 1
          end
        end
      elsif b
        return 3
      else
        return 4
      end

      helper = lambda { |z| z > 1 ? 1 : 2 }
      other = ->(z) { z + 1 }

      begin
        risky
      rescue StandardError => e
        retry_it
      ensure
        cleanup
      end

      return 5 if a
      value.to_i unless b
      @lines.map { |l| l.to_s }
    end

    private

    def compute
      [1, nil]
    end
  end
end

RSpec.describe Invoice do
  it "adds up" do
    expect(subject.total).to eq(1)
    assert_equal 1, subject.total
  end
end
"####;

fn main() {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_ruby::LANGUAGE.into())
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
                "  {}{}  name={:?} parameters={:?} condition={:?} alternative={:?} body={:?} receiver={:?}",
                "  ".repeat(depth),
                node.kind(),
                node.child_by_field_name("name").map(|n| n.utf8_text(src.as_bytes()).unwrap_or("")),
                node.child_by_field_name("parameters").map(|n| n.utf8_text(src.as_bytes()).unwrap_or("")),
                node.child_by_field_name("condition").map(|n| n.utf8_text(src.as_bytes()).unwrap_or("")),
                node.child_by_field_name("alternative").map(|n| n.kind()),
                node.child_by_field_name("body").map(|n| n.kind()),
                node.child_by_field_name("receiver").map(|n| n.utf8_text(src.as_bytes()).unwrap_or("")),
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
            "class", "module", "singleton_class", "method", "singleton_method", "block",
            "do_block", "lambda", "if", "elsif", "else", "unless", "case", "when",
            "while", "until", "begin", "rescue", "body_statement", "then",
            "method_parameters", "block_parameters", "lambda_parameters",
            "instance_variable", "assignment", "call", "if_modifier", "unless_modifier",
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
