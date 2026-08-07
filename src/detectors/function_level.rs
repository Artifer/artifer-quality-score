//! Rules about a single function.
//!
//! `details` strings follow CodeScene's formats (`cc = 14`, `bumps = 4`,
//! `Nesting depth = 5 conditionals`): compact, readable in a diff, and no reason to
//! invent a second dialect.

use super::*;

pub(crate) fn complex_method(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
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

pub(crate) fn large_method(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
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

pub(crate) fn deep_nested_complexity(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
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

pub(crate) fn bumpy_road(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
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

pub(crate) fn complex_conditional(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
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

/// Both argument-count rules: same measurement, different threshold and audience.
///
/// Constructors are charged under Constructor Over-Injection and ordinary functions
/// under Excess Number of Function Arguments, never both — charging twice would
/// double-count one signature.
pub(crate) fn argument_count(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let (category, threshold_key) = if f.is_constructor {
        (prof::CONSTRUCTOR_OVER_INJECTION, "constructor_max_arguments")
    } else {
        (prof::EXCESS_ARGUMENTS, "function_max_arguments")
    };
    let threshold = ctx.threshold(threshold_key);
    let count = f.parameters.len();
    if (count as f64) > threshold {
        out.push(RawFinding {
            category,
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
pub(crate) fn brain_method(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let loc_threshold = ctx.threshold("brain_method_lines_of_code");
    let cc_threshold = ctx.threshold("brain_method_cyclomatic_complexity");
    let nesting_threshold = ctx.threshold("brain_method_nesting_depth");
    let is_brain = (f.loc as f64) > loc_threshold
        && (f.complexity as f64) > cc_threshold
        && (f.nesting as f64) > nesting_threshold;
    if is_brain {
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

pub(crate) fn string_heavy_arguments(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
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
pub(crate) fn primitive_obsession(f: &FunctionContext, ctx: &FileContext, out: &mut Vec<RawFinding>) {
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

