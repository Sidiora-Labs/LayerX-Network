// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {CanonicalCheckpoint} from "./libraries/CanonicalCheckpoint.sol";
import {Arithmetic} from "./libraries/Arithmetic.sol";
import {CryptographyPrimitives} from "./libraries/CryptographyPrimitives.sol";
import {SafeCall} from "./libraries/SafeCall.sol";
import {Types} from "./libraries/Types.sol";
import {Constants} from "./libraries/Constants.sol";
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
        address bondController;
        uint256 amount;
        uint64 joinedEpoch;
        uint64 removedEpoch;
        uint64 ejectedAtVersion;
        uint64 withdrawalAvailableAt;
        uint256 pendingWithdrawal;
        bool jailed;
        bool unresolvedSlashing;
    }

    struct SignerAuthorization {
        uint64 activeFromEpoch;
        uint64 activeUntilEpoch;
        uint64 setVersion;
    }

    address public immutable custodyAuthority;
    address public immutable membershipAuthority;
    uint16 public immutable override protocolVersion;
    uint32 public immutable override networkId;
    address public override slashingAuthority;
    uint32 public immutable minimumBondBps;
    uint64 public immutable unbondingDelay;
    uint256 public custodiedValue;
    uint256 public slashedBalance;
    uint64 public override membershipVersion;
    uint64 public lastGovernanceSequence;
    uint256 private lockState = 1;
    mapping(bytes32 => BondRecord) private records;
    mapping(address => bytes32) public guarantorIdForSigner;
    mapping(bytes32 => mapping(address => SignerAuthorization)) public signerAuthorization;

    event GuarantorActivated(
        bytes32 indexed guarantorId,
        address indexed signer,
        address indexed bondController,
        uint64 joinedEpoch,
        uint64 setVersion,
        uint64 governanceSequence
    );
    event GuarantorSignerRotated(
        bytes32 indexed guarantorId,
        address indexed previousSigner,
        address indexed newSigner,
        uint64 activationEpoch,
        uint64 setVersion,
        uint64 governanceSequence
    );
    event GuarantorRemoved(
        bytes32 indexed guarantorId, uint64 removedEpoch, uint64 setVersion, uint64 governanceSequence
    );
    event GuarantorJailStatusSet(
        bytes32 indexed guarantorId, bool jailed, uint64 setVersion, uint64 governanceSequence
    );
    event BondDeposited(bytes32 indexed guarantorId, address indexed payer, uint256 amount, uint256 totalBond);
    event UnbondingStarted(bytes32 indexed guarantorId, uint256 amount, uint64 availableAt);
    event GuarantorSlashed(
        bytes32 indexed guarantorId,
        address indexed signer,
        bytes32 firstCheckpoint,
        bytes32 secondCheckpoint,
        uint256 amount,
        uint64 setVersion
    );
    event SlashingAuthoritySet(address indexed authority);

    constructor(
        address custody,
        address membershipGovernance,
        uint16 expectedProtocolVersion,
        uint32 expectedNetworkId,
        uint32 bondBps,
        uint256 initialCustodiedValue,
        uint64 withdrawalDelay,
        bytes32 componentConfigHash,
        uint192 componentRelease
    ) LayerXComponent(Predeploys.GUARANTOR_BOND, componentConfigHash, componentRelease) {
        if (
            custody == address(0) || membershipGovernance == address(0) || bondBps == 0 || bondBps > 10_000
                || expectedProtocolVersion == 0 || expectedNetworkId == 0 || withdrawalDelay < 1 days
                || block.chainid > type(uint64).max
        ) revert InvalidConfiguration();
        custodyAuthority = custody;
        membershipAuthority = membershipGovernance;
        protocolVersion = expectedProtocolVersion;
        networkId = expectedNetworkId;
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

    modifier onlyMembershipAuthority() {
        if (msg.sender != membershipAuthority) revert Unauthorized();
        _;
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

    function activateGuarantor(
        bytes32 guarantorId,
        address signer,
        address bondController,
        uint64 joinedEpoch,
        uint64 governanceSequence
    ) external onlyMembershipAuthority {
        if (
            guarantorId == bytes32(0) || signer == address(0) || bondController == address(0) || joinedEpoch == 0
                || records[guarantorId].signer != address(0) || guarantorIdForSigner[signer] != bytes32(0)
        ) revert InvalidBondAction();
        uint64 version = _advanceMembership(governanceSequence);
        records[guarantorId] = BondRecord({
            signer: signer,
            bondController: bondController,
            amount: 0,
            joinedEpoch: joinedEpoch,
            removedEpoch: 0,
            ejectedAtVersion: 0,
            withdrawalAvailableAt: 0,
            pendingWithdrawal: 0,
            jailed: false,
            unresolvedSlashing: false
        });
        guarantorIdForSigner[signer] = guarantorId;
        signerAuthorization[guarantorId][signer] = SignerAuthorization({
            activeFromEpoch: joinedEpoch, activeUntilEpoch: 0, setVersion: version
        });
        emit GuarantorActivated(
            guarantorId, signer, bondController, joinedEpoch, version, governanceSequence
        );
    }

    function rotateGuarantorSigner(
        bytes32 guarantorId,
        address newSigner,
        uint64 activationEpoch,
        uint64 governanceSequence
    ) external onlyMembershipAuthority {
        BondRecord storage record = records[guarantorId];
        if (
            record.signer == address(0) || newSigner == address(0) || newSigner == record.signer
                || guarantorIdForSigner[newSigner] != bytes32(0) || record.removedEpoch != 0 || record.jailed
        ) revert InvalidBondAction();
        SignerAuthorization storage previous = signerAuthorization[guarantorId][record.signer];
        if (activationEpoch <= previous.activeFromEpoch || previous.activeUntilEpoch != 0) revert InvalidBondAction();
        uint64 version = _advanceMembership(governanceSequence);
        address previousSigner = record.signer;
        previous.activeUntilEpoch = activationEpoch;
        record.signer = newSigner;
        guarantorIdForSigner[newSigner] = guarantorId;
        signerAuthorization[guarantorId][newSigner] = SignerAuthorization({
            activeFromEpoch: activationEpoch, activeUntilEpoch: 0, setVersion: version
        });
        emit GuarantorSignerRotated(
            guarantorId, previousSigner, newSigner, activationEpoch, version, governanceSequence
        );
    }

    function removeGuarantor(bytes32 guarantorId, uint64 removedEpoch, uint64 governanceSequence)
        external
        onlyMembershipAuthority
    {
        BondRecord storage record = records[guarantorId];
        SignerAuthorization storage current = signerAuthorization[guarantorId][record.signer];
        if (
            record.signer == address(0) || record.removedEpoch != 0 || removedEpoch <= current.activeFromEpoch
                || current.activeUntilEpoch != 0
        ) revert InvalidBondAction();
        uint64 version = _advanceMembership(governanceSequence);
        record.removedEpoch = removedEpoch;
        current.activeUntilEpoch = removedEpoch;
        emit GuarantorRemoved(guarantorId, removedEpoch, version, governanceSequence);
    }

    function setGuarantorJailStatus(bytes32 guarantorId, bool jailed, uint64 governanceSequence)
        external
        onlyMembershipAuthority
    {
        BondRecord storage record = records[guarantorId];
        if (
            record.signer == address(0) || record.removedEpoch != 0 || record.jailed == jailed
                || (!jailed && record.ejectedAtVersion != 0)
        ) {
            revert InvalidBondAction();
        }
        uint64 version = _advanceMembership(governanceSequence);
        record.jailed = jailed;
        emit GuarantorJailStatusSet(guarantorId, jailed, version, governanceSequence);
    }

    function depositBond(bytes32 guarantorId) external payable {
        BondRecord storage record = records[guarantorId];
        if (
            record.signer == address(0) || msg.value == 0 || record.removedEpoch != 0
                || record.ejectedAtVersion != 0
        ) revert InvalidBondAction();
        record.amount = Arithmetic.add(record.amount, msg.value);
        emit BondDeposited(guarantorId, msg.sender, msg.value, record.amount);
    }

    function beginUnbond(bytes32 guarantorId, uint256 amount) external {
        BondRecord storage record = records[guarantorId];
        if (
            record.bondController != msg.sender || amount == 0 || amount > record.amount
                || record.pendingWithdrawal != 0 || record.removedEpoch == 0 || record.unresolvedSlashing
        ) {
            revert InvalidBondAction();
        }
        record.pendingWithdrawal = amount;
        record.withdrawalAvailableAt = Arithmetic.toUint64(block.timestamp + unbondingDelay);
        emit UnbondingStarted(guarantorId, amount, record.withdrawalAvailableAt);
    }

    function cancelUnbond(bytes32 guarantorId) external {
        BondRecord storage record = records[guarantorId];
        if (record.bondController != msg.sender || record.pendingWithdrawal == 0) {
            revert InvalidBondAction();
        }
        record.pendingWithdrawal = 0;
        record.withdrawalAvailableAt = 0;
    }

    function finalizeUnbond(bytes32 guarantorId) external nonReentrant {
        BondRecord storage record = records[guarantorId];
        uint256 amount = record.pendingWithdrawal;
        if (
            record.bondController != msg.sender || amount == 0 || record.unresolvedSlashing
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
        SignerAuthorization storage authorization = signerAuthorization[guarantorId][signer];
        return authorization.activeFromEpoch != 0 && authorization.activeFromEpoch <= checkpointEpoch
            && (authorization.activeUntilEpoch == 0 || authorization.activeUntilEpoch > checkpointEpoch)
            && !record.jailed && !record.unresolvedSlashing && record.joinedEpoch != 0
            && record.ejectedAtVersion == 0
            && record.joinedEpoch <= checkpointEpoch
            && (record.removedEpoch == 0 || record.removedEpoch > checkpointEpoch) && record.amount >= minimumBond();
    }

    function setUnresolvedSlashing(bytes32 guarantorId, bool unresolved) external {
        if (msg.sender != custodyAuthority) revert Unauthorized();
        BondRecord storage record = records[guarantorId];
        if (record.signer == address(0)) revert InvalidBondAction();
        record.unresolvedSlashing = unresolved;
    }

    function submitEquivocation(
        CanonicalCheckpoint.GuarantorAttestation calldata first,
        CanonicalCheckpoint.GuarantorAttestation calldata second
    ) external {
        if (
            !_canonicalDomainAttestation(first) || !_canonicalDomainAttestation(second)
                || first.guarantorId != second.guarantorId || first.signer != second.signer
                || first.epoch != second.epoch || first.batchNumber != second.batchNumber
                || first.checkpointHash == second.checkpointHash
                || !_validSignature(first) || !_validSignature(second)
        ) {
            revert InvalidEquivocationEvidence();
        }
        BondRecord storage record = records[first.guarantorId];
        SignerAuthorization storage authorization = signerAuthorization[first.guarantorId][first.signer];
        if (
            authorization.activeFromEpoch == 0 || authorization.activeFromEpoch > first.epoch
                || (authorization.activeUntilEpoch != 0 && authorization.activeUntilEpoch <= first.epoch)
                || record.ejectedAtVersion != 0
        ) {
            revert InvalidEquivocationEvidence();
        }
        _slash(first.guarantorId, record, first.signer, first.checkpointHash, second.checkpointHash);
    }

    function slashForCheckpoint(bytes32 guarantorId, bytes32 checkpointHash) external {
        if (msg.sender != slashingAuthority) revert Unauthorized();
        BondRecord storage record = records[guarantorId];
        if (record.signer == address(0)) revert InvalidBondAction();
        if (record.ejectedAtVersion != 0) return;
        _slash(guarantorId, record, record.signer, checkpointHash, checkpointHash);
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

    function _canonicalDomainAttestation(CanonicalCheckpoint.GuarantorAttestation calldata attestation)
        private
        view
        returns (bool)
    {
        return attestation.protocolVersion == protocolVersion && attestation.networkId == networkId
            && attestation.paxeerChainId == uint64(block.chainid)
            && attestation.settlementContract == address(this) && attestation.epoch != 0
            && attestation.batchNumber != 0 && attestation.checkpointId == attestation.checkpointHash
            && attestation.replayed && attestation.dataAvailable
            && attestation.availabilityClassMask == Constants.ALL_AVAILABILITY_CLASSES
            && attestation.attestedAt != 0;
    }

    function _slash(
        bytes32 guarantorId,
        BondRecord storage record,
        address offendingSigner,
        bytes32 firstCheckpoint,
        bytes32 secondCheckpoint
    ) private {
        if (membershipVersion == type(uint64).max) revert InvalidBondAction();
        uint256 amount = record.amount;
        record.amount = 0;
        record.pendingWithdrawal = 0;
        record.withdrawalAvailableAt = 0;
        record.jailed = true;
        record.unresolvedSlashing = false;
        uint64 version = membershipVersion + 1;
        membershipVersion = version;
        record.ejectedAtVersion = version;
        slashedBalance = Arithmetic.add(slashedBalance, amount);
        emit GuarantorSlashed(guarantorId, offendingSigner, firstCheckpoint, secondCheckpoint, amount, version);
    }

    function _advanceMembership(uint64 governanceSequence) private returns (uint64 version) {
        if (lastGovernanceSequence == type(uint64).max || membershipVersion == type(uint64).max) {
            revert InvalidBondAction();
        }
        if (governanceSequence != lastGovernanceSequence + 1) revert InvalidBondAction();
        lastGovernanceSequence = governanceSequence;
        version = membershipVersion + 1;
        membershipVersion = version;
    }
}
