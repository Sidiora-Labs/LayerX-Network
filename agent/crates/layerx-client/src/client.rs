//! Connection lifecycle for the sole core-boundary client.

use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use layerx_proof::inclusion::SequencerAuthorization;
use layerx_proof::receipt::AuthorizedBatch;
use layerx_types::ids::Did;
use layerx_types::payload::ModuleRegistry;
use layerx_types::verify::VerificationLevel;

use crate::availability::{
    fetch, AvailabilitySelector, FetchContext, FetchError, FetchOutcome, Progress, Provider,
    ProviderSet,
};
use crate::batch::{self, BatchHeaderError, SignedBatchHeader};
use crate::evidence::{
    self, CheckpointSelector, EvidenceContext, EvidenceError, FinalityEvidenceCandidate,
    ProofBundleSelector, RegistrationAck, RootSelector, VerifiedCheckpoint, VerifiedProofBundle,
};
use crate::head::{Head, HeadError, HeadTracker};
use crate::lni::handshake::{perform, Handshake, HandshakeConfig, HandshakeError};
use crate::lni::preparation::{
    preparation_state, PreparationState, PreparationStateContext, PreparationStateError,
};
use crate::lni::report::capability_report;
use crate::lni::schema::Capability;
use crate::lni::transport::{ConnectionGate, Limits, TransportError, Uds};
use crate::read::{
    account, balance, history, module_state, Balance, HistoryCursor, HistoryPage, ReadContext,
    ReadError, ReadValue, Requested,
};
use crate::receipt::{
    lookup, resolve_unknown, Lookup, LookupContext, ReceiptError, ReceiptSelector, Resolution,
};
use crate::stream::{subscribe, Cursor, EventStream, StreamConfig, StreamError};
use crate::submit::{submit_signed, Submission, SubmissionContext, SubmitError, UnknownCause};

/// Externally visible connection lifecycle state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionState {
    Connected,
    Degraded,
    Incompatible,
    Unreachable,
}

/// Bounded deterministic jitter schedule for reconnect attempts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ReconnectPolicy {
    pub maximum_attempts: u8,
    pub base_delay: Duration,
    pub maximum_delay: Duration,
    pub jitter_percent: u8,
}

impl ReconnectPolicy {
    /// Returns one bounded deterministic delay from the attempt and endpoint.
    #[must_use]
    pub fn delay(self, attempt: u8, endpoint: &str) -> Duration {
        let exponent = u32::from(attempt.min(31));
        let factor = 1_u32.checked_shl(exponent).unwrap_or(u32::MAX);
        let base = self
            .base_delay
            .checked_mul(factor)
            .unwrap_or(self.maximum_delay)
            .min(self.maximum_delay);
        let jitter_bound = base
            .as_millis()
            .saturating_mul(u128::from(self.jitter_percent))
            / 100;
        if jitter_bound == 0 {
            return base;
        }
        let seed = endpoint.bytes().fold(u128::from(attempt), |value, byte| {
            value.wrapping_mul(131).wrapping_add(u128::from(byte))
        });
        let jitter = seed % (jitter_bound + 1);
        base.saturating_add(Duration::from_millis(
            u64::try_from(jitter).unwrap_or(u64::MAX),
        ))
        .min(self.maximum_delay)
    }
}

/// Explicit client connection configuration.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientConfig {
    pub endpoint: PathBuf,
    pub handshake: HandshakeConfig,
    pub limits: Limits,
    pub reconnect: ReconnectPolicy,
}

/// Failure to establish or safely refresh the boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionError {
    Transport(TransportError),
    Handshake(HandshakeError),
    Head(HeadError),
    AttemptsExhausted,
}

impl ConnectionError {
    /// Stable state corresponding to this connection failure.
    #[must_use]
    pub const fn state(self) -> ConnectionState {
        match self {
            Self::Handshake(
                HandshakeError::InterfaceIncompatible { .. }
                | HandshakeError::ProtocolVersion { .. }
                | HandshakeError::Network { .. }
                | HandshakeError::MalformedEnvelope
                | HandshakeError::MalformedNodeInfo,
            )
            | Self::Head(_) => ConnectionState::Incompatible,
            Self::Transport(_)
            | Self::Handshake(HandshakeError::Transport(_))
            | Self::AttemptsExhausted => ConnectionState::Unreachable,
        }
    }
}

/// Sole owner of a live core-boundary connection.
pub struct Client {
    config: ClientConfig,
    gate: ConnectionGate,
    transport: Option<Uds>,
    handshake: Handshake,
    head: HeadTracker,
    state: ConnectionState,
}

impl Client {
    /// Opens the configured Unix boundary and performs the mandatory handshake.
    ///
    /// # Errors
    ///
    /// Returns a typed transport or startup-refusal failure before exposing a
    /// client.
    pub fn connect(config: ClientConfig) -> Result<Self, ConnectionError> {
        let gate = ConnectionGate::new(config.limits.maximum_connections);
        let mut transport = Uds::connect(&config.endpoint, &gate, config.limits)
            .map_err(ConnectionError::Transport)?;
        let handshake =
            perform(&mut transport, &config.handshake, None).map_err(ConnectionError::Handshake)?;
        let head = HeadTracker::new(handshake.node());
        let state = state_for(&handshake);
        Ok(Self {
            config,
            gate,
            transport: Some(transport),
            handshake,
            head,
            state,
        })
    }

    /// Reopens and revalidates the boundary under bounded jittered backoff.
    ///
    /// # Errors
    ///
    /// Refuses incompatibility and head regression immediately; reports
    /// exhaustion after the configured number of transport attempts.
    pub fn reconnect(&mut self) -> Result<(), ConnectionError> {
        self.transport = None;
        self.state = ConnectionState::Unreachable;
        let endpoint = self.config.endpoint.to_string_lossy();
        for attempt in 0..self.config.reconnect.maximum_attempts {
            if attempt != 0 {
                thread::sleep(self.config.reconnect.delay(attempt, &endpoint));
            }
            let Ok(mut transport) =
                Uds::connect(&self.config.endpoint, &self.gate, self.config.limits)
            else {
                self.state = ConnectionState::Unreachable;
                continue;
            };
            let handshake = match perform(
                &mut transport,
                &self.config.handshake,
                Some(&self.handshake),
            ) {
                Ok(handshake) => handshake,
                Err(error) => {
                    let error = ConnectionError::Handshake(error);
                    self.state = error.state();
                    return Err(error);
                }
            };
            if let Err(error) = self.head.update(handshake.node()) {
                let error = ConnectionError::Head(error);
                self.state = error.state();
                return Err(error);
            }
            self.state = state_for(&handshake);
            self.handshake = handshake;
            self.transport = Some(transport);
            return Ok(());
        }
        Err(ConnectionError::AttemptsExhausted)
    }

    /// Marks a lost in-flight request without claiming its outcome.
    pub const fn mark_transport_lost(&mut self) {
        self.state = ConnectionState::Unreachable;
    }

    #[must_use]
    pub const fn state(&self) -> ConnectionState {
        self.state
    }

    #[must_use]
    pub const fn head(&self) -> Head {
        self.head.current()
    }

    #[must_use]
    pub const fn handshake(&self) -> &Handshake {
        &self.handshake
    }

    /// Requires a receipt key to have been advertised for its batch.
    ///
    /// # Errors
    ///
    /// Refuses an unadvertised key without attempting signature verification.
    pub fn require_receipt_key(&self, batch: u64, key: [u8; 32]) -> Result<(), HeadError> {
        self.head.require_sequencer_key(batch, key)
    }

    /// Obtains the actor's complete atomic preparation snapshot from the
    /// authenticated production node boundary.
    ///
    /// # Errors
    ///
    /// Refuses a capability or connection gap and every typed core refusal,
    /// malformed response, network mismatch, or head regression.
    pub fn preparation_state(
        &mut self,
        actor: &Did,
        correlation_id: u64,
    ) -> Result<PreparationState, PreparationStateError> {
        if !self
            .handshake
            .capabilities()
            .contains(Capability::PreparationState)
        {
            return Err(PreparationStateError::UnavailableCapability);
        }
        let transport = self
            .transport
            .as_mut()
            .ok_or(PreparationStateError::Disconnected)?;
        preparation_state(
            transport,
            actor,
            PreparationStateContext {
                interface_version: self.handshake.node().interface_version,
                expected_network_id: self.config.handshake.expected_network_id,
                minimum_observed_head: self.head.current().chain_sequence,
                correlation_id,
            },
        )
    }

    /// Verifies and transmits one signed activity through the sole boundary
    /// connection.
    ///
    /// # Errors
    ///
    /// Refuses unavailable submission capability, a disconnected client, or
    /// any pre-transmission canonical/signature failure.
    pub fn submit_signed(
        &mut self,
        registry: &ModuleRegistry,
        signer_public_key: [u8; 32],
        correlation_id: u64,
        attempt: u32,
        signed_bytes: &[u8],
    ) -> Result<Submission, SubmitError> {
        if !self.handshake.capabilities().contains(Capability::Submit)
            || !self
                .handshake
                .capabilities()
                .contains(Capability::AuthenticatedDurableSubmit)
        {
            return Err(SubmitError::UnavailableCapability);
        }
        let context = SubmissionContext {
            interface_version: self.handshake.node().interface_version,
            protocol_version: self.config.handshake.expected_protocol_version,
            network_id: self.config.handshake.expected_network_id,
            correlation_id,
            signer_public_key,
            attempt,
        };
        let transport = self.transport.as_mut().ok_or(SubmitError::Disconnected)?;
        let submission = submit_signed(transport, registry, context, signed_bytes)?;
        let transport_lost = matches!(
            &submission,
            Submission::Unknown(unknown)
                if matches!(unknown.cause(), UnknownCause::Transport(_))
        );
        if transport_lost {
            self.transport = None;
            self.state = ConnectionState::Unreachable;
        }
        Ok(submission)
    }

    /// Retrieves and verifies one receipt through the active boundary.
    ///
    /// # Errors
    ///
    /// Refuses a capability gap, disconnected boundary, malformed response,
    /// selector mismatch, or proof verification failure.
    pub fn lookup_receipt(
        &mut self,
        selector: ReceiptSelector,
        correlation_id: u64,
        authorised_batch: AuthorizedBatch,
    ) -> Result<Lookup, ReceiptError> {
        if !self
            .handshake
            .capabilities()
            .contains(Capability::ReceiptLookup)
        {
            return Err(ReceiptError::UnavailableCapability);
        }
        let transport = self.transport.as_mut().ok_or(ReceiptError::Disconnected)?;
        lookup(
            transport,
            selector,
            LookupContext {
                interface_version: self.handshake.node().interface_version,
                correlation_id,
                authorised_batch,
            },
        )
    }

    /// Retrieves and independently verifies one canonical signed batch header.
    pub fn batch_header(
        &mut self,
        batch_number: u64,
        correlation_id: u64,
    ) -> Result<SignedBatchHeader, BatchHeaderError> {
        if !self
            .handshake
            .capabilities()
            .contains(Capability::BatchHeader)
        {
            return Err(BatchHeaderError::UnavailableCapability);
        }
        let transport = self
            .transport
            .as_mut()
            .ok_or(BatchHeaderError::Disconnected)?;
        batch::lookup(
            transport,
            self.handshake.node().interface_version,
            batch_number,
            correlation_id,
            self.handshake.node().authorised_sequencer_key,
        )
    }

    /// Runs bounded receipt-only resolution for an unknown submission.
    ///
    /// # Errors
    ///
    /// Refuses a capability gap, disconnected boundary, or invalid receipt
    /// evidence. Absence and transport loss remain `Resolution::Unknown`.
    pub fn resolve_unknown(
        &mut self,
        unknown: &crate::submit::Unknown,
        correlation_id: u64,
        authorised_batch: AuthorizedBatch,
        policy: ReconnectPolicy,
    ) -> Result<Resolution, ReceiptError> {
        if !self
            .handshake
            .capabilities()
            .contains(Capability::ReceiptLookup)
        {
            return Err(ReceiptError::UnavailableCapability);
        }
        let transport = self.transport.as_mut().ok_or(ReceiptError::Disconnected)?;
        resolve_unknown(
            transport,
            unknown,
            LookupContext {
                interface_version: self.handshake.node().interface_version,
                correlation_id,
                authorised_batch,
            },
            policy,
        )
    }

    /// Retrieves a proof-gated balance through the sole boundary.
    ///
    /// # Errors
    ///
    /// Refuses capability/disconnection gaps and all read verification errors.
    pub fn balance(
        &mut self,
        account_id: [u8; 32],
        asset_id: [u8; 32],
        requested: VerificationLevel,
        correlation_id: u64,
        authorization: SequencerAuthorization,
    ) -> Result<Balance, ReadError> {
        self.require_account_read_capabilities(requested)?;
        let context = self.read_context(requested, correlation_id, authorization);
        let transport = self.transport.as_mut().ok_or(ReadError::Disconnected)?;
        balance(transport, account_id, asset_id, context)
    }

    /// Retrieves exact proof-gated account bytes.
    ///
    /// # Errors
    ///
    /// Returns the complete balance-read refusal set.
    pub fn account(
        &mut self,
        account_id: [u8; 32],
        requested: VerificationLevel,
        correlation_id: u64,
        authorization: SequencerAuthorization,
    ) -> Result<ReadValue, ReadError> {
        self.require_account_read_capabilities(requested)?;
        let context = self.read_context(requested, correlation_id, authorization);
        let transport = self.transport.as_mut().ok_or(ReadError::Disconnected)?;
        account(transport, account_id, context)
    }

    /// Retrieves exact proof-gated module state bytes.
    ///
    /// # Errors
    ///
    /// Returns the complete point-read refusal set.
    pub fn module_state(
        &mut self,
        module_id: u16,
        key: &[u8],
        requested: VerificationLevel,
        correlation_id: u64,
        authorization: SequencerAuthorization,
    ) -> Result<ReadValue, ReadError> {
        self.require_read_capability(Capability::AccountRead)?;
        let context = self.read_context(requested, correlation_id, authorization);
        let transport = self.transport.as_mut().ok_or(ReadError::Disconnected)?;
        module_state(transport, module_id, key, context)
    }

    /// Retrieves one gap-free, cursor-bound history page.
    ///
    /// # Errors
    ///
    /// Returns capability, disconnection, proof and sequence failures.
    #[allow(clippy::too_many_arguments)]
    pub fn history(
        &mut self,
        start_sequence: u64,
        end_sequence: u64,
        page_bound: u16,
        cursor: Option<HistoryCursor>,
        requested: VerificationLevel,
        correlation_id: u64,
        authorization: SequencerAuthorization,
    ) -> Result<HistoryPage, ReadError> {
        self.require_read_capability(Capability::HistoryRange)?;
        let context = self.read_context(requested, correlation_id, authorization);
        let transport = self.transport.as_mut().ok_or(ReadError::Disconnected)?;
        history(
            transport,
            start_sequence,
            end_sequence,
            page_bound,
            cursor,
            context,
        )
    }

    fn require_read_capability(&self, capability: Capability) -> Result<(), ReadError> {
        if self.handshake.capabilities().contains(capability) {
            Ok(())
        } else {
            Err(ReadError::UnavailableCapability)
        }
    }

    fn require_account_read_capabilities(
        &self,
        requested: VerificationLevel,
    ) -> Result<(), ReadError> {
        self.require_read_capability(Capability::AccountRead)?;
        if requires_historical_account_proofs(requested) {
            self.require_read_capability(Capability::HistoricalProofs)?;
        }
        Ok(())
    }

    fn read_context(
        &self,
        requested: VerificationLevel,
        correlation_id: u64,
        authorization: SequencerAuthorization,
    ) -> ReadContext {
        let head = self.head.current();
        let root_selector = if requested >= VerificationLevel::CHECKPOINT_FINALISED {
            RootSelector::Checkpoint(head.finalised_checkpoint)
        } else {
            RootSelector::Latest
        };
        ReadContext {
            interface_version: self.handshake.node().interface_version,
            correlation_id,
            expected_protocol_version: self.config.handshake.expected_protocol_version,
            expected_network_id: self.config.handshake.expected_network_id,
            requested: Requested::new(requested),
            head,
            sequencer_authorization: authorization,
            handshake_sequencer_key: self.handshake.node().authorised_sequencer_key,
            root_selector,
        }
    }

    /// Retrieves one independently verified activity, receipt, or account proof.
    pub fn proof_bundle(
        &mut self,
        selector: ProofBundleSelector,
        correlation_id: u64,
        registry: &ModuleRegistry,
    ) -> Result<VerifiedProofBundle, EvidenceError> {
        if !self
            .handshake
            .capabilities()
            .contains(Capability::ProofBundle)
        {
            return Err(EvidenceError::Unavailable);
        }
        let transport = self.transport.as_mut().ok_or(EvidenceError::Unavailable)?;
        evidence::proof_bundle(
            transport,
            selector,
            EvidenceContext {
                interface_version: self.handshake.node().interface_version,
                correlation_id,
                expected_protocol_version: self.config.handshake.expected_protocol_version,
                expected_network_id: self.handshake.node().network_id,
                handshake_sequencer_key: self.handshake.node().authorised_sequencer_key,
            },
            registry,
        )
    }

    /// Retrieves one independently verified finalized checkpoint.
    pub fn checkpoint_evidence(
        &mut self,
        selector: CheckpointSelector,
        correlation_id: u64,
    ) -> Result<VerifiedCheckpoint, EvidenceError> {
        if !self
            .handshake
            .capabilities()
            .contains(Capability::Checkpoint)
        {
            return Err(EvidenceError::Unavailable);
        }
        let transport = self.transport.as_mut().ok_or(EvidenceError::Unavailable)?;
        evidence::checkpoint(
            transport,
            selector,
            EvidenceContext {
                interface_version: self.handshake.node().interface_version,
                correlation_id,
                expected_protocol_version: self.config.handshake.expected_protocol_version,
                expected_network_id: self.handshake.node().network_id,
                handshake_sequencer_key: self.handshake.node().authorised_sequencer_key,
            },
        )
    }

    /// Registers a locally verified checkpoint evidence bundle and accepts only
    /// the durable idempotent acknowledgement.
    pub fn register_finality_evidence(
        &mut self,
        evidence: &FinalityEvidenceCandidate,
        correlation_id: u64,
    ) -> Result<RegistrationAck, EvidenceError> {
        if !self
            .handshake
            .capabilities()
            .contains(Capability::FinalityEvidenceRegister)
        {
            return Err(EvidenceError::Unavailable);
        }
        let transport = self.transport.as_mut().ok_or(EvidenceError::Unavailable)?;
        evidence::register_finality_evidence(
            transport,
            evidence,
            self.handshake.node().interface_version,
            correlation_id,
        )
    }

    /// Starts or resumes the ordered core event stream through this client's
    /// sole boundary connection.
    ///
    /// # Errors
    ///
    /// Refuses capability/disconnection gaps, invalid bounds and request
    /// transport failures.
    pub fn subscribe_events(
        &mut self,
        cursor: Cursor,
        mut config: StreamConfig,
    ) -> Result<EventStream<'_>, StreamError> {
        if !self
            .handshake
            .capabilities()
            .contains(Capability::EventSubscribe)
        {
            return Err(StreamError::UnavailableCapability);
        }
        config.interface_version = self.handshake.node().interface_version;
        let transport = self
            .transport
            .as_mut()
            .ok_or(StreamError::DisconnectedClient)?;
        subscribe(transport, cursor, config)
    }

    /// Retrieves and verifies availability data from the active node provider.
    /// Additional provider transports use the same `ProviderSet` fetch path.
    ///
    /// # Errors
    ///
    /// Refuses capability/disconnection gaps and invalid retrieval context.
    pub fn fetch_availability<F>(
        &mut self,
        selector: AvailabilitySelector,
        mut context: FetchContext,
        on_chunk: F,
    ) -> Result<FetchOutcome, FetchError>
    where
        F: FnMut(Progress<'_>),
    {
        if !self
            .handshake
            .capabilities()
            .contains(Capability::AvailabilityFetch)
        {
            return Err(FetchError::UnavailableCapability);
        }
        context.interface_version = self.handshake.node().interface_version;
        let transport = self
            .transport
            .as_mut()
            .ok_or(FetchError::DisconnectedClient)?;
        let mut providers = ProviderSet::new(vec![Provider {
            name: "primary".to_owned(),
            transport,
        }]);
        fetch(&mut providers, selector, context, on_chunk)
    }
}

fn requires_historical_account_proofs(requested: VerificationLevel) -> bool {
    requested.wire_rank() >= VerificationLevel::CHECKPOINT_FINALISED.wire_rank()
}

fn state_for(handshake: &Handshake) -> ConnectionState {
    if capability_report(handshake.capabilities())
        .gaps()
        .is_empty()
    {
        ConnectionState::Connected
    } else {
        ConnectionState::Degraded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finalised_account_reads_require_historical_proofs() {
        assert!(!requires_historical_account_proofs(
            VerificationLevel::STATE_PROVEN
        ));
        assert!(requires_historical_account_proofs(
            VerificationLevel::CHECKPOINT_FINALISED
        ));
        assert!(requires_historical_account_proofs(
            VerificationLevel::SETTLEMENT_ANCHORED
        ));
    }
}
