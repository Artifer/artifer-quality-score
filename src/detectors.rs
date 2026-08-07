//! The rule catalogue.
//!
//! Each detector reports a measurement and the threshold it crossed; none of them
//! knows anything about points. Turning findings into a score is `scoring`'s job
//! alone, which is what makes the penalty model swappable.
//!
//! `details` strings follow CodeScene's formats (`cc = 14`, `bumps = 4`,
//! `Nesting depth = 5 conditionals`) — they are compact, they read well in a diff,
//! and there is no reason to invent a second dialect.
//!
//! **Detector order is part of the contract.** `scoring` allocates each rule's
//! per-file budget largest-penalty-first and breaks ties by discovery order, so
//! emitting findings in a different order than the reference implementation would
//! change which findings get trimmed when a cap binds.

use crate::difflib;
use crate::languages::LanguageSpec;
use crate::metrics::{self, COMMENT_NODES};
use crate::models::Range;
use crate::profile::{self as prof, Profile};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

/// A detection, before it has a price.
pub struct RawFinding {
    pub category: &'static str,
    pub details: String,
    pub range: Range,
    pub value: f64,
    pub threshold: f64,
    pub function: Option<String>,
}

/// A named function and its measurements, computed once and shared.
pub struct FunctionContext<'tree> {
    pub node: Node<'tree>,
    pub name: String,
    pub range: Range,
    pub loc: usize,
    pub complexity: usize,
    pub nesting: usize,
    pub parameters: Vec<Node<'tree>>,
    pub is_constructor: bool,
}

pub struct FileContext<'tree, 'src> {
    pub path: String,
    pub spec: &'static LanguageSpec,
    pub profile: &'src Profile,
    pub root: Node<'tree>,
    pub source: &'src [u8],
    pub is_test: bool,
    pub loc: usize,
    pub functions: Vec<FunctionContext<'tree>>,
}

impl<'tree, 'src> FileContext<'tree, 'src> {
    pub fn build(
        path: String,
        spec: &'static LanguageSpec,
        profile: &'src Profile,
        root: Node<'tree>,
        source: &'src [u8],
    ) -> Self {
        let mut functions = Vec::new();
        for node in metrics::iter_functions(&root, spec, source) {
            let name = metrics::function_name(&node, source)
                .unwrap_or_else(|| "<anonymous>".to_string());
            let is_constructor = spec.constructor_names.contains(name.as_str());
            functions.push(FunctionContext {
                node,
                range: metrics::to_range(&node),
                loc: metrics::logical_lines(&node),
                complexity: metrics::cyclomatic_complexity(&node, spec, source),
                nesting: metrics::nesting_depth(&node, spec, source),
                parameters: metrics::parameters(&node, spec, source),
                is_constructor,
                name,
            });
        }
        Self {
            is_test: profile.is_test_path(&path),
            loc: metrics::logical_lines(&root),
            path,
            spec,
            profile,
            root,
            source,
            functions,
        }
    }

    fn threshold(&self, key: &str) -> f64 {
        self.profile.threshold(key)
    }

    /// A size limit, relaxed for test files.
    fn size_threshold(&self, key: &str) -> f64 {
        let value = self.profile.threshold(key);
        if self.is_test {
            value * self.profile.threshold("test_file_size_multiplier")
        } else {
            value
        }
    }
}

/// Just the declaration line — where a reader wants the marker.
fn signature_range(function: &FunctionContext) -> Range {
    Range {
        start_line: function.range.start_line,
        start_column: function.range.start_column,
        end_line: function.range.start_line,
        end_column: function.range.end_column,
    }
}

/// Format a ratio the way Python's `{:.0%}` does.
fn percent(ratio: f64) -> String {
    format!("{:.0}%", ratio * 100.0)
}

// --------------------------------------------------------------------------- //
// Function level
// --------------------------------------------------------------------------- //

fn complex_method(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let threshold = ctx.threshold("function_cyclomatic_complexity_warning");
    if (f.complexity as f64) > threshold {
        out.push(RawFinding {
            category: prof::COMPLEX_METHOD,
            details: format!("cc = {}", f.complexity),
            range: signature_range(f),
            value: f.complexity as f64,
            threshold,
            function: Some(f.name.clone()),
        });
    }
}

fn large_method(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let threshold = ctx.size_threshold("function_lines_of_code_warning");
    if (f.loc as f64) > threshold {
        out.push(RawFinding {
            category: prof::LARGE_METHOD,
            details: format!("LoC = {} lines", f.loc),
            range: signature_range(f),
            value: f.loc as f64,
            threshold,
            function: Some(f.name.clone()),
        });
    }
}

fn deep_nested_complexity(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let threshold = ctx.threshold("function_nesting_depth_warning");
    if (f.nesting as f64) > threshold {
        out.push(RawFinding {
            category: prof::DEEP_NESTED_COMPLEXITY,
            details: format!("Nesting depth = {} conditionals", f.nesting),
            range: signature_range(f),
            value: f.nesting as f64,
            threshold,
            function: Some(f.name.clone()),
        });
    }
}

fn bumpy_road(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let min_bumps = ctx.threshold("bumpy_road_min_bumps");
    let min_depth = ctx.threshold("bumpy_road_min_bump_depth") as usize;
    let found = metrics::bumps(&f.node, ctx.spec, ctx.source, min_depth);
    if (found.len() as f64) >= min_bumps {
        out.push(RawFinding {
            category: prof::BUMPY_ROAD,
            details: format!("bumps = {}", found.len()),
            range: signature_range(f),
            value: found.len() as f64,
            threshold: min_bumps,
            function: Some(f.name.clone()),
        });
    }
}

fn complex_conditional(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let min_count = ctx.threshold("complex_conditional_min_count");
    let min_operators = ctx.threshold("complex_conditional_min_operators") as usize;
    let found = metrics::complex_conditionals(&f.node, ctx.spec, ctx.source, min_operators);
    if (found.len() as f64) >= min_count && !found.is_empty() {
        let plural = if found.len() == 1 { "expression" } else { "expressions" };
        // Anchor on the first offending conditional, not the signature: this is an
        // expression-level smell and the exact spot is the actionable part.
        out.push(RawFinding {
            category: prof::COMPLEX_CONDITIONAL,
            details: format!("{} complex conditional {}", found.len(), plural),
            range: metrics::to_range(&found[0].0),
            value: found.len() as f64,
            threshold: min_count,
            function: Some(f.name.clone()),
        });
    }
}

fn excess_arguments(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
    // Constructors are covered by Constructor Over-Injection; charging both would
    // double-count the same signature.
    if f.is_constructor {
        return;
    }
    let threshold = ctx.threshold("function_max_arguments");
    let count = f.parameters.len();
    if (count as f64) > threshold {
        out.push(RawFinding {
            category: prof::EXCESS_ARGUMENTS,
            details: format!("Arguments = {count}"),
            range: signature_range(f),
            value: count as f64,
            threshold,
            function: Some(f.name.clone()),
        });
    }
}

fn constructor_over_injection(
    f: &FunctionContext,
    ctx: &FileContext,
    out: &mut Vec<RawFinding>,
) {
    if !f.is_constructor {
        return;
    }
    let threshold = ctx.threshold("constructor_max_arguments");
    let count = f.parameters.len();
    if (count as f64) > threshold {
        out.push(RawFinding {
            category: prof::CONSTRUCTOR_OVER_INJECTION,
            details: format!("Arguments = {count}"),
            range: signature_range(f),
            value: count as f64,
            threshold,
            function: Some(f.name.clone()),
        });
    }
}

/// Compound rule: size *and* complexity *and* nesting, all at once.
///
/// CodeScene's version also weighs a centrality score, which needs whole-repo
/// call-graph data we deliberately do not collect. Size, complexity and depth
/// together still isolate the one function in a module that has quietly become its
/// centre of gravity.
fn brain_method(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let loc_threshold = ctx.threshold("brain_method_lines_of_code");
    let cc_threshold = ctx.threshold("brain_method_cyclomatic_complexity");
    let nesting_threshold = ctx.threshold("brain_method_nesting_depth");
    if (f.loc as f64) > loc_threshold
        && (f.complexity as f64) > cc_threshold
        && (f.nesting as f64) > nesting_threshold
    {
        out.push(RawFinding {
            category: prof::BRAIN_METHOD,
            details: format!(
                "LoC = {} lines, cc = {}, nesting depth = {}",
                f.loc, f.complexity, f.nesting
            ),
            range: signature_range(f),
            value: f.complexity as f64,
            threshold: cc_threshold,
            function: Some(f.name.clone()),
        });
    }
}

fn string_heavy_arguments(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let threshold = ctx.threshold("function_string_arguments_warning");
    let strings = f
        .parameters
        .iter()
        .filter(|param| {
            let annotation = metrics::parameter_type(param, ctx.spec, ctx.source);
            metrics::is_string_type(annotation.as_deref(), ctx.spec)
        })
        .count();
    if (strings as f64) > threshold {
        out.push(RawFinding {
            category: prof::STRING_HEAVY_ARGUMENTS,
            details: format!("{strings} string arguments"),
            range: signature_range(f),
            value: strings as f64,
            threshold,
            function: Some(f.name.clone()),
        });
    }
}

/// A wide signature made almost entirely of built-in types.
///
/// Only annotated parameters are considered — an unannotated parameter carries no
/// evidence either way, and guessing would make the score depend on how thoroughly
/// the code is typed rather than on how well it is designed.
fn primitive_obsession(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let min_count = ctx.threshold("function_primitive_arguments_min_count");
    let ratio_threshold = ctx.threshold("function_primitive_arguments_ratio");
    let annotated: Vec<String> = f
        .parameters
        .iter()
        .filter_map(|param| metrics::parameter_type(param, ctx.spec, ctx.source))
        .filter(|name| !name.is_empty())
        .collect();
    if (annotated.len() as f64) < min_count {
        return;
    }
    let primitives = annotated
        .iter()
        .filter(|name| metrics::is_primitive_type(Some(name), ctx.spec))
        .count();
    let ratio = primitives as f64 / annotated.len() as f64;
    if ratio >= ratio_threshold {
        out.push(RawFinding {
            category: prof::PRIMITIVE_OBSESSION,
            details: format!(
                "{} of {} arguments are primitives ({})",
                primitives,
                annotated.len(),
                percent(ratio)
            ),
            range: signature_range(f),
            value: primitives as f64,
            threshold: min_count,
            function: Some(f.name.clone()),
        });
    }
}

// --------------------------------------------------------------------------- //
// File / module level
// --------------------------------------------------------------------------- //

/// Near-identical function bodies within one file.
///
/// Compares structure only: identifiers and literals are normalised away, so two
/// handlers that differ solely in field names still register. Intra-file by design —
/// cross-file clone detection needs an index, which would cost the sub-second scan
/// time the TDD loop depends on.
fn code_duplication(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let min_loc = ctx.threshold("function_duplication_min_lines_of_code_for_check");
    let min_similarity =
        ctx.threshold("function_duplication_min_similarity_percentage") / 100.0;

    let candidates: Vec<(&FunctionContext, Vec<&str>)> = ctx
        .functions
        .iter()
        .filter(|f| (f.loc as f64) >= min_loc)
        .map(|f| (f, metrics::normalised_tokens(&f.node, ctx.spec, ctx.source)))
        .collect();

    let mut reported: HashSet<String> = HashSet::new();
    for index in 0..candidates.len() {
        let (function, tokens) = &candidates[index];
        for (other, other_tokens) in candidates.iter().skip(index + 1) {
            if tokens.is_empty() || other_tokens.is_empty() {
                continue;
            }
            let (shorter, longer) = if tokens.len() <= other_tokens.len() {
                (tokens.len(), other_tokens.len())
            } else {
                (other_tokens.len(), tokens.len())
            };
            // Length alone caps the achievable ratio, so this skips the expensive
            // comparison for pairs that cannot possibly match.
            if (shorter as f64) / (longer as f64) < min_similarity {
                continue;
            }
            let ratio = difflib::ratio(tokens, other_tokens);
            if ratio < min_similarity {
                continue;
            }
            let key = format!("{}:{}", other.name, other.range.start_line);
            if !reported.insert(key) {
                continue;
            }
            out.push(RawFinding {
                category: prof::CODE_DUPLICATION,
                details: format!("{} similar to {}", percent(ratio), function.name),
                range: signature_range(other),
                value: ratio * 100.0,
                threshold: min_similarity * 100.0,
                function: Some(other.name.clone()),
            });
        }
    }
}

/// Maximal runs of consecutive assertion statements.
fn assertion_runs<'tree>(ctx: &FileContext<'tree, '_>) -> Vec<Vec<Node<'tree>>> {
    let mut runs = Vec::new();
    for node in metrics::all_nodes(&ctx.root) {
        if !matches!(node.kind(), "block" | "statement_block" | "module" | "program") {
            continue;
        }
        let mut run: Vec<Node<'tree>> = Vec::new();
        for statement in metrics::named_children(&node) {
            if metrics::is_assertion(&statement, ctx.spec, ctx.source) {
                run.push(statement);
                continue;
            }
            if !run.is_empty() {
                runs.push(std::mem::take(&mut run));
            }
            run.clear();
        }
        if !run.is_empty() {
            runs.push(run);
        }
    }
    runs
}

fn enclosing_function(node: &Node, ctx: &FileContext) -> Option<String> {
    let mut current = node.parent();
    while let Some(candidate) = current {
        if metrics::is_scope_boundary(&candidate, ctx.spec, ctx.source) {
            return metrics::function_name(&candidate, ctx.source);
        }
        current = candidate.parent();
    }
    None
}

/// A wall of consecutive asserts, which is a missing abstraction.
fn large_assertion_blocks(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    if !ctx.is_test {
        return;
    }
    let threshold = ctx.threshold("assertion_block_max_statements");
    for run in assertion_runs(ctx) {
        if (run.len() as f64) > threshold {
            out.push(RawFinding {
                category: prof::LARGE_ASSERTION_BLOCKS,
                details: format!("{} consecutive assertions", run.len()),
                range: metrics::to_range(&run[0]),
                value: run.len() as f64,
                threshold,
                function: enclosing_function(&run[0], ctx),
            });
        }
    }
}

/// The same assertion block copy-pasted across a test suite.
fn duplicated_assertion_blocks(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    if !ctx.is_test {
        return;
    }
    let min_statements = ctx.threshold("duplicated_assertion_block_min_statements") as usize;
    // Insertion-ordered grouping, matching the reference implementation's dict.
    let mut order: Vec<Vec<&'static str>> = Vec::new();
    let mut groups: HashMap<Vec<&'static str>, Vec<Node>> = HashMap::new();

    for run in assertion_runs(ctx) {
        if run.len() < min_statements {
            continue;
        }
        let mut signature: Vec<&'static str> = Vec::new();
        for statement in &run {
            signature.extend(metrics::normalised_tokens(statement, ctx.spec, ctx.source));
        }
        let entry = groups.entry(signature.clone());
        if matches!(entry, std::collections::hash_map::Entry::Vacant(_)) {
            order.push(signature.clone());
        }
        entry.or_default().push(run[0]);
    }

    for signature in order {
        let occurrences = &groups[&signature];
        if occurrences.len() < 2 {
            continue;
        }
        // Report on every repeat after the first; the original is not the problem.
        for node in occurrences.iter().skip(1) {
            out.push(RawFinding {
                category: prof::DUPLICATED_ASSERTION_BLOCKS,
                details: format!("assertion block repeated {} times", occurrences.len()),
                range: metrics::to_range(node),
                value: occurrences.len() as f64,
                threshold: 2.0,
                function: enclosing_function(node, ctx),
            });
        }
    }
}

fn file_size(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let threshold = ctx.size_threshold("file_lines_of_code_warning");
    if (ctx.loc as f64) > threshold {
        out.push(RawFinding {
            category: prof::FILE_SIZE,
            details: format!("LoC = {} lines", ctx.loc),
            range: Range::line(1),
            value: ctx.loc as f64,
            threshold,
            function: None,
        });
    }
}

/// Type/interface/enum declarations dominating a file.
fn declaration_size(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    if ctx.spec.declaration_nodes.is_empty() {
        return;
    }
    let total: usize = metrics::all_nodes(&ctx.root)
        .iter()
        .filter(|node| ctx.spec.declaration_nodes.contains(node.kind()))
        .map(metrics::logical_lines)
        .sum();
    let threshold = ctx.size_threshold("file_declaration_lines_warning");
    if (total as f64) > threshold {
        out.push(RawFinding {
            category: prof::DECLARATION_SIZE,
            details: format!("LoC = {total} lines of declarations"),
            range: Range::line(1),
            value: total as f64,
            threshold,
            function: None,
        });
    }
}

fn function_count(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let threshold = ctx.size_threshold("module_max_functions");
    let count = ctx.functions.len();
    if (count as f64) > threshold {
        out.push(RawFinding {
            category: prof::FUNCTION_COUNT,
            details: format!("{count} functions"),
            range: Range::line(1),
            value: count as f64,
            threshold,
            function: None,
        });
    }
}

/// Mean complexity across a module — diffuse difficulty, not one hotspot.
///
/// Requires at least three functions: with fewer, the per-function rules already say
/// everything there is to say.
fn overall_complexity(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    if ctx.functions.len() < 3 {
        return;
    }
    let threshold = ctx.threshold("module_mean_complexity_warning");
    let total: usize = ctx.functions.iter().map(|f| f.complexity).sum();
    let mean = total as f64 / ctx.functions.len() as f64;
    if mean > threshold {
        out.push(RawFinding {
            category: prof::OVERALL_COMPLEXITY,
            details: format!(
                "mean cc = {:.1} across {} functions",
                mean,
                ctx.functions.len()
            ),
            range: Range::line(1),
            value: mean,
            threshold,
            function: None,
        });
    }
}

fn class_methods<'a, 'tree>(
    class_node: &Node,
    ctx: &'a FileContext<'tree, '_>,
) -> Vec<&'a FunctionContext<'tree>> {
    ctx.functions
        .iter()
        .filter(|f| {
            f.node.start_byte() >= class_node.start_byte()
                && f.node.end_byte() <= class_node.end_byte()
                && f.node.id() != class_node.id()
        })
        .collect()
}

/// A large, many-method class that also contains a Brain Method.
fn brain_class(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let loc_threshold = ctx.size_threshold("class_max_lines_of_code");
    let fn_threshold = ctx.size_threshold("class_max_functions");
    let brain_loc = ctx.threshold("brain_method_lines_of_code");
    let brain_cc = ctx.threshold("brain_method_cyclomatic_complexity");
    let brain_nesting = ctx.threshold("brain_method_nesting_depth");

    for class_node in metrics::class_nodes(&ctx.root, ctx.spec) {
        let methods = class_methods(&class_node, ctx);
        let loc = metrics::logical_lines(&class_node);
        let has_brain_method = methods.iter().any(|f| {
            (f.loc as f64) > brain_loc
                && (f.complexity as f64) > brain_cc
                && (f.nesting as f64) > brain_nesting
        });
        if (loc as f64) > loc_threshold
            && (methods.len() as f64) > fn_threshold
            && has_brain_method
        {
            let name = metrics::class_name(&class_node, ctx.source)
                .unwrap_or_else(|| "<anonymous class>".to_string());
            out.push(RawFinding {
                category: prof::BRAIN_CLASS,
                details: format!(
                    "LoC = {} lines, {} functions, contains a Brain Method",
                    loc,
                    methods.len()
                ),
                range: Range::line(class_node.start_position().row + 1),
                value: loc as f64,
                threshold: loc_threshold,
                function: Some(name),
            });
        }
    }
}

/// Instance fields a method touches (`self.x` / `this.x`).
fn self_attributes(f: &FunctionContext, ctx: &FileContext) -> HashSet<String> {
    let mut fields = HashSet::new();
    for node in metrics::own_nodes(&f.node, ctx.spec, ctx.source) {
        // Python `attribute`, TS/JS `member_expression`, Rust `field_expression` — the
        // same construct under three names, with three sets of field names.
        if !matches!(node.kind(), "attribute" | "member_expression" | "field_expression") {
            continue;
        }
        let object = node
            .child_by_field_name("object")
            .or_else(|| node.child_by_field_name("value"));
        let Some(object) = object else { continue };
        let attribute = node
            .child_by_field_name("attribute")
            .or_else(|| node.child_by_field_name("property"))
            .or_else(|| node.child_by_field_name("field"));
        let Some(attribute) = attribute else { continue };
        let receiver = metrics::text(&object, ctx.source);
        if receiver == "self" || receiver == "this" {
            fields.insert(metrics::text(&attribute, ctx.source).to_string());
        }
    }
    fields
}

/// LCOM4: how many unrelated responsibilities a class actually holds.
///
/// Methods are connected when they share an instance field or one calls the other;
/// the metric is the number of connected components. Constructors are excluded, since
/// they touch every field and would make any class look perfectly cohesive.
fn low_cohesion(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let threshold = ctx.threshold("class_max_lcom4");

    for class_node in metrics::class_nodes(&ctx.root, ctx.spec) {
        if metrics::is_abstract_class(&class_node, ctx.spec, ctx.source) {
            continue;
        }
        let methods: Vec<&FunctionContext> = class_methods(&class_node, ctx)
            .into_iter()
            .filter(|f| {
                !f.is_constructor
                    && !metrics::is_pure_accessor(&f.node, ctx.spec, ctx.source)
            })
            .collect();
        if methods.len() < 2 {
            continue;
        }

        let mut fields: HashMap<String, HashSet<String>> = HashMap::new();
        let mut parent: HashMap<String, String> = HashMap::new();
        for f in &methods {
            fields.insert(f.name.clone(), self_attributes(f, ctx));
            parent.insert(f.name.clone(), f.name.clone());
        }
        let names: HashSet<String> = methods.iter().map(|f| f.name.clone()).collect();

        fn find(parent: &mut HashMap<String, String>, name: &str) -> String {
            let mut current = name.to_string();
            loop {
                let next = parent[&current].clone();
                if next == current {
                    return current;
                }
                let grandparent = parent[&next].clone();
                parent.insert(current.clone(), grandparent);
                current = next;
            }
        }

        fn union(parent: &mut HashMap<String, String>, left: &str, right: &str) {
            let left_root = find(parent, left);
            let right_root = find(parent, right);
            if left_root != right_root {
                parent.insert(right_root, left_root);
            }
        }

        for (index, f) in methods.iter().enumerate() {
            // Shared instance state connects two methods.
            for other in methods.iter().skip(index + 1) {
                let shared = fields[&f.name].intersection(&fields[&other.name]).count() > 0;
                if shared {
                    union(&mut parent, &f.name, &other.name);
                }
            }
            // So does one calling the other.
            let called: Vec<String> =
                fields[&f.name].intersection(&names).cloned().collect();
            for target in called {
                union(&mut parent, &f.name, &target);
            }
        }

        let roots: HashSet<String> =
            methods.iter().map(|f| find(&mut parent, &f.name)).collect();
        let components = roots.len();
        if (components as f64) > threshold {
            let name = metrics::class_name(&class_node, ctx.source)
                .unwrap_or_else(|| "<anonymous class>".to_string());
            out.push(RawFinding {
                category: prof::LOW_COHESION,
                details: format!(
                    "LCOM4 = {} unrelated responsibilities across {} functions",
                    components,
                    methods.len()
                ),
                range: Range::line(class_node.start_position().row + 1),
                value: components as f64,
                threshold,
                function: Some(name),
            });
        }
    }
}

/// Every applicable finding for a file, in a stable order.
///
/// File-level detectors run first, then each function through the function-level
/// detectors, in exactly the registration order of the reference implementation.
pub fn detect(ctx: &FileContext) -> Vec<RawFinding> {
    let mut findings = Vec::new();

    code_duplication(ctx, &mut findings);
    large_assertion_blocks(ctx, &mut findings);
    duplicated_assertion_blocks(ctx, &mut findings);
    file_size(ctx, &mut findings);
    declaration_size(ctx, &mut findings);
    function_count(ctx, &mut findings);
    overall_complexity(ctx, &mut findings);
    brain_class(ctx, &mut findings);
    low_cohesion(ctx, &mut findings);

    for function in &ctx.functions {
        complex_method(function, ctx, &mut findings);
        large_method(function, ctx, &mut findings);
        deep_nested_complexity(function, ctx, &mut findings);
        bumpy_road(function, ctx, &mut findings);
        complex_conditional(function, ctx, &mut findings);
        excess_arguments(function, ctx, &mut findings);
        constructor_over_injection(function, ctx, &mut findings);
        brain_method(function, ctx, &mut findings);
        string_heavy_arguments(function, ctx, &mut findings);
        primitive_obsession(function, ctx, &mut findings);
    }

    findings.retain(|finding| {
        if ctx.is_test && ctx.profile.is_test_exempt(finding.category) {
            return false;
        }
        // A rule set to weight 0 should vanish from the report, not sit at -0.00.
        ctx.profile.rule(finding.category).weight > 0.0
    });
    findings
}

/// Node kinds treated as comments, re-exported for the assertion-run scan.
pub const _COMMENT_NODES: &[&str] = COMMENT_NODES;
