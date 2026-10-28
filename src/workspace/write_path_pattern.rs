//! A0 path language: case-sensitive Unicode scalars, no normalization.
//! Candidates use structural validation, not pattern syntax. Language overlap
//! says nothing about filesystem reachability (including literal NUL scalars).

use std::collections::{HashMap, VecDeque};

pub fn validate_write_path(value: &str) -> Result<(), &'static str> {
    if value.is_empty() {
        return Err("empty path");
    }
    if value.contains('\\') {
        return Err("backslash is forbidden");
    }
    for component in value.split('/') {
        if component.is_empty() {
            return Err("empty component");
        }
        if component == "." || component == ".." {
            return Err("dot component");
        }
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WritePathPattern {
    components: Vec<Component>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
enum Component {
    GlobStar,
    Normal(Vec<char>),
}

impl WritePathPattern {
    pub fn parse(pattern: &str) -> Result<Self, &'static str> {
        validate_write_path(pattern)?;
        if pattern.starts_with('!') {
            return Err("leading negation is unsupported");
        }
        if pattern.contains(['?', '[', ']', '{', '}']) {
            return Err("unsupported pattern syntax");
        }
        let mut components = Vec::new();
        for component in pattern.split('/') {
            components.push(if component == "**" {
                Component::GlobStar
            } else {
                if component.contains("**") {
                    return Err("globstar must occupy a complete component");
                }
                Component::Normal(component.chars().collect())
            });
        }
        Ok(Self { components })
    }

    pub fn matches(&self, path: &str) -> bool {
        if validate_write_path(path).is_err() {
            return false;
        }
        let parts: Vec<&str> = path.split('/').collect();
        // Each row represents one pattern prefix against every path prefix.
        // O(pattern components * path components), plus normal-component DP.
        let mut row = vec![false; parts.len() + 1];
        row[0] = true;
        for component in &self.components {
            let mut next = vec![false; row.len()];
            match component {
                Component::GlobStar => {
                    next[0] = row[0];
                    for j in 1..row.len() {
                        next[j] = row[j] || next[j - 1];
                    }
                }
                Component::Normal(tokens) => {
                    for j in 1..row.len() {
                        next[j] = row[j - 1] && match_component(tokens, parts[j - 1]);
                    }
                }
            }
            row = next;
        }
        row[parts.len()]
    }
}

pub(crate) fn match_component(tokens: &[char], value: &str) -> bool {
    let scalars: Vec<char> = value.chars().collect();
    // O(pattern scalars * candidate scalars) time, O(candidate scalars) space.
    let mut row = vec![false; scalars.len() + 1];
    row[0] = true;
    for token in tokens {
        let mut next = vec![false; row.len()];
        if *token == '*' {
            next[0] = row[0];
        }
        for j in 1..row.len() {
            next[j] = if *token == '*' {
                row[j] || next[j - 1]
            } else {
                row[j - 1] && *token == scalars[j - 1]
            };
        }
        row = next;
    }
    row[scalars.len()]
}

/// Union of parsed patterns. Empty sets match no paths.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WritePathSet {
    patterns: Vec<WritePathPattern>,
}

impl WritePathSet {
    pub fn new(patterns: Vec<WritePathPattern>) -> Self {
        Self { patterns }
    }

    pub fn patterns(&self) -> &[WritePathPattern] {
        &self.patterns
    }

    pub fn matches(&self, path: &str) -> bool {
        self.patterns.iter().any(|pattern| pattern.matches(path))
    }
}

/// Exhaustion is uncertainty, never evidence that the sets are disjoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WritePathIntersectionError {
    ResourceLimit,
}

impl std::fmt::Display for WritePathIntersectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("write-path intersection resource limit")
    }
}
impl std::error::Error for WritePathIntersectionError {}

// shortcut: at most 100,000 input tokens, pairs, states, edges and emitted bytes per call;
// increase only with measured resource requirements from a real consumer.
const INTERSECTION_WORK_LIMIT: usize = 100_000;

struct Budget(usize);
impl Budget {
    fn spend(&mut self, count: usize) -> Result<(), WritePathIntersectionError> {
        self.0 = self
            .0
            .checked_sub(count)
            .ok_or(WritePathIntersectionError::ResourceLimit)?;
        Ok(())
    }
}

/// Returns a structurally valid common path, proven disjointness (`Ok(None)`),
/// or explicit resource refusal. No pattern accepted by the parser is silently
/// approximated. Limits apply only to intersection, never parsing or matching.
///
/// Each pattern is an epsilon NFA: globstar can advance without consuming a
/// component or loop on any component. Component stars work the same way on
/// Unicode scalars. Product-state reachability is exhaustive; a tiny structural
/// DFA excludes empty, dot and dotdot components. Thus exhausting the finite
/// product proves disjointness; stopping early always returns an error.
pub fn intersects(
    left: &WritePathSet,
    right: &WritePathSet,
) -> Result<Option<String>, WritePathIntersectionError> {
    if left.patterns.is_empty() || right.patterns.is_empty() {
        return Ok(None);
    }
    let mut budget = Budget(INTERSECTION_WORK_LIMIT);
    for pattern in left.patterns.iter().chain(&right.patterns) {
        budget.spend(1)?;
        for component in &pattern.components {
            budget.spend(match component {
                Component::GlobStar => 1,
                Component::Normal(tokens) => tokens.len() + 1,
            })?;
        }
    }
    for a in &left.patterns {
        for b in &right.patterns {
            budget.spend(1)?;
            let witness = search((0, 0, false), &mut budget, |(i, j, nonempty), budget| {
                if i == a.components.len() && j == b.components.len() && nonempty {
                    return Ok(None);
                }
                let mut edges = Vec::new();
                let x = a.components.get(i);
                let y = b.components.get(j);
                if matches!(x, Some(Component::GlobStar)) {
                    edges.push(((i + 1, j, nonempty), String::new()));
                }
                if matches!(y, Some(Component::GlobStar)) {
                    edges.push(((i, j + 1, nonempty), String::new()));
                }
                if let (Some(x), Some(y)) = (x, y)
                    && let Some(component) =
                        component_witness(component_tokens(x), component_tokens(y), budget)?
                {
                    let next_i = i + usize::from(!matches!(x, Component::GlobStar));
                    let next_j = j + usize::from(!matches!(y, Component::GlobStar));
                    let separator = if nonempty { "/" } else { "" };
                    edges.push(((next_i, next_j, true), format!("{separator}{component}")));
                }
                Ok(Some(edges))
            })?;
            if witness.is_some() {
                return Ok(witness);
            }
        }
    }
    Ok(None)
}

fn component_tokens(component: &Component) -> &[char] {
    match component {
        Component::GlobStar => &['*'],
        Component::Normal(tokens) => tokens,
    }
}

fn component_witness(
    a: &[char],
    b: &[char],
    budget: &mut Budget,
) -> Result<Option<String>, WritePathIntersectionError> {
    // Shape: 0 empty, 1 dot, 2 dotdot, 3 any other nonempty component.
    search((0, 0, 0), budget, |(i, j, shape), _| {
        if i == a.len() && j == b.len() && shape == 3 {
            return Ok(None);
        }
        let mut edges = Vec::new();
        let x = a.get(i).copied();
        let y = b.get(j).copied();
        if x == Some('*') {
            edges.push(((i + 1, j, shape), String::new()));
        }
        if y == Some('*') {
            edges.push(((i, j + 1, shape), String::new()));
        }
        if let (Some(x), Some(y)) = (x, y) {
            let scalar = match (x, y) {
                ('*', '*') => Some('x'),
                ('*', c) | (c, '*') => Some(c),
                (x, y) if x == y => Some(x),
                _ => None,
            };
            if let Some(c) = scalar {
                let next_shape = match (shape, c) {
                    (0, '.') => 1,
                    (1, '.') => 2,
                    _ => 3,
                };
                edges.push((
                    (
                        i + usize::from(x != '*'),
                        j + usize::from(y != '*'),
                        next_shape,
                    ),
                    c.to_string(),
                ));
            }
        }
        Ok(Some(edges))
    })
}

// A visited product state needs only one predecessor: future acceptance depends
// on that state, not on which prefix reached it. Parent links avoid prefix copies.
type SearchExpansion<S> = Result<Option<Vec<(S, String)>>, WritePathIntersectionError>;

fn search<S: Copy + Eq + std::hash::Hash>(
    start: S,
    budget: &mut Budget,
    mut expand: impl FnMut(S, &mut Budget) -> SearchExpansion<S>,
) -> Result<Option<String>, WritePathIntersectionError> {
    budget.spend(1)?;
    let mut seen = HashMap::from([(start, 0)]);
    let mut nodes = vec![(start, None, String::new())];
    let mut queue = VecDeque::from([0]);
    while let Some(index) = queue.pop_front() {
        budget.spend(1)?;
        let Some(edges) = expand(nodes[index].0, budget)? else {
            let mut parts = Vec::new();
            let mut current = index;
            while let Some(parent) = nodes[current].1 {
                parts.push(nodes[current].2.as_str());
                current = parent;
            }
            parts.reverse();
            return Ok(Some(parts.concat()));
        };
        for (state, text) in edges {
            budget.spend(1 + text.len())?;
            if let std::collections::hash_map::Entry::Vacant(entry) = seen.entry(state) {
                entry.insert(nodes.len());
                queue.push_back(nodes.len());
                nodes.push((state, Some(index), text));
            }
        }
    }
    Ok(None)
}

#[cfg(test)]
#[path = "write_path_pattern_tests.rs"]
mod tests;
