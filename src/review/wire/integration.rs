//! IntegrationRequest and IntegrationDecision codecs.

use super::ReviewWireError;
use super::ReviewWireErrorKind as K;
use super::fields::{
    Obj, array, check_fields, err, finding_fields, nonempty_array, opt_bool, opt_str, opt_strs,
    root, string, write_check, write_finding,
};
use super::json::{Json, Writer};
use crate::{
    AssuranceProfile, IntegrationDecision, IntegrationDecisionCheck,
    IntegrationDecisionCheckResult, IntegrationDecisionNonTemporalCore,
    IntegrationDecisionNullableString, IntegrationDecisionOutcome, IntegrationDecisionProvenance,
    IntegrationRequestA4Verdict, IntegrationRequestAttestations, IntegrationRequestGateLevel,
    IntegrationRequestNonTemporalCore, IntegrationRequestOpenFinding,
    IntegrationRequestPositiveInteger, IntegrationRequestPostMergeCheck,
    IntegrationRequestSignedInteger, IntegrationRequestTask, ReviewCapsuleCheckResult,
};

fn nullable(o: &mut Obj, key: &str) -> Result<IntegrationDecisionNullableString, ReviewWireError> {
    Ok(match o.nullable_string(key)? {
        None => IntegrationDecisionNullableString::Omitted,
        Some(None) => IntegrationDecisionNullableString::Null,
        Some(Some(s)) => IntegrationDecisionNullableString::String(s),
    })
}

fn write_nullable(w: &mut Writer, key: &str, v: &IntegrationDecisionNullableString) {
    match v {
        IntegrationDecisionNullableString::Omitted => {}
        IntegrationDecisionNullableString::Null => {
            w.key(key);
            w.null();
        }
        IntegrationDecisionNullableString::String(s) => {
            w.key(key);
            w.str(s);
        }
    }
}

// ---- IntegrationRequest -----------------------------------------------------

fn task(v: Json, p: &str) -> Result<IntegrationRequestTask, ReviewWireError> {
    let mut o = Obj::new(v, p.into())?;
    let task_id = o.id("task_id")?;
    let implementation_sha = o.sha("implementation_sha")?;
    let review_sha = o.sha("review_sha")?;
    let verdict = o.enumeration(
        "a4_verdict",
        &IntegrationRequestA4Verdict::ALL,
        IntegrationRequestA4Verdict::as_str,
    )?;
    let disposition = o.opt_string("findings_disposition")?;
    let merge_commit = nullable(&mut o, "merge_commit")?;
    o.end()?;
    IntegrationRequestTask::new(
        task_id,
        implementation_sha,
        review_sha,
        verdict,
        disposition,
        merge_commit,
    )
    .map_err(|_| err(p, K::InvalidValue))
}

fn post_merge(v: Json, p: &str) -> Result<IntegrationRequestPostMergeCheck, ReviewWireError> {
    let f = check_fields(v, p)?;
    let result = f.result_of(
        &ReviewCapsuleCheckResult::ALL,
        ReviewCapsuleCheckResult::as_str,
    )?;
    let exit = IntegrationRequestSignedInteger::from_decimal(&f.exit_digits)
        .map_err(|_| err(&format!("{p}/exit_code"), K::InvalidValue))?;
    IntegrationRequestPostMergeCheck::new_with_timestamps(
        f.source,
        f.command,
        exit,
        f.code_sha.as_str().to_owned(),
        f.timed_out,
        f.output_ref,
        result,
        f.started_at,
        f.finished_at,
    )
    .map_err(|_| err(p, K::InvalidValue))
}

fn open_finding(v: Json, p: &str) -> Result<IntegrationRequestOpenFinding, ReviewWireError> {
    let f = finding_fields(v, p)?;
    let line = f
        .line
        .map(|(digits, lp)| {
            IntegrationRequestPositiveInteger::from_decimal(&digits)
                .map_err(|_| err(&lp, K::InvalidValue))
        })
        .transpose()?;
    IntegrationRequestOpenFinding::new(
        f.finding_id,
        f.severity,
        f.category,
        f.description,
        f.blocking,
        f.path,
        line,
        f.evidence_ref,
        f.confidence,
        f.source,
    )
    .map_err(|_| err(p, K::InvalidValue))
}

pub fn decode_integration_request(
    text: &str,
) -> Result<IntegrationRequestNonTemporalCore, ReviewWireError> {
    let mut o = root(text)?;
    let request_id = o.id("request_id")?;
    let gate_level = o.enumeration(
        "gate_level",
        &IntegrationRequestGateLevel::ALL,
        IntegrationRequestGateLevel::as_str,
    )?;
    let source_branch = o.string("source_branch")?;
    let source_sha = o.sha("source_sha")?;
    let target_ref = o.string("target_ref")?;
    let (tv, tp) = o.req("tasks_included")?;
    let tasks = nonempty_array(tv, &tp, task)?;
    let assurance_profile = o.opt_enum(
        "assurance_profile",
        &AssuranceProfile::ALL,
        AssuranceProfile::as_str,
    )?;
    let post_merge_checks = o
        .opt("post_merge_checks")
        .map(|(v, p)| array(v, &p, post_merge))
        .transpose()?;
    let open = o
        .opt("open_nonblocking_findings")
        .map(|(v, p)| array(v, &p, open_finding))
        .transpose()?;
    let known_limitations = o.opt_strings("known_limitations")?;
    let attestations = o
        .opt_object("attestations")?
        .map(|mut a| {
            let v = IntegrationRequestAttestations::new(
                a.opt_bool("all_tasks_accepted")?,
                a.opt_bool("review_sha_equals_implementation_sha")?,
                a.opt_bool("no_commit_after_review")?,
                a.opt_bool("no_unresolved_blocking_finding")?,
                a.opt_bool("no_frozen_artifact_modified")?,
                a.opt_bool("no_out_of_scope_write")?,
            );
            a.end()?;
            Ok::<_, ReviewWireError>(v)
        })
        .transpose()?;
    o.end()?;
    IntegrationRequestNonTemporalCore::new(
        request_id,
        gate_level,
        source_branch,
        source_sha,
        target_ref,
        tasks,
        assurance_profile,
        post_merge_checks,
        open,
        known_limitations,
        attestations,
    )
    .map_err(|_| err("", K::InvalidValue))
}

pub fn encode_integration_request(c: &IntegrationRequestNonTemporalCore) -> String {
    let mut w = Writer::default();
    w.begin_object();
    w.key("request_id");
    w.str(c.request_id());
    w.key("gate_level");
    w.str(c.gate_level().as_str());
    w.key("source_branch");
    w.str(c.source_branch());
    w.key("source_sha");
    w.str(c.source_sha().as_str());
    w.key("target_ref");
    w.str(c.target_ref());
    w.key("tasks_included");
    w.begin_array();
    for t in c.tasks_included() {
        w.begin_object();
        w.key("task_id");
        w.str(t.task_id());
        w.key("implementation_sha");
        w.str(t.implementation_sha().as_str());
        w.key("review_sha");
        w.str(t.review_sha().as_str());
        w.key("a4_verdict");
        w.str(t.a4_verdict().as_str());
        opt_str(&mut w, "findings_disposition", t.findings_disposition());
        write_nullable(&mut w, "merge_commit", t.merge_commit());
        w.end_object();
    }
    w.end_array();
    opt_str(
        &mut w,
        "assurance_profile",
        c.assurance_profile().map(AssuranceProfile::as_str),
    );
    if let Some(checks) = c.post_merge_checks() {
        w.key("post_merge_checks");
        w.begin_array();
        for ch in checks {
            // decimal_digits() is the magnitude; the sign is separate.
            let exit = ch.exit_code();
            let exit = if exit.is_negative() {
                format!("-{}", exit.decimal_digits())
            } else {
                exit.decimal_digits().to_owned()
            };
            write_check(
                &mut w,
                ch.source(),
                ch.command(),
                &exit,
                ch.code_sha().as_str(),
                ch.timed_out(),
                ch.started_at(),
                ch.finished_at(),
                ch.output_ref(),
                ch.result().map(ReviewCapsuleCheckResult::as_str),
            );
        }
        w.end_array();
    }
    if let Some(findings) = c.open_nonblocking_findings() {
        w.key("open_nonblocking_findings");
        w.begin_array();
        for f in findings {
            write_finding(
                &mut w,
                f.finding_id(),
                f.severity(),
                f.category(),
                f.description(),
                f.blocking(),
                f.path(),
                f.line().map(|l| l.decimal_digits()),
                f.evidence_ref(),
                f.confidence(),
                f.source(),
            );
        }
        w.end_array();
    }
    opt_strs(&mut w, "known_limitations", c.known_limitations());
    if let Some(a) = c.attestations() {
        w.key("attestations");
        w.begin_object();
        opt_bool(&mut w, "all_tasks_accepted", a.all_tasks_accepted());
        opt_bool(
            &mut w,
            "review_sha_equals_implementation_sha",
            a.review_sha_equals_implementation_sha(),
        );
        opt_bool(&mut w, "no_commit_after_review", a.no_commit_after_review());
        opt_bool(
            &mut w,
            "no_unresolved_blocking_finding",
            a.no_unresolved_blocking_finding(),
        );
        opt_bool(
            &mut w,
            "no_frozen_artifact_modified",
            a.no_frozen_artifact_modified(),
        );
        opt_bool(&mut w, "no_out_of_scope_write", a.no_out_of_scope_write());
        w.end_object();
    }
    w.end_object();
    w.finish()
}

// ---- IntegrationDecision ----------------------------------------------------

fn decision_check(v: Json, p: &str) -> Result<IntegrationDecisionCheck, ReviewWireError> {
    let mut o = Obj::new(v, p.into())?;
    let check = o.string("check")?;
    let result = o.enumeration(
        "result",
        &IntegrationDecisionCheckResult::ALL,
        IntegrationDecisionCheckResult::as_str,
    )?;
    let detail = o.opt_string("detail")?;
    o.end()?;
    Ok(IntegrationDecisionCheck::new(check, result, detail))
}

fn provenance(v: Json, p: &str) -> Result<IntegrationDecisionProvenance, ReviewWireError> {
    let mut o = Obj::new(v, p.into())?;
    let task_id = o.id("task_id")?;
    let start_sha = o.sha("start_sha")?;
    let implementation_sha = o.sha("implementation_sha")?;
    let review_sha = o.sha("review_sha")?;
    let acceptance_sha = nullable(&mut o, "acceptance_sha")?;
    let integration_sha = nullable(&mut o, "integration_sha")?;
    o.end()?;
    IntegrationDecisionProvenance::new(
        task_id,
        start_sha,
        implementation_sha,
        review_sha,
        acceptance_sha,
        integration_sha,
    )
    .map_err(|_| err(p, K::InvalidValue))
}

pub fn decode_integration_decision(text: &str) -> Result<IntegrationDecision, ReviewWireError> {
    let mut o = root(text)?;
    let decision_id = o.id("decision_id")?;
    let request_id = o.id("request_id")?;
    let outcome = o.enumeration(
        "outcome",
        &IntegrationDecisionOutcome::ALL,
        IntegrationDecisionOutcome::as_str,
    )?;
    let (cv, cp) = o.req("checks_evaluated")?;
    let checks = nonempty_array(cv, &cp, decision_check)?;
    let chain = o
        .opt("provenance_chain")
        .map(|(v, p)| array(v, &p, provenance))
        .transpose()?;
    let unmet = o
        .opt("unmet_conditions")
        .map(|(v, p)| array(v, &p, string))
        .transpose()?;
    let integration_sha = nullable(&mut o, "integration_sha")?;
    let decided_at = o.date_time("decided_at")?;
    o.end()?;
    let core = IntegrationDecisionNonTemporalCore::new(
        decision_id,
        request_id,
        outcome,
        checks,
        chain,
        unmet,
        integration_sha,
    )
    .map_err(|_| err("", K::InvalidValue))?;
    Ok(IntegrationDecision::new(core, decided_at))
}

pub fn encode_integration_decision(value: &IntegrationDecision) -> String {
    let c = value.core();
    let mut w = Writer::default();
    w.begin_object();
    w.key("decision_id");
    w.str(c.decision_id());
    w.key("request_id");
    w.str(c.request_id());
    w.key("outcome");
    w.str(c.outcome().as_str());
    w.key("checks_evaluated");
    w.begin_array();
    for ch in c.checks_evaluated() {
        w.begin_object();
        w.key("check");
        w.str(ch.check());
        w.key("result");
        w.str(ch.result().as_str());
        opt_str(&mut w, "detail", ch.detail());
        w.end_object();
    }
    w.end_array();
    if let Some(chain) = c.provenance_chain() {
        w.key("provenance_chain");
        w.begin_array();
        for e in chain {
            w.begin_object();
            w.key("task_id");
            w.str(e.task_id());
            w.key("start_sha");
            w.str(e.start_sha().as_str());
            w.key("implementation_sha");
            w.str(e.implementation_sha().as_str());
            w.key("review_sha");
            w.str(e.review_sha().as_str());
            write_nullable(&mut w, "acceptance_sha", e.acceptance_sha());
            write_nullable(&mut w, "integration_sha", e.integration_sha());
            w.end_object();
        }
        w.end_array();
    }
    opt_strs(&mut w, "unmet_conditions", c.unmet_conditions());
    write_nullable(&mut w, "integration_sha", c.integration_sha());
    w.key("decided_at");
    w.str(value.decided_at().as_str());
    w.end_object();
    w.finish()
}
