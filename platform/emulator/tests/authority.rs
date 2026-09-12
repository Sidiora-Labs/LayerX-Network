//! Authority-resolution parity between the emulator core and an executing node.
//!
//! The emulator bridge resolves the authority of every activity through the
//! same `lxp_authority_resolve_activity` entry an executing node uses, so the
//! grant it binds, the scope it admits and the authority hash it derives are
//! the node's, not a locally synthesised stand-in. These tests drive the real
//! bridge over its C boundary, recompute the authority-hash preimage
//! independently in Rust from the published domain tag, and pin every field of
//! the owner grant the node would build for the same activity.

use std::ffi::{c_void, CStr};
use std::os::raw::{c_char, c_int, c_uchar, c_uint, c_ulonglong};

const EMULATOR_SEED: [u8; 32] = [0x42; 32];
const ACTOR_DID: &[u8] = b"did:layerx:authority";
const UNKNOWN_DID: &[u8] = b"did:layerx:unfunded";
const NETWORK_ID: u32 = 402;
const PROTOCOL_VERSION: u16 = 3;
const CLOCK_MS: u64 = 1_700_000_000_000;
const NOT_BEFORE_MS: u64 = 1_699_999_970_000;
const NOT_AFTER_MS: u64 = 1_700_000_120_000;
const TIMESTAMP_WINDOW_MS: u64 = 86_400_000;
const ASSET_MODULE: u16 = 1;
const GOVERNANCE_MODULE: u16 = 7;
const PROGRAMS_MODULE: u16 = 9;
const ORDINAL_MINIMUM: u16 = 1;
const ORDINAL_MAXIMUM: u16 = 11;
const AUTHORITY_OWNER: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct CoreAuthority {
    actor: [u8; 32],
    principal: [u8; 32],
    verified_key: [u8; 32],
    grant_id: [u8; 32],
    grantor: [u8; 32],
    grantee: [u8; 32],
    authority_hash: [u8; 32],
    kind: c_uint,
    not_before: c_ulonglong,
    not_after: c_ulonglong,
    scope_module_mask: c_ulonglong,
    scope_activity_ordinal_min: u16,
    scope_activity_ordinal_max: u16,
    scope_maximum_per_activity_hi: c_ulonglong,
    scope_maximum_per_activity_lo: c_ulonglong,
    scope_maximum_total_hi: c_ulonglong,
    scope_maximum_total_lo: c_ulonglong,
    scope_maximum_per_period_hi: c_ulonglong,
    scope_maximum_per_period_lo: c_ulonglong,
    revoked: u8,
}

impl CoreAuthority {
    fn blank() -> Self {
        Self {
            actor: [0; 32],
            principal: [0; 32],
            verified_key: [0; 32],
            grant_id: [0; 32],
            grantor: [0; 32],
            grantee: [0; 32],
            authority_hash: [0; 32],
            kind: 0,
            not_before: 0,
            not_after: 0,
            scope_module_mask: 0,
            scope_activity_ordinal_min: 0,
            scope_activity_ordinal_max: 0,
            scope_maximum_per_activity_hi: 0,
            scope_maximum_per_activity_lo: 0,
            scope_maximum_total_hi: 0,
            scope_maximum_total_lo: 0,
            scope_maximum_per_period_hi: 0,
            scope_maximum_per_period_lo: 0,
            revoked: 0,
        }
    }
}

unsafe extern "C" {
    fn platform_emulator_create_for_protocol(
        network_id: c_uint,
        timestamp_ms: c_ulonglong,
        sequencer_seed: *const c_uchar,
        protocol_version: u16,
    ) -> *mut c_void;
    fn platform_emulator_destroy(emulator: *mut c_void);
    fn platform_emulator_error_name(result: c_int) -> *const c_char;
    fn platform_emulator_prefund(
        emulator: *mut c_void,
        did: *const c_uchar,
        did_length: usize,
        public_key: *const c_uchar,
        amount_hi: c_ulonglong,
        amount_lo: c_ulonglong,
    ) -> c_int;
    fn platform_emulator_resolve_authority(
        emulator: *mut c_void,
        activity: *const c_uchar,
        length: usize,
        authority: *mut CoreAuthority,
    ) -> c_int;
}

struct Emulator {
    handle: *mut c_void,
}

impl Emulator {
    fn boot() -> Result<Self, String> {
        let handle = unsafe {
            platform_emulator_create_for_protocol(
                NETWORK_ID,
                CLOCK_MS,
                EMULATOR_SEED.as_ptr(),
                PROTOCOL_VERSION,
            )
        };
        if handle.is_null() {
            return Err("the emulator core refused to boot".to_owned());
        }
        Ok(Self { handle })
    }

    fn prefund(&self, did: &[u8], public_key: &[u8; 32], amount_lo: u64) -> Result<(), String> {
        let status = unsafe {
            platform_emulator_prefund(
                self.handle,
                did.as_ptr(),
                did.len(),
                public_key.as_ptr(),
                0,
                amount_lo,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(format!("prefund refused with {}", error_name(status)))
        }
    }

    fn resolve(&self, activity: &[u8]) -> Result<CoreAuthority, String> {
        let mut view = CoreAuthority::blank();
        let status = unsafe {
            platform_emulator_resolve_authority(
                self.handle,
                activity.as_ptr(),
                activity.len(),
                &raw mut view,
            )
        };
        if status == 0 {
            Ok(view)
        } else {
            Err(error_name(status))
        }
    }
}

impl Drop for Emulator {
    fn drop(&mut self) {
        unsafe { platform_emulator_destroy(self.handle) }
    }
}

fn error_name(status: c_int) -> String {
    let name = unsafe { platform_emulator_error_name(status) };
    if name.is_null() {
        return format!("status {status}");
    }
    unsafe { CStr::from_ptr(name) }
        .to_string_lossy()
        .into_owned()
}

fn checked<T, E: std::fmt::Debug>(result: Result<T, E>) -> Result<T, String> {
    result.map_err(|error| format!("{error:?}"))
}

fn sha256(parts: &[&[u8]]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Recomputes the core authority hash from the published domain tag exactly as
/// `lxp_authority_hash` derives it: the tag, the one-byte kind, the grant
/// identifier and the verified key.
fn authority_hash(kind: u8, grant_id: &[u8; 32], verified_key: &[u8; 32]) -> [u8; 32] {
    use layerx_wire::hash::Domain;
    sha256(&[Domain::AuthorityHash.tag(), &[kind], grant_id, verified_key])
}

/// Recomputes the core DID identifier exactly as `lxp_did_id_derive` does: the
/// domain tag, the big-endian DID length and the DID bytes.
fn did_identifier(did: &[u8]) -> Result<[u8; 32], String> {
    use layerx_wire::hash::Domain;
    let length = u16::try_from(did.len()).map_err(|error| error.to_string())?;
    Ok(sha256(&[Domain::DidId.tag(), &length.to_be_bytes(), did]))
}

fn signed_activity(
    did: &[u8],
    ordinal: u16,
    not_before: u64,
    not_after: u64,
    sequence: u64,
) -> Result<Vec<u8>, String> {
    use ed25519_dalek::{Signer, SigningKey};
    use layerx_types::activity::{Authority, EnvelopeBuilder, Signature, TimestampBound};
    use layerx_types::amount::Amount;
    use layerx_types::ids::{Did, IdempotencyKey};
    use layerx_types::payload::{
        ActivityType, ModuleId, ModuleRegistration, ModuleRegistry, Payload,
    };
    let key = SigningKey::from_bytes(&EMULATOR_SEED);
    let kind = checked(ActivityType::new(ModuleId::Programs, ordinal))?;
    let registry = checked(ModuleRegistry::new(&[checked(ModuleRegistration::new(
        ModuleId::Programs,
        &[kind],
    ))?]))?;
    let payload = checked(Payload::new(&registry, kind, &[0x11, 0x22, 0x33, 0x44]))?;
    let hash = checked(layerx_wire::hash::payload_hash_for(&payload))?;
    let mut idempotency = [9; 32];
    idempotency[24..].copy_from_slice(&sequence.to_be_bytes());
    let mut builder = EnvelopeBuilder::new();
    checked(builder.protocol_version(PROTOCOL_VERSION))?;
    checked(builder.network_id(NETWORK_ID))?;
    checked(builder.activity_type(kind))?;
    checked(builder.actor_did(checked(Did::new(did))?))?;
    checked(builder.authority(checked(Authority::owner(&key.verifying_key().to_bytes()))?))?;
    checked(builder.account_sequence(sequence))?;
    checked(builder.timestamp_bound(checked(TimestampBound::new(not_before, not_after))?))?;
    checked(builder.idempotency_key(IdempotencyKey::new(idempotency)))?;
    checked(builder.fee_limit(Amount::from_u128(1_000_000)))?;
    checked(builder.payload_hash(hash))?;
    checked(builder.payload(payload))?;
    let unsigned = checked(builder.build())?;
    let preimage = checked(layerx_wire::sign::preimage_unsigned(&unsigned))?;
    let signature = key.sign(preimage.as_bytes()).to_bytes();
    checked(layerx_wire::activity::encode_signed_envelope(
        &unsigned.attach_signature(checked(Signature::new(&signature))?),
    ))
}

fn booted() -> Result<(Emulator, [u8; 32]), String> {
    use ed25519_dalek::SigningKey;
    let public = SigningKey::from_bytes(&EMULATOR_SEED)
        .verifying_key()
        .to_bytes();
    let emulator = Emulator::boot()?;
    emulator.prefund(ACTOR_DID, &public, 100_000_000)?;
    Ok((emulator, public))
}

fn check_owner_grant(view: &CoreAuthority, public: &[u8; 32]) -> Result<(), String> {
    let did = did_identifier(ACTOR_DID)?;
    assert_eq!(view.kind, AUTHORITY_OWNER);
    assert_eq!(view.revoked, 0);
    assert_eq!(view.verified_key, *public);
    assert_eq!(view.grantor, did);
    assert_eq!(view.grantee, did);
    assert_eq!(view.actor, did);
    assert_eq!(view.principal, did);
    assert_ne!(view.grant_id, [0u8; 32]);
    assert_eq!(view.not_before, NOT_BEFORE_MS);
    assert_eq!(view.not_after, NOT_AFTER_MS + 1);
    assert_ne!(view.not_after, u64::MAX);
    Ok(())
}

fn check_declared_envelope_scope(view: &CoreAuthority) {
    let declared = (1u64 << ASSET_MODULE) | (1u64 << GOVERNANCE_MODULE) | (1u64 << PROGRAMS_MODULE);
    assert_eq!(view.scope_module_mask, declared);
    assert_ne!(view.scope_module_mask, u64::MAX);
    assert_eq!(view.scope_activity_ordinal_min, ORDINAL_MINIMUM);
    assert_eq!(view.scope_activity_ordinal_max, ORDINAL_MAXIMUM);
    assert_eq!(view.scope_maximum_per_activity_hi, 0);
    assert_eq!(view.scope_maximum_per_activity_lo, 0);
    assert_eq!(view.scope_maximum_total_hi, 0);
    assert_eq!(view.scope_maximum_total_lo, 0);
    assert_eq!(view.scope_maximum_per_period_hi, 0);
    assert_eq!(view.scope_maximum_per_period_lo, 0);
}

/// The emulator library publishes the build-script link directives for the
/// LayerX C core, so the bridge entry points these tests call resolve only when
/// the library itself is part of this binary. Driving its public entry keeps
/// that dependency explicit and pins the refusal an empty command line earns.
#[test]
fn emulator_entry_refuses_an_empty_command_line() {
    assert!(layerx_platform_emulator::run(Vec::<String>::new()).is_err());
}

#[test]
fn emulator_binds_the_node_owner_grant_and_its_authority_hash() -> Result<(), String> {
    let (emulator, public) = booted()?;
    let activity = signed_activity(ACTOR_DID, 3, NOT_BEFORE_MS, NOT_AFTER_MS, 0)?;
    let view = emulator.resolve(&activity).map_err(|name| {
        format!("the emulator refused to resolve a signed owner activity with {name}")
    })?;
    check_owner_grant(&view, &public)?;
    check_declared_envelope_scope(&view);
    let kind = u8::try_from(view.kind).map_err(|error| error.to_string())?;
    assert_eq!(
        view.authority_hash,
        authority_hash(kind, &view.grant_id, &view.verified_key)
    );
    assert_ne!(
        view.authority_hash,
        authority_hash(kind, &[0u8; 32], &view.verified_key)
    );
    Ok(())
}

#[test]
fn emulator_authority_hash_is_stable_across_equal_activities() -> Result<(), String> {
    let (emulator, public) = booted()?;
    let first = emulator.resolve(&signed_activity(
        ACTOR_DID,
        3,
        NOT_BEFORE_MS,
        NOT_AFTER_MS,
        0,
    )?)?;
    let second = emulator.resolve(&signed_activity(
        ACTOR_DID,
        1,
        NOT_BEFORE_MS,
        NOT_AFTER_MS,
        1,
    )?)?;
    let shifted = emulator.resolve(&signed_activity(
        ACTOR_DID,
        3,
        NOT_BEFORE_MS + 1,
        NOT_AFTER_MS,
        2,
    )?)?;
    check_owner_grant(&first, &public)?;
    check_declared_envelope_scope(&first);
    assert_eq!(first.grant_id, second.grant_id);
    assert_eq!(first.authority_hash, second.authority_hash);
    assert_ne!(first.grant_id, shifted.grant_id);
    assert_ne!(first.authority_hash, shifted.authority_hash);
    Ok(())
}

#[test]
fn emulator_refuses_authority_the_node_would_refuse() -> Result<(), String> {
    let (emulator, _) = booted()?;
    let unknown = signed_activity(UNKNOWN_DID, 3, NOT_BEFORE_MS, NOT_AFTER_MS, 0)?;
    assert_eq!(
        emulator.resolve(&unknown).err(),
        Some("LXP_ERR_UNKNOWN_DID".to_owned())
    );
    let early = signed_activity(ACTOR_DID, 3, CLOCK_MS + 1_000, CLOCK_MS + 2_000, 1)?;
    assert_eq!(
        emulator.resolve(&early).err(),
        Some("LXP_ERR_NOT_YET_VALID".to_owned())
    );
    let expired = signed_activity(ACTOR_DID, 3, CLOCK_MS - 2_000, CLOCK_MS - 1_000, 2)?;
    assert_eq!(
        emulator.resolve(&expired).err(),
        Some("LXP_ERR_EXPIRED".to_owned())
    );
    let unbounded = signed_activity(
        ACTOR_DID,
        3,
        NOT_BEFORE_MS,
        NOT_BEFORE_MS + TIMESTAMP_WINDOW_MS + 1,
        3,
    )?;
    assert_eq!(
        emulator.resolve(&unbounded).err(),
        Some("LXP_ERR_MALFORMED_ENVELOPE".to_owned())
    );
    Ok(())
}
