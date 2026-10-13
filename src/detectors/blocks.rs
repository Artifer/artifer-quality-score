//! Repeated statement blocks *inside* one file.
//!
//! The gap this fills: [`module_level::code_duplication`] compares whole functions with a
//! 15-line floor, and [`crate::clones`] matches whole functions across files. Neither can
//! see a three-line block repeated in forty-nine otherwise-different functions — the unit
//! of comparison is too big in both. That shape is extremely common in real code: the
//! same error-reporting preamble, the same permission check, the same telemetry call,
//! pasted wherever it was needed.
//!
//! Matching is exact on normalised tokens, like the rest of the duplication family, so
//! identifiers and literals are collapsed and only structure has to agree.
//!
//! Two things make this harder than the assertion-block rule it generalises:
//!
//! **The window has to slide.** [`test_level::duplicated_assertion_blocks`] groups
//! *maximal* runs, which works because a run of assertions is bounded by non-assertions.
//! A repeated block has no such boundary — it sits in the middle of code that differs —
//! so every window of `min_statements` consecutive statements is a candidate.
//!
//! **Sliding windows over-report.** A six-statement block repeated twice contains four
//! matching three-statement windows, which would be four findings for one problem. So a
//! matched group is grown to its true extent and every statement it covers is consumed,
//! leaving one finding per copy.

use super::*;

/// Where one candidate block sits: which statement sequence, and where within it.
#[derive(Clone, Copy, PartialEq)]
struct Span {
    sequence: usize,
    start: usize,
    length: usize,
}

impl Span {
    fn end(&self) -> usize {
        self.start + self.length
    }

    fn overlaps(&self, other: &Span) -> bool {
        self.sequence == other.sequence && self.start < other.end() && other.start < self.end()
    }
}

/// Whether a statement declares a function or class rather than doing something.
///
/// These terminate a run. Without that, three consecutive Django view functions —
/// `@api_view(...)`, `@permission_classes(...)`, `def view(...)` — read as a three
/// statement "block" repeated down the module, and a file of similarly-shaped views
/// reports a dozen findings that are really one observation about the whole file. Whole
/// repeated *functions* already have two rules of their own: `Code Duplication` within a
/// file and the Cross-File Duplication observation across them.
///
/// The check looks one level down as well as at the node itself, because a definition is
/// routinely wrapped: Python's `decorated_definition` holds the decorators and the `def`,
/// and TypeScript's `export_statement` holds the declaration.
fn defines_a_unit(node: &Node, spec: &LanguageSpec) -> bool {
    let is_definition =
        |kind: &str| spec.function_nodes.contains(kind) || spec.class_nodes.contains(kind);
    is_definition(node.kind())
        || metrics::named_children(node)
            .iter()
            .any(|child| is_definition(child.kind()))
}

/// Runs of consecutive sibling statements, split wherever a definition interrupts them.
///
/// Document order matters beyond tidiness: `scoring` breaks per-rule cap ties by
/// discovery order, so a different traversal would produce different penalties.
fn statement_sequences<'tree>(ctx: &FileContext<'tree, '_>) -> Vec<Vec<Node<'tree>>> {
    let in_test_module = |node: &Node| ctx.is_inline_test(node);

    metrics::all_nodes(&ctx.root)
        .iter()
        .filter(|node| {
            metrics::is_kind(node, &ctx.spec.statement_containers) && !in_test_module(node)
        })
        .flat_map(|node| runs_in(node, ctx.spec))
        .collect()
}

/// Split one statement list into runs, breaking wherever a definition appears.
fn runs_in<'tree>(container: &Node<'tree>, spec: &LanguageSpec) -> Vec<Vec<Node<'tree>>> {
    let mut runs: Vec<Vec<Node<'tree>>> = Vec::new();
    let mut run: Vec<Node<'tree>> = Vec::new();
    for statement in metrics::named_children(container) {
        if defines_a_unit(&statement, spec) {
            if !run.is_empty() {
                runs.push(std::mem::take(&mut run));
            }
            continue;
        }
        run.push(statement);
    }
    if !run.is_empty() {
        runs.push(run);
    }
    runs
}

/// The normalised token sequence of every statement, positionally parallel to `sequences`.
fn tokenise(ctx: &FileContext, sequences: &[Vec<Node>]) -> Vec<Vec<Vec<&'static str>>> {
    sequences
        .iter()
        .map(|statements| {
            statements
                .iter()
                .map(|statement| metrics::normalised_tokens(statement, ctx.spec, ctx.source))
                .collect()
        })
        .collect()
}

/// The signature of `length` statements starting at `start`.
fn signature(tokens: &[Vec<&'static str>], start: usize, length: usize) -> Vec<&'static str> {
    tokens[start..start + length].concat()
}

/// Group every fixed-size window in the file by its signature, in document order.
///
/// Order decides which group claims a contested statement, and document order is the right
/// one *because* extents are settled before copies are chosen: the earliest window inside a
/// repetition grows to cover the whole of it and consumes the rest, so a ten-statement
/// duplication is reported once, at its true length. Ordering by signature size instead —
/// an earlier attempt at the same goal — anchors mid-block and under-reports the extent.
/// It is also what keeps this deterministic: `scoring` breaks per-rule cap ties by emission
/// order.
#[allow(clippy::type_complexity)]
fn windows_by_signature(
    tokens: &[Vec<Vec<&'static str>>],
    width: usize,
) -> (
    Vec<Vec<&'static str>>,
    HashMap<Vec<&'static str>, Vec<Span>>,
) {
    let mut order: Vec<Vec<&'static str>> = Vec::new();
    let mut groups: HashMap<Vec<&'static str>, Vec<Span>> = HashMap::new();

    for (sequence, statements) in tokens.iter().enumerate() {
        if statements.len() < width {
            continue;
        }
        for start in 0..=(statements.len() - width) {
            let key = signature(statements, start, width);
            let entry = groups.entry(key.clone());
            if matches!(entry, std::collections::hash_map::Entry::Vacant(_)) {
                order.push(key);
            }
            entry.or_default().push(Span {
                sequence,
                start,
                length: width,
            });
        }
    }

    (order, groups)
}

/// How many consecutive statements `a` and `b` share, starting where each begins.
///
/// When both sit in the same sequence, `a` may only grow up to where `b` starts: letting
/// them run together would merge two repetitions into one and silently lose a copy.
/// Callers pass spans in document order, so `a` is always the earlier of the two.
fn common_run(a: &Span, b: &Span, tokens: &[Vec<Vec<&'static str>>]) -> usize {
    let limit = if a.sequence == b.sequence {
        b.start.saturating_sub(a.start)
    } else {
        usize::MAX
    };
    let (left, right) = (&tokens[a.sequence], &tokens[b.sequence]);
    let mut length = 0;
    while length < limit {
        match (left.get(a.start + length), right.get(b.start + length)) {
            (Some(x), Some(y)) if x == y => length += 1,
            _ => break,
        }
    }
    length
}

/// One repetition of a signature: the copies, all grown to the same true extent.
///
/// Extent is settled *before* copies are chosen, which is the whole subtlety. Choosing
/// disjoint fixed-width windows first and growing them afterwards looks equivalent and is
/// not: in a run of eight identical statements every window matches every other, so the
/// greedy pass picks four three-wide windows that then block each other from growing, and
/// one duplicated eight-statement block gets reported as "3 statements repeated 4 times".
/// Measuring the longest run against a partner first gets the honest answer.
fn best_repetition(
    spans: &[Span],
    tokens: &[Vec<Vec<&'static str>>],
    consumed: &HashSet<(usize, usize)>,
) -> Option<Vec<Span>> {
    let free = |span: &Span, length: usize| {
        !(span.start..span.start + length).any(|index| consumed.contains(&(span.sequence, index)))
    };

    let available: Vec<Span> = spans
        .iter()
        .copied()
        .filter(|s| free(s, s.length))
        .collect();
    let (anchor, rest) = available.split_first()?;

    let length = rest
        .iter()
        .map(|candidate| common_run(anchor, candidate, tokens))
        .max()
        .filter(|found| *found >= anchor.length)?;

    // A run of structurally identical statements is a list, not a repetition: a dispatch
    // table, a chain of registrations, nine detectors called in a row. Any window of it
    // matches any other window, so the machinery above happily reports the second half of
    // the list as a copy of the first. It is periodic, not duplicated.
    let extent = &tokens[anchor.sequence][anchor.start..anchor.start + length];
    if extent.windows(2).all(|pair| pair[0] == pair[1]) {
        return None;
    }

    let mut chosen = vec![Span { length, ..*anchor }];
    for candidate in rest {
        let copy = Span {
            length,
            ..*candidate
        };
        let agrees = common_run(anchor, candidate, tokens) >= length;
        let unclaimed = free(&copy, length) && !chosen.iter().any(|other| copy.overlaps(other));
        if agrees && unclaimed {
            chosen.push(copy);
        }
    }
    (chosen.len() >= 2).then_some(chosen)
}

/// A statement block repeated verbatim in structure somewhere else in the same file.
pub(crate) fn duplicated_block(ctx: &FileContext, out: &mut Vec<RawFinding>) {
    let width = ctx.threshold("duplicated_block_min_statements") as usize;
    let min_tokens = ctx.threshold("duplicated_block_min_tokens") as usize;
    if width == 0 {
        return;
    }

    let sequences = statement_sequences(ctx);
    let tokens = tokenise(ctx, &sequences);
    let (order, groups) = windows_by_signature(&tokens, width);

    let mut consumed: HashSet<(usize, usize)> = HashSet::new();
    for key in order {
        // A short block of generic statements — three assignments, three calls — matches
        // everywhere and means nothing. The token floor is what separates a repeated
        // *idea* from a repeated *shape*.
        if key.len() < min_tokens {
            continue;
        }
        // One signature can hold more than one independent repetition: two copies may
        // share a long run while three others share only the window. Taking the longest
        // first and repeating leaves the shorter group intact rather than dropping it.
        while let Some(copies) = best_repetition(&groups[&key], &tokens, &consumed) {
            consume(&copies, &mut consumed);
            emit(&copies, &sequences, ctx, out);
        }
    }
}

/// Claim every statement a reported repetition covers, so no window inside it fires again.
fn consume(copies: &[Span], consumed: &mut HashSet<(usize, usize)>) {
    for copy in copies {
        for index in copy.start..copy.end() {
            consumed.insert((copy.sequence, index));
        }
    }
}

/// The first copy is not the problem; every repeat after it is.
fn emit(copies: &[Span], sequences: &[Vec<Node>], ctx: &FileContext, out: &mut Vec<RawFinding>) {
    for copy in copies.iter().skip(1) {
        let node = sequences[copy.sequence][copy.start];
        out.push(RawFinding {
            category: prof::DUPLICATED_BLOCK,
            details: format!("{} statements repeated {} times", copy.length, copies.len()),
            range: metrics::to_range(&node),
            value: copies.len() as f64,
            threshold: 2.0,
            function: test_level::enclosing_function(&node, ctx),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(sequence: usize, start: usize, length: usize) -> Span {
        Span {
            sequence,
            start,
            length,
        }
    }

    #[test]
    fn spans_overlap_when_they_share_a_statement_of_one_sequence() {
        let block = span(0, 2, 3);
        for (other, shared) in [
            (span(0, 4, 3), true),
            (span(0, 0, 3), true),
            (span(0, 3, 1), true),
            // Touching is not sharing: one ends where the other begins.
            (span(0, 5, 3), false),
            (span(0, 0, 2), false),
            // The same statements of another sequence are other statements.
            (span(1, 2, 3), false),
        ] {
            assert_eq!(block.overlaps(&other), shared);
            assert_eq!(other.overlaps(&block), shared);
        }
    }
}
