use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use layerx_intents::CompiledIntent;
use layerx_interop_gateway::adapter::AdapterId;
use layerx_interop_gateway::gateway::{TranslationKind, TranslationRequest, TranslationStatus};
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::{TraceId, Traced};
use layerx_interop_gateway::GatewayCore;
use layerx_portable::PortableReceipt;
use layerx_proof::receipt::{verify, AuthorizedBatch};
use layerx_types::payload::ModuleId;
use p256::ecdsa::VerifyingKey;
use serde::Serialize;
use sha2::{Digest as _, Sha256};

use crate::error::Ap2Error;
use crate::jose::{verify_signature, KeyResolver};
use crate::model::Merchant;
use crate::verify::{MandateVerifier, VerificationContext, VerifiedMandates};

const ADAPTER_ID: &str = "ap2";
const IDEMPOTENCY_DOMAIN: &[u8] = b"LayerX/interop/AP2/idempotency/v1\0";
const REQUEST_DOMAIN: &[u8] = b"LayerX/interop/AP2/request/v1\0";
const TEXT_LIMIT: usize = 1_024;

/// Exact ISO-currency to LayerX-asset mapping supplied by deployment policy.
/// AP2 amounts are integer minor units and conversion is checked integer
/// multiplication only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayerXAssetBinding {
    pub currency: String,
    pub minor_unit_exponent: u8,
    pub atomic_units_per_minor_unit: u128,
    pub asset: [u8; 32],
    pub payer_receipt_account: [u8; 32],
    pub payee_receipt_account: [u8; 32],
    pub payee_merchant: Merchant,
}

impl LayerXAssetBinding {
    fn validate(&self) -> Result<(), Ap2Error> {
        if self.currency.len() != 3
            || !self.currency.bytes().all(|byte| byte.is_ascii_uppercase())
            || self.minor_unit_exponent > 18
            || self.atomic_units_per_minor_unit == 0
            || self.asset == [0; 32]
            || self.payer_receipt_account == [0; 32]
            || self.payee_receipt_account == [0; 32]
            || self.payer_receipt_account == self.payee_receipt_account
        {
            return Err(Ap2Error::AmountConversion);
        }
        self.payee_merchant.validate()
    }
}

/// Constraint-checked AP2 payment meaning handed to the sole typed `LayerX`
/// intent authority. It carries no signature or canonical payload constructor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorizedPayment {
    principal: PrincipalId,
    transaction_id: String,
    checkout_id: String,
    asset: [u8; 32],
    payer_receipt_account: [u8; 32],
    payee_receipt_account: [u8; 32],
    amount: u128,
    execution_at: u64,
    idempotency_key: [u8; 32],
    request_digest: [u8; 32],
}

impl AuthorizedPayment {
    #[must_use]
    pub const fn principal(&self) -> &PrincipalId {
        &self.principal
    }

    #[must_use]
    pub fn transaction_id(&self) -> &str {
        &self.transaction_id
    }

    #[must_use]
    pub fn checkout_id(&self) -> &str {
        &self.checkout_id
    }

    #[must_use]
    pub const fn asset(&self) -> [u8; 32] {
        self.asset
    }

    #[must_use]
    pub const fn payer_receipt_account(&self) -> [u8; 32] {
        self.payer_receipt_account
    }

    #[must_use]
    pub const fn payee_receipt_account(&self) -> [u8; 32] {
        self.payee_receipt_account
    }

    #[must_use]
    pub const fn amount(&self) -> u128 {
        self.amount
    }

    #[must_use]
    pub const fn execution_at(&self) -> u64 {
        self.execution_at
    }

    #[must_use]
    pub const fn idempotency_key(&self) -> [u8; 32] {
        self.idempotency_key
    }

    #[must_use]
    pub const fn request_digest(&self) -> [u8; 32] {
        self.request_digest
    }
}

/// Real typed-intent and execution boundary. `CompiledIntent` has no public
/// constructor, so an implementation can only return one through the existing
/// `layerx-intents` compiler authority.
pub trait LayerXIntentPlane {
    /// Maps the verified payment meaning into the existing typed intent path.
    ///
    /// # Errors
    ///
    /// Returns a typed construction or policy refusal.
    fn compile(
        &mut self,
        payment: &AuthorizedPayment,
        trace: &TraceId,
    ) -> Result<CompiledIntent, Ap2Error>;

    /// Executes the already compiled intent under its stable idempotency key.
    ///
    /// # Errors
    ///
    /// Returns a typed transport or protocol refusal; uncertainty must be
    /// returned as `Pending`, never permission to replay a new effect.
    fn execute(
        &mut self,
        payment: &AuthorizedPayment,
        compiled: CompiledIntent,
        trace: &TraceId,
    ) -> Result<PlaneOutcome, Ap2Error>;
}

/// Core-produced execution evidence and the merchant's real order identifier.
#[derive(Debug)]
pub struct ExecutedPayment {
    pub canonical_receipt: Vec<u8>,
    pub authorised_batch: AuthorizedBatch,
    pub order_id: String,
}

/// Honest economic result from the `LayerX` plane.
#[derive(Debug)]
pub enum PlaneOutcome {
    Pending,
    Refused,
    Executed(Box<ExecutedPayment>),
}

/// External signing boundary for AP2 Payment and Checkout Receipt JWTs. The
/// adapter verifies every returned raw ES256 signature before exporting it.
pub trait ReceiptSigner {
    fn issuer(&self) -> &str;
    fn key_id(&self) -> &str;
    fn verifying_key(&self) -> &VerifyingKey;

    /// Signs the exact compact-JWS `protected.payload` bytes.
    ///
    /// # Errors
    ///
    /// Returns a typed HSM, wallet or policy refusal.
    fn sign_es256(&mut self, signing_input: &[u8]) -> Result<[u8; 64], Ap2Error>;
}

/// Shared portable receipt format. Construction and external verification are
/// owned by `layerx-portable`, so AP2 cannot drift into a private proof shape.
pub type PortableLayerXEvidence = PortableReceipt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedAp2Evidence {
    pub payment_receipt_jwt: String,
    pub checkout_receipt_jwt: String,
    pub layerx: PortableLayerXEvidence,
    pub receipt_digest: [u8; 32],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdapterOutcome {
    Pending,
    Refused,
    AlreadySettled {
        receipt_digest: [u8; 32],
    },
    Settled(Box<SignedAp2Evidence>),
    SettledEvidenceUnavailable {
        receipt_digest: [u8; 32],
        reason: Ap2Error,
    },
}

/// AP2 edge adapter. Verification is pure until a `VerifiedMandates` value
/// exists, and every state-changing path is recorded by `GatewayCore` and can
/// reach success only through its receipt verifier.
pub struct Ap2Adapter<'a, R> {
    verifier: MandateVerifier<'a, R>,
}

impl<'a, R: KeyResolver> Ap2Adapter<'a, R> {
    #[must_use]
    pub const fn new(resolver: &'a R) -> Self {
        Self {
            verifier: MandateVerifier::new(resolver),
        }
    }

    /// Verifies and executes one AP2 checkout/payment pair.
    ///
    /// # Errors
    ///
    /// Returns a trace-bound cryptographic, constraint, gateway, typed-intent
    /// or receipt-evidence refusal. Once an economic effect is verified, a
    /// receipt-signing failure is returned as an honest settled outcome.
    #[allow(clippy::too_many_arguments)]
    pub fn execute(
        &self,
        gateway: &mut GatewayCore,
        principal: &PrincipalId,
        checkout_presentation: &str,
        payment_presentation: &str,
        verification: &VerificationContext<'_>,
        binding: &LayerXAssetBinding,
        plane: &mut impl LayerXIntentPlane,
        payment_signer: &mut impl ReceiptSigner,
        checkout_signer: &mut impl ReceiptSigner,
        trace: &TraceId,
        now: u64,
    ) -> Result<AdapterOutcome, Traced<Ap2Error>> {
        let fail = |error| trace.wrap(error);
        let mandates = self
            .verifier
            .verify(checkout_presentation, payment_presentation, verification)
            .map_err(fail)?;
        let payment = prepare_payment(principal, &mandates, verification, binding).map_err(fail)?;
        let request = TranslationRequest::new(
            adapter_id().map_err(fail)?,
            TranslationKind::StateChanging,
            payment.idempotency_key,
            payment.request_digest,
        )
        .map_err(|error| fail(Ap2Error::Gateway(error)))?;
        match gateway
            .begin_translation(principal, &request, trace, now)
            .map_err(|error| trace.wrap(Ap2Error::Gateway(error.into_error())))?
        {
            TranslationStatus::ReceiptVerified { receipt_digest } => {
                return Ok(AdapterOutcome::AlreadySettled { receipt_digest });
            }
            TranslationStatus::Refused => return Ok(AdapterOutcome::Refused),
            TranslationStatus::Pending => {}
            TranslationStatus::Translated => {
                return Err(fail(Ap2Error::Gateway(
                    layerx_interop_gateway::error::GatewayError::Corrupt(
                        "AP2 state-changing translation was read-only",
                    ),
                )));
            }
        }
        let compiled = match plane.compile(&payment, trace) {
            Ok(compiled) => compiled,
            Err(error) => {
                refuse(gateway, principal, payment.idempotency_key, trace, now)?;
                return Err(fail(error));
            }
        };
        if compiled.activity_type().module() != ModuleId::Asset
            || compiled.activity_type().ordinal() != 5
        {
            refuse(gateway, principal, payment.idempotency_key, trace, now)?;
            return Err(fail(Ap2Error::IntentMismatch));
        }
        match plane.execute(&payment, compiled, trace).map_err(fail)? {
            PlaneOutcome::Pending => Ok(AdapterOutcome::Pending),
            PlaneOutcome::Refused => {
                refuse(gateway, principal, payment.idempotency_key, trace, now)?;
                Ok(AdapterOutcome::Refused)
            }
            PlaneOutcome::Executed(executed) => Self::finish(
                gateway,
                principal,
                &payment,
                &mandates,
                &executed,
                payment_signer,
                checkout_signer,
                trace,
                now,
            ),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish(
        gateway: &mut GatewayCore,
        principal: &PrincipalId,
        payment: &AuthorizedPayment,
        mandates: &VerifiedMandates,
        executed: &ExecutedPayment,
        payment_signer: &mut impl ReceiptSigner,
        checkout_signer: &mut impl ReceiptSigner,
        trace: &TraceId,
        now: u64,
    ) -> Result<AdapterOutcome, Traced<Ap2Error>> {
        let fail = |error| trace.wrap(error);
        if executed.order_id.is_empty()
            || executed.order_id.len() > TEXT_LIMIT
            || executed
                .order_id
                .bytes()
                .any(|byte| byte.is_ascii_control())
        {
            return Err(fail(Ap2Error::EvidenceMismatch));
        }
        let verified = verify(&executed.canonical_receipt, &executed.authorised_batch)
            .map_err(|_| fail(Ap2Error::EvidenceMismatch))?;
        let protocol = verified
            .receipt()
            .protocol()
            .ok_or_else(|| fail(Ap2Error::EvidenceMismatch))?;
        if protocol.from() != payment.payer_receipt_account
            || protocol.to() != payment.payee_receipt_account
            || protocol.asset() != payment.asset
            || protocol.amount() != payment.amount
        {
            return Err(fail(Ap2Error::EvidenceMismatch));
        }
        let status = gateway
            .settle_with_receipt(
                principal,
                payment.idempotency_key,
                &executed.canonical_receipt,
                &executed.authorised_batch,
                trace,
                now,
            )
            .map_err(|error| trace.wrap(Ap2Error::Gateway(error.into_error())))?;
        let TranslationStatus::ReceiptVerified { receipt_digest } = status else {
            return Err(fail(Ap2Error::EvidenceMissing));
        };
        let status_label = settled_status_label(&status).map_err(fail)?;
        let layerx =
            PortableReceipt::export(&executed.canonical_receipt, &executed.authorised_batch)
                .map_err(|_| fail(Ap2Error::EvidenceMismatch))?;
        if layerx.receipt_digest() != URL_SAFE_NO_PAD.encode(receipt_digest) {
            return Err(fail(Ap2Error::EvidenceMismatch));
        }
        let signed = sign_evidence(
            mandates,
            &executed.order_id,
            layerx.clone(),
            receipt_digest,
            status_label,
            payment_signer,
            checkout_signer,
            now,
        );
        match signed {
            Ok(evidence) => Ok(AdapterOutcome::Settled(Box::new(evidence))),
            Err(reason) => Ok(AdapterOutcome::SettledEvidenceUnavailable {
                receipt_digest,
                reason,
            }),
        }
    }
}

/// Binds one verified mandate pair to deployment asset policy, refusing any
/// mandate whose verified payee does not match the pinned execution merchant.
///
/// # Errors
///
/// Returns a typed merchant-binding or amount-conversion refusal.
pub fn authorize_payment(
    principal: &PrincipalId,
    mandates: &VerifiedMandates,
    verification: &VerificationContext<'_>,
    binding: &LayerXAssetBinding,
) -> Result<AuthorizedPayment, Ap2Error> {
    prepare_payment(principal, mandates, verification, binding)
}

fn prepare_payment(
    principal: &PrincipalId,
    mandates: &VerifiedMandates,
    verification: &VerificationContext<'_>,
    binding: &LayerXAssetBinding,
) -> Result<AuthorizedPayment, Ap2Error> {
    binding.validate()?;
    if !binding.payee_merchant.matches(mandates.payee()) {
        return Err(Ap2Error::ConstraintViolated("payee_merchant_binding"));
    }
    if mandates.amount().currency() != binding.currency
        || verification.currency_minor_exponent != binding.minor_unit_exponent
    {
        return Err(Ap2Error::AmountConversion);
    }
    let amount = mandates
        .amount()
        .minor_units()
        .checked_mul(binding.atomic_units_per_minor_unit)
        .ok_or(Ap2Error::AmountConversion)?;
    if amount == 0 {
        return Err(Ap2Error::AmountConversion);
    }
    let stable_reference = mandates.stable_payment_reference();
    let idempotency_key = digest(
        IDEMPOTENCY_DOMAIN,
        &[principal.as_str().as_bytes(), stable_reference.as_bytes()],
    );
    let amount_bytes = amount.to_be_bytes();
    let execution_bytes = mandates.execution_at().to_be_bytes();
    let mut request_parts = mandates.request_material().to_vec();
    request_parts.extend([
        binding.currency.as_bytes(),
        &binding.asset,
        &binding.payer_receipt_account,
        &binding.payee_receipt_account,
        &amount_bytes,
        &execution_bytes,
    ]);
    let request_digest = digest(REQUEST_DOMAIN, &request_parts);
    Ok(AuthorizedPayment {
        principal: principal.clone(),
        transaction_id: mandates.transaction_id().to_owned(),
        checkout_id: mandates.checkout_id().to_owned(),
        asset: binding.asset,
        payer_receipt_account: binding.payer_receipt_account,
        payee_receipt_account: binding.payee_receipt_account,
        amount,
        execution_at: mandates.execution_at(),
        idempotency_key,
        request_digest,
    })
}

fn refuse(
    gateway: &mut GatewayCore,
    principal: &PrincipalId,
    idempotency_key: [u8; 32],
    trace: &TraceId,
    now: u64,
) -> Result<(), Traced<Ap2Error>> {
    gateway
        .refuse_translation(principal, idempotency_key, trace, now)
        .map(|_| ())
        .map_err(|error| trace.wrap(Ap2Error::Gateway(error.into_error())))
}

#[derive(Serialize)]
struct PaymentReceipt {
    status: &'static str,
    iss: String,
    iat: u64,
    reference: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    psp_confirmation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    network_confirmation_id: Option<String>,
    layerx_receipt_reference: String,
    layerx_evidence: PortableLayerXEvidence,
}

#[derive(Serialize)]
struct CheckoutReceipt {
    status: &'static str,
    iss: String,
    iat: u64,
    reference: String,
    order_id: String,
    layerx_receipt_reference: String,
    layerx_evidence: PortableLayerXEvidence,
}

fn settled_status_label(status: &TranslationStatus) -> Result<&'static str, Ap2Error> {
    match status {
        TranslationStatus::ReceiptVerified { .. } => Ok("Success"),
        TranslationStatus::Pending | TranslationStatus::Refused | TranslationStatus::Translated => {
            Err(Ap2Error::EvidenceMissing)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn sign_evidence(
    mandates: &VerifiedMandates,
    order_id: &str,
    layerx: PortableLayerXEvidence,
    receipt_digest: [u8; 32],
    status_label: &'static str,
    payment_signer: &mut impl ReceiptSigner,
    checkout_signer: &mut impl ReceiptSigner,
    now: u64,
) -> Result<SignedAp2Evidence, Ap2Error> {
    let layerx_receipt_reference = format!("lxp:{}", URL_SAFE_NO_PAD.encode(receipt_digest));
    let payment_receipt = PaymentReceipt {
        status: status_label,
        iss: payment_signer.issuer().to_owned(),
        iat: now,
        reference: mandates.payment_receipt_reference(),
        psp_confirmation_id: None,
        network_confirmation_id: None,
        layerx_receipt_reference: layerx_receipt_reference.clone(),
        layerx_evidence: layerx.clone(),
    };
    let payment_receipt_jwt = sign_receipt(&payment_receipt, payment_signer)?;
    let checkout_receipt = CheckoutReceipt {
        status: status_label,
        iss: checkout_signer.issuer().to_owned(),
        iat: now,
        reference: mandates.checkout_receipt_reference(),
        order_id: order_id.to_owned(),
        layerx_receipt_reference,
        layerx_evidence: layerx.clone(),
    };
    let checkout_receipt_jwt = sign_receipt(&checkout_receipt, checkout_signer)?;
    Ok(SignedAp2Evidence {
        payment_receipt_jwt,
        checkout_receipt_jwt,
        layerx,
        receipt_digest,
    })
}

#[derive(Serialize)]
struct ReceiptHeader<'a> {
    alg: &'static str,
    typ: &'static str,
    kid: &'a str,
}

fn sign_receipt<T: Serialize>(
    receipt: &T,
    signer: &mut impl ReceiptSigner,
) -> Result<String, Ap2Error> {
    if signer.issuer().is_empty()
        || signer.issuer().len() > TEXT_LIMIT
        || signer.key_id().is_empty()
        || signer.key_id().len() > 512
        || signer
            .issuer()
            .bytes()
            .chain(signer.key_id().bytes())
            .any(|byte| byte.is_ascii_control())
    {
        return Err(Ap2Error::ReceiptSigning);
    }
    let header = serde_json::to_vec(&ReceiptHeader {
        alg: "ES256",
        typ: "JWT",
        kid: signer.key_id(),
    })
    .map_err(|_| Ap2Error::ReceiptSigning)?;
    let payload = serde_json::to_vec(receipt).map_err(|_| Ap2Error::ReceiptSigning)?;
    let signing_input = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(header),
        URL_SAFE_NO_PAD.encode(payload)
    );
    let signature = signer.sign_es256(signing_input.as_bytes())?;
    verify_signature(signing_input.as_bytes(), &signature, signer.verifying_key())?;
    Ok(format!(
        "{signing_input}.{}",
        URL_SAFE_NO_PAD.encode(signature)
    ))
}

fn adapter_id() -> Result<AdapterId, Ap2Error> {
    AdapterId::new(ADAPTER_ID).map_err(|error| Ap2Error::Gateway(error.into()))
}

fn digest(domain: &[u8], values: &[&[u8]]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain);
    for value in values {
        digest.update(u64::try_from(value.len()).unwrap_or(u64::MAX).to_be_bytes());
        digest.update(value);
    }
    digest.finalize().into()
}
