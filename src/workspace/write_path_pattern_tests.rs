use super::*;

fn set(patterns: &[&str]) -> WritePathSet {
    WritePathSet::new(
        patterns
            .iter()
            .map(|p| WritePathPattern::parse(p).unwrap())
            .collect(),
    )
}

fn overlap(left: &[&str], right: &[&str], expected: bool) {
    let (left, right) = (set(left), set(right));
    let witness = intersects(&left, &right).unwrap();
    assert_eq!(witness.is_some(), expected, "{left:?} / {right:?}");
    if let Some(path) = witness {
        validate_write_path(&path).unwrap();
        assert!(left.matches(&path) && right.matches(&path), "{path:?}");
        assert!(left.patterns().iter().any(|p| p.matches(&path)));
        assert!(right.patterns().iter().any(|p| p.matches(&path)));
    }
}

#[test]
fn intersections_cover_globstars_multistars_sets_and_unicode() {
    for (left, right, expected) in [
        ("src/**/foo", "src/foo", true),
        ("**/**", "*", true),
        ("**/**", "**/**", true),
        ("a*b*c", "ab*bc", true),
        ("*a*b", "a*b*", true),
        ("a*b", "a*c", false),
        ("src/*.rs", "docs/**", false),
        ("src/*", "src/a/b", false),
        ("a/**/b/**/c", "**/a/b/c/**", true),
        ("é/猫*.rs", "é/*🦀.rs", true),
        ("É", "é", false),
        ("é", "e\u{301}", false),
        (".*/**", "**/.*", true),
        (".*", "*.", true),
        ("a\0b", "a*b", true),
    ] {
        overlap(&[left], &[right], expected);
        overlap(&[right], &[left], expected);
    }
    overlap(&["other", "src/a*b*c"], &["docs/**", "src/ab*bc"], true);
    overlap(&["a", "b/**"], &["c", "d/**"], false);
    overlap(&[], &["**"], false);
    overlap(&["**"], &[], false);
    overlap(&[], &[], false);
    assert_eq!(
        intersects(&set(&["src/**/foo"]), &set(&["src/foo"])).unwrap(),
        Some("src/foo".into())
    );
}

#[test]
fn public_candidates_are_structural_and_patterns_keep_exact_errors() {
    let all = set(&["**/**"]);
    for path in ["", "/a", "a/", "a//b", ".", "..", "a/./b", "a/../b", "a\\b"] {
        assert!(validate_write_path(path).is_err());
        assert!(!all.matches(path));
        assert!(!all.patterns()[0].matches(path));
    }
    for path in ["?[]{}!*", "!a", "***", "a\0b", "...", ".hidden", "a\nb"] {
        assert!(all.matches(path), "{path:?}");
    }
    for (pattern, reason) in [
        ("", "empty path"),
        ("a\\?", "backslash is forbidden"),
        ("!a//b", "empty component"),
        ("!a?", "leading negation is unsupported"),
        ("a**?", "unsupported pattern syntax"),
        ("a**", "globstar must occupy a complete component"),
    ] {
        assert_eq!(WritePathPattern::parse(pattern).unwrap_err(), reason);
    }
}

#[test]
fn bounded_refusal_is_not_disjoint_and_does_not_limit_public_matching() {
    let long = "a*".repeat(512);
    let left = set(&[&format!("{long}b")]);
    let right = set(&[&format!("{long}c")]);
    assert_eq!(
        intersects(&left, &right),
        Err(WritePathIntersectionError::ResourceLimit)
    );
    let huge = "a".repeat(INTERSECTION_WORK_LIMIT);
    let large = set(&[&huge]);
    assert!(set(&["*"]).matches(&huge));
    assert_eq!(
        intersects(&large, &large),
        Err(WritePathIntersectionError::ResourceLimit)
    );
    // Empty union is a proof even when the other side exceeds the search budget.
    assert_eq!(intersects(&large, &set(&[])), Ok(None));
    assert_eq!(intersects(&set(&["a"]), &set(&["b"])), Ok(None));
    // A refused pair cannot be skipped to claim disjointness for the union.
    let many = WritePathSet::new(vec![
        WritePathPattern::parse("z").unwrap(),
        left.patterns()[0].clone(),
    ]);
    assert_eq!(
        intersects(&many, &right),
        Err(WritePathIntersectionError::ResourceLimit)
    );
}

#[test]
fn exhaustive_small_component_language_agrees_with_independent_enumeration() {
    // For these tokens a shortest witness has at most four scalars; x stands
    // for every nonliteral scalar. Enumerate independently of product search.
    let patterns = [
        "a", "b", "*", "a*", "*b", "a*b", "*a*b", "*b*a", ".*", "*.", ".*.",
    ];
    let mut candidates = vec![String::new()];
    let mut frontier = vec![String::new()];
    for _ in 0..4 {
        frontier = frontier
            .iter()
            .flat_map(|s| ['a', 'b', '.', 'x'].map(|c| format!("{s}{c}")))
            .collect();
        candidates.extend(frontier.iter().cloned());
    }
    for a in patterns {
        for b in patterns {
            let (left, right) = (set(&[a]), set(&[b]));
            let expected = candidates
                .iter()
                .any(|p| left.matches(p) && right.matches(p));
            overlap(&[a], &[b], expected);
        }
    }
}

#[test]
fn exhaustive_small_path_language_agrees_with_enumeration() {
    // Two components per pattern => at most four in a shortest product path.
    // All normal component constraints here have a witness among these six.
    let components = ["a", "b", "*", "a*", "*b", "**"];
    let mut patterns: Vec<String> = components.iter().map(|s| (*s).into()).collect();
    for a in components {
        for b in components {
            patterns.push(format!("{a}/{b}"));
        }
    }
    let mut candidates = Vec::new();
    let mut frontier = vec![String::new()];
    for _ in 0..4 {
        frontier = frontier
            .iter()
            .flat_map(|s| {
                ["a", "b", "aa", "ab", "ba", "bb"].map(|c| {
                    if s.is_empty() {
                        c.to_owned()
                    } else {
                        format!("{s}/{c}")
                    }
                })
            })
            .collect();
        candidates.extend(frontier.iter().cloned());
    }
    let sets: Vec<_> = patterns.iter().map(|p| set(&[p])).collect();
    let matches: Vec<Vec<bool>> = sets
        .iter()
        .map(|s| candidates.iter().map(|p| s.matches(p)).collect())
        .collect();
    for (i, left) in sets.iter().enumerate() {
        for (j, right) in sets.iter().enumerate() {
            let expected = matches[i].iter().zip(&matches[j]).any(|(a, b)| *a && *b);
            let witness = intersects(left, right).unwrap();
            assert_eq!(
                witness.is_some(),
                expected,
                "{} / {}",
                patterns[i],
                patterns[j]
            );
            if let Some(p) = witness {
                assert!(left.matches(&p) && right.matches(&p));
            }
        }
    }
}
