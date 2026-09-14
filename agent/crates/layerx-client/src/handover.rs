use layerx_crypto::ed25519;
use layerx_proof::availability::AvailabilityClass;
use layerx_proof::inclusion::{verify_header, SequencerAuthorization, VerifiedBatchHeader};
use layerx_proof::receipt::verify_sequencer_signature;
use layerx_proof::state_witness::StateWitness;
use layerx_types::payload::ModuleRegistry;
use layerx_wire::activity::{decode_signed, encode_signed, Activity};
use layerx_wire::batch_maintenance::{decode_maintenance, MaintenanceReceipt};
use layerx_wire::handover::{
    decode_evidence, decode_recovery, sequencer_id, Evidence, MAX_RECOVERY_BYTES, MAX_TRANSITIONS,
};
use layerx_wire::hash::{activity_id, payload_hash};
use layerx_wire::receipt::{decode_batch_header, BatchHeader};
use sha2::{Digest as _, Sha256};
use std::fmt::Write as _;

use crate::availability::AvailabilityResult;
use crate::evidence::VerifiedCheckpoint;

const HANDOVER_ACTIVITY: u32 = 0x0007_0009;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryError {
    Genesis,
    Header,
    Continuity,
    Availability,
    Activity,
    Certificate,
    Predecessor,
    Finality,
    Receipt,
    Maintenance,
    Bounds,
    UnverifiedRange,
    Transport,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Interval {
    epoch: u64,
    public_key: [u8; 32],
    first_batch: u64,
    last_batch: u64,
    first_sequence: u64,
    last_sequence: u64,
}

/// Sequencer intervals derived from a pinned genesis and authenticated native batches.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SequencerHistory {
    network_id: u32,
    governance_key: [u8; 32],
    genesis_root: [u8; 32],
    intervals: Vec<Interval>,
    predecessor: Option<VerifiedBatchHeader>,
    predecessor_signature: [u8; 64],
    latest_evidence: Option<[u8; 32]>,
    registry: ModuleRegistry,
}

impl SequencerHistory {
    /// Reads the next contiguous native batch through a caller-authenticated transport.
    /// No signer claim from the transport becomes trusted before complete history verification.
    ///
    /// # Errors
    /// Refuses transport errors, incomplete bundles and every history verification failure.
    pub fn fetch_next(
        &mut self,
        transport: &mut dyn crate::lni::transport::FrameTransport,
        version: crate::lni::schema::Version,
        correlation_id: u64,
        limits: crate::availability::RetrievalLimits,
    ) -> Result<(), HistoryError> {
        use crate::availability::{
            fetch, AvailabilitySelector, FetchContext, FetchOutcome, Provider, ProviderSet,
        };
        use crate::evidence::{checkpoint, CheckpointSelector, EvidenceContext};
        let batch = self
            .predecessor
            .as_ref()
            .map_or(0, |prior| prior.header().batch_number())
            .checked_add(1)
            .ok_or(HistoryError::Bounds)?;
        if correlation_id == 0 || correlation_id.checked_add(2).is_none() {
            return Err(HistoryError::Bounds);
        }
        let candidate = crate::batch::lookup_untrusted(transport, version, batch, correlation_id)
            .map_err(|_| HistoryError::Transport)?;
        self.check_continuity(candidate.header())?;
        let header = candidate.header();
        let context = FetchContext {
            interface_version: version,
            correlation_id: correlation_id + 1,
            expected_batch_number: batch,
            data_availability_root: header.data_availability_root(),
            record_roots: layerx_proof::availability::RootCommitments {
                activity: header.activity_merkle_root(),
                receipt: header.receipt_merkle_root(),
                event: header.event_merkle_root(),
                oracle: header.oracle_root(),
            },
            limits,
        };
        let fetched = fetch(
            &mut ProviderSet::new(vec![Provider {
                name: String::from("authenticated-node"),
                transport,
            }]),
            AvailabilitySelector::Batch(batch),
            context,
            |_| {},
        )
        .map_err(|_| HistoryError::Availability)?;
        let FetchOutcome::Complete(availability) = fetched else {
            return Err(HistoryError::Availability);
        };
        let current = self.intervals.last().ok_or(HistoryError::Genesis)?;
        let finality = if current.epoch == header.epoch() {
            None
        } else {
            Some(
                checkpoint(
                    transport,
                    CheckpointSelector::Batch(batch - 1),
                    EvidenceContext {
                        interface_version: version,
                        correlation_id: correlation_id + 2,
                        expected_protocol_version: 3,
                        expected_network_id: self.network_id,
                        handshake_sequencer_key: current.public_key,
                    },
                )
                .map_err(|_| HistoryError::Finality)?,
            )
        };
        self.advance(
            candidate.canonical_bytes(),
            candidate.signature(),
            &availability,
            finality.as_ref(),
        )
    }

    #[must_use]
    pub const fn verified_head(&self) -> Option<&VerifiedBatchHeader> {
        self.predecessor.as_ref()
    }

    /// The root and initial sequencer key must come from independently pinned genesis configuration.
    ///
    /// # Errors
    /// Refuses absent, substituted or uncommitted governance authority.
    pub fn from_genesis(
        network_id: u32,
        genesis_root: [u8; 32],
        initial_sequencer_key: [u8; 32],
        authority_witness: &[u8],
        registry: ModuleRegistry,
    ) -> Result<Self, HistoryError> {
        let witness = StateWitness::decode(authority_witness).map_err(|_| HistoryError::Genesis)?;
        witness
            .verify(genesis_root)
            .map_err(|_| HistoryError::Genesis)?;
        let mut authority_key = [0_u8; 32];
        authority_key[..18].copy_from_slice(b"handover-authority");
        let governance_key: [u8; 32] = witness
            .value
            .as_slice()
            .try_into()
            .map_err(|_| HistoryError::Genesis)?;
        sequencer_id(&initial_sequencer_key).map_err(|_| HistoryError::Genesis)?;
        sequencer_id(&governance_key).map_err(|_| HistoryError::Genesis)?;
        if network_id == 0
            || genesis_root == [0; 32]
            || initial_sequencer_key == [0; 32]
            || witness.module_id != 7
            || witness.key != authority_key
            || governance_key == [0; 32]
            || governance_key == initial_sequencer_key
            || !registry.registrations().iter().any(|registration| {
                registration
                    .activity_types()
                    .iter()
                    .any(|kind| kind.value() == HANDOVER_ACTIVITY)
            })
        {
            return Err(HistoryError::Genesis);
        }
        Ok(Self {
            network_id,
            governance_key,
            genesis_root,
            registry,
            intervals: vec![Interval {
                epoch: 1,
                public_key: initial_sequencer_key,
                first_batch: 1,
                last_batch: u64::MAX,
                first_sequence: 1,
                last_sequence: u64::MAX,
            }],
            predecessor: None,
            predecessor_signature: [0; 64],
            latest_evidence: None,
        })
    }

    /// Rechecks complete native batch material before changing any trusted interval.
    /// Finality is the exact predecessor checkpoint independently checked through authenticated LNI.
    ///
    /// # Errors
    /// Refuses gaps, substitutions, unsigned handovers, incomplete availability and failed activation.
    pub fn advance(
        &mut self,
        canonical_header: &[u8],
        signature: &[u8; 64],
        availability: &AvailabilityResult,
        finality: Option<&VerifiedCheckpoint>,
    ) -> Result<(), HistoryError> {
        let header = decode_batch_header(canonical_header).map_err(|_| HistoryError::Header)?;
        self.check_continuity(&header)?;
        verify_availability(availability, &header)?;
        let recovery = recovery_bytes(availability)?;
        let (_, packet) = decode_recovery(&recovery).map_err(|_| HistoryError::Certificate)?;
        let current = self.intervals.last().ok_or(HistoryError::Genesis)?;
        let transition = header.epoch() != current.epoch;
        let activities = decode_activities(availability, &self.registry, transition)?;
        let mut next = current.clone();
        if transition {
            if self.intervals.len() > MAX_TRANSITIONS {
                return Err(HistoryError::Bounds);
            }
            let packet = packet.ok_or(HistoryError::Certificate)?;
            let evidence = decode_evidence(packet).map_err(|_| HistoryError::Certificate)?;
            let activity = activities.first().ok_or(HistoryError::Activity)?;
            self.check_transition(&header, &evidence, finality)?;
            verify_activation(activity, packet, &header, self.governance_key)?;
            verify_activation_receipts(availability, activity, &evidence, &header)?;
            next = Interval {
                epoch: header.epoch(),
                public_key: evidence.certificate.new_public_key,
                first_batch: header.batch_number(),
                last_batch: u64::MAX,
                first_sequence: header.first_sequence(),
                last_sequence: u64::MAX,
            };
        } else {
            let actual = packet.map(|bytes| <[u8; 32]>::from(Sha256::digest(bytes)));
            if actual != self.latest_evidence || finality.is_some() {
                return Err(HistoryError::Certificate);
            }
        }
        let authorization = interval_authorization(&next, header.batch_number())?;
        let verified = verify_header(canonical_header, signature, &authorization)
            .map_err(|_| HistoryError::Header)?;
        if transition {
            let previous = self.intervals.last_mut().ok_or(HistoryError::Genesis)?;
            previous.last_batch = header.batch_number() - 1;
            previous.last_sequence = header.first_sequence() - 1;
            self.intervals.push(next);
            self.latest_evidence = packet.map(|bytes| Sha256::digest(bytes).into());
        }
        self.predecessor = Some(verified);
        self.predecessor_signature = *signature;
        Ok(())
    }

    fn check_continuity(&self, header: &BatchHeader) -> Result<(), HistoryError> {
        let (batch, sequence, root) =
            self.predecessor
                .as_ref()
                .map_or((0, 0, self.genesis_root), |prior| {
                    (
                        prior.header().batch_number(),
                        prior.header().last_sequence(),
                        prior.header().resulting_state_root(),
                    )
                });
        let epoch = self.intervals.last().ok_or(HistoryError::Genesis)?.epoch;
        if header.network_id() != self.network_id
            || header.protocol_version() != 3
            || batch.checked_add(1) != Some(header.batch_number())
            || sequence.checked_add(1) != Some(header.first_sequence())
            || header.last_sequence() < header.first_sequence()
            || header.last_sequence() == u64::MAX
            || header.previous_state_root() != root
            || (header.epoch() != epoch && epoch.checked_add(1) != Some(header.epoch()))
        {
            return Err(HistoryError::Continuity);
        }
        Ok(())
    }

    fn check_transition(
        &self,
        header: &BatchHeader,
        evidence: &Evidence<'_>,
        finality: Option<&VerifiedCheckpoint>,
    ) -> Result<(), HistoryError> {
        let current = self.intervals.last().ok_or(HistoryError::Genesis)?;
        let previous = self.predecessor.as_ref().ok_or(HistoryError::Predecessor)?;
        let certificate = &evidence.certificate;
        if certificate.network_id != self.network_id
            || certificate.old_epoch != current.epoch
            || certificate.new_epoch != header.epoch()
            || certificate.old_public_key != current.public_key
            || certificate.activation_batch != header.batch_number()
            || certificate.predecessor_last_sequence.checked_add(1) != Some(header.first_sequence())
            || evidence.predecessor_header != previous.canonical_bytes()
            || evidence.predecessor_signature != self.predecessor_signature
            || self.governance_key == certificate.new_public_key
        {
            return Err(HistoryError::Predecessor);
        }
        ed25519::verify_message(
            &self.governance_key,
            &certificate.governance_signature,
            evidence.signed_certificate,
        )
        .map_err(|_| HistoryError::Certificate)?;
        let finality = finality.ok_or(HistoryError::Finality)?;
        if finality.checkpoint_bytes() != evidence.checkpoint_payload
            || finality.context_bytes() != evidence.finality_proof
            || finality.canonical_header() != evidence.predecessor_header
            || finality.report().evidence().checkpoint_id()
                != Some(certificate.predecessor_checkpoint_id)
            || finality
                .report()
                .evidence()
                .settlement_reference()
                .is_none()
        {
            return Err(HistoryError::Finality);
        }
        Ok(())
    }

    /// # Errors
    /// Refuses a batch outside the completely authenticated history prefix.
    pub fn authorization_for_batch(
        &self,
        batch: u64,
    ) -> Result<SequencerAuthorization, HistoryError> {
        let head = self
            .predecessor
            .as_ref()
            .ok_or(HistoryError::UnverifiedRange)?;
        if batch == 0 || batch > head.header().batch_number() {
            return Err(HistoryError::UnverifiedRange);
        }
        let interval = self
            .intervals
            .iter()
            .find(|entry| (entry.first_batch..=entry.last_batch).contains(&batch))
            .ok_or(HistoryError::UnverifiedRange)?;
        interval_authorization(interval, head.header().batch_number())
    }

    /// # Errors
    /// Refuses a sequence outside the completely authenticated history prefix.
    pub fn authorization_for_sequence(
        &self,
        sequence: u64,
    ) -> Result<SequencerAuthorization, HistoryError> {
        let head = self
            .predecessor
            .as_ref()
            .ok_or(HistoryError::UnverifiedRange)?;
        if sequence == 0 || sequence > head.header().last_sequence() {
            return Err(HistoryError::UnverifiedRange);
        }
        let interval = self
            .intervals
            .iter()
            .find(|entry| (entry.first_sequence..=entry.last_sequence).contains(&sequence))
            .ok_or(HistoryError::UnverifiedRange)?;
        interval_authorization(interval, head.header().batch_number())
    }
}

fn interval_authorization(
    interval: &Interval,
    verified_through: u64,
) -> Result<SequencerAuthorization, HistoryError> {
    Ok(SequencerAuthorization::new(
        sequencer_id(&interval.public_key).map_err(|_| HistoryError::Certificate)?,
        interval.public_key,
        interval.first_batch,
        interval.last_batch.min(verified_through),
    ))
}

fn verify_availability(
    availability: &AvailabilityResult,
    header: &BatchHeader,
) -> Result<(), HistoryError> {
    let roots = availability.record_roots();
    if availability.batch_number() != header.batch_number()
        || availability.data_availability_root() != header.data_availability_root()
        || availability.chunks.iter().any(|chunk| {
            chunk.chunk().batch_number != header.batch_number()
                || chunk.data_availability_root() != header.data_availability_root()
        })
        || roots.activity != header.activity_merkle_root()
        || roots.receipt != header.receipt_merkle_root()
        || roots.event != header.event_merkle_root()
        || roots.oracle != header.oracle_root()
    {
        return Err(HistoryError::Availability);
    }
    availability
        .records()
        .verify(&availability.chunks, roots)
        .map_err(|_| HistoryError::Availability)?;
    Ok(())
}

fn recovery_bytes(availability: &AvailabilityResult) -> Result<Vec<u8>, HistoryError> {
    let mut bytes = Vec::new();
    for chunk in &availability.chunks {
        let chunk = chunk.chunk();
        if chunk.class != AvailabilityClass::Recovery {
            continue;
        }
        if bytes
            .len()
            .checked_add(chunk.bytes.len())
            .is_none_or(|size| size > MAX_RECOVERY_BYTES)
        {
            return Err(HistoryError::Bounds);
        }
        bytes.extend_from_slice(&chunk.bytes);
    }
    Ok(bytes)
}

fn decode_activities(
    availability: &AvailabilityResult,
    registry: &ModuleRegistry,
    transition: bool,
) -> Result<Vec<Activity>, HistoryError> {
    let mut activities = Vec::new();
    for bytes in &availability.records().activities {
        let activity = decode_signed(bytes, registry).map_err(|_| HistoryError::Activity)?;
        if encode_signed(&activity).map_err(|_| HistoryError::Activity)? != *bytes
            || (activity.activity_type().value() == HANDOVER_ACTIVITY) != transition
        {
            return Err(HistoryError::Activity);
        }
        activities.push(activity);
    }
    if transition && activities.len() != 1 {
        return Err(HistoryError::Activity);
    }
    Ok(activities)
}

fn verify_activation(
    activity: &Activity,
    packet: &[u8],
    header: &BatchHeader,
    governance_key: [u8; 32],
) -> Result<(), HistoryError> {
    let bound = activity.timestamp_bound();
    let mut expected_did = String::from("did:layerx:");
    for byte in governance_key {
        write!(expected_did, "{byte:02x}").map_err(|_| HistoryError::Activity)?;
    }
    if activity.protocol_version() != 3
        || activity.network_id() != header.network_id()
        || activity.payload() != packet
        || activity.authority() != governance_key
        || activity.actor_did() != expected_did.as_bytes()
        || payload_hash(activity).map_err(|_| HistoryError::Activity)? != activity.payload_hash()
        || bound.not_before > header.timestamp_ms()
        || bound.not_after < header.timestamp_ms()
        || bound
            .not_after
            .checked_sub(bound.not_before)
            .is_none_or(|window| window > 300_000)
    {
        return Err(HistoryError::Activity);
    }
    let signature = activity
        .signature()
        .ok_or(HistoryError::Activity)?
        .try_into()
        .map_err(|_| HistoryError::Activity)?;
    let preimage = layerx_wire::sign::preimage(activity).map_err(|_| HistoryError::Activity)?;
    ed25519::verify_digest(&governance_key, &signature, preimage.as_bytes())
        .map_err(|_| HistoryError::Activity)
}

fn verify_activation_receipts(
    availability: &AvailabilityResult,
    activity: &Activity,
    evidence: &Evidence<'_>,
    header: &BatchHeader,
) -> Result<(), HistoryError> {
    let records = availability.records();
    if records.receipts.len() != 2
        || records.events.len() != 2
        || !records.oracle_inputs.is_empty()
        || header.first_sequence().checked_add(1) != Some(header.last_sequence())
    {
        return Err(HistoryError::Receipt);
    }
    let receipt =
        verify_sequencer_signature(&records.receipts[0], evidence.certificate.new_public_key)
            .map_err(|_| HistoryError::Receipt)?;
    let receipt = receipt.protocol().ok_or(HistoryError::Receipt)?;
    if receipt.protocol_version() != 3
        || receipt.module_id() != 7
        || receipt.module_version() != 1
        || receipt.result_code() != 0
        || receipt.global_sequence() != header.first_sequence()
        || receipt.timestamp() != header.timestamp_ms()
        || receipt.activity_id() != activity_id(activity).map_err(|_| HistoryError::Activity)?
        || receipt.previous_state_root() != header.previous_state_root()
        || receipt.activity_root() != header.activity_merkle_root()
    {
        return Err(HistoryError::Receipt);
    }
    let maintenance =
        decode_maintenance(&records.receipts[1]).map_err(|_| HistoryError::Maintenance)?;
    if !matches!(maintenance, MaintenanceReceipt::Batch(_))
        || maintenance.occupancy().previous_state_root != receipt.resulting_state_root()
    {
        return Err(HistoryError::Maintenance);
    }
    maintenance
        .verify_header(header)
        .map_err(|_| HistoryError::Maintenance)
}
