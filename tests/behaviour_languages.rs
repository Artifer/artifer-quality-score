//! Behavioural tests per language added after the Python reference was retired: Rust,
//! Go, Ruby, Java and PHP.
//!
//! Each section shows that the shared detectors read that language's own constructs —
//! its closures, its error handling, its way of reaching a field — and that its test
//! and generated files are recognised for what they are.

mod support;

use aqs::models::FileReport;
use aqs::profile::{self as prof, Profile};
use support::{
    assert_cohesive, assert_only_inner_is_complex, categories, detail, py, score, scratch_tree,
};

// --------------------------------------------------------------------------- //
// Rust
// --------------------------------------------------------------------------- //

fn rs(source: &str) -> FileReport {
    score(source, "sample.rs", "rust")
}

#[test]
fn rust_complexity_counts_branches_match_arms_and_boolean_operators() {
    let arms: String = (0..12).map(|i| format!("        {i} => {i},\n")).collect();
    let report = rs(&format!(
        "pub fn classify(v: u8) -> u8 {{\n    match v {{\n{arms}        _ => 0,\n    }}\n}}\n"
    ));
    // 1 + 13 match arms = cc 14.
    assert_eq!(
        detail(&report, prof::COMPLEX_METHOD).as_deref(),
        Some("cc = 14")
    );
}

#[test]
fn rust_question_mark_is_not_a_branch() {
    // `?` is linear error propagation, not a decision the reader tracks. Counting it
    // would penalise idiomatic Rust for being explicit about failure.
    let calls: String = (0..20)
        .map(|i| format!("    let v{i} = step{i}()?;\n"))
        .collect();
    let report = rs(&format!(
        "pub fn run() -> Result<(), E> {{\n{calls}    Ok(())\n}}\n"
    ));
    assert!(
        !categories(&report).contains(prof::COMPLEX_METHOD),
        "{:?}",
        report.all_findings()
    );
}

#[test]
fn rust_self_is_not_an_argument() {
    // `&self` is its own node kind, not a parameter whose text reads "self".
    let report = rs(
        "impl S {\n    pub fn f(&self, a: u8, b: u8, c: u8, d: u8) -> u8 { a + b + c + d }\n}\n",
    );
    assert!(!categories(&report).contains(prof::EXCESS_ARGUMENTS));

    let too_many =
        rs("impl S {\n    pub fn f(&self, a: u8, b: u8, c: u8, d: u8, e: u8) -> u8 { a }\n}\n");
    assert_eq!(
        detail(&too_many, prof::EXCESS_ARGUMENTS).as_deref(),
        Some("Arguments = 5")
    );
}

#[test]
fn rust_else_if_chains_stay_flat() {
    let chain: String = (0..8)
        .map(|i| {
            format!(
                "{} x == {i} {{ return {i}; }}",
                if i == 0 { "if" } else { " else if" }
            )
        })
        .collect();
    let report = rs(&format!(
        "pub fn f(x: u8) -> u8 {{\n    {chain}\n    0\n}}\n"
    ));
    assert!(!categories(&report).contains(prof::DEEP_NESTED_COMPLEXITY));
}

#[test]
fn rust_nesting_is_measured() {
    let report = rs(
        "pub fn f(a: bool, xs: &[u8]) -> u8 {\n    if a {\n        for x in xs {\n            while *x > 0 {\n                if *x > 1 {\n                    return *x;\n                }\n            }\n        }\n    }\n    0\n}\n",
    );
    assert_eq!(
        detail(&report, prof::DEEP_NESTED_COMPLEXITY).as_deref(),
        Some("Nesting depth = 4 conditionals")
    );
}

#[test]
fn rust_assert_macros_are_assertions() {
    let wall: String = (0..12)
        .map(|i| format!("        assert_eq!(a{i}, {i});\n"))
        .collect();
    let report = score(
        &format!(
            "#[cfg(test)]\nmod tests {{\n    #[test]\n    fn it_works() {{\n{wall}    }}\n}}\n"
        ),
        "tests/thing.rs",
        "rust",
    );
    assert_eq!(
        detail(&report, prof::LARGE_ASSERTION_BLOCKS).as_deref(),
        Some("12 consecutive assertions")
    );
}

#[test]
fn rust_trait_impls_are_exempt_from_cohesion() {
    // The method set of `impl Trait for Type` is dictated by the trait, so its grouping
    // is not the author's choice — the same reasoning as ABCs and super()-only overrides.
    let report = rs(
        "impl Display for Money {\n    fn fmt(&self, f: &mut Formatter) -> Result { write!(f, \"{}\", self.amount) }\n    fn other(&self) -> u8 { 1 }\n}\n",
    );
    assert!(!categories(&report).contains(prof::LOW_COHESION));
}

#[test]
fn rust_inherent_impls_are_judged_for_cohesion() {
    let report = rs(
        "impl Mixed {\n    fn add_order(&mut self, o: u8) { self.orders.push(o) }\n    fn count_orders(&self) -> usize { self.orders.len() }\n    fn add_email(&mut self, e: u8) { self.emails.push(e) }\n    fn count_emails(&self) -> usize { self.emails.len() }\n}\n",
    );
    let details = detail(&report, prof::LOW_COHESION).expect("fires on an inherent impl");
    assert!(details.contains("LCOM4 = 2"), "{details}");
}

#[test]
fn rust_named_closures_are_their_own_unit() {
    let branches: String = (0..14)
        .map(|i| format!("        if x == {i} {{ return {i}; }}\n"))
        .collect();
    let report = rs(&format!(
        "pub fn outer(x: u8) -> u8 {{\n    let inner = |x: u8| -> u8 {{\n{branches}        0\n    }};\n    inner(x)\n}}\n"
    ));
    assert_only_inner_is_complex(&report);
}

#[test]
fn pure_accessors_are_exempt_but_mutators_are_not() {
    // A value object of accessors is not six responsibilities; splitting it would be
    // absurd. But a class that merely appends to two unrelated collections is real low
    // cohesion, and the "no calls" condition is what keeps those apart.
    let accessors = py("class Config:\n\
         \x20   def timeout(self):\n\
         \x20       return self._timeout\n\
         \x20   def retries(self):\n\
         \x20       return self._retries\n\
         \x20   def host(self):\n\
         \x20       return self._host\n");
    assert!(
        !categories(&accessors).contains(prof::LOW_COHESION),
        "accessors: {:?}",
        accessors
            .all_findings()
            .iter()
            .map(|f| &f.details)
            .collect::<Vec<_>>()
    );

    let mutators = py("class Mixed:\n\
         \x20   def add_order(self, o):\n\
         \x20       self.orders.append(o)\n\
         \x20   def count_orders(self):\n\
         \x20       return len(self.orders)\n\
         \x20   def add_email(self, e):\n\
         \x20       self.emails.append(e)\n\
         \x20   def count_emails(self):\n\
         \x20       return len(self.emails)\n");
    assert!(
        categories(&mutators).contains(prof::LOW_COHESION),
        "mutators touching two collections must still be condemned"
    );
}

#[test]
fn rust_impl_blocks_are_named_by_their_type() {
    // `impl Money` has no `name` field; its identity is the type it implements.
    let report = rs(
        "impl Mixed {\n    fn add_order(&mut self, o: u8) { self.orders.push(o) }\n    fn count_orders(&self) -> usize { self.orders.len() }\n    fn add_email(&mut self, e: u8) { self.emails.push(e) }\n    fn count_emails(&self) -> usize { self.emails.len() }\n}\n",
    );
    let finding = report
        .all_findings()
        .into_iter()
        .find(|f| f.category == prof::LOW_COHESION)
        .expect("fires");
    assert_eq!(finding.function.as_deref(), Some("Mixed"));
}

#[test]
fn a_typoed_config_section_is_rejected() {
    // An unknown rule name was already fatal, but an unknown *section* was silently
    // ignored, so `"exclude_pattern"` or `"rule"` quietly did nothing. For a gate, a
    // config that fails to apply is worse than one that refuses to load.
    let root = scratch_tree(
        "typo-section",
        &[(".aqs.json", r#"{"rule":{"Complex Method":0.0}}"#)],
    );
    let error = aqs::profile::load_profile(&root, None).expect_err("must be rejected");
    assert!(
        format!("{error}").contains("unknown config section"),
        "{error}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn underscore_prefixed_keys_are_comments() {
    // JSON has no comments, so `_why` carries the rationale for an exemption.
    let root = scratch_tree(
        "comment-key",
        &[(
            ".aqs.json",
            r#"{"_why":{"a":"because"},"name":"documented"}"#,
        )],
    );
    assert_eq!(
        aqs::profile::load_profile(&root, None).expect("loads").name,
        "documented"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn methods_that_say_nothing_about_cohesion_are_excluded() {
    // Each case is a class LCOM4 would condemn if the corresponding exemption were
    // missing, and each exemption exists for a different reason.
    let cases = [
        (
            // An accessor is not a responsibility of its own, but reading two fields is
            // real evidence they belong together. Excluding it from connectivity as well
            // as from counting would *raise* LCOM4 by removing the link.
            "accessor connects the fields it reads",
            "class Money:\n\
             \x20   def total(self):\n\
             \x20       return self.net + self.tax\n\
             \x20   def bump_net(self, v):\n\
             \x20       self.net = compute(v)\n\
             \x20   def bump_tax(self, v):\n\
             \x20       self.tax = compute(v)\n",
        ),
        (
            // `return super().update(deleted_at=timezone.now())` delegates even though it
            // computes an argument on the way. Demanding a call-free statement counted it
            // as its own responsibility and flagged a cohesive Django QuerySet.
            "super() delegation that computes an argument",
            "class SoftDeleteQuerySet(models.QuerySet):\n\
             \x20   def delete(self):\n\
             \x20       return super().update(deleted_at=timezone.now())\n\
             \x20   def hard_delete(self):\n\
             \x20       return super().delete()\n\
             \x20   def alive(self):\n\
             \x20       return self.filter(deleted_at__isnull=True)\n\
             \x20   def dead(self):\n\
             \x20       return self.filter(deleted_at__isnull=False)\n",
        ),
        (
            // A value object of getters is not six unrelated jobs.
            "value object of accessors",
            "class Config:\n\
             \x20   def timeout(self):\n\
             \x20       return self._timeout\n\
             \x20   def retries(self):\n\
             \x20       return self._retries\n\
             \x20   def host(self):\n\
             \x20       return self._host\n",
        ),
    ];
    for (label, source) in cases {
        let report = py(source);
        assert!(
            !categories(&report).contains(prof::LOW_COHESION),
            "{label}: {:?}",
            report
                .all_findings()
                .iter()
                .map(|f| &f.details)
                .collect::<Vec<_>>()
        );
    }
}

// --------------------------------------------------------------------------- //
// Go
// --------------------------------------------------------------------------- //

fn go(source: &str) -> FileReport {
    score(source, "sample.go", "go")
}

#[test]
fn go_complexity_counts_branches_switch_cases_and_boolean_operators() {
    let cases: String = (0..12)
        .map(|i| format!("\tcase {i}:\n\t\treturn {i}\n"))
        .collect();
    let report = go(&format!(
        "package p\n\nfunc classify(v int) int {{\n\tswitch v {{\n{cases}\tdefault:\n\t}}\n\treturn 0\n}}\n"
    ));
    // 1 + 12 cases = cc 13. `default` is the absence of a decision, not one more.
    assert_eq!(
        detail(&report, prof::COMPLEX_METHOD).as_deref(),
        Some("cc = 13")
    );
}

#[test]
fn go_the_receiver_is_not_an_argument() {
    // A method's receiver sits outside the parameter list, in a `parameter_list` of its
    // own that comes first — so taking the first one would measure the receiver.
    let report =
        go("package p\n\nfunc (s *Server) f(a int, b int, c int, d int) int { return a }\n");
    assert!(!categories(&report).contains(prof::EXCESS_ARGUMENTS));

    let too_many =
        go("package p\n\nfunc (s *Server) f(a int, b int, c int, d int, e int) int { return a }\n");
    assert_eq!(
        detail(&too_many, prof::EXCESS_ARGUMENTS).as_deref(),
        Some("Arguments = 5")
    );
}

#[test]
fn go_parameters_sharing_a_type_are_counted_one_each() {
    // `func f(a, b, c int)` is a single parameter_declaration naming three parameters,
    // and the caller still has to pass three arguments.
    let report = go("package p\n\nfunc f(a, b, c, d, e int) int { return a }\n");
    assert_eq!(
        detail(&report, prof::EXCESS_ARGUMENTS).as_deref(),
        Some("Arguments = 5")
    );
}

#[test]
fn go_results_are_not_arguments() {
    // A named result list is a third `parameter_list` on the declaration.
    let report = go(
        "package p\n\nfunc f(a int, b int) (one int, two int, three int, four error) { return }\n",
    );
    assert!(!categories(&report).contains(prof::EXCESS_ARGUMENTS));
}

#[test]
fn go_else_if_chains_stay_flat() {
    // Go hangs the second `if` off the first one's `alternative` field with no clause
    // node in between, so kind alone cannot tell it from a nested `if`.
    let chain: String = (0..8)
        .map(|i| {
            format!(
                "{} x == {i} {{\n\t\treturn {i}\n\t}}",
                if i == 0 { "\tif" } else { " else if" }
            )
        })
        .collect();
    let report = go(&format!(
        "package p\n\nfunc f(x int) int {{\n{chain}\n\treturn 0\n}}\n"
    ));
    assert!(!categories(&report).contains(prof::DEEP_NESTED_COMPLEXITY));
}

#[test]
fn go_nesting_is_measured() {
    let report = go(
        "package p\n\nfunc f(a bool, xs []int) int {\n\tif a {\n\t\tfor _, x := range xs {\n\t\t\tswitch x {\n\t\t\tcase 1:\n\t\t\t\tif x > 1 {\n\t\t\t\t\treturn x\n\t\t\t\t}\n\t\t\t}\n\t\t}\n\t}\n\treturn 0\n}\n",
    );
    assert_eq!(
        detail(&report, prof::DEEP_NESTED_COMPLEXITY).as_deref(),
        Some("Nesting depth = 4 conditionals")
    );
}

#[test]
fn go_new_prefixed_functions_are_constructors() {
    // Go has no constructor syntax; `NewThing(..)` is the whole convention.
    let report = go("package p\n\nfunc NewServer(a, b, c, d, e, f int) *Server { return nil }\n");
    assert_eq!(
        detail(&report, prof::CONSTRUCTOR_OVER_INJECTION).as_deref(),
        Some("Arguments = 6")
    );

    // The capital after the prefix is what keeps ordinary functions out of it.
    let ordinary = go("package p\n\nfunc Newlines(a, b, c, d, e, f int) int { return a }\n");
    assert!(!categories(&ordinary).contains(prof::CONSTRUCTOR_OVER_INJECTION));
    assert!(categories(&ordinary).contains(prof::EXCESS_ARGUMENTS));
}

#[test]
fn go_named_closures_are_their_own_unit() {
    // `helper := func() {..}` binds through an `expression_list`, so the name is a
    // grandparent away rather than a parent away.
    let branches: String = (0..14)
        .map(|i| format!("\t\tif x == {i} {{\n\t\t\treturn {i}\n\t\t}}\n"))
        .collect();
    let report = go(&format!(
        "package p\n\nfunc outer(x int) int {{\n\tinner := func(x int) int {{\n{branches}\t\treturn 0\n\t}}\n\treturn inner(x)\n}}\n"
    ));
    assert_only_inner_is_complex(&report);
}

#[test]
fn go_testify_and_t_error_are_assertions() {
    let wall: String = (0..12)
        .map(|i| format!("\trequire.Equal(t, a{i}, {i})\n"))
        .collect();
    let report = score(
        &format!("package p\n\nfunc TestThing(t *testing.T) {{\n{wall}}}\n"),
        "thing_test.go",
        "go",
    );
    assert_eq!(
        detail(&report, prof::LARGE_ASSERTION_BLOCKS).as_deref(),
        Some("12 consecutive assertions")
    );
}

#[test]
fn go_type_declarations_count_as_declarations() {
    // A file that is mostly `type` blocks is describing shape, not behaviour, and the
    // size rules must not read it as a wall of logic.
    let types: String = (0..40)
        .map(|i| format!("type T{i} struct {{\n\ta int\n\tb string\n}}\n\n"))
        .collect();
    let report = go(&format!("package p\n\n{types}"));
    assert!(
        !categories(&report).contains(prof::FILE_SIZE),
        "{:?}",
        report.all_findings()
    );
}

#[test]
fn go_test_files_are_recognised_by_their_required_suffix() {
    // `go test` compiles nothing else, so this is the one test-path convention a
    // project cannot deviate from.
    let profile = Profile::default();
    assert!(profile
        .test_patterns
        .matches("internal/server/server_test.go"));
    assert!(profile.test_patterns.matches("server_test.go"));
    assert!(!profile.test_patterns.matches("internal/server/server.go"));
}

#[test]
fn go_vendored_dependencies_are_excluded() {
    let profile = Profile::default();
    for path in [
        "vendor/github.com/pkg/errors/errors.go",
        "api/v1/service.pb.go",
    ] {
        assert!(
            profile.exclude_patterns.matches(path),
            "{path} is not ours to score"
        );
    }
    assert!(!profile
        .exclude_patterns
        .matches("internal/vendors/stripe.go"));
}

#[test]
fn go_bumpy_roads_are_seen_through_the_statement_list_wrapper() {
    // Go's `block` holds a single `statement_list`, so reading the body's children
    // naively finds one statement — and every Go function would look smooth.
    let bump = "\tfor _, x := range xs {\n\t\tif x > 0 {\n\t\t\ttotal += x\n\t\t}\n\t}\n";
    let report = go(&format!(
        "package p\n\nfunc f(xs []int) int {{\n\ttotal := 0\n{bump}{bump}{bump}\treturn total\n}}\n"
    ));
    assert_eq!(
        detail(&report, prof::BUMPY_ROAD).as_deref(),
        Some("bumps = 3")
    );
}

#[test]
fn go_duplicated_blocks_are_found_inside_a_function() {
    let block = |i: usize| {
        format!(
            "\tif value{i} == nil {{\n\t\tlogger.Error(\"missing\", \"field\", \"value{i}\")\n\t\tmetrics.Increment(\"missing.value{i}\")\n\t\treturn errors.New(\"value{i} is required\")\n\t}}\n"
        )
    };
    let report = go(&format!(
        "package p\n\nfunc validate() error {{\n{}{}{}\treturn nil\n}}\n",
        block(1),
        block(2),
        block(3)
    ));
    assert!(
        categories(&report).contains(prof::DUPLICATED_BLOCK),
        "{:?}",
        report
            .all_findings()
            .iter()
            .map(|f| &f.details)
            .collect::<Vec<_>>()
    );
}

#[test]
fn go_a_nil_check_on_something_other_than_an_error_is_a_branch() {
    // Only an `err`-named value earns the exemption; `if conn != nil { return .. }` is an
    // ordinary decision.
    let checks: String = (0..12)
        .map(|i| format!("\tif conn{i} != nil {{\n\t\treturn conn{i}\n\t}}\n"))
        .collect();
    let report = go(&format!(
        "package p\n\nfunc pick() *Conn {{\n{checks}\treturn nil\n}}\n"
    ));
    assert_eq!(
        detail(&report, prof::COMPLEX_METHOD).as_deref(),
        Some("cc = 13")
    );
}

#[test]
fn go_error_guards_are_not_branches() {
    // `if err != nil { return .. }` is Rust's `?` with the sugar removed, and `?` is
    // already not counted. Counting it would judge a Go function for how many things it
    // calls that can fail.
    let calls: String = (0..20)
        .map(|i| {
            format!(
                "\tv{i}, err := step{i}()\n\tif err != nil {{\n\t\treturn err\n\t}}\n\t_ = v{i}\n"
            )
        })
        .collect();
    let report = go(&format!(
        "package p\n\nfunc run() error {{\n{calls}\treturn nil\n}}\n"
    ));
    assert!(
        !categories(&report).contains(prof::COMPLEX_METHOD),
        "{:?}",
        report
            .all_findings()
            .iter()
            .map(|f| &f.details)
            .collect::<Vec<_>>()
    );

    // The same shape with the call in the `if` initialiser, which is the more common
    // spelling when the value is not needed.
    let inline: String = (0..20)
        .map(|i| format!("\tif err := step{i}(); err != nil {{\n\t\treturn err\n\t}}\n"))
        .collect();
    let report = go(&format!(
        "package p\n\nfunc run() error {{\n{inline}\treturn nil\n}}\n"
    ));
    assert!(
        !categories(&report).contains(prof::COMPLEX_METHOD),
        "{:?}",
        report
            .all_findings()
            .iter()
            .map(|f| &f.details)
            .collect::<Vec<_>>()
    );
}

#[test]
fn go_error_handling_that_carries_on_is_still_a_branch() {
    // Recovering from the error is a decision the reader has to hold in mind. Only the
    // straight-back-out shape is exempt.
    let calls: String = (0..12)
        .map(|i| format!("\tv{i}, err := step{i}()\n\tif err != nil {{\n\t\tv{i} = fallback({i})\n\t}}\n\t_ = v{i}\n"))
        .collect();
    let report = go(&format!(
        "package p\n\nfunc run() error {{\n{calls}\treturn nil\n}}\n"
    ));
    assert_eq!(
        detail(&report, prof::COMPLEX_METHOD).as_deref(),
        Some("cc = 13")
    );

    // Nor does an `else` branch stop being a branch.
    let with_else: String = (0..12)
        .map(|i| format!("\tv{i}, err := step{i}()\n\tif err != nil {{\n\t\treturn err\n\t}} else {{\n\t\t_ = v{i}\n\t}}\n"))
        .collect();
    let report = go(&format!(
        "package p\n\nfunc run() error {{\n{with_else}\treturn nil\n}}\n"
    ));
    assert_eq!(
        detail(&report, prof::COMPLEX_METHOD).as_deref(),
        Some("cc = 13")
    );
}

// --------------------------------------------------------------------------- //
// Ruby
// --------------------------------------------------------------------------- //

fn rb(source: &str) -> FileReport {
    score(source, "sample.rb", "ruby")
}

#[test]
fn ruby_keyword_tokens_are_not_counted_as_branches() {
    // tree-sitter-ruby gives the `if` keyword the same node kind as the `if` node it
    // opens. Matching on kind alone counts every Ruby branch twice.
    let branches: String = (0..8)
        .map(|i| format!("    return {i} if x == {i}\n"))
        .collect();
    let report = rb(&format!("def f(x)\n{branches}    0\nend\n"));
    // 1 + 8 guard clauses = cc 9, one under the threshold.
    assert!(
        !categories(&report).contains(prof::COMPLEX_METHOD),
        "{:?}",
        report
            .all_findings()
            .iter()
            .map(|f| &f.details)
            .collect::<Vec<_>>()
    );

    let one_more: String = (0..11)
        .map(|i| format!("    return {i} if x == {i}\n"))
        .collect();
    let report = rb(&format!("def f(x)\n{one_more}    0\nend\n"));
    assert_eq!(
        detail(&report, prof::COMPLEX_METHOD).as_deref(),
        Some("cc = 12")
    );
}

#[test]
fn ruby_complexity_counts_branches_when_clauses_and_boolean_operators() {
    let whens: String = (0..12).map(|i| format!("  when {i} then {i}\n")).collect();
    let report = rb(&format!(
        "def classify(v)\n  case v\n{whens}  else\n    0\n  end\nend\n"
    ));
    // 1 + 12 `when` clauses = cc 13. `else` is the absence of a decision.
    assert_eq!(
        detail(&report, prof::COMPLEX_METHOD).as_deref(),
        Some("cc = 13")
    );
}

#[test]
fn ruby_and_or_count_but_arithmetic_does_not() {
    // Ruby models every infix operator as one `binary` node, so the operator text is
    // the only thing separating a boolean from a sum.
    let sums = rb("def f(a, b, c)\n  a + b + c + a + b + c\nend\n");
    assert!(!categories(&sums).contains(prof::COMPLEX_CONDITIONAL));

    let report = rb("def f(a, b, c)\n  if a && b || c and a\n    1\n  end\nend\n");
    assert_eq!(
        detail(&report, prof::COMPLEX_CONDITIONAL).as_deref(),
        Some("1 complex conditional expression")
    );
}

#[test]
fn ruby_elsif_chains_stay_flat_and_modifiers_do_not_nest() {
    let chain: String = (0..8)
        .map(|i| {
            format!(
                "{} x == {i}\n    {i}\n",
                if i == 0 { "  if" } else { "  elsif" }
            )
        })
        .collect();
    let report = rb(&format!("def f(x)\n{chain}  end\nend\n"));
    assert!(!categories(&report).contains(prof::DEEP_NESTED_COMPLEXITY));

    // `return unless valid?` is a guard clause; the whole point of writing it that way
    // is that it does not indent the method.
    let guards: String = (0..6)
        .map(|i| format!("  return {i} unless x == {i}\n"))
        .collect();
    let report = rb(&format!("def f(x)\n{guards}  0\nend\n"));
    assert!(!categories(&report).contains(prof::DEEP_NESTED_COMPLEXITY));
}

#[test]
fn ruby_nesting_is_measured() {
    let report = rb(
        "def f(a, xs)\n  if a\n    xs.each do |x|\n      while x > 0\n        case x\n        when 1\n          if x > 1\n            return x\n          end\n        end\n      end\n    end\n  end\n  0\nend\n",
    );
    // if, while, case, if = 4. The `each do |x|` block is a call, not a branch, and the
    // `while` keyword token is not a second `while`.
    assert_eq!(
        detail(&report, prof::DEEP_NESTED_COMPLEXITY).as_deref(),
        Some("Nesting depth = 4 conditionals")
    );
}

#[test]
fn ruby_instance_variables_are_instance_state() {
    // `@orders` is one node with no receiver, where every other language here writes a
    // receiver plus an attribute. Without it no Ruby class shares any state at all.
    let report = rb(
        "class Mixed\n  def add_order(o)\n    @orders.push(o)\n  end\n  def count_orders\n    @orders.length\n  end\n  def add_email(e)\n    @emails.push(e)\n  end\n  def count_emails\n    @emails.length\n  end\nend\n",
    );
    let details = detail(&report, prof::LOW_COHESION).expect("fires on two unrelated groups");
    assert!(details.contains("LCOM4 = 2"), "{details}");

    let cohesive = rb(
        "class Orders\n  def add_order(o)\n    @orders.push(o)\n  end\n  def count_orders\n    @orders.length\n  end\n  def clear_orders\n    @orders.clear\n  end\nend\n",
    );
    assert!(!categories(&cohesive).contains(prof::LOW_COHESION));
}

#[test]
fn ruby_attr_reader_state_counts_as_shared() {
    // `attr_reader :options` then a bare `options` is the same field as `@options`.
    // Without that, a class that goes through its own accessors shares nothing.
    let report = rb(
        "class Inject\n  attr_reader :options, :name\n  def initialize(options, name)\n    @options = options\n    @name = name\n  end\n  def run\n    puts options\n    puts name\n  end\n  def describe\n    \"#{name}: #{options}\"\n  end\nend\n",
    );
    assert!(
        !categories(&report).contains(prof::LOW_COHESION),
        "{:?}",
        report
            .all_findings()
            .iter()
            .map(|f| &f.details)
            .collect::<Vec<_>>()
    );
}

#[test]
fn ruby_bare_calls_connect_methods() {
    // Ruby calls a sibling method by bare name, so nothing marks it as a self-call.
    let report = rb(
        "class Report\n  def initialize\n    @orders = []\n  end\n  def add(o)\n    @orders.push(o)\n  end\n  def summary\n    format_line(count)\n  end\n  def count\n    @orders.length\n  end\n  def format_line(n)\n    \"n=#{n}\"\n  end\nend\n",
    );
    assert!(
        !categories(&report).contains(prof::LOW_COHESION),
        "{:?}",
        report
            .all_findings()
            .iter()
            .map(|f| &f.details)
            .collect::<Vec<_>>()
    );
}

#[test]
fn ruby_namespace_functions_are_not_responsibilities() {
    // `def self.parse` holds no instance state — the same reasoning that exempts a
    // Python @staticmethod and a Rust associated function.
    let functions: String = (0..6)
        .map(|i| format!("  def self.step{i}(a)\n    a + {i}\n  end\n"))
        .collect();
    let report = rb(&format!("module Util\n{functions}end\n"));
    assert!(!categories(&report).contains(prof::LOW_COHESION));
}

#[test]
fn ruby_namespace_modules_are_not_credited_with_what_they_wrap() {
    // `module Billing` wrapping a class writes none of its methods, and judging the
    // wrapper as well would report every finding twice.
    let report = rb(
        "module Billing\n  class Mixed\n    def add_order(o)\n      @orders.push(o)\n    end\n    def count_orders\n      @orders.length\n    end\n    def add_email(e)\n      @emails.push(e)\n    end\n    def count_emails\n      @emails.length\n    end\n  end\nend\n",
    );
    let flagged: Vec<String> = report
        .all_findings()
        .iter()
        .filter(|f| f.category == prof::LOW_COHESION)
        .map(|f| f.function.clone().unwrap_or_default())
        .collect();
    assert_eq!(flagged, vec!["Mixed".to_string()]);
}

#[test]
fn ruby_null_objects_are_interfaces() {
    // Every method empty is an interface, whatever the constructor stores. Telling the
    // author it has nine unrelated responsibilities tells them to stop writing null
    // objects.
    let stubs: String = (0..9)
        .map(|i| format!("  def print_{i}(*) end\n"))
        .collect();
    let report = rb(&format!(
        "class Quiet\n  def initialize(n)\n    @n = n\n  end\n{stubs}end\n"
    ));
    assert!(!categories(&report).contains(prof::LOW_COHESION));
}

#[test]
fn ruby_named_lambdas_are_their_own_unit() {
    let branches: String = (0..14)
        .map(|i| format!("    return {i} if x == {i}\n"))
        .collect();
    let report = rb(&format!(
        "def outer(x)\n  inner = ->(x) do\n{branches}    0\n  end\n  inner.call(x)\nend\n"
    ));
    assert_only_inner_is_complex(&report);
}

#[test]
fn ruby_rspec_expectations_are_recognised_as_assertions() {
    // Ruby has no `expression_statement`: the call *is* the statement.
    let wall: String = (0..12)
        .map(|i| format!("    expect(subject.a{i}).to eq({i})\n"))
        .collect();
    let report = score(
        &format!("RSpec.describe Invoice do\n  it \"adds up\" do\n{wall}  end\nend\n"),
        "spec/invoice_spec.rb",
        "ruby",
    );
    assert_eq!(
        detail(&report, prof::LARGE_ASSERTION_BLOCKS).as_deref(),
        Some("12 consecutive assertions")
    );
}

#[test]
fn ruby_minitest_assertions_are_recognised() {
    let minitest: String = (0..12)
        .map(|i| format!("    assert_equal {i}, thing.a{i}\n"))
        .collect();
    let report = score(
        &format!("class InvoiceTest < Minitest::Test\n  def test_totals\n{minitest}  end\nend\n"),
        "test/invoice_test.rb",
        "ruby",
    );
    assert_eq!(
        detail(&report, prof::LARGE_ASSERTION_BLOCKS).as_deref(),
        Some("12 consecutive assertions")
    );
}

#[test]
fn ruby_bare_super_is_a_delegation_and_an_empty_class_is_nothing() {
    let overrides: String = (0..6)
        .map(|i| format!("  def step{i}\n    super\n  end\n"))
        .collect();
    let report = rb(&format!(
        "class Marker\nend\n\nclass Audited < Base\n{overrides}end\n"
    ));
    assert_cohesive(&report);
}

#[test]
fn ruby_initialize_is_the_constructor() {
    let report = rb("class Thing\n  def initialize(a, b, c, d, e, f)\n    @a = a\n  end\nend\n");
    assert_eq!(
        detail(&report, prof::CONSTRUCTOR_OVER_INJECTION).as_deref(),
        Some("Arguments = 6")
    );
}

#[test]
fn ruby_spec_and_test_directories_are_test_code() {
    let profile = Profile::default();
    for path in [
        "spec/models/invoice_spec.rb",
        "test/invoice_test.rb",
        "app/spec/a_spec.rb",
    ] {
        assert!(profile.test_patterns.matches(path), "{path} is test code");
    }
    assert!(!profile.test_patterns.matches("app/models/invoice.rb"));
    assert!(profile.exclude_patterns.matches("db/schema.rb"));
    assert!(profile
        .exclude_patterns
        .matches("db/migrate/20240101_add_x.rb"));
}

// --------------------------------------------------------------------------- //
// Java
// --------------------------------------------------------------------------- //

/// Fields read through getters hold a class together; unrelated groups of them do not.
///
/// `value_object` is six getters over six fields, `mixed` is two collections each touched
/// by its own pair of methods. Every class-based language has to tell the two apart, and
/// what differs between them is only how a method reaches a field.
fn assert_cohesion_follows_field_use(value_object: &FileReport, mixed: &FileReport) {
    assert!(
        !categories(value_object).contains(prof::LOW_COHESION),
        "a value object of six getters is not six responsibilities: {:?}",
        value_object
            .all_findings()
            .iter()
            .map(|f| &f.details)
            .collect::<Vec<_>>()
    );
    let details = detail(mixed, prof::LOW_COHESION).expect("fires on two unrelated groups");
    assert!(details.contains("LCOM4 = 2"), "{details}");
}

fn java(source: &str) -> FileReport {
    score(source, "Sample.java", "java")
}

/// Wrap members in a class, which is the only place Java lets you write them.
fn in_class(members: &str) -> String {
    format!("class Sample {{\n{members}}}\n")
}

#[test]
fn java_constructors_are_recognised_by_node_kind() {
    // A Java constructor is named after its class, so there is no fixed name to look
    // for — only a node kind.
    let report = java(&in_class(
        "    Sample(int a, int b, int c, int d, int e, int f) { this.a = a; }\n",
    ));
    assert_eq!(
        detail(&report, prof::CONSTRUCTOR_OVER_INJECTION).as_deref(),
        Some("Arguments = 6")
    );
    assert!(!categories(&report).contains(prof::EXCESS_ARGUMENTS));
}

#[test]
fn java_switch_default_is_not_a_decision() {
    // Java writes `case` and `default` with the same node, so the exclusion TypeScript
    // gets from its grammar has to be made by hand.
    let cases: String = (0..12)
        .map(|i| format!("        case {i}: return {i};\n"))
        .collect();
    let report = java(&in_class(&format!(
        "    int classify(int v) {{\n        switch (v) {{\n{cases}        default: return 0;\n        }}\n    }}\n"
    )));
    assert_eq!(
        detail(&report, prof::COMPLEX_METHOD).as_deref(),
        Some("cc = 13")
    );
}

#[test]
fn java_arrow_switches_count_the_same_as_classic_ones() {
    let cases: String = (0..12)
        .map(|i| format!("            case {i} -> {i};\n"))
        .collect();
    let report = java(&in_class(&format!(
        "    int classify(int v) {{\n        return switch (v) {{\n{cases}            default -> 0;\n        }};\n    }}\n"
    )));
    assert_eq!(
        detail(&report, prof::COMPLEX_METHOD).as_deref(),
        Some("cc = 13")
    );
}

#[test]
fn java_else_if_chains_stay_flat() {
    let chain: String = (0..8)
        .map(|i| {
            format!(
                "{} (x == {i}) {{ return {i}; }}",
                if i == 0 { "        if" } else { " else if" }
            )
        })
        .collect();
    let report = java(&in_class(&format!(
        "    int f(int x) {{\n{chain}\n        return 0;\n    }}\n"
    )));
    assert!(!categories(&report).contains(prof::DEEP_NESTED_COMPLEXITY));
}

#[test]
fn java_nesting_counts_the_enhanced_for() {
    let report = java(&in_class(
        "    int f(boolean a, int[] xs) {\n        if (a) {\n            for (int x : xs) {\n                while (x > 0) {\n                    if (x > 1) { return x; }\n                }\n            }\n        }\n        return 0;\n    }\n",
    ));
    assert_eq!(
        detail(&report, prof::DEEP_NESTED_COMPLEXITY).as_deref(),
        Some("Nesting depth = 4 conditionals")
    );
}

#[test]
fn java_annotated_types_make_the_type_rules_work() {
    // Java is the first language here that annotates everything, so Primitive Obsession
    // is measuring what it was written for rather than staying quiet.
    let report = java(&in_class(
        "    void configure(String host, int port, boolean tls, long timeout) {}\n",
    ));
    assert_eq!(
        detail(&report, prof::PRIMITIVE_OBSESSION).as_deref(),
        Some("4 of 4 arguments are primitives (100%)")
    );

    let domain = java(&in_class(
        "    void configure(Host host, Port port, TlsMode tls, Timeout timeout) {}\n",
    ));
    assert!(!categories(&domain).contains(prof::PRIMITIVE_OBSESSION));
}

#[test]
fn java_bare_field_reads_are_field_reads() {
    // Java reaches a field by bare name as readily as through `this.`, and the class's
    // own field list is the only thing that separates that from a local.
    let getters: String = (0..6)
        .map(|i| format!("    long get{i}() {{ return field{i}; }}\n"))
        .collect();
    let fields: String = (0..6)
        .map(|i| format!("    private long field{i};\n"))
        .collect();
    let value_object = java(&in_class(&format!("{fields}{getters}")));
    let mixed = java(&in_class(
        "    private List<String> orders;\n    private List<String> emails;\n    void addOrder(String o) { orders.add(o); }\n    int countOrders() { return orders.size() + 1; }\n    void addEmail(String e) { emails.add(e); }\n    int countEmails() { return emails.size() + 1; }\n",
    ));
    assert_cohesion_follows_field_use(&value_object, &mixed);
}

#[test]
fn java_static_helpers_are_a_namespace_not_an_object() {
    // A class of static helpers holds no instance state, so there is nothing for
    // cohesion to judge — the same reasoning that exempts Ruby's `def self.parse`.
    let helpers: String = (0..6)
        .map(|i| format!("    static String step{i}(String a) {{ return a + {i}; }}\n"))
        .collect();
    let report = java(&in_class(&helpers));
    assert!(!categories(&report).contains(prof::LOW_COHESION));
}

#[test]
fn java_interfaces_are_exempt_from_cohesion() {
    // Method signatures with no body are an interface by definition — no shared state
    // and no internal calls, so cohesion always condemns them.
    let methods: String = (0..6)
        .map(|i| format!("    String name{i}(int a);\n"))
        .collect();
    let report = score(
        &format!("interface Shape {{\n{methods}}}\n"),
        "Shape.java",
        "java",
    );
    assert!(!categories(&report).contains(prof::LOW_COHESION));
}

#[test]
fn java_junit_assertions_are_recognised() {
    let wall: String = (0..12)
        .map(|i| format!("        assertEquals({i}, thing.a{i}());\n"))
        .collect();
    let report = score(
        &format!("class ThingTest {{\n    @Test\n    void totals() {{\n{wall}    }}\n}}\n"),
        "src/test/java/ThingTest.java",
        "java",
    );
    assert_eq!(
        detail(&report, prof::LARGE_ASSERTION_BLOCKS).as_deref(),
        Some("12 consecutive assertions")
    );
}

#[test]
fn java_records_and_enums_are_declarations() {
    // A file of records is describing shape, not behaviour.
    let records: String = (0..60)
        .map(|i| format!("record Money{i}(long amount, String currency) {{}}\n"))
        .collect();
    let report = score(&records, "Money.java", "java");
    assert!(
        !categories(&report).contains(prof::FILE_SIZE),
        "{:?}",
        report
            .all_findings()
            .iter()
            .map(|f| &f.details)
            .collect::<Vec<_>>()
    );
}

#[test]
fn java_test_and_build_paths_are_recognised() {
    let profile = Profile::default();
    for path in [
        "src/test/java/billing/InvoiceTest.java",
        "InvoiceTests.java",
        "SmokeIT.java",
    ] {
        assert!(profile.test_patterns.matches(path), "{path} is test code");
    }
    assert!(!profile
        .test_patterns
        .matches("src/main/java/billing/Invoice.java"));
    assert!(profile
        .exclude_patterns
        .matches("target/generated-sources/Api.java"));
}

// --------------------------------------------------------------------------- //
// PHP
// --------------------------------------------------------------------------- //

fn php(source: &str) -> FileReport {
    score(&format!("<?php\n{source}"), "Sample.php", "php")
}

/// Wrap members in a class, which is the only place PHP lets you write a method.
fn in_php_class(members: &str) -> String {
    format!("class Sample\n{{\n{members}}}\n")
}

#[test]
fn php_complexity_counts_branches_cases_and_boolean_operators() {
    let cases: String = (0..12)
        .map(|i| format!("        case {i}: return {i};\n"))
        .collect();
    let report = php(&in_php_class(&format!(
        "    function classify($v) {{\n        switch ($v) {{\n{cases}        default: return 0;\n        }}\n    }}\n"
    )));
    // 1 + 12 cases = cc 13. `default` is the absence of a decision.
    assert_eq!(
        detail(&report, prof::COMPLEX_METHOD).as_deref(),
        Some("cc = 13")
    );
}

#[test]
fn php_match_arms_count_but_the_default_does_not() {
    let arms: String = (0..12)
        .map(|i| format!("            {i} => \"a{i}\",\n"))
        .collect();
    let report = php(&in_php_class(&format!(
        "    function classify($v) {{\n        return match ($v) {{\n{arms}            default => \"z\",\n        }};\n    }}\n"
    )));
    assert_eq!(
        detail(&report, prof::COMPLEX_METHOD).as_deref(),
        Some("cc = 13")
    );
}

#[test]
fn php_low_precedence_and_or_count_but_arithmetic_does_not() {
    let sums = php(&in_php_class(
        "    function f($a, $b, $c) { return $a + $b + $c + $a; }\n",
    ));
    assert!(!categories(&sums).contains(prof::COMPLEX_CONDITIONAL));

    let report = php(&in_php_class(
        "    function f($a, $b, $c) {\n        if ($a && $b || $c and $a) { return 1; }\n    }\n",
    ));
    assert_eq!(
        detail(&report, prof::COMPLEX_CONDITIONAL).as_deref(),
        Some("1 complex conditional expression")
    );
}

#[test]
fn php_elseif_chains_stay_flat() {
    let chain: String = (0..8)
        .map(|i| {
            format!(
                "{} ($x == {i}) {{ return {i}; }}",
                if i == 0 { "        if" } else { " elseif" }
            )
        })
        .collect();
    let report = php(&in_php_class(&format!(
        "    function f($x) {{\n{chain}\n        return 0;\n    }}\n"
    )));
    assert!(!categories(&report).contains(prof::DEEP_NESTED_COMPLEXITY));
}

#[test]
fn php_nesting_counts_the_foreach() {
    let report = php(&in_php_class(
        "    function f($a, $xs) {\n        if ($a) {\n            foreach ($xs as $x) {\n                while ($x > 0) {\n                    if ($x > 1) { return $x; }\n                }\n            }\n        }\n        return 0;\n    }\n",
    ));
    assert_eq!(
        detail(&report, prof::DEEP_NESTED_COMPLEXITY).as_deref(),
        Some("Nesting depth = 4 conditionals")
    );
}

#[test]
fn php_typed_parameters_reach_the_type_rules() {
    let report = php(&in_php_class(
        "    function configure(string $host, int $port, bool $tls, float $timeout) {}\n",
    ));
    assert_eq!(
        detail(&report, prof::PRIMITIVE_OBSESSION).as_deref(),
        Some("4 of 4 arguments are primitives (100%)")
    );

    // Untyped is absence of evidence, not evidence of bad design.
    let untyped = php(&in_php_class(
        "    function configure($host, $port, $tls, $timeout) {}\n",
    ));
    assert!(!categories(&untyped).contains(prof::PRIMITIVE_OBSESSION));
}

#[test]
fn php_construct_is_the_constructor() {
    let report = php(&in_php_class(
        "    public function __construct(int $a, int $b, int $c, int $d, int $e, int $f) {}\n",
    ));
    assert_eq!(
        detail(&report, prof::CONSTRUCTOR_OVER_INJECTION).as_deref(),
        Some("Arguments = 6")
    );
}

#[test]
fn php_this_properties_are_instance_state() {
    let getters: String = (0..6)
        .map(|i| format!("    function get{i}() {{ return $this->field{i}; }}\n"))
        .collect();
    let fields: String = (0..6)
        .map(|i| format!("    private int $field{i};\n"))
        .collect();
    let value_object = php(&in_php_class(&format!("{fields}{getters}")));
    let mixed = php(&in_php_class(
        "    private array $orders;\n    private array $emails;\n    function addOrder($o) { $this->orders[] = $o; }\n    function countOrders() { return count($this->orders) + 1; }\n    function addEmail($e) { $this->emails[] = $e; }\n    function countEmails() { return count($this->emails) + 1; }\n",
    ));
    assert_cohesion_follows_field_use(&value_object, &mixed);
}

#[test]
fn php_promoted_constructor_properties_are_fields() {
    // `__construct(private Repo $repo)` declares a field and assigns it in one breath.
    let report = php(&in_php_class(
        "    public function __construct(private array $orders, private array $log) {}\n    function add($o) { $this->orders[] = $o; }\n    function count() { return count($this->orders); }\n    function note($m) { $this->log[] = $m; }\n    function history() { return count($this->log); }\n",
    ));
    let details = detail(&report, prof::LOW_COHESION).expect("two unrelated groups");
    assert!(details.contains("LCOM4 = 2"), "{details}");
}

#[test]
fn php_static_helpers_are_a_namespace_not_an_object() {
    let helpers: String = (0..6)
        .map(|i| format!("    public static function step{i}($a) {{ return $a + {i}; }}\n"))
        .collect();
    let report = php(&in_php_class(&helpers));
    assert!(!categories(&report).contains(prof::LOW_COHESION));
}

#[test]
fn php_phpunit_assertions_are_recognised() {
    let wall: String = (0..12)
        .map(|i| format!("        $this->assertSame({i}, $thing->a{i}());\n"))
        .collect();
    let report = score(
        &format!("<?php\nclass ThingTest\n{{\n    public function testTotals()\n    {{\n{wall}    }}\n}}\n"),
        "tests/ThingTest.php",
        "php",
    );
    assert_eq!(
        detail(&report, prof::LARGE_ASSERTION_BLOCKS).as_deref(),
        Some("12 consecutive assertions")
    );
}

#[test]
fn php_markup_is_not_counted_as_code() {
    // A `.php` file may be HTML with code islands in it. A line this tool cannot parse
    // is a line it cannot explain a finding about, so it is not code.
    let markup: String = (0..40).map(|i| format!("  <p>row {i}</p>\n")).collect();
    let report = score(
        &format!("<?php $title = \"x\"; ?>\n<html>\n{markup}</html>\n<?php echo $title; ?>\n"),
        "legacy.php",
        "php",
    );
    assert!(
        report.loc <= 4,
        "only the code lines count, got {}",
        report.loc
    );
    assert!(!categories(&report).contains(prof::FILE_SIZE));
}

#[test]
fn php_test_and_vendor_paths_are_recognised() {
    let profile = Profile::default();
    assert!(profile.test_patterns.matches("tests/Unit/InvoiceTest.php"));
    assert!(profile.test_patterns.matches("InvoiceTest.php"));
    assert!(!profile.test_patterns.matches("src/Billing/Invoice.php"));
    assert!(profile
        .exclude_patterns
        .matches("vendor/composer/ClassLoader.php"));
    assert!(profile
        .exclude_patterns
        .matches("resources/views/home.blade.php"));
}

#[test]
fn inline_test_modules_do_not_count_toward_a_module_s_functions() {
    // Rust keeps its unit tests in the file they test, so the path-based exemption
    // never sees them. Counting them would mean the cheapest way to fix a "god module"
    // finding is to delete the tests — the one gradient this tool must never contain.
    let shipped: String = (0..12)
        .map(|i| format!("pub fn f{i}() -> u8 {{ {i} }}\n"))
        .collect();
    let tests: String = (0..19)
        .map(|i| format!("    #[test]\n    fn t{i}() {{ assert_eq!(f{i}(), {i}); }}\n"))
        .collect();
    let report = score(
        &format!("{shipped}\n#[cfg(test)]\nmod tests {{\n    use super::*;\n{tests}}}\n"),
        "sample.rs",
        "rust",
    );
    assert!(
        !categories(&report).contains(prof::FUNCTION_COUNT),
        "12 shipped functions is under the limit, whatever the test count: {:?}",
        report
            .all_findings()
            .iter()
            .map(|f| &f.details)
            .collect::<Vec<_>>()
    );

    // What the file actually ships is still counted.
    let many: String = (0..25)
        .map(|i| format!("pub fn f{i}() -> u8 {{ {i} }}\n"))
        .collect();
    let report = score(&many, "sample.rs", "rust");
    assert_eq!(
        detail(&report, prof::FUNCTION_COUNT).as_deref(),
        Some("25 functions")
    );
}
