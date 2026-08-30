//! Canonical intent compilation through the protocol wire authority.

use layerx_types::account::AccountId;
use layerx_types::ids::Did;
use layerx_types::intent::{GrantSchedule, RolloverPolicy};
use layerx_types::limits::MAX_PAYLOAD_BYTES;
use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistry, Payload, PayloadError};
use layerx_wire::encode::Encoder;
use layerx_wire::hash;
use layerx_wire::WireError;

use crate::{Intent, IntentKind};

const ASSET_SEND_TAG: u16 = 0x5301;
const ASSET_SEND_FIELD_COUNT: u16 = 10;

/// A module payload, its registered activity type, and its domain-tagged hash.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledIntent {
    activity_type: ActivityType,
    payload: Payload,
    payload_hash: [u8; 32],
}

impl CompiledIntent {
    #[must_use]
    pub const fn activity_type(&self) -> ActivityType {
        self.activity_type
    }

    #[must_use]
    pub const fn payload(&self) -> &Payload {
        &self.payload
    }

    #[must_use]
    pub const fn payload_hash(&self) -> [u8; 32] {
        self.payload_hash
    }
}

/// Field whose canonical compilation failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompileField {
    ActivityType,
    Version,
    Header,
    Did,
    PrimaryKey,
    PendingKey,
    ChallengeWindow,
    Sequence,
    RecoveryRoot,
    Threshold,
    Network,
    PayoutAddress,
    OwnershipSignature,
    SessionGrant,
    AuthorityGrant,
    RevocationReason,
    From,
    To,
    Recipient,
    Account,
    Asset,
    Amount,
    IdempotencyKey,
    Expiration,
    ContextHash,
    Authorization,
    PayerGrant,
    Schedule,
    Allowance,
    Purpose,
    Budget,
    Period,
    Rollover,
    CarryCap,
    DepositProof,
    Checkpoint,
    Withdrawal,
    Payload,
    PayloadHash,
}

/// Protocol authority that refused compilation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompileErrorReason {
    Wire(WireError),
    Payload(PayloadError),
}

/// Canonical compilation failure naming the exact intent field.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompileError {
    pub field: CompileField,
    pub reason: CompileErrorReason,
}

impl CompileError {
    const fn wire(field: CompileField, reason: WireError) -> Self {
        Self {
            field,
            reason: CompileErrorReason::Wire(reason),
        }
    }

    const fn payload(field: CompileField, reason: PayloadError) -> Self {
        Self {
            field,
            reason: CompileErrorReason::Payload(reason),
        }
    }
}

/// Compiles a validated human intent to the registered canonical module payload.
///
/// # Errors
///
/// Returns a typed field-specific error when the wire encoder, domain hash, or
/// core-negotiated module registry refuses the value.
#[allow(clippy::too_many_lines)]
pub fn compile(intent: &Intent, registry: &ModuleRegistry) -> Result<CompiledIntent, CompileError> {
    let mut encoder = Encoder::new(MAX_PAYLOAD_BYTES);
    match intent.kind() {
        IntentKind::DidRegistration(value) => {
            header(&mut encoder, 0x7101, 2)?;
            did(&mut encoder, &value.did, CompileField::Did)?;
            fixed(
                &mut encoder,
                &value.primary_key.bytes(),
                CompileField::PrimaryKey,
            )?;
            finish(registry, ModuleId::Governance, 1, encoder)
        }
        IntentKind::KeyRotation(value) => {
            header(&mut encoder, 0x7102, 4)?;
            did(&mut encoder, &value.did, CompileField::Did)?;
            fixed(
                &mut encoder,
                &value.pending_key.bytes(),
                CompileField::PendingKey,
            )?;
            wire(
                CompileField::ChallengeWindow,
                encoder.u64(value.challenge_window.not_before()),
            )?;
            wire(
                CompileField::ChallengeWindow,
                encoder.u64(value.challenge_window.not_after()),
            )?;
            wire(
                CompileField::Sequence,
                encoder.u64(value.effective_sequence.value()),
            )?;
            finish(registry, ModuleId::Governance, 2, encoder)
        }
        IntentKind::RecoveryRegistration(value) => {
            header(&mut encoder, 0x7103, 3)?;
            did(&mut encoder, &value.did, CompileField::Did)?;
            fixed(
                &mut encoder,
                &value.recovery_root.bytes(),
                CompileField::RecoveryRoot,
            )?;
            wire(
                CompileField::Threshold,
                encoder.u16(value.threshold.value()),
            )?;
            finish(registry, ModuleId::Governance, 3, encoder)
        }
        IntentKind::EvmPayoutBinding(value) => {
            header(&mut encoder, 0x7104, 4)?;
            did(&mut encoder, &value.did, CompileField::Did)?;
            wire(CompileField::Network, encoder.u32(value.network.value()))?;
            fixed(
                &mut encoder,
                &value.payout_address.bytes(),
                CompileField::PayoutAddress,
            )?;
            wire(
                CompileField::OwnershipSignature,
                encoder.bytes(value.ownership_signature.as_bytes(), 128),
            )?;
            finish(registry, ModuleId::Governance, 4, encoder)
        }
        IntentKind::SessionGrant(value) => {
            header(&mut encoder, 0x7105, 1)?;
            wire(
                CompileField::SessionGrant,
                encoder.bytes(&value.registration_payload, 1024),
            )?;
            finish(registry, ModuleId::Governance, 5, encoder)
        }
        IntentKind::SessionRevoke(value) => {
            header(&mut encoder, 0x7106, 3)?;
            fixed(
                &mut encoder,
                &value.grant_id.bytes(),
                CompileField::AuthorityGrant,
            )?;
            wire(
                CompileField::RevocationReason,
                encoder.u8(value.reason.value()),
            )?;
            wire(
                CompileField::Sequence,
                encoder.u64(value.effective_sequence.value()),
            )?;
            finish(registry, ModuleId::Governance, 6, encoder)
        }
        IntentKind::LxpSend(value) => {
            wire(CompileField::Header, encoder.u16(ASSET_SEND_TAG))?;
            wire(CompileField::Header, encoder.u16(ASSET_SEND_FIELD_COUNT))?;
            let from = account(&mut encoder, &value.from, CompileField::From)?;
            account(&mut encoder, &value.to, CompileField::To)?;
            fixed(&mut encoder, &value.asset.bytes(), CompileField::Asset)?;
            wire(CompileField::Amount, encoder.u128(value.amount.value()))?;
            wire(
                CompileField::Sequence,
                encoder.u64(value.account_sequence.value()),
            )?;
            fixed(
                &mut encoder,
                &value.idempotency_key.bytes(),
                CompileField::IdempotencyKey,
            )?;
            wire(
                CompileField::Expiration,
                encoder.u64(value.expires_at.value()),
            )?;
            fixed(
                &mut encoder,
                &value.context_hash.bytes(),
                CompileField::ContextHash,
            )?;
            wire(CompileField::Authorization, encoder.u8(0))?;
            wire(
                CompileField::Authorization,
                encoder.u8(value.authorization.kind() as u8),
            )?;
            fixed(&mut encoder, &from, CompileField::Authorization)?;
            fixed(
                &mut encoder,
                &value.authorization.public_key().bytes(),
                CompileField::Authorization,
            )?;
            fixed(
                &mut encoder,
                &value.authorization.signature().bytes(),
                CompileField::Authorization,
            )?;
            fixed(
                &mut encoder,
                &value.context_hash.bytes(),
                CompileField::ContextHash,
            )?;
            wire(CompileField::Network, encoder.u32(value.network_id.value()))?;
            wire(
                CompileField::Version,
                encoder.u16(value.protocol_version.value()),
            )?;
            finish(registry, ModuleId::Asset, 5, encoder)
        }
        IntentKind::LxpReceive(value) => {
            header(&mut encoder, 0x5201, 8)?;
            account(&mut encoder, &value.from, CompileField::From)?;
            account(&mut encoder, &value.to, CompileField::To)?;
            fixed(&mut encoder, &value.asset.bytes(), CompileField::Asset)?;
            wire(CompileField::Amount, encoder.u128(value.amount.value()))?;
            fixed(
                &mut encoder,
                &value.payer_grant.bytes(),
                CompileField::PayerGrant,
            )?;
            wire(
                CompileField::Sequence,
                encoder.u64(value.receiver_sequence.value()),
            )?;
            fixed(
                &mut encoder,
                &value.idempotency_key.bytes(),
                CompileField::IdempotencyKey,
            )?;
            fixed(
                &mut encoder,
                &value.context_hash.bytes(),
                CompileField::ContextHash,
            )?;
            finish(registry, ModuleId::Asset, 6, encoder)
        }
        IntentKind::PayerGrantRegistration(value) => {
            header(&mut encoder, 0x4701, 10)?;
            fixed(
                &mut encoder,
                &value.grant_id.bytes(),
                CompileField::PayerGrant,
            )?;
            account(&mut encoder, &value.from, CompileField::From)?;
            account(&mut encoder, &value.recipient, CompileField::Recipient)?;
            fixed(&mut encoder, &value.asset.bytes(), CompileField::Asset)?;
            wire(
                CompileField::Amount,
                encoder.u128(value.per_draw_maximum.value()),
            )?;
            wire(
                CompileField::Allowance,
                encoder.u128(value.allowance.value()),
            )?;
            schedule(&mut encoder, value.schedule)?;
            wire(
                CompileField::Expiration,
                encoder.u64(value.expiration.value()),
            )?;
            fixed(&mut encoder, &value.purpose.bytes(), CompileField::Purpose)?;
            fixed(
                &mut encoder,
                &value.public_key.bytes(),
                CompileField::PrimaryKey,
            )?;
            finish(registry, ModuleId::Budget, 4, encoder)
        }
        IntentKind::BudgetCreate(value) => {
            header(&mut encoder, 0x4201, 10)?;
            fixed(&mut encoder, &value.budget_id.bytes(), CompileField::Budget)?;
            account(&mut encoder, &value.owner, CompileField::From)?;
            account(&mut encoder, &value.budget_account, CompileField::Account)?;
            fixed(&mut encoder, &value.asset.bytes(), CompileField::Asset)?;
            wire(
                CompileField::Amount,
                encoder.u128(value.per_period_limit.value()),
            )?;
            wire(
                CompileField::Period,
                encoder.u64(value.period_length.value()),
            )?;
            rollover(&mut encoder, value.rollover)?;
            wire(
                CompileField::CarryCap,
                encoder.u128(value.carry_cap.value()),
            )?;
            fixed(&mut encoder, &value.purpose.bytes(), CompileField::Purpose)?;
            wire(CompileField::Expiration, encoder.u64(value.expiry.value()))?;
            finish(registry, ModuleId::Budget, 1, encoder)
        }
        IntentKind::BudgetFund(value) => {
            header(&mut encoder, 0x4202, 6)?;
            fixed(&mut encoder, &value.budget_id.bytes(), CompileField::Budget)?;
            account(&mut encoder, &value.owner, CompileField::From)?;
            account(&mut encoder, &value.budget_account, CompileField::To)?;
            fixed(&mut encoder, &value.asset.bytes(), CompileField::Asset)?;
            wire(CompileField::Amount, encoder.u128(value.amount.value()))?;
            fixed(
                &mut encoder,
                &value.idempotency_key.bytes(),
                CompileField::IdempotencyKey,
            )?;
            finish(registry, ModuleId::Budget, 2, encoder)
        }
        IntentKind::BudgetDefund(value) => {
            header(&mut encoder, 0x4207, 7)?;
            fixed(&mut encoder, &value.budget_id.bytes(), CompileField::Budget)?;
            account(&mut encoder, &value.budget_account, CompileField::From)?;
            account(&mut encoder, &value.owner, CompileField::To)?;
            fixed(&mut encoder, &value.asset.bytes(), CompileField::Asset)?;
            wire(CompileField::Amount, encoder.u128(value.amount.value()))?;
            wire(
                CompileField::Sequence,
                encoder.u64(value.revocation_sequence.value()),
            )?;
            fixed(
                &mut encoder,
                &value.idempotency_key.bytes(),
                CompileField::IdempotencyKey,
            )?;
            finish(registry, ModuleId::Budget, 7, encoder)
        }
        IntentKind::BridgeDepositCredit(value) => {
            header(&mut encoder, 0x4801, 7)?;
            fixed(
                &mut encoder,
                &value.deposit_proof.bytes(),
                CompileField::DepositProof,
            )?;
            fixed(
                &mut encoder,
                &value.checkpoint.bytes(),
                CompileField::Checkpoint,
            )?;
            account(&mut encoder, &value.reserve, CompileField::From)?;
            account(&mut encoder, &value.recipient, CompileField::Recipient)?;
            fixed(&mut encoder, &value.asset.bytes(), CompileField::Asset)?;
            wire(CompileField::Amount, encoder.u128(value.amount.value()))?;
            fixed(
                &mut encoder,
                &value.idempotency_key.bytes(),
                CompileField::IdempotencyKey,
            )?;
            finish(registry, ModuleId::Bridge, 1, encoder)
        }
        IntentKind::BridgeWithdrawRequest(value) => {
            header(&mut encoder, 0x4802, 7)?;
            fixed(
                &mut encoder,
                &value.withdrawal_id.bytes(),
                CompileField::Withdrawal,
            )?;
            account(&mut encoder, &value.owner, CompileField::From)?;
            account(&mut encoder, &value.withdrawals_account, CompileField::To)?;
            fixed(
                &mut encoder,
                &value.payout_address.bytes(),
                CompileField::PayoutAddress,
            )?;
            fixed(&mut encoder, &value.asset.bytes(), CompileField::Asset)?;
            wire(CompileField::Amount, encoder.u128(value.amount.value()))?;
            fixed(
                &mut encoder,
                &value.idempotency_key.bytes(),
                CompileField::IdempotencyKey,
            )?;
            finish(registry, ModuleId::Bridge, 2, encoder)
        }
    }
}

fn header(encoder: &mut Encoder, tag: u16, field_count: u16) -> Result<(), CompileError> {
    wire(CompileField::Header, encoder.u16(tag))?;
    wire(CompileField::Header, encoder.u16(field_count))
}

fn fixed(encoder: &mut Encoder, value: &[u8], field: CompileField) -> Result<(), CompileError> {
    wire(field, encoder.fixed(value))
}

fn account(
    encoder: &mut Encoder,
    value: &AccountId,
    field: CompileField,
) -> Result<[u8; 32], CompileError> {
    let digest = hash::account_id(value).map_err(|error| CompileError::wire(field, error))?;
    fixed(encoder, &digest, field)?;
    Ok(digest)
}

fn did(encoder: &mut Encoder, value: &Did, field: CompileField) -> Result<(), CompileError> {
    let digest = hash::did_id(value).map_err(|error| CompileError::wire(field, error))?;
    fixed(encoder, &digest, field)
}

fn schedule(encoder: &mut Encoder, value: GrantSchedule) -> Result<(), CompileError> {
    match value {
        GrantSchedule::SingleUse => wire(CompileField::Schedule, encoder.u8(1)),
        GrantSchedule::Recurring(period) => {
            wire(CompileField::Schedule, encoder.u8(2))?;
            wire(CompileField::Schedule, encoder.u64(period.value()))
        }
    }
}

fn rollover(encoder: &mut Encoder, value: RolloverPolicy) -> Result<(), CompileError> {
    let tag = match value {
        RolloverPolicy::None => 1,
        RolloverPolicy::Capped => 2,
    };
    wire(CompileField::Rollover, encoder.u8(tag))
}

fn wire(field: CompileField, result: Result<(), WireError>) -> Result<(), CompileError> {
    result.map_err(|error| CompileError::wire(field, error))
}

fn finish(
    registry: &ModuleRegistry,
    module: ModuleId,
    ordinal: u16,
    encoder: Encoder,
) -> Result<CompiledIntent, CompileError> {
    let activity_type = ActivityType::new(module, ordinal)
        .map_err(|error| CompileError::payload(CompileField::ActivityType, error))?;
    let payload = Payload::new(registry, activity_type, &encoder.finish())
        .map_err(|error| CompileError::payload(CompileField::Payload, error))?;
    let payload_hash = hash::payload_hash_for(&payload)
        .map_err(|error| CompileError::wire(CompileField::PayloadHash, error))?;
    Ok(CompiledIntent {
        activity_type,
        payload,
        payload_hash,
    })
}
