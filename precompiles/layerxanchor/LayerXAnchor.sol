// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant LAYERX_ANCHOR_PRECOMPILE_ADDRESS = 0x0000000000000000000000000000000000001014;

ILayerXAnchor constant LAYERX_ANCHOR_CONTRACT = ILayerXAnchor(LAYERX_ANCHOR_PRECOMPILE_ADDRESS);

/// Native LayerX anchor: checkpoint registry, finality authority, availability
/// record and guarantor bonds. Certificates are self-verifying, so anyone may
/// submit them. A checkpoint is final once the required threshold of bonded,
/// active guarantors attested it, it continues the finalized chain, no
/// challenge is open and the challenge window elapsed.
///
/// Bond amounts are in the chain base denomination (1 unit = 1e12 wei); a
/// payable call must carry a value that is a whole number of base units.
interface ILayerXAnchor {
    struct Checkpoint {
        uint64 batchNumber;
        bytes32 checkpointId;
        bytes32 headerDigest;
        uint64 epoch;
        uint64 firstSequence;
        uint64 lastSequence;
        bytes32 previousStateRoot;
        bytes32 stateRoot;
        bytes32 receiptRoot;
        bytes32 dataAvailabilityRoot;
        bytes32 sequencerId;
        uint64 timestampMs;
        uint8 status;
        uint8 signers;
        uint8 availabilityMask;
        uint32 openChallenges;
        uint64 submittedHeight;
        uint64 finalizedHeight;
    }

    struct Guarantor {
        bytes32 guarantorId;
        address signer;
        address operator;
        uint256 bond;
        uint256 unbonding;
        uint8 status;
        bool eligible;
    }

    event CheckpointSubmitted(uint64 indexed batchNumber, bytes32 indexed checkpointId, bytes32 stateRoot, bytes32 receiptRoot, uint8 signers);
    event CheckpointFinalized(uint64 indexed batchNumber, bytes32 indexed checkpointId, bytes32 stateRoot, bytes32 receiptRoot);
    event AvailabilityAttested(uint64 indexed batchNumber, bytes32 indexed guarantorId, uint8 classMask, uint8 availabilityMask);
    event GuarantorRegistered(bytes32 indexed guarantorId, address indexed signer, address operator, uint256 bond, uint8 status);
    event GuarantorActivated(bytes32 indexed guarantorId);
    event BondIncreased(bytes32 indexed guarantorId, uint256 amount, uint256 bond);
    event UnbondBegun(bytes32 indexed guarantorId, uint256 amount, uint64 completionTime);
    event UnbondCompleted(bytes32 indexed guarantorId, uint256 amount);
    event GuarantorSlashed(bytes32 indexed guarantorId, uint8 reason, uint64 batchNumber, uint256 amount, address reporter, uint256 reporterReward);
    event ChallengeOpened(uint64 indexed challengeId, uint64 indexed batchNumber, uint8 kind, bytes32 evidenceHash, address challenger);
    event ChallengeResolved(uint64 indexed challengeId, uint64 indexed batchNumber, bool upheld);
    event SequencerAuthorized(bytes32 indexed sequencerId, bytes32 publicKey, uint64 firstBatchNumber, uint64 lastBatchNumber);

    /// header: canonical 354-byte batch header. headerSignature: the 64-byte
    /// Ed25519 sequencer signature. certificate: the guarantor checkpoint
    /// certificate (header, validity proof, up to 32 attestations ascending by
    /// guarantor identifier, threshold, settlement reference).
    /// status: 1 submitted, 2 final.
    function submitCheckpoint(bytes calldata header, bytes calldata headerSignature, bytes calldata certificate) external returns (bytes32 checkpointId, uint8 status);

    /// attestation: one 274-byte guarantor attestation over a known checkpoint.
    function submitAvailabilityAttestation(bytes calldata attestation) external returns (uint8 availabilityMask);

    /// Finalizes a submitted checkpoint whose challenge or window has cleared.
    function finalize(uint64 batchNumber) external returns (bool);

    function registerGuarantor(bytes32 guarantorId, address signer) external payable returns (bool);

    function increaseBond(bytes32 guarantorId) external payable returns (bool);

    function beginUnbond(bytes32 guarantorId, uint256 amount) external returns (uint64 completionTime);

    function completeUnbond(bytes32 guarantorId) external returns (uint256 amount);

    /// evidenceA, evidenceB: two 274-byte attestations by one guarantor naming
    /// different checkpoints for one batch.
    function submitEquivocation(bytes calldata evidenceA, bytes calldata evidenceB) external returns (uint256 slashed);

    /// kind: 0 fraud, 1 data availability. The value must be the challenge bond.
    function openChallenge(uint64 batchNumber, uint8 kind, bytes32 evidenceHash) external payable returns (uint64 challengeId);

    /// Authority only.
    function resolveChallenge(uint64 challengeId, bool upheld) external returns (bool);

    /// Authority only.
    function activateGuarantor(bytes32 guarantorId) external returns (bool);

    /// Authority only.
    function setSequencerAuthorization(bytes32 sequencerId, bytes32 publicKey, uint64 firstBatchNumber, uint64 lastBatchNumber) external returns (bool);

    function latestFinalized() external view returns (uint64 batchNumber, bool exists);

    function checkpoint(uint64 batchNumber) external view returns (Checkpoint memory);

    function finalizedStateRoot(uint64 batchNumber) external view returns (bytes32 stateRoot, bool finalized);

    function finalizedReceiptRoot(uint64 batchNumber) external view returns (bytes32 receiptRoot, bool finalized);

    function guarantor(bytes32 guarantorId) external view returns (Guarantor memory);

    function threshold() external view returns (uint32);

    /// 0 unknown, 1 submitted, 2 final.
    function statusOf(uint64 batchNumber) external view returns (uint8);

    /// The batch a checkpoint identifier is recorded for and its status; status 0 when it is not recorded.
    function checkpointBatch(bytes32 checkpointId) external view returns (uint64 batchNumber, uint8 status);

    /// The guarantors whose attestations the checkpoint was admitted with, in certificate order.
    function checkpointGuarantors(uint64 batchNumber) external view returns (bytes32[] memory guarantorIds);
}
