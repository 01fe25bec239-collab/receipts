//! Authenticated V1 ingestion and immutable cache replacement decisions.
use std::fmt;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, VerifyingKey};

use super::{ProductEntitlement, ProductEntitlementKeyId, ProductEntitlementSubjectId, wire};

/// Failures contain only static classification and canonical field names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntitlementVerificationError {
    InvalidUtf8,
    DocumentTooLarge,
    MalformedJson,
    UnknownField,
    DuplicateField(&'static str),
    MissingRequiredField(&'static str),
    InvalidPhysicalField(&'static str),
    UnsupportedTimestamp(&'static str),
    UnsupportedVersion,
    InvalidSignatureEncoding,
    InvalidPublicKey,
    DuplicateTrustedKeyId,
    UnknownKeyId,
    SignatureVerificationFailed,
    SubjectMismatch,
    VersionRollback,
    EqualVersionDivergence,
    UnsupportedDeviceBindingV1,
}

impl fmt::Display for EntitlementVerificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "entitlement verification failed: {self:?}")
    }
}
impl std::error::Error for EntitlementVerificationError {}

use EntitlementVerificationError as Error;

/// Public verification anchors supplied only by trusted product-release configuration.
/// Never populate this set from entitlement input or an untrusted cache record.
/// The set is immutable; a changed release constructs a new verifier.
pub struct EntitlementVerifier {
    keys: Vec<(ProductEntitlementKeyId, VerifyingKey)>,
}

/// Authority produced only after every applicable ingestion gate succeeds.
/// Raw cache bytes must be loaded through `EntitlementVerifier::reverify_cached`.
///
/// ```compile_fail
/// use receipts_state::entitlement::{ProductEntitlement, VerifiedProductEntitlement};
/// fn assert_verified(raw: ProductEntitlement) -> VerifiedProductEntitlement {
///     raw.into()
/// }
/// ```
///
/// ```compile_fail
/// use receipts_state::entitlement::VerifiedProductEntitlement;
/// let proof = VerifiedProductEntitlement::default();
/// ```
///
/// ```compile_fail
/// use receipts_state::entitlement::{ProductEntitlement, VerifiedProductEntitlement};
/// fn forge(entitlement: ProductEntitlement) -> VerifiedProductEntitlement {
///     VerifiedProductEntitlement { entitlement, raw: vec![], message: vec![], signature: [0; 64] }
/// }
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct VerifiedProductEntitlement {
    entitlement: ProductEntitlement,
    raw: Vec<u8>,
    message: Vec<u8>,
    signature: [u8; 64],
}

impl fmt::Debug for VerifiedProductEntitlement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedProductEntitlement")
            .finish_non_exhaustive()
    }
}

impl VerifiedProductEntitlement {
    pub fn entitlement(&self) -> &ProductEntitlement {
        &self.entitlement
    }

    /// Exact original signed document, suitable for subsequent cache re-verification.
    pub fn raw_document(&self) -> &[u8] {
        &self.raw
    }

    pub fn trusted_key_id(&self) -> &ProductEntitlementKeyId {
        self.entitlement.key_id()
    }

    // Invoked only after incoming cryptographic and subject verification.
    fn check_replay(
        &self,
        incoming: &ProductEntitlement,
        message: &[u8],
        signature: &[u8; 64],
    ) -> Result<(), Error> {
        match incoming
            .entitlement_version()
            .cmp(&self.entitlement.entitlement_version())
        {
            std::cmp::Ordering::Less => Err(Error::VersionRollback),
            std::cmp::Ordering::Equal
                if message != self.message || signature != &self.signature =>
            {
                Err(Error::EqualVersionDivergence)
            }
            _ => Ok(()),
        }
    }
}

/// A successful ingestion decision. Neither variant performs cache I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntitlementCacheDecision {
    /// Retain the current artifact, including its original wire representation.
    KeepExisting,
    /// Every gate passed; the caller may replace its cache with this artifact.
    Replace(Box<VerifiedProductEntitlement>),
}

impl EntitlementVerifier {
    pub fn new<'a>(
        keys: impl IntoIterator<Item = (ProductEntitlementKeyId, &'a str)>,
    ) -> Result<Self, Error> {
        let mut trusted = Vec::new();
        for (id, encoded) in keys {
            if trusted.iter().any(|(existing, _)| existing == &id) {
                return Err(Error::DuplicateTrustedKeyId);
            }
            let bytes = decode_exact::<32>(encoded).ok_or(Error::InvalidPublicKey)?;
            let key = VerifyingKey::from_bytes(&bytes).map_err(|_| Error::InvalidPublicKey)?;
            if key.is_weak() {
                return Err(Error::InvalidPublicKey);
            }
            trusted.push((id, key));
        }
        Ok(Self { keys: trusted })
    }

    /// Disk presence grants no authority. Repeat all verification gates on cache load.
    pub fn reverify_cached(
        &self,
        raw: &[u8],
        expected: &ProductEntitlementSubjectId,
    ) -> Result<VerifiedProductEntitlement, Error> {
        self.verify(raw, expected, None)
    }

    /// Reverify prior authority under this trust set before comparing versions.
    /// A failure leaves the caller's existing immutable cache artifact untouched.
    pub fn ingest(
        &self,
        raw: &[u8],
        expected: &ProductEntitlementSubjectId,
        cached: Option<&VerifiedProductEntitlement>,
    ) -> Result<EntitlementCacheDecision, Error> {
        let prior = cached
            .map(|cached| self.reverify_cached(cached.raw_document(), expected))
            .transpose()?;
        let incoming = self.verify(raw, expected, prior.as_ref())?;
        if prior.as_ref().is_some_and(|prior| {
            prior.entitlement.entitlement_version() == incoming.entitlement.entitlement_version()
        }) {
            Ok(EntitlementCacheDecision::KeepExisting)
        } else {
            Ok(EntitlementCacheDecision::Replace(Box::new(incoming)))
        }
    }

    fn verify(
        &self,
        raw: &[u8],
        expected: &ProductEntitlementSubjectId,
        prior: Option<&VerifiedProductEntitlement>,
    ) -> Result<VerifiedProductEntitlement, Error> {
        let entitlement = wire::parse(raw)?;
        let message = signed_message(&entitlement);
        let key = self
            .keys
            .iter()
            .find(|(id, _)| id == entitlement.key_id())
            .map(|(_, key)| key)
            .ok_or(Error::UnknownKeyId)?;
        let signature = decode_exact::<64>(entitlement.signature().as_str())
            .ok_or(Error::InvalidSignatureEncoding)?;
        key.verify_strict(&message, &Signature::from_bytes(&signature))
            .map_err(|_| Error::SignatureVerificationFailed)?;
        if entitlement.subject_id() != expected {
            return Err(Error::SubjectMismatch);
        }
        if let Some(prior) = prior {
            prior.check_replay(&entitlement, &message, &signature)?;
        }
        if entitlement.device_binding().is_some() {
            return Err(Error::UnsupportedDeviceBindingV1);
        }
        Ok(VerifiedProductEntitlement {
            entitlement,
            raw: raw.to_vec(),
            message,
            signature,
        })
    }
}

fn decode_exact<const N: usize>(encoded: &str) -> Option<[u8; N]> {
    let mut bytes = [0; N];
    let length = URL_SAFE_NO_PAD.decode_slice(encoded, &mut bytes).ok()?;
    (length == N).then_some(bytes)
}

/// Frozen binary projection of physically validated fields, never JSON serialization.
pub(super) fn signed_message(entitlement: &ProductEntitlement) -> Vec<u8> {
    fn string(out: &mut Vec<u8>, value: &str) {
        out.extend_from_slice(&(value.len() as u64).to_be_bytes());
        out.extend_from_slice(value.as_bytes());
    }
    fn optional(out: &mut Vec<u8>, value: Option<&str>) {
        match value {
            None => out.push(0),
            Some(value) => {
                out.push(1);
                string(out, value);
            }
        }
    }
    let mut out = b"receipts.product-entitlement.v1\0".to_vec();
    string(&mut out, entitlement.subject_id().as_str());
    string(&mut out, entitlement.tier_id().as_str());
    out.extend_from_slice(&(entitlement.capabilities().len() as u64).to_be_bytes());
    for capability in entitlement.capabilities() {
        string(&mut out, capability.as_str());
    }
    string(&mut out, entitlement.issued_at().as_str());
    string(&mut out, entitlement.expires_at().as_str());
    out.extend_from_slice(&entitlement.entitlement_version().get().to_be_bytes());
    string(&mut out, entitlement.key_id().as_str());
    optional(
        &mut out,
        entitlement
            .offline_grace_until()
            .map(|value| value.as_str()),
    );
    optional(&mut out, entitlement.device_binding());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_version_signature_comparison_is_independent_of_payload_comparison() {
        use crate::entitlement::verification_vectors::{PUBLIC_KEY, VALID};
        let verifier = EntitlementVerifier::new([(
            ProductEntitlementKeyId::new("k".into()).unwrap(),
            PUBLIC_KEY,
        )])
        .unwrap();
        let expected = ProductEntitlementSubjectId::new("é".into()).unwrap();
        let prior = verifier
            .reverify_cached(VALID.as_bytes(), &expected)
            .unwrap();
        let mut different_signature = prior.signature;
        different_signature[0] ^= 1;
        // Isolate the byte-comparison gate with a successfully verified prior.
        // No altered proof is constructed; public ingestion separately tests crypto rejection.
        assert_eq!(
            prior.check_replay(&prior.entitlement, &prior.message, &different_signature),
            Err(Error::EqualVersionDivergence)
        );
        assert_eq!(
            prior.check_replay(&prior.entitlement, &prior.message, &prior.signature),
            Ok(())
        );
    }
}
