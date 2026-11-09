//! Sound write-scope containment over the Workspace path language.
//!
//! [`WriteScopePort`](crate::scheduler::WriteScopePort) answers overlap only,
//! which cannot prove that a requested pattern stays inside an owner's paths.
//! This module proves containment `allowed ⊆ owned` structurally, using the
//! real Workspace grammar for validity (`WritePathPattern::parse`): `/`
//! separates components, a whole component `**` matches zero or more
//! components and `*` inside a component matches any run of scalars.
//!
//! The proof is sufficient, never necessary: a union is contained only when
//! one owned pattern contains it, and anything the proof cannot show is
//! reported as not contained. Callers refuse in that case.

use receipts_workspace_execution::WritePathPattern;

/// Per-pattern bound; longer patterns are undecided, not admitted.
// ponytail: quadratic DP per component pair, so bound the input; raise with a real consumer.
const MAX_PATTERN_BYTES: usize = 1024;

/// What the structural proof established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Containment {
    Contained,
    /// Not shown to be inside; the pattern may reach outside the owner.
    NotProven,
    /// A pattern is invalid in the path language or beyond the bound.
    Undecided,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Part<'a> {
    GlobStar,
    Normal(&'a str),
}

fn parts(pattern: &str) -> Option<Vec<Part<'_>>> {
    if pattern.len() > MAX_PATTERN_BYTES || WritePathPattern::parse(pattern).is_err() {
        return None;
    }
    Some(
        pattern
            .split('/')
            .map(|c| {
                if c == "**" {
                    Part::GlobStar
                } else {
                    Part::Normal(c)
                }
            })
            .collect(),
    )
}

/// Whether `allowed` lies inside the union of `owned`.
pub(crate) fn contained_in(allowed: &str, owned: &[String]) -> Containment {
    let Some(allowed) = parts(allowed) else {
        return Containment::Undecided;
    };
    let mut proven = false;
    for pattern in owned {
        let Some(owner) = parts(pattern) else {
            return Containment::Undecided;
        };
        proven |= path_contained(&allowed, &owner);
    }
    if proven {
        Containment::Contained
    } else {
        Containment::NotProven
    }
}

/// `allowed ⊆ owner` over component sequences. `g[i][j]`: the suffix
/// `allowed[i..]` is inside the suffix `owner[j..]`.
fn path_contained(allowed: &[Part<'_>], owner: &[Part<'_>]) -> bool {
    let (n, m) = (allowed.len(), owner.len());
    let mut g = vec![vec![false; m + 1]; n + 1];
    g[n][m] = true;
    for i in (0..=n).rev() {
        for j in (0..m).rev() {
            g[i][j] = match owner[j] {
                // `**` takes none of the allowed components, or swallows one
                // (whatever it is, including an allowed `**`) and stays.
                Part::GlobStar => g[i][j + 1] || (i < n && g[i + 1][j]),
                Part::Normal(tokens) => {
                    i < n
                        && matches!(allowed[i], Part::Normal(a) if component_contained(a, tokens))
                        && g[i + 1][j + 1]
                }
            };
        }
    }
    g[0][0]
}

/// `a ⊆ b` for two components. The allowed `*` is an opaque symbol that only
/// a `*` of `b` can absorb, so every way of filling it stays inside `b`.
fn component_contained(a: &str, b: &str) -> bool {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let (p, q) = (a.len(), b.len());
    let mut g = vec![vec![false; q + 1]; p + 1];
    g[p][q] = true;
    for i in (0..=p).rev() {
        for j in (0..q).rev() {
            g[i][j] = if b[j] == '*' {
                g[i][j + 1] || (i < p && g[i + 1][j])
            } else {
                i < p && a[i] == b[j] && a[i] != '*' && g[i + 1][j + 1]
            };
        }
    }
    g[0][0]
}

#[cfg(test)]
#[path = "containment_tests.rs"]
mod tests;
