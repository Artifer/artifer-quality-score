//! Rules about a whole file or module: size, breadth, and diffuse complexity.

use super::*;

/// Near-identical function bodies within one file.
///
/// Compares structure only: identifiers and literals are normalised away, so two
/// handlers that differ solely in field names still register. Intra-file by design —
/// cross-file clone detection needs an index, which would cost the sub-second scan
/// time the TDD loop depends on.
pub(crate) fn code_duplication(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let min_loc = ctx.threshold("function_duplication_min_lines_of_code_for_check");
    let min_similarity = ctx.threshold("function_duplication_min_similarity_percentage") / 100.0;

    let candidates: Vec<(&FunctionContext, Vec<&str>)> = ctx
        .functions
        .iter()
        .filter(|f| (f.loc as f64) >= min_loc)
        .map(|f| (f, metrics::normalised_tokens(&f.node, ctx.spec, ctx.source)))
        .filter(|(_, tokens)| !tokens.is_empty())
        .collect();

    let mut reported: HashSet<String> = HashSet::new();
    for index in 0..candidates.len() {
        let (function, tokens) = &candidates[index];
        for (other, other_tokens) in candidates.iter().skip(index + 1) {
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

pub(crate) fn file_size(ctx: &FileContext, out: &mut Vec<RawFinding>) {
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
pub(crate) fn declaration_size(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    if ctx.spec.declaration_nodes.is_empty() {
        return;
    }
    let total: usize = metrics::all_nodes(&ctx.root)
        .iter()
        .filter(|node| metrics::is_kind(node, &ctx.spec.declaration_nodes))
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

/// How many functions a reader has to hold, and what to call them in the report.
///
/// Two exclusions, both about counting what the file *ships*. A Vue component is
/// counted on what runs — its methods, watchers and lifecycle hooks — and not on what
/// merely describes it; see `sfc::DECLARATIVE_OPTIONS` for why. And a Rust file's
/// inline `#[cfg(test)]` module is not part of the module a reader has to hold: counting
/// it would mean a file of nine functions and nineteen tests reads as a 28-function god
/// module, and the cheapest way to fix that would be to delete the tests.
///
/// The wording changes with the count so the number stays one a reader can reproduce by
/// counting the same things.
fn counted_functions(ctx: &FileContext) -> (usize, &'static str) {
    let shipped = ctx
        .functions
        .iter()
        .filter(|f| !ctx.is_inline_test(&f.node));

    if !crate::sfc::is_vue(&ctx.path) {
        return (shipped.count(), "functions");
    }

    let declarative = crate::sfc::declarative_ranges(&ctx.root, ctx.source);
    let behaviour = shipped
        .filter(|f| !crate::sfc::within(&declarative, f.node.start_byte()))
        .count();

    (behaviour, "methods and handlers")
}

pub(crate) fn function_count(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let threshold = ctx.size_threshold("module_max_functions");
    let (count, noun) = counted_functions(ctx);
    if (count as f64) > threshold {
        out.push(RawFinding {
            category: prof::FUNCTION_COUNT,
            details: format!("{count} {noun}"),
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
pub(crate) fn overall_complexity(ctx: &FileContext, out: &mut Vec<RawFinding>) {
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
