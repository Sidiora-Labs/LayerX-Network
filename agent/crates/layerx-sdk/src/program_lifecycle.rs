use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistration, ModuleRegistry};
use layerx_types::program_lifecycle::{
    NativeProgramDeploy, NativeProgramUpgrade, NativeProgramWindDown,
};
use layerx_wire::activity::decode_signed;
use layerx_wire::hash::activity_id;
use sha2::{Digest as _, Sha256};

use crate::programs::{ProgramOperationError, MAX_SIGNED_ACTIVITY_BYTES};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeProgramLifecycleRequest {
    ordinal: u16,
    signed_activity: Vec<u8>,
    activity_id: [u8; 32],
    idempotency_key: [u8; 32],
}

pub type NativeProgramDeployRequest = NativeProgramLifecycleRequest;
pub type NativeProgramUpgradeRequest = NativeProgramLifecycleRequest;
pub type NativeProgramWindDownRequest = NativeProgramLifecycleRequest;

impl NativeProgramLifecycleRequest {
    /// # Errors
    /// Refuses invalid code, payload framing, scope, or signed activity binding.
    pub fn deploy(
        registry: &ModuleRegistry,
        value: NativeProgramDeploy<'_>,
        signed: &[u8],
    ) -> Result<Self, ProgramOperationError> {
        let digest: [u8; 32] = Sha256::digest(value.wasm).into();
        if digest != value.new_hash {
            return Err(ProgramOperationError::IdentityMismatch);
        }
        Self::bind(
            registry,
            1,
            &value.encode().map_err(|_| ProgramOperationError::Decode)?,
            signed,
        )
    }

    /// # Errors
    /// Refuses invalid code, flags, payload framing, scope, or activity binding.
    pub fn upgrade(
        registry: &ModuleRegistry,
        value: NativeProgramUpgrade<'_>,
        signed: &[u8],
    ) -> Result<Self, ProgramOperationError> {
        let digest: [u8; 32] = Sha256::digest(value.wasm).into();
        if digest != value.new_hash {
            return Err(ProgramOperationError::IdentityMismatch);
        }
        Self::bind(
            registry,
            2,
            &value.encode().map_err(|_| ProgramOperationError::Decode)?,
            signed,
        )
    }

    /// # Errors
    /// Refuses invalid operations, payload bounds, scope, or activity binding.
    pub fn wind_down(
        registry: &ModuleRegistry,
        value: NativeProgramWindDown<'_>,
        signed: &[u8],
    ) -> Result<Self, ProgramOperationError> {
        Self::bind(
            registry,
            7,
            &value.encode().map_err(|_| ProgramOperationError::Decode)?,
            signed,
        )
    }

    fn bind(
        registry: &ModuleRegistry,
        ordinal: u16,
        payload: &[u8],
        signed: &[u8],
    ) -> Result<Self, ProgramOperationError> {
        if signed.is_empty() || signed.len() > MAX_SIGNED_ACTIVITY_BYTES {
            return Err(ProgramOperationError::Bounds);
        }
        let activity =
            decode_signed(signed, registry).map_err(|_| ProgramOperationError::Decode)?;
        if activity.protocol_version() != 3
            || activity.activity_type().module() != ModuleId::Programs
            || activity.activity_type().ordinal() != ordinal
            || activity.payload() != payload
            || activity.payload_hash()
                != layerx_wire::hash::payload_hash(&activity)
                    .map_err(|_| ProgramOperationError::Decode)?
        {
            return Err(ProgramOperationError::IdentityMismatch);
        }
        Ok(Self {
            ordinal,
            signed_activity: signed.to_vec(),
            activity_id: activity_id(&activity).map_err(|_| ProgramOperationError::Decode)?,
            idempotency_key: activity.idempotency_key(),
        })
    }

    #[must_use]
    pub const fn ordinal(&self) -> u16 {
        self.ordinal
    }
    #[must_use]
    pub fn signed_activity(&self) -> &[u8] {
        &self.signed_activity
    }
    #[must_use]
    pub const fn bound_activity_id(&self) -> [u8; 32] {
        self.activity_id
    }
    #[must_use]
    pub const fn bound_idempotency_key(&self) -> [u8; 32] {
        self.idempotency_key
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProgramLifecycleSubmission {
    Acknowledged {
        activity_id: [u8; 32],
        receipt: Vec<u8>,
        result_code: i32,
    },
    Unknown {
        activity_id: [u8; 32],
        idempotency_key: [u8; 32],
        retained_signed_activity: Vec<u8>,
    },
}

/// # Errors
/// Refuses invalid signatures, noncanonical receipts, or mismatched lifecycle scope.
pub fn verify_lifecycle_receipt(
    receipt: &[u8],
    expected_activity: [u8; 32],
    sequencer_public_key: [u8; 32],
) -> Result<layerx_wire::receipt::Receipt, ProgramOperationError> {
    let verified = layerx_proof::receipt::verify_sequencer_signature(receipt, sequencer_public_key)
        .map_err(|_| ProgramOperationError::Verification)?;
    let protocol = verified
        .protocol()
        .ok_or(ProgramOperationError::Verification)?;
    if protocol.protocol_version() != 3
        || protocol.module_id() != 9
        || protocol.module_version() != 4
        || protocol.operation() != 0
        || protocol.program_outcome().is_some()
        || protocol.activity_id() != expected_activity
    {
        return Err(ProgramOperationError::Verification);
    }
    Ok(verified)
}

/// # Errors
/// Refuses an invalid Programs activity registration.
pub fn programs_module_registry() -> Result<ModuleRegistry, ProgramOperationError> {
    let activities = [1, 2, 3, 7].map(|ordinal| ActivityType::new(ModuleId::Programs, ordinal));
    let activities = activities
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| ProgramOperationError::Decode)?;
    let registration = ModuleRegistration::new(ModuleId::Programs, &activities)
        .map_err(|_| ProgramOperationError::Decode)?;
    ModuleRegistry::new(&[registration]).map_err(|_| ProgramOperationError::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(value: &serde_json::Value, name: &str) -> Result<Vec<u8>, String> {
        let text = value[name].as_str().ok_or_else(|| name.to_owned())?;
        if text.len() % 2 != 0 {
            return Err(name.to_owned());
        }
        text.as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let pair = std::str::from_utf8(pair).map_err(|error| error.to_string())?;
                u8::from_str_radix(pair, 16).map_err(|error| error.to_string())
            })
            .collect()
    }

    #[test]
    fn call_receipt_is_not_lifecycle_evidence() -> Result<(), String> {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../platform/sdk/conformance/fixtures/receipt-programs-positive-v3.json"
        ))
        .map_err(|error| error.to_string())?;
        let receipt = bytes(&fixture, "canonical_receipt_hex")?;
        let key = bytes(&fixture["authorized_batch"], "sequencer_public_key_hex")?
            .try_into()
            .map_err(|_| "key length")?;
        assert!(verify_lifecycle_receipt(&receipt, [0x41; 32], key).is_err());
        Ok(())
    }

    #[test]
    fn c_signed_lifecycle_fixtures_bind_exactly() -> Result<(), String> {
        let registry = programs_module_registry().map_err(|error| format!("{error:?}"))?;
        for (name, ordinal) in [
            ("deploy", 1),
            ("upgrade", 2),
            ("wind-down-route", 7),
            ("wind-down-deprecate", 7),
            ("wind-down-tombstone", 7),
            ("wind-down-exit", 7),
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../platform/sdk/conformance/fixtures")
                .join(format!("native-program-{name}-v3.json"));
            let fixture: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?;
            let payload = bytes(&fixture, "payload_hex")?;
            let signed = bytes(&fixture, "signed_activity_hex")?;
            let request = match ordinal {
                1 => NativeProgramLifecycleRequest::deploy(
                    &registry,
                    NativeProgramDeploy::decode(&payload).map_err(|error| format!("{error:?}"))?,
                    &signed,
                ),
                2 => NativeProgramLifecycleRequest::upgrade(
                    &registry,
                    NativeProgramUpgrade::decode(&payload).map_err(|error| format!("{error:?}"))?,
                    &signed,
                ),
                _ => NativeProgramLifecycleRequest::wind_down(
                    &registry,
                    NativeProgramWindDown::decode(&payload)
                        .map_err(|error| format!("{error:?}"))?,
                    &signed,
                ),
            }
            .map_err(|error| format!("{error:?}"))?;
            assert_eq!(request.ordinal(), ordinal);
            assert_eq!(
                request.bound_activity_id().as_slice(),
                bytes(&fixture, "activity_id_hex")?
            );
            assert_eq!(
                request.bound_idempotency_key().as_slice(),
                bytes(&fixture, "idempotency_key_hex")?
            );
            assert_eq!(request.signed_activity(), signed);
            let activity =
                decode_signed(&signed, &registry).map_err(|error| format!("{error:?}"))?;
            let public_key: [u8; 32] = bytes(&fixture, "public_key_hex")?
                .try_into()
                .map_err(|_| "public key length")?;
            let signature: [u8; 64] = activity
                .signature()
                .ok_or("signature absent")?
                .try_into()
                .map_err(|_| "signature length")?;
            let preimage =
                layerx_wire::sign::preimage(&activity).map_err(|error| format!("{error:?}"))?;
            layerx_crypto::ed25519::verify_digest(&public_key, &signature, preimage.as_bytes())
                .map_err(|error| format!("{error:?}"))?;
            for length in 0..signed.len() {
                assert!(NativeProgramLifecycleRequest::bind(
                    &registry,
                    ordinal,
                    &payload,
                    &signed[..length]
                )
                .is_err());
            }
            let mut wrong_protocol = signed.clone();
            let mut wrong_hash = signed.clone();
            let hash_offset = signed.len() - 69 - payload.len() - 5 - 32;
            wrong_hash[hash_offset] ^= 1;
            assert!(decode_signed(&wrong_hash, &registry).is_ok());
            assert!(
                NativeProgramLifecycleRequest::bind(&registry, ordinal, &payload, &wrong_hash)
                    .is_err()
            );
            wrong_protocol[1] = 2;
            assert!(NativeProgramLifecycleRequest::bind(
                &registry,
                ordinal,
                &payload,
                &wrong_protocol
            )
            .is_err());
            let mut altered = payload.clone();
            altered[0] ^= 1;
            assert!(
                NativeProgramLifecycleRequest::bind(&registry, ordinal, &altered, &signed).is_err()
            );
            assert!(NativeProgramLifecycleRequest::bind(
                &registry,
                if ordinal == 1 { 2 } else { 1 },
                &payload,
                &signed
            )
            .is_err());
        }
        Ok(())
    }
}
