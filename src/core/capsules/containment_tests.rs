//! Containment is checked against the real Workspace matcher: every claimed
//! containment must hold for every path of a bounded exhaustive universe.

use receipts_workspace_execution::WritePathPattern;

use super::*;

fn contained(allowed: &str, owned: &[&str]) -> Containment {
    let owned: Vec<String> = owned.iter().map(|p| p.to_string()).collect();
    contained_in(allowed, &owned)
}

#[test]
fn known_shapes() {
    use Containment::*;
    for (allowed, owner, expected) in [
        ("src/core/scheduler/**", "src/core/**", Contained),
        ("src/core/**", "src/core/**", Contained),
        ("src/core", "src/core/**", Contained),
        ("src/core/x.rs", "src/core/**", Contained),
        ("src/core/*/x.rs", "src/core/**", Contained),
        ("src/core/**/a/**", "src/core/**", Contained),
        ("src/core/a*.rs", "src/core/*.rs", Contained),
        ("src/core/*.rs", "src/core/*.rs", Contained),
        ("src/*/safe/**", "src/core/**", NotProven),
        ("src/co*/x.rs", "src/core/**", NotProven),
        ("src/**/core/x.rs", "src/core/**", NotProven),
        ("src/core/**", "src/core/x.rs", NotProven),
        ("src/core/**", "src/core", NotProven),
        ("src/core/*.rs", "src/core/a*.rs", NotProven),
        ("src/core/x.rs", "docs/**", NotProven),
        ("src/core/sch?duler/**", "src/core/**", Undecided),
        ("src/core/../x", "src/core/**", Undecided),
        ("src/core/x/", "src/core/**", Undecided),
    ] {
        assert_eq!(
            contained(allowed, &[owner]),
            expected,
            "{allowed} in {owner}"
        );
    }
    // A union needs a single member that holds the whole pattern.
    assert_eq!(
        contained("src/a/**", &["docs/**", "src/**"]),
        Containment::Contained
    );
    assert_eq!(
        contained("src/**", &["src/a/**", "src/b/**"]),
        Containment::NotProven
    );
    assert_eq!(
        contained("src/a/**", &["docs/**", "bad?"]),
        Containment::Undecided
    );
    assert_eq!(
        contained(&format!("src/{}", "a".repeat(2000)), &["src/**"]),
        Containment::Undecided
    );
}

#[test]
fn claimed_containment_holds_for_every_path_in_a_bounded_universe() {
    let atoms = ["a", "b", "*", "**", "a*", "*a", "*b*"];
    let mut patterns: Vec<String> = atoms.iter().map(|a| a.to_string()).collect();
    for first in atoms {
        for second in atoms {
            patterns.push(format!("{first}/{second}"));
        }
    }
    let words = ["a", "b", "ab", "ba", "aab"];
    let mut universe: Vec<String> = words.iter().map(|w| w.to_string()).collect();
    for first in words {
        for second in words {
            universe.push(format!("{first}/{second}"));
            for third in words {
                universe.push(format!("{first}/{second}/{third}"));
            }
        }
    }
    let mut proven = 0;
    for allowed in &patterns {
        let allowed_matcher = WritePathPattern::parse(allowed).unwrap();
        for owner in &patterns {
            if contained(allowed, &[owner]) != Containment::Contained {
                continue;
            }
            proven += 1;
            let owner_matcher = WritePathPattern::parse(owner).unwrap();
            for path in &universe {
                assert!(
                    !allowed_matcher.matches(path) || owner_matcher.matches(path),
                    "{allowed} claimed inside {owner} but {path} escapes"
                );
            }
        }
    }
    // The universe is not vacuous: a good number of containments are proven.
    assert!(proven > 100, "{proven}");
}
