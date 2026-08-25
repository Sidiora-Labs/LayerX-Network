// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {CanonicalCheckpoint} from "./libraries/CanonicalCheckpoint.sol";
import {Arithmetic} from "./libraries/Arithmetic.sol";
import {CryptographyPrimitives} from "./libraries/CryptographyPrimitives.sol";
import {SafeCall} from "./libraries/SafeCall.sol";
import {Types} from "./libraries/Types.sol";
import {IGuarantorEligibility} from "./interfaces/IGuarantorEligibility.sol";
import {Predeploys} from "./deployment/Predeploys.sol";
import {LayerXComponent} from "./security/LayerXComponent.sol";

contract GuarantorBond is IGuarantorEligibility, LayerXComponent {
    error Unauthorized();
    error InvalidConfiguration();
    error InvalidBondAction();
    error InvalidEquivocationEvidence();
    error TransferFailed();
    error Reentrancy();

    struct BondRecord {
        address signer;
        uint256 amount;
        uint64 joinedEpoch;
        uint64 removedEpoch;
        uint64 withdrawalAvailableAt;
        uint256 pendingWithdrawal;
        bool jailed;
        bool unresolvedSlashing;
    }

    address public immutable custodyAuthority;
    address public override slashingAuthority;
    uint32 public immutable minimumBondBps;
    uint64 public immutable unbondingDelay;
    uint256 public custodiedValue;
    uint256 public slashedBalance;
    uint256 private lockState = 1;
    mapping(bytes32 => BondRecord) private records;
    mapping(address => bytes32) public guarantorIdForSigner;

    event BondDeposited(bytes32 indexed guarantorId, address indexed signer, uint256 amount, uint64 joinedEpoch);
    event UnbondingStarted(bytes32 indexed guarantorId, uint256 amount, uint64 availableAt);
    event GuarantorSlashed(
        bytes32 indexed guarantorId,
        address indexed signer,
        bytes32 firstCheckpoint,
        bytes32 secondCheckpoint,
        uint256 amount
    );
    event SlashingAuthoritySet(address indexed authority);

    constructor(
        address authority,
        uint32 bondBps,
        uint256 initialCustodiedValue,
        uint64 withdrawalDelay,
        bytes32 componentConfigHash,
        uint192 componentRelease
    ) LayerXComponent(Predeploys.GUARANTOR_BOND, componentConfigHash, componentRelease) {
        if (authority == address(0) || bondBps == 0 || bondBps > 10_000 || withdrawalDelay < 1 days) revert InvalidConfiguration();
        custodyAuthority = authority;
        minimumBondBps = bondBps;
        custodiedValue = initialCustodiedValue;
        unbondingDelay = withdrawalDelay;
    }

    modifier nonReentrant() {
        if (lockState != 1) revert Reentrancy();
        lockState = 2;
        _;
        lockState = 1;
    }

    function minimumBond() public view returns (uint256) {
        if (custodiedValue == 0) return 0;
        return Arithmetic.mulDiv(custodiedValue, minimumBondBps, 10_000, Types.Rounding.Up);
    }

    function bondRecord(bytes32 guarantorId) external view returns (BondRecord memory) {
        return records[guarantorId];
    }

    function updateCustodiedValue(uint256 value) external {
        if (msg.sender != custodyAuthority) revert Unauthorized();
        custodiedValue = value;
    }

    function setSlashingAuthority(address authority) external {
        if (msg.sender != custodyAuthority || authority.code.length == 0) {
            revert Unauthorized();
        }
        slashingAuthority = authority;
        emit SlashingAuthoritySet(authority);
    }

    function depositBond(bytes32 guarantorId, uint64 joinedEpoch) external payable {
        if (guarantorId == bytes32(0) || msg.value == 0 || joinedEpoch == 0) {
            revert InvalidBondAction();
        }
        BondRecord storage record = records[guarantorId];
        bytes32 existingId = guarantorIdForSigner[msg.sender];
        if (
            (record.signer != address(0) && record.signer != msg.sender)
                || (existingId != bytes32(0) && existingId != guarantorId) || record.jailed || record.removedEpoch != 0
        ) {
            revert InvalidBondAction();
        }
        if (record.signer == address(0)) {
            record.signer = msg.sender;
            record.joinedEpoch = joinedEpoch;
            guarantorIdForSigner[msg.sender] = guarantorId;
        } else if (record.joinedEpoch != joinedEpoch) {
            revert InvalidBondAction();
        }
        record.amount = Arithmetic.add(record.amount, msg.value);
        emit BondDeposited(guarantorId, msg.sender, msg.value, joinedEpoch);
    }

    function beginUnbond(bytes32 guarantorId, uint256 amount) external {
        BondRecord storage record = records[guarantorId];
        if (
            record.signer != msg.sender || amount == 0 || amount > record.amount || record.pendingWithdrawal != 0
                || record.jailed || record.unresolvedSlashing
        ) {
            revert InvalidBondAction();
        }
        record.pendingWithdrawal = amount;
        record.withdrawalAvailableAt = Arithmetic.toUint64(block.timestamp + unbondingDelay);
        emit UnbondingStarted(guarantorId, amount, record.withdrawalAvailableAt);
    }

    function cancelUnbond(bytes32 guarantorId) external {
        BondRecord storage record = records[guarantorId];
        if (record.signer != msg.sender || record.pendingWithdrawal == 0) {
            revert InvalidBondAction();
        }
        record.pendingWithdrawal = 0;
        record.withdrawalAvailableAt = 0;
    }

    function finalizeUnbond(bytes32 guarantorId) external nonReentrant {
        BondRecord storage record = records[guarantorId];
        uint256 amount = record.pendingWithdrawal;
        if (
            record.signer != msg.sender || amount == 0 || record.jailed || record.unresolvedSlashing
                || block.timestamp < record.withdrawalAvailableAt
        ) {
            revert InvalidBondAction();
        }
        record.pendingWithdrawal = 0;
        record.withdrawalAvailableAt = 0;
        record.amount -= amount;
        SafeCall.CallResult memory result = SafeCall.sendValue(msg.sender, amount, 100_000);
        if (!result.success) revert TransferFailed();
    }

    function bondedActive(bytes32 guarantorId, address signer, uint64 checkpointEpoch) external view returns (bool) {
        BondRecord storage record = records[guarantorId];
        return record.signer == signer && !record.jailed && !record.unresolvedSlashing && record.pendingWithdrawal == 0
            && record.joinedEpoch != 0 && record.joinedEpoch <= checkpointEpoch
            && (record.removedEpoch == 0 || record.removedEpoch > checkpointEpoch) && record.amount >= minimumBond();
    }

    function setUnresolvedSlashing(bytes32 guarantorId, bool unresolved) external {
        if (msg.sender != custodyAuthority) revert Unauthorized();
        BondRecord storage record = records[guarantorId];
        if (record.signer == address(0)) revert InvalidBondAction();
        record.unresolvedSlashing = unresolved;
    }

    function jailGuarantor(bytes32 guarantorId, uint64 removedEpoch) external {
        if (msg.sender != custodyAuthority) revert Unauthorized();
        BondRecord storage record = records[guarantorId];
        if (record.signer == address(0) || removedEpoch == 0) {
            revert InvalidBondAction();
        }
        record.jailed = true;
        record.removedEpoch = removedEpoch;
        record.pendingWithdrawal = 0;
        record.withdrawalAvailableAt = 0;
    }

    function submitEquivocation(
        CanonicalCheckpoint.GuarantorAttestation calldata first,
        CanonicalCheckpoint.GuarantorAttestation calldata second,
        uint64 removedEpoch
    ) external {
        if (
            removedEpoch == 0 || first.guarantorId != second.guarantorId || first.signer != second.signer
                || first.checkpointHash == second.checkpointHash
                || (first.batchNumber != second.batchNumber && first.checkpointId != second.checkpointId)
                || !_validSignature(first) || !_validSignature(second)
        ) {
            revert InvalidEquivocationEvidence();
        }
        BondRecord storage record = records[first.guarantorId];
        if (record.signer != first.signer || record.jailed) {
            revert InvalidEquivocationEvidence();
        }
        _slash(first.guarantorId, record, removedEpoch, first.checkpointHash, second.checkpointHash);
    }

    function slashForCheckpoint(bytes32 guarantorId, bytes32 checkpointHash, uint64 removedEpoch) external {
        if (msg.sender != slashingAuthority || removedEpoch == 0) {
            revert Unauthorized();
        }
        BondRecord storage record = records[guarantorId];
        if (record.signer == address(0)) revert InvalidBondAction();
        if (record.jailed) return;
        _slash(guarantorId, record, removedEpoch, checkpointHash, checkpointHash);
    }

    function sweepSlashed(address payable recipient, uint256 amount) external nonReentrant {
        if (msg.sender != custodyAuthority || recipient == address(0) || amount > slashedBalance) {
            revert Unauthorized();
        }
        slashedBalance -= amount;
        SafeCall.CallResult memory result = SafeCall.sendValue(recipient, amount, 100_000);
        if (!result.success) revert TransferFailed();
    }

    function _validSignature(CanonicalCheckpoint.GuarantorAttestation calldata attestation)
        private
        pure
        returns (bool)
    {
        if (attestation.signer == address(0)) return false;
        return CryptographyPrimitives.recoverOrZero(
            CanonicalCheckpoint.attestationHash(attestation), attestation.r, attestation.s, attestation.v
        ) == attestation.signer;
    }

    function _slash(
        bytes32 guarantorId,
        BondRecord storage record,
        uint64 removedEpoch,
        bytes32 firstCheckpoint,
        bytes32 secondCheckpoint
    ) private {
        uint256 amount = record.amount;
        record.amount = 0;
        record.pendingWithdrawal = 0;
        record.withdrawalAvailableAt = 0;
        record.jailed = true;
        record.unresolvedSlashing = false;
        record.removedEpoch = removedEpoch;
        slashedBalance = Arithmetic.add(slashedBalance, amount);
        emit GuarantorSlashed(guarantorId, record.signer, firstCheckpoint, secondCheckpoint, amount);
    }
}
