//! Receipt-gated EVM payout binding and re-binding journeys.

use std::fmt::{Display, Formatter};

use k256::ecdsa::{RecoveryId, Signature as EcdsaSignature, VerifyingKey};
use layerx_intents::{compile, CompiledIntent, EvmPayoutBinding, Intent, IntentKind};
use layerx_proof::receipt::{verify, AuthorizedBatch, ReceiptCheck};
use layerx_types::activity::Signature;
use layerx_types::ids::{Did, IdempotencyKey};
use layerx_types::intent::{EvmAddress, NetworkId};
use layerx_types::payload::ModuleRegistry;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use sha3::Keccak256;

use crate::audit::{
    AuditChain, AuditError, AuditEvent, IdentityEvent, NotificationChannel, NotificationClass,
    SecurityChangeKind, StepUpEvidence as AuditStepUpEvidence,
};
use crate::auth::{
    AccessDecision, AuthError, AuthorizationRequest, OperationClass, OperationDigest, Passkeys,
    StepUpEvidence,
};
use crate::custody::{KeyId, Operation as CustodyOperation};
use crate::journeys::{JourneyEngine, JourneyKind, JourneyLeg, JourneyPlan};
use crate::notify::JourneyId;
use crate::store::{EvidenceRef, PrincipalScope, RowKey, StoreError, Table};
use crate::trace::TraceId;
use layerx_agent_api::identity::{AgentDid, AuthorityRef};

const CORE_DID_DOMAIN: &[u8] = b"LXP/v1/did-id\0";
const CORE_BINDING_DOMAIN: &[u8] = b"LXP/v1/evm-payout-binding\0";
const REBIND_DOMAIN: &[u8] = b"layerx-human-wallet-rebind/v1\0";
const ACTIVE_KEY: &str = "wallet-binding-active";
const PENDING_KEY: &str = "wallet-binding-pending";
const GOVERNANCE_BINDING_OPERATION: u8 = 4;
const MAX_STATEMENT_TTL: u64 = 600;

/// Exact, human-readable ownership statement paired with the digest accepted by core.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BindingStatement {
    did: Vec<u8>,
    network_id: u32,
    address: [u8; 20],
    issued_at: u64,
    expires_at: u64,
    text: String,
    signing_digest: [u8; 32],
}

/// Server-issued replacement statement and its operation-bound step-up digest.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RebindAction {
    statement: BindingStatement,
    confirms: OperationDigest,
}

impl RebindAction {
    /// Exact statement the replacement wallet must sign.
    #[must_use]
    pub const fn statement(&self) -> &BindingStatement {
        &self.statement
    }

    /// Exact operation digest a fresh passkey ceremony must confirm.
    #[must_use]
    pub const fn confirms(&self) -> OperationDigest {
        self.confirms
    }
}

impl BindingStatement {
    /// DID named by the statement.
    #[must_use]
    pub fn did(&self) -> &[u8] {
        &self.did
    }

    /// Numeric protocol network named by the statement.
    #[must_use]
    pub const fn network_id(&self) -> u32 {
        self.network_id
    }

    /// Exact recovered EVM address expected by the journey.
    #[must_use]
    pub const fn address(&self) -> [u8; 20] {
        self.address
    }

    /// Full EIP-55 address rendered in the statement.
    #[must_use]
    pub fn checksummed_address(&self) -> String {
        checksum_address(self.address)
    }

    /// Exact statement presented to the wallet owner.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Core-compatible, domain-separated digest the EVM key signs.
    #[must_use]
    pub const fn signing_digest(&self) -> [u8; 32] {
        self.signing_digest
    }

    /// Expiry after which the ownership proof cannot be submitted.
    #[must_use]
    pub const fn expires_at(&self) -> u64 {
        self.expires_at
    }
}

/// Submission accepted by the real agent contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentSubmission {
    /// Stable agent-layer submission identifier.
    pub submission_id: [u8; 32],
    /// Activity identifier whose receipt must eventually verify.
    pub activity_id: [u8; 32],
}

/// Typed agent request. The canonical payload remains owned by `layerx-intents`.
pub struct BindingAgentRequest<'a> {
    /// Actor DID used to prepare the agent activity envelope.
    pub actor: &'a Did,
    /// Versioned domain intent.
    pub intent: &'a Intent,
    /// Canonical, registry-checked compiled intent.
    pub compiled: &'a CompiledIntent,
    /// Caller-selected idempotency identity.
    pub idempotency_key: IdempotencyKey,
    /// Signer recovered locally before the request crossed the boundary.
    pub recovered_signer: EvmAddress,
}

/// Agent-layer contract used by the journey. Implementations submit to the agent, never core.
pub trait AgentBindingContract {
    /// Submits one verified, compiled wallet-binding intent.
    ///
    /// # Errors
    ///
    /// Returns a closed failure when the agent did not accept this exact request.
    fn submit_binding(
        &mut self,
        request: BindingAgentRequest<'_>,
    ) -> Result<AgentSubmission, AgentBindingError>;
}

/// Closed agent submission failure states.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentBindingError {
    /// Agent is temporarily unavailable and no success was observed.
    Unavailable,
    /// Agent refused the exact request.
    Refused,
    /// Agent response violated the typed contract.
    ContractViolation,
}

/// Receipt plus independently sourced authorization. For binding operation 4,
/// core records the EVM address right-aligned in the signed receipt's `to` field.
pub struct AgentBindingReceipt {
    /// Agent submission this receipt resolves.
    pub submission_id: [u8; 32],
    /// Canonical sequencer-signed protocol receipt bytes.
    pub canonical_receipt: Vec<u8>,
    /// Independently obtained batch/sequencer authority facts.
    pub authorized_batch: AuthorizedBatch,
}

/// One active, receipt-backed payout binding.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ActiveBinding {
    address: [u8; 20],
    network_id: u32,
    activity_id: [u8; 32],
    receipt_digest: [u8; 32],
    activated_at: u64,
}

impl ActiveBinding {
    /// Active EVM address.
    #[must_use]
    pub const fn address(&self) -> [u8; 20] {
        self.address
    }

    /// Active protocol network.
    #[must_use]
    pub const fn network_id(&self) -> u32 {
        self.network_id
    }

    /// Activity proven by the activation receipt.
    #[must_use]
    pub const fn activity_id(&self) -> [u8; 32] {
        self.activity_id
    }

    /// Digest of the full receipt retained in history.
    #[must_use]
    pub const fn receipt_digest(&self) -> [u8; 32] {
        self.receipt_digest
    }
}

/// Full immutable receipt record for a past or current binding.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BindingHistory {
    /// Binding made active by the receipt.
    pub binding: ActiveBinding,
    /// Exact statement presented for the ownership signature.
    pub statement: String,
    /// Full canonical signed receipt, not a summary.
    pub canonical_receipt: Vec<u8>,
    /// Whether this activation replaced an earlier active binding.
    pub rebind: bool,
}

/// Actionable security notification emitted after a successful rebind.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SecurityNotification {
    /// Stable notification class for clients.
    pub class: String,
    /// Plain action-oriented copy.
    pub message: String,
    /// Settings route for reviewing the active binding.
    pub deep_link: String,
    /// Stable client copy key for the review action.
    pub action_copy_key: String,
    /// Receipt proving the change.
    pub receipt_digest: [u8; 32],
    /// Time the verified change became active.
    pub created_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct PendingBinding {
    did: Vec<u8>,
    network_id: u32,
    address: [u8; 20],
    statement: String,
    signing_digest: [u8; 32],
    signature: Vec<u8>,
    idempotency_key: [u8; 32],
    submission_id: [u8; 32],
    activity_id: [u8; 32],
    rebind: bool,
    step_up_digest: Option<[u8; 32]>,
}

/// Observable journey state. A pending rebind always carries the still-effective binding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BindingState {
    /// No receipt-backed binding exists.
    Unbound,
    /// Initial binding submitted, but not active.
    Binding { candidate: [u8; 20] },
    /// Exactly one receipt-backed binding is active.
    Active(ActiveBinding),
    /// Replacement is pending while the old binding remains effective.
    Rebinding {
        active: ActiveBinding,
        candidate: [u8; 20],
    },
}

/// Typed journey failure states; no verification failure is collapsed into success.
#[derive(Debug)]
pub enum BindingError {
    InvalidStatement(&'static str),
    ExpiredStatement,
    InvalidOwnershipSignature,
    SignerAddressMismatch,
    Intent,
    Compile,
    Authentication(AuthError),
    ReauthenticationRequired { intended_destination: String },
    Agent(AgentBindingError),
    InvalidAgentResponse,
    Store(StoreError),
    Audit(AuditError),
    NoPendingBinding,
    BindingAlreadyActive,
    RebindRequiresActiveBinding,
    PendingBindingExists,
    ReceiptVerification(ReceiptCheck),
    ReceiptSubmissionMismatch,
    ReceiptActivityMismatch,
    ReceiptOperationMismatch,
    ReceiptAddressMismatch,
    CorruptState,
}

impl Display for BindingError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidStatement(reason) => {
                write!(formatter, "invalid wallet statement: {reason}")
            }
            Self::ExpiredStatement => formatter.write_str("wallet statement expired"),
            Self::InvalidOwnershipSignature => {
                formatter.write_str("invalid EVM ownership signature")
            }
            Self::SignerAddressMismatch => {
                formatter.write_str("signature does not recover the stated address")
            }
            Self::Intent => formatter.write_str("wallet binding intent was refused"),
            Self::Compile => formatter.write_str("wallet binding intent compilation failed"),
            Self::Authentication(error) => {
                write!(formatter, "wallet rebind authorization failed: {error}")
            }
            Self::ReauthenticationRequired { .. } => {
                formatter.write_str("wallet rebind requires reauthentication")
            }
            Self::Agent(error) => write!(
                formatter,
                "agent wallet binding submission failed: {error:?}"
            ),
            Self::InvalidAgentResponse => {
                formatter.write_str("agent returned invalid binding identifiers")
            }
            Self::Store(error) => write!(formatter, "wallet binding store failed: {error}"),
            Self::Audit(error) => write!(formatter, "wallet binding audit failed: {error}"),
            Self::NoPendingBinding => {
                formatter.write_str("no wallet binding is awaiting a receipt")
            }
            Self::BindingAlreadyActive => {
                formatter.write_str("a wallet binding is already active; use rebind")
            }
            Self::RebindRequiresActiveBinding => {
                formatter.write_str("wallet rebind requires an active binding")
            }
            Self::PendingBindingExists => {
                formatter.write_str("another wallet binding is still pending")
            }
            Self::ReceiptVerification(check) => {
                write!(formatter, "wallet binding receipt failed at {check:?}")
            }
            Self::ReceiptSubmissionMismatch => {
                formatter.write_str("receipt resolves another agent submission")
            }
            Self::ReceiptActivityMismatch => formatter.write_str("receipt proves another activity"),
            Self::ReceiptOperationMismatch => {
                formatter.write_str("receipt is not a wallet binding operation")
            }
            Self::ReceiptAddressMismatch => {
                formatter.write_str("receipt-recorded address differs from the verified signer")
            }
            Self::CorruptState => formatter.write_str("stored wallet binding state is corrupt"),
        }
    }
}

impl std::error::Error for BindingError {}

impl From<StoreError> for BindingError {
    fn from(value: StoreError) -> Self {
        Self::Store(value)
    }
}

impl From<AuditError> for BindingError {
    fn from(value: AuditError) -> Self {
        Self::Audit(value)
    }
}

impl From<AuthError> for BindingError {
    fn from(value: AuthError) -> Self {
        Self::Authentication(value)
    }
}

/// Receipt-gated wallet binding service.
#[derive(Clone, Debug)]
pub struct BindingJourney {
    registry: ModuleRegistry,
}

impl BindingJourney {
    pub fn rebind_operation_digest_verified(
        receipt_digest: [u8; 32],
        active_address: [u8; 20],
        statement: &BindingStatement,
    ) -> OperationDigest {
        let mut hasher = Sha256::new();
        hasher.update(REBIND_DOMAIN);
        hasher.update(receipt_digest);
        hasher.update(active_address);
        hasher.update(statement.signing_digest);
        hasher.update(statement.address);
        hasher.update(statement.network_id.to_be_bytes());
        OperationDigest::new(hasher.finalize().into())
    }
    #[allow(clippy::too_many_arguments)]
    pub fn start_durable(
        &self,
        scope: &mut PrincipalScope<'_>,
        statement: &BindingStatement,
        ownership_signature: &[u8],
        idempotency_key: IdempotencyKey,
        actor: AgentDid,
        authority: AuthorityRef,
        account_sequence: u64,
        not_before: u64,
        not_after: u64,
        fee_limit: u128,
        rebind: bool,
        now: u64,
    ) -> Result<JourneyEngine, BindingError> {
        validate_statement(statement, now)?;
        let recovered = recover_address(statement.signing_digest, ownership_signature)?;
        if recovered != statement.address {
            return Err(BindingError::SignerAddressMismatch);
        }
        let did =
            Did::new(&statement.did).map_err(|_| BindingError::InvalidStatement("invalid DID"))?;
        let network = NetworkId::new(statement.network_id)
            .map_err(|_| BindingError::InvalidStatement("invalid network"))?;
        let signature = Signature::new(ownership_signature).map_err(|_| BindingError::Intent)?;
        let binding = EvmPayoutBinding::new(did, network, EvmAddress::new(recovered), signature)
            .map_err(|_| BindingError::Intent)?;
        let intent = Intent::v1(IntentKind::EvmPayoutBinding(binding));
        let action_key = idempotency_key.bytes();
        let journey_id = JourneyId::new(format!("jrn_{}", hex(&action_key)))
            .map_err(|_| BindingError::CorruptState)?;
        let leg = JourneyLeg::new(
            intent,
            action_key,
            actor,
            authority,
            account_sequence,
            not_before,
            not_after,
            fee_limit,
        )
        .map_err(|_| BindingError::InvalidAgentResponse)?;
        let plan = JourneyPlan::new(
            journey_id,
            JourneyKind::WalletBinding,
            action_key,
            KeyId::new("human-primary").map_err(|_| BindingError::CorruptState)?,
            if rebind {
                CustodyOperation::WalletRebinding
            } else {
                CustodyOperation::ProtocolMutation
            },
            vec![leg],
        )
        .map_err(|_| BindingError::InvalidAgentResponse)?;
        JourneyEngine::start(scope, &plan, &self.registry, now)
            .map_err(|_| BindingError::Agent(AgentBindingError::Refused))
    }
    /// Creates a journey against the governance registry negotiated through the agent boundary.
    #[must_use]
    pub const fn new(registry: ModuleRegistry) -> Self {
        Self { registry }
    }

    /// Creates the exact display statement and core-compatible signing digest.
    ///
    /// # Errors
    ///
    /// Refuses invalid lifetimes, non-displayable DIDs, and timestamp overflow.
    pub fn issue_statement(
        did: &Did,
        network: NetworkId,
        address: EvmAddress,
        issued_at: u64,
        ttl: u64,
    ) -> Result<BindingStatement, BindingError> {
        if ttl == 0 || ttl > MAX_STATEMENT_TTL {
            return Err(BindingError::InvalidStatement("invalid lifetime"));
        }
        let did_text = std::str::from_utf8(did.as_bytes())
            .map_err(|_| BindingError::InvalidStatement("DID is not UTF-8"))?;
        let expires_at = issued_at
            .checked_add(ttl)
            .ok_or(BindingError::InvalidStatement("expiry overflow"))?;
        let address_bytes = address.bytes();
        let text = format!(
            "layerx-wallet-binding-v1\nDID: {did_text}\nnetwork_id: {}\naddress: {}\nissued_at: {issued_at}\nexpires_at: {expires_at}\nThis links a payout address only; it grants no authority to move funds.",
            network.value(), checksum_address(address_bytes)
        );
        Ok(BindingStatement {
            did: did.as_bytes().to_vec(),
            network_id: network.value(),
            address: address_bytes,
            issued_at,
            expires_at,
            text,
            signing_digest: core_binding_digest(did.as_bytes(), network.value()),
        })
    }

    /// Returns the exact operation digest a fresh passkey ceremony must confirm for rebind.
    #[must_use]
    pub fn rebind_operation_digest(
        active: &ActiveBinding,
        statement: &BindingStatement,
    ) -> OperationDigest {
        let mut hasher = Sha256::new();
        hasher.update(REBIND_DOMAIN);
        hasher.update(active.receipt_digest);
        hasher.update(active.address);
        hasher.update(statement.signing_digest);
        hasher.update(statement.address);
        hasher.update(statement.network_id.to_be_bytes());
        OperationDigest::new(hasher.finalize().into())
    }

    /// Issues one replacement statement together with its server-derived step-up digest.
    ///
    /// # Errors
    ///
    /// Refuses absent active state, pending binding work, and invalid statement inputs.
    #[allow(clippy::too_many_arguments, clippy::unused_self)]
    pub fn issue_rebind_action(
        &self,
        scope: &PrincipalScope<'_>,
        did: &Did,
        network: NetworkId,
        address: EvmAddress,
        issued_at: u64,
        ttl: u64,
    ) -> Result<RebindAction, BindingError> {
        let current = active(scope)?.ok_or(BindingError::RebindRequiresActiveBinding)?;
        if pending(scope)?.is_some() {
            return Err(BindingError::PendingBindingExists);
        }
        let statement = Self::issue_statement(did, network, address, issued_at, ttl)?;
        let confirms = Self::rebind_operation_digest(&current, &statement);
        Ok(RebindAction {
            statement,
            confirms,
        })
    }

    /// Submits an initial binding only after local EVM ownership verification.
    ///
    /// # Errors
    ///
    /// Refuses active/pending conflicts, invalid ownership, compilation, agent, and store failures.
    pub fn submit_initial<A: AgentBindingContract>(
        &self,
        scope: &mut PrincipalScope<'_>,
        statement: &BindingStatement,
        ownership_signature: &[u8],
        idempotency_key: IdempotencyKey,
        agent: &mut A,
        now: u64,
    ) -> Result<AgentSubmission, BindingError> {
        if active(scope)?.is_some() {
            return Err(BindingError::BindingAlreadyActive);
        }
        self.submit(
            scope,
            statement,
            ownership_signature,
            idempotency_key,
            agent,
            now,
            false,
            None,
        )
    }

    /// Submits a replacement after a real, operation-bound passkey step-up.
    ///
    /// # Errors
    ///
    /// Refuses absent active state, failed authentication or step-up, invalid ownership,
    /// compilation, agent, and store failures.
    #[allow(clippy::too_many_arguments)]
    pub fn submit_rebind<A: AgentBindingContract>(
        &self,
        scope: &mut PrincipalScope<'_>,
        passkeys: &Passkeys,
        access_token: &str,
        csrf_token: &str,
        step_up: &StepUpEvidence,
        statement: &BindingStatement,
        ownership_signature: &[u8],
        idempotency_key: IdempotencyKey,
        agent: &mut A,
        now: u64,
    ) -> Result<AgentSubmission, BindingError> {
        let current = active(scope)?.ok_or(BindingError::RebindRequiresActiveBinding)?;
        let digest = Self::rebind_operation_digest(&current, statement);
        let request = AuthorizationRequest {
            operation: OperationClass::WalletRebind,
            digest: Some(digest),
            step_up: Some(step_up),
            intended_destination: "/app/settings/wallet",
        };
        match passkeys.authorize(scope, access_token, Some(csrf_token), &request, now)? {
            AccessDecision::Authorized(_) => self.submit(
                scope,
                statement,
                ownership_signature,
                idempotency_key,
                agent,
                now,
                true,
                Some(step_up.confirms().bytes()),
            ),
            AccessDecision::Reauthenticate {
                intended_destination,
            } => Err(BindingError::ReauthenticationRequired {
                intended_destination,
            }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn submit<A: AgentBindingContract>(
        &self,
        scope: &mut PrincipalScope<'_>,
        statement: &BindingStatement,
        ownership_signature: &[u8],
        idempotency_key: IdempotencyKey,
        agent: &mut A,
        now: u64,
        rebind: bool,
        step_up_digest: Option<[u8; 32]>,
    ) -> Result<AgentSubmission, BindingError> {
        validate_statement(statement, now)?;
        let recovered = recover_address(statement.signing_digest, ownership_signature)?;
        if recovered != statement.address {
            return Err(BindingError::SignerAddressMismatch);
        }
        if let Some(existing) = pending(scope)? {
            if existing.idempotency_key == idempotency_key.bytes()
                && existing.signing_digest == statement.signing_digest
                && existing.address == statement.address
                && existing.rebind == rebind
            {
                return Ok(AgentSubmission {
                    submission_id: existing.submission_id,
                    activity_id: existing.activity_id,
                });
            }
            return Err(BindingError::PendingBindingExists);
        }
        let did =
            Did::new(&statement.did).map_err(|_| BindingError::InvalidStatement("invalid DID"))?;
        let network = NetworkId::new(statement.network_id)
            .map_err(|_| BindingError::InvalidStatement("invalid network"))?;
        let signature = Signature::new(ownership_signature).map_err(|_| BindingError::Intent)?;
        let binding =
            EvmPayoutBinding::new(did.clone(), network, EvmAddress::new(recovered), signature)
                .map_err(|_| BindingError::Intent)?;
        let intent = Intent::v1(IntentKind::EvmPayoutBinding(binding));
        let compiled = compile(&intent, &self.registry).map_err(|_| BindingError::Compile)?;
        let submission = agent
            .submit_binding(BindingAgentRequest {
                actor: &did,
                intent: &intent,
                compiled: &compiled,
                idempotency_key,
                recovered_signer: EvmAddress::new(recovered),
            })
            .map_err(BindingError::Agent)?;
        if submission.submission_id == [0; 32] || submission.activity_id == [0; 32] {
            return Err(BindingError::InvalidAgentResponse);
        }
        put_json(
            scope,
            Table::Journeys,
            row_key(PENDING_KEY)?,
            now,
            &PendingBinding {
                did: statement.did.clone(),
                network_id: statement.network_id,
                address: statement.address,
                statement: statement.text.clone(),
                signing_digest: statement.signing_digest,
                signature: ownership_signature.to_vec(),
                idempotency_key: idempotency_key.bytes(),
                submission_id: submission.submission_id,
                activity_id: submission.activity_id,
                rebind,
                step_up_digest,
            },
        )?;
        Ok(submission)
    }

    /// Activates a pending binding only after all receipt and signer-address checks pass.
    ///
    /// # Errors
    ///
    /// Refuses every receipt, activity, operation, signer-address, persistence, and audit failure.
    #[allow(clippy::too_many_lines, clippy::unused_self)]
    pub fn finalize(
        &self,
        scope: &mut PrincipalScope<'_>,
        receipt: &AgentBindingReceipt,
        now: u64,
        trace: &TraceId,
    ) -> Result<ActiveBinding, BindingError> {
        let pending = pending(scope)?.ok_or(BindingError::NoPendingBinding)?;
        if receipt.submission_id != pending.submission_id {
            return Err(BindingError::ReceiptSubmissionMismatch);
        }
        let verified = verify(&receipt.canonical_receipt, &receipt.authorized_batch)
            .map_err(|failure| BindingError::ReceiptVerification(failure.check))?;
        let protocol = verified
            .receipt()
            .protocol()
            .ok_or(BindingError::CorruptState)?;
        if protocol.activity_id() != pending.activity_id {
            return Err(BindingError::ReceiptActivityMismatch);
        }
        if protocol.operation() != GOVERNANCE_BINDING_OPERATION {
            return Err(BindingError::ReceiptOperationMismatch);
        }
        let receipt_destination = protocol.to();
        if receipt_destination[..12] != [0; 12] || receipt_destination[12..] != pending.address {
            return Err(BindingError::ReceiptAddressMismatch);
        }
        let receipt_digest = verified
            .evidence()
            .receipt_digest()
            .ok_or(BindingError::CorruptState)?;
        let candidate_binding = ActiveBinding {
            address: pending.address,
            network_id: pending.network_id,
            activity_id: pending.activity_id,
            receipt_digest,
            activated_at: now,
        };
        let candidate_history = BindingHistory {
            binding: candidate_binding.clone(),
            statement: pending.statement.clone(),
            canonical_receipt: verified.canonical_bytes().to_vec(),
            rebind: pending.rebind,
        };
        let history_key = history_key(receipt_digest)?;
        let history = match get_json::<BindingHistory>(scope, Table::Journeys, &history_key)? {
            Some(existing)
                if existing.binding.address == candidate_binding.address
                    && existing.binding.network_id == candidate_binding.network_id
                    && existing.binding.activity_id == candidate_binding.activity_id
                    && existing.binding.receipt_digest == candidate_binding.receipt_digest
                    && existing.statement == candidate_history.statement
                    && existing.canonical_receipt == candidate_history.canonical_receipt
                    && existing.rebind == candidate_history.rebind =>
            {
                existing
            }
            Some(_) => return Err(BindingError::CorruptState),
            None => {
                put_json(
                    scope,
                    Table::Journeys,
                    history_key.clone(),
                    now,
                    &candidate_history,
                )?;
                candidate_history
            }
        };
        let binding = history.binding.clone();

        let mut audit = AuditChain::open(scope)?;
        let prior_entries = audit.entries(scope)?;
        if !prior_entries.iter().any(|entry| {
            matches!(
                entry.event(),
                AuditEvent::IdentityLifecycle {
                    event: IdentityEvent::WalletBinding,
                    receipt_digest: existing,
                } if *existing == receipt_digest
            ) && entry.evidence().iter().any(|evidence| {
                evidence.table() == Table::Journeys && evidence.key() == &history_key
            })
        }) {
            audit.append(
                scope,
                now,
                trace,
                &AuditEvent::IdentityLifecycle {
                    event: IdentityEvent::WalletBinding,
                    receipt_digest,
                },
                &[EvidenceRef::new(Table::Journeys, history_key)],
            )?;
        }
        if pending.rebind {
            let ceremony_digest = pending.step_up_digest.ok_or(BindingError::CorruptState)?;
            if !prior_entries.iter().any(|entry| {
                matches!(
                    entry.event(),
                    AuditEvent::SecurityChange {
                        change: SecurityChangeKind::WalletRebinding,
                        step_up: AuditStepUpEvidence::Fresh {
                            ceremony_digest: existing,
                        },
                    } if *existing == ceremony_digest
                )
            }) {
                audit.append(
                    scope,
                    now,
                    trace,
                    &AuditEvent::SecurityChange {
                        change: SecurityChangeKind::WalletRebinding,
                        step_up: AuditStepUpEvidence::Fresh { ceremony_digest },
                    },
                    &[],
                )?;
            }
            let notification_key = notification_key(receipt_digest)?;
            let notification = SecurityNotification {
                class: "security".to_owned(),
                message: format!(
                    "Your payout wallet is now {}. Review this change if you did not make it.",
                    checksum_address(pending.address)
                ),
                deep_link: "/app/settings/wallet".to_owned(),
                action_copy_key: "notification.action.review-wallet".to_owned(),
                receipt_digest,
                created_at: binding.activated_at,
            };
            match get_json::<SecurityNotification>(scope, Table::Notifications, &notification_key)?
            {
                Some(existing) if existing == notification => {}
                Some(_) => return Err(BindingError::CorruptState),
                None => put_json(
                    scope,
                    Table::Notifications,
                    notification_key.clone(),
                    now,
                    &notification,
                )?,
            }
            if !prior_entries.iter().any(|entry| {
                matches!(
                    entry.event(),
                    AuditEvent::NotificationDispatch {
                        class: NotificationClass::Security,
                        channel: NotificationChannel::InApp,
                    }
                ) && entry.evidence().iter().any(|evidence| {
                    evidence.table() == Table::Notifications && evidence.key() == &notification_key
                })
            }) {
                audit.append(
                    scope,
                    now,
                    trace,
                    &AuditEvent::NotificationDispatch {
                        class: NotificationClass::Security,
                        channel: NotificationChannel::InApp,
                    },
                    &[EvidenceRef::new(Table::Notifications, notification_key)],
                )?;
            }
        }

        // This is deliberately last: no unverified or incompletely audited binding becomes active.
        put_json(scope, Table::Cache, row_key(ACTIVE_KEY)?, now, &binding)?;
        scope.remove(Table::Journeys, &row_key(PENDING_KEY)?)?;
        Ok(binding)
    }

    /// Reads the state while preserving the old active value during a rebind.
    ///
    /// # Errors
    ///
    /// Returns a typed error for unreadable or contradictory durable state.
    #[allow(clippy::unused_self)]
    pub fn state(&self, scope: &PrincipalScope<'_>) -> Result<BindingState, BindingError> {
        let active = active(scope)?;
        let pending = pending(scope)?;
        match (active, pending) {
            (None, None) => Ok(BindingState::Unbound),
            (None, Some(candidate)) if !candidate.rebind => Ok(BindingState::Binding {
                candidate: candidate.address,
            }),
            (Some(active), None) => Ok(BindingState::Active(active)),
            (Some(active), Some(candidate)) if candidate.rebind => Ok(BindingState::Rebinding {
                active,
                candidate: candidate.address,
            }),
            _ => Err(BindingError::CorruptState),
        }
    }

    /// Returns every receipt-backed activation in deterministic receipt-key order.
    ///
    /// # Errors
    ///
    /// Returns a typed error when any retained history row is corrupt.
    #[allow(clippy::unused_self)]
    pub fn history(&self, scope: &PrincipalScope<'_>) -> Result<Vec<BindingHistory>, BindingError> {
        scope
            .keys(Table::Journeys)
            .into_iter()
            .filter(|key| key.as_str().starts_with("wallet-binding-history-"))
            .map(|key| get_json(scope, Table::Journeys, &key)?.ok_or(BindingError::CorruptState))
            .collect()
    }

    /// Returns all actionable wallet-rebind security notifications.
    ///
    /// # Errors
    ///
    /// Returns a typed error when any retained notification row is corrupt.
    #[allow(clippy::unused_self)]
    pub fn security_notifications(
        &self,
        scope: &PrincipalScope<'_>,
    ) -> Result<Vec<SecurityNotification>, BindingError> {
        scope
            .keys(Table::Notifications)
            .into_iter()
            .filter(|key| key.as_str().starts_with("wallet-binding-security-"))
            .map(|key| {
                get_json(scope, Table::Notifications, &key)?.ok_or(BindingError::CorruptState)
            })
            .collect()
    }
}

fn validate_statement(statement: &BindingStatement, now: u64) -> Result<(), BindingError> {
    if now < statement.issued_at || now > statement.expires_at {
        return Err(BindingError::ExpiredStatement);
    }
    let did =
        Did::new(&statement.did).map_err(|_| BindingError::InvalidStatement("invalid DID"))?;
    let network = NetworkId::new(statement.network_id)
        .map_err(|_| BindingError::InvalidStatement("invalid network"))?;
    let rebuilt = BindingJourney::issue_statement(
        &did,
        network,
        EvmAddress::new(statement.address),
        statement.issued_at,
        statement.expires_at.saturating_sub(statement.issued_at),
    )?;
    if rebuilt != *statement {
        return Err(BindingError::InvalidStatement(
            "statement changed after issue",
        ));
    }
    Ok(())
}

fn core_binding_digest(did: &[u8], network_id: u32) -> [u8; 32] {
    let mut did_hasher = Sha256::new();
    did_hasher.update(CORE_DID_DOMAIN);
    did_hasher.update(did);
    let did_id: [u8; 32] = did_hasher.finalize().into();
    let mut binding_hasher = Sha256::new();
    binding_hasher.update(CORE_BINDING_DOMAIN);
    binding_hasher.update(did_id);
    binding_hasher.update(network_id.to_be_bytes());
    binding_hasher.finalize().into()
}

fn recover_address(digest: [u8; 32], bytes: &[u8]) -> Result<[u8; 20], BindingError> {
    let compact: &[u8; 64] = bytes
        .get(..64)
        .and_then(|value| value.try_into().ok())
        .ok_or(BindingError::InvalidOwnershipSignature)?;
    if bytes.len() != 65 {
        return Err(BindingError::InvalidOwnershipSignature);
    }
    let signature =
        EcdsaSignature::from_slice(compact).map_err(|_| BindingError::InvalidOwnershipSignature)?;
    if signature.normalize_s().is_some() {
        return Err(BindingError::InvalidOwnershipSignature);
    }
    let recovery_byte = match bytes[64] {
        27 | 28 => bytes[64] - 27,
        value @ 0..=3 => value,
        _ => return Err(BindingError::InvalidOwnershipSignature),
    };
    let recovery =
        RecoveryId::from_byte(recovery_byte).ok_or(BindingError::InvalidOwnershipSignature)?;
    let key = VerifyingKey::recover_from_prehash(&digest, &signature, recovery)
        .map_err(|_| BindingError::InvalidOwnershipSignature)?;
    let point = key.to_encoded_point(false);
    let public = point
        .as_bytes()
        .get(1..)
        .ok_or(BindingError::InvalidOwnershipSignature)?;
    let hash = Keccak256::digest(public);
    let mut address = [0_u8; 20];
    address.copy_from_slice(&hash[12..]);
    Ok(address)
}

fn checksum_address(address: [u8; 20]) -> String {
    let lower = hex(&address);
    let digest = Keccak256::digest(lower.as_bytes());
    let mut output = String::with_capacity(42);
    output.push_str("0x");
    for (index, byte) in lower.bytes().enumerate() {
        let nibble = if index % 2 == 0 {
            digest[index / 2] >> 4
        } else {
            digest[index / 2] & 0x0f
        };
        if byte.is_ascii_alphabetic() && nibble >= 8 {
            output.push(char::from(byte.to_ascii_uppercase()));
        } else {
            output.push(char::from(byte));
        }
    }
    output
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

fn row_key(value: &str) -> Result<RowKey, BindingError> {
    Ok(RowKey::new(value)?)
}

fn history_key(digest: [u8; 32]) -> Result<RowKey, BindingError> {
    row_key(&format!("wallet-binding-history-{}", hex(&digest)))
}

fn notification_key(digest: [u8; 32]) -> Result<RowKey, BindingError> {
    row_key(&format!("wallet-binding-security-{}", hex(&digest)))
}

fn put_json<T: Serialize>(
    scope: &mut PrincipalScope<'_>,
    table: Table,
    key: RowKey,
    now: u64,
    value: &T,
) -> Result<(), BindingError> {
    let bytes = serde_json::to_vec(value).map_err(|_| BindingError::CorruptState)?;
    scope.put(table, key, now, bytes)?;
    Ok(())
}

fn get_json<T: for<'de> Deserialize<'de>>(
    scope: &PrincipalScope<'_>,
    table: Table,
    key: &RowKey,
) -> Result<Option<T>, BindingError> {
    scope
        .get(table, key)
        .map(|row| serde_json::from_slice(row.bytes()).map_err(|_| BindingError::CorruptState))
        .transpose()
}

fn active(scope: &PrincipalScope<'_>) -> Result<Option<ActiveBinding>, BindingError> {
    get_json(scope, Table::Cache, &row_key(ACTIVE_KEY)?)
}

fn pending(scope: &PrincipalScope<'_>) -> Result<Option<PendingBinding>, BindingError> {
    get_json(scope, Table::Journeys, &row_key(PENDING_KEY)?)
}
