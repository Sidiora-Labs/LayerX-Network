//! Versioned human intent vocabulary over `layerx-types` domain values.

use layerx_types::account::{AccountId, AccountNamespace};
use layerx_types::activity::{Signature, TimestampBound};
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, CheckpointId, Did, IdempotencyKey};
use layerx_types::intent::{
    ApprovalThreshold, AuthorityGrantId, BudgetId, ContextHash, DepositProofId, EvmAddress,
    GrantSchedule, NetworkId, PayerGrantId, PeriodLength, ProtocolVersion, PublicKey, PurposeHash,
    RecoveryRoot, RolloverPolicy, SendAuthorization, Sequence, SessionRevocationReason,
    TimestampSeconds, WithdrawalId,
};
#[cfg(test)]
use layerx_types::intent::{AuthorizationSignature, SendAuthorizationKind};
use layerx_types::payload::ModuleId;
use layerx_wire::decode::Decoder;

const MAX_SESSION_GRANT_BYTES: usize = 1024;

/// Version of the intent-to-canonical-payload contract.
///
/// Any change to bytes produced for an existing intent requires a new variant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u16)]
pub enum IntentVersion {
    V1 = 1,
}

impl IntentVersion {
    pub const CURRENT: Self = Self::V1;

    #[must_use]
    pub const fn value(self) -> u16 {
        self as u16
    }
}

/// A versioned intent whose fields are already valid protocol domain values.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Intent {
    version: IntentVersion,
    kind: IntentKind,
}

impl Intent {
    #[must_use]
    pub const fn v1(kind: IntentKind) -> Self {
        Self {
            version: IntentVersion::V1,
            kind,
        }
    }

    #[must_use]
    pub const fn version(&self) -> IntentVersion {
        self.version
    }

    #[must_use]
    pub const fn kind(&self) -> &IntentKind {
        &self.kind
    }

    #[must_use]
    pub const fn module(&self) -> ModuleId {
        self.kind.module()
    }
}

/// The fourteen v1 journeys and the protocol module payload each compiles to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IntentKind {
    /// Onboarding: register a DID and primary key in the governance identity registry.
    DidRegistration(DidRegistration),
    /// Identity security: announce a governance identity-key rotation.
    KeyRotation(KeyRotation),
    /// Recovery setup: register the governance recovery commitment and threshold.
    RecoveryRegistration(RecoveryRegistration),
    /// Withdrawal setup: bind an EVM payout address in the governance identity registry.
    EvmPayoutBinding(EvmPayoutBinding),
    /// Agent authority: grant a scoped, expiring protocol session.
    SessionGrant(SessionGrant),
    /// Agent authority: revoke an existing protocol session.
    SessionRevoke(SessionRevoke),
    /// Move money: compile an asset-module 402LXP send.
    LxpSend(LxpSend),
    /// Receive money: compile an asset-module payer-grant draw.
    LxpReceive(LxpReceive),
    /// Delegated payment setup: register a budget-module payer grant.
    PayerGrantRegistration(PayerGrantRegistration),
    /// Agent funding setup: create a protocol budget-module account.
    BudgetCreate(BudgetCreate),
    /// Agent funding: move assets into a protocol budget.
    BudgetFund(BudgetFund),
    /// Agent reclaim: close or reduce a protocol budget.
    BudgetDefund(BudgetDefund),
    /// Paxeer deposit: credit a proven external deposit through the bridge module.
    BridgeDepositCredit(BridgeDepositCredit),
    /// Paxeer withdrawal: request a bridge-module withdrawal.
    BridgeWithdrawRequest(BridgeWithdrawRequest),
}

impl IntentKind {
    #[must_use]
    pub const fn module(&self) -> ModuleId {
        match self {
            Self::DidRegistration(_)
            | Self::KeyRotation(_)
            | Self::RecoveryRegistration(_)
            | Self::EvmPayoutBinding(_)
            | Self::SessionGrant(_)
            | Self::SessionRevoke(_) => ModuleId::Governance,
            Self::LxpSend(_) | Self::LxpReceive(_) => ModuleId::Asset,
            Self::PayerGrantRegistration(_)
            | Self::BudgetCreate(_)
            | Self::BudgetFund(_)
            | Self::BudgetDefund(_) => ModuleId::Budget,
            Self::BridgeDepositCredit(_) | Self::BridgeWithdrawRequest(_) => ModuleId::Bridge,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionGrant {
    pub(crate) registration_payload: Vec<u8>,
}

impl SessionGrant {
    /// Builds a scoped protocol session grant.
    ///
    /// # Errors
    ///
    /// Refuses anything other than the exact bounded canonical session-key
    /// authority grant emitted by `layerx-crypto`.
    pub fn new(registration_payload: Vec<u8>) -> Result<Self, IntentError> {
        validate_session_grant(&registration_payload)?;
        Ok(Self {
            registration_payload,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRevoke {
    pub(crate) grant_id: AuthorityGrantId,
    pub(crate) reason: SessionRevocationReason,
    pub(crate) effective_sequence: Sequence,
}

impl SessionRevoke {
    /// Builds a protocol session revocation.
    ///
    /// # Errors
    ///
    /// Refuses the reserved zero grant identifier and zero effective sequence.
    pub fn new(
        grant_id: AuthorityGrantId,
        reason: SessionRevocationReason,
        effective_sequence: Sequence,
    ) -> Result<Self, IntentError> {
        nonzero_identifier(grant_id.bytes(), IntentField::AuthorityGrant)?;
        if effective_sequence.value() == 0 {
            return Err(IntentError::zero(IntentField::EffectiveSequence));
        }
        Ok(Self {
            grant_id,
            reason,
            effective_sequence,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DidRegistration {
    pub(crate) did: Did,
    pub(crate) primary_key: PublicKey,
}

impl DidRegistration {
    /// Builds the onboarding registration.
    ///
    /// # Errors
    ///
    /// Refuses the reserved all-zero key.
    pub fn new(did: Did, primary_key: PublicKey) -> Result<Self, IntentError> {
        nonzero_key(primary_key, IntentField::PrimaryKey)?;
        Ok(Self { did, primary_key })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyRotation {
    pub(crate) did: Did,
    pub(crate) pending_key: PublicKey,
    pub(crate) challenge_window: TimestampBound,
    pub(crate) effective_sequence: Sequence,
}

impl KeyRotation {
    /// Builds a key-rotation announcement over a validated time window.
    ///
    /// # Errors
    ///
    /// Refuses the reserved all-zero key.
    pub fn new(
        did: Did,
        pending_key: PublicKey,
        challenge_window: TimestampBound,
        effective_sequence: Sequence,
    ) -> Result<Self, IntentError> {
        nonzero_key(pending_key, IntentField::PendingKey)?;
        Ok(Self {
            did,
            pending_key,
            challenge_window,
            effective_sequence,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryRegistration {
    pub(crate) did: Did,
    pub(crate) recovery_root: RecoveryRoot,
    pub(crate) threshold: ApprovalThreshold,
}

impl RecoveryRegistration {
    /// Builds recovery registration from an exact commitment and non-zero threshold.
    ///
    /// # Errors
    ///
    /// Refuses the reserved all-zero root.
    pub fn new(
        did: Did,
        recovery_root: RecoveryRoot,
        threshold: ApprovalThreshold,
    ) -> Result<Self, IntentError> {
        if recovery_root.is_zero() {
            return Err(IntentError::zero(IntentField::RecoveryRoot));
        }
        Ok(Self {
            did,
            recovery_root,
            threshold,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvmPayoutBinding {
    pub(crate) did: Did,
    pub(crate) network: NetworkId,
    pub(crate) payout_address: EvmAddress,
    pub(crate) ownership_signature: Signature,
}

impl EvmPayoutBinding {
    /// Builds a payout binding backed by a non-empty ownership signature.
    ///
    /// # Errors
    ///
    /// Refuses an empty signature.
    pub fn new(
        did: Did,
        network: NetworkId,
        payout_address: EvmAddress,
        ownership_signature: Signature,
    ) -> Result<Self, IntentError> {
        if ownership_signature.as_bytes().is_empty() {
            return Err(IntentError::empty(IntentField::OwnershipSignature));
        }
        Ok(Self {
            did,
            network,
            payout_address,
            ownership_signature,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LxpSend {
    pub(crate) from: AccountId,
    pub(crate) to: AccountId,
    pub(crate) asset: AssetId,
    pub(crate) amount: Amount,
    pub(crate) account_sequence: Sequence,
    pub(crate) idempotency_key: IdempotencyKey,
    pub(crate) expires_at: TimestampSeconds,
    pub(crate) context_hash: ContextHash,
    pub(crate) authorization: SendAuthorization,
    pub(crate) network_id: NetworkId,
    pub(crate) protocol_version: ProtocolVersion,
}

impl LxpSend {
    /// Returns validated fields for canonical boundary encoding.
    #[must_use]
    pub fn to_wire_parts(
        &self,
    ) -> (
        &AccountId,
        &AccountId,
        AssetId,
        Amount,
        Sequence,
        IdempotencyKey,
        TimestampSeconds,
        ContextHash,
        SendAuthorization,
        NetworkId,
        ProtocolVersion,
    ) {
        (
            &self.from,
            &self.to,
            self.asset,
            self.amount,
            self.account_sequence,
            self.idempotency_key,
            self.expires_at,
            self.context_hash,
            self.authorization,
            self.network_id,
            self.protocol_version,
        )
    }
    /// Builds an asset send between distinct validated account namespaces.
    ///
    /// # Errors
    ///
    /// Refuses zero value and self-transfer.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        from: AccountId,
        to: AccountId,
        asset: AssetId,
        amount: Amount,
        account_sequence: Sequence,
        idempotency_key: IdempotencyKey,
        expires_at: TimestampSeconds,
        context_hash: ContextHash,
        authorization: SendAuthorization,
        network_id: NetworkId,
        protocol_version: ProtocolVersion,
    ) -> Result<Self, IntentError> {
        movement(&from, &to, amount)?;
        if expires_at.value() == 0 {
            return Err(IntentError::zero(IntentField::Expiration));
        }
        nonzero_key(authorization.public_key(), IntentField::AuthorizationKey)?;
        Ok(Self {
            from,
            to,
            asset,
            amount,
            account_sequence,
            idempotency_key,
            expires_at,
            context_hash,
            authorization,
            network_id,
            protocol_version,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LxpReceive {
    pub(crate) from: AccountId,
    pub(crate) to: AccountId,
    pub(crate) asset: AssetId,
    pub(crate) amount: Amount,
    pub(crate) payer_grant: PayerGrantId,
    pub(crate) receiver_sequence: Sequence,
    pub(crate) idempotency_key: IdempotencyKey,
    pub(crate) context_hash: ContextHash,
}

impl LxpReceive {
    /// Returns validated fields for canonical boundary encoding.
    #[must_use]
    pub fn to_wire_parts(
        &self,
    ) -> (
        &AccountId,
        &AccountId,
        AssetId,
        Amount,
        PayerGrantId,
        Sequence,
        IdempotencyKey,
        ContextHash,
    ) {
        (
            &self.from,
            &self.to,
            self.asset,
            self.amount,
            self.payer_grant,
            self.receiver_sequence,
            self.idempotency_key,
            self.context_hash,
        )
    }
    /// Builds one 402LXP receive under a named payer grant.
    ///
    /// # Errors
    ///
    /// Refuses zero value, self-transfer and the reserved zero grant.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        from: AccountId,
        to: AccountId,
        asset: AssetId,
        amount: Amount,
        payer_grant: PayerGrantId,
        receiver_sequence: Sequence,
        idempotency_key: IdempotencyKey,
        context_hash: ContextHash,
    ) -> Result<Self, IntentError> {
        movement(&from, &to, amount)?;
        if payer_grant.is_zero() {
            return Err(IntentError::zero(IntentField::PayerGrant));
        }
        Ok(Self {
            from,
            to,
            asset,
            amount,
            payer_grant,
            receiver_sequence,
            idempotency_key,
            context_hash,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PayerGrantRegistration {
    pub(crate) grant_id: PayerGrantId,
    pub(crate) from: AccountId,
    pub(crate) recipient: AccountId,
    pub(crate) asset: AssetId,
    pub(crate) per_draw_maximum: Amount,
    pub(crate) allowance: Amount,
    pub(crate) schedule: GrantSchedule,
    pub(crate) expiration: TimestampSeconds,
    pub(crate) purpose: PurposeHash,
    pub(crate) public_key: PublicKey,
}

impl PayerGrantRegistration {
    /// Builds the exact budget-module grant constraints.
    ///
    /// # Errors
    ///
    /// Refuses zero identifiers, keys, values, expiry, self-grants, and an
    /// allowance smaller than one permitted draw.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        grant_id: PayerGrantId,
        from: AccountId,
        recipient: AccountId,
        asset: AssetId,
        per_draw_maximum: Amount,
        allowance: Amount,
        schedule: GrantSchedule,
        expiration: TimestampSeconds,
        purpose: PurposeHash,
        public_key: PublicKey,
    ) -> Result<Self, IntentError> {
        movement(&from, &recipient, per_draw_maximum)?;
        if grant_id.is_zero() {
            return Err(IntentError::zero(IntentField::PayerGrant));
        }
        if allowance == Amount::ZERO {
            return Err(IntentError::zero(IntentField::Allowance));
        }
        if allowance < per_draw_maximum {
            return Err(IntentError::range(IntentField::Allowance));
        }
        if expiration.value() == 0 {
            return Err(IntentError::zero(IntentField::Expiration));
        }
        nonzero_key(public_key, IntentField::PrimaryKey)?;
        Ok(Self {
            grant_id,
            from,
            recipient,
            asset,
            per_draw_maximum,
            allowance,
            schedule,
            expiration,
            purpose,
            public_key,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetCreate {
    pub(crate) budget_id: BudgetId,
    pub(crate) owner: AccountId,
    pub(crate) budget_account: AccountId,
    pub(crate) asset: AssetId,
    pub(crate) per_period_limit: Amount,
    pub(crate) period_length: PeriodLength,
    pub(crate) rollover: RolloverPolicy,
    pub(crate) carry_cap: Amount,
    pub(crate) purpose: PurposeHash,
    pub(crate) expiry: TimestampSeconds,
}

impl BudgetCreate {
    /// Returns validated fields for canonical boundary encoding.
    #[must_use]
    pub fn to_wire_parts(
        &self,
    ) -> (
        BudgetId,
        &AccountId,
        &AccountId,
        AssetId,
        Amount,
        PeriodLength,
        RolloverPolicy,
        Amount,
        PurposeHash,
        TimestampSeconds,
    ) {
        (
            self.budget_id,
            &self.owner,
            &self.budget_account,
            self.asset,
            self.per_period_limit,
            self.period_length,
            self.rollover,
            self.carry_cap,
            self.purpose,
            self.expiry,
        )
    }
    /// Returns the exact protocol-enforced limit carried by this intent.
    #[must_use]
    pub const fn per_period_limit(&self) -> Amount {
        self.per_period_limit
    }

    /// Builds a protocol budget for an explicit budget namespace.
    ///
    /// # Errors
    ///
    /// Refuses zero identifiers, limits or expiry, a non-budget destination,
    /// and a carry cap above the period limit.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        budget_id: BudgetId,
        owner: AccountId,
        budget_account: AccountId,
        asset: AssetId,
        per_period_limit: Amount,
        period_length: PeriodLength,
        rollover: RolloverPolicy,
        carry_cap: Amount,
        purpose: PurposeHash,
        expiry: TimestampSeconds,
    ) -> Result<Self, IntentError> {
        budget_namespace(&budget_account)?;
        if budget_id.is_zero() {
            return Err(IntentError::zero(IntentField::Budget));
        }
        if per_period_limit == Amount::ZERO {
            return Err(IntentError::zero(IntentField::Amount));
        }
        if carry_cap > per_period_limit {
            return Err(IntentError::range(IntentField::CarryCap));
        }
        if expiry.value() == 0 {
            return Err(IntentError::zero(IntentField::Expiration));
        }
        if owner == budget_account {
            return Err(IntentError::same(IntentField::BudgetAccount));
        }
        Ok(Self {
            budget_id,
            owner,
            budget_account,
            asset,
            per_period_limit,
            period_length,
            rollover,
            carry_cap,
            purpose,
            expiry,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetFund {
    pub(crate) budget_id: BudgetId,
    pub(crate) owner: AccountId,
    pub(crate) budget_account: AccountId,
    pub(crate) asset: AssetId,
    pub(crate) amount: Amount,
    pub(crate) idempotency_key: IdempotencyKey,
}

impl BudgetFund {
    /// Returns validated fields for canonical boundary encoding.
    #[must_use]
    pub fn to_wire_parts(
        &self,
    ) -> (
        BudgetId,
        &AccountId,
        &AccountId,
        AssetId,
        Amount,
        IdempotencyKey,
    ) {
        (
            self.budget_id,
            &self.owner,
            &self.budget_account,
            self.asset,
            self.amount,
            self.idempotency_key,
        )
    }
    /// Builds one budget funding transfer.
    ///
    /// # Errors
    ///
    /// Refuses invalid budget namespace, zero identifier/value or self-transfer.
    pub fn new(
        budget_id: BudgetId,
        owner: AccountId,
        budget_account: AccountId,
        asset: AssetId,
        amount: Amount,
        idempotency_key: IdempotencyKey,
    ) -> Result<Self, IntentError> {
        budget_movement(budget_id, &owner, &budget_account, amount)?;
        Ok(Self {
            budget_id,
            owner,
            budget_account,
            asset,
            amount,
            idempotency_key,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BudgetDefund {
    pub(crate) budget_id: BudgetId,
    pub(crate) budget_account: AccountId,
    pub(crate) owner: AccountId,
    pub(crate) asset: AssetId,
    pub(crate) amount: Amount,
    pub(crate) revocation_sequence: Sequence,
    pub(crate) idempotency_key: IdempotencyKey,
}

impl BudgetDefund {
    /// Returns validated fields for canonical boundary encoding.
    #[must_use]
    pub fn to_wire_parts(
        &self,
    ) -> (
        BudgetId,
        &AccountId,
        &AccountId,
        AssetId,
        Amount,
        Sequence,
        IdempotencyKey,
    ) {
        (
            self.budget_id,
            &self.budget_account,
            &self.owner,
            self.asset,
            self.amount,
            self.revocation_sequence,
            self.idempotency_key,
        )
    }
    /// Builds one deterministic budget reclaim.
    ///
    /// # Errors
    ///
    /// Refuses invalid budget namespace, zero identifier/value or self-transfer.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        budget_id: BudgetId,
        budget_account: AccountId,
        owner: AccountId,
        asset: AssetId,
        amount: Amount,
        revocation_sequence: Sequence,
        idempotency_key: IdempotencyKey,
    ) -> Result<Self, IntentError> {
        budget_movement(budget_id, &owner, &budget_account, amount)?;
        Ok(Self {
            budget_id,
            budget_account,
            owner,
            asset,
            amount,
            revocation_sequence,
            idempotency_key,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BridgeDepositCredit {
    pub(crate) deposit_proof: DepositProofId,
    pub(crate) checkpoint: CheckpointId,
    pub(crate) reserve: AccountId,
    pub(crate) recipient: AccountId,
    pub(crate) asset: AssetId,
    pub(crate) amount: Amount,
    pub(crate) idempotency_key: IdempotencyKey,
}

impl BridgeDepositCredit {
    /// Returns validated fields for canonical boundary encoding.
    #[must_use]
    pub fn to_wire_parts(
        &self,
    ) -> (
        DepositProofId,
        CheckpointId,
        &AccountId,
        &AccountId,
        AssetId,
        Amount,
        IdempotencyKey,
    ) {
        (
            self.deposit_proof,
            self.checkpoint,
            &self.reserve,
            &self.recipient,
            self.asset,
            self.amount,
            self.idempotency_key,
        )
    }
    /// Builds a Paxeer deposit credit tied to one checkpoint and proof.
    ///
    /// # Errors
    ///
    /// Refuses zero proof/value and self-transfer.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        deposit_proof: DepositProofId,
        checkpoint: CheckpointId,
        reserve: AccountId,
        recipient: AccountId,
        asset: AssetId,
        amount: Amount,
        idempotency_key: IdempotencyKey,
    ) -> Result<Self, IntentError> {
        movement(&reserve, &recipient, amount)?;
        if deposit_proof.is_zero() {
            return Err(IntentError::zero(IntentField::DepositProof));
        }
        Ok(Self {
            deposit_proof,
            checkpoint,
            reserve,
            recipient,
            asset,
            amount,
            idempotency_key,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BridgeWithdrawRequest {
    pub(crate) withdrawal_id: WithdrawalId,
    pub(crate) owner: AccountId,
    pub(crate) withdrawals_account: AccountId,
    pub(crate) payout_address: EvmAddress,
    pub(crate) asset: AssetId,
    pub(crate) amount: Amount,
    pub(crate) idempotency_key: IdempotencyKey,
}

impl BridgeWithdrawRequest {
    /// Returns validated fields for canonical boundary encoding.
    #[must_use]
    pub fn to_wire_parts(
        &self,
    ) -> (
        WithdrawalId,
        &AccountId,
        &AccountId,
        EvmAddress,
        AssetId,
        Amount,
        IdempotencyKey,
    ) {
        (
            self.withdrawal_id,
            &self.owner,
            &self.withdrawals_account,
            self.payout_address,
            self.asset,
            self.amount,
            self.idempotency_key,
        )
    }
    /// Builds a Paxeer withdrawal request to an exact EVM payout address.
    ///
    /// # Errors
    ///
    /// Refuses zero withdrawal/value and self-transfer.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        withdrawal_id: WithdrawalId,
        owner: AccountId,
        withdrawals_account: AccountId,
        payout_address: EvmAddress,
        asset: AssetId,
        amount: Amount,
        idempotency_key: IdempotencyKey,
    ) -> Result<Self, IntentError> {
        movement(&owner, &withdrawals_account, amount)?;
        if withdrawal_id.is_zero() {
            return Err(IntentError::zero(IntentField::Withdrawal));
        }
        Ok(Self {
            withdrawal_id,
            owner,
            withdrawals_account,
            payout_address,
            asset,
            amount,
            idempotency_key,
        })
    }
}

/// Field named by a failed intent construction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntentField {
    Amount,
    Allowance,
    AuthorizationKey,
    Budget,
    BudgetAccount,
    CarryCap,
    DepositProof,
    Destination,
    Expiration,
    OwnershipSignature,
    PayerGrant,
    PendingKey,
    PrimaryKey,
    RecoveryRoot,
    SessionGrant,
    AuthorityGrant,
    EffectiveSequence,
    Withdrawal,
}

/// Typed reason an intent could not be constructed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntentErrorReason {
    Empty,
    Zero,
    InvalidRange,
    InvalidCanonicalEncoding,
    SameSourceAndDestination,
    WrongAccountNamespace,
}

/// Construction failure naming both the field and invariant.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntentError {
    pub field: IntentField,
    pub reason: IntentErrorReason,
}

impl IntentError {
    const fn empty(field: IntentField) -> Self {
        Self {
            field,
            reason: IntentErrorReason::Empty,
        }
    }

    const fn zero(field: IntentField) -> Self {
        Self {
            field,
            reason: IntentErrorReason::Zero,
        }
    }

    const fn range(field: IntentField) -> Self {
        Self {
            field,
            reason: IntentErrorReason::InvalidRange,
        }
    }

    const fn same(field: IntentField) -> Self {
        Self {
            field,
            reason: IntentErrorReason::SameSourceAndDestination,
        }
    }

    const fn namespace(field: IntentField) -> Self {
        Self {
            field,
            reason: IntentErrorReason::WrongAccountNamespace,
        }
    }
}

fn nonzero_key(key: PublicKey, field: IntentField) -> Result<(), IntentError> {
    if key.is_zero() {
        Err(IntentError::zero(field))
    } else {
        Ok(())
    }
}

fn nonzero_identifier(bytes: [u8; 32], field: IntentField) -> Result<(), IntentError> {
    if bytes.iter().all(|byte| *byte == 0) {
        Err(IntentError::zero(field))
    } else {
        Ok(())
    }
}

fn validate_session_grant(bytes: &[u8]) -> Result<(), IntentError> {
    let invalid = || IntentError {
        field: IntentField::SessionGrant,
        reason: IntentErrorReason::InvalidCanonicalEncoding,
    };
    if bytes.len() > MAX_SESSION_GRANT_BYTES {
        return Err(invalid());
    }
    let mut decoder = Decoder::new(bytes, 0);
    decoder.structure_header(0x2001).map_err(|_| invalid())?;
    if decoder.u8().map_err(|_| invalid())? != 1 {
        return Err(invalid());
    }
    let grantor = decoder.bytes(32).map_err(|_| invalid())?;
    let grantee = decoder.bytes(32).map_err(|_| invalid())?;
    if grantor == [0; 32]
        || grantee != grantor
        || decoder.u8().map_err(|_| invalid())? != 2
        || decoder.bytes(32).map_err(|_| invalid())? == [0; 32]
    {
        return Err(invalid());
    }
    let module_mask = decoder.u64().map_err(|_| invalid())?;
    let ordinal_min = decoder.u16().map_err(|_| invalid())?;
    let ordinal_max = decoder.u16().map_err(|_| invalid())?;
    if module_mask == 0
        || module_mask & !0x03fe != 0
        || ordinal_min == 0
        || ordinal_min > ordinal_max
    {
        return Err(invalid());
    }
    if decoder.bytes(32).map_err(|_| invalid())? != [0; 32]
        || decoder.u128().map_err(|_| invalid())? != 0
        || decoder.u128().map_err(|_| invalid())? != 0
        || decoder.u128().map_err(|_| invalid())? != 0
        || decoder.u64().map_err(|_| invalid())? != 0
        || decoder.u128().map_err(|_| invalid())? != 0
        || decoder.u128().map_err(|_| invalid())? != 0
        || decoder.u64().map_err(|_| invalid())? != 0
        || decoder.bytes(32).map_err(|_| invalid())? != [0; 32]
    {
        return Err(invalid());
    }
    let not_before = decoder.u64().map_err(|_| invalid())?;
    let not_after = decoder.u64().map_err(|_| invalid())?;
    if not_after == 0
        || not_after <= not_before
        || decoder.u64().map_err(|_| invalid())? == 0
        || decoder.u8().map_err(|_| invalid())? != 0
        || decoder.u64().map_err(|_| invalid())? != 0
        || decoder.bytes(64).map_err(|_| invalid())? != [0; 64]
    {
        return Err(invalid());
    }
    decoder.finish().map_err(|_| invalid())
}

fn movement(from: &AccountId, to: &AccountId, amount: Amount) -> Result<(), IntentError> {
    if amount == Amount::ZERO {
        return Err(IntentError::zero(IntentField::Amount));
    }
    if from == to {
        return Err(IntentError::same(IntentField::Destination));
    }
    Ok(())
}

fn budget_namespace(account: &AccountId) -> Result<(), IntentError> {
    if account.namespace() == AccountNamespace::AgentBudget {
        Ok(())
    } else {
        Err(IntentError::namespace(IntentField::BudgetAccount))
    }
}

fn budget_movement(
    budget_id: BudgetId,
    owner: &AccountId,
    budget_account: &AccountId,
    amount: Amount,
) -> Result<(), IntentError> {
    if budget_id.is_zero() {
        return Err(IntentError::zero(IntentField::Budget));
    }
    budget_namespace(budget_account)?;
    movement(owner, budget_account, amount)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn did() -> Did {
        Did::new(b"did:layerx:alice").unwrap_or_else(|error| panic!("did: {error:?}"))
    }

    fn account(value: &str) -> AccountId {
        AccountId::parse(value).unwrap_or_else(|error| panic!("account: {error:?}"))
    }

    #[test]
    fn vocabulary_is_versioned_complete_and_module_mapped() {
        let registration = DidRegistration::new(did(), PublicKey::new([1; 32]))
            .unwrap_or_else(|error| panic!("registration: {error:?}"));
        let intent = Intent::v1(IntentKind::DidRegistration(registration));
        assert_eq!(intent.version(), IntentVersion::CURRENT);
        assert_eq!(intent.version().value(), 1);
        assert_eq!(intent.module(), ModuleId::Governance);

        let send = LxpSend::new(
            account("agent:did:layerx:alice:main"),
            account("agent:did:layerx:bob:main"),
            AssetId::new([2; 32]),
            Amount::from_u128(1),
            Sequence::from_u64(7),
            IdempotencyKey::new([3; 32]),
            TimestampSeconds::from_u64(1_000),
            ContextHash::new([4; 32]),
            SendAuthorization::new(
                SendAuthorizationKind::Owner,
                PublicKey::new([5; 32]),
                AuthorizationSignature::new([6; 64]),
            ),
            NetworkId::new(77).unwrap_or_else(|error| panic!("network: {error:?}")),
            ProtocolVersion::new(1).unwrap_or_else(|error| panic!("version: {error:?}")),
        )
        .unwrap_or_else(|error| panic!("send: {error:?}"));
        assert_eq!(
            Intent::v1(IntentKind::LxpSend(send)).module(),
            ModuleId::Asset
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn all_twelve_v1_kinds_are_constructible_only_from_domain_types() {
        let owner = || account("agent:did:layerx:alice:main");
        let recipient = || account("agent:did:layerx:bob:main");
        let budget = || account("agent:did:layerx:alice:budget:primary");
        let reserve = || account("system:paxeer-reserve");
        let withdrawals = || account("system:paxeer-withdrawals");
        let asset = || AssetId::new([2; 32]);
        let key = || IdempotencyKey::new([3; 32]);
        let amount = || Amount::from_u128(25);
        let intents = vec![
            IntentKind::DidRegistration(
                DidRegistration::new(did(), PublicKey::new([1; 32]))
                    .unwrap_or_else(|error| panic!("register: {error:?}")),
            ),
            IntentKind::KeyRotation(
                KeyRotation::new(
                    did(),
                    PublicKey::new([2; 32]),
                    TimestampBound::new(10, 20).unwrap_or_else(|error| panic!("window: {error:?}")),
                    Sequence::from_u64(7),
                )
                .unwrap_or_else(|error| panic!("rotation: {error:?}")),
            ),
            IntentKind::RecoveryRegistration(
                RecoveryRegistration::new(
                    did(),
                    RecoveryRoot::new([3; 32]),
                    ApprovalThreshold::new(2)
                        .unwrap_or_else(|error| panic!("threshold: {error:?}")),
                )
                .unwrap_or_else(|error| panic!("recovery: {error:?}")),
            ),
            IntentKind::EvmPayoutBinding(
                EvmPayoutBinding::new(
                    did(),
                    NetworkId::new(77).unwrap_or_else(|error| panic!("network: {error:?}")),
                    EvmAddress::new([4; 20]),
                    Signature::new(&[5; 65]).unwrap_or_else(|error| panic!("signature: {error:?}")),
                )
                .unwrap_or_else(|error| panic!("binding: {error:?}")),
            ),
            IntentKind::LxpSend(
                LxpSend::new(
                    owner(),
                    recipient(),
                    asset(),
                    amount(),
                    Sequence::from_u64(7),
                    key(),
                    TimestampSeconds::from_u64(1_000),
                    ContextHash::new([6; 32]),
                    SendAuthorization::new(
                        SendAuthorizationKind::Owner,
                        PublicKey::new([7; 32]),
                        AuthorizationSignature::new([8; 64]),
                    ),
                    NetworkId::new(77).unwrap_or_else(|error| panic!("network: {error:?}")),
                    ProtocolVersion::new(1).unwrap_or_else(|error| panic!("version: {error:?}")),
                )
                .unwrap_or_else(|error| panic!("send: {error:?}")),
            ),
            IntentKind::LxpReceive(
                LxpReceive::new(
                    owner(),
                    recipient(),
                    asset(),
                    amount(),
                    PayerGrantId::new([7; 32]),
                    Sequence::from_u64(8),
                    key(),
                    ContextHash::new([8; 32]),
                )
                .unwrap_or_else(|error| panic!("receive: {error:?}")),
            ),
            IntentKind::PayerGrantRegistration(
                PayerGrantRegistration::new(
                    PayerGrantId::new([9; 32]),
                    owner(),
                    recipient(),
                    asset(),
                    amount(),
                    Amount::from_u128(100),
                    GrantSchedule::Recurring(
                        PeriodLength::new(60).unwrap_or_else(|error| panic!("period: {error:?}")),
                    ),
                    TimestampSeconds::from_u64(1_000),
                    PurposeHash::new([10; 32]),
                    PublicKey::new([11; 32]),
                )
                .unwrap_or_else(|error| panic!("grant: {error:?}")),
            ),
            IntentKind::BudgetCreate(
                BudgetCreate::new(
                    BudgetId::new([12; 32]),
                    owner(),
                    budget(),
                    asset(),
                    Amount::from_u128(100),
                    PeriodLength::new(86_400).unwrap_or_else(|error| panic!("period: {error:?}")),
                    RolloverPolicy::Capped,
                    Amount::from_u128(50),
                    PurposeHash::new([13; 32]),
                    TimestampSeconds::from_u64(2_000),
                )
                .unwrap_or_else(|error| panic!("budget create: {error:?}")),
            ),
            IntentKind::BudgetFund(
                BudgetFund::new(
                    BudgetId::new([12; 32]),
                    owner(),
                    budget(),
                    asset(),
                    amount(),
                    key(),
                )
                .unwrap_or_else(|error| panic!("budget fund: {error:?}")),
            ),
            IntentKind::BudgetDefund(
                BudgetDefund::new(
                    BudgetId::new([12; 32]),
                    budget(),
                    owner(),
                    asset(),
                    amount(),
                    Sequence::from_u64(9),
                    key(),
                )
                .unwrap_or_else(|error| panic!("budget defund: {error:?}")),
            ),
            IntentKind::BridgeDepositCredit(
                BridgeDepositCredit::new(
                    DepositProofId::new([14; 32]),
                    CheckpointId::new([15; 32]),
                    reserve(),
                    owner(),
                    asset(),
                    amount(),
                    key(),
                )
                .unwrap_or_else(|error| panic!("deposit: {error:?}")),
            ),
            IntentKind::BridgeWithdrawRequest(
                BridgeWithdrawRequest::new(
                    WithdrawalId::new([16; 32]),
                    owner(),
                    withdrawals(),
                    EvmAddress::new([17; 20]),
                    asset(),
                    amount(),
                    key(),
                )
                .unwrap_or_else(|error| panic!("withdrawal: {error:?}")),
            ),
        ];
        assert_eq!(intents.len(), 12);
        assert_eq!(
            intents.iter().map(IntentKind::module).collect::<Vec<_>>(),
            [
                ModuleId::Governance,
                ModuleId::Governance,
                ModuleId::Governance,
                ModuleId::Governance,
                ModuleId::Asset,
                ModuleId::Asset,
                ModuleId::Budget,
                ModuleId::Budget,
                ModuleId::Budget,
                ModuleId::Budget,
                ModuleId::Bridge,
                ModuleId::Bridge,
            ]
        );
    }

    #[test]
    fn invalid_values_are_rejected_before_an_intent_exists() {
        assert_eq!(
            DidRegistration::new(did(), PublicKey::new([0; 32])),
            Err(IntentError::zero(IntentField::PrimaryKey))
        );
        let owner = account("agent:did:layerx:alice:main");
        assert_eq!(
            LxpSend::new(
                owner.clone(),
                owner,
                AssetId::new([2; 32]),
                Amount::from_u128(1),
                Sequence::from_u64(1),
                IdempotencyKey::new([3; 32]),
                TimestampSeconds::from_u64(100),
                ContextHash::new([4; 32]),
                SendAuthorization::new(
                    SendAuthorizationKind::Owner,
                    PublicKey::new([5; 32]),
                    AuthorizationSignature::new([6; 64]),
                ),
                NetworkId::new(77).unwrap_or_else(|error| panic!("network: {error:?}")),
                ProtocolVersion::new(1).unwrap_or_else(|error| panic!("version: {error:?}")),
            ),
            Err(IntentError::same(IntentField::Destination))
        );
        assert_eq!(
            BudgetFund::new(
                BudgetId::new([1; 32]),
                account("agent:did:layerx:alice:main"),
                account("agent:did:layerx:bob:main"),
                AssetId::new([2; 32]),
                Amount::from_u128(1),
                IdempotencyKey::new([3; 32]),
            ),
            Err(IntentError::namespace(IntentField::BudgetAccount))
        );
    }
}
