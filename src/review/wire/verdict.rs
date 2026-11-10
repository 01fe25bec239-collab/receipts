//! A4Review and SafetyInterruption codecs.

use super::ReviewWireError;
use super::ReviewWireErrorKind as K;
use super::fields::{
    Obj, a4_finding, array, check_fields, err, nonempty_array, opt_bool, opt_str, opt_strs, root,
    write_a4_finding, write_check, write_refs,
};
use super::json::{Json, Writer};
use crate::{
    A4Review, A4ReviewDimension, A4ReviewDimensionAssessment, A4ReviewDimensionReview,
    A4ReviewNonTemporalCore, A4ReviewRecommendedAction, A4ReviewReproduction,
    A4ReviewReproductionCheck, A4ReviewReproductionCheckResult, A4ReviewReproductionLimitation,
    A4ReviewReviewer, A4ReviewVerdict, SafetyInterruption, SafetyInterruptionDetectionConfidence,
    SafetyInterruptionNonTemporalCore, SafetyInterruptionState, SafetyInterruptionTerminalOutcome,
};

fn dimension(v: Json, p: &str) -> Result<A4ReviewDimensionReview, ReviewWireError> {
    let mut o = Obj::new(v, p.into())?;
    let d = o.enumeration(
        "dimension",
        &A4ReviewDimension::ALL,
        A4ReviewDimension::as_str,
    )?;
    let a = o.enumeration(
        "assessment",
        &A4ReviewDimensionAssessment::ALL,
        A4ReviewDimensionAssessment::as_str,
    )?;
    let note = o.opt_string("note")?;
    o.end()?;
    Ok(A4ReviewDimensionReview::new(d, a, note))
}

fn reproduction_check(v: Json, p: &str) -> Result<A4ReviewReproductionCheck, ReviewWireError> {
    let f = check_fields(v, p)?;
    let result = f.result_of(
        &A4ReviewReproductionCheckResult::ALL,
        A4ReviewReproductionCheckResult::as_str,
    )?;
    A4ReviewReproductionCheck::new_with_timestamps(
        f.source,
        f.command.clone(),
        f.exit_i64()?,
        f.code_sha.as_str().to_owned(),
        f.timed_out,
        f.output_ref.clone(),
        result,
        f.started_at.clone(),
        f.finished_at.clone(),
    )
    .map_err(|_| err(&f.path, K::InvalidValue))
}

/// Decodes the A4Review document; the result is not yet bound to any
/// capsule (see `review_parse` for exact-subject binding).
pub fn decode_a4_review(text: &str) -> Result<A4Review, ReviewWireError> {
    let mut o = root(text)?;
    let review_id = o.id("review_id")?;
    let task_id = o.id("task_id")?;
    let reviewed_sha = o.sha("reviewed_sha")?;
    let reviewer = o
        .opt_object("reviewer")?
        .map(|mut r| {
            let provider_id = r.opt_nonempty("provider_id")?;
            let model_id = r.opt_nonempty("model_id")?;
            let runtime_id = r.opt_nonempty("runtime_id")?;
            let session_ref = r.opt_string("session_ref")?;
            let path = r.path.clone();
            r.end()?;
            A4ReviewReviewer::new(provider_id, model_id, runtime_id, session_ref)
                .map_err(|_| err(&path, K::InvalidValue))
        })
        .transpose()?;
    let reviewed_at = o.opt_date_time("reviewed_at")?;
    let independence_attested = o.boolean("independence_attested")?;
    let verdict = o.enumeration("verdict", &A4ReviewVerdict::ALL, A4ReviewVerdict::as_str)?;
    let (bv, bp) = o.req("blocking_findings")?;
    let blocking = array(bv, &bp, a4_finding)?;
    let (nv, np) = o.req("nonblocking_findings")?;
    let nonblocking = array(nv, &np, a4_finding)?;
    let (dv, dp) = o.req("dimensions")?;
    let dimensions = nonempty_array(dv, &dp, dimension)?;
    let reproduction = o
        .opt_object("reproduction")?
        .map(|mut r| {
            let performed = r.opt_bool("performed")?;
            let checks = r
                .opt("checks")
                .map(|(v, p)| array(v, &p, reproduction_check))
                .transpose()?;
            let limitation = match r.nullable_string("limitation")? {
                None => A4ReviewReproductionLimitation::Omitted,
                Some(None) => A4ReviewReproductionLimitation::Null,
                Some(Some(t)) => A4ReviewReproductionLimitation::Text(t),
            };
            r.end()?;
            Ok::<_, ReviewWireError>(A4ReviewReproduction::new(performed, checks, limitation))
        })
        .transpose()?;
    let unauthorized = o.opt_strings("unauthorized_file_changes")?;
    let recommended_action = o.opt_enum(
        "recommended_action",
        &A4ReviewRecommendedAction::ALL,
        A4ReviewRecommendedAction::as_str,
    )?;
    let rationale = o.opt_string("rationale")?;
    o.end()?;
    let core = A4ReviewNonTemporalCore::new(
        review_id,
        task_id,
        reviewed_sha,
        verdict,
        independence_attested,
        blocking,
        nonblocking,
        dimensions,
        reviewer,
        reproduction,
        unauthorized,
        recommended_action,
        rationale,
    )
    .map_err(|_| err("", K::InvalidValue))?;
    Ok(A4Review::new(core, reviewed_at))
}

pub fn encode_a4_review(value: &A4Review) -> String {
    let c = value.core();
    let mut w = Writer::default();
    w.begin_object();
    w.key("review_id");
    w.str(c.review_id());
    w.key("task_id");
    w.str(c.task_id());
    w.key("reviewed_sha");
    w.str(c.reviewed_sha());
    if let Some(r) = c.reviewer() {
        w.key("reviewer");
        w.begin_object();
        opt_str(&mut w, "provider_id", r.provider_id());
        opt_str(&mut w, "model_id", r.model_id());
        opt_str(&mut w, "runtime_id", r.runtime_id());
        opt_str(&mut w, "session_ref", r.session_ref());
        w.end_object();
    }
    opt_str(
        &mut w,
        "reviewed_at",
        value.reviewed_at().map(|t| t.as_str()),
    );
    w.key("independence_attested");
    w.bool(c.independence_attested());
    w.key("verdict");
    w.str(c.verdict().as_str());
    for (key, findings) in [
        ("blocking_findings", c.blocking_findings()),
        ("nonblocking_findings", c.nonblocking_findings()),
    ] {
        w.key(key);
        w.begin_array();
        for f in findings {
            write_a4_finding(&mut w, f);
        }
        w.end_array();
    }
    w.key("dimensions");
    w.begin_array();
    for d in c.dimensions() {
        w.begin_object();
        w.key("dimension");
        w.str(d.dimension().as_str());
        w.key("assessment");
        w.str(d.assessment().as_str());
        opt_str(&mut w, "note", d.note());
        w.end_object();
    }
    w.end_array();
    if let Some(r) = c.reproduction() {
        w.key("reproduction");
        w.begin_object();
        opt_bool(&mut w, "performed", r.performed());
        if let Some(checks) = r.checks() {
            w.key("checks");
            w.begin_array();
            for ch in checks {
                write_check(
                    &mut w,
                    ch.source(),
                    ch.command(),
                    &ch.exit_code().to_string(),
                    ch.code_sha(),
                    ch.timed_out(),
                    ch.started_at(),
                    ch.finished_at(),
                    ch.output_ref(),
                    ch.result().map(A4ReviewReproductionCheckResult::as_str),
                );
            }
            w.end_array();
        }
        match r.limitation() {
            A4ReviewReproductionLimitation::Omitted => {}
            A4ReviewReproductionLimitation::Null => {
                w.key("limitation");
                w.null();
            }
            A4ReviewReproductionLimitation::Text(t) => {
                w.key("limitation");
                w.str(t);
            }
        }
        w.end_object();
    }
    opt_strs(
        &mut w,
        "unauthorized_file_changes",
        c.unauthorized_file_changes(),
    );
    opt_str(
        &mut w,
        "recommended_action",
        c.recommended_action()
            .map(A4ReviewRecommendedAction::as_str),
    );
    opt_str(&mut w, "rationale", c.rationale());
    w.end_object();
    w.finish()
}

// ---- SafetyInterruption -----------------------------------------------------

pub fn decode_safety_interruption(text: &str) -> Result<SafetyInterruption, ReviewWireError> {
    let mut o = root(text)?;
    let interruption_id = o.id("interruption_id")?;
    let task_id = o.id("task_id")?;
    let attempt_id = o.id("attempt_id")?;
    let provider_id = o.nonempty("provider_id")?;
    let model_id = o.opt_nonempty("model_id")?;
    let state = o.enumeration(
        "state",
        &SafetyInterruptionState::ALL,
        SafetyInterruptionState::as_str,
    )?;
    let detection_confidence = o.opt_enum(
        "detection_confidence",
        &SafetyInterruptionDetectionConfidence::ALL,
        SafetyInterruptionDetectionConfidence::as_str,
    )?;
    let defensive = o.opt_bool("task_classified_defensive")?;
    let narrowed = o.opt_bool("capsule_narrowed")?;
    let retried = o.opt_bool("retry_attempted")?;
    let retry_provider_id = o.opt_nonempty("retry_provider_id")?;
    let tooling = o.opt_bool("deterministic_tooling_used")?;
    let terminal = o.opt_enum(
        "terminal_outcome",
        &SafetyInterruptionTerminalOutcome::ALL,
        SafetyInterruptionTerminalOutcome::as_str,
    )?;
    let observed_at = o.date_time("observed_at")?;
    let preserved = o.opt_refs("preserved_evidence_refs")?;
    o.end()?;
    let core = SafetyInterruptionNonTemporalCore::new(
        interruption_id,
        task_id,
        attempt_id,
        provider_id,
        model_id,
        state,
        detection_confidence,
        defensive,
        narrowed,
        retried,
        retry_provider_id,
        tooling,
        terminal,
        preserved,
    )
    .map_err(|_| err("", K::InvalidValue))?;
    Ok(SafetyInterruption::new(core, observed_at))
}

pub fn encode_safety_interruption(value: &SafetyInterruption) -> String {
    let c = value.core();
    let mut w = Writer::default();
    w.begin_object();
    w.key("interruption_id");
    w.str(c.interruption_id());
    w.key("task_id");
    w.str(c.task_id());
    w.key("attempt_id");
    w.str(c.attempt_id());
    w.key("provider_id");
    w.str(c.provider_id());
    opt_str(&mut w, "model_id", c.model_id());
    w.key("state");
    w.str(c.state().as_str());
    opt_str(
        &mut w,
        "detection_confidence",
        c.detection_confidence()
            .map(SafetyInterruptionDetectionConfidence::as_str),
    );
    opt_bool(
        &mut w,
        "task_classified_defensive",
        c.task_classified_defensive(),
    );
    opt_bool(&mut w, "capsule_narrowed", c.capsule_narrowed());
    opt_bool(&mut w, "retry_attempted", c.retry_attempted());
    opt_str(&mut w, "retry_provider_id", c.retry_provider_id());
    opt_bool(
        &mut w,
        "deterministic_tooling_used",
        c.deterministic_tooling_used(),
    );
    opt_str(
        &mut w,
        "terminal_outcome",
        c.terminal_outcome()
            .map(SafetyInterruptionTerminalOutcome::as_str),
    );
    w.key("observed_at");
    w.str(value.observed_at().as_str());
    write_refs(
        &mut w,
        "preserved_evidence_refs",
        c.preserved_evidence_refs(),
    );
    w.end_object();
    w.finish()
}
