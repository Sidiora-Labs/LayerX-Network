// SPDX-License-Identifier: Apache-2.0
pragma solidity 0.8.30;

import {Ownable} from "@openzeppelin/contracts/access/Ownable.sol";
import {Ownable2Step} from "@openzeppelin/contracts/access/Ownable2Step.sol";
import {IERC20} from "@openzeppelin/contracts/token/ERC20/IERC20.sol";
import {SafeERC20} from "@openzeppelin/contracts/token/ERC20/utils/SafeERC20.sol";
import {ReentrancyGuard} from "@openzeppelin/contracts/utils/ReentrancyGuard.sol";

import {BridgeAttestation} from "./BridgeAttestation.sol";

/// @notice Foreign-chain custody half of the PaxeerX bridge. Deposits lock
/// ERC20 or the chain's native coin and emit BridgeDeposit for the Paxeer side
/// to mint against. Releases pay out locked funds once a threshold of attestors
/// has signed the outbound digest for a Paxeer burn. Not upgradeable: no proxy
/// surface and no EIP-165 surface. Ownership moves in two steps.
contract PaxeerXVault is Ownable2Step, ReentrancyGuard {
    using SafeERC20 for IERC20;

    address public constant NATIVE_ASSET = address(0);
    uint256 private constant SECP256K1_HALF_ORDER = 0x7fffffffffffffffffffffffffffffff5d576e7357a4501ddfe92f46681b20a0;

    struct AssetCap {
        uint256 perTx;
        uint256 total;
    }

    bool public paused;
    uint64 public depositNonce;
    uint256 public threshold;
    address[] private attestorList;
    mapping(address => bool) public isAttestor;
    mapping(address => AssetCap) public caps;
    mapping(address => uint256) public outstanding;
    mapping(bytes32 => bool) public nullified;
    /// @notice Every asset the owner has ever capped, so the rescue path can
    /// tell an asset this bridge carries from a token that arrived by accident.
    mapping(address => bool) public registered;

    event BridgeDeposit(
        address indexed asset, uint256 amount, address indexed sender, bytes32 indexed paxeerRecipient, uint64 nonce
    );
    event BridgeRelease(
        address indexed asset,
        uint256 amount,
        address indexed recipient,
        bytes32 indexed paxeerTxHash,
        uint64 paxeerNonce
    );
    event AttestorsSet(address[] attestors, uint256 threshold);
    event CapSet(address indexed asset, uint256 perTx, uint256 total);
    event AssetRegistered(address indexed asset);
    event Rescued(address indexed token, address indexed to, uint256 amount);
    event Paused(address account);
    event Unpaused(address account);

    error WhenPaused();
    error NotPaused();
    error ZeroAmount();
    error InvalidRecipient();
    error InvalidAttestor(address attestor);
    error InvalidThreshold(uint256 threshold, uint256 attestors);
    error AssetNotEnabled(address asset);
    error PerTxCapExceeded(uint256 amount, uint256 perTx);
    error TotalCapExceeded(uint256 outstandingAfter, uint256 total);
    error InsufficientOutstanding(uint256 amount, uint256 outstanding);
    error NullifierUsed(bytes32 nullifier);
    error BelowThreshold(uint256 signatures, uint256 threshold);
    error InvalidSignature();
    error SignersNotAscending(address signer);
    error UnknownSigner(address signer);
    error UseDepositNative();
    error TransferAmountMismatch(uint256 received, uint256 amount);
    error NativeTransferFailed();
    error NativeAssetNotRescuable();
    error OutstandingNotZero(address token, uint256 outstanding);
    error AssetRegisteredForBridging(address token);
    error NothingToRescue(address token);

    modifier whenNotPaused() {
        if (paused) revert WhenPaused();
        _;
    }

    constructor(address owner_, address[] memory attestors_, uint256 threshold_) Ownable(owner_) {
        _setAttestors(attestors_, threshold_);
    }

    function pause() external onlyOwner {
        if (paused) revert WhenPaused();
        paused = true;
        emit Paused(msg.sender);
    }

    function unpause() external onlyOwner {
        if (!paused) revert NotPaused();
        paused = false;
        emit Unpaused(msg.sender);
    }

    function setAttestors(address[] calldata attestors_, uint256 threshold_) external onlyOwner {
        _setAttestors(attestors_, threshold_);
    }

    /// @notice Sets the per-transaction cap (deposits and releases) and the
    /// cap on the total amount locked for an asset. A zero total disables new
    /// deposits while releases of already locked funds stay possible. Naming an
    /// asset here registers it for bridging for good, so the rescue path can
    /// never reach it again.
    function setCap(address asset, uint256 perTx, uint256 total) external onlyOwner {
        if (!registered[asset]) {
            registered[asset] = true;
            emit AssetRegistered(asset);
        }
        caps[asset] = AssetCap({perTx: perTx, total: total});
        emit CapSet(asset, perTx, total);
    }

    /// @notice Moves the whole balance of a token that holds nothing on the
    /// bridge's behalf and was never registered for bridging. It is the only
    /// way a token that reached this contract by accident can leave it, and it
    /// reaches neither the native asset nor any asset the owner has capped.
    function rescue(address token, address to) external onlyOwner nonReentrant {
        if (token == NATIVE_ASSET) revert NativeAssetNotRescuable();
        if (to == address(0)) revert InvalidRecipient();
        uint256 locked_ = outstanding[token];
        if (locked_ != 0) revert OutstandingNotZero(token, locked_);
        if (registered[token]) revert AssetRegisteredForBridging(token);
        uint256 balance = IERC20(token).balanceOf(address(this));
        if (balance == 0) revert NothingToRescue(token);
        emit Rescued(token, to, balance);
        IERC20(token).safeTransfer(to, balance);
    }

    function attestors() external view returns (address[] memory) {
        return attestorList;
    }

    function deposit(address asset, uint256 amount, bytes32 paxeerRecipient) external whenNotPaused nonReentrant {
        if (asset == NATIVE_ASSET) revert UseDepositNative();
        _admitDeposit(asset, amount, paxeerRecipient);
        IERC20 token = IERC20(asset);
        uint256 before = token.balanceOf(address(this));
        token.safeTransferFrom(msg.sender, address(this), amount);
        uint256 received = token.balanceOf(address(this)) - before;
        if (received != amount) revert TransferAmountMismatch(received, amount);
        _recordDeposit(asset, amount, paxeerRecipient);
    }

    function depositNative(bytes32 paxeerRecipient) external payable whenNotPaused nonReentrant {
        _admitDeposit(NATIVE_ASSET, msg.value, paxeerRecipient);
        _recordDeposit(NATIVE_ASSET, msg.value, paxeerRecipient);
    }

    /// @notice Releases locked funds for a Paxeer burn identified by
    /// (paxeerTxHash, paxeerNonce). Signatures are 65-byte r||s||v ECDSA
    /// signatures over outboundDigest, ordered by strictly ascending signer.
    function release(
        address asset,
        uint256 amount,
        address recipient,
        bytes32 paxeerTxHash,
        uint64 paxeerNonce,
        bytes[] calldata signatures
    ) external whenNotPaused nonReentrant {
        if (amount == 0) revert ZeroAmount();
        if (recipient == address(0)) revert InvalidRecipient();
        AssetCap memory cap = caps[asset];
        if (amount > cap.perTx) revert PerTxCapExceeded(amount, cap.perTx);
        uint256 locked_ = outstanding[asset];
        if (amount > locked_) revert InsufficientOutstanding(amount, locked_);
        bytes32 nullifier = nullifierOf(paxeerTxHash, paxeerNonce);
        if (nullified[nullifier]) revert NullifierUsed(nullifier);

        bytes32 digest = BridgeAttestation.outboundDigest(
            block.chainid, address(this), paxeerTxHash, paxeerNonce, recipient, asset, amount
        );
        _verifyThreshold(digest, signatures);

        nullified[nullifier] = true;
        outstanding[asset] = locked_ - amount;
        emit BridgeRelease(asset, amount, recipient, paxeerTxHash, paxeerNonce);

        if (asset == NATIVE_ASSET) {
            (bool ok,) = recipient.call{value: amount}("");
            if (!ok) revert NativeTransferFailed();
        } else {
            IERC20(asset).safeTransfer(recipient, amount);
        }
    }

    function releaseDigest(bytes32 paxeerTxHash, uint64 paxeerNonce, address recipient, address asset, uint256 amount)
        external
        view
        returns (bytes32)
    {
        return BridgeAttestation.outboundDigest(
            block.chainid, address(this), paxeerTxHash, paxeerNonce, recipient, asset, amount
        );
    }

    function depositDigest(bytes32 txHash, uint64 logIndex, bytes32 paxeerRecipient, address asset, uint256 amount)
        external
        view
        returns (bytes32)
    {
        return
            BridgeAttestation.inboundDigest(
                block.chainid, address(this), txHash, logIndex, paxeerRecipient, asset, amount
            );
    }

    function nullifierOf(bytes32 paxeerTxHash, uint64 paxeerNonce) public pure returns (bytes32) {
        return keccak256(abi.encodePacked(paxeerTxHash, paxeerNonce));
    }

    function _setAttestors(address[] memory attestors_, uint256 threshold_) private {
        if (threshold_ == 0 || threshold_ > attestors_.length) {
            revert InvalidThreshold(threshold_, attestors_.length);
        }
        address[] memory previous = attestorList;
        for (uint256 i = 0; i < previous.length; ++i) {
            isAttestor[previous[i]] = false;
        }
        for (uint256 i = 0; i < attestors_.length; ++i) {
            address attestor = attestors_[i];
            if (attestor == address(0) || isAttestor[attestor]) revert InvalidAttestor(attestor);
            isAttestor[attestor] = true;
        }
        attestorList = attestors_;
        threshold = threshold_;
        emit AttestorsSet(attestors_, threshold_);
    }

    function _admitDeposit(address asset, uint256 amount, bytes32 paxeerRecipient) private view {
        if (amount == 0) revert ZeroAmount();
        if (paxeerRecipient == bytes32(0)) revert InvalidRecipient();
        AssetCap memory cap = caps[asset];
        if (cap.total == 0) revert AssetNotEnabled(asset);
        if (amount > cap.perTx) revert PerTxCapExceeded(amount, cap.perTx);
        uint256 after_ = outstanding[asset] + amount;
        if (after_ > cap.total) revert TotalCapExceeded(after_, cap.total);
    }

    function _recordDeposit(address asset, uint256 amount, bytes32 paxeerRecipient) private {
        outstanding[asset] += amount;
        uint64 nonce = depositNonce++;
        emit BridgeDeposit(asset, amount, msg.sender, paxeerRecipient, nonce);
    }

    function _verifyThreshold(bytes32 digest, bytes[] calldata signatures) private view {
        uint256 required = threshold;
        if (signatures.length < required) revert BelowThreshold(signatures.length, required);
        address last = address(0);
        for (uint256 i = 0; i < signatures.length; ++i) {
            address signer = _recover(digest, signatures[i]);
            if (signer <= last) revert SignersNotAscending(signer);
            if (!isAttestor[signer]) revert UnknownSigner(signer);
            last = signer;
        }
    }

    function _recover(bytes32 digest, bytes calldata signature) private pure returns (address signer) {
        if (signature.length != 65) revert InvalidSignature();
        bytes32 r = bytes32(signature[0:32]);
        bytes32 s = bytes32(signature[32:64]);
        uint8 v = uint8(signature[64]);
        if (uint256(s) > SECP256K1_HALF_ORDER || (v != 27 && v != 28)) revert InvalidSignature();
        signer = ecrecover(digest, v, r, s);
        if (signer == address(0)) revert InvalidSignature();
    }
}
