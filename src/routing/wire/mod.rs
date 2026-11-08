//! Strict JSON codecs for the seven Model-Routing schemas (MR-WIRE):
//! RoutingRequest, RoutingDecision, AvailabilityState, QuotaState,
//! ModelCapability, ModelObservation and ProviderPolicyEligibility.
//!
//! Decoding refuses non-JSON and trailing input ([`RoutingWireError::Json`]),
//! anything outside the schema's shape — unknown, duplicate, missing,
//! wrongly typed or wrongly null fields, enum values, length/minimum bounds
//! ([`RoutingWireError::Shape`]) — and schema-valid input the operational
//! carrier cannot hold ([`RoutingWireError::Refused`], e.g. a timestamp that is
//! not RFC 3339, an integer written as `1.0`, a missing `occurred_at`).
//! The four record families State persists (RoutingDecision, ModelCapability,
//! ModelObservation, AvailabilityState) are also admitted by State's strict
//! schema validator, which refuses duplicate keys at every depth.
//!
//! Absent and explicit null stay distinct. Numbers are bounded carriers
//! (u64/i64/finite f64), not arbitrary-precision lexemes; encoding writes
//! sorted keys, so `encode(decode(canonical)) == canonical`. Decoding grants
//! no routing, dispatch or persistence authority.

use serde::de::{Deserializer, IgnoredAny};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Number, Value};

use crate::intelligence::LifecycleState;
use crate::policy_eligibility::{
    ModelRoutingDateTimeV1, PolicyEvidenceLabel, PolicyStatus, ProviderPolicyEligibility,
    TechnicalStatus,
};
use crate::registry::{ModelCapabilityRecord, SubjectKind};
use crate::{
    AlternativeCandidate, AvailabilitySignalSource, AvailabilityState, AvailabilityStateKind,
    AvailabilityStateNonTemporalCore, CapabilityEvidenceNonTemporalCore, DecisionConfidence,
    DifficultyHint, EstimatedCostClass, EvidenceConfidence, EvidenceSourceRef,
    EvidenceSourceRefType, ModelObservation, ModelObservationOutcome, QuotaScope, QuotaState,
    QuotaStateRequiredCore, QuotaVisibility, RegistryFreshness, RoutingDecisionMode,
    RoutingDecisionNonTemporalCore, RoutingDecisionOutcome, RoutingPriority, RoutingQualityFloor,
    RoutingRequest, RoutingRequestConstraints, RoutingRequestNonTemporalCore, RoutingRequestRole,
    RoutingScoreComponents, RoutingTaskClass, SelectionReason,
};
use receipts_state::{DecisionRecordKind, DecisionRecordV1};

#[cfg(test)]
mod tests;

/// Why a document was refused. Field names come from the schemas; input
/// values are never echoed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingWireError {
    /// Not exactly one JSON value.
    Json,
    /// Not the named schema's shape.
    Shape(&'static str),
    /// Schema-valid, but the named field is outside the operational carrier.
    Refused(&'static str),
}

impl std::fmt::Display for RoutingWireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json => f.write_str("input is not one JSON value"),
            Self::Shape(schema) => write!(f, "{schema} schema shape refused"),
            Self::Refused(field) => write!(f, "schema-valid {field} is outside the carrier"),
        }
    }
}

impl std::error::Error for RoutingWireError {}

type R<T> = Result<T, RoutingWireError>;

// ---- decode helpers --------------------------------------------------------

/// Present-but-nullable: `None` absent, `Some(None)` null.
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

/// Optional but never null: explicit null fails the field.
fn non_null<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

/// Any JSON value, keeping explicit null distinct from absence.
fn any_value<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

fn parse<T: DeserializeOwned>(json: &str, schema: &'static str) -> R<T> {
    serde_json::from_str::<IgnoredAny>(json).map_err(|_| RoutingWireError::Json)?;
    serde_json::from_str(json).map_err(|_| RoutingWireError::Shape(schema))
}

/// State's complete schema admission for the families it persists.
fn state_admits(kind: DecisionRecordKind, id: &str, json: &str, schema: &'static str) -> R<()> {
    DecisionRecordV1::parse("wire-codec", kind, id, json)
        .map(|_| ())
        .map_err(|_| RoutingWireError::Shape(schema))
}

fn pick<T: Copy>(all: &[T], name: fn(T) -> &'static str, text: &str, schema: &'static str) -> R<T> {
    all.iter()
        .copied()
        .find(|v| name(*v) == text)
        .ok_or(RoutingWireError::Shape(schema))
}

fn time(text: String, field: &'static str) -> R<ModelRoutingDateTimeV1> {
    ModelRoutingDateTimeV1::try_new(text).map_err(|_| RoutingWireError::Refused(field))
}

fn length_ok(value: &str, min: usize, max: Option<usize>) -> bool {
    let n = value.chars().count();
    n >= min && max.is_none_or(|max| n <= max)
}

/// A JSON-Schema integer is a number with no fractional part; anything else
/// is a shape error. An integral value the carrier cannot hold exactly
/// (`1.0`, beyond the u64/i64 range) is refused instead.
fn integral(value: &Value, schema: &'static str) -> R<()> {
    match value.as_f64() {
        Some(f) if value.is_number() && f.fract() == 0.0 => Ok(()),
        _ => Err(RoutingWireError::Shape(schema)),
    }
}

/// `minimum: 0` integer (negatives are a shape error).
fn bounded_u64(value: &Value, schema: &'static str, field: &'static str) -> R<u64> {
    integral(value, schema)?;
    if value.as_f64().is_some_and(|f| f < 0.0) {
        return Err(RoutingWireError::Shape(schema));
    }
    value.as_u64().ok_or(RoutingWireError::Refused(field))
}

// ---- encode helpers --------------------------------------------------------

struct Obj(Map<String, Value>);
impl Obj {
    fn new() -> Self {
        Self(Map::new())
    }
    fn put(&mut self, key: &str, value: impl Into<Value>) -> &mut Self {
        self.0.insert(key.to_owned(), value.into());
        self
    }
    fn opt(&mut self, key: &str, value: Option<impl Into<Value>>) -> &mut Self {
        if let Some(value) = value {
            self.put(key, value);
        }
        self
    }
    fn done(&mut self) -> Value {
        Value::Object(std::mem::take(&mut self.0))
    }
}

fn num(value: f64) -> Value {
    Value::Number(Number::from_f64(value).expect("carriers hold finite numbers only"))
}
fn null_or<T>(value: Option<T>, f: impl FnOnce(T) -> Value) -> Value {
    value.map_or(Value::Null, f)
}
fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}
fn strings(values: &[String]) -> Value {
    Value::Array(values.iter().map(|v| text(v)).collect())
}

// ---- RoutingRequest --------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestDto {
    request_id: String,
    #[serde(default, deserialize_with = "non_null")]
    task_id: Option<String>,
    role: String,
    task_class: String,
    quality_floor: String,
    required_capabilities: Vec<String>,
    #[serde(default, deserialize_with = "non_null")]
    preferred_capabilities: Option<Vec<String>>,
    #[serde(default, deserialize_with = "non_null")]
    quality_priority: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    cost_priority: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    constraints: Option<ConstraintsDto>,
    #[serde(default, deserialize_with = "nullable")]
    context_size_hint: Option<Option<Value>>,
    #[serde(default, deserialize_with = "nullable")]
    deadline: Option<Option<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConstraintsDto {
    #[serde(default, deserialize_with = "non_null")]
    distinct_provider_from: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    avoid_providers: Option<Vec<String>>,
    #[serde(default, deserialize_with = "non_null")]
    user_pinned_model: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    user_pinned_provider: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    max_cost: Option<Option<f64>>,
}

const REQ: &str = "RoutingRequest";

pub fn decode_routing_request(json: &str) -> R<RoutingRequest> {
    let d: RequestDto = parse(json, REQ)?;
    let shape = RoutingWireError::Shape(REQ);
    if !length_ok(&d.request_id, 1, Some(200))
        || d.task_id
            .as_deref()
            .is_some_and(|t| !length_ok(t, 1, Some(200)))
        || d.required_capabilities.is_empty()
    {
        return Err(shape);
    }
    let priority = |p: Option<String>| -> R<Option<RoutingPriority>> {
        p.map(|p| pick(&RoutingPriority::ALL, RoutingPriority::as_str, &p, REQ))
            .transpose()
    };
    let constraints = d
        .constraints
        .map(|c| -> R<RoutingRequestConstraints> {
            let ids = [
                &c.distinct_provider_from,
                &c.user_pinned_model,
                &c.user_pinned_provider,
            ];
            if ids.iter().any(|v| v.as_deref() == Some(""))
                || c.avoid_providers
                    .as_ref()
                    .is_some_and(|v| v.iter().any(String::is_empty))
            {
                return Err(shape);
            }
            RoutingRequestConstraints::try_new(
                c.distinct_provider_from,
                c.avoid_providers,
                c.user_pinned_model,
                c.user_pinned_provider,
                c.max_cost,
            )
            .map_err(|_| RoutingWireError::Refused("constraints"))
        })
        .transpose()?;
    let context_size_hint = match d.context_size_hint {
        None => None,
        Some(None) => Some(None),
        Some(Some(v)) => Some(Some(bounded_u64(&v, REQ, "context_size_hint")?)),
    };
    let core = RoutingRequestNonTemporalCore::try_new(
        d.request_id,
        d.task_id,
        pick(
            &RoutingRequestRole::ALL,
            RoutingRequestRole::as_str,
            &d.role,
            REQ,
        )?,
        pick(
            &RoutingTaskClass::ALL,
            RoutingTaskClass::as_str,
            &d.task_class,
            REQ,
        )?,
        pick(
            &RoutingQualityFloor::ALL,
            RoutingQualityFloor::as_str,
            &d.quality_floor,
            REQ,
        )?,
        d.required_capabilities,
        d.preferred_capabilities,
        priority(d.quality_priority)?,
        priority(d.cost_priority)?,
        constraints,
        context_size_hint,
    )
    .map_err(|_| RoutingWireError::Refused("required_capabilities"))?;
    let deadline = match d.deadline {
        None => None,
        Some(None) => Some(None),
        Some(Some(t)) => Some(Some(time(t, "deadline")?)),
    };
    Ok(RoutingRequest::new(core, deadline))
}

pub fn encode_routing_request(request: &RoutingRequest) -> String {
    let c = request.core();
    let mut o = Obj::new();
    o.put("request_id", c.request_id())
        .opt("task_id", c.task_id())
        .put("role", c.role().as_str())
        .put("task_class", c.task_class().as_str())
        .put("quality_floor", c.quality_floor().as_str())
        .put("required_capabilities", strings(c.required_capabilities()))
        .opt(
            "preferred_capabilities",
            c.preferred_capabilities().map(strings),
        )
        .opt("quality_priority", c.quality_priority().map(|p| p.as_str()))
        .opt("cost_priority", c.cost_priority().map(|p| p.as_str()))
        .opt(
            "context_size_hint",
            c.context_size_hint().map(|v| null_or(v, Value::from)),
        )
        .opt(
            "deadline",
            request.deadline().map(|v| null_or(v, |t| text(t.as_str()))),
        );
    if let Some(k) = c.constraints() {
        let mut co = Obj::new();
        co.opt("distinct_provider_from", k.distinct_provider_from())
            .opt("avoid_providers", k.avoid_providers().map(strings))
            .opt("user_pinned_model", k.user_pinned_model())
            .opt("user_pinned_provider", k.user_pinned_provider())
            .opt("max_cost", k.max_cost().map(|v| null_or(v, num)));
        o.put("constraints", co.done());
    }
    o.done().to_string()
}

// ---- shared evidence pieces --------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRefDto {
    ref_type: String,
    target: String,
    #[serde(default, deserialize_with = "non_null")]
    digest: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    section: Option<String>,
}

fn source_ref(d: SourceRefDto, schema: &'static str) -> R<EvidenceSourceRef> {
    let ref_type = pick(
        &EvidenceSourceRefType::ALL,
        EvidenceSourceRefType::as_str,
        &d.ref_type,
        schema,
    )?;
    EvidenceSourceRef::try_new(ref_type, d.target, d.digest, d.section)
        .map_err(|_| RoutingWireError::Shape(schema))
}

fn encode_source_ref(s: &EvidenceSourceRef) -> Value {
    Obj::new()
        .put("ref_type", s.ref_type().as_str())
        .put("target", s.target())
        .opt("digest", s.digest())
        .opt("section", s.section())
        .done()
}

fn confidence(text: &str, schema: &'static str) -> R<EvidenceConfidence> {
    pick(
        &EvidenceConfidence::ALL,
        EvidenceConfidence::as_str,
        text,
        schema,
    )
}

/// `minimum: 0` integer, nullable; absent/null kept.
fn non_negative(
    v: Option<Option<Value>>,
    schema: &'static str,
    field: &'static str,
) -> R<Option<Option<u64>>> {
    match v {
        None => Ok(None),
        Some(None) => Ok(Some(None)),
        Some(Some(v)) => Ok(Some(Some(bounded_u64(&v, schema, field)?))),
    }
}

/// Signed nullable integer with no minimum.
fn signed(
    v: Option<Option<Value>>,
    schema: &'static str,
    field: &'static str,
) -> R<Option<Option<i64>>> {
    match v {
        None => Ok(None),
        Some(None) => Ok(Some(None)),
        Some(Some(v)) => {
            integral(&v, schema)?;
            v.as_i64()
                .map(|n| Some(Some(n)))
                .ok_or(RoutingWireError::Refused(field))
        }
    }
}

// ---- RoutingDecision --------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecisionDto {
    decision_id: String,
    request_id: String,
    #[serde(default, deserialize_with = "non_null")]
    task_id: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    occurred_at: Option<String>,
    outcome: String,
    #[serde(default, deserialize_with = "non_null")]
    selected_provider: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    selected_model: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    selected_runtime: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    selection_reason: Option<ReasonDto>,
    #[serde(default, deserialize_with = "non_null")]
    capability_evidence: Option<Vec<EvidenceDto>>,
    #[serde(default, deserialize_with = "non_null")]
    confidence: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    availability_at_decision: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    estimated_cost_class: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    alternative_candidates: Option<Vec<AlternativeDto>>,
    registry_freshness: FreshnessDto,
    mode: String,
    #[serde(default, deserialize_with = "non_null")]
    user_involved: Option<bool>,
    #[serde(default, deserialize_with = "non_null")]
    user_pin_applied: Option<bool>,
    #[serde(default, deserialize_with = "non_null")]
    fallback_from: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReasonDto {
    #[serde(default, deserialize_with = "non_null")]
    quality_floor_applied: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    hard_filters_passed: Option<Vec<String>>,
    #[serde(default, deserialize_with = "non_null")]
    score_components: Option<ScoreDto>,
    #[serde(default, deserialize_with = "non_null")]
    decisive_factor: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScoreDto {
    #[serde(default, deserialize_with = "nullable")]
    expected_implementation_cost: Option<Option<f64>>,
    #[serde(default, deserialize_with = "nullable")]
    expected_rejection_probability: Option<Option<f64>>,
    #[serde(default, deserialize_with = "nullable")]
    expected_repair_cost: Option<Option<f64>>,
    #[serde(default, deserialize_with = "nullable")]
    expected_review_cost: Option<Option<f64>>,
    #[serde(default, deserialize_with = "nullable")]
    latency_estimate_seconds: Option<Option<f64>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvidenceDto {
    capability: String,
    #[serde(default, deserialize_with = "any_value")]
    value: Option<Value>,
    confidence: String,
    #[serde(default, deserialize_with = "non_null")]
    source_ref: Option<SourceRefDto>,
    #[serde(default, deserialize_with = "non_null")]
    observed_at: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    sample_size: Option<Option<Value>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AlternativeDto {
    provider_id: String,
    model_id: String,
    runtime_id: String,
    #[serde(default, deserialize_with = "nullable")]
    score: Option<Option<f64>>,
    rejection_reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FreshnessDto {
    #[serde(default, deserialize_with = "nullable")]
    model_list_age_seconds: Option<Option<Value>>,
    #[serde(default, deserialize_with = "nullable")]
    capability_age_seconds: Option<Option<Value>>,
    #[serde(default, deserialize_with = "nullable")]
    calibration_sample_size: Option<Option<Value>>,
    stale: bool,
}

const DEC: &str = "RoutingDecision";

pub fn decode_routing_decision(json: &str) -> R<RoutingDecisionNonTemporalCore> {
    let d: DecisionDto = parse(json, DEC)?;
    state_admits(
        DecisionRecordKind::RoutingDecision,
        &d.decision_id,
        json,
        DEC,
    )?;
    let reason = d
        .selection_reason
        .map(|r| -> R<SelectionReason> {
            let floor = r
                .quality_floor_applied
                .map(|f| {
                    pick(
                        &RoutingQualityFloor::ALL,
                        RoutingQualityFloor::as_str,
                        &f,
                        DEC,
                    )
                })
                .transpose()?;
            let score = r
                .score_components
                .map(|s| {
                    RoutingScoreComponents::try_new(
                        s.expected_implementation_cost,
                        s.expected_rejection_probability,
                        s.expected_repair_cost,
                        s.expected_review_cost,
                        s.latency_estimate_seconds,
                    )
                    .map_err(|_| RoutingWireError::Shape(DEC))
                })
                .transpose()?;
            Ok(SelectionReason::new(
                floor,
                r.hard_filters_passed,
                score,
                r.decisive_factor,
            ))
        })
        .transpose()?;
    let evidence = d
        .capability_evidence
        .map(|items| {
            items
                .into_iter()
                .map(|e| -> R<CapabilityEvidenceNonTemporalCore> {
                    let observed_at = e
                        .observed_at
                        .ok_or(RoutingWireError::Refused("capability_evidence.observed_at"))?;
                    let core = CapabilityEvidenceNonTemporalCore::new(
                        time(observed_at, "capability_evidence.observed_at")?,
                        e.capability,
                        confidence(&e.confidence, DEC)?,
                        e.source_ref.map(|s| source_ref(s, DEC)).transpose()?,
                        non_negative(e.sample_size, DEC, "sample_size")?,
                    );
                    Ok(match e.value {
                        Some(v) => core.with_value(v),
                        None => core,
                    })
                })
                .collect::<R<Vec<_>>>()
        })
        .transpose()?;
    let alternatives = d
        .alternative_candidates
        .map(|items| {
            items
                .into_iter()
                .map(|a| {
                    AlternativeCandidate::try_new(
                        a.provider_id,
                        a.model_id,
                        a.runtime_id,
                        a.score,
                        a.rejection_reason,
                    )
                    .map_err(|_| RoutingWireError::Shape(DEC))
                })
                .collect::<R<Vec<_>>>()
        })
        .transpose()?;
    let f = d.registry_freshness;
    let freshness = RegistryFreshness::new(
        signed(f.model_list_age_seconds, DEC, "model_list_age_seconds")?,
        signed(f.capability_age_seconds, DEC, "capability_age_seconds")?,
        signed(f.calibration_sample_size, DEC, "calibration_sample_size")?,
        f.stale,
    );
    let occurred_at = d
        .occurred_at
        .ok_or(RoutingWireError::Refused("occurred_at"))?;
    RoutingDecisionNonTemporalCore::try_new(
        time(occurred_at, "occurred_at")?,
        d.decision_id,
        d.request_id,
        d.task_id,
        pick(
            &RoutingDecisionOutcome::ALL,
            RoutingDecisionOutcome::as_str,
            &d.outcome,
            DEC,
        )?,
        d.selected_provider,
        d.selected_model,
        d.selected_runtime,
        reason,
        evidence,
        d.confidence
            .map(|c| {
                pick(
                    &DecisionConfidence::ALL,
                    DecisionConfidence::as_str,
                    &c,
                    DEC,
                )
            })
            .transpose()?,
        d.availability_at_decision,
        d.estimated_cost_class
            .map(|c| {
                pick(
                    &EstimatedCostClass::ALL,
                    EstimatedCostClass::as_str,
                    &c,
                    DEC,
                )
            })
            .transpose()?,
        alternatives,
        freshness,
        pick(
            &RoutingDecisionMode::ALL,
            RoutingDecisionMode::as_str,
            &d.mode,
            DEC,
        )?,
        d.user_involved,
        d.user_pin_applied,
        d.fallback_from,
    )
    .map_err(|_| RoutingWireError::Shape(DEC))
}

fn encode_evidence(e: &CapabilityEvidenceNonTemporalCore) -> Value {
    Obj::new()
        .put("capability", e.capability())
        .opt("value", e.value().cloned())
        .put("confidence", e.confidence().as_str())
        .opt("source_ref", e.source_ref().map(encode_source_ref))
        .put("observed_at", e.observed_at().as_str())
        .opt(
            "sample_size",
            e.sample_size().map(|v| null_or(v, Value::from)),
        )
        .done()
}

pub fn encode_routing_decision(d: &RoutingDecisionNonTemporalCore) -> String {
    let i64s = |v: Option<Option<i64>>| v.map(|v| null_or(v, Value::from));
    let f64s = |v: Option<Option<f64>>| v.map(|v| null_or(v, num));
    let mut o = Obj::new();
    o.put("decision_id", d.decision_id())
        .put("request_id", d.request_id())
        .opt("task_id", d.task_id())
        .put("occurred_at", d.occurred_at().as_str())
        .put("outcome", d.outcome().as_str())
        .opt("selected_provider", d.selected_provider())
        .opt("selected_model", d.selected_model())
        .opt("selected_runtime", d.selected_runtime())
        .opt(
            "capability_evidence",
            d.capability_evidence()
                .map(|items| Value::Array(items.iter().map(encode_evidence).collect())),
        )
        .opt("confidence", d.confidence().map(|c| c.as_str()))
        .opt("availability_at_decision", d.availability_at_decision())
        .opt(
            "estimated_cost_class",
            d.estimated_cost_class().map(|c| c.as_str()),
        )
        .opt(
            "alternative_candidates",
            d.alternative_candidates().map(|items| {
                Value::Array(
                    items
                        .iter()
                        .map(|a| {
                            Obj::new()
                                .put("provider_id", a.provider_id())
                                .put("model_id", a.model_id())
                                .put("runtime_id", a.runtime_id())
                                .opt("score", f64s(a.score()))
                                .put("rejection_reason", a.rejection_reason())
                                .done()
                        })
                        .collect(),
                )
            }),
        )
        .put("mode", d.mode().as_str())
        .opt("user_involved", d.user_involved())
        .opt("user_pin_applied", d.user_pin_applied())
        .opt("fallback_from", d.fallback_from());
    if let Some(r) = d.selection_reason() {
        let mut ro = Obj::new();
        ro.opt(
            "quality_floor_applied",
            r.quality_floor_applied().map(|f| f.as_str()),
        )
        .opt("hard_filters_passed", r.hard_filters_passed().map(strings))
        .opt("decisive_factor", r.decisive_factor());
        if let Some(s) = r.score_components() {
            let so = Obj::new()
                .opt(
                    "expected_implementation_cost",
                    f64s(s.expected_implementation_cost()),
                )
                .opt(
                    "expected_rejection_probability",
                    f64s(s.expected_rejection_probability()),
                )
                .opt("expected_repair_cost", f64s(s.expected_repair_cost()))
                .opt("expected_review_cost", f64s(s.expected_review_cost()))
                .opt(
                    "latency_estimate_seconds",
                    f64s(s.latency_estimate_seconds()),
                )
                .done();
            ro.put("score_components", so);
        }
        o.put("selection_reason", ro.done());
    }
    let f = d.registry_freshness();
    let fo = Obj::new()
        .opt("model_list_age_seconds", i64s(f.model_list_age_seconds()))
        .opt("capability_age_seconds", i64s(f.capability_age_seconds()))
        .opt("calibration_sample_size", i64s(f.calibration_sample_size()))
        .put("stale", f.stale())
        .done();
    o.put("registry_freshness", fo);
    o.done().to_string()
}

// ---- AvailabilityState ------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AvailabilityDto {
    provider_id: String,
    #[serde(default, deserialize_with = "non_null")]
    model_id: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    runtime_id: Option<String>,
    state: String,
    #[serde(default, deserialize_with = "nullable")]
    retry_after_seconds: Option<Option<Value>>,
    #[serde(default, deserialize_with = "non_null")]
    signal_source: Option<String>,
    observed_at: String,
    #[serde(default, deserialize_with = "non_null")]
    note: Option<String>,
}

const AVA: &str = "AvailabilityState";

pub fn decode_availability_state(json: &str) -> R<AvailabilityState> {
    let d: AvailabilityDto = parse(json, AVA)?;
    state_admits(
        DecisionRecordKind::AvailabilityObservation,
        "wire",
        json,
        AVA,
    )?;
    let core = AvailabilityStateNonTemporalCore::try_new(
        d.provider_id,
        d.model_id,
        d.runtime_id,
        pick(
            &AvailabilityStateKind::ALL,
            AvailabilityStateKind::as_str,
            &d.state,
            AVA,
        )?,
        non_negative(d.retry_after_seconds, AVA, "retry_after_seconds")?,
        d.signal_source
            .map(|s| {
                pick(
                    &AvailabilitySignalSource::ALL,
                    AvailabilitySignalSource::as_str,
                    &s,
                    AVA,
                )
            })
            .transpose()?,
        d.note,
    )
    .map_err(|_| RoutingWireError::Shape(AVA))?;
    Ok(AvailabilityState::new(
        core,
        time(d.observed_at, "observed_at")?,
    ))
}

pub fn encode_availability_state(a: &AvailabilityState) -> String {
    let c = a.core();
    Obj::new()
        .put("provider_id", c.provider_id())
        .opt("model_id", c.model_id())
        .opt("runtime_id", c.runtime_id())
        .put("state", c.state().as_str())
        .opt(
            "retry_after_seconds",
            c.retry_after_seconds().map(|v| null_or(v, Value::from)),
        )
        .opt("signal_source", c.signal_source().map(|s| s.as_str()))
        .put("observed_at", a.observed_at().as_str())
        .opt("note", c.note())
        .done()
        .to_string()
}

// ---- QuotaState --------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuotaDto {
    provider_id: String,
    #[serde(default, deserialize_with = "non_null")]
    model_id: Option<String>,
    quota_scope: String,
    #[serde(default, deserialize_with = "nullable")]
    limit: Option<Option<f64>>,
    #[serde(default, deserialize_with = "nullable")]
    remaining: Option<Option<f64>>,
    #[serde(default, deserialize_with = "nullable")]
    resets_at: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable")]
    retry_after_seconds: Option<Option<Value>>,
    #[serde(default, deserialize_with = "non_null")]
    visibility: Option<String>,
    observed_at: String,
}

const QUO: &str = "QuotaState";

pub fn decode_quota_state(json: &str) -> R<QuotaState> {
    let d: QuotaDto = parse(json, QUO)?;
    let provider = crate::registry::ProviderId::try_new(d.provider_id)
        .map_err(|_| RoutingWireError::Shape(QUO))?;
    let core = QuotaStateRequiredCore::new(
        provider,
        pick(&QuotaScope::ALL, QuotaScope::as_str, &d.quota_scope, QUO)?,
        time(d.observed_at, "observed_at")?,
    );
    let resets_at = match d.resets_at {
        None => None,
        Some(None) => Some(None),
        Some(Some(t)) => Some(Some(time(t, "resets_at")?)),
    };
    QuotaState::try_new(
        core,
        d.model_id,
        d.limit,
        d.remaining,
        resets_at,
        non_negative(d.retry_after_seconds, QUO, "retry_after_seconds")?,
        d.visibility
            .map(|v| pick(&QuotaVisibility::ALL, QuotaVisibility::as_str, &v, QUO))
            .transpose()?,
    )
    .map_err(|_| RoutingWireError::Shape(QUO))
}

pub fn encode_quota_state(q: &QuotaState) -> String {
    let c = q.core();
    Obj::new()
        .put("provider_id", c.provider_id().as_str())
        .opt("model_id", q.model_id())
        .put("quota_scope", c.quota_scope().as_str())
        .opt("limit", q.limit().map(|v| null_or(v, num)))
        .opt("remaining", q.remaining().map(|v| null_or(v, num)))
        .opt(
            "resets_at",
            q.resets_at().map(|v| null_or(v, |t| text(t.as_str()))),
        )
        .opt(
            "retry_after_seconds",
            q.retry_after_seconds().map(|v| null_or(v, Value::from)),
        )
        .opt("visibility", q.visibility().map(|v| v.as_str()))
        .put("observed_at", c.observed_at().as_str())
        .done()
        .to_string()
}

// ---- ModelCapability ----------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CapabilityDto {
    subject_kind: String,
    #[serde(default, deserialize_with = "non_null")]
    provider_id: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    model_id: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    runtime_id: Option<String>,
    capability: String,
    #[serde(default, deserialize_with = "any_value")]
    value: Option<Value>,
    confidence: String,
    #[serde(default, deserialize_with = "non_null")]
    source_ref: Option<SourceRefDto>,
    observed_at: String,
    #[serde(default, deserialize_with = "nullable")]
    sample_size: Option<Option<Value>>,
    #[serde(default, deserialize_with = "non_null")]
    lifecycle_state: Option<String>,
}

const CAP: &str = "ModelCapability";

pub fn decode_model_capability(json: &str) -> R<ModelCapabilityRecord> {
    let d: CapabilityDto = parse(json, CAP)?;
    state_admits(DecisionRecordKind::RegistryCapability, "wire", json, CAP)?;
    ModelCapabilityRecord::try_new(
        pick(&SubjectKind::ALL, SubjectKind::as_str, &d.subject_kind, CAP)?,
        d.provider_id,
        d.model_id,
        d.runtime_id,
        d.capability,
        d.value,
        confidence(&d.confidence, CAP)?,
        d.source_ref.map(|s| source_ref(s, CAP)).transpose()?,
        time(d.observed_at, "observed_at")?,
        non_negative(d.sample_size, CAP, "sample_size")?,
        d.lifecycle_state
            .map(|l| pick(&LifecycleState::ALL, LifecycleState::as_str, &l, CAP))
            .transpose()?,
    )
    .map_err(|_| RoutingWireError::Shape(CAP))
}

pub fn encode_model_capability(c: &ModelCapabilityRecord) -> String {
    Obj::new()
        .put("subject_kind", c.subject_kind().as_str())
        .opt("provider_id", c.provider_id())
        .opt("model_id", c.model_id())
        .opt("runtime_id", c.runtime_id())
        .put("capability", c.capability())
        .opt("value", c.value().cloned())
        .put("confidence", c.confidence().as_str())
        .opt("source_ref", c.source_ref().map(encode_source_ref))
        .put("observed_at", c.observed_at().as_str())
        .opt(
            "sample_size",
            c.sample_size().map(|v| null_or(v, Value::from)),
        )
        .opt("lifecycle_state", c.lifecycle_state().map(|l| l.as_str()))
        .done()
        .to_string()
}

// ---- ModelObservation ----------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ObservationDto {
    observation_id: String,
    provider_id: String,
    model_id: String,
    runtime_id: String,
    #[serde(default, deserialize_with = "non_null")]
    task_id: Option<String>,
    task_class: String,
    outcome: String,
    #[serde(default, deserialize_with = "non_null")]
    repair_count: Option<Value>,
    #[serde(default, deserialize_with = "nullable")]
    cost: Option<Option<f64>>,
    #[serde(default, deserialize_with = "nullable")]
    tokens: Option<Option<Value>>,
    #[serde(default, deserialize_with = "nullable")]
    latency_seconds: Option<Option<f64>>,
    #[serde(default, deserialize_with = "nullable")]
    cost_to_accepted_result: Option<Option<f64>>,
    #[serde(default, deserialize_with = "non_null")]
    difficulty_hint: Option<String>,
    #[serde(default, deserialize_with = "non_null")]
    excluded_from_calibration: Option<bool>,
    observed_at: String,
}

const OBS: &str = "ModelObservation";

pub fn decode_model_observation(json: &str) -> R<ModelObservation> {
    let d: ObservationDto = parse(json, OBS)?;
    state_admits(
        DecisionRecordKind::ModelObservation,
        &d.observation_id,
        json,
        OBS,
    )?;
    let repair_count = d
        .repair_count
        .map(|v| bounded_u64(&v, OBS, "repair_count"))
        .transpose()?;
    ModelObservation::try_new(
        d.observation_id,
        d.provider_id,
        d.model_id,
        d.runtime_id,
        d.task_id,
        d.task_class,
        pick(
            &ModelObservationOutcome::ALL,
            ModelObservationOutcome::as_str,
            &d.outcome,
            OBS,
        )?,
        repair_count,
        d.cost,
        signed(d.tokens, OBS, "tokens")?,
        d.latency_seconds,
        d.cost_to_accepted_result,
        d.difficulty_hint
            .map(|h| pick(&DifficultyHint::ALL, DifficultyHint::as_str, &h, OBS))
            .transpose()?,
        d.excluded_from_calibration,
        time(d.observed_at, "observed_at")?,
    )
    .map_err(|_| RoutingWireError::Shape(OBS))
}

pub fn encode_model_observation(o: &ModelObservation) -> String {
    let f64s = |v: Option<Option<f64>>| v.map(|v| null_or(v, num));
    Obj::new()
        .put("observation_id", o.observation_id())
        .put("provider_id", o.provider_id())
        .put("model_id", o.model_id())
        .put("runtime_id", o.runtime_id())
        .opt("task_id", o.task_id())
        .put("task_class", o.task_class())
        .put("outcome", o.outcome().as_str())
        .opt("repair_count", o.repair_count())
        .opt("cost", f64s(o.cost()))
        .opt("tokens", o.tokens().map(|v| null_or(v, Value::from)))
        .opt("latency_seconds", f64s(o.latency_seconds()))
        .opt("cost_to_accepted_result", f64s(o.cost_to_accepted_result()))
        .opt("difficulty_hint", o.difficulty_hint().map(|h| h.as_str()))
        .opt("excluded_from_calibration", o.excluded_from_calibration())
        .put("observed_at", o.observed_at().as_str())
        .done()
        .to_string()
}

// ---- ProviderPolicyEligibility ------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PolicyDto {
    provider_id: String,
    runtime_id: String,
    credential_mode: String,
    technical_status: String,
    policy_status: String,
    #[serde(default, deserialize_with = "non_null")]
    allowed_execution_contexts: Option<Vec<String>>,
    verified_at: String,
    #[serde(default, deserialize_with = "non_null")]
    evidence_source: Option<String>,
    evidence_label: String,
    #[serde(default, deserialize_with = "nullable")]
    terms_version_or_digest: Option<Option<String>>,
    #[serde(default, deserialize_with = "non_null")]
    reason: Option<String>,
    #[serde(default, deserialize_with = "nullable")]
    reverification_deadline: Option<Option<String>>,
    #[serde(default, deserialize_with = "nullable")]
    source_claim_id: Option<Option<String>>,
}

const POL: &str = "ProviderPolicyEligibility";

pub fn decode_provider_policy_eligibility(json: &str) -> R<ProviderPolicyEligibility> {
    let d: PolicyDto = parse(json, POL)?;
    let deadline = match d.reverification_deadline {
        None => None,
        Some(None) => Some(None),
        Some(Some(t)) => Some(Some(time(t, "reverification_deadline")?)),
    };
    ProviderPolicyEligibility::try_new(
        d.provider_id,
        d.runtime_id,
        d.credential_mode,
        pick(
            &TechnicalStatus::ALL,
            TechnicalStatus::as_str,
            &d.technical_status,
            POL,
        )?,
        pick(
            &PolicyStatus::ALL,
            PolicyStatus::as_str,
            &d.policy_status,
            POL,
        )?,
        d.allowed_execution_contexts,
        time(d.verified_at, "verified_at")?,
        d.evidence_source,
        pick(
            &PolicyEvidenceLabel::ALL,
            PolicyEvidenceLabel::as_str,
            &d.evidence_label,
            POL,
        )?,
        d.terms_version_or_digest,
        d.reason,
        deadline,
        d.source_claim_id,
    )
    .map_err(|_| RoutingWireError::Shape(POL))
}

pub fn encode_provider_policy_eligibility(p: &ProviderPolicyEligibility) -> String {
    let nullable_text = |v: Option<Option<&str>>| v.map(|v| null_or(v, text));
    Obj::new()
        .put("provider_id", p.provider_id())
        .put("runtime_id", p.runtime_id())
        .put("credential_mode", p.credential_mode())
        .put("technical_status", p.technical_status().as_str())
        .put("policy_status", p.policy_status().as_str())
        .opt(
            "allowed_execution_contexts",
            p.allowed_execution_contexts().map(strings),
        )
        .put("verified_at", p.verified_at().as_str())
        .opt("evidence_source", p.evidence_source())
        .put("evidence_label", p.evidence_label().as_str())
        .opt(
            "terms_version_or_digest",
            nullable_text(p.terms_version_or_digest()),
        )
        .opt("reason", p.reason())
        .opt(
            "reverification_deadline",
            p.reverification_deadline()
                .map(|v| null_or(v, |t| text(t.as_str()))),
        )
        .opt("source_claim_id", nullable_text(p.source_claim_id()))
        .done()
        .to_string()
}
