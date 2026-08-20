//! Rules that only apply to test code.
//!
//! A wall of assertions is a missing abstraction, and a copy-pasted assertion block
//! makes a suite expensive to change. Neither says anything about production code, so
//! both are gated on the file being a test.

use super::*;

/// Maximal runs of consecutive assertion statements.
pub(crate) fn assertion_runs<'tree>(ctx: &FileContext<'tree, '_>) -> Vec<Vec<Node<'tree>>> {
    let mut runs = Vec::new();
    for node in metrics::all_nodes(&ctx.root) {
        if !metrics::is_kind(&node, &ctx.spec.statement_containers) {
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

pub(crate) fn enclosing_function(node: &Node, ctx: &FileContext) -> Option<String> {
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
pub(crate) fn large_assertion_blocks(ctx: &FileContext, out: &mut Vec<RawFinding>) {
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

/// Group assertion runs by their normalised token signature, insertion-ordered.
#[allow(clippy::type_complexity)]
pub(crate) fn group_assertion_runs<'tree>(
    ctx: &FileContext<'tree, '_>,
    min_statements: usize,
) -> (Vec<Vec<&'static str>>, HashMap<Vec<&'static str>, Vec<Node<'tree>>>) {
    let mut order: Vec<Vec<&'static str>> = Vec::new();
    let mut groups: HashMap<Vec<&'static str>, Vec<Node<'tree>>> = HashMap::new();
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
    (order, groups)
}

/// The same assertion block copy-pasted across a test suite.
pub(crate) fn duplicated_assertion_blocks(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    if !ctx.is_test {
        return;
    }
    let min_statements = ctx.threshold("duplicated_assertion_block_min_statements") as usize;
    let (order, groups) = group_assertion_runs(ctx, min_statements);

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

