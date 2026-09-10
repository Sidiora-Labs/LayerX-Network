//! Structured, byte-bound descriptions of canonical activities.

use std::fmt;

use sha2::{Digest as _, Sha256};

use layerx_types::payload::{ActivityType, ModuleId, ModuleRegistry};
use layerx_wire::activity::{decode_unsigned, encode_unsigned, Activity, TimestampBound};
use layerx_wire::decode::Decoder;
use layerx_wire::encode::Encoder;
use layerx_wire::hash;
use layerx_wire::WireError;

use crate::{
    ct,
    payments::{Grant, Payment},
    SignatureMessage,
};

const ASSET_SEND_ORDINAL: u16 = 5;
const SEND_WIRE_TAG: u16 = 0x5301;
const SEND_FIELD_COUNT: u16 = 10;
const BUDGET_CREATE_ORDINAL: u16 = 1;
const BUDGET_CREATE_WIRE_TAG: u16 = 0x4201;
const BUDGET_CREATE_FIELD_COUNT: u16 = 10;
const BUDGET_FUND_ORDINAL: u16 = 2;
const BUDGET_FUND_WIRE_TAG: u16 = 0x4202;
const BUDGET_FUND_FIELD_COUNT: u16 = 6;
const ASSET_RECEIVE_ORDINAL: u16 = 6;
const BUDGET_DEFUND_ORDINAL: u16 = 7;
const BUDGET_DEFUND_WIRE_TAG: u16 = 0x4207;
const BUDGET_DEFUND_FIELD_COUNT: u16 = 7;
const BRIDGE_DEPOSIT_CREDIT_ORDINAL: u16 = 1;
const BRIDGE_DEPOSIT_CREDIT_WIRE_TAG: u16 = 0x4801;
const BRIDGE_DEPOSIT_CREDIT_FIELD_COUNT: u16 = 7;
const ASSET_WITHDRAW_ORDINAL: u16 = 9;
const GOVERNANCE_EVM_BINDING_ORDINAL: u16 = 4;
const GOVERNANCE_EVM_BINDING_WIRE_TAG: u16 = 0x7104;
const GOVERNANCE_EVM_BINDING_FIELD_COUNT: u16 = 4;
const MAX_SEND_CONDITIONS: usize = 8;
const MAX_SEND_PAYLOAD_BYTES: usize = 512;
const MAX_TRANSPORT_DISCLOSURE_BYTES: usize = 1_048_576;

/// The semantic role of one counterparty named in a disclosure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CounterpartyRole {
    /// Account whose balance is debited.
    Payer,
    /// Account whose balance is credited.
    Recipient,
}

/// One complete counterparty entry decoded from a module payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Counterparty {
    /// Role this account has in the activity.
    pub role: CounterpartyRole,
    /// Canonical protocol account identifier.
    pub account: [u8; 32],
}

/// The semantic role of one amount named in a disclosure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AmountRole {
    /// Principal transferred from payer to recipient.
    Transfer,
    /// Maximum spend permitted in one configured budget period.
    SpendingLimit,
    /// Maximum units that can ever be issued for a registered asset.
    SupplyCap,
    /// Maximum units one payer-grant draw may transfer.
    PerDrawMaximum,
    /// Total units authorized by one payer grant.
    GrantAllowance,
}

/// One complete amount entry decoded from a module payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisclosedAmount {
    /// Role this amount has in the activity.
    pub role: AmountRole,
    /// Unsigned protocol amount.
    pub value: u128,
}

/// Complete governance wallet-binding semantics decoded from canonical bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisclosedEvmPayoutBinding {
    /// Core DID identifier whose payout address changes.
    pub did_id: [u8; 32],
    /// Protocol network named by the wallet ownership proof.
    pub network_id: u32,
    /// Exact EVM address recovered from the ownership signature.
    pub payout_address: [u8; 20],
    /// Commitment to the full public ownership signature carried by the intent.
    pub ownership_signature_digest: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DisclosedWithdrawal {
    pub account_sequence: u64,
    pub evm_recipient: [u8; 20],
    pub request_anchor: [u8; 32],
}

/// Every time bound that can make the disclosed activity expire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Expiry {
    /// Inclusive envelope lower bound.
    pub not_before: u64,
    /// Inclusive envelope upper bound.
    pub not_after: u64,
    /// Module-payload expiry enforced by the asset transfer.
    pub payload_expires_at: u64,
}

/// A complete human-reviewable description derived from canonical bytes.
///
/// Semantic fields are public so a remote review surface can serialize and
/// present them. Any change is detected before a signer receives a request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Disclosure {
    /// Exact registered activity type.
    pub activity_type: ActivityType,
    /// Actor DID from the canonical envelope.
    pub actor: Vec<u8>,
    /// Exact protocol authority representation.
    pub authority: Vec<u8>,
    /// Every account whose balance participates in the operation.
    pub counterparties: Vec<Counterparty>,
    /// Every value transferred by the operation.
    pub amounts: Vec<DisclosedAmount>,
    /// Asset identifier the amounts are denominated in.
    pub asset: [u8; 32],
    /// Maximum fee authorised by the envelope.
    pub fee_limit: u128,
    /// Envelope and module expiry bounds.
    pub expiry: Expiry,
    /// Exact retry identity shared by envelope and payload.
    pub idempotency_key: [u8; 32],
    /// Governance wallet-binding semantics, present only for that activity type.
    pub evm_payout_binding: Option<DisclosedEvmPayoutBinding>,
    pub withdrawal: Option<DisclosedWithdrawal>,
    /// Decoded payment or Programs payload, present for those activity types.
    pub payment: Option<Payment>,
    activity: Activity,
    signing_digest: [u8; 32],
}

/// Typed refusal produced while deriving or validating a disclosure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisclosureError {
    /// The canonical activity envelope was rejected by the wire crate.
    Wire(WireError),
    /// No complete semantic decoder exists for the named activity.
    UnsupportedActivity(u32),
    /// The module payload was malformed or semantically inconsistent.
    MalformedPayload,
    /// The envelope's payload commitment did not cover its payload bytes.
    PayloadHash,
    /// A named disclosure field differed from the canonical bytes.
    FieldMismatch(&'static str),
}

impl fmt::Display for DisclosureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Wire(error) => write!(
                formatter,
                "canonical activity rejected at byte {} with result {}",
                error.offset,
                error.result.raw()
            ),
            Self::UnsupportedActivity(activity_type) => {
                write!(
                    formatter,
                    "activity type {activity_type:#010x} has no complete disclosure"
                )
            }
            Self::MalformedPayload => {
                formatter.write_str("activity payload disclosure is malformed")
            }
            Self::PayloadHash => formatter.write_str("payload_hash does not match payload bytes"),
            Self::FieldMismatch(field) => {
                write!(
                    formatter,
                    "disclosure field {field} does not match canonical bytes"
                )
            }
        }
    }
}

impl std::error::Error for DisclosureError {}

impl From<WireError> for DisclosureError {
    fn from(error: WireError) -> Self {
        Self::Wire(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct SendSemantics {
    from: [u8; 32],
    to: [u8; 32],
    asset: [u8; 32],
    amount: u128,
    sequence: u64,
    idempotency_key: [u8; 32],
    expires_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct BudgetCreateSemantics {
    owner: [u8; 32],
    budget: [u8; 32],
    asset: [u8; 32],
    per_period_limit: u128,
    expires_at: u64,
}

fn fixed<const N: usize>(decoder: &mut Decoder<'_>) -> Result<[u8; N], DisclosureError> {
    decoder
        .fixed(N)
        .map_err(DisclosureError::from)?
        .try_into()
        .map_err(|_| DisclosureError::MalformedPayload)
}

fn decode_send(payload: &[u8], activity: &Activity) -> Result<SendSemantics, DisclosureError> {
    let mut decoder = Decoder::new(payload, 0);
    if decoder.u16()? != SEND_WIRE_TAG || decoder.u16()? != SEND_FIELD_COUNT {
        return Err(DisclosureError::MalformedPayload);
    }
    let from = fixed(&mut decoder)?;
    let to = fixed(&mut decoder)?;
    let asset = fixed(&mut decoder)?;
    let amount = decoder.u128()?;
    let sequence = decoder.u64()?;
    let idempotency_key = fixed(&mut decoder)?;
    let expires_at = decoder.u64()?;
    let context_hash: [u8; 32] = fixed(&mut decoder)?;
    let condition_count = usize::from(decoder.u8()?);
    if condition_count > MAX_SEND_CONDITIONS {
        return Err(DisclosureError::MalformedPayload);
    }
    for _ in 0..condition_count {
        if !matches!(decoder.u8()?, 1 | 2) {
            return Err(DisclosureError::MalformedPayload);
        }
        let _ = decoder.u64()?;
    }
    if !(1..=6).contains(&decoder.u8()?) {
        return Err(DisclosureError::MalformedPayload);
    }
    let controller: [u8; 32] = fixed(&mut decoder)?;
    let public_key: [u8; 32] = fixed(&mut decoder)?;
    let signature: [u8; 64] = fixed(&mut decoder)?;
    let signed_context_hash: [u8; 32] = fixed(&mut decoder)?;
    let network_id = decoder.u32()?;
    let protocol_version = decoder.u16()?;
    decoder.finish()?;

    if controller != from
        || signed_context_hash != context_hash
        || network_id != activity.network_id()
        || protocol_version != activity.protocol_version()
        || idempotency_key != activity.idempotency_key()
    {
        return Err(DisclosureError::MalformedPayload);
    }
    let authorization_offset = payload
        .len()
        .checked_sub(167)
        .ok_or(DisclosureError::MalformedPayload)?;
    let mut h = Sha256::new();
    h.update(hash::Domain::SignaturePreimage.tag());
    h.update(&payload[..2]);
    h.update(&payload[4..authorization_offset + 33]);
    h.update(&payload[authorization_offset + 129..]);
    crate::ed25519::verify_digest(&public_key, &signature, &h.finalize().into())
        .map_err(|_| DisclosureError::MalformedPayload)?;
    Ok(SendSemantics {
        from,
        to,
        asset,
        amount,
        sequence,
        idempotency_key,
        expires_at,
    })
}

fn decode_receive(payload: &[u8], activity: &Activity) -> Result<SendSemantics, DisclosureError> {
    let payment = Payment::decode(ModuleId::Asset, 6, payload, activity.actor_did())?;
    let Payment::Receive {
        from,
        to,
        asset,
        amount,
        sequence,
        idempotency_key,
        receiver_authorization: auth,
        ..
    } = payment
    else {
        return Err(DisclosureError::MalformedPayload);
    };
    if idempotency_key != activity.idempotency_key()
        || auth.network_id != activity.network_id()
        || auth.protocol_version != activity.protocol_version()
    {
        return Err(DisclosureError::MalformedPayload);
    }
    Ok(SendSemantics {
        from,
        to,
        asset,
        amount,
        sequence,
        idempotency_key,
        expires_at: activity.timestamp_bound().not_after,
    })
}

fn decode_budget_defund(
    payload: &[u8],
    activity: &Activity,
) -> Result<SendSemantics, DisclosureError> {
    let mut decoder = Decoder::new(payload, 0);
    if decoder.u16()? != BUDGET_DEFUND_WIRE_TAG || decoder.u16()? != BUDGET_DEFUND_FIELD_COUNT {
        return Err(DisclosureError::MalformedPayload);
    }
    let _budget_id: [u8; 32] = fixed(&mut decoder)?;
    let from = fixed(&mut decoder)?;
    let to = fixed(&mut decoder)?;
    let asset = fixed(&mut decoder)?;
    let amount = decoder.u128()?;
    let sequence = decoder.u64()?;
    let idempotency_key = fixed(&mut decoder)?;
    decoder.finish()?;
    if amount == 0 || from == to || idempotency_key != activity.idempotency_key() {
        return Err(DisclosureError::MalformedPayload);
    }
    Ok(SendSemantics {
        from,
        to,
        asset,
        amount,
        sequence,
        idempotency_key,
        expires_at: activity.timestamp_bound().not_after,
    })
}

fn decode_budget_create(payload: &[u8]) -> Result<BudgetCreateSemantics, DisclosureError> {
    let mut decoder = Decoder::new(payload, 0);
    if decoder.u16()? != BUDGET_CREATE_WIRE_TAG || decoder.u16()? != BUDGET_CREATE_FIELD_COUNT {
        return Err(DisclosureError::MalformedPayload);
    }
    let budget_id: [u8; 32] = fixed(&mut decoder)?;
    let owner = fixed(&mut decoder)?;
    let budget = fixed(&mut decoder)?;
    let asset = fixed(&mut decoder)?;
    let per_period_limit = decoder.u128()?;
    let period_length = decoder.u64()?;
    let rollover = decoder.u8()?;
    let carry_cap = decoder.u128()?;
    let _purpose: [u8; 32] = fixed(&mut decoder)?;
    let expires_at = decoder.u64()?;
    decoder.finish()?;
    if budget_id == [0; 32]
        || owner == budget
        || per_period_limit == 0
        || period_length == 0
        || !matches!(rollover, 1 | 2)
        || carry_cap > per_period_limit
        || expires_at == 0
    {
        return Err(DisclosureError::MalformedPayload);
    }
    Ok(BudgetCreateSemantics {
        owner,
        budget,
        asset,
        per_period_limit,
        expires_at,
    })
}

fn decode_budget_fund(
    payload: &[u8],
    activity: &Activity,
) -> Result<SendSemantics, DisclosureError> {
    let mut decoder = Decoder::new(payload, 0);
    if decoder.u16()? != BUDGET_FUND_WIRE_TAG || decoder.u16()? != BUDGET_FUND_FIELD_COUNT {
        return Err(DisclosureError::MalformedPayload);
    }
    let budget_id: [u8; 32] = fixed(&mut decoder)?;
    let from = fixed(&mut decoder)?;
    let to = fixed(&mut decoder)?;
    let asset = fixed(&mut decoder)?;
    let amount = decoder.u128()?;
    let idempotency_key = fixed(&mut decoder)?;
    decoder.finish()?;
    if budget_id == [0; 32]
        || from == to
        || amount == 0
        || idempotency_key != activity.idempotency_key()
    {
        return Err(DisclosureError::MalformedPayload);
    }
    Ok(SendSemantics {
        from,
        to,
        asset,
        amount,
        sequence: activity.account_sequence(),
        idempotency_key,
        expires_at: activity.timestamp_bound().not_after,
    })
}

fn decode_bridge_deposit_credit(
    payload: &[u8],
    activity: &Activity,
) -> Result<SendSemantics, DisclosureError> {
    let mut decoder = Decoder::new(payload, 0);
    if decoder.u16()? != BRIDGE_DEPOSIT_CREDIT_WIRE_TAG
        || decoder.u16()? != BRIDGE_DEPOSIT_CREDIT_FIELD_COUNT
    {
        return Err(DisclosureError::MalformedPayload);
    }
    let _deposit_proof: [u8; 32] = fixed(&mut decoder)?;
    let _checkpoint: [u8; 32] = fixed(&mut decoder)?;
    let from = fixed(&mut decoder)?;
    let to = fixed(&mut decoder)?;
    let asset = fixed(&mut decoder)?;
    let amount = decoder.u128()?;
    let idempotency_key = fixed(&mut decoder)?;
    decoder.finish()?;
    if idempotency_key != activity.idempotency_key() || amount == 0 || from == to {
        return Err(DisclosureError::MalformedPayload);
    }
    Ok(SendSemantics {
        from,
        to,
        asset,
        amount,
        sequence: activity.account_sequence(),
        idempotency_key,
        expires_at: activity.timestamp_bound().not_after,
    })
}

fn decode_withdraw(activity: &Activity) -> Result<DisclosureFields, DisclosureError> {
    let mut decoder = Decoder::new(activity.payload(), 0);
    let asset = fixed(&mut decoder)?;
    let amount = decoder.u128()?;
    let evm_recipient = fixed(&mut decoder)?;
    let request_anchor = fixed(&mut decoder)?;
    let fee_limit = decoder.u64()?;
    decoder.finish()?;
    if amount == 0
        || evm_recipient == [0; 20]
        || request_anchor == [0; 32]
        || u128::from(fee_limit) != activity.fee_limit()
    {
        return Err(DisclosureError::MalformedPayload);
    }
    let actor =
        std::str::from_utf8(activity.actor_did()).map_err(|_| DisclosureError::MalformedPayload)?;
    let account = layerx_types::account::AccountId::parse(&format!("agent:{actor}:main"))
        .map_err(|_| DisclosureError::MalformedPayload)?;
    let payer = hash::account_id_for_protocol(&account, activity.protocol_version())?;
    let bounds = activity.timestamp_bound();
    Ok(DisclosureFields {
        activity_type: activity.activity_type(),
        actor: activity.actor_did().to_vec(),
        authority: activity.authority().to_vec(),
        counterparties: vec![Counterparty {
            role: CounterpartyRole::Payer,
            account: payer,
        }],
        amounts: vec![DisclosedAmount {
            role: AmountRole::Transfer,
            value: amount,
        }],
        asset,
        fee_limit: u128::from(fee_limit),
        expiry: Expiry {
            not_before: bounds.not_before,
            not_after: bounds.not_after,
            payload_expires_at: bounds.not_after,
        },
        idempotency_key: activity.idempotency_key(),
        evm_payout_binding: None,
        withdrawal: Some(DisclosedWithdrawal {
            account_sequence: activity.account_sequence(),
            evm_recipient,
            request_anchor,
        }),
        payment: None,
    })
}

fn decode_evm_payout_binding(
    payload: &[u8],
    activity: &Activity,
) -> Result<DisclosedEvmPayoutBinding, DisclosureError> {
    let mut decoder = Decoder::new(payload, 0);
    if decoder.u16()? != GOVERNANCE_EVM_BINDING_WIRE_TAG
        || decoder.u16()? != GOVERNANCE_EVM_BINDING_FIELD_COUNT
    {
        return Err(DisclosureError::MalformedPayload);
    }
    let did_id = fixed(&mut decoder)?;
    let network_id = decoder.u32()?;
    let payout_address = fixed(&mut decoder)?;
    let ownership_signature = decoder.bytes(128)?;
    decoder.finish()?;
    if network_id != activity.network_id() || ownership_signature.len() != 65 {
        return Err(DisclosureError::MalformedPayload);
    }
    let mut hasher = Sha256::new();
    hasher.update(b"LXP/agent/evm-ownership-signature/v1\0");
    hasher.update(ownership_signature);
    Ok(DisclosedEvmPayoutBinding {
        did_id,
        network_id,
        payout_address,
        ownership_signature_digest: hasher.finalize().into(),
    })
}

fn semantics(activity: &Activity) -> Result<SendSemantics, DisclosureError> {
    let activity_type = activity.activity_type();
    if activity.payload().len() > MAX_SEND_PAYLOAD_BYTES {
        return Err(DisclosureError::MalformedPayload);
    }
    match (activity_type.module(), activity_type.ordinal()) {
        (ModuleId::Asset, ASSET_SEND_ORDINAL) => decode_send(activity.payload(), activity),
        (ModuleId::Asset, ASSET_RECEIVE_ORDINAL) => decode_receive(activity.payload(), activity),
        (ModuleId::Budget, BUDGET_DEFUND_ORDINAL) => {
            decode_budget_defund(activity.payload(), activity)
        }
        (ModuleId::Budget, BUDGET_FUND_ORDINAL) => decode_budget_fund(activity.payload(), activity),
        (ModuleId::Bridge, BRIDGE_DEPOSIT_CREDIT_ORDINAL) => {
            decode_bridge_deposit_credit(activity.payload(), activity)
        }
        _ => Err(DisclosureError::UnsupportedActivity(activity_type.value())),
    }
}

fn disclose_party(
    counterparties: &mut Vec<Counterparty>,
    role: CounterpartyRole,
    account: [u8; 32],
) {
    counterparties.push(Counterparty { role, account });
}

fn disclose_amount(amounts: &mut Vec<DisclosedAmount>, role: AmountRole, value: u128) {
    amounts.push(DisclosedAmount { role, value });
}

fn disclose_grant(
    grant: &Grant,
    counterparties: &mut Vec<Counterparty>,
    amounts: &mut Vec<DisclosedAmount>,
) {
    disclose_party(counterparties, CounterpartyRole::Payer, grant.from);
    disclose_party(counterparties, CounterpartyRole::Recipient, grant.recipient);
    disclose_grant_amounts(grant, amounts);
}

fn disclose_grant_amounts(grant: &Grant, amounts: &mut Vec<DisclosedAmount>) {
    disclose_amount(amounts, AmountRole::PerDrawMaximum, grant.per_draw_maximum);
    disclose_amount(amounts, AmountRole::GrantAllowance, grant.allowance);
}

fn payment_monetary_fields(
    payment: &Payment,
) -> (Vec<Counterparty>, Vec<DisclosedAmount>, [u8; 32]) {
    let mut counterparties = Vec::new();
    let mut amounts = Vec::new();
    let asset = match payment {
        Payment::Register(registration) => {
            disclose_amount(&mut amounts, AmountRole::SupplyCap, registration.supply_cap);
            registration.asset
        }
        Payment::OpenAccount { asset }
        | Payment::Pause { asset }
        | Payment::Unpause { asset }
        | Payment::ProgramAccount { asset, .. } => *asset,
        Payment::Receive {
            from,
            to,
            asset,
            amount,
            payer_grant,
            ..
        } => {
            disclose_party(&mut counterparties, CounterpartyRole::Payer, *from);
            disclose_party(&mut counterparties, CounterpartyRole::Recipient, *to);
            disclose_amount(&mut amounts, AmountRole::Transfer, *amount);
            disclose_grant_amounts(payer_grant, &mut amounts);
            *asset
        }
        Payment::Mint { asset, to, amount } => {
            disclose_party(&mut counterparties, CounterpartyRole::Recipient, *to);
            disclose_amount(&mut amounts, AmountRole::Transfer, *amount);
            *asset
        }
        Payment::Burn {
            asset,
            from,
            amount,
        } => {
            disclose_party(&mut counterparties, CounterpartyRole::Payer, *from);
            disclose_amount(&mut amounts, AmountRole::Transfer, *amount);
            *asset
        }
        Payment::IssueGrant(grant) => {
            disclose_grant(grant, &mut counterparties, &mut amounts);
            grant.asset
        }
        Payment::ProgramTransfer { legs, .. } => {
            for leg in legs {
                disclose_party(&mut counterparties, CounterpartyRole::Payer, leg.from);
                disclose_party(&mut counterparties, CounterpartyRole::Recipient, leg.to);
                disclose_amount(&mut amounts, AmountRole::Transfer, leg.amount);
            }
            [0; 32]
        }
        Payment::RevokeGrant { .. } => [0; 32],
    };
    (counterparties, amounts, asset)
}

fn payment_fields(activity: &Activity) -> Result<DisclosureFields, DisclosureError> {
    let TimestampBound {
        not_before,
        not_after,
    } = activity.timestamp_bound();
    let kind = (
        activity.activity_type().module(),
        activity.activity_type().ordinal(),
    );
    if kind == (ModuleId::Programs, 6)
        && !layerx_wire::limits::protocol_version_uses_occupancy(activity.protocol_version())
    {
        return Err(DisclosureError::MalformedPayload);
    }
    let payment = Payment::decode(kind.0, kind.1, activity.payload(), activity.actor_did())?;
    if let Payment::Receive {
        idempotency_key,
        receiver_authorization: auth,
        ..
    } = &payment
    {
        if *idempotency_key != activity.idempotency_key()
            || auth.network_id != activity.network_id()
            || auth.protocol_version != activity.protocol_version()
        {
            return Err(DisclosureError::MalformedPayload);
        }
    }
    let (counterparties, amounts, asset) = payment_monetary_fields(&payment);
    let payload_expires_at = match &payment {
        Payment::Receive { payer_grant, .. } => payer_grant.expiration,
        Payment::IssueGrant(grant) => grant.expiration,
        _ => not_after,
    };
    Ok(DisclosureFields {
        activity_type: activity.activity_type(),
        actor: activity.actor_did().to_vec(),
        authority: activity.authority().to_vec(),
        counterparties,
        amounts,
        asset,
        fee_limit: activity.fee_limit(),
        expiry: Expiry {
            not_before,
            not_after,
            payload_expires_at,
        },
        idempotency_key: activity.idempotency_key(),
        evm_payout_binding: None,
        withdrawal: None,
        payment: Some(payment),
    })
}

fn governance_fields(
    activity: &Activity,
    not_before: u64,
    not_after: u64,
) -> Result<DisclosureFields, DisclosureError> {
    let binding = decode_evm_payout_binding(activity.payload(), activity)?;
    Ok(DisclosureFields {
        activity_type: activity.activity_type(),
        actor: activity.actor_did().to_vec(),
        authority: activity.authority().to_vec(),
        counterparties: Vec::new(),
        amounts: Vec::new(),
        asset: [0; 32],
        fee_limit: activity.fee_limit(),
        expiry: Expiry {
            not_before,
            not_after,
            payload_expires_at: not_after,
        },
        idempotency_key: activity.idempotency_key(),
        evm_payout_binding: Some(binding),
        withdrawal: None,
        payment: None,
    })
}

fn decoded_fields(activity: &Activity) -> Result<DisclosureFields, DisclosureError> {
    if activity.activity_type().module() == ModuleId::Asset
        && activity.activity_type().ordinal() == ASSET_WITHDRAW_ORDINAL
    {
        return decode_withdraw(activity);
    }
    let TimestampBound {
        not_before,
        not_after,
    } = activity.timestamp_bound();
    let kind = (
        activity.activity_type().module(),
        activity.activity_type().ordinal(),
    );
    if matches!(
        kind,
        (ModuleId::Asset, 1 | 2 | 3 | 4 | 6 | 7 | 8 | 10 | 11) | (ModuleId::Programs, 5 | 6)
    ) {
        return payment_fields(activity);
    }
    if kind == (ModuleId::Governance, GOVERNANCE_EVM_BINDING_ORDINAL) {
        return governance_fields(activity, not_before, not_after);
    }
    if kind == (ModuleId::Budget, BUDGET_CREATE_ORDINAL) {
        let budget = decode_budget_create(activity.payload())?;
        return Ok(DisclosureFields {
            activity_type: activity.activity_type(),
            actor: activity.actor_did().to_vec(),
            authority: activity.authority().to_vec(),
            counterparties: vec![
                Counterparty {
                    role: CounterpartyRole::Payer,
                    account: budget.owner,
                },
                Counterparty {
                    role: CounterpartyRole::Recipient,
                    account: budget.budget,
                },
            ],
            amounts: vec![DisclosedAmount {
                role: AmountRole::SpendingLimit,
                value: budget.per_period_limit,
            }],
            asset: budget.asset,
            fee_limit: activity.fee_limit(),
            expiry: Expiry {
                not_before,
                not_after,
                payload_expires_at: budget.expires_at,
            },
            idempotency_key: activity.idempotency_key(),
            evm_payout_binding: None,
            withdrawal: None,
            payment: None,
        });
    }
    let send = semantics(activity)?;
    Ok(DisclosureFields {
        activity_type: activity.activity_type(),
        actor: activity.actor_did().to_vec(),
        authority: activity.authority().to_vec(),
        counterparties: vec![
            Counterparty {
                role: CounterpartyRole::Payer,
                account: send.from,
            },
            Counterparty {
                role: CounterpartyRole::Recipient,
                account: send.to,
            },
        ],
        amounts: vec![DisclosedAmount {
            role: AmountRole::Transfer,
            value: send.amount,
        }],
        asset: send.asset,
        fee_limit: activity.fee_limit(),
        expiry: Expiry {
            not_before,
            not_after,
            payload_expires_at: send.expires_at,
        },
        idempotency_key: send.idempotency_key,
        evm_payout_binding: None,
        withdrawal: None,
        payment: None,
    })
}

struct DisclosureFields {
    activity_type: ActivityType,
    actor: Vec<u8>,
    authority: Vec<u8>,
    counterparties: Vec<Counterparty>,
    amounts: Vec<DisclosedAmount>,
    asset: [u8; 32],
    fee_limit: u128,
    expiry: Expiry,
    idempotency_key: [u8; 32],
    evm_payout_binding: Option<DisclosedEvmPayoutBinding>,
    withdrawal: Option<DisclosedWithdrawal>,
    payment: Option<Payment>,
}

impl Disclosure {
    #[must_use]
    pub const fn envelope_sequence(&self) -> u64 {
        self.activity.account_sequence()
    }

    /// # Errors
    /// Returns a payload decoding error for malformed canonical semantics.
    pub fn payload_sequence(&self) -> Result<Option<u64>, DisclosureError> {
        if let Some(payment) = &self.payment {
            return Ok(match payment {
                Payment::Receive { sequence, .. } => Some(*sequence),
                _ => None,
            });
        }
        match (self.activity_type.module(), self.activity_type.ordinal()) {
            (ModuleId::Asset, 5 | 6) | (ModuleId::Budget, 7) => {
                Ok(Some(semantics(&self.activity)?.sequence))
            }
            _ => Ok(None),
        }
    }

    fn validate_fields(&self) -> Result<(), DisclosureError> {
        let expected = decoded_fields(&self.activity)?;
        macro_rules! require_field {
            ($field:ident) => {
                if self.$field != expected.$field {
                    return Err(DisclosureError::FieldMismatch(stringify!($field)));
                }
            };
        }
        require_field!(activity_type);
        require_field!(actor);
        require_field!(authority);
        require_field!(counterparties);
        require_field!(amounts);
        require_field!(asset);
        require_field!(fee_limit);
        require_field!(expiry);
        require_field!(idempotency_key);
        require_field!(withdrawal);
        require_field!(evm_payout_binding);
        require_field!(payment);
        Ok(())
    }

    /// Re-encodes the activity represented by this disclosure.
    ///
    /// # Errors
    ///
    /// Names the first changed semantic field or returns a typed wire error.
    pub fn reencode(&self) -> Result<Vec<u8>, DisclosureError> {
        self.validate_fields()?;
        encode_unsigned(&self.activity).map_err(DisclosureError::from)
    }

    /// Computes a domain-separated audit digest over the validated disclosure form.
    ///
    /// # Errors
    ///
    /// Returns the first semantic mismatch or canonical transport encoding failure.
    pub fn audit_digest(&self) -> Result<[u8; 32], DisclosureError> {
        let transport = self.transport_bytes()?;
        let mut hasher = Sha256::new();
        hasher.update(b"LXP/agent/disclosure/v1\0");
        hasher.update(transport);
        Ok(hasher.finalize().into())
    }

    pub(crate) fn transport_bytes(&self) -> Result<Vec<u8>, DisclosureError> {
        self.validate_fields()?;
        let mut encoder = Encoder::new(MAX_TRANSPORT_DISCLOSURE_BYTES);
        encoder.structure_header(0x4453)?;
        encoder.u8(3)?;
        encoder.u64(self.envelope_sequence())?;
        match self.payload_sequence()? {
            Some(sequence) => {
                encoder.u8(1)?;
                encoder.u64(sequence)?;
            }
            None => encoder.u8(0)?,
        }
        encoder.u32(self.activity_type.value())?;
        encoder.bytes(&self.actor, 255)?;
        encoder.bytes(&self.authority, 524_288)?;
        encoder.sequence_length(self.counterparties.len(), 64)?;
        for counterparty in &self.counterparties {
            encoder.u8(match counterparty.role {
                CounterpartyRole::Payer => 1,
                CounterpartyRole::Recipient => 2,
            })?;
            encoder.fixed(&counterparty.account)?;
        }
        encoder.sequence_length(self.amounts.len(), 64)?;
        for amount in &self.amounts {
            encoder.u8(match amount.role {
                AmountRole::Transfer => 1,
                AmountRole::SpendingLimit => 2,
                AmountRole::SupplyCap => 3,
                AmountRole::PerDrawMaximum => 4,
                AmountRole::GrantAllowance => 5,
            })?;
            encoder.u128(amount.value)?;
        }
        encoder.fixed(&self.asset)?;
        encoder.u128(self.fee_limit)?;
        encoder.u64(self.expiry.not_before)?;
        encoder.u64(self.expiry.not_after)?;
        encoder.u64(self.expiry.payload_expires_at)?;
        encoder.fixed(&self.idempotency_key)?;
        match self.evm_payout_binding {
            Some(binding) => {
                encoder.u8(1)?;
                encoder.fixed(&binding.did_id)?;
                encoder.u32(binding.network_id)?;
                encoder.fixed(&binding.payout_address)?;
                encoder.fixed(&binding.ownership_signature_digest)?;
            }
            None => encoder.u8(0)?,
        }
        if let Some(withdrawal) = self.withdrawal {
            encoder.u64(withdrawal.account_sequence)?;
            encoder.fixed(&withdrawal.evm_recipient)?;
            encoder.fixed(&withdrawal.request_anchor)?;
        }
        if let Some(payment) = &self.payment {
            encoder.bytes(&payment.encode(&self.actor)?, 32768)?;
        }
        Ok(encoder.finish())
    }

    pub(crate) fn validated_digest(&self) -> Result<[u8; 32], DisclosureError> {
        let reencoded = self.reencode()?;
        let message = SignatureMessage::new(
            hash::Domain::SignaturePreimage,
            self.activity.protocol_version(),
            self.activity.network_id(),
            &reencoded,
        )
        .map_err(|_| DisclosureError::MalformedPayload)?;
        let digest = message.digest();
        if !ct::eq_fixed(&digest, &self.signing_digest) {
            return Err(DisclosureError::FieldMismatch("canonical_bytes"));
        }
        Ok(digest)
    }

    pub(crate) const fn scope(&self) -> (u16, u32) {
        (self.activity.protocol_version(), self.activity.network_id())
    }

    pub(crate) fn validate_bytes(
        &self,
        canonical: &[u8],
        registry: &ModuleRegistry,
    ) -> Result<(), DisclosureError> {
        let actual = bind(canonical, registry)?;
        macro_rules! require_field {
            ($field:ident) => {
                if self.$field != actual.$field {
                    return Err(DisclosureError::FieldMismatch(stringify!($field)));
                }
            };
        }
        require_field!(activity_type);
        require_field!(actor);
        require_field!(authority);
        require_field!(counterparties);
        require_field!(amounts);
        require_field!(asset);
        require_field!(fee_limit);
        require_field!(expiry);
        require_field!(idempotency_key);
        require_field!(withdrawal);
        let reencoded = self.reencode()?;
        if !ct::eq(&reencoded, canonical) {
            return Err(DisclosureError::FieldMismatch("canonical_bytes"));
        }
        Ok(())
    }
}

/// Decodes canonical unsigned bytes into their complete structured disclosure.
///
/// Opaque module payloads are rejected until a complete semantic decoder is
/// available; they can never pass through the signing interface undisclosed.
///
/// # Errors
///
/// Returns a typed wire, payload, commitment, or unsupported-activity refusal.
pub fn bind(canonical: &[u8], registry: &ModuleRegistry) -> Result<Disclosure, DisclosureError> {
    let activity = decode_unsigned(canonical, registry)?;
    if !ct::eq_fixed(&hash::payload_hash(&activity)?, &activity.payload_hash()) {
        return Err(DisclosureError::PayloadHash);
    }
    let reencoded = encode_unsigned(&activity)?;
    if !ct::eq(&reencoded, canonical) {
        return Err(DisclosureError::FieldMismatch("canonical_bytes"));
    }
    let fields = decoded_fields(&activity)?;
    let message = SignatureMessage::new(
        hash::Domain::SignaturePreimage,
        activity.protocol_version(),
        activity.network_id(),
        canonical,
    )
    .map_err(|_| DisclosureError::MalformedPayload)?;
    Ok(Disclosure {
        activity_type: fields.activity_type,
        actor: fields.actor,
        authority: fields.authority,
        counterparties: fields.counterparties,
        amounts: fields.amounts,
        asset: fields.asset,
        fee_limit: fields.fee_limit,
        expiry: fields.expiry,
        idempotency_key: fields.idempotency_key,
        evm_payout_binding: fields.evm_payout_binding,
        withdrawal: fields.withdrawal,
        payment: fields.payment,
        activity,
        signing_digest: message.digest(),
    })
}
