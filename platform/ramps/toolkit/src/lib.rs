#![forbid(unsafe_code)]

pub mod clients;
pub mod engine;
pub mod journal;

use layerx_intents::{compile, DisclosureCheck, Intent, IntentKind, LxpReceive, LxpSend};
use layerx_proof::receipt::{verify, AuthorizedBatch, ReceiptCheck};
use layerx_types::account::{AccountId, AccountNamespace};
use layerx_types::amount::Amount;
use layerx_types::ids::{AssetId, IdempotencyKey};
use layerx_types::intent::{
    AuthorizationSignature, ContextHash, NetworkId, PayerGrantId, ProtocolVersion, PublicKey,
    SendAuthorization, SendAuthorizationKind, Sequence, TimestampSeconds,
};
use layerx_types::payload::ModuleRegistry;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub const EXTERNAL_CUSTODY_LABEL: &str =
    "External custody: this independent market maker controls the off-platform funds and payout.";
pub const ORDER_DIGEST_DOMAIN: &[u8] = b"LXP/market-maker-ramp/order/v1\0";
pub const PROVIDER_CONTRACT_VERSION: &str = "layerx-ramp-provider-v1";
pub const COMPLIANCE_CONTRACT_VERSION: &str = "layerx-ramp-compliance-v1";
pub const PAXEER_CONTRACT_VERSION: &str = "layerx-ramp-paxeer-v1";

const MAX_IDENTIFIER_BYTES: usize = 128;
const MAX_CURRENCY_BYTES: usize = 16;
const MAX_TOKEN_BYTES: usize = 512;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RampDirection {
    OnRamp,
    OffRamp,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthenticatedPrincipal {
    pub principal_id: String,
    pub account: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OperatorIdentity {
    pub principal_id: String,
    pub account: String,
    pub signer_key_handle: String,
}

impl OperatorIdentity {
    pub fn validate(&self) -> Result<(), RampError> {
        validate_operator(self)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuoteTerms {
    pub quote_id: String,
    pub direction: RampDirection,
    pub layerx_asset: [u8; 32],
    pub layerx_amount: u128,
    pub external_currency: String,
    pub external_amount_minor: u128,
    pub rate_numerator: u128,
    pub rate_denominator: u128,
    pub fee_minor: u128,
    pub maximum_slippage_bps: u16,
    pub context: [u8; 32],
    pub provider_token: String,
    pub payout_token: String,
    pub expires_at: u64,
}

impl QuoteTerms {
    pub fn validate(&self, now: u64) -> Result<(), RampError> {
        validate_identifier(&self.quote_id)?;
        if self.layerx_asset == [0; 32]
            || self.layerx_amount == 0
            || self.external_amount_minor == 0
            || self.rate_numerator == 0
            || self.rate_denominator == 0
            || self.maximum_slippage_bps > 10_000
            || self.context == [0; 32]
            || self.expires_at <= now
            || self.external_currency.is_empty()
            || self.external_currency.len() > MAX_CURRENCY_BYTES
            || self.provider_token.is_empty()
            || self.provider_token.len() > MAX_TOKEN_BYTES
            || self.payout_token.is_empty()
            || self.payout_token.len() > MAX_TOKEN_BYTES
        {
            return Err(RampError::InvalidOrder);
        }
        validate_opaque(&self.provider_token, MAX_TOKEN_BYTES)?;
        validate_opaque(&self.payout_token, MAX_TOKEN_BYTES)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateOrder {
    pub order_id: String,
    pub quote_id: String,
    pub payer_grant: Option<[u8; 32]>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RampOrder {
    pub order_id: String,
    pub quote: QuoteTerms,
    pub customer: AuthenticatedPrincipal,
    pub operator: OperatorIdentity,
    pub payer_grant: Option<[u8; 32]>,
    pub context: [u8; 32],
    pub order_digest: [u8; 32],
}

impl RampOrder {
    pub fn bind(
        request: CreateOrder,
        quote: QuoteTerms,
        customer: AuthenticatedPrincipal,
        operator: OperatorIdentity,
        now: u64,
    ) -> Result<Self, RampError> {
        validate_identifier(&request.order_id)?;
        validate_identifier(&request.quote_id)?;
        quote.validate(now)?;
        validate_principal(&customer)?;
        validate_operator(&operator)?;
        if request.quote_id != quote.quote_id
            || request.payer_grant == Some([0; 32])
            || matches!(quote.direction, RampDirection::OnRamp) && request.payer_grant.is_some()
            || matches!(quote.direction, RampDirection::OffRamp) && request.payer_grant.is_none()
        {
            return Err(RampError::InvalidOrder);
        }
        let context = quote.context;
        let mut order = Self {
            order_id: request.order_id,
            quote,
            customer,
            operator,
            payer_grant: request.payer_grant,
            context,
            order_digest: [0; 32],
        };
        order.order_digest = order.digest();
        Ok(order)
    }

    #[must_use]
    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(ORDER_DIGEST_DOMAIN);
        field(&mut hasher, self.direction_name().as_bytes());
        field(&mut hasher, self.order_id.as_bytes());
        field(&mut hasher, self.quote.quote_id.as_bytes());
        field(&mut hasher, self.customer.principal_id.as_bytes());
        field(&mut hasher, self.customer.account.as_bytes());
        field(&mut hasher, self.operator.principal_id.as_bytes());
        field(&mut hasher, self.operator.account.as_bytes());
        field(&mut hasher, self.operator.signer_key_handle.as_bytes());
        field(&mut hasher, &self.quote.layerx_asset);
        field(&mut hasher, &self.quote.layerx_amount.to_be_bytes());
        field(&mut hasher, self.quote.external_currency.as_bytes());
        field(
            &mut hasher,
            &self.quote.external_amount_minor.to_be_bytes(),
        );
        field(&mut hasher, &self.quote.rate_numerator.to_be_bytes());
        field(&mut hasher, &self.quote.rate_denominator.to_be_bytes());
        field(&mut hasher, &self.quote.fee_minor.to_be_bytes());
        field(
            &mut hasher,
            &self.quote.maximum_slippage_bps.to_be_bytes(),
        );
        field(&mut hasher, &self.context);
        field(&mut hasher, self.quote.provider_token.as_bytes());
        field(&mut hasher, self.quote.payout_token.as_bytes());
        field(&mut hasher, &self.quote.expires_at.to_be_bytes());
        field(
            &mut hasher,
            self.payer_grant.as_ref().map_or(&[], |value| value.as_slice()),
        );
        hasher.finalize().into()
    }

    #[must_use]
    pub const fn direction(&self) -> RampDirection {
        self.quote.direction
    }

    fn direction_name(&self) -> &'static str {
        match self.quote.direction {
            RampDirection::OnRamp => "on_ramp",
            RampDirection::OffRamp => "off_ramp",
        }
    }

    pub fn validate_bound(&self) -> Result<(), RampError> {
        if self.order_digest != self.digest() || self.context != self.quote.context {
            return Err(RampError::OrderBinding);
        }
        Ok(())
    }
}

fn validate_identifier(value: &str) -> Result<(), RampError> {
    if value.is_empty()
        || value.len() > MAX_IDENTIFIER_BYTES
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(RampError::InvalidOrder);
    }
    Ok(())
}

pub(crate) fn validate_principal(principal: &AuthenticatedPrincipal) -> Result<(), RampError> {
    validate_identifier(&principal.principal_id)?;
    let account = AccountId::parse(&principal.account).map_err(|_| RampError::InvalidPrincipal)?;
    if account.namespace() != AccountNamespace::AgentMain {
        return Err(RampError::InvalidPrincipal);
    }
    Ok(())
}

fn validate_operator(operator: &OperatorIdentity) -> Result<(), RampError> {
    validate_identifier(&operator.principal_id)?;
    validate_opaque(&operator.signer_key_handle, MAX_TOKEN_BYTES)?;
    let account = AccountId::parse(&operator.account).map_err(|_| RampError::InvalidPrincipal)?;
    if account.namespace() != AccountNamespace::AgentMain {
        return Err(RampError::InvalidPrincipal);
    }
    Ok(())
}

fn validate_opaque(value: &str, maximum: usize) -> Result<(), RampError> {
    if value.is_empty()
        || value.len() > maximum
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || matches!(byte, b'\'' | b'"' | b'\\'))
    {
        return Err(RampError::InvalidOrder);
    }
    Ok(())
}

fn field(hasher: &mut Sha256, bytes: &[u8]) {
    let length = u128::from(bytes.len());
    hasher.update(length.to_be_bytes());
    hasher.update(bytes);
}

#[derive(Clone, Debug)]
pub struct CompiledPayment {
    pub intent: Intent,
    pub activity_type: layerx_types::payload::ActivityType,
    pub payload: layerx_types::payload::Payload,
    pub canonical_payload: Vec<u8>,
    pub payload_hash: [u8; 32],
}

pub fn compile_payer_grant_draw(
    order: &RampOrder,
    receiver_sequence: u64,
    registry: &ModuleRegistry,
) -> Result<CompiledPayment, RampError> {
    order.validate_bound()?;
    let grant = order.payer_grant.ok_or(RampError::PayerGrantRequired)?;
    let (from, to) = match order.direction() {
        RampDirection::OnRamp => (&order.operator.account, &order.customer.account),
        RampDirection::OffRamp => (&order.customer.account, &order.operator.account),
    };
    let receive = LxpReceive::new(
        AccountId::parse(from).map_err(|_| RampError::InvalidPrincipal)?,
        AccountId::parse(to).map_err(|_| RampError::InvalidPrincipal)?,
        AssetId::new(order.quote.layerx_asset),
        Amount::from_u128(order.quote.layerx_amount),
        PayerGrantId::new(grant),
        Sequence::from_u64(receiver_sequence),
        IdempotencyKey::new(order.order_digest),
        ContextHash::new(order.context),
    )
    .map_err(|_| RampError::Intent)?;
    compile_intent(Intent::v1(IntentKind::LxpReceive(receive)), registry)
}

pub fn operator_send_authorization_message(
    order: &RampOrder,
    account_sequence: u64,
    network_id: u32,
    protocol_version: u16,
) -> Result<Vec<u8>, RampError> {
    order.validate_bound()?;
    if order.direction() != RampDirection::OnRamp
        || order.payer_grant.is_some()
        || network_id == 0
        || protocol_version == 0
    {
        return Err(RampError::InvalidOrder);
    }
    let from = layerx_wire::hash::account_id(
        &AccountId::parse(&order.operator.account).map_err(|_| RampError::InvalidPrincipal)?,
    )
    .map_err(|_| RampError::Intent)?;
    let to = layerx_wire::hash::account_id(
        &AccountId::parse(&order.customer.account).map_err(|_| RampError::InvalidPrincipal)?,
    )
    .map_err(|_| RampError::Intent)?;
    let mut message = Vec::with_capacity(236);
    message.extend_from_slice(&0x5301_u16.to_be_bytes());
    message.extend_from_slice(&from);
    message.extend_from_slice(&to);
    message.extend_from_slice(&order.quote.layerx_asset);
    message.extend_from_slice(&order.quote.layerx_amount.to_be_bytes());
    message.extend_from_slice(&account_sequence.to_be_bytes());
    message.extend_from_slice(&order.order_digest);
    message.extend_from_slice(&order.quote.expires_at.to_be_bytes());
    message.extend_from_slice(&order.context);
    message.push(0);
    message.push(SendAuthorizationKind::Owner as u8);
    message.extend_from_slice(&from);
    message.extend_from_slice(&order.context);
    message.extend_from_slice(&network_id.to_be_bytes());
    message.extend_from_slice(&protocol_version.to_be_bytes());
    Ok(message)
}

pub fn compile_operator_send(
    order: &RampOrder,
    account_sequence: u64,
    network_id: u32,
    protocol_version: u16,
    public_key: [u8; 32],
    authorization_signature: [u8; 64],
    registry: &ModuleRegistry,
) -> Result<CompiledPayment, RampError> {
    operator_send_authorization_message(order, account_sequence, network_id, protocol_version)?;
    let send = LxpSend::new(
        AccountId::parse(&order.operator.account).map_err(|_| RampError::InvalidPrincipal)?,
        AccountId::parse(&order.customer.account).map_err(|_| RampError::InvalidPrincipal)?,
        AssetId::new(order.quote.layerx_asset),
        Amount::from_u128(order.quote.layerx_amount),
        Sequence::from_u64(account_sequence),
        IdempotencyKey::new(order.order_digest),
        TimestampSeconds::from_u64(order.quote.expires_at),
        ContextHash::new(order.context),
        SendAuthorization::new(
            SendAuthorizationKind::Owner,
            PublicKey::new(public_key),
            AuthorizationSignature::new(authorization_signature),
        ),
        NetworkId::new(network_id).map_err(|_| RampError::Intent)?,
        ProtocolVersion::new(protocol_version).map_err(|_| RampError::Intent)?,
    )
    .map_err(|_| RampError::Intent)?;
    compile_intent(Intent::v1(IntentKind::LxpSend(send)), registry)
}

fn compile_intent(intent: Intent, registry: &ModuleRegistry) -> Result<CompiledPayment, RampError> {
    let compiled = compile(&intent, registry).map_err(|_| RampError::Intent)?;
    let disclosure = DisclosureCheck::verify(&intent, &compiled).map_err(|_| RampError::Intent)?;
    Ok(CompiledPayment {
        intent,
        activity_type: compiled.activity_type(),
        payload: compiled.payload().clone(),
        canonical_payload: disclosure.canonical_payload().to_vec(),
        payload_hash: disclosure.payload_hash(),
    })
}

#[derive(Clone, Debug)]
pub struct ReceiptEvidence {
    pub activity_id: [u8; 32],
    pub canonical_receipt: Vec<u8>,
    pub authorized_batch: AuthorizedBatch,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct VerifiedLayerxLeg {
    pub activity_id: [u8; 32],
    pub receipt_digest: [u8; 32],
    pub batch_id: [u8; 32],
    pub resulting_state_root: [u8; 32],
}

pub fn verify_order_receipt(
    order: &RampOrder,
    evidence: &ReceiptEvidence,
) -> Result<VerifiedLayerxLeg, RampError> {
    order.validate_bound()?;
    let verified = verify(&evidence.canonical_receipt, &evidence.authorized_batch)
        .map_err(|failure| RampError::Receipt(failure.check))?;
    let protocol = verified
        .receipt()
        .protocol()
        .ok_or(RampError::ReceiptMismatch)?;
    let (from, to) = match order.direction() {
        RampDirection::OnRamp => (&order.operator.account, &order.customer.account),
        RampDirection::OffRamp => (&order.customer.account, &order.operator.account),
    };
    let expected_from = layerx_wire::hash::account_id(
        &AccountId::parse(from).map_err(|_| RampError::InvalidPrincipal)?,
    )
    .map_err(|_| RampError::ReceiptMismatch)?;
    let expected_to = layerx_wire::hash::account_id(
        &AccountId::parse(to).map_err(|_| RampError::InvalidPrincipal)?,
    )
    .map_err(|_| RampError::ReceiptMismatch)?;
    if protocol.activity_id() != evidence.activity_id
        || protocol.from() != expected_from
        || protocol.to() != expected_to
        || protocol.asset() != order.quote.layerx_asset
        || protocol.amount() != order.quote.layerx_amount
        || protocol.context_hash() != order.context
    {
        return Err(RampError::ReceiptMismatch);
    }
    let receipt_digest = verified
        .evidence()
        .receipt_digest()
        .ok_or(RampError::ReceiptMismatch)?;
    Ok(VerifiedLayerxLeg {
        activity_id: evidence.activity_id,
        receipt_digest,
        batch_id: protocol.batch_id(),
        resulting_state_root: protocol.resulting_state_root(),
    })
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AggregateStatus {
    Pending,
    Unknown,
    Refused,
    ManualReview,
    Reversed,
    Done,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RampPresentation {
    pub external_custody_label: &'static str,
    pub status: AggregateStatus,
    pub order_digest: [u8; 32],
    pub activity_id: Option<[u8; 32]>,
    pub receipt_digest: Option<[u8; 32]>,
    pub provider_evidence_digest: Option<[u8; 32]>,
    pub refusal_code: Option<String>,
    pub retry_at: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RampError {
    InvalidPrincipal,
    InvalidOrder,
    OrderBinding,
    PayerGrantRequired,
    Intent,
    Receipt(ReceiptCheck),
    ReceiptMismatch,
    Journal,
    Conflict,
    LeaseHeld,
    IllegalTransition,
    Compliance,
    Provider,
    Layerx,
    Paxeer,
    Configuration,
}

#[must_use]
pub const fn platform_ramp_toolkit() -> &'static str {
    "durable-ordinary-principal-receipt-gated-market-maker-ramp"
}
