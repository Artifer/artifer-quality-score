//! A faithful port of the parts of Python's `difflib.SequenceMatcher` that
//! `Code Duplication` depends on.
//!
//! This exists because similarity ratios have to agree with the reference
//! implementation to the last bit. Any "reasonable" substitute — Levenshtein,
//! Jaccard, a different LCS — produces different numbers, which would move the
//! duplication threshold and therefore the score.
//!
//! Two details of the original are easy to miss and both matter:
//!
//! **The autojunk heuristic.** For sequences of 200 elements or more, any element
//! occurring in more than 1% of `b` is dropped from the index entirely. Our token
//! streams routinely exceed 200 elements, so this changes real answers — omit it and
//! long functions compare as far more similar than Python reports.
//!
//! **`ratio()` counts matched elements, not edit distance.** It is `2*M/T` over the
//! recursively-found matching blocks, which is not a metric and not symmetric with
//! respect to which sequence is which.

use std::collections::HashMap;

/// Sequence matcher over token slices, with `isjunk=None` and `autojunk=True` —
/// the configuration the reference implementation uses.
pub struct SequenceMatcher<'a> {
    a: &'a [&'a str],
    b: &'a [&'a str],
    /// Index from element to its positions in `b`, minus popular elements.
    b2j: HashMap<&'a str, Vec<usize>>,
}

/// The search window in both sequences. Four bare indices at every call site is
/// exactly the argument swap Primitive Obsession warns about.
#[derive(Debug, Clone)]
struct Window {
    a: std::ops::Range<usize>,
    b: std::ops::Range<usize>,
}

#[derive(Debug, Clone, Copy)]
struct Match {
    a_start: usize,
    b_start: usize,
    size: usize,
}

impl<'a> SequenceMatcher<'a> {
    pub fn new(a: &'a [&'a str], b: &'a [&'a str]) -> Self {
        let mut b2j: HashMap<&'a str, Vec<usize>> = HashMap::new();
        for (index, element) in b.iter().enumerate() {
            b2j.entry(element).or_default().push(index);
        }

        // Purge popular elements. `isjunk` is None here, so there is no junk set and
        // the junk-extension loops in find_longest_match never fire.
        let n = b.len();
        if n >= 200 {
            let ntest = n / 100 + 1;
            b2j.retain(|_, indices| indices.len() <= ntest);
        }

        Self { a, b, b2j }
    }

    /// One row of the dynamic-programming scan: match lengths ending at each `j`.
    fn scan_row(
        &self,
        i: usize,
        previous: &HashMap<usize, usize>,
        b_range: &std::ops::Range<usize>,
        best: &mut Match,
    ) -> HashMap<usize, usize> {
        let mut row: HashMap<usize, usize> = HashMap::new();
        let Some(indices) = self.b2j.get(self.a[i]) else {
            return row;
        };
        for &j in indices {
            if j < b_range.start {
                continue;
            }
            if j >= b_range.end {
                break;
            }
            let earlier = if j == 0 {
                0
            } else {
                previous.get(&(j - 1)).copied().unwrap_or(0)
            };
            let length = earlier + 1;
            row.insert(j, length);
            if length > best.size {
                *best = Match {
                    a_start: i + 1 - length,
                    b_start: j + 1 - length,
                    size: length,
                };
            }
        }
        row
    }

    fn find_longest_match(&self, window: Window) -> Match {
        let mut best = Match {
            a_start: window.a.start,
            b_start: window.b.start,
            size: 0,
        };
        let mut previous: HashMap<usize, usize> = HashMap::new();
        for i in window.a.clone() {
            previous = self.scan_row(i, &previous, &window.b, &mut best);
        }
        // Extend the match with equal elements on either side. With no junk set, these
        // are the only extension loops CPython applies.
        self.extend(best, window)
    }

    /// Grow a match outwards while the elements on either side are equal.
    // @aqs(disable:"Complex Conditional") the bounds check and the equality read better
    // together than behind a predicate name
    fn extend(&self, found: Match, window: Window) -> Match {
        let Match {
            mut a_start,
            mut b_start,
            size,
        } = found;
        let (mut a_end, mut b_end) = (a_start + size, b_start + size);
        // Inline rather than behind `matches_before`/`matches_after` helpers: for a
        // two-line loop, seeing the bounds check and the equality together says more
        // than a predicate name does.
        while a_start > window.a.start
            && b_start > window.b.start
            && self.a[a_start - 1] == self.b[b_start - 1]
        {
            a_start -= 1;
            b_start -= 1;
        }
        while a_end < window.a.end && b_end < window.b.end && self.a[a_end] == self.b[b_end] {
            a_end += 1;
            b_end += 1;
        }
        Match {
            a_start,
            b_start,
            size: a_end - a_start,
        }
    }

    /// Total number of matched elements across all matching blocks.
    fn total_matches(&self) -> usize {
        let mut queue = vec![Window {
            a: 0..self.a.len(),
            b: 0..self.b.len(),
        }];
        let mut total = 0usize;
        while let Some(window) = queue.pop() {
            let found = self.find_longest_match(window.clone());
            if found.size == 0 {
                continue;
            }
            total += found.size;
            let (a_end, b_end) = (found.a_start + found.size, found.b_start + found.size);
            // What lies before the match and what lies after it. Either may be empty on
            // one side, and an empty window simply matches nothing.
            queue.push(Window {
                a: window.a.start..found.a_start,
                b: window.b.start..found.b_start,
            });
            queue.push(Window {
                a: a_end..window.a.end,
                b: b_end..window.b.end,
            });
        }
        total
    }

    /// Similarity in `[0.0, 1.0]`, as `2*M/T`.
    pub fn ratio(&self) -> f64 {
        let length = self.a.len() + self.b.len();
        if length == 0 {
            return 1.0;
        }
        2.0 * self.total_matches() as f64 / length as f64
    }
}

/// Convenience wrapper matching `SequenceMatcher(None, a, b).ratio()`.
pub fn ratio(a: &[&str], b: &[&str]) -> f64 {
    SequenceMatcher::new(a, b).ratio()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_sequences_are_one() {
        let a = ["a", "b", "c"];
        assert_eq!(ratio(&a, &a), 1.0);
    }

    /// Past 200 elements `autojunk` drops the popular ones from the index, so a match is
    /// found on the rare elements and then grown outwards over the popular ones on both
    /// sides. Without that growth two near-identical long functions would score low.
    #[test]
    fn a_long_sequence_matches_across_its_popular_elements() {
        let a = statements(60);
        let mut b = a.clone();
        b[121] = "renamed".to_string();
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        let b: Vec<&str> = b.iter().map(String::as_str).collect();
        // All but the renamed element match: 239 of 240, on both sides.
        let similarity = ratio(&a, &b);
        assert!((similarity - 478.0 / 480.0).abs() < 1e-9, "{similarity}");
    }

    #[test]
    fn growing_a_match_forwards_steps_through_both_sequences() {
        // `a` repeats a popular `)` where `b` has something else. Growth must stop there
        // rather than hold `b` still and match its `)` a second time.
        let (mut a, mut b) = (statements(60), statements(60));
        a[123] = ")".to_string();
        b[123] = "renamed".to_string();
        let a: Vec<&str> = a.iter().map(String::as_str).collect();
        let b: Vec<&str> = b.iter().map(String::as_str).collect();
        let similarity = ratio(&a, &b);
        assert!((similarity - 478.0 / 480.0).abs() < 1e-9, "{similarity}");
    }

    /// `( name0 ) ; ( name1 ) ; ...` -- three popular elements around each rare one.
    fn statements(count: usize) -> Vec<String> {
        let mut tokens: Vec<String> = Vec::new();
        for index in 0..count {
            tokens.extend([
                "(".to_string(),
                format!("name{index}"),
                ")".to_string(),
                ";".to_string(),
            ]);
        }
        tokens
    }

    #[test]
    fn an_element_outside_the_window_is_not_matched_again() {
        // After `a b` is matched, `x` is looked for only before it in `b` — where it is not.
        let similarity = ratio(&["x", "a", "b"], &["a", "b", "x"]);
        assert!((similarity - 2.0 / 3.0).abs() < 1e-9, "{similarity}");
    }

    #[test]
    fn what_matches_on_either_side_of_the_longest_run_counts_too() {
        // `x y z` is the longest run; `a` before it and `q` after it match as well.
        let similarity = ratio(
            &["a", "1", "x", "y", "z", "2", "q"],
            &["a", "3", "x", "y", "z", "4", "q"],
        );
        assert!((similarity - 10.0 / 14.0).abs() < 1e-9, "{similarity}");
    }

    #[test]
    fn of_equally_long_runs_the_earliest_is_matched_first() {
        // Matching `x` first leaves `z` to be found after it; matching `y` leaves nothing.
        let similarity = ratio(&["x", "z", "y"], &["y", "x", "q", "z"]);
        assert!((similarity - 4.0 / 7.0).abs() < 1e-9, "{similarity}");
    }

    #[test]
    fn disjoint_sequences_are_zero() {
        assert_eq!(ratio(&["a", "b"], &["c", "d"]), 0.0);
    }

    #[test]
    fn empty_sequences_are_one() {
        assert_eq!(ratio(&[], &[]), 1.0);
    }

    #[test]
    fn matches_known_python_values() {
        // Verified against CPython: SequenceMatcher(None, a, b).ratio()
        assert_eq!(ratio(&["a", "b", "c", "d"], &["a", "b", "x", "d"]), 0.75);
        assert_eq!(ratio(&["a", "b", "c"], &["a", "c"]), 0.8);
        assert_eq!(
            ratio(&["q", "a", "b", "x", "c", "d"], &["a", "b", "y", "c", "d"]),
            8.0 / 11.0
        );
    }

    /// Two sequences sharing one very common element and differing elsewhere.
    fn autojunk_pair(n: usize) -> (Vec<String>, Vec<String>) {
        let a = (0..n)
            .map(|i| {
                if i % 3 != 0 {
                    "common".to_string()
                } else {
                    format!("u{i}")
                }
            })
            .collect();
        let b = (0..n)
            .map(|i| {
                if i % 3 != 0 {
                    "common".to_string()
                } else {
                    format!("v{i}")
                }
            })
            .collect();
        (a, b)
    }

    #[test]
    fn autojunk_changes_the_answer_exactly_as_python_does() {
        // Verified against CPython. Below 200 elements the shared element still
        // matches; at 240 it is purged as popular and the ratio collapses to zero.
        for (n, expected) in [(150usize, 0.666_666_666_666_666_6), (240usize, 0.0)] {
            let (a, b) = autojunk_pair(n);
            let a_refs: Vec<&str> = a.iter().map(String::as_str).collect();
            let b_refs: Vec<&str> = b.iter().map(String::as_str).collect();
            assert_eq!(ratio(&a_refs, &b_refs), expected, "n = {n}");
        }
    }

    #[test]
    fn identical_sequences_still_match_despite_autojunk() {
        // Purging every element leaves bestsize 0, but the equal-element extension
        // loop then walks the whole span, so self-comparison stays 1.0. CPython
        // returns exactly 1.0 here too.
        let long: Vec<&str> = vec!["common"; 400];
        assert_eq!(ratio(&long, &long), 1.0);
    }
}
