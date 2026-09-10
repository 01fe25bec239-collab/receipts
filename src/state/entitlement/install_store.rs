//! Install-scoped durable evidence. Bootstrap supplies the absolute path and trusted subject.
use std::{fmt, path::Path, time::Duration};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::{
    ActivationIdentityFields, ActivationState, ActivationStateKind, EntitlementCacheDecision,
    EntitlementVerificationError, EntitlementVerifier, ProductEntitlementSubjectId,
    VerifiedProductEntitlement, install_store_migrations,
};
use crate::CanonicalTimestampV1;

/// Bounded failures deliberately discard input-bearing SQLite and decoding diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallEntitlementStoreError {
    AbsolutePathRequired,
    ParentDirectoryMissing,
    Sqlite,
    DurabilityConfiguration,
    UnrecognizedSchema,
    InvalidActivation,
    ActivationSubjectContradiction,
    CurrentCacheRejected(EntitlementVerificationError),
    IncomingRejected(EntitlementVerificationError),
    #[cfg(test)]
    InjectedFailure,
}
impl fmt::Display for InstallEntitlementStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "install entitlement store: {self:?}")
    }
}
impl std::error::Error for InstallEntitlementStoreError {}
impl From<rusqlite::Error> for InstallEntitlementStoreError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Sqlite
    }
}
use InstallEntitlementStoreError as Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallCachedEntitlementFailure {
    ExpectedSubjectUnavailable,
    ActivationSubjectContradiction,
    VerificationRejected(EntitlementVerificationError),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallCachedEntitlement {
    Missing,
    Verified(Box<VerifiedProductEntitlement>),
    Unusable(InstallCachedEntitlementFailure),
}
#[derive(Clone, PartialEq, Eq)]
pub struct InstallEntitlementSnapshot {
    pub activation: Option<ActivationState>,
    pub cache: InstallCachedEntitlement,
}
impl fmt::Debug for InstallEntitlementSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InstallEntitlementSnapshot")
            .field(
                "activation",
                &self
                    .activation
                    .as_ref()
                    .map(ActivationState::activation_state),
            )
            .field("cache", &self.cache)
            .finish()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallEntitlementIngestOutcome {
    KeepExisting,
    Replace,
}

/// Dedicated install evidence store; no SQL or unchecked persistence escape.
///
/// Trusted subject is required for ingest:
/// ```compile_fail
/// use receipts_state::entitlement::*;
/// fn unbound(repo: &mut InstallEntitlementRepository, verifier: &EntitlementVerifier) {
///     repo.ingest_and_persist(b"{}", verifier, None, None);
/// }
/// ```
/// The underlying connection remains private:
/// ```compile_fail
/// use receipts_state::entitlement::InstallEntitlementRepository;
/// fn escape(repo: &InstallEntitlementRepository) { let _ = &repo.conn; }
/// ```
/// ```compile_fail
/// use receipts_state::entitlement::InstallEntitlementRepository;
/// fn escape(repo: &mut InstallEntitlementRepository) { repo.execute_sql("DELETE FROM install_entitlement_cache"); }
/// ```
/// ```compile_fail
/// use receipts_state::entitlement::InstallEntitlementRepository;
/// fn unchecked(repo: &mut InstallEntitlementRepository) { repo.save_raw_cache(b"{}"); }
/// ```
/// ```compile_fail
/// use receipts_state::entitlement::{InstallEntitlementRepository, VerifiedProductEntitlement};
/// fn replay(repo: &mut InstallEntitlementRepository, proof: &VerifiedProductEntitlement) {
///     repo.save_verified_entitlement(proof);
/// }
/// ```
pub struct InstallEntitlementRepository {
    #[cfg(not(test))]
    conn: Connection,
    #[cfg(test)]
    pub(super) conn: Connection,
    #[cfg(test)]
    pub(super) after_activation_read: Option<Box<dyn FnOnce() + Send>>,
    #[cfg(test)]
    pub(super) fail_after_activation: bool,
}
impl fmt::Debug for InstallEntitlementRepository {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("InstallEntitlementRepository")
    }
}
impl InstallEntitlementRepository {
    /// Creates only the supplied file, never its parent or an OS-selected location.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(Error::AbsolutePathRequired);
        }
        if !path.parent().is_some_and(Path::is_dir) {
            return Err(Error::ParentDirectoryMissing);
        }
        let mut conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_millis(5000))?;
        // Read-only recognition precedes even journal-mode mutation of foreign databases.
        {
            let tx = conn.transaction()?;
            install_store_migrations::inspect(&tx)?;
        }
        let mode: String = conn.query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))?;
        conn.execute_batch("PRAGMA synchronous = FULL; PRAGMA foreign_keys = ON;")?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(Error::DurabilityConfiguration);
        }
        for (pragma, expected) in [
            ("busy_timeout", 5000),
            ("synchronous", 2),
            ("foreign_keys", 1),
        ] {
            let actual: i64 = conn.query_row(&format!("PRAGMA {pragma}"), [], |row| row.get(0))?;
            if actual != expected {
                return Err(Error::DurabilityConfiguration);
            }
        }
        install_store_migrations::reconcile(&mut conn)?;
        Ok(Self {
            conn,
            #[cfg(test)]
            fail_after_activation: false,
            #[cfg(test)]
            after_activation_read: None,
        })
    }

    /// One read snapshot; no cleanup or repair. Disk identity is never a trust root.
    pub fn load_reverified(
        &mut self,
        verifier: &EntitlementVerifier,
        expected_subject: Option<&ProductEntitlementSubjectId>,
    ) -> Result<InstallEntitlementSnapshot, Error> {
        let tx = self.conn.transaction()?;
        let activation = read_activation(&tx)?;
        #[cfg(test)]
        if let Some(hook) = self.after_activation_read.take() {
            hook();
        }
        let raw = read_cache(&tx)?;
        use InstallCachedEntitlementFailure as Failure;
        let cache = match (raw, expected_subject) {
            (None, expected) => {
                if let Some(expected) = expected {
                    check_subject(activation.as_ref(), expected)?;
                }
                InstallCachedEntitlement::Missing
            }
            (Some(_), None) => {
                InstallCachedEntitlement::Unusable(Failure::ExpectedSubjectUnavailable)
            }
            (Some(raw), Some(expected)) => {
                if check_subject(activation.as_ref(), expected).is_err() {
                    InstallCachedEntitlement::Unusable(Failure::ActivationSubjectContradiction)
                } else {
                    match verifier.reverify_cached(&raw, expected) {
                        Ok(proof) => InstallCachedEntitlement::Verified(Box::new(proof)),
                        Err(error) => {
                            InstallCachedEntitlement::Unusable(Failure::VerificationRejected(error))
                        }
                    }
                }
            }
        };
        tx.commit()?;
        Ok(InstallEntitlementSnapshot { activation, cache })
    }

    pub fn persist_activation(&mut self, activation: &ActivationState) -> Result<(), Error> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        write_activation(&tx, activation)?;
        tx.commit()?;
        Ok(())
    }

    pub fn clear_cache_and_persist_activation(
        &mut self,
        activation: &ActivationState,
    ) -> Result<(), Error> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        write_activation(&tx, activation)?;
        #[cfg(test)]
        if self.fail_after_activation {
            return Err(Error::InjectedFailure);
        }
        tx.execute("DELETE FROM install_entitlement_cache", [])?;
        tx.commit()?;
        Ok(())
    }

    /// All replacement authority is evaluated after acquiring the shared store's write lock.
    pub fn ingest_and_persist(
        &mut self,
        incoming_raw_document: &[u8],
        verifier: &EntitlementVerifier,
        trusted_expected_subject: &ProductEntitlementSubjectId,
        optional_activation: Option<&ActivationState>,
    ) -> Result<InstallEntitlementIngestOutcome, Error> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let durable_activation = read_activation(&tx)?;
        check_subject(
            optional_activation.or(durable_activation.as_ref()),
            trusted_expected_subject,
        )?;
        let current = read_cache(&tx)?
            .map(|raw| verifier.reverify_cached(&raw, trusted_expected_subject))
            .transpose()
            .map_err(Error::CurrentCacheRejected)?;
        let decision = verifier
            .ingest(
                incoming_raw_document,
                trusted_expected_subject,
                current.as_ref(),
            )
            .map_err(Error::IncomingRejected)?;
        if let Some(activation) = optional_activation {
            write_activation(&tx, activation)?;
            #[cfg(test)]
            if self.fail_after_activation {
                return Err(Error::InjectedFailure);
            }
        }
        let outcome = match decision {
            EntitlementCacheDecision::KeepExisting => InstallEntitlementIngestOutcome::KeepExisting,
            EntitlementCacheDecision::Replace(proof) => {
                tx.execute("INSERT INTO install_entitlement_cache VALUES (1, ?1) ON CONFLICT(singleton_id) DO UPDATE SET raw_document = excluded.raw_document", [proof.raw_document()])?;
                InstallEntitlementIngestOutcome::Replace
            }
        };
        tx.commit()?;
        Ok(outcome)
    }
}

fn check_subject(
    activation: Option<&ActivationState>,
    expected: &ProductEntitlementSubjectId,
) -> Result<(), Error> {
    if activation
        .and_then(ActivationState::subject_id)
        .is_some_and(|subject| subject != expected.as_str())
    {
        return Err(Error::ActivationSubjectContradiction);
    }
    Ok(())
}
fn read_cache(conn: &Connection) -> Result<Option<Vec<u8>>, Error> {
    if conn.query_row(
        "SELECT count(*) FROM install_entitlement_cache WHERE singleton_id != 1",
        [],
        |r| r.get::<_, i64>(0),
    )? != 0
    {
        return Err(Error::UnrecognizedSchema);
    }
    Ok(conn
        .query_row(
            "SELECT raw_document FROM install_entitlement_cache WHERE singleton_id = 1",
            [],
            |row| row.get(0),
        )
        .optional()?)
}
fn read_activation(conn: &Connection) -> Result<Option<ActivationState>, Error> {
    if conn.query_row(
        "SELECT count(*) FROM install_activation_state WHERE singleton_id != 1",
        [],
        |r| r.get::<_, i64>(0),
    )? != 0
    {
        return Err(Error::InvalidActivation);
    }
    let row = conn.query_row("SELECT activation_state, subject_id, first_activated_at, last_known_tier_id, last_entitlement_seen_at, last_observed_server_time, logged_out_at, recorded_at FROM install_activation_state WHERE singleton_id = 1", [], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, Option<String>>(4)?, row.get::<_, Option<String>>(5)?, row.get::<_, Option<String>>(6)?, row.get::<_, String>(7)?))
    }).optional().map_err(|_| Error::InvalidActivation)?;
    let Some((kind, subject, first, tier, seen, server, logout, recorded)) = row else {
        return Ok(None);
    };
    let kind = ActivationStateKind::ALL
        .into_iter()
        .find(|value| value.as_str() == kind)
        .ok_or(Error::InvalidActivation)?;
    let timestamp =
        |value: &str| CanonicalTimestampV1::parse(value).map_err(|_| Error::InvalidActivation);
    Ok(Some(
        ActivationState::new(
            ActivationIdentityFields::new(kind, subject, tier),
            first.as_deref().map(timestamp).transpose()?,
            seen.as_deref().map(timestamp).transpose()?,
            server.as_deref().map(timestamp).transpose()?,
            logout.as_deref().map(timestamp).transpose()?,
            timestamp(&recorded)?,
        )
        .map_err(|_| Error::InvalidActivation)?,
    ))
}
fn write_activation(conn: &Connection, activation: &ActivationState) -> Result<(), Error> {
    conn.execute("INSERT INTO install_activation_state VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        ON CONFLICT(singleton_id) DO UPDATE SET activation_state = excluded.activation_state,
        subject_id = excluded.subject_id, first_activated_at = excluded.first_activated_at,
        last_known_tier_id = excluded.last_known_tier_id, last_entitlement_seen_at = excluded.last_entitlement_seen_at,
        last_observed_server_time = excluded.last_observed_server_time, logged_out_at = excluded.logged_out_at,
        recorded_at = excluded.recorded_at", params![activation.activation_state().as_str(), activation.subject_id(),
        activation.first_activated_at().map(CanonicalTimestampV1::as_str), activation.last_known_tier_id(),
        activation.last_entitlement_seen_at().map(CanonicalTimestampV1::as_str), activation.last_observed_server_time().map(CanonicalTimestampV1::as_str),
        activation.logged_out_at().map(CanonicalTimestampV1::as_str), activation.recorded_at().as_str()])?;
    Ok(())
}
