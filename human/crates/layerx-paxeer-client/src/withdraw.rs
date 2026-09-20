use std::time::Duration;

use layerx_proof::inclusion::{verify_receipt, InclusionError, SequencerAuthorization};
use layerx_proof::merkle::Proof;
use layerx_proof::receipt::{
    verify_outcome, AuthorizedBatch, ReceiptCheck, VerificationFailure, VerifiedReceipt,
};
use layerx_types::intent::EvmAddress;
use layerx_wire::receipt::{decode_batch_header, decode_merkle_proof};
use sha2::{Digest as _, Sha256};

use crate::client::{
    BlockRef, ClientConfigError, EndpointError, ExecutionOutcome, LogRecord, PaxeerClient,
    TransactionHash, TransactionInclusion,
};
use crate::custody::{
    decode_asset, decode_claim, decode_claim_finalised, decode_claim_queued,
    decode_custody_release, decode_status, get_asset_calldata, get_claim_calldata,
    native_asset_id_calldata, nullifier_status_calldata, withdrawal_claim_id, withdrawal_nullifier,
    CustodyAbiError, CustodyAsset, CustodyClaim, WithdrawalMaterial, CLAIM_FINALISED_TOPIC,
    CLAIM_QUEUED_TOPIC, CUSTODY_PRECOMPILE, CUSTODY_RELEASE_TOPIC, WEI_PER_BASE_UNIT,
};
use crate::finality::{
    FinalityReport, FinalityStage, FinalityTracker, TrackerConfig, TrackerConfigError,
};
use crate::json::Json;
use crate::rpc::EndpointConfig;

const WORD: usize = 32;
const WITHDRAWAL_EVENT_BYTES: usize = 254;

/// The native `layerxAnchor` precompile whose finalized roots the custody
/// precompile checks every withdrawal header against.
pub const ANCHOR_PRECOMPILE: EvmAddress = EvmAddress::new([
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10, 0x14,
]);

const SELECTOR_BALANCE_OF: [u8; 4] = [0x70, 0xa0, 0x82, 0x31];
pub(crate) const SELECTOR_FINALIZED_STATE_ROOT: [u8; 4] = [0x0f, 0x60, 0x7f, 0xe4];
pub(crate) const SELECTOR_FINALIZED_RECEIPT_ROOT: [u8; 4] = [0xe0, 0xa3, 0xcc, 0xaa];
pub(crate) const SELECTOR_LATEST_FINALIZED: [u8; 4] = [0x6c, 0xdd, 0x45, 0xae];

/// Declared Paxeer withdrawal boundary and finality policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalConfig {
    pub endpoints: Vec<EndpointConfig>,
    pub minimum_endpoint_agreement: usize,
    pub required_confirmations: u64,
    pub poll_cadence: Duration,
    pub delayed_after_polls: u64,
}

/// Why withdrawal boundary configuration was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WithdrawalConfigError {
    Endpoints(ClientConfigError),
    Agreement(TrackerConfigError),
    ZeroRequiredConfirmations,
    ZeroPollCadence,
    ZeroDelayedAfterPolls,
    UnsupportedProtocolVersion,
}

/// Exact facts the verified `LayerX` debit receipt must establish.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DebitExpectation {
    pub activity_id: [u8; 32],
    pub network_id: u32,
    pub withdrawal_id: [u8; 32],
    pub account: [u8; 32],
    pub withdrawals_account: [u8; 32],
    pub asset_id: [u8; 32],
    pub amount: u128,
    pub recipient: EvmAddress,
}

impl DebitExpectation {
    /// Constructs the exact debit claim only after rejecting every empty
    /// consensus binding. This is the owner-side ingress used by bounded wire
    /// decoders; callers cannot accidentally admit the less strict test
    /// literal representation.
    ///
    /// # Errors
    /// Refuses empty consensus bindings.
    pub fn validated(self) -> Result<Self, DebitFault> {
        validate_debit_expectation(&self)?;
        Ok(self)
    }
}

/// Exact reason canonical `LayerX` debit evidence was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DebitFault {
    EmptyField(&'static str),
    Unverifiable(VerificationFailure),
    Refused { result_code: i32 },
    WrongActivity { expected: [u8; 32], found: [u8; 32] },
    WrongAsset { expected: [u8; 32], found: [u8; 32] },
    WrongAmount { expected: u128, found: u128 },
    WrongAccount { expected: [u8; 32], found: [u8; 32] },
    WrongWithdrawalsAccount { expected: [u8; 32], found: [u8; 32] },
}

/// A `LayerX` debit admitted only after canonical receipt verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommittedWithdrawalDebit {
    expectation: DebitExpectation,
    receipt_reference: [u8; 32],
    sequencer_public_key: [u8; 32],
    verified: VerifiedReceipt,
}

impl CommittedWithdrawalDebit {
    /// Verifies the canonical signed `LayerX` receipt and every withdrawal debit binding.
    ///
    /// # Errors
    ///
    /// Returns the first structural, signature, result, or field mismatch.
    pub fn verify(
        receipt_bytes: &[u8],
        batch: &AuthorizedBatch,
        expectation: DebitExpectation,
    ) -> Result<Self, DebitFault> {
        validate_debit_expectation(&expectation)?;
        let verified = verify_outcome(receipt_bytes, batch).map_err(DebitFault::Unverifiable)?;
        let protocol = verified.receipt().protocol().ok_or({
            DebitFault::Unverifiable(VerificationFailure {
                check: ReceiptCheck::ReceiptShape,
            })
        })?;
        if protocol.activity_id() != expectation.activity_id {
            return Err(DebitFault::WrongActivity {
                expected: expectation.activity_id,
                found: protocol.activity_id(),
            });
        }
        if protocol.result_code() != 0 {
            return Err(DebitFault::Refused {
                result_code: protocol.result_code(),
            });
        }
        if protocol.asset() != expectation.asset_id {
            return Err(DebitFault::WrongAsset {
                expected: expectation.asset_id,
                found: protocol.asset(),
            });
        }
        if protocol.amount() != expectation.amount {
            return Err(DebitFault::WrongAmount {
                expected: expectation.amount,
                found: protocol.amount(),
            });
        }
        if protocol.from() != expectation.account {
            return Err(DebitFault::WrongAccount {
                expected: expectation.account,
                found: protocol.from(),
            });
        }
        if protocol.to() != expectation.withdrawals_account {
            return Err(DebitFault::WrongWithdrawalsAccount {
                expected: expectation.withdrawals_account,
                found: protocol.to(),
            });
        }
        Ok(Self {
            expectation,
            receipt_reference: Sha256::digest(receipt_bytes).into(),
            sequencer_public_key: batch.sequencer_public_key(),
            verified,
        })
    }

    #[must_use]
    pub const fn expectation(&self) -> DebitExpectation {
        self.expectation
    }

    #[must_use]
    pub const fn receipt_reference(&self) -> [u8; 32] {
        self.receipt_reference
    }

    #[must_use]
    pub const fn verified_receipt(&self) -> &VerifiedReceipt {
        &self.verified
    }
}

/// Why a claim was refused before a wallet transaction could be requested.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimRefusal {
    UnsupportedProtocolVersion {
        protocol_version: u16,
    },
    DebitProtocolMismatch {
        expected: u16,
        found: u16,
    },
    NetworkMismatch {
        debit: u32,
        header: u32,
    },
    /// The material is empty, oversized or not canonical wire bytes.
    Material(&'static str),
    /// The material's receipt is not the verified debit receipt.
    ReceiptMismatch {
        debit: [u8; 32],
        material: [u8; 32],
    },
    /// The receipt is not included under the sequencer-signed header.
    Inclusion(&'static str),
    /// The receipt does not commit the expected native withdrawal.
    Effect(&'static str),
    NullifierMismatch {
        local: [u8; 32],
        receipt: [u8; 32],
    },
    /// Paxeer has not finalized the batch the receipt is included in.
    BatchNotFinalised {
        batch_number: u64,
    },
    /// Paxeer finalized different roots for the batch than the signed header.
    FinalisedRootMismatch {
        batch_number: u64,
    },
    /// 1 reserved, 2 consumed, 3 cancelled.
    NullifierUsed {
        nullifier: [u8; 32],
        status: u8,
    },
    AssetUnavailable {
        asset_id: [u8; 32],
    },
}

/// Any typed failure while constructing, observing, or verifying a withdrawal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WithdrawalError {
    Endpoint(EndpointError),
    Contract {
        detail: String,
    },
    Refused(ClaimRefusal),
    NotFinal {
        stage: FinalityStage,
    },
    Displaced {
        lost: TransactionInclusion,
        head: u64,
        requeued: bool,
    },
    Reverted {
        inclusion: TransactionInclusion,
    },
    InclusionChanged {
        tracked: BlockRef,
        observed: Option<BlockRef>,
    },
    MissingQuorumEvidence,
    EvidenceSourceMismatch,
    TransactionTarget {
        expected: EvmAddress,
        found: Option<EvmAddress>,
    },
    TransactionInput,
    TransactionValue,
    MissingEvent(&'static str),
    DuplicateEvent(&'static str),
    MalformedEvent {
        event: &'static str,
        detail: String,
    },
    ClaimState {
        detail: String,
    },
    PayoutNotVerified {
        detail: String,
    },
    CancellationNotVerified {
        detail: String,
    },
}

/// A precompile-valid claim assembled from one verified debit and its
/// inclusion material.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawalClaim {
    debit: CommittedWithdrawalDebit,
    material: WithdrawalMaterial,
    batch_number: u64,
    anchor: [u8; 32],
    nullifier: [u8; 32],
    claim_id: [u8; 32],
    calldata: Vec<u8>,
}

impl WithdrawalClaim {
    /// The custody precompile every claim transaction targets.
    #[must_use]
    pub const fn contract(&self) -> EvmAddress {
        CUSTODY_PRECOMPILE
    }

    #[must_use]
    pub const fn debit(&self) -> &CommittedWithdrawalDebit {
        &self.debit
    }

    #[must_use]
    pub const fn material(&self) -> &WithdrawalMaterial {
        &self.material
    }

    /// The `LayerX` batch whose signed header commits the receipt.
    #[must_use]
    pub const fn batch_number(&self) -> u64 {
        self.batch_number
    }

    /// The request anchor the receipt's nullifier binds.
    #[must_use]
    pub const fn anchor(&self) -> [u8; 32] {
        self.anchor
    }

    #[must_use]
    pub const fn nullifier(&self) -> [u8; 32] {
        self.nullifier
    }

    /// The claim identifier the precompile derives for this withdrawal.
    #[must_use]
    pub const fn claim_id(&self) -> [u8; 32] {
        self.claim_id
    }

    /// Exact `requestWithdrawal` bytes.
    #[must_use]
    pub fn calldata(&self) -> &[u8] {
        &self.calldata
    }
}

/// A queued claim whose transaction and on-chain claim record both verified final.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmittedWithdrawalClaim {
    claim: WithdrawalClaim,
    claim_id: [u8; 32],
    available_at: u64,
    submission_transaction: TransactionHash,
    submission_inclusion: TransactionInclusion,
}

impl SubmittedWithdrawalClaim {
    #[must_use]
    pub const fn claim(&self) -> &WithdrawalClaim {
        &self.claim
    }

    #[must_use]
    pub const fn claim_id(&self) -> [u8; 32] {
        self.claim_id
    }

    #[must_use]
    pub const fn available_at(&self) -> u64 {
        self.available_at
    }

    #[must_use]
    pub const fn submission_transaction(&self) -> TransactionHash {
        self.submission_transaction
    }

    #[must_use]
    pub const fn submission_inclusion(&self) -> TransactionInclusion {
        self.submission_inclusion
    }

    /// Exact `finaliseWithdrawal` bytes: the precompile re-verifies the same
    /// evidence before it pays.
    #[must_use]
    pub fn finalise_calldata(&self) -> Vec<u8> {
        self.claim.material.finalise_calldata()
    }
}

/// Paxeer custody disposition after the authority cancelled a pending claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PaxeerFundsDisposition {
    RetainedInVault {
        vault: EvmAddress,
        asset_id: [u8; 32],
        amount: u128,
    },
}

/// `LayerX` debit disposition after Paxeer correctly refuses payout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProtocolDebitDisposition {
    RemainsCommittedPendingProtocolRecovery { debit_receipt_reference: [u8; 32] },
}

/// Both sides of the funds boundary after a claim is cancelled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CancelledFundsDisposition {
    pub paxeer: PaxeerFundsDisposition,
    pub layerx: ProtocolDebitDisposition,
}

/// Honest claim state; a `Paid` state is deliberately absent until payout evidence verifies.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClaimProgress {
    WaitingForChallengeWindow {
        available_at: u64,
        observed_at: u64,
        remaining: Duration,
    },
    ReadyToFinalise {
        available_at: u64,
        observed_at: u64,
    },
    PaidAwaitingPayoutVerification,
    /// The custody authority cancelled the pending claim inside its window.
    /// Cancellation is a module message: no EVM transaction or log exists.
    Cancelled {
        disposition: CancelledFundsDisposition,
    },
}

/// Final evidence joining the `LayerX` debit, its anchor and the Paxeer payout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PayoutEvidence {
    pub debit_receipt_reference: [u8; 32],
    /// The request anchor the claim's nullifier binds.
    pub checkpoint_hash: [u8; 32],
    pub claim_id: [u8; 32],
    pub payout_transaction: TransactionHash,
    pub payout_inclusion: TransactionInclusion,
    /// The custody precompile that released the funds.
    pub vault: EvmAddress,
    /// The asset's registered ERC20 pointer; zero when the asset has none.
    pub token: EvmAddress,
    pub asset_id: [u8; 32],
    pub recipient: EvmAddress,
    pub amount: u128,
}

/// Agreed custody state proving the authority cancelled the claim without
/// releasing custody: claim status 3 and nullifier status 3.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CancellationEvidence {
    pub debit_receipt_reference: [u8; 32],
    pub checkpoint_hash: [u8; 32],
    pub claim_id: [u8; 32],
    /// Paxeer head at which every configured quorum read agreed.
    pub observed_head: u64,
    pub disposition: CancelledFundsDisposition,
}

/// Real Paxeer withdrawal boundary: precompile reads, claim construction and evidence verification.
#[derive(Clone, Debug)]
pub struct WithdrawalBoundary {
    protocol_version: u16,
    client: PaxeerClient,
    endpoints: Vec<EndpointConfig>,
    required_confirmations: u64,
    poll_cadence: Duration,
    delayed_after_polls: u64,
    minimum_endpoint_agreement: usize,
}

struct VerifiedMaterial {
    batch_number: u64,
    state_root: [u8; 32],
    receipt_root: [u8; 32],
    anchor: [u8; 32],
    nullifier: [u8; 32],
}

fn verify_material(
    debit: &CommittedWithdrawalDebit,
    material: &WithdrawalMaterial,
) -> Result<VerifiedMaterial, ClaimRefusal> {
    let expectation = &debit.expectation;
    let digest: [u8; 32] = Sha256::digest(&material.receipt).into();
    if digest != debit.receipt_reference {
        return Err(ClaimRefusal::ReceiptMismatch {
            debit: debit.receipt_reference,
            material: digest,
        });
    }
    let header =
        decode_batch_header(&material.header).map_err(|_| ClaimRefusal::Material("header"))?;
    if header.network_id() != expectation.network_id {
        return Err(ClaimRefusal::NetworkMismatch {
            debit: expectation.network_id,
            header: header.network_id(),
        });
    }
    let wire = decode_merkle_proof(&material.proof).map_err(|_| ClaimRefusal::Material("proof"))?;
    let proof = Proof::new(
        wire.leaf_index(),
        wire.leaf_count(),
        wire.siblings().to_vec(),
    )
    .map_err(|_| ClaimRefusal::Material("proof"))?;
    let authorization = SequencerAuthorization::new(
        header.sequencer_id(),
        debit.sequencer_public_key,
        header.batch_number(),
        header.batch_number(),
    );
    verify_receipt(
        &material.receipt,
        &proof,
        &material.header,
        &material.header_signature,
        &authorization,
    )
    .map_err(|error| {
        ClaimRefusal::Inclusion(match error {
            InclusionError::HeaderSignature => "header_signature",
            InclusionError::Merkle(_) => "receipt_path",
            _ => "header",
        })
    })?;
    let protocol = debit
        .verified
        .receipt()
        .protocol()
        .ok_or(ClaimRefusal::Effect("receipt"))?;
    let body = protocol
        .effects()
        .get(1)
        .map(|effect| effect.body())
        .filter(|body| body.len() == WITHDRAWAL_EVENT_BYTES)
        .ok_or(ClaimRefusal::Effect("event"))?;
    if body[2..6] != expectation.network_id.to_be_bytes()
        || body[6..38] != expectation.withdrawal_id
        || body[38..70] != expectation.account
        || body[70..102] != expectation.asset_id
        || body[102..118] != expectation.amount.to_be_bytes()
        || body[118..130] != [0; 12]
        || body[130..150] != expectation.recipient.bytes()
    {
        return Err(ClaimRefusal::Effect("withdrawal"));
    }
    let mut anchor = [0_u8; 32];
    anchor.copy_from_slice(&body[150..182]);
    if anchor == [0; 32] {
        return Err(ClaimRefusal::Effect("anchor"));
    }
    let nullifier = withdrawal_nullifier(
        expectation.network_id,
        &expectation.withdrawal_id,
        &expectation.account,
        &expectation.asset_id,
        expectation.amount,
        &anchor,
    );
    if nullifier != protocol.context_hash() {
        return Err(ClaimRefusal::NullifierMismatch {
            local: nullifier,
            receipt: protocol.context_hash(),
        });
    }
    Ok(VerifiedMaterial {
        batch_number: header.batch_number(),
        state_root: header.resulting_state_root(),
        receipt_root: header.receipt_merkle_root(),
        anchor,
        nullifier,
    })
}

impl WithdrawalBoundary {
    fn validate_debit_protocol(
        &self,
        debit: &CommittedWithdrawalDebit,
    ) -> Result<(), WithdrawalError> {
        let found = debit.verified.receipt().protocol().map_or(
            0,
            layerx_intents::canonical::ProtocolReceipt::protocol_version,
        );
        if found != self.protocol_version {
            return Err(WithdrawalError::Refused(
                ClaimRefusal::DebitProtocolMismatch {
                    expected: self.protocol_version,
                    found,
                },
            ));
        }
        Ok(())
    }

    /// Validates and adopts a declared Paxeer withdrawal boundary.
    ///
    /// # Errors
    ///
    /// Refuses missing endpoints, zero depth/cadence/stall bounds, or invalid URLs.
    pub fn new(config: WithdrawalConfig) -> Result<Self, WithdrawalConfigError> {
        Self::new_for_protocol(
            config,
            layerx_intents::canonical::STATE_COMMITMENT_PROTOCOL_VERSION,
        )
    }

    /// Validates and adopts a declared Paxeer withdrawal boundary bound to one
    /// explicitly selected `LayerX` protocol version. The custody precompile
    /// only accepts state-commitment protocol withdrawal receipts, so that is
    /// the only version this boundary adopts.
    ///
    /// # Errors
    ///
    /// Refuses an unsupported protocol version before every refusal of
    /// [`Self::new`].
    pub fn new_for_protocol(
        config: WithdrawalConfig,
        protocol_version: u16,
    ) -> Result<Self, WithdrawalConfigError> {
        if protocol_version != layerx_intents::canonical::STATE_COMMITMENT_PROTOCOL_VERSION {
            return Err(WithdrawalConfigError::UnsupportedProtocolVersion);
        }
        if config.required_confirmations == 0 {
            return Err(WithdrawalConfigError::ZeroRequiredConfirmations);
        }
        if config.poll_cadence.is_zero() {
            return Err(WithdrawalConfigError::ZeroPollCadence);
        }
        if config.delayed_after_polls == 0 {
            return Err(WithdrawalConfigError::ZeroDelayedAfterPolls);
        }
        crate::finality::validate_endpoint_agreement(
            &config.endpoints,
            config.minimum_endpoint_agreement,
        )
        .map_err(WithdrawalConfigError::Agreement)?;
        let client = PaxeerClient::new(config.endpoints.clone())
            .map_err(WithdrawalConfigError::Endpoints)?;
        Ok(Self {
            protocol_version,
            client,
            endpoints: config.endpoints,
            required_confirmations: config.required_confirmations,
            poll_cadence: config.poll_cadence,
            delayed_after_polls: config.delayed_after_polls,
            minimum_endpoint_agreement: config.minimum_endpoint_agreement,
        })
    }

    /// The custody precompile every withdrawal transaction targets.
    #[must_use]
    pub const fn custody_precompile(&self) -> EvmAddress {
        CUSTODY_PRECOMPILE
    }

    #[must_use]
    pub const fn protocol_version(&self) -> u16 {
        self.protocol_version
    }

    fn finalised_root(
        &self,
        selector: [u8; 4],
        batch_number: u64,
        what: &str,
    ) -> Result<Option<[u8; 32]>, WithdrawalError> {
        let mut data = selector.to_vec();
        data.extend_from_slice(&quantity_word(&batch_number.to_be_bytes()));
        let words = exact_words(&self.call_contract(ANCHOR_PRECOMPILE, &data)?, 2, what)?;
        Ok(match word_u8(&words[1], what)? {
            0 => None,
            1 => Some(words[0]),
            other => {
                return Err(WithdrawalError::Contract {
                    detail: format!("{what}: expected boolean, got {other}"),
                })
            }
        })
    }

    /// Constructs exact `requestWithdrawal` bytes only after the receipt's
    /// inclusion under the sequencer-signed header verifies locally and Paxeer
    /// agrees the batch is finalized with the header's roots, the nullifier is
    /// unused and the asset can be released.
    ///
    /// # Errors
    ///
    /// Returns the first material, inclusion, finality, nullifier, asset or
    /// endpoint refusal.
    pub fn construct_claim(
        &self,
        debit: CommittedWithdrawalDebit,
        material: WithdrawalMaterial,
    ) -> Result<WithdrawalClaim, WithdrawalError> {
        let claim = self.assemble_claim(debit, material)?;
        let status = self.nullifier_status(claim.nullifier)?;
        if status != 0 {
            return Err(WithdrawalError::Refused(ClaimRefusal::NullifierUsed {
                nullifier: claim.nullifier,
                status,
            }));
        }
        let asset = self.asset(claim.debit.expectation.asset_id)?;
        if !asset.enabled || asset.paused {
            return Err(WithdrawalError::Refused(ClaimRefusal::AssetUnavailable {
                asset_id: claim.debit.expectation.asset_id,
            }));
        }
        Ok(claim)
    }

    /// Rebuilds the exact claim of a withdrawal that is already on the custody
    /// ledger, for a restarted or re-entered service that must re-derive the
    /// same `requestWithdrawal` bytes, claim identifier and anchor.
    ///
    /// Every proof the claim rests on is verified exactly as
    /// [`Self::construct_claim`] verifies it. Only the two admission gates of a
    /// *new* claim are absent: an already queued, paid or cancelled withdrawal
    /// has a reserved, consumed or cancelled nullifier by construction, and its
    /// asset may since have been paused. The real state of the rebuilt claim is
    /// never assumed here — it is proved by [`Self::restore_submission`],
    /// [`Self::progress`], [`Self::verify_payout`] and
    /// [`Self::verify_cancellation`], each of which reads the stored claim and
    /// its nullifier and refuses any state it was not asked for.
    ///
    /// # Errors
    ///
    /// Returns the first material, inclusion, finality or endpoint refusal.
    pub fn restore_claim(
        &self,
        debit: CommittedWithdrawalDebit,
        material: WithdrawalMaterial,
    ) -> Result<WithdrawalClaim, WithdrawalError> {
        self.assemble_claim(debit, material)
    }

    /// The material, inclusion and finalized-root verification both claim
    /// constructors share, up to and including the derived claim identifier.
    fn assemble_claim(
        &self,
        debit: CommittedWithdrawalDebit,
        material: WithdrawalMaterial,
    ) -> Result<WithdrawalClaim, WithdrawalError> {
        self.validate_debit_protocol(&debit)?;
        let material = material
            .validated()
            .map_err(|error| WithdrawalError::Refused(ClaimRefusal::Material(abi_field(&error))))?;
        let verified = verify_material(&debit, &material).map_err(WithdrawalError::Refused)?;
        let batch_number = verified.batch_number;
        let state_root = self.finalised_root(
            SELECTOR_FINALIZED_STATE_ROOT,
            batch_number,
            "finalizedStateRoot",
        )?;
        let receipt_root = self.finalised_root(
            SELECTOR_FINALIZED_RECEIPT_ROOT,
            batch_number,
            "finalizedReceiptRoot",
        )?;
        let (Some(state_root), Some(receipt_root)) = (state_root, receipt_root) else {
            return Err(WithdrawalError::Refused(ClaimRefusal::BatchNotFinalised {
                batch_number,
            }));
        };
        if state_root != verified.state_root || receipt_root != verified.receipt_root {
            return Err(WithdrawalError::Refused(
                ClaimRefusal::FinalisedRootMismatch { batch_number },
            ));
        }
        let chain_id = quantity(&self.rpc("eth_chainId", &[])?, "eth_chainId")?;
        let claim_id =
            withdrawal_claim_id(chain_id, verified.nullifier, debit.expectation.recipient);
        let calldata = material.request_calldata();
        Ok(WithdrawalClaim {
            debit,
            material,
            batch_number,
            anchor: verified.anchor,
            nullifier: verified.nullifier,
            claim_id,
            calldata,
        })
    }

    /// Creates a configured tracker for a wallet-submitted claim or permissionless payout call.
    ///
    /// # Errors
    ///
    /// Returns the tracker's declared configuration refusal.
    pub fn track(
        &self,
        transaction: TransactionHash,
    ) -> Result<FinalityTracker, TrackerConfigError> {
        FinalityTracker::new(
            TrackerConfig {
                endpoints: self.endpoints.clone(),
                minimum_endpoint_agreement: self.minimum_endpoint_agreement,
                required_confirmations: self.required_confirmations,
                poll_cadence: self.poll_cadence,
                delayed_after_polls: self.delayed_after_polls,
            },
            transaction,
        )
    }

    /// Accepts a submitted claim only after the transaction, queue event and stored claim agree.
    ///
    /// # Errors
    ///
    /// Returns a finality, execution, transaction, event or stored-state mismatch.
    pub fn accept_submission(
        &self,
        claim: WithdrawalClaim,
        report: &FinalityReport,
    ) -> Result<SubmittedWithdrawalClaim, WithdrawalError> {
        self.submission_from_queue(claim, report, Some(1))
    }

    /// Reconstructs a previously accepted submission from its immutable queue
    /// transaction after a service restart. The current claim may already be
    /// pending, paid, or cancelled, but its original queue event and every
    /// stored claim field must still bind to the supplied claim.
    ///
    /// # Errors
    ///
    /// Returns the same transaction/event/state mismatches as
    /// [`Self::accept_submission`] and refuses absent or unknown claim states.
    pub fn restore_submission(
        &self,
        claim: WithdrawalClaim,
        report: &FinalityReport,
    ) -> Result<SubmittedWithdrawalClaim, WithdrawalError> {
        self.submission_from_queue(claim, report, None)
    }

    fn submission_from_queue(
        &self,
        claim: WithdrawalClaim,
        report: &FinalityReport,
        expected_status: Option<u8>,
    ) -> Result<SubmittedWithdrawalClaim, WithdrawalError> {
        let observed = self.verify_transaction(report, claim.calldata())?;
        let event = decode_claim_queued(custody_log(
            &observed.logs,
            CLAIM_QUEUED_TOPIC,
            "ClaimQueued",
        )?)
        .map_err(|error| malformed("ClaimQueued", &error))?;
        let expectation = &claim.debit.expectation;
        if event.claim_id != claim.claim_id
            || event.nullifier != claim.nullifier
            || event.anchor != claim.anchor
            || event.asset_id != expectation.asset_id
            || event.recipient != expectation.recipient
            || event.amount != expectation.amount
        {
            return Err(WithdrawalError::MalformedEvent {
                event: "ClaimQueued",
                detail: "event fields do not bind to the constructed claim".to_owned(),
            });
        }
        let record = self.claim_record(event.claim_id)?;
        let status = expected_status.unwrap_or(record.status);
        if !matches!(status, 1..=3) {
            return Err(WithdrawalError::ClaimState {
                detail: format!("unknown or absent claim status {status}"),
            });
        }
        verify_claim_record(&claim, event.available_at, &record, status)?;
        Ok(SubmittedWithdrawalClaim {
            claim,
            claim_id: event.claim_id,
            available_at: event.available_at,
            submission_transaction: report.transaction(),
            submission_inclusion: observed.inclusion,
        })
    }

    /// Reads the current claim state without ever turning custody `paid` into user-visible
    /// payout completion before the payout transaction itself is verified.
    ///
    /// # Errors
    ///
    /// Returns malformed or contradictory custody state and endpoint failures.
    pub fn progress(
        &self,
        submitted: &SubmittedWithdrawalClaim,
    ) -> Result<ClaimProgress, WithdrawalError> {
        let record = self.claim_record(submitted.claim_id)?;
        verify_claim_record(
            &submitted.claim,
            submitted.available_at,
            &record,
            record.status,
        )?;
        match record.status {
            1 => {
                let now = self.latest_timestamp()?;
                Ok(if now < submitted.available_at {
                    ClaimProgress::WaitingForChallengeWindow {
                        available_at: submitted.available_at,
                        observed_at: now,
                        remaining: Duration::from_secs(submitted.available_at.saturating_sub(now)),
                    }
                } else {
                    ClaimProgress::ReadyToFinalise {
                        available_at: submitted.available_at,
                        observed_at: now,
                    }
                })
            }
            2 => Ok(ClaimProgress::PaidAwaitingPayoutVerification),
            3 => Ok(ClaimProgress::Cancelled {
                disposition: cancelled_disposition(submitted),
            }),
            status => Err(WithdrawalError::ClaimState {
                detail: format!("unknown or absent claim status {status}"),
            }),
        }
    }

    /// Verifies final Paxeer payout from the exact finalise call, custody state, the
    /// `ClaimFinalised` and `CustodyRelease` logs of the precompile and the
    /// independently read recipient balance.
    ///
    /// # Errors
    ///
    /// Returns finality, execution, transaction, state or evidence mismatches. No evidence means no
    /// paid-out result.
    pub fn verify_payout(
        &self,
        submitted: &SubmittedWithdrawalClaim,
        report: &FinalityReport,
    ) -> Result<PayoutEvidence, WithdrawalError> {
        let expected_input = submitted.finalise_calldata();
        let observed = self.verify_transaction(report, &expected_input)?;
        let record = self.claim_record(submitted.claim_id)?;
        verify_claim_record(&submitted.claim, submitted.available_at, &record, 2)?;
        let expectation = &submitted.claim.debit.expectation;
        let finalised = decode_claim_finalised(custody_log(
            &observed.logs,
            CLAIM_FINALISED_TOPIC,
            "ClaimFinalised",
        )?)
        .map_err(|error| malformed("ClaimFinalised", &error))?;
        if finalised.claim_id != submitted.claim_id
            || finalised.nullifier != submitted.claim.nullifier
        {
            return Err(WithdrawalError::MalformedEvent {
                event: "ClaimFinalised",
                detail: "indexed identifiers do not bind to the claim".to_owned(),
            });
        }
        let release = decode_custody_release(custody_log(
            &observed.logs,
            CUSTODY_RELEASE_TOPIC,
            "CustodyRelease",
        )?)
        .map_err(|error| malformed("CustodyRelease", &error))?;
        if release.claim_id != submitted.claim_id
            || release.asset_id != expectation.asset_id
            || release.recipient != expectation.recipient
            || release.amount != expectation.amount
            || release.settlement_module != CUSTODY_PRECOMPILE
        {
            return Err(WithdrawalError::PayoutNotVerified {
                detail: "custody release does not bind to the claim".to_owned(),
            });
        }
        let asset = self.asset(expectation.asset_id)?;
        let balance = if asset.pointer.bytes() == [0; 20] {
            let native = exact_word(
                &self.call_contract(CUSTODY_PRECOMPILE, &native_asset_id_calldata())?,
                "nativeAssetId",
            )?;
            if native != expectation.asset_id {
                return Err(WithdrawalError::PayoutNotVerified {
                    detail: "asset has no pointer and is not the native asset".to_owned(),
                });
            }
            let wei = quantity_bytes(
                &self.rpc(
                    "eth_getBalance",
                    &[
                        Json::Text(bytes_hex(&expectation.recipient.bytes())),
                        Json::Text("latest".to_owned()),
                    ],
                )?,
                "eth_getBalance",
            )?;
            if wei[..16] == [0; 16] {
                word_u128(&wei, "eth_getBalance")? / WEI_PER_BASE_UNIT
            } else {
                u128::MAX
            }
        } else {
            let mut data = SELECTOR_BALANCE_OF.to_vec();
            data.extend_from_slice(&address_word(expectation.recipient));
            word_u128(
                &exact_word(&self.call_contract(asset.pointer, &data)?, "balanceOf")?,
                "balanceOf",
            )?
        };
        if balance < expectation.amount {
            return Err(WithdrawalError::PayoutNotVerified {
                detail: "recipient balance is below the released amount".to_owned(),
            });
        }
        self.verify_nullifier(submitted.claim.nullifier, 2)?;
        Ok(PayoutEvidence {
            debit_receipt_reference: submitted.claim.debit.receipt_reference,
            checkpoint_hash: submitted.claim.anchor,
            claim_id: submitted.claim_id,
            payout_transaction: report.transaction(),
            payout_inclusion: observed.inclusion,
            vault: CUSTODY_PRECOMPILE,
            token: asset.pointer,
            asset_id: expectation.asset_id,
            recipient: expectation.recipient,
            amount: expectation.amount,
        })
    }

    /// Verifies that the custody authority cancelled the pending claim: the
    /// stored claim is cancelled, its nullifier is terminally cancelled and
    /// the asset's custody still covers it. Cancellation is a module message,
    /// so the evidence is the endpoint-agreed custody state, not a transaction.
    ///
    /// # Errors
    ///
    /// Returns claim-state, nullifier or endpoint mismatches.
    pub fn verify_cancellation(
        &self,
        submitted: &SubmittedWithdrawalClaim,
    ) -> Result<CancellationEvidence, WithdrawalError> {
        let record = self.claim_record(submitted.claim_id)?;
        verify_claim_record(&submitted.claim, submitted.available_at, &record, 3)?;
        self.verify_nullifier(submitted.claim.nullifier, 3)?;
        let observed_head = quantity(&self.rpc("eth_blockNumber", &[])?, "eth_blockNumber")?;
        Ok(CancellationEvidence {
            debit_receipt_reference: submitted.claim.debit.receipt_reference,
            checkpoint_hash: submitted.claim.anchor,
            claim_id: submitted.claim_id,
            observed_head,
            disposition: cancelled_disposition(submitted),
        })
    }

    fn verify_transaction(
        &self,
        report: &FinalityReport,
        expected_input: &[u8],
    ) -> Result<ObservedTransaction, WithdrawalError> {
        let (tracked, confirmations) = match report.stage() {
            FinalityStage::Final {
                inclusion,
                confirmations,
                ..
            } => (inclusion, confirmations),
            FinalityStage::Displaced {
                lost,
                head,
                requeued,
            } => {
                return Err(WithdrawalError::Displaced {
                    lost,
                    head,
                    requeued,
                })
            }
            stage => return Err(WithdrawalError::NotFinal { stage }),
        };
        if confirmations < self.required_confirmations {
            return Err(WithdrawalError::NotFinal {
                stage: report.stage(),
            });
        }
        if tracked.execution == ExecutionOutcome::Reverted {
            return Err(WithdrawalError::Reverted { inclusion: tracked });
        }
        let evidence = report
            .evidence()
            .ok_or(WithdrawalError::MissingQuorumEvidence)?;
        if evidence.binding() != &self.client.quorum_binding(self.minimum_endpoint_agreement) {
            return Err(WithdrawalError::EvidenceSourceMismatch);
        }
        let observed_inclusion = match evidence.transaction() {
            crate::client::TransactionView::Included(inclusion) => inclusion,
            crate::client::TransactionView::Unknown | crate::client::TransactionView::Pending => {
                return Err(WithdrawalError::InclusionChanged {
                    tracked: tracked.block,
                    observed: None,
                });
            }
        };
        if observed_inclusion != tracked {
            return Err(WithdrawalError::InclusionChanged {
                tracked: tracked.block,
                observed: Some(observed_inclusion.block),
            });
        }
        if evidence.canonical_block() != Some(tracked.block) {
            return Err(WithdrawalError::InclusionChanged {
                tracked: tracked.block,
                observed: evidence.canonical_block(),
            });
        }
        let observed_confirmations = evidence
            .head()
            .saturating_sub(tracked.block.number)
            .saturating_add(1);
        if observed_confirmations < self.required_confirmations {
            return Err(WithdrawalError::NotFinal {
                stage: report.stage(),
            });
        }
        let transaction = self.transaction(report.transaction())?;
        if transaction.to != Some(CUSTODY_PRECOMPILE) {
            return Err(WithdrawalError::TransactionTarget {
                expected: CUSTODY_PRECOMPILE,
                found: transaction.to,
            });
        }
        if transaction.input != expected_input {
            return Err(WithdrawalError::TransactionInput);
        }
        if transaction.value.iter().any(|byte| *byte != 0) {
            return Err(WithdrawalError::TransactionValue);
        }
        let logs = evidence
            .receipt_logs()
            .ok_or(WithdrawalError::MissingQuorumEvidence)?
            .iter()
            .map(|log| LogRecord {
                address: log.address,
                topics: log.topics.clone(),
                data: log.data.clone(),
            })
            .collect();
        Ok(ObservedTransaction {
            inclusion: observed_inclusion,
            logs,
        })
    }

    fn claim_record(&self, claim_id: [u8; 32]) -> Result<CustodyClaim, WithdrawalError> {
        decode_claim(&self.call_contract(CUSTODY_PRECOMPILE, &get_claim_calldata(claim_id))?)
            .map_err(|error| contract("getClaim", &error))
    }

    fn asset(&self, asset_id: [u8; 32]) -> Result<CustodyAsset, WithdrawalError> {
        let asset =
            decode_asset(&self.call_contract(CUSTODY_PRECOMPILE, &get_asset_calldata(asset_id))?)
                .map_err(|error| contract("getAsset", &error))?;
        if asset.asset_id != asset_id {
            return Err(WithdrawalError::Contract {
                detail: "getAsset: record names a different asset".to_owned(),
            });
        }
        Ok(asset)
    }

    fn nullifier_status(&self, nullifier: [u8; 32]) -> Result<u8, WithdrawalError> {
        decode_status(
            &self.call_contract(CUSTODY_PRECOMPILE, &nullifier_status_calldata(nullifier))?,
        )
        .map_err(|error| contract("nullifierStatus", &error))
    }

    fn verify_nullifier(&self, nullifier: [u8; 32], expected: u8) -> Result<(), WithdrawalError> {
        let status = self.nullifier_status(nullifier)?;
        if status == expected {
            Ok(())
        } else {
            Err(WithdrawalError::ClaimState {
                detail: format!("nullifier status {status}, expected {expected}"),
            })
        }
    }

    fn latest_timestamp(&self) -> Result<u64, WithdrawalError> {
        let block = self.rpc(
            "eth_getBlockByNumber",
            &[Json::Text("latest".to_owned()), Json::Bool(false)],
        )?;
        let timestamp = required(&block, "timestamp")?;
        quantity(timestamp, "block.timestamp")
    }

    fn transaction(&self, transaction: TransactionHash) -> Result<ObservedCall, WithdrawalError> {
        let value = self.rpc(
            "eth_getTransactionByHash",
            &[Json::Text(transaction.to_hex())],
        )?;
        if value.is_null() {
            return Err(WithdrawalError::Contract {
                detail: "transaction disappeared after finality".to_owned(),
            });
        }
        if fixed::<32>(required(&value, "hash")?, "transaction.hash")? != transaction.bytes() {
            return Err(WithdrawalError::Contract {
                detail: "transaction hash does not match its request".to_owned(),
            });
        }
        let to = match value.member("to") {
            None | Some(Json::Null) => None,
            Some(word) => Some(EvmAddress::new(fixed::<20>(word, "transaction.to")?)),
        };
        Ok(ObservedCall {
            to,
            input: variable_bytes(required(&value, "input")?, "transaction.input")?,
            value: quantity_bytes(required(&value, "value")?, "transaction.value")?,
        })
    }

    fn call_contract(&self, contract: EvmAddress, data: &[u8]) -> Result<Vec<u8>, WithdrawalError> {
        let result = self.rpc(
            "eth_call",
            &[
                Json::Object(vec![
                    ("to".to_owned(), Json::Text(bytes_hex(&contract.bytes()))),
                    ("data".to_owned(), Json::Text(bytes_hex(data))),
                ]),
                Json::Text("latest".to_owned()),
            ],
        )?;
        variable_bytes(&result, "eth_call result")
    }

    fn rpc(&self, method: &str, params: &[Json]) -> Result<Json, WithdrawalError> {
        self.client
            .agreed_call(method, params, self.minimum_endpoint_agreement)
            .map_err(WithdrawalError::Endpoint)
    }
}

struct ObservedCall {
    to: Option<EvmAddress>,
    input: Vec<u8>,
    value: [u8; 32],
}

struct ObservedTransaction {
    inclusion: TransactionInclusion,
    logs: Vec<LogRecord>,
}

pub(crate) const fn supported_protocol_version(protocol_version: u16) -> bool {
    matches!(
        protocol_version,
        layerx_intents::canonical::PROTOCOL_VERSION
            | layerx_intents::canonical::STATE_COMMITMENT_PROTOCOL_VERSION
    )
}

fn validate_debit_expectation(expectation: &DebitExpectation) -> Result<(), DebitFault> {
    for (name, value) in [
        ("activity_id", expectation.activity_id),
        ("withdrawal_id", expectation.withdrawal_id),
        ("account", expectation.account),
        ("withdrawals_account", expectation.withdrawals_account),
        ("asset_id", expectation.asset_id),
    ] {
        if value == [0; 32] {
            return Err(DebitFault::EmptyField(name));
        }
    }
    if expectation.network_id == 0 {
        return Err(DebitFault::EmptyField("network_id"));
    }
    if expectation.amount == 0 {
        return Err(DebitFault::EmptyField("amount"));
    }
    if expectation.recipient.bytes() == [0; 20] {
        return Err(DebitFault::EmptyField("recipient"));
    }
    Ok(())
}

const fn abi_field(error: &CustodyAbiError) -> &'static str {
    match error {
        CustodyAbiError::EvidenceBounds(field) | CustodyAbiError::Layout(field) => field,
        CustodyAbiError::WeiRemainder | CustodyAbiError::AmountOverflow => "amount",
    }
}

fn contract(what: &str, error: &CustodyAbiError) -> WithdrawalError {
    WithdrawalError::Contract {
        detail: format!("{what}: {error:?}"),
    }
}

fn malformed(event: &'static str, error: &CustodyAbiError) -> WithdrawalError {
    WithdrawalError::MalformedEvent {
        event,
        detail: format!("{error:?}"),
    }
}

fn custody_log<'a>(
    logs: &'a [LogRecord],
    topic: [u8; 32],
    name: &'static str,
) -> Result<&'a LogRecord, WithdrawalError> {
    let mut matches = logs
        .iter()
        .filter(|log| log.address == CUSTODY_PRECOMPILE && log.topics.first() == Some(&topic));
    let first = matches.next().ok_or(WithdrawalError::MissingEvent(name))?;
    if matches.next().is_some() {
        return Err(WithdrawalError::DuplicateEvent(name));
    }
    Ok(first)
}

fn cancelled_disposition(submitted: &SubmittedWithdrawalClaim) -> CancelledFundsDisposition {
    CancelledFundsDisposition {
        paxeer: PaxeerFundsDisposition::RetainedInVault {
            vault: CUSTODY_PRECOMPILE,
            asset_id: submitted.claim.debit.expectation.asset_id,
            amount: submitted.claim.debit.expectation.amount,
        },
        layerx: ProtocolDebitDisposition::RemainsCommittedPendingProtocolRecovery {
            debit_receipt_reference: submitted.claim.debit.receipt_reference,
        },
    }
}

fn verify_claim_record(
    claim: &WithdrawalClaim,
    available_at: u64,
    record: &CustodyClaim,
    expected_status: u8,
) -> Result<(), WithdrawalError> {
    let expectation = &claim.debit.expectation;
    if record.claim_id != claim.claim_id
        || record.kind != 1
        || record.nullifier != claim.nullifier
        || record.withdrawal_id != expectation.withdrawal_id
        || record.account != expectation.account
        || record.asset_id != expectation.asset_id
        || record.recipient != expectation.recipient
        || record.amount != expectation.amount
        || record.batch_number != claim.batch_number
        || record.anchor != claim.anchor
        || record.available_at != available_at
    {
        return Err(WithdrawalError::ClaimState {
            detail: "stored claim does not bind to the constructed claim".to_owned(),
        });
    }
    if record.status != expected_status {
        return Err(WithdrawalError::ClaimState {
            detail: format!("claim status {}, expected {expected_status}", record.status),
        });
    }
    Ok(())
}

fn quantity_word(bytes: &[u8]) -> [u8; 32] {
    let mut word = [0_u8; 32];
    for (slot, byte) in word
        .iter_mut()
        .skip(WORD.saturating_sub(bytes.len()))
        .zip(bytes)
    {
        *slot = *byte;
    }
    word
}

fn address_word(address: EvmAddress) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[12..].copy_from_slice(&address.bytes());
    word
}

fn required<'a>(value: &'a Json, name: &str) -> Result<&'a Json, WithdrawalError> {
    value.member(name).ok_or_else(|| WithdrawalError::Contract {
        detail: format!("missing {name}"),
    })
}

fn quantity(value: &Json, what: &str) -> Result<u64, WithdrawalError> {
    let bytes = quantity_bytes(value, what)?;
    word_u64(&bytes, what)
}

fn quantity_bytes(value: &Json, what: &str) -> Result<[u8; 32], WithdrawalError> {
    let text = value.as_text().ok_or_else(|| WithdrawalError::Contract {
        detail: format!("{what}: expected hex quantity"),
    })?;
    let digits = text
        .strip_prefix("0x")
        .ok_or_else(|| WithdrawalError::Contract {
            detail: format!("{what}: missing 0x prefix"),
        })?;
    if digits.is_empty() || digits.len() > 64 {
        return Err(WithdrawalError::Contract {
            detail: format!("{what}: invalid quantity width"),
        });
    }
    let mut normalized = String::new();
    if digits.len() % 2 != 0 {
        normalized.push('0');
    }
    normalized.push_str(digits);
    let bytes = decode_digits(&normalized, what)?;
    let mut word = [0_u8; 32];
    word[WORD.saturating_sub(bytes.len())..].copy_from_slice(&bytes);
    Ok(word)
}

fn fixed<const N: usize>(value: &Json, what: &str) -> Result<[u8; N], WithdrawalError> {
    let bytes = variable_bytes(value, what)?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| WithdrawalError::Contract {
            detail: format!("{what}: expected {N} bytes, got {}", bytes.len()),
        })
}

fn variable_bytes(value: &Json, what: &str) -> Result<Vec<u8>, WithdrawalError> {
    let text = value.as_text().ok_or_else(|| WithdrawalError::Contract {
        detail: format!("{what}: expected hex bytes"),
    })?;
    let digits = text
        .strip_prefix("0x")
        .ok_or_else(|| WithdrawalError::Contract {
            detail: format!("{what}: missing 0x prefix"),
        })?;
    if digits.len() % 2 != 0 {
        return Err(WithdrawalError::Contract {
            detail: format!("{what}: odd hex length"),
        });
    }
    decode_digits(digits, what)
}

fn decode_digits(digits: &str, what: &str) -> Result<Vec<u8>, WithdrawalError> {
    digits
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = hex_nibble(pair[0]);
            let low = hex_nibble(pair[1]);
            match (high, low) {
                (Some(high), Some(low)) => Ok((high << 4) | low),
                _ => Err(WithdrawalError::Contract {
                    detail: format!("{what}: non-hex digit"),
                }),
            }
        })
        .collect()
}

const fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn bytes_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut text = String::from("0x");
    for byte in bytes {
        text.push(char::from(DIGITS[usize::from(byte >> 4)]));
        text.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    text
}

fn exact_word(bytes: &[u8], what: &str) -> Result<[u8; 32], WithdrawalError> {
    bytes.try_into().map_err(|_| WithdrawalError::Contract {
        detail: format!("{what}: expected one word, got {} bytes", bytes.len()),
    })
}

fn exact_words(bytes: &[u8], count: usize, what: &str) -> Result<Vec<[u8; 32]>, WithdrawalError> {
    if bytes.len() != count.saturating_mul(WORD) {
        return Err(WithdrawalError::Contract {
            detail: format!("{what}: expected {count} words, got {} bytes", bytes.len()),
        });
    }
    Ok(bytes_words(bytes))
}

fn bytes_words(bytes: &[u8]) -> Vec<[u8; 32]> {
    bytes
        .chunks_exact(WORD)
        .map(|chunk| {
            let mut word = [0_u8; 32];
            word.copy_from_slice(chunk);
            word
        })
        .collect()
}

fn word_u8(word: &[u8; 32], what: &str) -> Result<u8, WithdrawalError> {
    if word[..31].iter().any(|byte| *byte != 0) {
        return Err(WithdrawalError::Contract {
            detail: format!("{what}: value exceeds u8"),
        });
    }
    Ok(word[31])
}

fn word_u64(word: &[u8; 32], what: &str) -> Result<u64, WithdrawalError> {
    if word[..24].iter().any(|byte| *byte != 0) {
        return Err(WithdrawalError::Contract {
            detail: format!("{what}: value exceeds u64"),
        });
    }
    let mut bytes = [0_u8; 8];
    bytes.copy_from_slice(&word[24..]);
    Ok(u64::from_be_bytes(bytes))
}

fn word_u128(word: &[u8; 32], what: &str) -> Result<u128, WithdrawalError> {
    if word[..16].iter().any(|byte| *byte != 0) {
        return Err(WithdrawalError::Contract {
            detail: format!("{what}: value exceeds u128"),
        });
    }
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&word[16..]);
    Ok(u128::from_be_bytes(bytes))
}
