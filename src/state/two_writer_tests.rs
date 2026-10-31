//! Two independent connections on one store file race the same compare-and-set
//! decision. Barriers (never sleeps) release both at once; SQLite's write lock
//! serialises them, so exactly one change commits and the other writer gets a
//! typed refusal with no partial history, watermark or binding write. A
//! diagnosed busy failure is also an acceptable loser outcome, but never a
//! second commit. Nothing retries until it passes.

use std::path::Path;
use std::sync::Barrier;
use std::time::Duration;

use crate::context_epoch::{ContextEpoch, ContextEpochTrigger};
use crate::error::StateError;
use crate::executor_binding_lease_expiry_tests::{BINDING, DEADLINE, clock, seeded};
use crate::logical_role::LogicalRoleStatus as S;
use crate::logical_role_lifecycle::RoleChangeRefusal;
use crate::logical_role_lifecycle_tests::{ROLE, binding, evidence, role};
use crate::repository::SqliteStateRepository;
use crate::tests::{TempDir, state_epoch};

const BEFORE: &str = "2026-08-18T09:59:59.999999999Z";
const RENEWED: &str = "2026-08-18T11:00:00.000000000Z";
const RENEWED_2: &str = "2026-08-18T12:00:00.000000000Z";
const ROUNDS: usize = 8;

/// Runs both jobs on separate connections, released together by a barrier.
fn race<T: Send>(
    path: &Path,
    a: impl FnOnce(&mut SqliteStateRepository) -> T + Send,
    b: impl FnOnce(&mut SqliteStateRepository) -> T + Send,
) -> (T, T) {
    let barrier = Barrier::new(2);
    let mut repo_a = SqliteStateRepository::open(path).unwrap();
    let mut repo_b = SqliteStateRepository::open(path).unwrap();
    std::thread::scope(|scope| {
        let ha = scope.spawn(|| {
            barrier.wait();
            a(&mut repo_a)
        });
        let hb = scope.spawn(|| {
            barrier.wait();
            b(&mut repo_b)
        });
        (ha.join().unwrap(), hb.join().unwrap())
    })
}

/// The loser's error is a typed refusal or a diagnosed busy failure.
fn assert_loser(error: &StateError, allowed: impl Fn(&StateError) -> bool) {
    assert!(
        allowed(error) || matches!(error, StateError::TransactionBeginFailed { .. }),
        "unexpected loser error: {error:?}"
    );
}

fn stale(error: &StateError) -> bool {
    matches!(
        error,
        StateError::LogicalRoleChangeRefused {
            reason: RoleChangeRefusal::StaleExpectation,
            ..
        }
    )
}

fn role_repo(tag: &str) -> (TempDir, SqliteStateRepository) {
    let tmp = TempDir::new(tag);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
    repo.create_logical_role(role(ROLE, "p")).unwrap();
    (tmp, repo)
}

#[test]
fn role_status_race_with_same_expected_state_commits_exactly_one_change() {
    for round in 0..ROUNDS {
        let (tmp, repo) = role_repo(&format!("race-status-{round}"));
        drop(repo);
        // Alternate identical and conflicting targets from the same expected state.
        let (to_a, to_b) = if round % 2 == 0 {
            (S::Suspended, S::Suspended)
        } else {
            (S::Suspended, S::Retired)
        };
        let (ra, rb) = race(
            tmp.db_path().as_path(),
            |r| r.transition_logical_role_status(ROLE, S::Active, to_a, &evidence()),
            |r| r.transition_logical_role_status(ROLE, S::Active, to_b, &evidence()),
        );
        let (winner_target, loser) = match (ra, rb) {
            (Ok(()), Err(e)) => (to_a, e),
            (Err(e), Ok(())) => (to_b, e),
            other => panic!("exactly one winner expected: {other:?}"),
        };
        assert_loser(&loser, stale);
        let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
        let history = repo.list_logical_role_history(ROLE).unwrap();
        assert_eq!(history.len(), 1, "one committed history effect");
        assert_eq!(
            (history[0].from_status, history[0].to_status),
            (S::Active, winner_target)
        );
        assert_eq!(
            repo.find_logical_role(ROLE).unwrap().unwrap().status,
            winner_target
        );
    }
}

#[test]
fn binding_pointer_race_with_same_expected_pointer_commits_exactly_one_change() {
    for round in 0..ROUNDS {
        let (tmp, mut repo) = role_repo(&format!("race-binding-{round}"));
        repo.create_executor_binding(binding("b1", ROLE)).unwrap();
        drop(repo);
        let set = |r: &mut SqliteStateRepository| {
            r.set_logical_role_active_binding(ROLE, None, Some("b1"), &evidence())
        };
        let (ra, rb) = race(tmp.db_path().as_path(), set, set);
        let loser = match (ra, rb) {
            (Ok(()), Err(e)) | (Err(e), Ok(())) => e,
            other => panic!("exactly one winner expected: {other:?}"),
        };
        assert_loser(&loser, stale);
        let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
        assert_eq!(repo.list_logical_role_history(ROLE).unwrap().len(), 1);
        assert_eq!(
            repo.find_logical_role(ROLE)
                .unwrap()
                .unwrap()
                .active_binding_id
                .as_deref(),
            Some("b1")
        );
    }
}

#[test]
fn context_race_with_same_expected_epoch_commits_exactly_one_change() {
    for round in 0..ROUNDS {
        let (tmp, mut repo) = role_repo(&format!("race-context-{round}"));
        for epoch in [1, 2] {
            repo.append_context_epoch(ContextEpoch {
                project_id: "p".into(),
                epoch: state_epoch(epoch),
                advanced_at: "t".into(),
                trigger: ContextEpochTrigger::NewWave,
            })
            .unwrap();
        }
        drop(repo);
        let (ra, rb) = race(
            tmp.db_path().as_path(),
            |r| {
                r.set_logical_role_context(
                    ROLE,
                    &state_epoch(0),
                    None,
                    &state_epoch(1),
                    None,
                    &evidence(),
                )
            },
            |r| {
                r.set_logical_role_context(
                    ROLE,
                    &state_epoch(0),
                    None,
                    &state_epoch(2),
                    None,
                    &evidence(),
                )
            },
        );
        let (won, loser) = match (ra, rb) {
            (Ok(()), Err(e)) => (1, e),
            (Err(e), Ok(())) => (2, e),
            other => panic!("exactly one winner expected: {other:?}"),
        };
        assert_loser(&loser, stale);
        let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
        assert_eq!(repo.list_logical_role_history(ROLE).unwrap().len(), 1);
        assert_eq!(
            repo.find_logical_role(ROLE)
                .unwrap()
                .unwrap()
                .current_context_epoch,
            state_epoch(won)
        );
    }
}

fn renewal_refused(error: &StateError) -> bool {
    matches!(error, StateError::ExecutorLeaseRenewalRefused { .. })
}

fn watermark(repo: &SqliteStateRepository) -> Option<crate::TrustedTimeWatermarkV1> {
    repo.find_trusted_time_watermark("project-lease").unwrap()
}

#[test]
fn renewal_race_to_the_same_later_deadline_commits_exactly_one_renewal() {
    for round in 0..ROUNDS {
        let (tmp, repo, original) = seeded(&format!("race-renew-same-{round}"), DEADLINE);
        drop(repo);
        let renew = |r: &mut SqliteStateRepository| {
            r.renew_executor_binding_lease(&clock(BEFORE), BINDING, RENEWED)
        };
        let (ra, rb) = race(tmp.db_path().as_path(), renew, renew);
        let loser = match (ra, rb) {
            (Ok(()), Err(e)) | (Err(e), Ok(())) => e,
            other => panic!("exactly one winner expected: {other:?}"),
        };
        assert_loser(&loser, renewal_refused);
        let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
        let after = repo.find_executor_binding(BINDING).unwrap().unwrap();
        let mut expected = original;
        expected.lease_expires_at = RENEWED.to_string();
        assert_eq!(after, expected, "only the lease changed, once");
        assert_eq!(watermark(&repo).unwrap().last_accepted_trusted_time, BEFORE);
    }
}

#[test]
fn renewals_to_different_later_deadlines_may_both_succeed_but_end_at_the_latest() {
    for round in 0..ROUNDS {
        let (tmp, repo, _) = seeded(&format!("race-renew-ordered-{round}"), DEADLINE);
        drop(repo);
        let (r1, r2) = race(
            tmp.db_path().as_path(),
            |r| r.renew_executor_binding_lease(&clock(BEFORE), BINDING, RENEWED),
            |r| r.renew_executor_binding_lease(&clock(BEFORE), BINDING, RENEWED_2),
        );
        // The later deadline always lands; the earlier one lands only if it ran first.
        assert!(r2.is_ok(), "{r2:?}");
        if let Err(e) = &r1 {
            assert_loser(e, renewal_refused);
        }
        let repo = SqliteStateRepository::open(tmp.db_path()).unwrap();
        assert_eq!(
            repo.find_executor_binding(BINDING)
                .unwrap()
                .unwrap()
                .lease_expires_at,
            RENEWED_2
        );
        assert_eq!(watermark(&repo).unwrap().last_accepted_trusted_time, BEFORE);
    }
}

#[test]
fn sequential_identical_deadline_renewal_is_refused_without_advancing_the_watermark() {
    let (_tmp, mut repo, _) = seeded("renew-identical-sequential", DEADLINE);
    repo.renew_executor_binding_lease(&clock(BEFORE), BINDING, RENEWED)
        .unwrap();
    let before = (
        watermark(&repo),
        repo.find_executor_binding(BINDING).unwrap(),
    );
    // A later, still-eligible sample with the identical deadline: refused, and
    // the refused sample is not accepted into the watermark.
    let later = "2026-08-18T10:30:00.000000000Z";
    for deadline in [RENEWED, DEADLINE] {
        assert!(matches!(
            repo.renew_executor_binding_lease(&clock(later), BINDING, deadline),
            Err(StateError::ExecutorLeaseRenewalRefused { .. })
        ));
        assert_eq!(
            (
                watermark(&repo),
                repo.find_executor_binding(BINDING).unwrap()
            ),
            before
        );
    }
    // A sequentially later deadline is a new renewal and succeeds, advancing time.
    repo.renew_executor_binding_lease(&clock(later), BINDING, RENEWED_2)
        .unwrap();
    assert_eq!(watermark(&repo).unwrap().last_accepted_trusted_time, later);
    assert_eq!(
        repo.find_executor_binding(BINDING)
            .unwrap()
            .unwrap()
            .lease_expires_at,
        RENEWED_2
    );
}

#[test]
fn held_write_lock_gives_a_diagnosed_busy_failure_and_no_partial_write() {
    // Lease renewal.
    let (tmp, mut repo, _) = seeded("busy-renew", DEADLINE);
    let before = (
        watermark(&repo),
        repo.find_executor_binding(BINDING).unwrap(),
    );
    repo.connection().busy_timeout(Duration::ZERO).unwrap();
    let holder = rusqlite::Connection::open(tmp.db_path()).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(matches!(
        repo.renew_executor_binding_lease(&clock(BEFORE), BINDING, RENEWED),
        Err(StateError::TransactionBeginFailed { .. })
    ));
    holder.execute_batch("ROLLBACK").unwrap();
    assert_eq!(
        (
            watermark(&repo),
            repo.find_executor_binding(BINDING).unwrap()
        ),
        before
    );
    repo.renew_executor_binding_lease(&clock(BEFORE), BINDING, RENEWED)
        .unwrap();
    drop(holder);

    // Role compare-and-set.
    let (tmp, mut repo) = role_repo("busy-role");
    repo.connection().busy_timeout(Duration::ZERO).unwrap();
    let holder = rusqlite::Connection::open(tmp.db_path()).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE").unwrap();
    assert!(matches!(
        repo.transition_logical_role_status(ROLE, S::Active, S::Suspended, &evidence()),
        Err(StateError::TransactionBeginFailed { .. })
    ));
    holder.execute_batch("ROLLBACK").unwrap();
    assert!(repo.list_logical_role_history(ROLE).unwrap().is_empty());
    assert_eq!(
        repo.find_logical_role(ROLE).unwrap().unwrap().status,
        S::Active
    );
    repo.transition_logical_role_status(ROLE, S::Active, S::Suspended, &evidence())
        .unwrap();
}
