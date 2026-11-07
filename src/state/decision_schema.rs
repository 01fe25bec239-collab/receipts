//! Schema-shaped admission tables for the eight persisted decision families.
//!
//! Each [`Obj`] mirrors one owner-frozen JSON Schema (`additionalProperties`
//! false, the listed `required`, every nested object/array/enum/identifier/
//! SHA/integer rule). Truly open fields (`ModelCapability.value`,
//! `RoutingDecision.capability_evidence[].value`) are [`Shape::Any`]. Nothing
//! here converts a number through `f64` or invents a ceiling; `date-time` is
//! an annotation and admits any string. Strict SHA treatment applies only
//! where the schema states the 40-hex pattern; other SHA-like fields
//! (`merge_commit`, `acceptance_sha`, `integration_sha`) stay plain strings.

use std::collections::BTreeMap;

use serde_json::value::RawValue;

use crate::graph_repository::{hex, id};
use crate::json_integer;

#[derive(Clone, Copy)]
pub(crate) enum Shape {
    /// String of 1..=200 Unicode scalar values.
    Id,
    /// String with `minLength` 1.
    NonEmpty,
    /// Lowercase 40-hex SHA.
    Sha,
    Str,
    Bool,
    Enum(&'static [&'static str]),
    Array(&'static Shape),
    NonEmptyArray(&'static Shape),
    Object(&'static Obj),
    /// JSON `integer` (mathematical: `1.0`, `1e3`, `-0` qualify).
    Int,
    IntMin0,
    IntMin1,
    /// Any JSON number token.
    Number,
    /// Number with `minimum` 0 and `maximum` 1.
    UnitInterval,
    /// `type: [<inner>, "null"]`.
    Null(&'static Shape),
    /// No schema constraint.
    Any,
}

pub(crate) struct Obj {
    pub(crate) fields: &'static [(&'static str, Shape)],
    pub(crate) required: &'static [&'static str],
}

use Shape::*;

const CONFIDENCE5: &[&str] = &[
    "OFFICIAL_VERIFIED",
    "INDEPENDENT_VERIFIED",
    "LOCAL_EMPIRICAL",
    "USER_DECLARED",
    "UNVERIFIED",
];
const HML: &[&str] = &["HIGH", "MEDIUM", "LOW"];
const FLOOR: &[&str] = &["FRONTIER", "BALANCED", "ECONOMY"];
const LMHU: &[&str] = &["LOW", "MEDIUM", "HIGH", "UNKNOWN"];
const ASSURANCE: &[&str] = &["LIGHT", "STANDARD", "HIGH_ASSURANCE"];

/// `ref_type`/`target`/`digest`/`section` evidence reference.
const REF: Shape = Object(&Obj {
    fields: &[
        (
            "ref_type",
            Enum(&["REPO_PATH", "STATE_QUERY", "ARTIFACT_ID", "URL"]),
        ),
        ("target", NonEmpty),
        ("digest", Str),
        ("section", Str),
    ],
    required: &["ref_type", "target"],
});

const FINDING: Shape = Object(&Obj {
    fields: &[
        ("finding_id", Id),
        (
            "severity",
            Enum(&["INFO", "LOW", "MEDIUM", "HIGH", "CRITICAL"]),
        ),
        (
            "category",
            Enum(&[
                "ARCHITECTURE_VIOLATION",
                "CONTRACT_VIOLATION",
                "SECURITY_BOUNDARY_VIOLATION",
                "WRITE_SCOPE_VIOLATION",
                "UNDISCLOSED_CHANGE",
                "MISSING_REQUIRED_NEGATIVE_TEST",
                "UNREPRODUCIBLE_EVIDENCE",
                "OVERSTATED_LABEL",
                "TEST_WEAKENED_OR_DELETED",
                "ACCEPTANCE_CRITERION_UNMET",
                "CORRECTNESS",
                "ERROR_HANDLING",
                "REGRESSION_RISK",
                "STYLE",
                "NAMING",
                "MINOR_PERFORMANCE",
                "DOC_GAP",
            ]),
        ),
        ("description", NonEmpty),
        ("blocking", Bool),
        ("path", Str),
        ("line", IntMin1),
        ("evidence_ref", REF),
        ("confidence", Enum(HML)),
        (
            "source",
            Enum(&[
                "LLM_REVIEW",
                "STATIC_ANALYSIS",
                "DEPENDENCY_SCAN",
                "TEST",
                "CONFIG_CHECK",
            ]),
        ),
    ],
    required: &[
        "finding_id",
        "severity",
        "category",
        "description",
        "blocking",
    ],
});

const EXEC_CHECK: Shape = Object(&Obj {
    fields: &[
        (
            "source",
            Enum(&[
                "WORKER_EXECUTION",
                "BROKER_EXECUTION",
                "REVIEW_EXECUTION",
                "GIT_PROVENANCE",
            ]),
        ),
        ("command", NonEmptyArray(&Str)),
        ("exit_code", Int),
        ("code_sha", Sha),
        ("timed_out", Bool),
        ("started_at", Str),
        ("finished_at", Str),
        ("output_ref", REF),
        (
            "result",
            Enum(&["PASS", "FAIL", "ERROR", "SKIPPED", "UNKNOWN"]),
        ),
    ],
    required: &["source", "command", "exit_code", "code_sha"],
});

pub(crate) const ROUTING_DECISION: Obj = Obj {
    fields: &[
        ("decision_id", Id),
        ("request_id", Id),
        ("task_id", Id),
        ("occurred_at", Str),
        (
            "outcome",
            Enum(&[
                "SELECTED",
                "NO_ELIGIBLE_CANDIDATE",
                "USER_INPUT_REQUIRED",
                "BLOCKED",
            ]),
        ),
        ("selected_provider", NonEmpty),
        ("selected_model", NonEmpty),
        ("selected_runtime", NonEmpty),
        (
            "selection_reason",
            Object(&Obj {
                fields: &[
                    ("quality_floor_applied", Enum(FLOOR)),
                    ("hard_filters_passed", Array(&Str)),
                    (
                        "score_components",
                        Object(&Obj {
                            fields: &[
                                ("expected_implementation_cost", Null(&Number)),
                                ("expected_rejection_probability", Null(&UnitInterval)),
                                ("expected_repair_cost", Null(&Number)),
                                ("expected_review_cost", Null(&Number)),
                                ("latency_estimate_seconds", Null(&Number)),
                            ],
                            required: &[],
                        }),
                    ),
                    ("decisive_factor", Str),
                ],
                required: &[],
            }),
        ),
        (
            "capability_evidence",
            Array(&Object(&Obj {
                fields: &[
                    ("capability", Str),
                    ("value", Any),
                    ("confidence", Enum(CONFIDENCE5)),
                    ("source_ref", REF),
                    ("observed_at", Str),
                    ("sample_size", Null(&IntMin0)),
                ],
                required: &["capability", "confidence"],
            })),
        ),
        ("confidence", Enum(HML)),
        ("availability_at_decision", Str),
        ("estimated_cost_class", Enum(LMHU)),
        (
            "alternative_candidates",
            Array(&Object(&Obj {
                fields: &[
                    ("provider_id", NonEmpty),
                    ("model_id", NonEmpty),
                    ("runtime_id", NonEmpty),
                    ("score", Null(&Number)),
                    ("rejection_reason", Str),
                ],
                required: &["provider_id", "model_id", "runtime_id", "rejection_reason"],
            })),
        ),
        (
            "registry_freshness",
            Object(&Obj {
                fields: &[
                    ("model_list_age_seconds", Null(&Int)),
                    ("capability_age_seconds", Null(&Int)),
                    ("calibration_sample_size", Null(&Int)),
                    ("stale", Bool),
                ],
                required: &["stale"],
            }),
        ),
        (
            "mode",
            Enum(&["AUTO_CURRENT", "ASK_ON_UNCERTAINTY", "USER_CONTROLLED"]),
        ),
        ("user_involved", Bool),
        ("user_pin_applied", Bool),
        ("fallback_from", Id),
    ],
    required: &[
        "decision_id",
        "request_id",
        "outcome",
        "registry_freshness",
        "mode",
    ],
};

pub(crate) const MODEL_CAPABILITY: Obj = Obj {
    fields: &[
        (
            "subject_kind",
            Enum(&["MODEL", "RUNTIME", "MODEL_RUNTIME_PAIR"]),
        ),
        ("provider_id", NonEmpty),
        ("model_id", NonEmpty),
        ("runtime_id", NonEmpty),
        ("capability", NonEmpty),
        ("value", Any),
        ("confidence", Enum(CONFIDENCE5)),
        ("source_ref", REF),
        ("observed_at", Str),
        ("sample_size", Null(&IntMin0)),
        (
            "lifecycle_state",
            Enum(&[
                "DISCOVERED",
                "UNASSESSED",
                "CAPABILITY_VERIFIED",
                "CALIBRATING",
                "ROUTABLE",
                "DEPRECATED",
                "DISABLED",
                "UNAVAILABLE",
                "USER_BLOCKED",
            ]),
        ),
    ],
    required: &["subject_kind", "capability", "confidence", "observed_at"],
};

pub(crate) const MODEL_OBSERVATION: Obj = Obj {
    fields: &[
        ("observation_id", Id),
        ("provider_id", NonEmpty),
        ("model_id", NonEmpty),
        ("runtime_id", NonEmpty),
        ("task_id", Id),
        ("task_class", Str),
        (
            "outcome",
            Enum(&[
                "ACCEPTED_FIRST_PASS",
                "ACCEPTED_AFTER_REPAIR",
                "REJECTED",
                "TIMEOUT",
                "RUNTIME_FAILURE",
                "CANCELLED",
                "USER_OVERRIDE",
            ]),
        ),
        ("repair_count", IntMin0),
        ("cost", Null(&Number)),
        ("tokens", Null(&Int)),
        ("latency_seconds", Null(&Number)),
        ("cost_to_accepted_result", Null(&Number)),
        ("difficulty_hint", Enum(LMHU)),
        ("excluded_from_calibration", Bool),
        ("observed_at", Str),
    ],
    required: &[
        "observation_id",
        "provider_id",
        "model_id",
        "runtime_id",
        "task_class",
        "outcome",
        "observed_at",
    ],
};

pub(crate) const AVAILABILITY_STATE: Obj = Obj {
    fields: &[
        ("provider_id", NonEmpty),
        ("model_id", NonEmpty),
        ("runtime_id", NonEmpty),
        (
            "state",
            Enum(&[
                "AVAILABLE",
                "DEGRADED",
                "RATE_LIMITED",
                "SESSION_EXHAUSTED",
                "AUTH_REQUIRED",
                "PROVIDER_DOWN",
                "SAFETY_CHECK_PENDING",
                "POLICY_BLOCKED",
                "UNKNOWN",
            ]),
        ),
        ("retry_after_seconds", Null(&IntMin0)),
        (
            "signal_source",
            Enum(&[
                "RATE_LIMIT_HEADER",
                "EXIT_CODE",
                "STDERR_CLASSIFICATION",
                "LOCAL_USAGE_VIEW",
                "LATENCY_TREND",
                "PROBE",
                "UNKNOWN",
            ]),
        ),
        ("observed_at", Str),
        ("note", Str),
    ],
    required: &["provider_id", "state", "observed_at"],
};

pub(crate) const REVIEW_REQUEST: Obj = Obj {
    fields: &[
        ("request_id", Id),
        ("task_id", Id),
        ("attempt_id", Id),
        ("workstream_id", Id),
        ("baseline_sha", Sha),
        ("implementation_sha", Sha),
        ("branch", Str),
        ("objective", NonEmpty),
        (
            "acceptance_criteria",
            NonEmptyArray(&Object(&Obj {
                fields: &[
                    ("id", Id),
                    ("description", NonEmpty),
                    ("kind", Enum(&["DETERMINISTIC", "SEMANTIC"])),
                    ("check_command", Array(&Str)),
                    ("rationale", Str),
                ],
                required: &["id", "description", "kind"],
            })),
        ),
        ("non_goals", Array(&Str)),
        ("allowed_write_paths", NonEmptyArray(&Str)),
        ("architecture_refs", Array(&REF)),
        ("contract_refs", Array(&REF)),
        ("a3_handoff_ref", REF),
        ("assurance_profile", Enum(ASSURANCE)),
        (
            "review_scope",
            Enum(&["FULL", "SECURITY", "REGRESSION", "REPAIR_VERIFICATION"]),
        ),
        (
            "review_policy",
            Object(&Obj {
                fields: &[
                    ("required", Bool),
                    ("distinct_provider", Enum(&["OFF", "PREFERRED", "REQUIRED"])),
                    ("reviewer_floor", Enum(FLOOR)),
                    ("excluded_session_refs", Array(&Str)),
                ],
                required: &[],
            }),
        ),
        ("prior_review_ids", Array(&Id)),
        ("attempt_number", IntMin1),
        ("requested_at", Str),
        ("context_epoch", IntMin0),
    ],
    required: &[
        "request_id",
        "task_id",
        "attempt_id",
        "implementation_sha",
        "baseline_sha",
        "objective",
        "acceptance_criteria",
        "allowed_write_paths",
        "assurance_profile",
        "review_scope",
        "context_epoch",
    ],
};

pub(crate) const A4_REVIEW: Obj = Obj {
    fields: &[
        ("review_id", Id),
        ("task_id", Id),
        ("reviewed_sha", Sha),
        (
            "reviewer",
            Object(&Obj {
                fields: &[
                    ("provider_id", NonEmpty),
                    ("model_id", NonEmpty),
                    ("runtime_id", NonEmpty),
                    ("session_ref", Str),
                ],
                required: &[],
            }),
        ),
        ("reviewed_at", Str),
        ("independence_attested", Bool),
        (
            "verdict",
            Enum(&["PASS", "PASS_WITH_NONBLOCKING_FINDINGS", "REJECT"]),
        ),
        ("blocking_findings", Array(&FINDING)),
        ("nonblocking_findings", Array(&FINDING)),
        (
            "dimensions",
            NonEmptyArray(&Object(&Obj {
                fields: &[
                    (
                        "dimension",
                        Enum(&[
                            "SCOPE_COMPLIANCE",
                            "CONTRACT_COMPLIANCE",
                            "ARCHITECTURE_COMPLIANCE",
                            "CORRECTNESS",
                            "ERROR_HANDLING",
                            "SECURITY_TRUST_BOUNDARIES",
                            "TEST_ADEQUACY",
                            "NEGATIVE_TESTS",
                            "REGRESSION_RISK",
                            "WRITE_SCOPE",
                            "UNDISCLOSED_CHANGES",
                            "EVIDENCE_ACCURACY",
                        ]),
                    ),
                    (
                        "assessment",
                        Enum(&["SATISFIED", "NOT_SATISFIED", "NOT_APPLICABLE"]),
                    ),
                    ("note", Str),
                ],
                required: &["dimension", "assessment"],
            })),
        ),
        (
            "reproduction",
            Object(&Obj {
                fields: &[
                    ("performed", Bool),
                    ("checks", Array(&EXEC_CHECK)),
                    ("limitation", Null(&Str)),
                ],
                required: &[],
            }),
        ),
        ("unauthorized_file_changes", Array(&Str)),
        (
            "recommended_action",
            Enum(&[
                "ACCEPT",
                "ACCEPT_WITH_FINDINGS_LOGGED",
                "REPAIR_REQUIRED",
                "ESCALATE_TO_A1",
            ]),
        ),
        ("rationale", Str),
    ],
    required: &[
        "review_id",
        "task_id",
        "reviewed_sha",
        "verdict",
        "independence_attested",
        "blocking_findings",
        "nonblocking_findings",
        "dimensions",
    ],
};

pub(crate) const INTEGRATION_REQUEST: Obj = Obj {
    fields: &[
        ("request_id", Id),
        ("gate_level", Enum(&["A2_ACCEPTANCE", "A1_INTEGRATION"])),
        ("source_branch", Str),
        ("source_sha", Sha),
        ("target_ref", Str),
        (
            "tasks_included",
            NonEmptyArray(&Object(&Obj {
                fields: &[
                    ("task_id", Id),
                    ("implementation_sha", Sha),
                    ("review_sha", Sha),
                    (
                        "a4_verdict",
                        Enum(&["PASS", "PASS_WITH_NONBLOCKING_FINDINGS"]),
                    ),
                    ("findings_disposition", Str),
                    ("merge_commit", Null(&Str)),
                ],
                required: &["task_id", "implementation_sha", "review_sha", "a4_verdict"],
            })),
        ),
        ("assurance_profile", Enum(ASSURANCE)),
        ("post_merge_checks", Array(&EXEC_CHECK)),
        ("open_nonblocking_findings", Array(&FINDING)),
        ("known_limitations", Array(&Str)),
        (
            "attestations",
            Object(&Obj {
                fields: &[
                    ("all_tasks_accepted", Bool),
                    ("review_sha_equals_implementation_sha", Bool),
                    ("no_commit_after_review", Bool),
                    ("no_unresolved_blocking_finding", Bool),
                    ("no_frozen_artifact_modified", Bool),
                    ("no_out_of_scope_write", Bool),
                ],
                required: &[],
            }),
        ),
    ],
    required: &[
        "request_id",
        "gate_level",
        "source_branch",
        "source_sha",
        "target_ref",
        "tasks_included",
    ],
};

pub(crate) const INTEGRATION_DECISION: Obj = Obj {
    fields: &[
        ("decision_id", Id),
        ("request_id", Id),
        (
            "outcome",
            Enum(&["ACCEPT", "REPAIR", "BLOCKED", "HUMAN_REQUIRED"]),
        ),
        (
            "checks_evaluated",
            NonEmptyArray(&Object(&Obj {
                fields: &[
                    ("check", Str),
                    ("result", Enum(&["PASS", "FAIL", "NOT_APPLICABLE"])),
                    ("detail", Str),
                ],
                required: &["check", "result"],
            })),
        ),
        (
            "provenance_chain",
            Array(&Object(&Obj {
                fields: &[
                    ("task_id", Id),
                    ("start_sha", Sha),
                    ("implementation_sha", Sha),
                    ("review_sha", Sha),
                    ("acceptance_sha", Null(&Str)),
                    ("integration_sha", Null(&Str)),
                ],
                required: &["task_id", "start_sha", "implementation_sha", "review_sha"],
            })),
        ),
        ("unmet_conditions", Array(&Str)),
        ("integration_sha", Null(&Str)),
        ("decided_at", Str),
    ],
    required: &[
        "decision_id",
        "request_id",
        "outcome",
        "checks_evaluated",
        "decided_at",
    ],
};

fn string(raw: &RawValue) -> Option<String> {
    serde_json::from_str::<String>(raw.get()).ok()
}
fn number(raw: &RawValue) -> bool {
    matches!(raw.get().as_bytes().first(), Some(b'-' | b'0'..=b'9'))
}
fn integer(raw: &RawValue) -> bool {
    number(raw) && json_integer::is_integer(raw)
}

/// True when `raw` satisfies `shape`. Duplicate keys are refused separately
/// by the caller; every object is parsed here into unique raw members.
pub(crate) fn admits(shape: &Shape, raw: &RawValue) -> bool {
    let text = raw.get();
    match shape {
        Id => string(raw).is_some_and(|s| id(&s)),
        NonEmpty => string(raw).is_some_and(|s| !s.is_empty()),
        Sha => string(raw).is_some_and(|s| hex(&s, 40)),
        Str => string(raw).is_some(),
        Bool => matches!(text, "true" | "false"),
        Enum(values) => string(raw).is_some_and(|s| values.contains(&s.as_str())),
        Int => integer(raw),
        IntMin0 => integer(raw) && !json_integer::is_negative(raw),
        IntMin1 => integer(raw) && !json_integer::is_negative(raw) && !json_integer::is_zero(raw),
        Number => number(raw),
        UnitInterval => {
            number(raw) && !json_integer::is_negative(raw) && !json_integer::exceeds_one(raw)
        }
        Null(inner) => text == "null" || admits(inner, raw),
        Any => true,
        Array(items) | NonEmptyArray(items) => {
            let Ok(members) = serde_json::from_str::<Vec<Box<RawValue>>>(text) else {
                return false;
            };
            (!matches!(shape, NonEmptyArray(_)) || !members.is_empty())
                && members.iter().all(|member| admits(items, member))
        }
        Object(obj) => admits_object(obj, raw),
    }
}

pub(crate) fn admits_object(obj: &Obj, raw: &RawValue) -> bool {
    let Ok(members) = serde_json::from_str::<BTreeMap<String, Box<RawValue>>>(raw.get()) else {
        return false;
    };
    members.iter().all(|(key, value)| {
        obj.fields
            .iter()
            .find(|(name, _)| name == key)
            .is_some_and(|(_, shape)| admits(shape, value))
    }) && obj.required.iter().all(|key| members.contains_key(*key))
}
