//! The rule catalogue.
//!
//! Split by what each rule looks at. Every detector reports a measurement and the
//! threshold it crossed; turning findings into a score is `scoring`'s job alone, which
//! is what makes the penalty model swappable.
//!
//! | module | rules |
//! |---|---|
//! | [`context`] | the shared per-file measurements, not a rule |
//! | [`function_level`] | Complex Method, Large Method, Deep Nesting, Bumpy Road, Complex Conditional, argument counts, Brain Method, the type smells |
//! | [`module_level`] | Code Duplication, file and declaration size, function count, Overall Code Complexity |
//! | [`test_level`] | Large and Duplicated Assertion Blocks |
//! | [`cohesion`] | Low Cohesion, Brain Class |
//!
//! **Emission order is part of the contract.** `scoring` allocates each rule's per-file
//! budget largest-penalty-first and breaks ties by discovery order, so `detect` must run
//! the rules in the order below and nowhere else.

use crate::difflib;
use crate::languages::LanguageSpec;
use crate::metrics;
use crate::models::Range;
use crate::profile::{self as prof, Profile};
use std::collections::{HashMap, HashSet};
use tree_sitter::Node;

pub mod cohesion;
pub mod context;
pub mod function_level;
pub mod module_level;
pub mod test_level;

pub use context::{FileContext, FunctionContext, RawFinding};

use cohesion::{brain_class, low_cohesion};
use context::{percent, signature_range};
use function_level::{
    argument_count, brain_method, bumpy_road, complex_conditional, complex_method,
    deep_nested_complexity, large_method, primitive_obsession, string_heavy_arguments,
};
use module_level::{
    code_duplication, declaration_size, file_size, function_count, overall_complexity,
};
use test_level::{duplicated_assertion_blocks, large_assertion_blocks};

/// Every applicable finding for a file, in a stable order.
///
/// File-level detectors run first, then each function through the function-level
/// detectors, in exactly this order.
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
        argument_count(function, ctx, &mut findings);
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
