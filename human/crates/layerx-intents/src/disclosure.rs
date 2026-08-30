//! Independent decode, field comparison, and canonical re-encoding gate.

use layerx_crypto::disclosure::{AmountRole, CounterpartyRole, Disclosure};
use layerx_types::account::AccountId;
use layerx_types::ids::Did;
use layerx_types::intent::{GrantSchedule, RolloverPolicy};
use layerx_types::limits::MAX_PAYLOAD_BYTES;
use layerx_types::payload::{ActivityType, ModuleId, PayloadError};
use layerx_wire::decode::Decoder;
use layerx_wire::encode::Encoder;
use layerx_wire::hash;
use layerx_wire::WireError;

use crate::{CompiledIntent, Intent, IntentKind};

/// Intent or disclosure field named by a differential-gate failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisclosureField {
    ActivityType,
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
    Account,
    Period,
    Rollover,
    CarryCap,
    DepositProof,
    Checkpoint,
    Withdrawal,
    PayloadHash,
    PayloadBytes,
}

/// A defect found before any signature request can be issued.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisclosureCheckError {
    FieldMismatch(DisclosureField),
    Wire {
        field: DisclosureField,
        error: WireError,
    },
    Payload {
        field: DisclosureField,
        error: PayloadError,
    },
    UnsupportedAgentDisclosure,
}

/// Evidence that an independently decoded disclosure matched its intent and
/// re-encoded to the exact compiled payload bytes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisclosureCheck {
    activity_type: ActivityType,
    canonical_payload: Vec<u8>,
    payload_hash: [u8; 32],
}

impl DisclosureCheck {
    /// Independently decodes every field, compares it with the originating
    /// intent, and re-encodes through `layerx-wire`.
    ///
    /// # Errors
    ///
    /// Returns the first mismatched or non-canonical field. A caller must
    /// abort the journey on every error.
    #[allow(clippy::too_many_lines)]
    pub fn verify(
        intent: &Intent,
        compiled: &CompiledIntent,
    ) -> Result<Self, DisclosureCheckError> {
        let expected_type = expected_activity_type(intent)?;
        if compiled.activity_type() != expected_type
            || compiled.payload().activity_type() != expected_type
        {
            return Err(DisclosureCheckError::FieldMismatch(
                DisclosureField::ActivityType,
            ));
        }

        let payload = compiled.payload().as_bytes();
        let mut round_trip = RoundTrip::new(payload);
        match intent.kind() {
            IntentKind::DidRegistration(value) => {
                round_trip.header(0x7101, 2)?;
                round_trip.did(&value.did, DisclosureField::Did)?;
                round_trip.fixed(&value.primary_key.bytes(), DisclosureField::PrimaryKey)?;
            }
            IntentKind::KeyRotation(value) => {
                round_trip.header(0x7102, 4)?;
                round_trip.did(&value.did, DisclosureField::Did)?;
                round_trip.fixed(&value.pending_key.bytes(), DisclosureField::PendingKey)?;
                round_trip.u64(
                    value.challenge_window.not_before(),
                    DisclosureField::ChallengeWindow,
                )?;
                round_trip.u64(
                    value.challenge_window.not_after(),
                    DisclosureField::ChallengeWindow,
                )?;
                round_trip.u64(value.effective_sequence.value(), DisclosureField::Sequence)?;
            }
            IntentKind::RecoveryRegistration(value) => {
                round_trip.header(0x7103, 3)?;
                round_trip.did(&value.did, DisclosureField::Did)?;
                round_trip.fixed(&value.recovery_root.bytes(), DisclosureField::RecoveryRoot)?;
                round_trip.u16(value.threshold.value(), DisclosureField::Threshold)?;
            }
            IntentKind::EvmPayoutBinding(value) => {
                round_trip.header(0x7104, 4)?;
                round_trip.did(&value.did, DisclosureField::Did)?;
                round_trip.u32(value.network.value(), DisclosureField::Network)?;
                round_trip.fixed(
                    &value.payout_address.bytes(),
                    DisclosureField::PayoutAddress,
                )?;
                round_trip.bytes(
                    value.ownership_signature.as_bytes(),
                    128,
                    DisclosureField::OwnershipSignature,
                )?;
            }
            IntentKind::SessionGrant(value) => {
                round_trip.header(0x7105, 1)?;
                round_trip.bytes(
                    &value.registration_payload,
                    1024,
                    DisclosureField::SessionGrant,
                )?;
            }
            IntentKind::SessionRevoke(value) => {
                round_trip.header(0x7106, 3)?;
                round_trip.fixed(&value.grant_id.bytes(), DisclosureField::AuthorityGrant)?;
                round_trip.u8(value.reason.value(), DisclosureField::RevocationReason)?;
                round_trip.u64(value.effective_sequence.value(), DisclosureField::Sequence)?;
            }
            IntentKind::LxpSend(value) => {
                round_trip.header(0x5301, 10)?;
                let from = round_trip.account(&value.from, DisclosureField::From)?;
                round_trip.account(&value.to, DisclosureField::To)?;
                round_trip.fixed(&value.asset.bytes(), DisclosureField::Asset)?;
                round_trip.u128(value.amount.value(), DisclosureField::Amount)?;
                round_trip.u64(value.account_sequence.value(), DisclosureField::Sequence)?;
                round_trip.fixed(
                    &value.idempotency_key.bytes(),
                    DisclosureField::IdempotencyKey,
                )?;
                round_trip.u64(value.expires_at.value(), DisclosureField::Expiration)?;
                round_trip.fixed(&value.context_hash.bytes(), DisclosureField::ContextHash)?;
                round_trip.u8(0, DisclosureField::Authorization)?;
                round_trip.u8(
                    value.authorization.kind() as u8,
                    DisclosureField::Authorization,
                )?;
                round_trip.fixed(&from, DisclosureField::Authorization)?;
                round_trip.fixed(
                    &value.authorization.public_key().bytes(),
                    DisclosureField::Authorization,
                )?;
                round_trip.fixed(
                    &value.authorization.signature().bytes(),
                    DisclosureField::Authorization,
                )?;
                round_trip.fixed(&value.context_hash.bytes(), DisclosureField::ContextHash)?;
                round_trip.u32(value.network_id.value(), DisclosureField::Network)?;
                round_trip.u16(value.protocol_version.value(), DisclosureField::Header)?;
            }
            IntentKind::LxpReceive(value) => {
                round_trip.header(0x5201, 8)?;
                round_trip.account(&value.from, DisclosureField::From)?;
                round_trip.account(&value.to, DisclosureField::To)?;
                round_trip.fixed(&value.asset.bytes(), DisclosureField::Asset)?;
                round_trip.u128(value.amount.value(), DisclosureField::Amount)?;
                round_trip.fixed(&value.payer_grant.bytes(), DisclosureField::PayerGrant)?;
                round_trip.u64(value.receiver_sequence.value(), DisclosureField::Sequence)?;
                round_trip.fixed(
                    &value.idempotency_key.bytes(),
                    DisclosureField::IdempotencyKey,
                )?;
                round_trip.fixed(&value.context_hash.bytes(), DisclosureField::ContextHash)?;
            }
            IntentKind::PayerGrantRegistration(value) => {
                round_trip.header(0x4701, 10)?;
                round_trip.fixed(&value.grant_id.bytes(), DisclosureField::PayerGrant)?;
                round_trip.account(&value.from, DisclosureField::From)?;
                round_trip.account(&value.recipient, DisclosureField::Recipient)?;
                round_trip.fixed(&value.asset.bytes(), DisclosureField::Asset)?;
                round_trip.u128(value.per_draw_maximum.value(), DisclosureField::Amount)?;
                round_trip.u128(value.allowance.value(), DisclosureField::Allowance)?;
                round_trip.schedule(value.schedule)?;
                round_trip.u64(value.expiration.value(), DisclosureField::Expiration)?;
                round_trip.fixed(&value.purpose.bytes(), DisclosureField::Purpose)?;
                round_trip.fixed(&value.public_key.bytes(), DisclosureField::PrimaryKey)?;
            }
            IntentKind::BudgetCreate(value) => {
                round_trip.header(0x4201, 10)?;
                round_trip.fixed(&value.budget_id.bytes(), DisclosureField::Budget)?;
                round_trip.account(&value.owner, DisclosureField::From)?;
                round_trip.account(&value.budget_account, DisclosureField::Account)?;
                round_trip.fixed(&value.asset.bytes(), DisclosureField::Asset)?;
                round_trip.u128(value.per_period_limit.value(), DisclosureField::Amount)?;
                round_trip.u64(value.period_length.value(), DisclosureField::Period)?;
                round_trip.rollover(value.rollover)?;
                round_trip.u128(value.carry_cap.value(), DisclosureField::CarryCap)?;
                round_trip.fixed(&value.purpose.bytes(), DisclosureField::Purpose)?;
                round_trip.u64(value.expiry.value(), DisclosureField::Expiration)?;
            }
            IntentKind::BudgetFund(value) => {
                round_trip.header(0x4202, 6)?;
                round_trip.fixed(&value.budget_id.bytes(), DisclosureField::Budget)?;
                round_trip.account(&value.owner, DisclosureField::From)?;
                round_trip.account(&value.budget_account, DisclosureField::To)?;
                round_trip.fixed(&value.asset.bytes(), DisclosureField::Asset)?;
                round_trip.u128(value.amount.value(), DisclosureField::Amount)?;
                round_trip.fixed(
                    &value.idempotency_key.bytes(),
                    DisclosureField::IdempotencyKey,
                )?;
            }
            IntentKind::BudgetDefund(value) => {
                round_trip.header(0x4207, 7)?;
                round_trip.fixed(&value.budget_id.bytes(), DisclosureField::Budget)?;
                round_trip.account(&value.budget_account, DisclosureField::From)?;
                round_trip.account(&value.owner, DisclosureField::To)?;
                round_trip.fixed(&value.asset.bytes(), DisclosureField::Asset)?;
                round_trip.u128(value.amount.value(), DisclosureField::Amount)?;
                round_trip.u64(value.revocation_sequence.value(), DisclosureField::Sequence)?;
                round_trip.fixed(
                    &value.idempotency_key.bytes(),
                    DisclosureField::IdempotencyKey,
                )?;
            }
            IntentKind::BridgeDepositCredit(value) => {
                round_trip.header(0x4801, 7)?;
                round_trip.fixed(&value.deposit_proof.bytes(), DisclosureField::DepositProof)?;
                round_trip.fixed(&value.checkpoint.bytes(), DisclosureField::Checkpoint)?;
                round_trip.account(&value.reserve, DisclosureField::From)?;
                round_trip.account(&value.recipient, DisclosureField::Recipient)?;
                round_trip.fixed(&value.asset.bytes(), DisclosureField::Asset)?;
                round_trip.u128(value.amount.value(), DisclosureField::Amount)?;
                round_trip.fixed(
                    &value.idempotency_key.bytes(),
                    DisclosureField::IdempotencyKey,
                )?;
            }
            IntentKind::BridgeWithdrawRequest(value) => {
                round_trip.header(0x4802, 7)?;
                round_trip.fixed(&value.withdrawal_id.bytes(), DisclosureField::Withdrawal)?;
                round_trip.account(&value.owner, DisclosureField::From)?;
                round_trip.account(&value.withdrawals_account, DisclosureField::To)?;
                round_trip.fixed(
                    &value.payout_address.bytes(),
                    DisclosureField::PayoutAddress,
                )?;
                round_trip.fixed(&value.asset.bytes(), DisclosureField::Asset)?;
                round_trip.u128(value.amount.value(), DisclosureField::Amount)?;
                round_trip.fixed(
                    &value.idempotency_key.bytes(),
                    DisclosureField::IdempotencyKey,
                )?;
            }
        }

        let canonical_payload = round_trip.finish()?;
        if canonical_payload != payload {
            return Err(DisclosureCheckError::FieldMismatch(
                DisclosureField::PayloadBytes,
            ));
        }
        let payload_hash = hash::payload_hash_for(compiled.payload()).map_err(|error| {
            DisclosureCheckError::Wire {
                field: DisclosureField::PayloadHash,
                error,
            }
        })?;
        if payload_hash != compiled.payload_hash() {
            return Err(DisclosureCheckError::FieldMismatch(
                DisclosureField::PayloadHash,
            ));
        }
        Ok(Self {
            activity_type: expected_type,
            canonical_payload,
            payload_hash,
        })
    }

    /// Compares every semantic field returned by the agent send disclosure
    /// with the originating intent. The agent's own decoder has already bound
    /// sequence, context, network, and version to the canonical envelope.
    ///
    /// # Errors
    ///
    /// Returns a field mismatch or refuses an intent for which the agent layer
    /// has not returned a complete disclosure type.
    pub fn verify_agent(
        intent: &Intent,
        disclosure: &Disclosure,
    ) -> Result<(), DisclosureCheckError> {
        let IntentKind::LxpSend(send) = intent.kind() else {
            return Err(DisclosureCheckError::UnsupportedAgentDisclosure);
        };
        let expected_type = expected_activity_type(intent)?;
        require(
            disclosure.activity_type == expected_type,
            DisclosureField::ActivityType,
        )?;
        let from = hash::account_id(&send.from).map_err(|error| DisclosureCheckError::Wire {
            field: DisclosureField::From,
            error,
        })?;
        let to = hash::account_id(&send.to).map_err(|error| DisclosureCheckError::Wire {
            field: DisclosureField::To,
            error,
        })?;
        require(
            disclosure.counterparties.len() == 2
                && disclosure.counterparties[0].role == CounterpartyRole::Payer
                && disclosure.counterparties[0].account == from,
            DisclosureField::From,
        )?;
        require(
            disclosure.counterparties[1].role == CounterpartyRole::Recipient
                && disclosure.counterparties[1].account == to,
            DisclosureField::To,
        )?;
        require(
            disclosure.amounts.len() == 1
                && disclosure.amounts[0].role == AmountRole::Transfer
                && disclosure.amounts[0].value == send.amount.value(),
            DisclosureField::Amount,
        )?;
        require(
            disclosure.asset == send.asset.bytes(),
            DisclosureField::Asset,
        )?;
        require(
            disclosure.idempotency_key == send.idempotency_key.bytes(),
            DisclosureField::IdempotencyKey,
        )?;
        require(
            disclosure.expiry.payload_expires_at == send.expires_at.value(),
            DisclosureField::Expiration,
        )
    }

    #[must_use]
    pub const fn activity_type(&self) -> ActivityType {
        self.activity_type
    }

    #[must_use]
    pub fn canonical_payload(&self) -> &[u8] {
        &self.canonical_payload
    }

    #[must_use]
    pub const fn payload_hash(&self) -> [u8; 32] {
        self.payload_hash
    }
}

struct RoundTrip<'a> {
    decoder: Decoder<'a>,
    encoder: Encoder,
}

impl<'a> RoundTrip<'a> {
    const fn new(payload: &'a [u8]) -> Self {
        Self {
            decoder: Decoder::new(payload, MAX_PAYLOAD_BYTES),
            encoder: Encoder::new(MAX_PAYLOAD_BYTES),
        }
    }

    fn header(&mut self, tag: u16, fields: u16) -> Result<(), DisclosureCheckError> {
        self.u16(tag, DisclosureField::Header)?;
        self.u16(fields, DisclosureField::Header)
    }

    fn u8(&mut self, expected: u8, field: DisclosureField) -> Result<(), DisclosureCheckError> {
        let actual = self
            .decoder
            .u8()
            .map_err(|error| DisclosureCheckError::Wire { field, error })?;
        require(actual == expected, field)?;
        self.encoder
            .u8(actual)
            .map_err(|error| DisclosureCheckError::Wire { field, error })
    }

    fn u16(&mut self, expected: u16, field: DisclosureField) -> Result<(), DisclosureCheckError> {
        let actual = self
            .decoder
            .u16()
            .map_err(|error| DisclosureCheckError::Wire { field, error })?;
        require(actual == expected, field)?;
        self.encoder
            .u16(actual)
            .map_err(|error| DisclosureCheckError::Wire { field, error })
    }

    fn u32(&mut self, expected: u32, field: DisclosureField) -> Result<(), DisclosureCheckError> {
        let actual = self
            .decoder
            .u32()
            .map_err(|error| DisclosureCheckError::Wire { field, error })?;
        require(actual == expected, field)?;
        self.encoder
            .u32(actual)
            .map_err(|error| DisclosureCheckError::Wire { field, error })
    }

    fn u64(&mut self, expected: u64, field: DisclosureField) -> Result<(), DisclosureCheckError> {
        let actual = self
            .decoder
            .u64()
            .map_err(|error| DisclosureCheckError::Wire { field, error })?;
        require(actual == expected, field)?;
        self.encoder
            .u64(actual)
            .map_err(|error| DisclosureCheckError::Wire { field, error })
    }

    fn u128(&mut self, expected: u128, field: DisclosureField) -> Result<(), DisclosureCheckError> {
        let actual = self
            .decoder
            .u128()
            .map_err(|error| DisclosureCheckError::Wire { field, error })?;
        require(actual == expected, field)?;
        self.encoder
            .u128(actual)
            .map_err(|error| DisclosureCheckError::Wire { field, error })
    }

    fn fixed(
        &mut self,
        expected: &[u8],
        field: DisclosureField,
    ) -> Result<(), DisclosureCheckError> {
        let actual = self
            .decoder
            .fixed(expected.len())
            .map_err(|error| DisclosureCheckError::Wire { field, error })?;
        require(actual == expected, field)?;
        self.encoder
            .fixed(actual)
            .map_err(|error| DisclosureCheckError::Wire { field, error })
    }

    fn bytes(
        &mut self,
        expected: &[u8],
        maximum: usize,
        field: DisclosureField,
    ) -> Result<(), DisclosureCheckError> {
        let actual = self
            .decoder
            .bytes(maximum)
            .map_err(|error| DisclosureCheckError::Wire { field, error })?;
        require(actual == expected, field)?;
        self.encoder
            .bytes(actual, maximum)
            .map_err(|error| DisclosureCheckError::Wire { field, error })
    }

    fn account(
        &mut self,
        account: &AccountId,
        field: DisclosureField,
    ) -> Result<[u8; 32], DisclosureCheckError> {
        let digest = hash::account_id(account)
            .map_err(|error| DisclosureCheckError::Wire { field, error })?;
        self.fixed(&digest, field)?;
        Ok(digest)
    }

    fn did(&mut self, did: &Did, field: DisclosureField) -> Result<(), DisclosureCheckError> {
        let digest =
            hash::did_id(did).map_err(|error| DisclosureCheckError::Wire { field, error })?;
        self.fixed(&digest, field)
    }

    fn schedule(&mut self, schedule: GrantSchedule) -> Result<(), DisclosureCheckError> {
        match schedule {
            GrantSchedule::SingleUse => self.u8(1, DisclosureField::Schedule),
            GrantSchedule::Recurring(period) => {
                self.u8(2, DisclosureField::Schedule)?;
                self.u64(period.value(), DisclosureField::Schedule)
            }
        }
    }

    fn rollover(&mut self, rollover: RolloverPolicy) -> Result<(), DisclosureCheckError> {
        self.u8(
            match rollover {
                RolloverPolicy::None => 1,
                RolloverPolicy::Capped => 2,
            },
            DisclosureField::Rollover,
        )
    }

    fn finish(self) -> Result<Vec<u8>, DisclosureCheckError> {
        self.decoder
            .finish()
            .map_err(|error| DisclosureCheckError::Wire {
                field: DisclosureField::PayloadBytes,
                error,
            })?;
        Ok(self.encoder.finish())
    }
}

fn expected_activity_type(intent: &Intent) -> Result<ActivityType, DisclosureCheckError> {
    let (module, ordinal) = match intent.kind() {
        IntentKind::DidRegistration(_) => (ModuleId::Governance, 1),
        IntentKind::KeyRotation(_) => (ModuleId::Governance, 2),
        IntentKind::RecoveryRegistration(_) => (ModuleId::Governance, 3),
        IntentKind::EvmPayoutBinding(_) => (ModuleId::Governance, 4),
        IntentKind::SessionGrant(_) => (ModuleId::Governance, 5),
        IntentKind::SessionRevoke(_) => (ModuleId::Governance, 6),
        IntentKind::LxpSend(_) => (ModuleId::Asset, 5),
        IntentKind::LxpReceive(_) => (ModuleId::Asset, 6),
        IntentKind::PayerGrantRegistration(_) => (ModuleId::Budget, 4),
        IntentKind::BudgetCreate(_) => (ModuleId::Budget, 1),
        IntentKind::BudgetFund(_) => (ModuleId::Budget, 2),
        IntentKind::BudgetDefund(_) => (ModuleId::Budget, 7),
        IntentKind::BridgeDepositCredit(_) => (ModuleId::Bridge, 1),
        IntentKind::BridgeWithdrawRequest(_) => (ModuleId::Bridge, 2),
    };
    ActivityType::new(module, ordinal).map_err(|error| DisclosureCheckError::Payload {
        field: DisclosureField::ActivityType,
        error,
    })
}

fn require(condition: bool, field: DisclosureField) -> Result<(), DisclosureCheckError> {
    if condition {
        Ok(())
    } else {
        Err(DisclosureCheckError::FieldMismatch(field))
    }
}
