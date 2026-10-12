//! Dev tool: dump the tree-sitter node types a Go sample produces.
//!
//! The companion of `probe_rust`. Every language spec in this crate was derived from
//! what the pinned grammar actually emits, rather than from what its node names are
//! assumed to be.

mod support;

const SAMPLE: &str = r####"
package money

import "fmt"

type Money struct {
	amount   int64
	currency string
}

type Shape interface {
	Area() float64
	Name() string
}

type Alias = Money

const Rate float64 = 1.5

func New(amount int64, currency string) *Money {
	return &Money{amount: amount, currency: currency}
}

func (m *Money) Doubled() int64 { return m.amount * 2 }

func (m Money) Classify(a bool, b bool, c string, d uint, e float64) (uint8, error) {
	v, err := compute()
	if err != nil {
		return 0, err
	}
	if a && b || c == "" {
		for i := uint(0); i < d; i++ {
			for _, item := range items {
				switch v {
				case 1:
					return 1, nil
				case 2:
					return 2, nil
				default:
				}
				switch t := any(v).(type) {
				case int:
					_ = t
				}
				select {
				case <-done:
					break
				default:
				}
			}
		}
	} else if b {
		return 3, nil
	} else {
		return 4, nil
	}
	closure := func(z uint) uint8 {
		if z > 1 {
			return 1
		}
		return 2
	}
	go func() { fmt.Println("x") }()
	defer func() { _ = e }()
	return closure(d), nil
}

func TestClassify(t *testing.T) {
	if got := 1; got != 1 {
		t.Errorf("bad")
	}
	require.NoError(t, nil)
	assert.Equal(t, 1, 1)
}
"####;

/// The node kinds the detectors depend on.
const WANTED: &[&str] = &[
    "function_declaration",
    "method_declaration",
    "func_literal",
    "parameter_list",
    "parameter_declaration",
    "variadic_parameter_declaration",
    "if_statement",
    "for_statement",
    "expression_switch_statement",
    "type_switch_statement",
    "select_statement",
    "expression_case",
    "default_case",
    "type_case",
    "communication_case",
    "type_declaration",
    "short_var_declaration",
    "selector_expression",
    "call_expression",
];

/// The fields shown for each of them; see `support::run` for the suffixes.
const FIELDS: &[&str] = &[
    "name",
    "type",
    "receiver",
    "parameters",
    "condition",
    "alternative:kind",
    "body?",
];

fn main() {
    support::run(tree_sitter_go::LANGUAGE.into(), SAMPLE, WANTED, FIELDS);
}
