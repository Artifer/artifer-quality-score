//! What makes a method one of a class's responsibilities, and what lets it off.
//!
//! Each case here is the same cohesive pair of methods plus a third, and the question is
//! only whether that third one counts.

mod support;

use aqs::models::FileReport;
use aqs::profile::{self as prof, Profile};
use support::{assert_cohesive, categories, detail, score, score_with};

/// Two methods that share `self.balance`, then `third`.
fn account(header: &str, third: &str) -> FileReport {
    let source = format!(
        "{header}\n    def deposit(self, n):\n        self.balance += n\n        return self.balance\n\n    \
         def withdraw(self, n):\n        self.balance -= n\n        return self.balance\n\n{third}"
    );
    score(&source, "account.py", "python")
}

fn assert_two_responsibilities(report: &FileReport) {
    let details = detail(report, prof::LOW_COHESION).expect("the third method stands alone");
    assert!(details.contains("LCOM4 = 2"), "{details}");
}

#[test]
fn a_method_that_reads_no_field_is_a_responsibility_of_its_own() {
    // One statement and no calls, but a module constant is not instance state.
    let report = account(
        "class Account:",
        "    def version(self):\n        return VERSION\n",
    );
    assert_two_responsibilities(&report);
}

#[test]
fn a_one_line_decision_is_not_an_accessor() {
    let report = account(
        "class Account:",
        "    def limit(self):\n        return self.ceiling if self.ceiling else 0\n",
    );
    assert_two_responsibilities(&report);
}

#[test]
fn a_super_delegation_is_let_off_even_when_it_computes_an_argument() {
    for third in [
        "    def update(self):\n        return super().update(stamp=now())\n",
        "    def save(self):\n        super().save(stamp=now())\n",
    ] {
        assert_cohesive(&account("class Account(Base):", third));
    }
}

#[test]
fn a_returned_super_method_call_is_a_delegation_too() {
    let source = "class Account extends Base {\n  \
                  deposit(n: number) { this.balance += n; return this.balance; }\n  \
                  withdraw(n: number) { this.balance -= n; return this.balance; }\n  \
                  update() { return super.update(stamp()); }\n}\n";
    assert_cohesive(&score(source, "account.ts", "typescript"));
}

/// A third method that touches a field of its own: a second responsibility.
const UNRELATED: &str =
    "    def rename(self, name):\n        self.name = name\n        return self.name\n";

#[test]
fn an_abstract_base_or_an_abstract_method_makes_the_class_an_interface() {
    assert_two_responsibilities(&account("class Account:", UNRELATED));

    assert_cohesive(&account("class Account(ABC):", UNRELATED));
    assert_cohesive(&account("class Account(typing.Protocol[T]):", UNRELATED));
    let decorated = format!("{UNRELATED}\n    @abstractmethod\n    def close(self):\n        self.open = False\n        return self.open\n");
    assert_cohesive(&account("class Account:", &decorated));
}

/// Two methods on the balance and one on the name, in each language's own spelling.
const DECLARED: [(&str, &str, &str); 3] = [
    (
        "typescript",
        "account.ts",
        "class Account {\n  \
         deposit(n: number) { this.balance += n; return this.balance; }\n  \
         withdraw(n: number) { this.balance -= n; return this.balance; }\n  \
         rename(name: string) { this.name = name; return this.name; }\n}\n",
    ),
    (
        "java",
        "Account.java",
        "public class Account {\n  int balance; String name;\n  \
         int deposit(int n) { this.balance += n; return this.balance; }\n  \
         int withdraw(int n) { this.balance -= n; return this.balance; }\n  \
         String rename(String name) { this.name = name; return this.name; }\n}\n",
    ),
    (
        "php",
        "Account.php",
        "<?php\nclass Account {\n  \
         function deposit($n) { $this->balance += $n; return $this->balance; }\n  \
         function withdraw($n) { $this->balance -= $n; return $this->balance; }\n  \
         function rename($name) { $this->name = $name; return $this->name; }\n}\n",
    ),
];

#[test]
fn a_class_declared_abstract_is_an_interface_in_every_language_that_has_the_word() {
    for (language, path, source) in DECLARED {
        assert_two_responsibilities(&score(source, path, language));
        let declared = source.replacen("class Account", "abstract class Account", 1);
        assert_cohesive(&score(&declared, path, language));
    }
}

#[test]
fn an_abstract_class_is_still_a_class_for_the_other_rules() {
    let mut profile = Profile::default();
    for key in [
        "class_max_lines_of_code",
        "class_max_functions",
        "brain_method_lines_of_code",
        "brain_method_cyclomatic_complexity",
        "brain_method_nesting_depth",
    ] {
        profile.thresholds.insert(key.to_string(), 0.0);
    }
    let source = "abstract class Engine {\n  start() { return 1; }\n  \
                  decide(x: number) {\n    if (x) {\n      return 1;\n    }\n    return 0;\n  }\n}\n";
    let report = score_with(&profile, source, "engine.ts", "typescript");
    assert!(categories(&report).contains(prof::BRAIN_CLASS));
}

#[test]
fn an_arrow_property_that_only_reads_a_field_is_an_accessor() {
    // The body is a bare expression in brackets: one thing, not a list of statements.
    let source =
        "class Account {\n  deposit(n: number) { this.balance += n; return this.balance; }\n  \
                  withdraw(n: number) { this.balance -= n; return this.balance; }\n  \
                  label = () => (this.name);\n}\n";
    assert_cohesive(&score(source, "account.ts", "typescript"));
}

#[test]
fn a_static_method_is_not_one_of_the_object_s_responsibilities() {
    let report = account(
        "class Account:",
        "    @staticmethod\n    def parse(text):\n        cleaned = text.strip()\n        return int(cleaned)\n",
    );
    assert_cohesive(&report);
}

#[test]
fn a_method_that_returns_a_string_is_not_a_stub() {
    let report = score(
        "class Labels:\n    def title(self):\n        return \"Title\"\n\n    def footer(self):\n        return \"Footer\"\n",
        "labels.py",
        "python",
    );
    assert_two_responsibilities(&report);
}
