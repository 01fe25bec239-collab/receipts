//! Heuristic credential-shape detector for capsule text (OR-W2-3).
//!
//! This is a heuristic, not a proof: passing it does not establish that a
//! string carries no secret. The structural rule is the stronger control:
//! no capsule field is a credential slot, so a credential has nowhere
//! legitimate to go. The heuristic catches common accidents: well-known
//! token prefixes, private-key headers, `key=value` secret assignments and
//! long mixed-class high-entropy tokens. Matches are reported by field name
//! only; the value is never echoed.

const PREFIXES: [&str; 19] = [
    "sk-",
    "sk_live_",
    "sk_test_",
    "rk_live_",
    "pk_live_",
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "AKIA",
    "ASIA",
    "AIza",
    "eyJ",
];

const ASSIGNMENTS: [&str; 7] = [
    "password=",
    "passwd=",
    "secret=",
    "api_key=",
    "apikey=",
    "token=",
    "authorization:",
];

/// Minimum suffix after a known prefix for it to look like a real token.
const PREFIX_TAIL: usize = 16;
/// Minimum length and Shannon entropy (bits/char) for an unknown token.
const ENTROPY_LENGTH: usize = 32;
const ENTROPY_BITS: f64 = 4.0;

pub(crate) fn looks_like_credential(value: &str) -> bool {
    if value.contains("-----BEGIN") {
        return true;
    }
    let lower = value.to_ascii_lowercase();
    if ASSIGNMENTS.iter().any(|key| {
        lower
            .match_indices(key)
            .any(|(at, _)| !lower[at + key.len()..].trim_start().is_empty())
    }) {
        return true;
    }
    value
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '=')))
        .any(token)
}

fn token(t: &str) -> bool {
    if PREFIXES
        .iter()
        .any(|p| t.starts_with(p) && t.len() >= p.len() + PREFIX_TAIL)
    {
        return true;
    }
    if t.len() < ENTROPY_LENGTH {
        return false;
    }
    let classes = [
        t.bytes().any(|b| b.is_ascii_lowercase()),
        t.bytes().any(|b| b.is_ascii_uppercase()),
        t.bytes().any(|b| b.is_ascii_digit()),
    ];
    // Hex digests and identifiers use at most two classes.
    if classes.iter().filter(|c| **c).count() < 3 {
        return false;
    }
    let mut counts = [0usize; 256];
    for b in t.bytes() {
        counts[b as usize] += 1;
    }
    let n = t.len() as f64;
    let entropy: f64 = counts
        .iter()
        .filter(|c| **c > 0)
        .map(|c| {
            let p = *c as f64 / n;
            -p * p.log2()
        })
        .sum();
    entropy >= ENTROPY_BITS
}

#[cfg(test)]
mod tests {
    use super::looks_like_credential as hit;

    #[test]
    fn known_shapes_are_flagged_and_ordinary_text_is_not() {
        for secret in [
            "use sk-abcdefghijklmnopqrstuvwx",
            "ghp_0123456789abcdefABCDEF0123",
            "AKIAIOSFODNN7EXAMPLEXXXX",
            "-----BEGIN OPENSSH PRIVATE KEY-----",
            "curl -H Authorization: Bearer x",
            "PASSWORD=hunter2",
            "token Zx9Qm2Lp7Rt4Vn8Kc3Hj6Bw1Yd5Gf0Ae",
        ] {
            assert!(hit(secret), "{secret}");
        }
        for ordinary in [
            "Implement the scheduler; keep tests green.",
            "0123456789abcdef0123456789abcdef01234567",
            "src/core/scheduler/dispatch_scheduler_tests.rs",
            "cargo test --workspace --locked",
            "token budget is 5",
            "sk-short",
            "receipts_orchestration::scheduler::schedule_dispatch_v1",
        ] {
            assert!(!hit(ordinary), "{ordinary}");
        }
    }
}
