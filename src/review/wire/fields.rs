//! Strict object readers and shared sub-codecs for the seven Review codecs.

use super::json::{Json, NotInteger, SyntaxError, Writer, integer_digits};
use super::{ReviewWireError, ReviewWireErrorKind as K};
use crate::{
    A4ReviewFinding, A4ReviewFindingCategory, A4ReviewFindingConfidence, A4ReviewFindingSeverity,
    A4ReviewFindingSource, ReviewDateTimeV1,
};
use receipts_workspace_execution::{
    CommitSha, WorkspaceCheckpointCheckSource, WorkspaceCheckpointExecutedCheckCore,
    WorkspaceCheckpointRef, WorkspaceCheckpointRefType,
};
use std::num::NonZeroU64;

pub(crate) fn err(path: &str, kind: K) -> ReviewWireError {
    ReviewWireError {
        path: if path.is_empty() {
            "/".into()
        } else {
            path.into()
        },
        kind,
    }
}

pub(crate) fn root(text: &str) -> Result<Obj, ReviewWireError> {
    let value = super::json::parse(text).map_err(|e| {
        err(
            "",
            match e {
                SyntaxError::DuplicateKey => K::DuplicateKey,
                SyntaxError::Invalid | SyntaxError::TooDeep => K::Syntax,
            },
        )
    })?;
    Obj::new(value, String::new())
}

/// An object whose members must each be consumed exactly once; [`Obj::end`]
/// refuses any unknown member.
pub(crate) struct Obj {
    members: Vec<(String, Option<Json>)>,
    pub(crate) path: String,
}

impl Obj {
    pub(crate) fn new(value: Json, path: String) -> Result<Self, ReviewWireError> {
        match value {
            Json::Object(m) => Ok(Self {
                members: m.into_iter().map(|(k, v)| (k, Some(v))).collect(),
                path,
            }),
            _ => Err(err(&path, K::WrongType)),
        }
    }
    pub(crate) fn sub(&self, key: &str) -> String {
        format!("{}/{key}", self.path)
    }
    pub(crate) fn opt(&mut self, key: &str) -> Option<(Json, String)> {
        let path = self.sub(key);
        self.members
            .iter_mut()
            .find(|(k, _)| k == key)
            .and_then(|(_, v)| v.take())
            .map(|v| (v, path))
    }
    pub(crate) fn req(&mut self, key: &str) -> Result<(Json, String), ReviewWireError> {
        let path = self.sub(key);
        self.opt(key).ok_or_else(|| err(&path, K::MissingField))
    }
    pub(crate) fn end(self) -> Result<(), ReviewWireError> {
        match self.members.iter().find(|(_, v)| v.is_some()) {
            Some((k, _)) => Err(err(&format!("{}/{k}", self.path), K::UnknownField)),
            None => Ok(()),
        }
    }

    // ---- typed members -----------------------------------------------------
    pub(crate) fn string(&mut self, key: &str) -> Result<String, ReviewWireError> {
        let (v, p) = self.req(key)?;
        string(v, &p)
    }
    pub(crate) fn opt_string(&mut self, key: &str) -> Result<Option<String>, ReviewWireError> {
        self.opt(key).map(|(v, p)| string(v, &p)).transpose()
    }
    pub(crate) fn boolean(&mut self, key: &str) -> Result<bool, ReviewWireError> {
        let (v, p) = self.req(key)?;
        boolean(v, &p)
    }
    pub(crate) fn opt_bool(&mut self, key: &str) -> Result<Option<bool>, ReviewWireError> {
        self.opt(key).map(|(v, p)| boolean(v, &p)).transpose()
    }
    pub(crate) fn strings(&mut self, key: &str) -> Result<Vec<String>, ReviewWireError> {
        let (v, p) = self.req(key)?;
        array(v, &p, string)
    }
    pub(crate) fn opt_strings(
        &mut self,
        key: &str,
    ) -> Result<Option<Vec<String>>, ReviewWireError> {
        self.opt(key).map(|(v, p)| array(v, &p, string)).transpose()
    }
    pub(crate) fn sha(&mut self, key: &str) -> Result<String, ReviewWireError> {
        let (v, p) = self.req(key)?;
        sha(v, &p)
    }
    pub(crate) fn id(&mut self, key: &str) -> Result<String, ReviewWireError> {
        let (v, p) = self.req(key)?;
        id(v, &p)
    }
    pub(crate) fn opt_id(&mut self, key: &str) -> Result<Option<String>, ReviewWireError> {
        self.opt(key).map(|(v, p)| id(v, &p)).transpose()
    }
    pub(crate) fn nonempty(&mut self, key: &str) -> Result<String, ReviewWireError> {
        let (v, p) = self.req(key)?;
        nonempty(v, &p)
    }
    pub(crate) fn opt_nonempty(&mut self, key: &str) -> Result<Option<String>, ReviewWireError> {
        self.opt(key).map(|(v, p)| nonempty(v, &p)).transpose()
    }
    pub(crate) fn date_time(&mut self, key: &str) -> Result<ReviewDateTimeV1, ReviewWireError> {
        let (v, p) = self.req(key)?;
        date_time(v, &p)
    }
    pub(crate) fn opt_date_time(
        &mut self,
        key: &str,
    ) -> Result<Option<ReviewDateTimeV1>, ReviewWireError> {
        self.opt(key).map(|(v, p)| date_time(v, &p)).transpose()
    }
    pub(crate) fn enumeration<T: Copy>(
        &mut self,
        key: &str,
        all: &[T],
        name: fn(T) -> &'static str,
    ) -> Result<T, ReviewWireError> {
        let (v, p) = self.req(key)?;
        enumeration(v, &p, all, name)
    }
    pub(crate) fn opt_enum<T: Copy>(
        &mut self,
        key: &str,
        all: &[T],
        name: fn(T) -> &'static str,
    ) -> Result<Option<T>, ReviewWireError> {
        self.opt(key)
            .map(|(v, p)| enumeration(v, &p, all, name))
            .transpose()
    }
    /// Nullable string: `Ok(None)` absent, `Ok(Some(None))` null.
    pub(crate) fn nullable_string(
        &mut self,
        key: &str,
    ) -> Result<Option<Option<String>>, ReviewWireError> {
        match self.opt(key) {
            None => Ok(None),
            Some((Json::Null, _)) => Ok(Some(None)),
            Some((v, p)) => string(v, &p).map(|s| Some(Some(s))),
        }
    }
    pub(crate) fn opt_refs(
        &mut self,
        key: &str,
    ) -> Result<Option<Vec<WorkspaceCheckpointRef>>, ReviewWireError> {
        self.opt(key)
            .map(|(v, p)| array(v, &p, reference))
            .transpose()
    }
    pub(crate) fn opt_ref(
        &mut self,
        key: &str,
    ) -> Result<Option<WorkspaceCheckpointRef>, ReviewWireError> {
        self.opt(key).map(|(v, p)| reference(v, &p)).transpose()
    }
    pub(crate) fn object(&mut self, key: &str) -> Result<Obj, ReviewWireError> {
        let (v, p) = self.req(key)?;
        Obj::new(v, p)
    }
    pub(crate) fn opt_object(&mut self, key: &str) -> Result<Option<Obj>, ReviewWireError> {
        self.opt(key).map(|(v, p)| Obj::new(v, p)).transpose()
    }
    /// Canonical non-negative integer digits.
    pub(crate) fn non_negative(&mut self, key: &str) -> Result<String, ReviewWireError> {
        let (v, p) = self.req(key)?;
        let digits = integer(v, &p)?;
        if digits.starts_with('-') {
            return Err(err(&p, K::InvalidValue));
        }
        Ok(digits)
    }
}

// ---- scalar decoders --------------------------------------------------------

pub(crate) fn string(v: Json, p: &str) -> Result<String, ReviewWireError> {
    match v {
        Json::String(s) => Ok(s),
        _ => Err(err(p, K::WrongType)),
    }
}
pub(crate) fn boolean(v: Json, p: &str) -> Result<bool, ReviewWireError> {
    match v {
        Json::Bool(b) => Ok(b),
        _ => Err(err(p, K::WrongType)),
    }
}
pub(crate) fn nonempty(v: Json, p: &str) -> Result<String, ReviewWireError> {
    let s = string(v, p)?;
    if s.is_empty() {
        return Err(err(p, K::InvalidValue));
    }
    Ok(s)
}
/// Schema identifiers: 1..=200 characters.
pub(crate) fn id(v: Json, p: &str) -> Result<String, ReviewWireError> {
    let s = string(v, p)?;
    if !(1..=200).contains(&s.chars().count()) {
        return Err(err(p, K::InvalidValue));
    }
    Ok(s)
}
/// `^[0-9a-f]{40}$`.
pub(crate) fn sha(v: Json, p: &str) -> Result<String, ReviewWireError> {
    let s = string(v, p)?;
    if s.len() != 40
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(err(p, K::InvalidValue));
    }
    Ok(s)
}
pub(crate) fn commit(v: Json, p: &str) -> Result<CommitSha, ReviewWireError> {
    CommitSha::parse(&sha(v, p)?).map_err(|_| err(p, K::InvalidValue))
}
pub(crate) fn date_time(v: Json, p: &str) -> Result<ReviewDateTimeV1, ReviewWireError> {
    // ReviewDateTimeV1 is the adopted physical binding; spellings it refuses
    // are reported as unsupported, not as schema-invalid.
    ReviewDateTimeV1::try_new(string(v, p)?).map_err(|_| err(p, K::UnsupportedValue))
}
pub(crate) fn integer(v: Json, p: &str) -> Result<String, ReviewWireError> {
    match v {
        Json::Number(t) => integer_digits(&t).map_err(|e| match e {
            NotInteger::Fraction => err(p, K::WrongType),
            NotInteger::Oversized => err(p, K::UnsupportedValue),
        }),
        _ => Err(err(p, K::WrongType)),
    }
}
pub(crate) fn enumeration<T: Copy>(
    v: Json,
    p: &str,
    all: &[T],
    name: fn(T) -> &'static str,
) -> Result<T, ReviewWireError> {
    let s = string(v, p)?;
    all.iter()
        .copied()
        .find(|t| name(*t) == s)
        .ok_or_else(|| err(p, K::InvalidValue))
}
pub(crate) fn array<T>(
    v: Json,
    p: &str,
    item: fn(Json, &str) -> Result<T, ReviewWireError>,
) -> Result<Vec<T>, ReviewWireError> {
    match v {
        Json::Array(items) => items
            .into_iter()
            .enumerate()
            .map(|(i, v)| item(v, &format!("{p}/{i}")))
            .collect(),
        _ => Err(err(p, K::WrongType)),
    }
}
pub(crate) fn nonempty_array<T>(
    v: Json,
    p: &str,
    item: fn(Json, &str) -> Result<T, ReviewWireError>,
) -> Result<Vec<T>, ReviewWireError> {
    let items = array(v, p, item)?;
    if items.is_empty() {
        return Err(err(p, K::InvalidValue));
    }
    Ok(items)
}

// ---- shared nested shapes ---------------------------------------------------

pub(crate) fn reference(v: Json, p: &str) -> Result<WorkspaceCheckpointRef, ReviewWireError> {
    let mut o = Obj::new(v, p.into())?;
    let ref_type = o.enumeration(
        "ref_type",
        &WorkspaceCheckpointRefType::ALL,
        WorkspaceCheckpointRefType::as_str,
    )?;
    let target = o.nonempty("target")?;
    let digest = o.opt_string("digest")?;
    let section = o.opt_string("section")?;
    o.end()?;
    WorkspaceCheckpointRef::new(ref_type, target, digest, section)
        .map_err(|_| err(p, K::InvalidValue))
}

pub(crate) fn write_ref(w: &mut Writer, r: &WorkspaceCheckpointRef) {
    w.begin_object();
    w.key("ref_type");
    w.str(r.ref_type().as_str());
    w.key("target");
    w.str(r.target());
    if let Some(d) = r.digest() {
        w.key("digest");
        w.str(d);
    }
    if let Some(s) = r.section() {
        w.key("section");
        w.str(s);
    }
    w.end_object();
}

pub(crate) fn write_refs(w: &mut Writer, key: &str, refs: Option<&[WorkspaceCheckpointRef]>) {
    if let Some(refs) = refs {
        w.key(key);
        w.begin_array();
        for r in refs {
            write_ref(w, r);
        }
        w.end_array();
    }
}

pub(crate) fn opt_str(w: &mut Writer, key: &str, v: Option<&str>) {
    if let Some(v) = v {
        w.key(key);
        w.str(v);
    }
}
pub(crate) fn opt_strs(w: &mut Writer, key: &str, v: Option<&[String]>) {
    if let Some(v) = v {
        w.key(key);
        w.strs(v);
    }
}
pub(crate) fn opt_bool(w: &mut Writer, key: &str, v: Option<bool>) {
    if let Some(v) = v {
        w.key(key);
        w.bool(v);
    }
}

/// The common executed-check fields; `exit_digits` keeps the exact integer
/// so each owner applies its own carrier.
pub(crate) struct CheckFields {
    pub(crate) source: WorkspaceCheckpointCheckSource,
    pub(crate) command: Vec<String>,
    pub(crate) exit_digits: String,
    pub(crate) code_sha: CommitSha,
    pub(crate) timed_out: Option<bool>,
    pub(crate) output_ref: Option<WorkspaceCheckpointRef>,
    pub(crate) result: Option<String>,
    pub(crate) started_at: Option<ReviewDateTimeV1>,
    pub(crate) finished_at: Option<ReviewDateTimeV1>,
    pub(crate) path: String,
}

impl CheckFields {
    pub(crate) fn exit_i64(&self) -> Result<i64, ReviewWireError> {
        self.exit_digits
            .parse()
            .map_err(|_| err(&format!("{}/exit_code", self.path), K::UnsupportedValue))
    }
    /// The Workspace executed-check carrier (i64 exit code).
    pub(crate) fn core(&self) -> Result<WorkspaceCheckpointExecutedCheckCore, ReviewWireError> {
        WorkspaceCheckpointExecutedCheckCore::new(
            self.source,
            self.command.clone(),
            self.exit_i64()?,
            self.code_sha.clone(),
            self.timed_out,
            self.output_ref.clone(),
        )
        .map_err(|_| err(&self.path, K::InvalidValue))
    }
    pub(crate) fn result_of<T: Copy>(
        &self,
        all: &[T],
        name: fn(T) -> &'static str,
    ) -> Result<Option<T>, ReviewWireError> {
        self.result
            .as_deref()
            .map(|r| {
                all.iter()
                    .copied()
                    .find(|x| name(*x) == r)
                    .ok_or_else(|| err(&format!("{}/result", self.path), K::InvalidValue))
            })
            .transpose()
    }
}

pub(crate) fn check_fields(v: Json, p: &str) -> Result<CheckFields, ReviewWireError> {
    let mut o = Obj::new(v, p.into())?;
    let source = o.enumeration(
        "source",
        &WorkspaceCheckpointCheckSource::ALL,
        WorkspaceCheckpointCheckSource::as_str,
    )?;
    let (cmd, cp) = o.req("command")?;
    let command = nonempty_array(cmd, &cp, string)?;
    let (ec, ep) = o.req("exit_code")?;
    let exit_digits = integer(ec, &ep)?;
    let (sv, sp) = o.req("code_sha")?;
    let code_sha = commit(sv, &sp)?;
    let timed_out = o.opt_bool("timed_out")?;
    let started_at = o.opt_date_time("started_at")?;
    let finished_at = o.opt_date_time("finished_at")?;
    let output_ref = o.opt_ref("output_ref")?;
    let result = o.opt_string("result")?;
    o.end()?;
    Ok(CheckFields {
        source,
        command,
        exit_digits,
        code_sha,
        timed_out,
        output_ref,
        result,
        started_at,
        finished_at,
        path: p.into(),
    })
}

/// Writes one executed check in schema property order.
#[allow(clippy::too_many_arguments)]
pub(crate) fn write_check(
    w: &mut Writer,
    source: WorkspaceCheckpointCheckSource,
    command: &[String],
    exit_code: &str,
    code_sha: &str,
    timed_out: Option<bool>,
    started_at: Option<&ReviewDateTimeV1>,
    finished_at: Option<&ReviewDateTimeV1>,
    output_ref: Option<&WorkspaceCheckpointRef>,
    result: Option<&str>,
) {
    w.begin_object();
    w.key("source");
    w.str(source.as_str());
    w.key("command");
    w.strs(command);
    w.key("exit_code");
    w.raw_integer(exit_code);
    w.key("code_sha");
    w.str(code_sha);
    opt_bool(w, "timed_out", timed_out);
    opt_str(w, "started_at", started_at.map(ReviewDateTimeV1::as_str));
    opt_str(w, "finished_at", finished_at.map(ReviewDateTimeV1::as_str));
    if let Some(r) = output_ref {
        w.key("output_ref");
        write_ref(w, r);
    }
    opt_str(w, "result", result);
    w.end_object();
}

/// Finding fields shared by A4Review and IntegrationRequest; `line` is
/// returned as canonical digits so each owner applies its own carrier.
pub(crate) struct FindingFields {
    pub(crate) finding_id: String,
    pub(crate) severity: A4ReviewFindingSeverity,
    pub(crate) category: A4ReviewFindingCategory,
    pub(crate) description: String,
    pub(crate) blocking: bool,
    pub(crate) path: Option<String>,
    pub(crate) line: Option<(String, String)>,
    pub(crate) evidence_ref: Option<WorkspaceCheckpointRef>,
    pub(crate) confidence: Option<A4ReviewFindingConfidence>,
    pub(crate) source: Option<A4ReviewFindingSource>,
}

pub(crate) fn finding_fields(v: Json, p: &str) -> Result<FindingFields, ReviewWireError> {
    let mut o = Obj::new(v, p.into())?;
    let finding_id = o.id("finding_id")?;
    let severity = o.enumeration(
        "severity",
        &A4ReviewFindingSeverity::ALL,
        A4ReviewFindingSeverity::as_str,
    )?;
    let category = o.enumeration(
        "category",
        &A4ReviewFindingCategory::ALL,
        A4ReviewFindingCategory::as_str,
    )?;
    let description = o.nonempty("description")?;
    let blocking = o.boolean("blocking")?;
    let path = o.opt_string("path")?;
    let line = o
        .opt("line")
        .map(|(v, lp)| {
            let digits = integer(v, &lp)?;
            if digits.starts_with('-') || digits == "0" {
                return Err(err(&lp, K::InvalidValue));
            }
            Ok((digits, lp))
        })
        .transpose()?;
    let evidence_ref = o.opt_ref("evidence_ref")?;
    let confidence = o.opt_enum(
        "confidence",
        &A4ReviewFindingConfidence::ALL,
        A4ReviewFindingConfidence::as_str,
    )?;
    let source = o.opt_enum(
        "source",
        &A4ReviewFindingSource::ALL,
        A4ReviewFindingSource::as_str,
    )?;
    o.end()?;
    Ok(FindingFields {
        finding_id,
        severity,
        category,
        description,
        blocking,
        path,
        line,
        evidence_ref,
        confidence,
        source,
    })
}

pub(crate) fn a4_finding(v: Json, p: &str) -> Result<A4ReviewFinding, ReviewWireError> {
    let f = finding_fields(v, p)?;
    let line = f
        .line
        .map(|(digits, lp)| {
            digits
                .parse::<u64>()
                .ok()
                .and_then(NonZeroU64::new)
                .ok_or_else(|| err(&lp, K::UnsupportedValue))
        })
        .transpose()?;
    A4ReviewFinding::new(
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_finding(
    w: &mut Writer,
    finding_id: &str,
    severity: A4ReviewFindingSeverity,
    category: A4ReviewFindingCategory,
    description: &str,
    blocking: bool,
    path: Option<&str>,
    line: Option<&str>,
    evidence_ref: Option<&WorkspaceCheckpointRef>,
    confidence: Option<A4ReviewFindingConfidence>,
    source: Option<A4ReviewFindingSource>,
) {
    w.begin_object();
    w.key("finding_id");
    w.str(finding_id);
    w.key("severity");
    w.str(severity.as_str());
    w.key("category");
    w.str(category.as_str());
    w.key("description");
    w.str(description);
    w.key("blocking");
    w.bool(blocking);
    opt_str(w, "path", path);
    if let Some(line) = line {
        w.key("line");
        w.raw_integer(line);
    }
    if let Some(r) = evidence_ref {
        w.key("evidence_ref");
        write_ref(w, r);
    }
    opt_str(
        w,
        "confidence",
        confidence.map(A4ReviewFindingConfidence::as_str),
    );
    opt_str(w, "source", source.map(A4ReviewFindingSource::as_str));
    w.end_object();
}

pub(crate) fn write_a4_finding(w: &mut Writer, f: &A4ReviewFinding) {
    let line = f.line().map(|l| l.to_string());
    write_finding(
        w,
        f.finding_id(),
        f.severity(),
        f.category(),
        f.description(),
        f.blocking(),
        f.path(),
        line.as_deref(),
        f.evidence_ref(),
        f.confidence(),
        f.source(),
    );
}
