// Publication and retrieval of batch mirror archives against in-memory
// Ethereum and Solana test networks, including stall and reorg scenarios.
//
// The archives are built exclusively from the node boundary types
// (VerifiedRead<BatchValue> and a fully verified AvailabilityResult), the
// Publisher drives the real chain-boundary traits, and the two networks are
// genuine in-memory ledgers that store and return the exact archive bytes by
// commitment. Only the external L1s are simulated; every LayerX code path -
// archive assembly, commitment, publication, confirmation, reorg handling,
// retrieval validation and freshness accounting - runs for real.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use layerx_client::availability::AvailabilityResult;
use layerx_mirror::{
    Archive, ArchiveCommitment, ArchiveData, BatchAuthorization, ChainFailure, ChainPosition,
    CheckpointCoordinate, CheckpointFreshness, ConfigError, EthereumArchiveClient,
    EthereumArchiveWrite, EthereumConfig, EthereumObservation, EthereumSubmission,
    GenericPublisher, MirrorDegradation, MirrorState, NodeBatch, NodeHead, PublicationId,
    RetrievalState, SignedHeaderTrust, SolanaArchiveClient, SolanaArchiveWrite, SolanaConfig,
    SolanaObservation, SolanaSubmission,
};
use layerx_proof::availability::{
    reassemble, verify_chunk, AvailabilityClass, Chunk, RootCommitments,
};
use layerx_proof::merkle::build_leaf_hash_proof;
use layerx_wire::receipt::decode_batch_header;

const NETWORK_ID: u32 = 77;
const REQUIRED_CONFIRMATIONS: u64 = 3;
const REQUIRED_ROOTED_SLOTS: u64 = 32;

// -------------------------------------------------------------------------
// Archive fixtures built through the node boundary.
// -------------------------------------------------------------------------

fn ethereum_config() -> EthereumConfig {
    EthereumConfig {
        chain_id: 1,
        archive_contract: [0xAB; 20],
        required_confirmations: REQUIRED_CONFIRMATIONS,
    }
}

fn solana_config() -> SolanaConfig {
    SolanaConfig {
        genesis_hash: [0xCD; 32],
        archive_program: [0x01; 32],
        archive_account: [0x02; 32],
        required_rooted_slots: REQUIRED_ROOTED_SLOTS,
    }
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-publisher")
        .join(name);
    std::fs::read(&path)
        .unwrap_or_else(|error| panic!("native fixture {}: {error}", path.display()))
}

fn take<const N: usize>(input: &mut &[u8]) -> [u8; N] {
    let (value, remaining) = input.split_at_checked(N).expect("native fixture field");
    *input = remaining;
    value.try_into().expect("exact native fixture field length")
}

fn stored_chunks(batch_number: u64, committed_root: [u8; 32]) -> Vec<Chunk> {
    let file = fixture_bytes(&format!("{batch_number}.lxda"));
    let mut input = file.as_slice();
    assert_eq!(take::<4>(&mut input), *b"LXD1");
    assert_eq!(u64::from_be_bytes(take(&mut input)), batch_number);
    assert_eq!(take::<32>(&mut input), committed_root);
    let count = u32::from_be_bytes(take(&mut input));
    let total = u64::from_be_bytes(take(&mut input));
    let mut chunks = Vec::new();
    let mut actual_total = 0_u64;
    for _ in 0..count {
        let index = u32::from_be_bytes(take(&mut input));
        let class = match take::<1>(&mut input)[0] {
            1 => AvailabilityClass::Activities,
            2 => AvailabilityClass::Receipts,
            3 => AvailabilityClass::Oracle,
            4 => AvailabilityClass::StateDiff,
            5 => AvailabilityClass::Recovery,
            value => panic!("native fixture availability class {value}"),
        };
        let class_offset = u64::from_be_bytes(take(&mut input));
        let length = u32::from_be_bytes(take(&mut input));
        let claimed_hash = take(&mut input);
        let length = usize::try_from(length).expect("native chunk length");
        let (bytes, remaining) = input.split_at_checked(length).expect("native chunk bytes");
        input = remaining;
        actual_total += u64::try_from(bytes.len()).expect("native chunk total");
        chunks.push(Chunk {
            batch_number,
            index,
            class,
            class_offset,
            bytes: bytes.to_vec(),
            claimed_hash,
        });
    }
    assert_eq!(actual_total, total);
    assert!(input.is_empty());
    chunks
}

fn build_archive(batch_number: u64, node_head: NodeHead) -> Archive {
    let header_bytes = fixture_bytes(&format!("{batch_number}.header"));
    let header = decode_batch_header(&header_bytes).expect("native signed batch header");
    assert_eq!(header.network_id(), NETWORK_ID);
    assert_eq!(header.batch_number(), batch_number);
    let trust = SignedHeaderTrust {
        sequencer_id: fixture_bytes("sequencer.id")
            .try_into()
            .expect("native sequencer ID"),
        sequencer_public_key: fixture_bytes("sequencer.public")
            .try_into()
            .expect("native sequencer public key"),
        first_batch_number: 1,
        last_batch_number: 6,
    };
    let batch = NodeBatch::verify(
        header_bytes,
        BatchAuthorization {
            sequencer_id: trust.sequencer_id,
            sequencer_public_key: trust.sequencer_public_key,
            first_batch_number: trust.first_batch_number,
            last_batch_number: trust.last_batch_number,
            header_signature: fixture_bytes(&format!("{batch_number}.signature"))
                .try_into()
                .expect("native batch signature"),
        },
        &trust,
    )
    .expect("authenticated native batch");
    let chunks = stored_chunks(batch_number, header.data_availability_root());
    let hashes = chunks
        .iter()
        .map(|chunk| chunk.claimed_hash)
        .collect::<Vec<_>>();
    let verified = chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            let (proof, computed_root) =
                build_leaf_hash_proof(&hashes, index).expect("native availability inclusion proof");
            assert_eq!(computed_root, header.data_availability_root());
            verify_chunk(chunk, &proof, batch_number, &computed_root)
                .expect("authenticated native availability chunk")
        })
        .collect::<Vec<_>>();
    let roots = RootCommitments {
        activity: header.activity_merkle_root(),
        receipt: header.receipt_merkle_root(),
        event: header.event_merkle_root(),
        oracle: header.oracle_root(),
    };
    let (records, _) = reassemble(&verified, roots).expect("canonical native record streams");
    let availability =
        AvailabilityResult::from_verified("native-da-store".to_owned(), verified, records, roots)
            .expect("complete authenticated native availability");
    Archive::from_node(&batch, &availability, None, node_head)
        .unwrap_or_else(|error| panic!("archive assembly failed: {error:?}"))
}

fn sealed_head(batch_number: u64) -> NodeHead {
    NodeHead {
        latest_sealed_batch: batch_number,
        latest_finalised_checkpoint: None,
    }
}

// -------------------------------------------------------------------------
// In-memory Ethereum test network.
// -------------------------------------------------------------------------

#[derive(Clone)]
enum EthObserve {
    Pending,
    Canonical {
        block_number: u64,
        block_hash: [u8; 32],
        confirmations: u64,
    },
    Reorged {
        former_block_number: u64,
        former_block_hash: [u8; 32],
    },
    Rejected,
    Fail(ChainFailure),
}

#[derive(Clone)]
struct EthAppend {
    chain_id: u64,
    archive_contract: [u8; 20],
    commitment: ArchiveCommitment,
    network_id: u32,
    batch_number: u64,
    checkpoint: Option<CheckpointCoordinate>,
    archive: Vec<u8>,
}

struct EthState {
    stored: BTreeMap<ArchiveCommitment, Vec<u8>>,
    appends: Vec<EthAppend>,
    append_failure: Option<ChainFailure>,
    observe: EthObserve,
    retrieve_failure: Option<ChainFailure>,
    withhold: bool,
    tamper: bool,
}

#[derive(Clone)]
struct Ethereum {
    state: Rc<RefCell<EthState>>,
}

impl Ethereum {
    fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(EthState {
                stored: BTreeMap::new(),
                appends: Vec::new(),
                append_failure: None,
                observe: EthObserve::Pending,
                retrieve_failure: None,
                withhold: false,
                tamper: false,
            })),
        }
    }

    fn set_observe(&self, observe: EthObserve) {
        self.state.borrow_mut().observe = observe;
    }

    fn confirmed(&self, confirmations: u64) {
        self.set_observe(EthObserve::Canonical {
            block_number: 4_200,
            block_hash: [0x11; 32],
            confirmations,
        });
    }

    fn fail_append(&self, failure: ChainFailure) {
        self.state.borrow_mut().append_failure = Some(failure);
    }

    fn fail_retrieve(&self, failure: ChainFailure) {
        self.state.borrow_mut().retrieve_failure = Some(failure);
    }

    fn withhold(&self) {
        self.state.borrow_mut().withhold = true;
    }

    fn append_count(&self) -> usize {
        self.state.borrow().appends.len()
    }

    fn last_append(&self) -> EthAppend {
        self.state
            .borrow()
            .appends
            .last()
            .cloned()
            .unwrap_or_else(|| panic!("no ethereum append recorded"))
    }
}

impl EthereumArchiveClient for Ethereum {
    fn append(
        &mut self,
        request: EthereumArchiveWrite<'_>,
    ) -> Result<EthereumSubmission, ChainFailure> {
        let mut state = self.state.borrow_mut();
        if let Some(failure) = state.append_failure.clone() {
            return Err(failure);
        }
        state.appends.push(EthAppend {
            chain_id: request.chain_id,
            archive_contract: request.archive_contract,
            commitment: request.commitment,
            network_id: request.network_id,
            batch_number: request.batch_number,
            checkpoint: request.checkpoint,
            archive: request.archive.to_vec(),
        });
        state
            .stored
            .insert(request.commitment, request.archive.to_vec());
        Ok(EthereumSubmission {
            transaction_hash: *request.commitment.as_bytes(),
        })
    }

    fn observe(
        &mut self,
        _transaction_hash: [u8; 32],
    ) -> Result<EthereumObservation, ChainFailure> {
        match self.state.borrow().observe.clone() {
            EthObserve::Pending => Ok(EthereumObservation::Pending),
            EthObserve::Canonical {
                block_number,
                block_hash,
                confirmations,
            } => Ok(EthereumObservation::Canonical {
                block_number,
                block_hash,
                confirmations,
            }),
            EthObserve::Reorged {
                former_block_number,
                former_block_hash,
            } => Ok(EthereumObservation::Reorged {
                former_block_number,
                former_block_hash,
            }),
            EthObserve::Rejected => Ok(EthereumObservation::Rejected),
            EthObserve::Fail(failure) => Err(failure),
        }
    }

    fn retrieve(
        &mut self,
        _archive_contract: [u8; 20],
        commitment: ArchiveCommitment,
    ) -> Result<Option<Vec<u8>>, ChainFailure> {
        let state = self.state.borrow();
        if let Some(failure) = state.retrieve_failure.clone() {
            return Err(failure);
        }
        if state.withhold {
            return Ok(None);
        }
        match state.stored.get(&commitment) {
            None => Ok(None),
            Some(bytes) => {
                let mut out = bytes.clone();
                if state.tamper {
                    if let Some(first) = out.first_mut() {
                        *first ^= 0x01;
                    }
                }
                Ok(Some(out))
            }
        }
    }
}

// -------------------------------------------------------------------------
// In-memory Solana test network.
// -------------------------------------------------------------------------

#[derive(Clone)]
enum SolObserve {
    Pending,
    Canonical {
        slot: u64,
        blockhash: [u8; 32],
        rooted_slots: u64,
    },
    Reorged {
        former_slot: u64,
        former_blockhash: [u8; 32],
    },
    Rejected,
}

#[derive(Clone)]
struct SolAppend {
    genesis_hash: [u8; 32],
    archive_program: [u8; 32],
    archive_account: [u8; 32],
    commitment: ArchiveCommitment,
    network_id: u32,
    batch_number: u64,
    checkpoint: Option<CheckpointCoordinate>,
    archive: Vec<u8>,
}

struct SolState {
    stored: BTreeMap<ArchiveCommitment, Vec<u8>>,
    appends: Vec<SolAppend>,
    append_failure: Option<ChainFailure>,
    observe: SolObserve,
    retrieve_failure: Option<ChainFailure>,
    withhold: bool,
    tamper: bool,
}

#[derive(Clone)]
struct Solana {
    state: Rc<RefCell<SolState>>,
}

impl Solana {
    fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(SolState {
                stored: BTreeMap::new(),
                appends: Vec::new(),
                append_failure: None,
                observe: SolObserve::Pending,
                retrieve_failure: None,
                withhold: false,
                tamper: false,
            })),
        }
    }

    fn set_observe(&self, observe: SolObserve) {
        self.state.borrow_mut().observe = observe;
    }

    fn confirmed(&self, rooted_slots: u64) {
        self.set_observe(SolObserve::Canonical {
            slot: 9_000,
            blockhash: [0x22; 32],
            rooted_slots,
        });
    }

    fn tamper(&self) {
        self.state.borrow_mut().tamper = true;
    }

    fn append_count(&self) -> usize {
        self.state.borrow().appends.len()
    }

    fn last_append(&self) -> SolAppend {
        self.state
            .borrow()
            .appends
            .last()
            .cloned()
            .unwrap_or_else(|| panic!("no solana append recorded"))
    }
}

fn solana_signature(commitment: ArchiveCommitment) -> [u8; 64] {
    let mut signature = [0_u8; 64];
    signature[..32].copy_from_slice(commitment.as_bytes());
    signature[32..].copy_from_slice(commitment.as_bytes());
    signature
}

impl SolanaArchiveClient for Solana {
    fn append(
        &mut self,
        request: SolanaArchiveWrite<'_>,
    ) -> Result<SolanaSubmission, ChainFailure> {
        let mut state = self.state.borrow_mut();
        if let Some(failure) = state.append_failure.clone() {
            return Err(failure);
        }
        state.appends.push(SolAppend {
            genesis_hash: request.genesis_hash,
            archive_program: request.archive_program,
            archive_account: request.archive_account,
            commitment: request.commitment,
            network_id: request.network_id,
            batch_number: request.batch_number,
            checkpoint: request.checkpoint,
            archive: request.archive.to_vec(),
        });
        state
            .stored
            .insert(request.commitment, request.archive.to_vec());
        Ok(SolanaSubmission {
            signature: solana_signature(request.commitment),
        })
    }

    fn observe(&mut self, _signature: [u8; 64]) -> Result<SolanaObservation, ChainFailure> {
        match self.state.borrow().observe.clone() {
            SolObserve::Pending => Ok(SolanaObservation::Pending),
            SolObserve::Canonical {
                slot,
                blockhash,
                rooted_slots,
            } => Ok(SolanaObservation::Canonical {
                slot,
                blockhash,
                rooted_slots,
            }),
            SolObserve::Reorged {
                former_slot,
                former_blockhash,
            } => Ok(SolanaObservation::Reorged {
                former_slot,
                former_blockhash,
            }),
            SolObserve::Rejected => Ok(SolanaObservation::Rejected),
        }
    }

    fn retrieve(
        &mut self,
        _archive_program: [u8; 32],
        _archive_account: [u8; 32],
        commitment: ArchiveCommitment,
    ) -> Result<Option<Vec<u8>>, ChainFailure> {
        let state = self.state.borrow();
        if let Some(failure) = state.retrieve_failure.clone() {
            return Err(failure);
        }
        if state.withhold {
            return Ok(None);
        }
        match state.stored.get(&commitment) {
            None => Ok(None),
            Some(bytes) => {
                let mut out = bytes.clone();
                if state.tamper {
                    if let Some(first) = out.first_mut() {
                        *first ^= 0x01;
                    }
                }
                Ok(Some(out))
            }
        }
    }
}

fn new_publisher(ethereum: &Ethereum, solana: &Solana) -> GenericPublisher<Ethereum, Solana> {
    GenericPublisher::new(
        ethereum_config(),
        ethereum.clone(),
        solana_config(),
        solana.clone(),
    )
    .unwrap_or_else(|error| panic!("publisher construction failed: {error:?}"))
}

// -------------------------------------------------------------------------
// Tests.
// -------------------------------------------------------------------------

#[test]
fn confirms_and_retrieves_the_same_archive_on_both_test_networks() {
    let archive = build_archive(1, sealed_head(1));
    let ethereum = Ethereum::new();
    let solana = Solana::new();
    ethereum.confirmed(REQUIRED_CONFIRMATIONS);
    solana.confirmed(REQUIRED_ROOTED_SLOTS);
    let mut publisher = new_publisher(&ethereum, &solana);

    let report = publisher.publish(&archive);
    assert_eq!(report.commitment, archive.commitment());

    let MirrorState::Confirmed {
        commitment: eth_commitment,
        publication: eth_publication,
        position: eth_position,
        freshness: eth_freshness,
    } = report.ethereum
    else {
        panic!("ethereum did not confirm: {:?}", report.ethereum);
    };
    assert_eq!(eth_commitment, archive.commitment());
    assert_eq!(
        eth_publication,
        PublicationId::Ethereum(*archive.commitment().as_bytes())
    );
    assert_eq!(
        eth_position,
        ChainPosition::Ethereum {
            block_number: 4_200,
            block_hash: [0x11; 32],
            confirmations: REQUIRED_CONFIRMATIONS,
        }
    );
    assert_eq!(eth_freshness.latest_batch_mirrored, Some(1));
    assert_eq!(eth_freshness.batch_lag, 0);
    assert_eq!(eth_freshness.node_latest_sealed_batch, 1);

    let MirrorState::Confirmed {
        publication: sol_publication,
        position: sol_position,
        freshness: sol_freshness,
        ..
    } = report.solana
    else {
        panic!("solana did not confirm: {:?}", report.solana);
    };
    assert_eq!(
        sol_publication,
        PublicationId::Solana(solana_signature(archive.commitment()))
    );
    assert_eq!(
        sol_position,
        ChainPosition::Solana {
            slot: 9_000,
            blockhash: [0x22; 32],
            rooted_slots: REQUIRED_ROOTED_SLOTS,
        }
    );
    assert_eq!(sol_freshness.latest_batch_mirrored, Some(1));
    assert_eq!(sol_freshness.batch_lag, 0);

    assert_eq!(publisher.ethereum_cursor().latest_batch, Some(1));
    assert_eq!(publisher.solana_cursor().latest_batch, Some(1));

    let retrieval = publisher.retrieve(archive.commitment());
    let RetrievalState::Retrieved(eth_archive) = retrieval.ethereum else {
        panic!("ethereum retrieval failed: {:?}", retrieval.ethereum);
    };
    assert_eq!(*eth_archive, *archive.data());
    let RetrievalState::Retrieved(sol_archive) = retrieval.solana else {
        panic!("solana retrieval failed: {:?}", retrieval.solana);
    };
    assert_eq!(*sol_archive, *archive.data());
}

#[test]
fn publishes_pure_archives_without_custody_semantics() {
    let archive = build_archive(2, sealed_head(2));
    let ethereum = Ethereum::new();
    let solana = Solana::new();
    ethereum.confirmed(REQUIRED_CONFIRMATIONS);
    solana.confirmed(REQUIRED_ROOTED_SLOTS);
    let mut publisher = new_publisher(&ethereum, &solana);
    let _ = publisher.publish(&archive);

    let eth_append = ethereum.last_append();
    assert_eq!(eth_append.chain_id, ethereum_config().chain_id);
    assert_eq!(
        eth_append.archive_contract,
        ethereum_config().archive_contract
    );
    assert_eq!(eth_append.commitment, archive.commitment());
    assert_eq!(eth_append.network_id, NETWORK_ID);
    assert_eq!(eth_append.batch_number, 2);
    assert_eq!(eth_append.checkpoint, None);
    assert_eq!(eth_append.archive, archive.bytes());

    let sol_append = solana.last_append();
    assert_eq!(sol_append.genesis_hash, solana_config().genesis_hash);
    assert_eq!(sol_append.archive_program, solana_config().archive_program);
    assert_eq!(sol_append.archive_account, solana_config().archive_account);
    assert_eq!(sol_append.commitment, archive.commitment());
    assert_eq!(sol_append.network_id, NETWORK_ID);
    assert_eq!(sol_append.batch_number, 2);
    assert_eq!(sol_append.checkpoint, None);
    assert_eq!(sol_append.archive, archive.bytes());

    // The published bytes decode to a pure batch archive: canonical batch
    // header, availability chunks, public record streams and an optional
    // checkpoint - and nothing resembling a vault, portal or custody claim.
    let decoded = ArchiveData::decode(&eth_append.archive)
        .unwrap_or_else(|error| panic!("published archive did not decode: {error:?}"));
    assert_eq!(&decoded, archive.data());
    assert_eq!(decoded.network_id, NETWORK_ID);
    assert_eq!(decoded.batch_number, 2);
    assert!(decoded.checkpoint.is_none());
}

#[test]
fn a_stalled_mirror_reports_lag_while_the_other_confirms() {
    // Batch 5 confirms on both chains.
    let first = build_archive(5, sealed_head(5));
    let ethereum = Ethereum::new();
    let solana = Solana::new();
    ethereum.confirmed(REQUIRED_CONFIRMATIONS);
    solana.confirmed(REQUIRED_ROOTED_SLOTS);
    let mut publisher = new_publisher(&ethereum, &solana);
    let confirmed = publisher.publish(&first);
    assert!(matches!(confirmed.ethereum, MirrorState::Confirmed { .. }));
    assert!(matches!(confirmed.solana, MirrorState::Confirmed { .. }));

    // Batch 6 is sealed at the node; ethereum stalls in the mempool while
    // solana confirms. Neither outcome suppresses the other.
    let second = build_archive(6, sealed_head(6));
    ethereum.set_observe(EthObserve::Pending);
    solana.confirmed(REQUIRED_ROOTED_SLOTS);
    let report = publisher.publish(&second);

    let MirrorState::Pending {
        freshness: eth_freshness,
        ..
    } = report.ethereum
    else {
        panic!("ethereum should be pending: {:?}", report.ethereum);
    };
    // The stalled mirror states its lag honestly rather than presenting the
    // newer batch as mirrored.
    assert_eq!(eth_freshness.latest_batch_mirrored, Some(5));
    assert_eq!(eth_freshness.node_latest_sealed_batch, 6);
    assert_eq!(eth_freshness.batch_lag, 1);

    let MirrorState::Confirmed {
        freshness: sol_freshness,
        ..
    } = report.solana
    else {
        panic!("solana should confirm: {:?}", report.solana);
    };
    assert_eq!(sol_freshness.latest_batch_mirrored, Some(6));
    assert_eq!(sol_freshness.batch_lag, 0);

    assert_eq!(publisher.ethereum_cursor().latest_batch, Some(5));
    assert_eq!(publisher.solana_cursor().latest_batch, Some(6));
}

#[test]
fn a_pending_publication_is_idempotent_and_later_confirms() {
    let archive = build_archive(3, sealed_head(3));
    let ethereum = Ethereum::new();
    let solana = Solana::new();
    ethereum.set_observe(EthObserve::Pending);
    solana.set_observe(SolObserve::Pending);
    let mut publisher = new_publisher(&ethereum, &solana);

    let first = publisher.publish(&archive);
    assert!(matches!(first.ethereum, MirrorState::Pending { .. }));
    let second = publisher.publish(&archive);
    assert!(matches!(second.ethereum, MirrorState::Pending { .. }));
    // A re-check of the same archive never re-appends the transaction.
    assert_eq!(ethereum.append_count(), 1);
    assert_eq!(solana.append_count(), 1);

    // Once the transaction reaches finality the same archive confirms without
    // a duplicate write.
    ethereum.confirmed(REQUIRED_CONFIRMATIONS);
    solana.confirmed(REQUIRED_ROOTED_SLOTS);
    let third = publisher.publish(&archive);
    assert!(matches!(third.ethereum, MirrorState::Confirmed { .. }));
    assert!(matches!(third.solana, MirrorState::Confirmed { .. }));
    assert_eq!(ethereum.append_count(), 1);
    assert_eq!(solana.append_count(), 1);
}

#[test]
fn below_finality_confirmations_stay_pending() {
    let archive = build_archive(4, sealed_head(4));
    let ethereum = Ethereum::new();
    let solana = Solana::new();
    ethereum.confirmed(REQUIRED_CONFIRMATIONS - 1);
    solana.confirmed(REQUIRED_ROOTED_SLOTS - 1);
    let mut publisher = new_publisher(&ethereum, &solana);
    let report = publisher.publish(&archive);
    assert!(matches!(report.ethereum, MirrorState::Pending { .. }));
    assert!(matches!(report.solana, MirrorState::Pending { .. }));
    // Nothing is confirmed yet, so no batch is mirrored.
    assert_eq!(publisher.ethereum_cursor().latest_batch, None);
    assert_eq!(publisher.solana_cursor().latest_batch, None);
}

#[test]
fn a_reorg_is_observable_and_the_cursor_retreats() {
    let archive = build_archive(4, sealed_head(4));
    let ethereum = Ethereum::new();
    let solana = Solana::new();
    ethereum.confirmed(REQUIRED_CONFIRMATIONS);
    solana.confirmed(REQUIRED_ROOTED_SLOTS);
    let mut publisher = new_publisher(&ethereum, &solana);
    let confirmed = publisher.publish(&archive);
    assert!(matches!(confirmed.ethereum, MirrorState::Confirmed { .. }));
    assert_eq!(publisher.ethereum_cursor().latest_batch, Some(4));

    // The chain that carried the archive reorgs it out; re-checking surfaces
    // the reorg and retreats the freshness cursor rather than hiding it.
    ethereum.set_observe(EthObserve::Reorged {
        former_block_number: 4_200,
        former_block_hash: [0x11; 32],
    });
    solana.set_observe(SolObserve::Reorged {
        former_slot: 9_000,
        former_blockhash: [0x22; 32],
    });
    let report = publisher.publish(&archive);

    let MirrorState::Reorged {
        former_position: eth_former,
        freshness: eth_freshness,
        ..
    } = report.ethereum
    else {
        panic!("ethereum should report a reorg: {:?}", report.ethereum);
    };
    assert_eq!(
        eth_former,
        ChainPosition::Ethereum {
            block_number: 4_200,
            block_hash: [0x11; 32],
            confirmations: 0,
        }
    );
    assert_eq!(eth_freshness.latest_batch_mirrored, None);
    assert_eq!(eth_freshness.batch_lag, 4);

    let MirrorState::Reorged {
        former_position: sol_former,
        ..
    } = report.solana
    else {
        panic!("solana should report a reorg: {:?}", report.solana);
    };
    assert_eq!(
        sol_former,
        ChainPosition::Solana {
            slot: 9_000,
            blockhash: [0x22; 32],
            rooted_slots: 0,
        }
    );

    assert_eq!(publisher.ethereum_cursor().latest_batch, None);
    assert_eq!(publisher.solana_cursor().latest_batch, None);
}

#[test]
fn a_rejected_transaction_degrades_typed() {
    let archive = build_archive(3, sealed_head(3));
    let ethereum = Ethereum::new();
    let solana = Solana::new();
    ethereum.set_observe(EthObserve::Rejected);
    solana.set_observe(SolObserve::Rejected);
    let mut publisher = new_publisher(&ethereum, &solana);
    let report = publisher.publish(&archive);
    assert!(matches!(
        report.ethereum,
        MirrorState::Degraded {
            degradation: MirrorDegradation::TransactionRejected,
            ..
        }
    ));
    assert!(matches!(
        report.solana,
        MirrorState::Degraded {
            degradation: MirrorDegradation::TransactionRejected,
            ..
        }
    ));
}

#[test]
fn mirror_chain_unavailability_is_a_typed_degradation_not_a_block() {
    let archive = build_archive(6, sealed_head(6));
    let ethereum = Ethereum::new();
    let solana = Solana::new();
    // Ethereum RPC is unreachable; Solana is healthy.
    ethereum.fail_append(ChainFailure::Unavailable(
        "ethereum rpc unreachable".to_owned(),
    ));
    solana.confirmed(REQUIRED_ROOTED_SLOTS);
    let mut publisher = new_publisher(&ethereum, &solana);

    // Publication returns a report - LayerX operation is never blocked by a
    // mirror chain being down.
    let report = publisher.publish(&archive);
    let MirrorState::Degraded {
        degradation: eth_degradation,
        freshness: eth_freshness,
        ..
    } = report.ethereum
    else {
        panic!("ethereum should degrade: {:?}", report.ethereum);
    };
    assert!(matches!(
        eth_degradation,
        MirrorDegradation::Chain(ChainFailure::Unavailable(_))
    ));
    assert_eq!(eth_freshness.latest_batch_mirrored, None);
    assert_eq!(eth_freshness.batch_lag, 6);
    // The healthy mirror is unaffected by the other chain's outage.
    assert!(matches!(report.solana, MirrorState::Confirmed { .. }));
    assert_eq!(publisher.solana_cursor().latest_batch, Some(6));

    // An observe-time outage degrades the same way without stopping progress.
    let observe_only = Ethereum::new();
    observe_only.set_observe(EthObserve::Fail(ChainFailure::RateLimited(
        "ethereum rate limited".to_owned(),
    )));
    let healthy_solana = Solana::new();
    healthy_solana.confirmed(REQUIRED_ROOTED_SLOTS);
    let mut second = new_publisher(&observe_only, &healthy_solana);
    let report = second.publish(&archive);
    assert!(matches!(
        report.ethereum,
        MirrorState::Degraded {
            degradation: MirrorDegradation::Chain(ChainFailure::RateLimited(_)),
            ..
        }
    ));
    assert!(matches!(report.solana, MirrorState::Confirmed { .. }));
}

#[test]
fn confirmation_requires_the_archive_to_be_retrievable_intact() {
    // A canonical, finalised transaction whose stored bytes cannot be read
    // back must not be reported as confirmed.
    let archive = build_archive(2, sealed_head(2));
    let ethereum = Ethereum::new();
    let solana = Solana::new();
    ethereum.confirmed(REQUIRED_CONFIRMATIONS);
    ethereum.withhold();
    solana.confirmed(REQUIRED_ROOTED_SLOTS);
    solana.tamper();
    let mut publisher = new_publisher(&ethereum, &solana);
    let report = publisher.publish(&archive);
    assert!(matches!(
        report.ethereum,
        MirrorState::Degraded {
            degradation: MirrorDegradation::ArchiveNotRetrievable,
            ..
        }
    ));
    assert!(matches!(
        report.solana,
        MirrorState::Degraded {
            degradation: MirrorDegradation::RetrievedCommitmentMismatch,
            ..
        }
    ));
    assert_eq!(publisher.ethereum_cursor().latest_batch, None);
    assert_eq!(publisher.solana_cursor().latest_batch, None);
}

#[test]
fn retrieval_reports_missing_tampered_and_unavailable_mirrors() {
    let archive = build_archive(6, sealed_head(6));
    let ethereum = Ethereum::new();
    let solana = Solana::new();
    ethereum.confirmed(REQUIRED_CONFIRMATIONS);
    solana.confirmed(REQUIRED_ROOTED_SLOTS);
    let mut publisher = new_publisher(&ethereum, &solana);
    let confirmed = publisher.publish(&archive);
    assert!(matches!(confirmed.ethereum, MirrorState::Confirmed { .. }));

    // Ethereum can no longer serve the archive; Solana serves corrupted bytes.
    ethereum.withhold();
    solana.tamper();
    let retrieval = publisher.retrieve(archive.commitment());
    assert_eq!(retrieval.ethereum, RetrievalState::Missing);
    assert_eq!(
        retrieval.solana,
        RetrievalState::Degraded(MirrorDegradation::RetrievedCommitmentMismatch)
    );
    // A retrieval that no longer validates retreats the freshness cursor.
    assert_eq!(publisher.ethereum_cursor().latest_batch, None);
    assert_eq!(publisher.solana_cursor().latest_batch, None);

    // A transport failure on retrieval is surfaced as a typed chain failure.
    let offline = Ethereum::new();
    offline.confirmed(REQUIRED_CONFIRMATIONS);
    let healthy_solana = Solana::new();
    healthy_solana.confirmed(REQUIRED_ROOTED_SLOTS);
    let mut second = new_publisher(&offline, &healthy_solana);
    let _ = second.publish(&archive);
    offline.fail_retrieve(ChainFailure::Unavailable(
        "ethereum archive node down".to_owned(),
    ));
    let retrieval = second.retrieve(archive.commitment());
    assert!(matches!(
        retrieval.ethereum,
        RetrievalState::Degraded(MirrorDegradation::Chain(ChainFailure::Unavailable(_)))
    ));
    assert!(matches!(retrieval.solana, RetrievalState::Retrieved(_)));
}

#[test]
fn freshness_states_checkpoint_lag_honestly() {
    // The node has no finalised checkpoint yet: the mirror says so rather than
    // implying a checkpoint is mirrored.
    let no_checkpoint = build_archive(5, sealed_head(5));
    let ethereum = Ethereum::new();
    let solana = Solana::new();
    ethereum.confirmed(REQUIRED_CONFIRMATIONS);
    solana.confirmed(REQUIRED_ROOTED_SLOTS);
    let mut publisher = new_publisher(&ethereum, &solana);
    let report = publisher.publish(&no_checkpoint);
    assert_eq!(
        report.ethereum.freshness().checkpoint,
        CheckpointFreshness::NodeHasNoCheckpoint
    );
    assert_eq!(report.ethereum.freshness().latest_checkpoint_mirrored, None);

    // The node has finalised a checkpoint but no checkpoint-bearing archive has
    // been mirrored yet: the mirror reports the target it must still reach.
    let coordinate = CheckpointCoordinate {
        batch_number: 5,
        checkpoint_id: [0x7C; 32],
    };
    let awaiting = build_archive(
        5,
        NodeHead {
            latest_sealed_batch: 5,
            latest_finalised_checkpoint: Some(coordinate),
        },
    );
    let fresh_eth = Ethereum::new();
    let fresh_sol = Solana::new();
    fresh_eth.confirmed(REQUIRED_CONFIRMATIONS);
    fresh_sol.confirmed(REQUIRED_ROOTED_SLOTS);
    let mut second = new_publisher(&fresh_eth, &fresh_sol);
    let report = second.publish(&awaiting);
    assert_eq!(
        report.ethereum.freshness().checkpoint,
        CheckpointFreshness::NotYetMirrored { target_batch: 5 }
    );
    assert_eq!(
        report.ethereum.freshness().node_latest_finalised_checkpoint,
        Some(coordinate)
    );
    assert_eq!(report.ethereum.freshness().latest_checkpoint_mirrored, None);
}

#[test]
fn invalid_chain_configuration_is_rejected_before_any_write() {
    let zero_ethereum = EthereumConfig {
        chain_id: 0,
        archive_contract: [0; 20],
        required_confirmations: 0,
    };
    assert_eq!(
        GenericPublisher::new(
            zero_ethereum,
            Ethereum::new(),
            solana_config(),
            Solana::new()
        )
        .err(),
        Some(ConfigError::Ethereum)
    );

    let zero_solana = SolanaConfig {
        genesis_hash: [0; 32],
        archive_program: [0; 32],
        archive_account: [0; 32],
        required_rooted_slots: 0,
    };
    assert_eq!(
        GenericPublisher::new(
            ethereum_config(),
            Ethereum::new(),
            zero_solana,
            Solana::new()
        )
        .err(),
        Some(ConfigError::Solana)
    );
}
