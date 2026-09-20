// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

address constant LAYERX_CUSTODY_PRECOMPILE_ADDRESS = 0x0000000000000000000000000000000000001013;

ILayerXCustody constant LAYERX_CUSTODY_CONTRACT = ILayerXCustody(LAYERX_CUSTODY_PRECOMPILE_ADDRESS);

/// Native LayerX custody. Funds are held by the layerxcustody module account
/// and leave it only against verified LayerX evidence: a sequencer-signed
/// withdrawal receipt included in a finalized batch, or a native state proof of
/// a balance under the latest finalized state root plus the account
/// authority's recipient signature. No attestor key exists on either path.
///
/// Amounts are bank base units of the asset's denom, one to one with the
/// LayerX u128 amount. deposit() refuses a msg.value that is not a whole number
/// of base units.
///
/// beneficiary is the 32-byte LayerX account identifier, opaque to Paxeer and
/// non-zero, exactly as LayerXVault carried it.
///
/// Identifiers keep the Solidity formulas with address(this) = this precompile:
///   depositId = sha256(abi.encode("LXP/Paxeer/custody-deposit/v1", chainid, this, payer, assetId, beneficiary, amount, nonce))
///   claimId   = sha256(abi.encode("LXP/Paxeer/withdrawal-claim/v1", chainid, this, nullifier, recipient))
///   exit claimId = sha256(abi.encode("LXP/Paxeer/emergency-exit/v1", chainid, this, nullifier))
///
/// Gas = 3000 + 16 * len(calldata after the selector) + 4000 * signatures
///     + 100 * proofNodes + 5000 * writes
/// signatures: 2 for requestWithdrawal and finaliseWithdrawal, 1 for
/// requestForcedExit and executeForcedExit, 0 otherwise. proofNodes:
/// len(proof) / 32 for withdrawals, len(witness) / 32 for forced exits.
/// writes: 8 for deposit and depositToken, 6 for the request methods, 12 for
/// finaliseWithdrawal and executeForcedExit, 0 for views.
interface ILayerXCustody {
    struct DepositRecord {
        bytes32 depositId;
        uint64 index;
        address depositor;
        bytes32 beneficiary;
        bytes32 assetId;
        string denom;
        uint256 amount;
        uint64 nonce;
        uint64 height;
    }

    /// kind: 1 withdrawal, 2 forced exit. status: 0 none, 1 pending, 2 paid, 3 cancelled.
    struct Claim {
        bytes32 claimId;
        uint8 kind;
        uint8 status;
        bytes32 nullifier;
        bytes32 withdrawalId;
        bytes32 account;
        bytes32 assetId;
        string denom;
        address recipient;
        uint256 amount;
        uint64 batchNumber;
        bytes32 anchor;
        uint64 availableAt;
    }

    struct Asset {
        bytes32 assetId;
        string denom;
        address pointer;
        bool enabled;
        bool paused;
        uint256 minimumDeposit;
        uint256 custodyCap;
        uint256 custodied;
        uint256 released;
        uint256 pending;
    }

    event CustodyDeposit(
        bytes32 indexed depositId,
        bytes32 indexed assetId,
        address indexed payer,
        bytes32 beneficiary,
        uint256 amount,
        uint64 nonce
    );
    event ClaimQueued(
        bytes32 indexed claimId,
        bytes32 indexed nullifier,
        bytes32 indexed checkpointHash,
        bytes32 assetId,
        address recipient,
        uint256 amount,
        uint64 availableAt
    );
    event ClaimFinalised(bytes32 indexed claimId, bytes32 indexed nullifier);
    event CustodyRelease(
        bytes32 indexed claimId,
        bytes32 indexed assetId,
        address indexed recipient,
        uint256 amount,
        address settlementModule
    );
    event EmergencyExitExecuted(
        bytes32 indexed claimId,
        bytes32 indexed nullifier,
        bytes32 indexed checkpointHash,
        bytes32 account,
        bytes32 assetId,
        address recipient,
        uint256 amount
    );

    event DepositRootRegistered(
        bytes32 indexed checkpointId, bytes32 indexed depositRoot, bytes32 commitment, uint16 version
    );

    /// Custody msg.value of the native coin for a LayerX account.
    function deposit(bytes32 beneficiary) external payable returns (bytes32 depositId);

    /// Custody a bank denom addressed by its registered ERC20 pointer.
    function depositToken(address pointer, uint256 amount, bytes32 beneficiary) external returns (bytes32 depositId);

    /// Verify a withdrawal and queue its claim; payable after the withdrawal delay.
    function requestWithdrawal(
        bytes calldata receipt,
        bytes calldata proof,
        bytes calldata header,
        bytes calldata headerSignature
    ) external returns (bytes32 claimId, uint64 availableAt);

    /// Re-verify a withdrawal and pay the recipient the receipt names. Queues
    /// the claim first when it was never requested.
    function finaliseWithdrawal(
        bytes calldata receipt,
        bytes calldata proof,
        bytes calldata header,
        bytes calldata headerSignature
    ) external returns (bytes32 claimId);

    /// Prove a whole balance under the latest finalized state root and queue its exit.
    function requestForcedExit(
        bytes calldata witness,
        uint64 batchNumber,
        bytes32 account,
        bytes32 assetId,
        address recipient,
        bytes calldata recipientSignature
    ) external returns (bytes32 claimId, uint64 availableAt);

    /// Pay a forced exit, queueing it first when it was never requested.
    function executeForcedExit(
        bytes calldata witness,
        uint64 batchNumber,
        bytes32 account,
        bytes32 assetId,
        address recipient,
        bytes calldata recipientSignature
    ) external returns (bytes32 claimId);

    function depositCount() external view returns (uint64 count);

    function depositNonce(address depositor, bytes32 assetId) external view returns (uint64 nonce);

    function getDeposit(bytes32 depositId) external view returns (DepositRecord memory record);

    function getDepositByIndex(uint64 index) external view returns (DepositRecord memory record);

    function getClaim(bytes32 claimId) external view returns (Claim memory claim);

    /// 0 none, 1 reserved, 2 consumed, 3 cancelled.
    function nullifierStatus(bytes32 nullifier) external view returns (uint8 status);

    function getAsset(bytes32 assetId) external view returns (Asset memory asset);

    function assetByPointer(address pointer) external view returns (bytes32 assetId);

    function nativeAssetId() external view returns (bytes32 assetId);

    function exitEligible() external view returns (bool eligible);

    /// Record the deposit root of a finalized checkpoint. Only the account that
    /// submitted the checkpoint to layerxAnchor may call. `registration` is
    /// "LX:PAXEER:DEPOSIT:ROOT:v1" followed by checkpointId, stateRoot,
    /// depositRoot, custodyReference, network (4 bytes) and protocol version
    /// (2 bytes); `signature` is the deposit root authority's Ed25519 signature
    /// over it. `leafOrdering` is committed to, not verified.
    function registerDepositRoot(bytes calldata registration, bytes calldata signature, bytes32[] calldata leafOrdering)
        external;

    /// Ed25519 key that signs deposit root registrations; zero when unset.
    function depositRootAuthority() external view returns (bytes32 authority);

    /// Registered deposit root of a checkpoint; zero when none.
    function depositRootRegistered(bytes32 checkpointId) external view returns (bytes32 depositRoot);

    /// SHA-256 of abi.encode(uint16 2, registration, signature, leafOrdering); zero when none.
    function depositRegistrationDigest(bytes32 checkpointId) external view returns (bytes32 commitment);
}
