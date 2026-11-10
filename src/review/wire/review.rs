//! ReviewRequest, ReviewCapsule and A3Handoff codecs.

use super::ReviewWireError;
use super::ReviewWireErrorKind as K;
use super::fields::{
    Obj, array, check_fields, err, id, nonempty_array, opt_bool, opt_str, opt_strs, reference,
    root, string, write_check, write_ref, write_refs,
};
use super::json::{Json, Writer};
use crate::{
    A3HandoffBlocker, A3HandoffCheck, A3HandoffCheckResult, A3HandoffContractConsumed,
    A3HandoffEvidenceLabel, A3HandoffImplementer, A3HandoffLabeledEvidence,
    A3HandoffNonTemporalCore, AssuranceProfile, DistinctProviderPolicy, ReviewCapsuleCheck,
    ReviewCapsuleCheckResult, ReviewCapsuleCriterion, ReviewCapsuleCriterionKind,
    ReviewCapsuleNonTemporalCore, ReviewCapsuleReviewScope, ReviewCapsuleSeverityPolicy,
    ReviewRequest, ReviewRequestAttemptNumber, ReviewRequestNonNegativeInteger,
    ReviewRequestNonTemporalCore, ReviewRequestPolicy, ReviewRequestReviewerFloor,
};

fn criterion(v: Json, p: &str) -> Result<ReviewCapsuleCriterion, ReviewWireError> {
    let mut o = Obj::new(v, p.into())?;
    let cid = o.id("id")?;
    let description = o.nonempty("description")?;
    let kind = o.enumeration(
        "kind",
        &ReviewCapsuleCriterionKind::ALL,
        ReviewCapsuleCriterionKind::as_str,
    )?;
    let check_command = o.opt_strings("check_command")?;
    let rationale = o.opt_string("rationale")?;
    o.end()?;
    ReviewCapsuleCriterion::new(cid, description, kind, check_command, rationale)
        .map_err(|_| err(p, K::InvalidValue))
}

fn write_criterion(w: &mut Writer, c: &ReviewCapsuleCriterion) {
    w.begin_object();
    w.key("id");
    w.str(c.id());
    w.key("description");
    w.str(c.description());
    w.key("kind");
    w.str(c.kind().as_str());
    opt_strs(w, "check_command", c.check_command());
    opt_str(w, "rationale", c.rationale());
    w.end_object();
}

fn criteria(o: &mut Obj) -> Result<Vec<ReviewCapsuleCriterion>, ReviewWireError> {
    let (v, p) = o.req("acceptance_criteria")?;
    nonempty_array(v, &p, criterion)
}

fn epoch(o: &mut Obj) -> Result<ReviewRequestNonNegativeInteger, ReviewWireError> {
    let p = o.sub("context_epoch");
    let digits = o.non_negative("context_epoch")?;
    ReviewRequestNonNegativeInteger::from_decimal(&digits).map_err(|_| err(&p, K::InvalidValue))
}

fn nonempty_strings(o: &mut Obj, key: &str) -> Result<Vec<String>, ReviewWireError> {
    let (v, p) = o.req(key)?;
    nonempty_array(v, &p, string)
}

// ---- ReviewRequest ----------------------------------------------------------

pub fn decode_review_request(text: &str) -> Result<ReviewRequest, ReviewWireError> {
    let mut o = root(text)?;
    let request_id = o.id("request_id")?;
    let task_id = o.id("task_id")?;
    let attempt_id = o.id("attempt_id")?;
    let workstream_id = o.opt_id("workstream_id")?;
    let baseline_sha = o.sha("baseline_sha")?;
    let implementation_sha = o.sha("implementation_sha")?;
    let branch = o.opt_string("branch")?;
    let objective = o.nonempty("objective")?;
    let acceptance_criteria = criteria(&mut o)?;
    let non_goals = o.opt_strings("non_goals")?;
    let allowed_write_paths = nonempty_strings(&mut o, "allowed_write_paths")?;
    let architecture_refs = o.opt_refs("architecture_refs")?;
    let contract_refs = o.opt_refs("contract_refs")?;
    let a3_handoff_ref = o.opt_ref("a3_handoff_ref")?;
    let assurance_profile = o.enumeration(
        "assurance_profile",
        &AssuranceProfile::ALL,
        AssuranceProfile::as_str,
    )?;
    let review_scope = o.enumeration(
        "review_scope",
        &ReviewCapsuleReviewScope::ALL,
        ReviewCapsuleReviewScope::as_str,
    )?;
    let review_policy = o
        .opt_object("review_policy")?
        .map(|mut p| {
            let required = p.opt_bool("required")?;
            let distinct_provider = p.opt_enum(
                "distinct_provider",
                &DistinctProviderPolicy::ALL,
                DistinctProviderPolicy::as_str,
            )?;
            let reviewer_floor = p.opt_enum(
                "reviewer_floor",
                &ReviewRequestReviewerFloor::ALL,
                ReviewRequestReviewerFloor::as_str,
            )?;
            let excluded = p.opt_strings("excluded_session_refs")?;
            p.end()?;
            Ok::<_, ReviewWireError>(ReviewRequestPolicy::new(
                required,
                distinct_provider,
                reviewer_floor,
                excluded,
            ))
        })
        .transpose()?;
    let prior_review_ids = o
        .opt("prior_review_ids")
        .map(|(v, p)| array(v, &p, id))
        .transpose()?;
    let attempt_number = o
        .opt("attempt_number")
        .map(|(v, p)| {
            let digits = super::fields::integer(v, &p)?;
            ReviewRequestAttemptNumber::from_decimal(&digits).map_err(|_| err(&p, K::InvalidValue))
        })
        .transpose()?;
    let requested_at = o.opt_date_time("requested_at")?;
    let context_epoch = epoch(&mut o)?;
    o.end()?;
    let core = ReviewRequestNonTemporalCore::new(
        request_id,
        task_id,
        attempt_id,
        implementation_sha,
        baseline_sha,
        objective,
        acceptance_criteria,
        allowed_write_paths,
        assurance_profile,
        review_scope,
        context_epoch,
        workstream_id,
        branch,
        non_goals,
        architecture_refs,
        contract_refs,
        a3_handoff_ref,
        review_policy,
        prior_review_ids,
        attempt_number,
    )
    .map_err(|_| err("", K::InvalidValue))?;
    Ok(ReviewRequest::new(core, requested_at))
}

pub fn encode_review_request(value: &ReviewRequest) -> String {
    let c = value.core();
    let mut w = Writer::default();
    w.begin_object();
    w.key("request_id");
    w.str(c.request_id());
    w.key("task_id");
    w.str(c.task_id());
    w.key("attempt_id");
    w.str(c.attempt_id());
    opt_str(&mut w, "workstream_id", c.workstream_id());
    w.key("baseline_sha");
    w.str(c.baseline_sha().as_str());
    w.key("implementation_sha");
    w.str(c.implementation_sha().as_str());
    opt_str(&mut w, "branch", c.branch());
    w.key("objective");
    w.str(c.objective());
    w.key("acceptance_criteria");
    w.begin_array();
    for cr in c.acceptance_criteria() {
        write_criterion(&mut w, cr);
    }
    w.end_array();
    opt_strs(&mut w, "non_goals", c.non_goals());
    w.key("allowed_write_paths");
    w.strs(c.allowed_write_paths());
    write_refs(&mut w, "architecture_refs", c.architecture_refs());
    write_refs(&mut w, "contract_refs", c.contract_refs());
    if let Some(r) = c.a3_handoff_ref() {
        w.key("a3_handoff_ref");
        write_ref(&mut w, r);
    }
    w.key("assurance_profile");
    w.str(c.assurance_profile().as_str());
    w.key("review_scope");
    w.str(c.review_scope().as_str());
    if let Some(p) = c.review_policy() {
        w.key("review_policy");
        w.begin_object();
        opt_bool(&mut w, "required", p.required());
        opt_str(
            &mut w,
            "distinct_provider",
            p.distinct_provider().map(DistinctProviderPolicy::as_str),
        );
        opt_str(
            &mut w,
            "reviewer_floor",
            p.reviewer_floor().map(ReviewRequestReviewerFloor::as_str),
        );
        opt_strs(&mut w, "excluded_session_refs", p.excluded_session_refs());
        w.end_object();
    }
    opt_strs(&mut w, "prior_review_ids", c.prior_review_ids());
    if let Some(n) = c.attempt_number() {
        w.key("attempt_number");
        w.raw_integer(n.decimal_digits());
    }
    opt_str(
        &mut w,
        "requested_at",
        value.requested_at().map(|t| t.as_str()),
    );
    w.key("context_epoch");
    w.raw_integer(c.context_epoch().decimal_digits());
    w.end_object();
    w.finish()
}

// ---- ReviewCapsule ----------------------------------------------------------

fn capsule_check(v: Json, p: &str) -> Result<ReviewCapsuleCheck, ReviewWireError> {
    let f = check_fields(v, p)?;
    let result = f.result_of(
        &ReviewCapsuleCheckResult::ALL,
        ReviewCapsuleCheckResult::as_str,
    )?;
    Ok(ReviewCapsuleCheck::new_with_timestamps(
        f.core()?,
        result,
        f.started_at,
        f.finished_at,
    ))
}

pub fn decode_review_capsule(text: &str) -> Result<ReviewCapsuleNonTemporalCore, ReviewWireError> {
    let mut o = root(text)?;
    let review_id = o.id("review_id")?;
    let task_id = o.id("task_id")?;
    let attempt_id = o.id("attempt_id")?;
    let baseline_sha = o.sha("baseline_sha")?;
    let implementation_sha = o.sha("implementation_sha")?;
    let objective = o.nonempty("objective")?;
    let acceptance_criteria = criteria(&mut o)?;
    let non_goals = o.opt_strings("non_goals")?;
    let architecture_refs = o.opt_refs("architecture_refs")?;
    let contract_refs = o.opt_refs("contract_refs")?;
    let (dv, dp) = o.req("diff")?;
    let diff = reference(dv, &dp)?;
    let allowed_write_paths = nonempty_strings(&mut o, "allowed_write_paths")?;
    let checks = o
        .opt("checks")
        .map(|(v, p)| array(v, &p, capsule_check))
        .transpose()?;
    let security_requirements = o.opt_strings("security_requirements")?;
    let review_scope = o.enumeration(
        "review_scope",
        &ReviewCapsuleReviewScope::ALL,
        ReviewCapsuleReviewScope::as_str,
    )?;
    let mut sp = o.object("severity_policy")?;
    let blocking = nonempty_strings(&mut sp, "blocking_categories")?;
    let nonblocking = sp.opt_strings("nonblocking_categories")?;
    sp.end()?;
    let severity_policy = ReviewCapsuleSeverityPolicy::new(blocking, nonblocking)
        .map_err(|_| err("/severity_policy", K::InvalidValue))?;
    let reproduction_required = o.boolean("reproduction_required")?;
    let structured_output_schema = o.opt_ref("structured_output_schema")?;
    let context_epoch = epoch(&mut o)?;
    o.end()?;
    ReviewCapsuleNonTemporalCore::new_with_context_epoch(
        review_id,
        task_id,
        attempt_id,
        baseline_sha,
        implementation_sha,
        objective,
        acceptance_criteria,
        non_goals,
        architecture_refs,
        contract_refs,
        diff,
        allowed_write_paths,
        checks,
        security_requirements,
        review_scope,
        severity_policy,
        reproduction_required,
        structured_output_schema,
        context_epoch,
    )
    .map_err(|_| err("", K::InvalidValue))
}

pub fn encode_review_capsule(c: &ReviewCapsuleNonTemporalCore) -> String {
    let mut w = Writer::default();
    w.begin_object();
    w.key("review_id");
    w.str(c.review_id());
    w.key("task_id");
    w.str(c.task_id());
    w.key("attempt_id");
    w.str(c.attempt_id());
    w.key("baseline_sha");
    w.str(c.baseline_sha().as_str());
    w.key("implementation_sha");
    w.str(c.implementation_sha().as_str());
    w.key("objective");
    w.str(c.objective());
    w.key("acceptance_criteria");
    w.begin_array();
    for cr in c.acceptance_criteria() {
        write_criterion(&mut w, cr);
    }
    w.end_array();
    opt_strs(&mut w, "non_goals", c.non_goals());
    write_refs(&mut w, "architecture_refs", c.architecture_refs());
    write_refs(&mut w, "contract_refs", c.contract_refs());
    w.key("diff");
    write_ref(&mut w, c.diff());
    w.key("allowed_write_paths");
    w.strs(c.allowed_write_paths());
    if let Some(checks) = c.checks() {
        w.key("checks");
        w.begin_array();
        for ch in checks {
            write_check(
                &mut w,
                ch.source(),
                ch.command(),
                &ch.exit_code().to_string(),
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
    opt_strs(&mut w, "security_requirements", c.security_requirements());
    w.key("review_scope");
    w.str(c.review_scope().as_str());
    w.key("severity_policy");
    w.begin_object();
    w.key("blocking_categories");
    w.strs(c.severity_policy().blocking_categories());
    opt_strs(
        &mut w,
        "nonblocking_categories",
        c.severity_policy().nonblocking_categories(),
    );
    w.end_object();
    w.key("reproduction_required");
    w.bool(c.reproduction_required());
    if let Some(r) = c.structured_output_schema() {
        w.key("structured_output_schema");
        write_ref(&mut w, r);
    }
    w.key("context_epoch");
    w.raw_integer(c.context_epoch().decimal_digits());
    w.end_object();
    w.finish()
}

// ---- A3Handoff --------------------------------------------------------------

fn handoff_check(v: Json, p: &str) -> Result<A3HandoffCheck, ReviewWireError> {
    let f = check_fields(v, p)?;
    let result = f.result_of(&A3HandoffCheckResult::ALL, A3HandoffCheckResult::as_str)?;
    Ok(A3HandoffCheck::new_with_timestamps(
        f.core()?,
        result,
        f.started_at,
        f.finished_at,
    ))
}

fn contract(v: Json, p: &str) -> Result<A3HandoffContractConsumed, ReviewWireError> {
    let mut o = Obj::new(v, p.into())?;
    let contract_id = o.string("contract_id")?;
    let version = o.string("version")?;
    o.end()?;
    Ok(A3HandoffContractConsumed::new(contract_id, version))
}

fn labeled(v: Json, p: &str) -> Result<A3HandoffLabeledEvidence, ReviewWireError> {
    let mut o = Obj::new(v, p.into())?;
    let claim = o.string("claim")?;
    let label = o.enumeration(
        "label",
        &A3HandoffEvidenceLabel::ALL,
        A3HandoffEvidenceLabel::as_str,
    )?;
    o.end()?;
    Ok(A3HandoffLabeledEvidence::new(claim, label))
}

pub fn decode_a3_handoff(text: &str) -> Result<A3HandoffNonTemporalCore, ReviewWireError> {
    let mut o = root(text)?;
    let task_id = o.id("task_id")?;
    let attempt_id = o.id("attempt_id")?;
    let implementer = o
        .opt_object("implementer")?
        .map(|mut i| {
            let provider_id = i.opt_nonempty("provider_id")?;
            let model_id = i.opt_nonempty("model_id")?;
            let runtime_id = i.opt_nonempty("runtime_id")?;
            let binding_id = i.opt_id("binding_id")?;
            let path = i.path.clone();
            i.end()?;
            A3HandoffImplementer::new(provider_id, model_id, runtime_id, binding_id)
                .map_err(|_| err(&path, K::InvalidValue))
        })
        .transpose()?;
    let start_sha = o.sha("start_sha")?;
    let final_sha = o.sha("final_sha")?;
    let branch = o.string("branch")?;
    let files_changed = o.strings("files_changed")?;
    let files_created = o.opt_strings("files_created")?;
    let files_deleted = o.opt_strings("files_deleted")?;
    let contracts_consumed = o
        .opt("contracts_consumed")
        .map(|(v, p)| array(v, &p, contract))
        .transpose()?;
    let implementation_summary = o.opt_string("implementation_summary")?;
    let (cv, cp) = o.req("checks")?;
    let checks = array(cv, &cp, handoff_check)?;
    let not_run = o.opt_strings("not_run")?;
    let evidence_labels = o
        .opt("evidence_labels")
        .map(|(v, p)| array(v, &p, labeled))
        .transpose()?;
    let known_limitations = o.opt_strings("known_limitations")?;
    let assumptions = o.opt_strings("assumptions")?;
    let security_notes = o.opt_string("security_notes")?;
    let subtask_requests = o
        .opt("subtask_requests")
        .map(|(v, p)| array(v, &p, id))
        .transpose()?;
    let open_questions = o.opt_strings("open_questions")?;
    let evidence_refs = o.opt_refs("evidence_refs")?;
    let ready_for_a4 = o.boolean("ready_for_a4")?;
    let blocker = o.nullable_string("blocker")?.map(|b| match b {
        None => A3HandoffBlocker::ExplicitNull,
        Some(t) => A3HandoffBlocker::Text(t),
    });
    o.end()?;
    A3HandoffNonTemporalCore::new(
        task_id,
        attempt_id,
        start_sha,
        final_sha,
        branch,
        files_changed,
        checks,
        ready_for_a4,
        implementer,
        files_created,
        files_deleted,
        contracts_consumed,
        implementation_summary,
        not_run,
        evidence_labels,
        known_limitations,
        assumptions,
        security_notes,
        subtask_requests,
        open_questions,
        evidence_refs,
        blocker,
    )
    .map_err(|_| err("", K::InvalidValue))
}

pub fn encode_a3_handoff(c: &A3HandoffNonTemporalCore) -> String {
    let mut w = Writer::default();
    w.begin_object();
    w.key("task_id");
    w.str(c.task_id());
    w.key("attempt_id");
    w.str(c.attempt_id());
    if let Some(i) = c.implementer() {
        w.key("implementer");
        w.begin_object();
        opt_str(&mut w, "provider_id", i.provider_id());
        opt_str(&mut w, "model_id", i.model_id());
        opt_str(&mut w, "runtime_id", i.runtime_id());
        opt_str(&mut w, "binding_id", i.binding_id());
        w.end_object();
    }
    w.key("start_sha");
    w.str(c.start_sha().as_str());
    w.key("final_sha");
    w.str(c.final_sha().as_str());
    w.key("branch");
    w.str(c.branch());
    w.key("files_changed");
    w.strs(c.files_changed());
    opt_strs(&mut w, "files_created", c.files_created());
    opt_strs(&mut w, "files_deleted", c.files_deleted());
    if let Some(cs) = c.contracts_consumed() {
        w.key("contracts_consumed");
        w.begin_array();
        for x in cs {
            w.begin_object();
            w.key("contract_id");
            w.str(x.contract_id());
            w.key("version");
            w.str(x.version());
            w.end_object();
        }
        w.end_array();
    }
    opt_str(&mut w, "implementation_summary", c.implementation_summary());
    w.key("checks");
    w.begin_array();
    for ch in c.checks() {
        write_check(
            &mut w,
            ch.source(),
            ch.command(),
            &ch.exit_code().to_string(),
            ch.code_sha().as_str(),
            ch.timed_out(),
            ch.started_at(),
            ch.finished_at(),
            ch.output_ref(),
            ch.result().map(A3HandoffCheckResult::as_str),
        );
    }
    w.end_array();
    opt_strs(&mut w, "not_run", c.not_run());
    if let Some(ls) = c.evidence_labels() {
        w.key("evidence_labels");
        w.begin_array();
        for l in ls {
            w.begin_object();
            w.key("claim");
            w.str(l.claim());
            w.key("label");
            w.str(l.label().as_str());
            w.end_object();
        }
        w.end_array();
    }
    opt_strs(&mut w, "known_limitations", c.known_limitations());
    opt_strs(&mut w, "assumptions", c.assumptions());
    opt_str(&mut w, "security_notes", c.security_notes());
    opt_strs(&mut w, "subtask_requests", c.subtask_requests());
    opt_strs(&mut w, "open_questions", c.open_questions());
    write_refs(&mut w, "evidence_refs", c.evidence_refs());
    w.key("ready_for_a4");
    w.bool(c.ready_for_a4());
    match c.blocker() {
        None => {}
        Some(A3HandoffBlocker::ExplicitNull) => {
            w.key("blocker");
            w.null();
        }
        Some(A3HandoffBlocker::Text(t)) => {
            w.key("blocker");
            w.str(t);
        }
    }
    w.end_object();
    w.finish()
}
