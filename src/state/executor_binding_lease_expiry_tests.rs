use crate::error::StateError;
use crate::event::{ActorKind, EventActor, EventEnvelope, EventSubject, EventType, SubjectKind};
use crate::executor_binding::{ExecutorBinding, ReleaseReason};
use crate::executor_binding_lease_expiry::{
    ExecutorLeaseExpiryOutcomeV1, ExecutorLeaseExpiryRequestV2,
    ExecutorReleasedLifecycleAuthorityV1, expected_provenance,
};
use crate::logical_role::{LogicalRole, LogicalRoleStatus, LogicalRoleType};
use crate::repository::SqliteStateRepository;
use crate::tests::{FakeTrustedClock, TempDir, trusted_clock_at};

const PROJECT: &str = "project-lease";
const ROLE: &str = "role-lease";
pub(crate) const BINDING: &str = "binding-lease";
pub(crate) const DEADLINE: &str = "2026-08-18T10:00:00.000000000Z";
const EVENT_ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

pub(crate) fn seeded(
    tag: &str,
    deadline: &str,
) -> (TempDir, SqliteStateRepository, ExecutorBinding) {
    let tmp = TempDir::new(tag);
    let mut repo = SqliteStateRepository::open(tmp.db_path()).expect("open");
    repo.create_logical_role(role(ROLE)).expect("role");
    let binding = make_binding(BINDING, ROLE, deadline);
    repo.create_executor_binding(binding.clone())
        .expect("binding");
    (tmp, repo, binding)
}

pub(crate) fn request(
    binding: &ExecutorBinding,
    at: &str,
    event_id: &str,
) -> ExecutorLeaseExpiryRequestV2 {
    let actor = EventActor {
        kind: ActorKind::System,
        id: Some("orchestration-core".to_string()),
    };
    ExecutorLeaseExpiryRequestV2 {
        binding_id: binding.binding_id.clone(),
        lifecycle_authority: ExecutorReleasedLifecycleAuthorityV1 {
            actor: actor.clone(),
            correlation_id: "corr-lease".to_string(),
        },
        executor_released_event: EventEnvelope {
            event_id: event_id.to_string(),
            project_id: PROJECT.to_string(),
            goal_id: None,
            event_type: EventType::ExecutorReleased,
            actor,
            subject: EventSubject {
                kind: SubjectKind::Role,
                id: binding.role_id.clone(),
            },
            occurred_at: at.to_string(),
            payload: expected_provenance(binding, PROJECT, at).expect("provenance"),
            correlation_id: "corr-lease".to_string(),
            epoch: 0,
        },
    }
}

pub(crate) fn clock(at: &str) -> FakeTrustedClock {
    trusted_clock_at(at)
}

fn role(role_id: &str) -> LogicalRole {
    LogicalRole {
        role_id: role_id.to_string(),
        project_id: PROJECT.to_string(),
        role_type: LogicalRoleType::RuntimeA2,
        status: LogicalRoleStatus::Active,
        current_context_epoch: 0,
        name: None,
        workstream_id: None,
        ownership_paths: vec![],
        integration_branch: None,
        context_manifest_id: None,
        active_binding_id: None,
        created_at: None,
    }
}

fn make_binding(binding_id: &str, role_id: &str, deadline: &str) -> ExecutorBinding {
    ExecutorBinding {
        binding_id: binding_id.to_string(),
        role_id: role_id.to_string(),
        provider_id: "provider-a".to_string(),
        model_id: "model-a".to_string(),
        runtime_id: "runtime-a".to_string(),
        session_ref: None,
        routing_decision_id: None,
        bound_at: "2026-08-18T09:00:00.000000000Z".to_string(),
        lease_expires_at: deadline.to_string(),
        released_at: None,
        release_reason: None,
        rehydration_completed: None,
    }
}

fn assert_unreleased(repo: &SqliteStateRepository, event_id: &str) {
    let binding = repo
        .find_executor_binding(BINDING)
        .expect("find binding")
        .expect("binding present");
    assert_eq!((binding.released_at, binding.release_reason), (None, None));
    assert!(repo.find_event(event_id).expect("find event").is_none());
}

#[test]
fn expiry_predicate_release_atomicity_idempotency_and_rebind_contract() {
    let cases = [
        (
            "expiry-before",
            "2026-08-18T09:59:59.999999999Z",
            ExecutorLeaseExpiryOutcomeV1::NotExpired,
        ),
        (
            "expiry-equal",
            DEADLINE,
            ExecutorLeaseExpiryOutcomeV1::Released,
        ),
        (
            "expiry-after",
            "2026-08-18T10:00:00.000000001Z",
            ExecutorLeaseExpiryOutcomeV1::Released,
        ),
    ];
    for (tag, at, expected) in cases {
        let (_tmp, mut repo, original) = seeded(tag, DEADLINE);
        let outcome = repo
            .expire_executor_binding_lease(&clock(at), request(&original, at, EVENT_ID))
            .expect("expiry decision");
        assert_eq!(outcome, expected);
        if expected == ExecutorLeaseExpiryOutcomeV1::NotExpired {
            assert_unreleased(&repo, EVENT_ID);
            let conflict = repo
                .create_executor_binding(make_binding("successor", ROLE, DEADLINE))
                .expect_err("past-looking/unreleased binding still blocks");
            assert!(matches!(
                conflict,
                StateError::ExecutorBindingUnreleasedConflict { .. }
            ));
        } else {
            let released = repo
                .find_executor_binding(BINDING)
                .expect("find")
                .expect("present");
            assert_eq!(released.released_at.as_deref(), Some(at));
            assert_eq!(released.release_reason, Some(ReleaseReason::LeaseExpired));
            assert_eq!(released.provider_id, original.provider_id);
            assert_eq!(released.model_id, original.model_id);
            assert_eq!(released.runtime_id, original.runtime_id);
            assert_eq!(
                repo.find_event(EVENT_ID).expect("event"),
                Some(request(&original, at, EVENT_ID).executor_released_event)
            );
            assert_eq!(
                repo.expire_executor_binding_lease(
                    &clock(at),
                    request(&original, at, "01ARZ3NDEKTSV4RRFFQ69G5FAW")
                )
                .expect("idempotent"),
                ExecutorLeaseExpiryOutcomeV1::AlreadyLeaseExpired
            );
            assert_eq!(repo.count_table_rows("event").expect("count"), 1);
            repo.create_executor_binding(make_binding("successor", ROLE, DEADLINE))
                .expect("later separate rebind may now occur");
        }
    }
}

#[test]
fn amended_reference_is_exact_canonical_and_forbids_identity_components() {
    let (_tmp, mut repo, binding) = seeded("expiry-reference", DEADLINE);
    let at = DEADLINE;
    let accepted = request(&binding, at, EVENT_ID);
    assert_eq!(
        accepted.executor_released_event.payload.reference,
        r#"{"binding_id":"binding-lease","project_id":"project-lease","v":1}"#
    );
    let invalid = [
        r#"{"binding_id":"binding-lease","executor_id":"x","project_id":"project-lease","v":1}"#,
        r#"{"binding_id":"binding-lease","project_id":"project-lease","provider_id":"x","v":1}"#,
        r#"{"binding_id":"binding-lease","model_id":"x","project_id":"project-lease","v":1}"#,
        r#"{"binding_id":"binding-lease","project_id":"project-lease","runtime_id":"x","v":1}"#,
        r#"{"binding_id":"binding-lease","extra":true,"project_id":"project-lease","v":1}"#,
        r#"{"project_id":"project-lease","v":1}"#,
        r#"{"binding_id":"binding-lease","v":1}"#,
        r#"{"binding_id":"binding-lease","project_id":"project-lease","v":2}"#,
        r#"{"binding_id":"binding-lease","project_id":"wrong-project","v":1}"#,
        r#"{ "binding_id":"binding-lease","project_id":"project-lease","v":1}"#,
        r#"{"project_id":"project-lease","binding_id":"binding-lease","v":1}"#,
        "urn:executor:binding-lease",
        "executor://binding-lease",
    ];
    for (index, reference) in invalid.into_iter().enumerate() {
        let mut candidate = accepted.clone();
        let mut event_id = EVENT_ID.to_string();
        event_id.pop();
        event_id.push(char::from(b"0123456789ABCDEFGHJKMNPQ"[index]));
        candidate.executor_released_event.event_id = event_id;
        candidate.executor_released_event.payload.reference = reference.to_string();
        let result = repo.expire_executor_binding_lease(&clock(at), candidate);
        assert!(
            matches!(
                result,
                Err(StateError::ExecutorReleasedProvenanceInvalid { .. })
            ),
            "reference case {index} returned {result:?}"
        );
        assert_eq!(
            repo.find_executor_binding(BINDING)
                .expect("find")
                .expect("present")
                .release_reason,
            None
        );
    }
}

#[test]
fn bound_identity_digest_is_local_and_every_component_is_binding_qualified() {
    let (_tmp, mut repo, binding) = seeded("expiry-digest", DEADLINE);
    let valid = request(&binding, DEADLINE, EVENT_ID);
    assert!(
        valid
            .executor_released_event
            .payload
            .digest
            .starts_with("sha256:executor-released-provenance:v1:")
    );
    assert_eq!(
        valid.executor_released_event.payload.digest,
        "sha256:executor-released-provenance:v1:7bd53470d12fbd4a28bad49778d4d84226241631d235ff25351e0df2c5fe729c"
    );
    for digest in [
        "malformed",
        "sha256:executor-released-provenance:v1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "sha256:executor-released-provenance:v2:7bd53470d12fbd4a28bad49778d4d84226241631d235ff25351e0df2c5fe729c",
    ] {
        let mut candidate = valid.clone();
        candidate.executor_released_event.payload.digest = digest.to_string();
        assert!(
            repo.expire_executor_binding_lease(&clock(DEADLINE), candidate)
                .is_err()
        );
    }
    for field in [
        "provider",
        "model",
        "runtime",
        "role",
        "released_at",
        "reason",
    ] {
        let mut altered = binding.clone();
        let altered_at = if field == "released_at" {
            "2026-08-18T10:00:00.000000001Z"
        } else {
            DEADLINE
        };
        match field {
            "provider" => altered.provider_id = "wrong-provider".to_string(),
            "model" => altered.model_id = "wrong-model".to_string(),
            "runtime" => altered.runtime_id = "wrong-runtime".to_string(),
            "role" => altered.role_id = "wrong-role".to_string(),
            "reason" => {
                let mut candidate = valid.clone();
                candidate.executor_released_event.payload.digest =
                    "sha256:executor-released-provenance:v1:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string();
                assert!(
                    repo.expire_executor_binding_lease(&clock(DEADLINE), candidate)
                        .is_err()
                );
                continue;
            }
            _ => {}
        }
        let mut candidate = valid.clone();
        candidate.executor_released_event.payload.digest =
            expected_provenance(&altered, PROJECT, altered_at)
                .expect("altered digest")
                .digest;
        assert!(
            repo.expire_executor_binding_lease(&clock(DEADLINE), candidate)
                .is_err()
        );
    }

    repo.create_logical_role(role("role-same-tuple"))
        .expect("role2");
    let same_tuple = make_binding("binding-other", "role-same-tuple", DEADLINE);
    repo.create_executor_binding(same_tuple.clone())
        .expect("binding2");
    let mut wrong_binding = valid.clone();
    wrong_binding.executor_released_event.payload =
        expected_provenance(&same_tuple, PROJECT, DEADLINE).expect("other provenance");
    assert!(
        repo.expire_executor_binding_lease(&clock(DEADLINE), wrong_binding)
            .is_err()
    );
}

#[test]
fn event_subject_actor_correlation_project_type_time_and_baseline_are_exact() {
    let (_tmp, mut repo, binding) = seeded("expiry-coherence", DEADLINE);
    for mutation in 0..9 {
        let mut candidate = request(&binding, DEADLINE, EVENT_ID);
        match mutation {
            0 => candidate.executor_released_event.project_id = "wrong-project".to_string(),
            1 => candidate.executor_released_event.subject.kind = SubjectKind::Task,
            2 => candidate.executor_released_event.subject.id = "wrong-role".to_string(),
            3 => candidate.executor_released_event.actor.kind = ActorKind::User,
            4 => candidate.executor_released_event.actor.id = Some("wrong-actor".to_string()),
            5 => candidate.executor_released_event.actor.id = None,
            6 => candidate.executor_released_event.correlation_id = "wrong-corr".to_string(),
            7 => {
                candidate.executor_released_event.occurred_at =
                    "2026-08-18T10:00:00.000000001Z".to_string()
            }
            8 => candidate.executor_released_event.event_type = EventType::ExecutorBound,
            _ => unreachable!(),
        }
        assert!(
            repo.expire_executor_binding_lease(&clock(DEADLINE), candidate)
                .is_err()
        );
    }
    let mut empty_authority = request(&binding, DEADLINE, EVENT_ID);
    empty_authority.lifecycle_authority.correlation_id.clear();
    empty_authority
        .executor_released_event
        .correlation_id
        .clear();
    assert!(
        repo.expire_executor_binding_lease(&clock(DEADLINE), empty_authority)
            .is_err()
    );
    let mut invalid_envelope = request(&binding, DEADLINE, EVENT_ID);
    invalid_envelope.executor_released_event.event_id = "bad".to_string();
    assert!(matches!(
        repo.expire_executor_binding_lease(&clock(DEADLINE), invalid_envelope),
        Err(StateError::EventValidation { .. })
    ));
    let mut wrong_target = request(&binding, DEADLINE, EVENT_ID);
    wrong_target.binding_id = "missing-binding".to_string();
    assert!(matches!(
        repo.expire_executor_binding_lease(&clock(DEADLINE), wrong_target),
        Err(StateError::ExecutorBindingNotFound { .. })
    ));
}

#[test]
fn event_insert_failures_roll_back_release_but_keep_phase_a_watermark() {
    let (_tmp, mut repo, binding) = seeded("expiry-event-failure", DEADLINE);
    let duplicate = request(&binding, DEADLINE, EVENT_ID).executor_released_event;
    repo.append_event(duplicate.clone())
        .expect("historical duplicate id");
    assert!(matches!(
        repo.expire_executor_binding_lease(&clock(DEADLINE), request(&binding, DEADLINE, EVENT_ID)),
        Err(StateError::EventAlreadyExists { .. })
    ));
    assert_eq!(
        repo.find_executor_binding(BINDING)
            .expect("find")
            .expect("present")
            .release_reason,
        None
    );
    assert!(
        repo.find_trusted_time_watermark(PROJECT)
            .expect("watermark")
            .is_some()
    );

    let (_tmp2, mut repo, binding) = seeded("expiry-trigger-failure", DEADLINE);
    repo.run_transaction(|uow| {
        uow.execute_batch(
            "CREATE TRIGGER fail_executor_released BEFORE INSERT ON event BEGIN SELECT RAISE(FAIL, 'forced event insertion failure'); END;",
        )
    })
    .expect("trigger");
    assert!(matches!(
        repo.expire_executor_binding_lease(&clock(DEADLINE), request(&binding, DEADLINE, EVENT_ID)),
        Err(StateError::EventWriteFailed { .. })
    ));
    assert_unreleased(&repo, EVENT_ID);
    assert!(
        repo.find_trusted_time_watermark(PROJECT)
            .expect("watermark")
            .is_some()
    );
}

#[test]
fn expiry_never_rewrites_an_existing_non_expiry_release() {
    let (_tmp, mut repo, binding) = seeded("expiry-other-release", DEADLINE);
    let released_at = "2026-08-18T09:30:00.000000000Z";
    repo.release_executor_binding(BINDING, released_at, ReleaseReason::Completed)
        .expect("existing release");
    assert!(matches!(
        repo.expire_executor_binding_lease(&clock(DEADLINE), request(&binding, DEADLINE, EVENT_ID)),
        Err(StateError::ExecutorBindingAlreadyReleased { .. })
    ));
    let found = repo
        .find_executor_binding(BINDING)
        .expect("find")
        .expect("present");
    assert_eq!(found.released_at.as_deref(), Some(released_at));
    assert_eq!(found.release_reason, Some(ReleaseReason::Completed));
    assert!(repo.find_event(EVENT_ID).expect("event").is_none());
}

#[test]
fn schema_and_historical_boundaries_add_only_the_watermark() {
    let (_tmp, mut repo, binding) = seeded("expiry-schema", DEADLINE);
    assert_eq!(
        repo.table_columns("executor_binding").expect("columns"),
        [
            "binding_id",
            "role_id",
            "provider_id",
            "model_id",
            "runtime_id",
            "session_ref",
            "routing_decision_id",
            "bound_at",
            "lease_expires_at",
            "released_at",
            "release_reason",
            "rehydration_completed"
        ]
    );
    let indexes = repo
        .sqlite_master_entries("index", "executor_binding")
        .expect("indexes");
    assert!(indexes.iter().all(|(_, sql)| !sql.contains("provider_id")
        && !sql.contains("model_id")
        && !sql.contains("runtime_id")));
    let mut historical = request(&binding, DEADLINE, EVENT_ID).executor_released_event;
    historical.payload.reference = "historical-reference".to_string();
    historical.payload.digest = "historical-digest".to_string();
    repo.append_event(historical.clone())
        .expect("historical append");
    assert_eq!(repo.find_event(EVENT_ID).expect("read"), Some(historical));
}
