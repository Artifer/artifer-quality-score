//! Dev tool: dump the tree-sitter node types a Ruby sample produces.
//!
//! The companion of `probe_rust` and `probe_go`. Every language spec in this crate was
//! derived from what the pinned grammar actually emits, rather than from what its node
//! names are assumed to be.

mod support;

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

/// The node kinds the detectors depend on.
const WANTED: &[&str] = &[
    "class",
    "module",
    "singleton_class",
    "method",
    "singleton_method",
    "block",
    "do_block",
    "lambda",
    "if",
    "elsif",
    "else",
    "unless",
    "case",
    "when",
    "while",
    "until",
    "begin",
    "rescue",
    "body_statement",
    "then",
    "method_parameters",
    "block_parameters",
    "lambda_parameters",
    "instance_variable",
    "assignment",
    "call",
    "if_modifier",
    "unless_modifier",
];

/// The fields shown for each of them; see `support::run` for the suffixes.
const FIELDS: &[&str] = &[
    "name",
    "parameters",
    "condition",
    "alternative:kind",
    "body:kind",
    "receiver",
];

fn main() {
    support::run(tree_sitter_ruby::LANGUAGE.into(), SAMPLE, WANTED, FIELDS);
}
