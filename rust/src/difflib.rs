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

    fn find_longest_match(
        &self,
        alo: usize,
        ahi: usize,
        blo: usize,
        bhi: usize,
    ) -> Match {
        let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0usize);
        let mut j2len: HashMap<usize, usize> = HashMap::new();

        for i in alo..ahi {
            let mut newj2len: HashMap<usize, usize> = HashMap::new();
            if let Some(indices) = self.b2j.get(self.a[i]) {
                for &j in indices {
                    if j < blo {
                        continue;
                    }
                    if j >= bhi {
                        break;
                    }
                    let previous =
                        if j == 0 { 0 } else { j2len.get(&(j - 1)).copied().unwrap_or(0) };
                    let k = previous + 1;
                    newj2len.insert(j, k);
                    if k > bestsize {
                        besti = i + 1 - k;
                        bestj = j + 1 - k;
                        bestsize = k;
                    }
                }
            }
            j2len = newj2len;
        }

        // Extend the match with equal elements on either side. With no junk set,
        // these are the only extension loops that apply.
        while besti > alo && bestj > blo && self.a[besti - 1] == self.b[bestj - 1] {
            besti -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while besti + bestsize < ahi
            && bestj + bestsize < bhi
            && self.a[besti + bestsize] == self.b[bestj + bestsize]
        {
            bestsize += 1;
        }

        Match { a_start: besti, b_start: bestj, size: bestsize }
    }

    /// Total number of matched elements across all matching blocks.
    fn total_matches(&self) -> usize {
        let mut queue = vec![(0usize, self.a.len(), 0usize, self.b.len())];
        let mut total = 0usize;
        while let Some((alo, ahi, blo, bhi)) = queue.pop() {
            let found = self.find_longest_match(alo, ahi, blo, bhi);
            if found.size == 0 {
                continue;
            }
            total += found.size;
            if alo < found.a_start && blo < found.b_start {
                queue.push((alo, found.a_start, blo, found.b_start));
            }
            if found.a_start + found.size < ahi && found.b_start + found.size < bhi {
                queue.push((
                    found.a_start + found.size,
                    ahi,
                    found.b_start + found.size,
                    bhi,
                ));
            }
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
        assert_eq!(ratio(&["q", "a", "b", "x", "c", "d"], &["a", "b", "y", "c", "d"]), 8.0 / 11.0);
    }

    /// Two sequences sharing one very common element and differing elsewhere.
    fn autojunk_pair(n: usize) -> (Vec<String>, Vec<String>) {
        let a = (0..n)
            .map(|i| if i % 3 != 0 { "common".to_string() } else { format!("u{i}") })
            .collect();
        let b = (0..n)
            .map(|i| if i % 3 != 0 { "common".to_string() } else { format!("v{i}") })
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
