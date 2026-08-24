#![forbid(unsafe_code)]

pub mod ethereum;
mod journal;
mod rpc;
pub mod solana;
mod source_codec;
#[cfg(test)]
mod tests;

pub use journal::JournalConfig;
pub use rpc::{RpcEndpointConfig, RpcQuorumConfig};

use std::fmt::{Debug, Display, Formatter};

use layerx_interop_gateway::adapter::{AdapterDescriptor, AdapterId, ConformanceSuite, PinnedSpec};
use layerx_interop_gateway::error::GatewayError;
use layerx_interop_gateway::gateway::{TranslationKind, TranslationRequest, TranslationStatus};
use layerx_interop_gateway::principal::PrincipalId;
use layerx_interop_gateway::trace::{TraceId, Traced};
use layerx_interop_gateway::GatewayCore;
use layerx_proof::receipt::{verify, AuthorizedBatch, VerifiedReceipt};
use sha2::{Digest as _, Sha256};

const ADAPTER_ID: &str = "migration";
const EVIDENCE_LIMIT: usize = 1024 * 1024;
const HISTORY_PAGE_LIMIT: usize = 256;
const REQUEST_DOMAIN: &[u8] = b"LayerX/interop/migration/request/v1\0";
const IDEMPOTENCY_DOMAIN: &[u8] = b"LayerX/interop/migration/idempotency/v1\0";
const CUSTODY_CONTEXT_DOMAIN: &[u8] = b"LayerX/interop/migration/custody-context/v1\0";
const BINDING_CONTEXT_DOMAIN: &[u8] = b"LayerX/interop/migration/binding-context/v1\0";

mod sealed {
    pub trait SourceVerifier {}
}

/// Source chains supported by the migration boundary. Each value commits to
/// the exact network, preventing an address or transaction from silently
/// crossing between networks.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SourceChain {
    Ethereum { chain_id: u64 },
    Solana { genesis_hash: [u8; 32] },
}

impl SourceChain {
    fn validate(self) -> Result<(), MigrationError> {
        match self {
            Self::Ethereum { chain_id } if chain_id != 0 => Ok(()),
            Self::Solana { genesis_hash } if genesis_hash != [0; 32] => Ok(()),
            Self::Ethereum { .. } | Self::Solana { .. } => Err(MigrationError::InvalidNetwork),
        }
    }

    fn commit(self, hash: &mut Sha256) {
        match self {
            Self::Ethereum { chain_id } => {
                hash.update([1]);
                hash.update(chain_id.to_be_bytes());
            }
            Self::Solana { genesis_hash } => {
                hash.update([2]);
                hash.update(genesis_hash);
            }
        }
    }
}

/// An address whose byte length is fixed by its source chain.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ExternalAddress {
    Ethereum([u8; 20]),
    Solana([u8; 32]),
}

impl ExternalAddress {
    fn validate_for(self, chain: SourceChain) -> Result<(), MigrationError> {
        match (chain, self) {
            (SourceChain::Ethereum { .. }, Self::Ethereum(address)) if address != [0; 20] => Ok(()),
            (SourceChain::Solana { .. }, Self::Solana(address)) if address != [0; 32] => Ok(()),
            (_, Self::Ethereum(address)) if address == [0; 20] => {
                Err(MigrationError::InvalidAddress)
            }
            (_, Self::Solana(address)) if address == [0; 32] => Err(MigrationError::InvalidAddress),
            (_, Self::Ethereum(_) | Self::Solana(_)) => Err(MigrationError::AddressChainMismatch),
        }
    }

    fn commit(self, hash: &mut Sha256) {
        match self {
            Self::Ethereum(address) => {
                hash.update([1]);
                hash.update(address);
            }
            Self::Solana(address) => {
                hash.update([2]);
                hash.update(address);
            }
        }
    }
}

/// Exact native source-chain transaction identifier.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum SourceTransaction {
    Ethereum([u8; 32]),
    Solana([u8; 64]),
}

impl SourceTransaction {
    /// Creates a non-zero Ethereum transaction hash.
    ///
    /// # Errors
    ///
    /// Refuses the reserved zero identifier.
    pub fn ethereum(value: [u8; 32]) -> Result<Self, MigrationError> {
        if value == [0; 32] {
            Err(MigrationError::InvalidTransaction)
        } else {
            Ok(Self::Ethereum(value))
        }
    }

    /// Creates a non-zero native Solana transaction signature.
    ///
    /// # Errors
    ///
    /// Refuses the reserved zero signature.
    pub fn solana(value: [u8; 64]) -> Result<Self, MigrationError> {
        if value == [0; 64] {
            Err(MigrationError::InvalidTransaction)
        } else {
            Ok(Self::Solana(value))
        }
    }

    #[must_use]
    pub const fn bytes(&self) -> &[u8] {
        match self {
            Self::Ethereum(value) => value,
            Self::Solana(value) => value,
        }
    }

    fn validate_for(self, chain: SourceChain) -> Result<(), MigrationError> {
        if matches!(
            (chain, self),
            (SourceChain::Ethereum { .. }, Self::Ethereum(_))
                | (SourceChain::Solana { .. }, Self::Solana(_))
        ) {
            Ok(())
        } else {
            Err(MigrationError::InvalidTransaction)
        }
    }

    fn commit(self, hash: &mut Sha256) {
        match self {
            Self::Ethereum(value) => {
                hash.update([1]);
                hash.update(value);
            }
            Self::Solana(value) => {
                hash.update([2]);
                hash.update(value);
            }
        }
    }
}

/// Bounded source evidence. Raw bytes never enter debug, display, telemetry,
/// or gateway audit records; those boundaries receive only this digest.
pub struct SourceEvidence {
    canonical: Vec<u8>,
    digest: [u8; 32],
}

impl SourceEvidence {
    /// Commits to one canonical source-chain proof envelope.
    ///
    /// # Errors
    ///
    /// Refuses empty and oversized evidence.
    pub fn new(canonical: Vec<u8>) -> Result<Self, MigrationError> {
        if canonical.is_empty() || canonical.len() > EVIDENCE_LIMIT {
            return Err(MigrationError::InvalidEvidence);
        }
        let digest = Sha256::digest(&canonical).into();
        Ok(Self { canonical, digest })
    }

    #[must_use]
    pub fn canonical(&self) -> &[u8] {
        &self.canonical
    }

    #[must_use]
    pub const fn digest(&self) -> [u8; 32] {
        self.digest
    }
}

impl Debug for SourceEvidence {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SourceEvidence")
            .field("digest", &self.digest)
            .field("canonical", &"[REDACTED]")
            .finish()
    }
}

/// Source ownership facts after chain-specific signature verification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifiedOwnership {
    chain: SourceChain,
    address: ExternalAddress,
    layerx_identity: [u8; 32],
    evidence_digest: [u8; 32],
}

impl VerifiedOwnership {
    #[must_use]
    pub const fn chain(&self) -> SourceChain {
        self.chain
    }

    #[must_use]
    pub const fn address(&self) -> ExternalAddress {
        self.address
    }

    #[must_use]
    pub const fn layerx_identity(&self) -> [u8; 32] {
        self.layerx_identity
    }

    #[must_use]
    pub const fn evidence_digest(&self) -> [u8; 32] {
        self.evidence_digest
    }

    fn validate(self, evidence: &SourceEvidence) -> Result<(), MigrationError> {
        self.chain.validate()?;
        self.address.validate_for(self.chain)?;
        if self.layerx_identity == [0; 32] || self.evidence_digest != evidence.digest() {
            return Err(MigrationError::EvidenceMismatch);
        }
        Ok(())
    }
}

/// Final source-chain custody facts. Construction is delegated to a real
/// chain verifier; the adapter rechecks all closed invariants before credit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifiedAssetFinality {
    chain: SourceChain,
    transaction: SourceTransaction,
    source: ExternalAddress,
    source_asset: [u8; 32],
    source_amount: u128,
    custody_reference: [u8; 32],
    layerx_asset: [u8; 32],
    layerx_amount: u128,
    destination: [u8; 32],
    finality_height: u64,
    evidence_digest: [u8; 32],
}

impl VerifiedAssetFinality {
    #[must_use]
    pub const fn chain(&self) -> SourceChain {
        self.chain
    }

    #[must_use]
    pub const fn transaction(&self) -> SourceTransaction {
        self.transaction
    }

    #[must_use]
    pub const fn source(&self) -> ExternalAddress {
        self.source
    }

    #[must_use]
    pub const fn source_asset(&self) -> [u8; 32] {
        self.source_asset
    }

    #[must_use]
    pub const fn source_amount(&self) -> u128 {
        self.source_amount
    }

    #[must_use]
    pub const fn custody_reference(&self) -> [u8; 32] {
        self.custody_reference
    }

    #[must_use]
    pub const fn layerx_asset(&self) -> [u8; 32] {
        self.layerx_asset
    }

    #[must_use]
    pub const fn layerx_amount(&self) -> u128 {
        self.layerx_amount
    }

    #[must_use]
    pub const fn destination(&self) -> [u8; 32] {
        self.destination
    }

    #[must_use]
    pub const fn finality_height(&self) -> u64 {
        self.finality_height
    }

    #[must_use]
    pub const fn evidence_digest(&self) -> [u8; 32] {
        self.evidence_digest
    }

    fn validate(self, evidence: &SourceEvidence) -> Result<(), MigrationError> {
        self.chain.validate()?;
        self.transaction.validate_for(self.chain)?;
        self.source.validate_for(self.chain)?;
        if self.source_asset == [0; 32]
            || self.source_amount == 0
            || self.custody_reference == [0; 32]
            || self.layerx_asset == [0; 32]
            || self.layerx_amount == 0
            || self.destination == [0; 32]
            || self.finality_height == 0
            || self.evidence_digest != evidence.digest()
        {
            return Err(MigrationError::EvidenceMismatch);
        }
        Ok(())
    }
}

/// A chain-specific verifier is the only boundary allowed to turn canonical
/// source evidence into ownership, finality, or history facts.
pub trait SourceVerifier: sealed::SourceVerifier {
    /// Verifies address ownership under the exact requested network.
    ///
    /// # Errors
    ///
    /// Returns a closed source verification refusal.
    fn verify_ownership(
        &self,
        evidence: &SourceEvidence,
        trace: &TraceId,
    ) -> Result<VerifiedOwnership, MigrationError>;

    /// Verifies source inclusion, execution, custody lock, and finality.
    ///
    /// # Errors
    ///
    /// Returns pending while finality is not established, and a terminal
    /// refusal for displaced, reverted, malformed, or mismatched evidence.
    fn verify_asset_finality(
        &self,
        evidence: &SourceEvidence,
        trace: &TraceId,
    ) -> Result<VerifiedAssetFinality, MigrationError>;

    /// Verifies a bounded page of source history.
    ///
    /// # Errors
    ///
    /// Refuses unauthenticated, unbounded, or internally inconsistent pages.
    fn verify_history(
        &self,
        evidence: &SourceEvidence,
        trace: &TraceId,
    ) -> Result<VerifiedHistoryPage, MigrationError>;

    /// Commits the verified page's authenticated cursor only after the caller's
    /// external-provenance sink durably accepted that exact page.
    ///
    /// # Errors
    ///
    /// Refuses cursor skips, replay conflicts, or checkpoint corruption.
    fn commit_history(
        &self,
        evidence: &SourceEvidence,
        page: &VerifiedHistoryPage,
        trace: &TraceId,
    ) -> Result<(), MigrationError>;
}

/// The source-chain meaning of one imported transaction.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ExternalHistoryKind {
    Incoming,
    Outgoing,
    Contract,
}

/// An imported history record. Its provenance is structurally external and
/// the type contains no field capable of carrying a `LayerX` receipt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExternalHistoryRecord {
    chain: SourceChain,
    transaction: SourceTransaction,
    address: ExternalAddress,
    kind: ExternalHistoryKind,
    timestamp: u64,
    source_asset: [u8; 32],
    source_amount: u128,
    provenance: ExternalProvenance,
}

/// Closed provenance label used by every imported record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalProvenance {
    Ethereum,
    Solana,
}

impl ExternalHistoryRecord {
    #[must_use]
    pub const fn chain(&self) -> SourceChain {
        self.chain
    }

    #[must_use]
    pub const fn transaction(&self) -> SourceTransaction {
        self.transaction
    }

    #[must_use]
    pub const fn address(&self) -> ExternalAddress {
        self.address
    }

    #[must_use]
    pub const fn kind(&self) -> ExternalHistoryKind {
        self.kind
    }

    #[must_use]
    pub const fn timestamp(&self) -> u64 {
        self.timestamp
    }

    #[must_use]
    pub const fn source_asset(&self) -> [u8; 32] {
        self.source_asset
    }

    #[must_use]
    pub const fn source_amount(&self) -> u128 {
        self.source_amount
    }

    #[must_use]
    pub const fn provenance(&self) -> ExternalProvenance {
        self.provenance
    }

    fn validate(self) -> Result<(), MigrationError> {
        self.chain.validate()?;
        self.transaction.validate_for(self.chain)?;
        self.address.validate_for(self.chain)?;
        let provenance_matches = matches!(
            (self.chain, self.provenance),
            (SourceChain::Ethereum { .. }, ExternalProvenance::Ethereum)
                | (SourceChain::Solana { .. }, ExternalProvenance::Solana)
        );
        if !provenance_matches
            || self.timestamp == 0
            || self.source_asset == [0; 32]
            || (self.source_amount == 0 && self.kind != ExternalHistoryKind::Contract)
        {
            return Err(MigrationError::InvalidHistory);
        }
        Ok(())
    }
}

/// One independently verified, bounded history page.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedHistoryPage {
    records: Vec<ExternalHistoryRecord>,
    next_cursor: Option<[u8; 32]>,
    evidence_digest: [u8; 32],
}

impl VerifiedHistoryPage {
    #[must_use]
    pub fn records(&self) -> &[ExternalHistoryRecord] {
        &self.records
    }

    #[must_use]
    pub const fn next_cursor(&self) -> Option<[u8; 32]> {
        self.next_cursor
    }

    #[must_use]
    pub const fn evidence_digest(&self) -> [u8; 32] {
        self.evidence_digest
    }

    fn validate(&self, evidence: &SourceEvidence) -> Result<(), MigrationError> {
        if self.records.len() > HISTORY_PAGE_LIMIT || self.evidence_digest != evidence.digest() {
            return Err(MigrationError::InvalidHistory);
        }
        for record in &self.records {
            record.validate()?;
        }
        Ok(())
    }
}

/// Storage boundary that accepts only the external-provenance record type.
pub trait ExternalHistorySink {
    /// Persists a verified external history page under the caller principal.
    /// Implementations deduplicate records by chain, transaction, address,
    /// asset, and kind so multi-asset transactions and overlapping pages stay
    /// distinct and idempotent.
    ///
    /// # Errors
    ///
    /// Returns a typed refusal without relabelling any record as protocol
    /// activity or a receipt.
    fn store_external(
        &mut self,
        principal: &PrincipalId,
        page: &VerifiedHistoryPage,
        trace: &TraceId,
    ) -> Result<(), MigrationError>;
}

/// Adapter-created request for the deployed account-binding authority. Its
/// private fields prevent callers from bypassing verification or substituting
/// an idempotency key when invoking a production plane directly.
pub struct BindingExecution<'a> {
    ownership: &'a VerifiedOwnership,
    idempotency_key: [u8; 32],
}

impl BindingExecution<'_> {
    #[must_use]
    pub const fn ownership(&self) -> &VerifiedOwnership {
        self.ownership
    }

    #[must_use]
    pub const fn idempotency_key(&self) -> [u8; 32] {
        self.idempotency_key
    }
}

/// Adapter-created request for the deployed custody-credit authority. Only
/// the migration adapter can construct this request after source verification.
pub struct CustodyExecution<'a> {
    finality: &'a VerifiedAssetFinality,
    idempotency_key: [u8; 32],
}

impl CustodyExecution<'_> {
    #[must_use]
    pub const fn finality(&self) -> &VerifiedAssetFinality {
        self.finality
    }

    #[must_use]
    pub const fn idempotency_key(&self) -> [u8; 32] {
        self.idempotency_key
    }
}

/// Real plane outcome. An executed operation must carry canonical receipt
/// bytes and independently obtained batch authority evidence.
#[derive(Debug)]
pub enum MigrationPlaneResult {
    Pending,
    Refused,
    Executed {
        canonical_receipt: Vec<u8>,
        authorised_batch: AuthorizedBatch,
    },
}

/// Existing protocol authority boundary for binding and custody credit.
pub trait MigrationPlane {
    /// Executes an adapter-authorized account binding.
    ///
    /// # Errors
    ///
    /// Returns a typed plane refusal and never invents a receipt.
    fn bind_account(
        &mut self,
        request: &BindingExecution<'_>,
        trace: &TraceId,
    ) -> Result<MigrationPlaneResult, MigrationError>;

    /// Executes an adapter-authorized custody credit.
    ///
    /// # Errors
    ///
    /// Returns a typed plane refusal and durably deduplicates the request's
    /// canonical source claim independently of transport retries.
    fn credit_custody(
        &mut self,
        request: &CustodyExecution<'_>,
        trace: &TraceId,
    ) -> Result<MigrationPlaneResult, MigrationError>;
}

/// Exact protocol receipt policy for external-address bindings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BindingReceiptPolicy {
    sequencer_public_key: [u8; 32],
    authority: [u8; 32],
    module_id: u16,
    module_version: u32,
    parameter_version: u32,
    operation: u8,
    asset: [u8; 32],
    amount: u128,
}

impl BindingReceiptPolicy {
    /// Creates a fail-closed policy for one deployed binding operation.
    ///
    /// # Errors
    ///
    /// Refuses reserved authority, operation, asset, or amount values.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        sequencer_public_key: [u8; 32],
        authority: [u8; 32],
        module_id: u16,
        module_version: u32,
        parameter_version: u32,
        operation: u8,
        asset: [u8; 32],
        amount: u128,
    ) -> Result<Self, MigrationError> {
        if sequencer_public_key == [0; 32]
            || authority == [0; 32]
            || module_id == 0
            || module_version == 0
            || operation == 0
            || asset == [0; 32]
            || amount == 0
        {
            return Err(MigrationError::Configuration);
        }
        Ok(Self {
            sequencer_public_key,
            authority,
            module_id,
            module_version,
            parameter_version,
            operation,
            asset,
            amount,
        })
    }

    /// Returns the exact external-claim commitment carried by the binding
    /// operation's protocol receipt.
    #[must_use]
    pub fn context_hash(&self, ownership: &VerifiedOwnership) -> [u8; 32] {
        binding_context_hash(ownership)
    }

    const fn sequencer_public_key(&self) -> [u8; 32] {
        self.sequencer_public_key
    }

    fn verify(
        &self,
        ownership: &VerifiedOwnership,
        receipt: &VerifiedReceipt,
    ) -> Result<(), MigrationError> {
        let protocol = receipt
            .receipt()
            .protocol()
            .ok_or(MigrationError::ReceiptMismatch)?;
        if protocol.result_code() != 0
            || protocol.authorization_hash() == [0; 32]
            || protocol.from() != self.authority
            || protocol.module_id() != self.module_id
            || protocol.module_version() != self.module_version
            || protocol.parameter_version() != self.parameter_version
            || protocol.operation() != self.operation
            || protocol.asset() != self.asset
            || protocol.amount() != self.amount
            || protocol.to() != ownership.layerx_identity
            || protocol.context_hash() != self.context_hash(ownership)
            || protocol
                .debit_balance_before()
                .checked_sub(protocol.debit_balance_after())
                != Some(self.amount)
            || protocol
                .credit_balance_after()
                .checked_sub(protocol.credit_balance_before())
                != Some(self.amount)
        {
            return Err(MigrationError::ReceiptMismatch);
        }
        Ok(())
    }
}

/// Exact protocol receipt policy for custody credits. The authority and
/// operation coordinates come from deployment configuration, while the
/// context commitment is derived from the independently verified source fact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CustodyReceiptPolicy {
    sequencer_public_key: [u8; 32],
    custody_authority: [u8; 32],
    module_id: u16,
    module_version: u32,
    parameter_version: u32,
    operation: u8,
}

impl CustodyReceiptPolicy {
    /// Creates a fail-closed receipt policy for one deployed custody operation.
    ///
    /// # Errors
    ///
    /// Refuses reserved authority, module, or operation values.
    pub fn new(
        sequencer_public_key: [u8; 32],
        custody_authority: [u8; 32],
        module_id: u16,
        module_version: u32,
        parameter_version: u32,
        operation: u8,
    ) -> Result<Self, MigrationError> {
        if sequencer_public_key == [0; 32]
            || custody_authority == [0; 32]
            || module_id == 0
            || module_version == 0
            || operation == 0
        {
            return Err(MigrationError::Configuration);
        }
        Ok(Self {
            sequencer_public_key,
            custody_authority,
            module_id,
            module_version,
            parameter_version,
            operation,
        })
    }

    /// Returns the exact source-claim commitment that the custody operation
    /// must carry in the protocol receipt context.
    #[must_use]
    pub fn context_hash(&self, finality: &VerifiedAssetFinality) -> [u8; 32] {
        custody_context_hash(finality)
    }

    const fn sequencer_public_key(&self) -> [u8; 32] {
        self.sequencer_public_key
    }

    fn verify(
        &self,
        finality: &VerifiedAssetFinality,
        receipt: &VerifiedReceipt,
    ) -> Result<(), MigrationError> {
        let protocol = receipt
            .receipt()
            .protocol()
            .ok_or(MigrationError::ReceiptMismatch)?;
        if protocol.result_code() != 0
            || protocol.authorization_hash() == [0; 32]
            || protocol.from() != self.custody_authority
            || protocol.module_id() != self.module_id
            || protocol.module_version() != self.module_version
            || protocol.parameter_version() != self.parameter_version
            || protocol.operation() != self.operation
            || protocol.context_hash() != self.context_hash(finality)
            || protocol.asset() != finality.layerx_asset
            || protocol.amount() != finality.layerx_amount
            || protocol.to() != finality.destination
            || protocol
                .debit_balance_before()
                .checked_sub(protocol.debit_balance_after())
                != Some(finality.layerx_amount)
            || protocol
                .credit_balance_after()
                .checked_sub(protocol.credit_balance_before())
                != Some(finality.layerx_amount)
        {
            return Err(MigrationError::ReceiptMismatch);
        }
        Ok(())
    }
}

/// Honest adapter result. Pending source finality is a typed error; pending
/// here means the source was verified and the protocol operation is open.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationState {
    ProtocolPending,
    AccountMapped { receipt_digest: [u8; 32] },
    AssetCredited { receipt_digest: [u8; 32] },
    HistoryImported { record_count: usize },
    Refused,
}

/// Stateless migration orchestrator using the principal-scoped gateway for
/// idempotency, audit, trace, and receipt-gated terminal state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationAdapter;

impl MigrationAdapter {
    /// Verifies ownership and binds the external account through the protocol.
    ///
    /// # Errors
    ///
    /// Returns source, plane, binding-policy, gateway, or receipt refusals.
    #[allow(clippy::too_many_arguments)]
    pub fn map_account(
        gateway: &mut GatewayCore,
        principal: &PrincipalId,
        evidence: &SourceEvidence,
        verifier: &impl SourceVerifier,
        plane: &mut impl MigrationPlane,
        binding_policy: &BindingReceiptPolicy,
        trace: &TraceId,
        now: u64,
    ) -> Result<MigrationState, Traced<MigrationError>> {
        let fail = |error| trace.wrap(error);
        let ownership = verifier.verify_ownership(evidence, trace).map_err(fail)?;
        ownership.validate(evidence).map_err(fail)?;
        let key = ownership_key(&ownership);
        let request = translation_request(key, ownership_digest(&ownership), trace)?;
        let opened = begin(gateway, principal, &request, trace, now)?;
        match opened {
            TranslationStatus::ReceiptVerified { receipt_digest } => {
                return Ok(MigrationState::AccountMapped { receipt_digest });
            }
            TranslationStatus::Refused => return Ok(MigrationState::Refused),
            TranslationStatus::Pending => {}
            TranslationStatus::Translated => {
                return Err(fail(MigrationError::Gateway(GatewayError::Corrupt(
                    "state-changing migration has a read-only completion",
                ))));
            }
        }
        let execution = BindingExecution {
            ownership: &ownership,
            idempotency_key: key,
        };
        let outcome = plane.bind_account(&execution, trace).map_err(fail)?;
        settle(
            gateway,
            principal,
            key,
            outcome,
            binding_policy.sequencer_public_key(),
            trace,
            now,
            |receipt| binding_policy.verify(&ownership, receipt),
            |receipt_digest| MigrationState::AccountMapped { receipt_digest },
        )
    }

    /// Verifies source finality and credits only through the custody boundary.
    ///
    /// # Errors
    ///
    /// Returns pending until finality is proven and refuses every evidence or
    /// receipt mismatch without crediting value.
    #[allow(clippy::too_many_arguments)]
    pub fn migrate_asset(
        gateway: &mut GatewayCore,
        principal: &PrincipalId,
        evidence: &SourceEvidence,
        verifier: &impl SourceVerifier,
        plane: &mut impl MigrationPlane,
        receipt_policy: &CustodyReceiptPolicy,
        trace: &TraceId,
        now: u64,
    ) -> Result<MigrationState, Traced<MigrationError>> {
        let fail = |error| trace.wrap(error);
        let finality = verifier
            .verify_asset_finality(evidence, trace)
            .map_err(fail)?;
        finality.validate(evidence).map_err(fail)?;
        let key = asset_key(&finality);
        let request = translation_request(key, finality_digest(&finality), trace)?;
        let opened = begin(gateway, principal, &request, trace, now)?;
        match opened {
            TranslationStatus::ReceiptVerified { receipt_digest } => {
                return Ok(MigrationState::AssetCredited { receipt_digest });
            }
            TranslationStatus::Refused => return Ok(MigrationState::Refused),
            TranslationStatus::Pending => {}
            TranslationStatus::Translated => {
                return Err(fail(MigrationError::Gateway(GatewayError::Corrupt(
                    "state-changing migration has a read-only completion",
                ))));
            }
        }
        let execution = CustodyExecution {
            finality: &finality,
            idempotency_key: key,
        };
        let outcome = plane.credit_custody(&execution, trace).map_err(fail)?;
        settle(
            gateway,
            principal,
            key,
            outcome,
            receipt_policy.sequencer_public_key(),
            trace,
            now,
            |receipt| receipt_policy.verify(&finality, receipt),
            |receipt_digest| MigrationState::AssetCredited { receipt_digest },
        )
    }

    /// Imports a verified source history page as external provenance only.
    ///
    /// # Errors
    ///
    /// Refuses invalid evidence, storage failures, and conflicting reuse of a
    /// history page identity.
    #[allow(clippy::too_many_arguments)]
    pub fn import_history(
        gateway: &mut GatewayCore,
        principal: &PrincipalId,
        evidence: &SourceEvidence,
        verifier: &impl SourceVerifier,
        sink: &mut impl ExternalHistorySink,
        trace: &TraceId,
        now: u64,
    ) -> Result<MigrationState, Traced<MigrationError>> {
        let fail = |error| trace.wrap(error);
        let page = verifier.verify_history(evidence, trace).map_err(fail)?;
        page.validate(evidence).map_err(fail)?;
        let key = history_key(&page);
        let request = TranslationRequest::new(
            adapter_id().map_err(fail)?,
            TranslationKind::ReadOnly,
            key,
            history_digest(&page),
        )
        .map_err(|error| fail(MigrationError::Gateway(error)))?;
        let opened = begin(gateway, principal, &request, trace, now)?;
        match opened {
            TranslationStatus::Translated => {
                return Ok(MigrationState::HistoryImported {
                    record_count: page.records.len(),
                });
            }
            TranslationStatus::Refused => return Ok(MigrationState::Refused),
            TranslationStatus::Pending => {}
            TranslationStatus::ReceiptVerified { .. } => {
                return Err(fail(MigrationError::Gateway(GatewayError::Corrupt(
                    "read-only history import has a settlement receipt",
                ))));
            }
        }
        sink.store_external(principal, &page, trace).map_err(fail)?;
        verifier
            .commit_history(evidence, &page, trace)
            .map_err(fail)?;
        gateway
            .complete_read_only(principal, key, trace, now)
            .map_err(|error| trace.wrap(MigrationError::Gateway(error.into_error())))?;
        Ok(MigrationState::HistoryImported {
            record_count: page.records.len(),
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn settle(
    gateway: &mut GatewayCore,
    principal: &PrincipalId,
    key: [u8; 32],
    outcome: MigrationPlaneResult,
    sequencer_public_key: [u8; 32],
    trace: &TraceId,
    now: u64,
    verify_receipt: impl FnOnce(&VerifiedReceipt) -> Result<(), MigrationError>,
    completed: impl FnOnce([u8; 32]) -> MigrationState,
) -> Result<MigrationState, Traced<MigrationError>> {
    let fail = |error| trace.wrap(error);
    match outcome {
        MigrationPlaneResult::Pending => Ok(MigrationState::ProtocolPending),
        MigrationPlaneResult::Refused => {
            gateway
                .refuse_translation(principal, key, trace, now)
                .map_err(|error| trace.wrap(MigrationError::Gateway(error.into_error())))?;
            Ok(MigrationState::Refused)
        }
        MigrationPlaneResult::Executed {
            canonical_receipt,
            authorised_batch,
        } => {
            if authorised_batch.sequencer_public_key() != sequencer_public_key {
                return Err(fail(MigrationError::ReceiptMismatch));
            }
            let verified = verify(&canonical_receipt, &authorised_batch)
                .map_err(|_| fail(MigrationError::ReceiptMismatch))?;
            verify_receipt(&verified).map_err(fail)?;
            let status = gateway
                .settle_with_receipt(
                    principal,
                    key,
                    &canonical_receipt,
                    &authorised_batch,
                    trace,
                    now,
                )
                .map_err(|error| trace.wrap(MigrationError::Gateway(error.into_error())))?;
            let TranslationStatus::ReceiptVerified { receipt_digest } = status else {
                return Err(fail(MigrationError::ReceiptRequired));
            };
            Ok(completed(receipt_digest))
        }
    }
}

fn begin(
    gateway: &mut GatewayCore,
    principal: &PrincipalId,
    request: &TranslationRequest,
    trace: &TraceId,
    now: u64,
) -> Result<TranslationStatus, Traced<MigrationError>> {
    gateway
        .begin_translation(principal, request, trace, now)
        .map_err(|error| trace.wrap(MigrationError::Gateway(error.into_error())))
}

fn translation_request(
    key: [u8; 32],
    digest: [u8; 32],
    trace: &TraceId,
) -> Result<TranslationRequest, Traced<MigrationError>> {
    let fail = |error| trace.wrap(error);
    TranslationRequest::new(
        adapter_id().map_err(fail)?,
        TranslationKind::StateChanging,
        key,
        digest,
    )
    .map_err(|error| fail(MigrationError::Gateway(error)))
}

fn custody_context_hash(finality: &VerifiedAssetFinality) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(CUSTODY_CONTEXT_DOMAIN);
    hash.update(asset_key(finality));
    finality.chain.commit(&mut hash);
    finality.transaction.commit(&mut hash);
    hash.update(finality.custody_reference);
    hash.update(finality.evidence_digest);
    hash.update(finality.layerx_asset);
    hash.update(finality.layerx_amount.to_be_bytes());
    hash.update(finality.destination);
    hash.finalize().into()
}

fn binding_context_hash(ownership: &VerifiedOwnership) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(BINDING_CONTEXT_DOMAIN);
    hash.update(ownership_key(ownership));
    ownership.chain.commit(&mut hash);
    ownership.address.commit(&mut hash);
    hash.update(ownership.layerx_identity);
    hash.update(ownership.evidence_digest);
    hash.finalize().into()
}

fn adapter_id() -> Result<AdapterId, MigrationError> {
    AdapterId::new(ADAPTER_ID).map_err(|error| MigrationError::Gateway(error.into()))
}

/// Declares the migration boundary against pinned Ethereum/Solana source
/// specifications and a real conformance suite supplied by deployment.
///
/// # Errors
///
/// Returns an adapter declaration refusal if the stable identifier is invalid.
pub fn migration_adapter_descriptor(
    spec: PinnedSpec,
    conformance: ConformanceSuite,
) -> Result<AdapterDescriptor, MigrationError> {
    Ok(AdapterDescriptor::new(adapter_id()?, spec, conformance))
}

fn ownership_key(ownership: &VerifiedOwnership) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(IDEMPOTENCY_DOMAIN);
    hash.update(b"ownership\0");
    ownership.chain.commit(&mut hash);
    ownership.address.commit(&mut hash);
    hash.update(ownership.layerx_identity);
    hash.finalize().into()
}

fn asset_key(finality: &VerifiedAssetFinality) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(IDEMPOTENCY_DOMAIN);
    hash.update(b"asset\0");
    finality.chain.commit(&mut hash);
    finality.transaction.commit(&mut hash);
    hash.update(finality.custody_reference);
    hash.finalize().into()
}

fn history_key(page: &VerifiedHistoryPage) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(IDEMPOTENCY_DOMAIN);
    hash.update(b"history\0");
    hash.update(page.evidence_digest);
    hash.finalize().into()
}

fn ownership_digest(ownership: &VerifiedOwnership) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(REQUEST_DOMAIN);
    hash.update(b"ownership\0");
    ownership.chain.commit(&mut hash);
    ownership.address.commit(&mut hash);
    hash.update(ownership.layerx_identity);
    hash.update(ownership.evidence_digest);
    hash.finalize().into()
}

fn finality_digest(finality: &VerifiedAssetFinality) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(REQUEST_DOMAIN);
    hash.update(b"asset\0");
    finality.chain.commit(&mut hash);
    finality.source.commit(&mut hash);
    finality.transaction.commit(&mut hash);
    hash.update(finality.source_asset);
    hash.update(finality.source_amount.to_be_bytes());
    hash.update(finality.custody_reference);
    hash.update(finality.layerx_asset);
    hash.update(finality.layerx_amount.to_be_bytes());
    hash.update(finality.destination);
    hash.update(finality.finality_height.to_be_bytes());
    hash.update(finality.evidence_digest);
    hash.finalize().into()
}

fn history_digest(page: &VerifiedHistoryPage) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(REQUEST_DOMAIN);
    hash.update(b"history\0");
    hash.update(page.evidence_digest);
    hash.update((page.records.len() as u64).to_be_bytes());
    if let Some(cursor) = page.next_cursor {
        hash.update([1]);
        hash.update(cursor);
    } else {
        hash.update([0]);
    }
    for record in &page.records {
        record.chain.commit(&mut hash);
        record.transaction.commit(&mut hash);
        record.address.commit(&mut hash);
        hash.update([match record.kind {
            ExternalHistoryKind::Incoming => 1,
            ExternalHistoryKind::Outgoing => 2,
            ExternalHistoryKind::Contract => 3,
        }]);
        hash.update(record.timestamp.to_be_bytes());
        hash.update(record.source_asset);
        hash.update(record.source_amount.to_be_bytes());
    }
    hash.finalize().into()
}

/// Stable, redaction-safe migration refusals.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationError {
    Configuration,
    InvalidNetwork,
    InvalidAddress,
    AddressChainMismatch,
    InvalidTransaction,
    InvalidEvidence,
    EvidenceMismatch,
    SourcePending,
    SourceReverted,
    SourceDisplaced,
    FinalityWindowExceeded,
    RpcUnavailable,
    RpcRateLimited { retry_after_seconds: u64 },
    RpcDivergence,
    RpcResponseMismatch,
    CustodyEventMismatch,
    CustodyProgramMismatch,
    OwnershipSignatureMismatch,
    CheckpointIntegrity,
    CheckpointConflict,
    InvalidHistory,
    StorageRefused,
    PlaneRefused,
    ReceiptRequired,
    ReceiptMismatch,
    Gateway(GatewayError),
}

impl Display for MigrationError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Configuration => formatter.write_str("migration client configuration is invalid"),
            Self::InvalidNetwork => formatter.write_str("source network is invalid"),
            Self::InvalidAddress => formatter.write_str("source address is invalid"),
            Self::AddressChainMismatch => {
                formatter.write_str("source address does not belong to the declared chain")
            }
            Self::InvalidTransaction => formatter.write_str("source transaction is invalid"),
            Self::InvalidEvidence => formatter.write_str("source evidence is invalid"),
            Self::EvidenceMismatch => formatter.write_str("source evidence facts do not match"),
            Self::SourcePending => formatter.write_str("source finality is still pending"),
            Self::SourceReverted => formatter.write_str("source transaction reverted"),
            Self::SourceDisplaced => formatter.write_str("source transaction was displaced"),
            Self::FinalityWindowExceeded => {
                formatter.write_str("source ancestry exceeds the configured verification window")
            }
            Self::RpcUnavailable => formatter.write_str("source RPC is unavailable"),
            Self::RpcRateLimited {
                retry_after_seconds,
            } => write!(
                formatter,
                "source RPC rate limited the request; retry after {retry_after_seconds} seconds"
            ),
            Self::RpcDivergence => formatter.write_str("source RPC quorum disagreed"),
            Self::RpcResponseMismatch => {
                formatter.write_str("source RPC response did not match the request")
            }
            Self::CustodyEventMismatch => {
                formatter.write_str("source custody event did not match the migration claim")
            }
            Self::CustodyProgramMismatch => {
                formatter.write_str("source custody program instruction or account did not match")
            }
            Self::OwnershipSignatureMismatch => {
                formatter.write_str("source ownership signature did not match")
            }
            Self::CheckpointIntegrity => {
                formatter.write_str("migration checkpoint integrity verification failed")
            }
            Self::CheckpointConflict => {
                formatter.write_str("migration checkpoint or cursor conflicts with durable state")
            }
            Self::InvalidHistory => formatter.write_str("external history page is invalid"),
            Self::StorageRefused => formatter.write_str("external history storage refused"),
            Self::PlaneRefused => formatter.write_str("protocol migration operation refused"),
            Self::ReceiptRequired => formatter.write_str("a verified protocol receipt is required"),
            Self::ReceiptMismatch => {
                formatter.write_str("protocol receipt does not match the migration")
            }
            Self::Gateway(error) => write!(formatter, "gateway translation failed: {error}"),
        }
    }
}

impl std::error::Error for MigrationError {}

/// Codify anchor for Ethereum migration tooling.
#[must_use]
pub const fn interop_migrate_ethereum() -> &'static str {
    "verified-finality-ethereum-migration"
}

/// Codify anchor for Solana migration tooling.
#[must_use]
pub const fn interop_migrate_solana() -> &'static str {
    "verified-finality-solana-migration"
}
