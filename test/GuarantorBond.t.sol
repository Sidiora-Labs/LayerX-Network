// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {CanonicalCheckpoint} from "../contracts/libraries/CanonicalCheckpoint.sol";
import {GuarantorBond} from "../contracts/GuarantorBond.sol";
import {Constants} from "../contracts/libraries/Constants.sol";

interface BondVm {
    function addr(uint256 privateKey) external returns (address);
    function deal(address account, uint256 balance) external;
    function prank(address sender) external;
    function sign(uint256 privateKey, bytes32 digest) external returns (uint8 v, bytes32 r, bytes32 s);
    function expectRevert(bytes4 selector) external;
    function etch(address target, bytes calldata code) external;
    function warp(uint256 timestamp) external;
}

contract BondToken {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    function mint(address recipient, uint256 amount) external {
        balanceOf[recipient] += amount;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function transfer(address recipient, uint256 amount) external returns (bool) {
        balanceOf[msg.sender] -= amount;
        balanceOf[recipient] += amount;
        return true;
    }

    function transferFrom(address sender, address recipient, uint256 amount) external virtual returns (bool) {
        allowance[sender][msg.sender] -= amount;
        balanceOf[sender] -= amount;
        balanceOf[recipient] += amount;
        return true;
    }
}

contract FeeOnTransferBondToken is BondToken {
    function transferFrom(address sender, address recipient, uint256 amount) external override returns (bool) {
        allowance[sender][msg.sender] -= amount;
        balanceOf[sender] -= amount;
        balanceOf[recipient] += amount - 1;
        return true;
    }
}

contract GuarantorBondTest {
    BondVm private constant vm = BondVm(address(uint160(uint256(keccak256("hevm cheat code")))));
    bytes32 private constant CONFIG = keccak256("bond-test-config");
    uint192 private constant RELEASE = uint192(1) << 128;

    function testPermissionlessPortableEquivocationSlashes() public {
        uint256 privateKey = 7;
        address signer = vm.addr(privateKey);
        bytes32 guarantorId = bytes32(uint256(7));
        GuarantorBond bond = _newBond();
        bond.activateGuarantor(guarantorId, signer, signer, 1, 1);
        _fundBond(bond, signer, guarantorId, 1 ether);
        CanonicalCheckpoint.GuarantorAttestation memory first =
            _statement(guarantorId, signer, bytes32(uint256(1)), address(bond), 1);
        CanonicalCheckpoint.GuarantorAttestation memory second =
            _statement(guarantorId, signer, bytes32(uint256(2)), address(bond), 1);
        _sign(first, privateKey);
        _sign(second, privateKey);
        vm.prank(address(0xBEEF));
        bond.submitEquivocation(first, second);
        GuarantorBond.BondRecord memory record = bond.bondRecord(guarantorId);
        require(
            record.jailed && record.amount == 0 && record.removedEpoch == 0 && record.ejectedAtVersion == 3,
            "not slashed"
        );
        require(bond.slashedBalance() == 1 ether, "slash not conserved");
        require(bond.membershipVersion() == 3, "evidence removal was not versioned");
        vm.expectRevert(GuarantorBond.InvalidBondAction.selector);
        bond.setGuarantorJailStatus(guarantorId, false, 2);
        vm.expectRevert(GuarantorBond.InvalidBondAction.selector);
        vm.prank(signer);
        bond.depositBond(guarantorId, 1 ether);
        bond.removeGuarantor(guarantorId, 4, 2);
        require(bond.bondRecord(guarantorId).removedEpoch == 4, "governed removal missing");
    }

    function testEquivocationRejectsForeignNetworkAndPaxeerDomain() public {
        uint256 privateKey = 15;
        address signer = vm.addr(privateKey);
        bytes32 guarantorId = bytes32(uint256(15));
        GuarantorBond bond = _newBond();
        bond.activateGuarantor(guarantorId, signer, signer, 1, 1);
        _fundBond(bond, signer, guarantorId, 1 ether);
        CanonicalCheckpoint.GuarantorAttestation memory first =
            _statement(guarantorId, signer, bytes32(uint256(41)), address(bond), 2);
        CanonicalCheckpoint.GuarantorAttestation memory second =
            _statement(guarantorId, signer, bytes32(uint256(42)), address(bond), 2);
        _sign(first, privateKey);
        second.networkId = 43;
        _sign(second, privateKey);
        vm.expectRevert(GuarantorBond.InvalidEquivocationEvidence.selector);
        bond.submitEquivocation(first, second);

        first.networkId = 43;
        _sign(first, privateKey);
        vm.expectRevert(GuarantorBond.InvalidEquivocationEvidence.selector);
        bond.submitEquivocation(first, second);

        first.networkId = 42;
        _sign(first, privateKey);
        second.networkId = 42;
        second.paxeerChainId = first.paxeerChainId + 1;
        _sign(second, privateKey);
        vm.expectRevert(GuarantorBond.InvalidEquivocationEvidence.selector);
        bond.submitEquivocation(first, second);

        second.paxeerChainId = first.paxeerChainId;
        second.settlementContract = address(0xF0E1);
        _sign(second, privateKey);
        vm.expectRevert(GuarantorBond.InvalidEquivocationEvidence.selector);
        bond.submitEquivocation(first, second);
        require(bond.bondRecord(guarantorId).amount == 1 ether, "foreign-domain evidence slashed bond");
    }

    function testEquivocationRejectsDifferentCoordinateAndUnauthorizedEra() public {
        uint256 privateKey = 16;
        address signer = vm.addr(privateKey);
        bytes32 guarantorId = bytes32(uint256(16));
        GuarantorBond bond = _newBond();
        bond.activateGuarantor(guarantorId, signer, signer, 5, 1);
        _fundBond(bond, signer, guarantorId, 1 ether);
        CanonicalCheckpoint.GuarantorAttestation memory first =
            _statement(guarantorId, signer, bytes32(uint256(51)), address(bond), 5);
        CanonicalCheckpoint.GuarantorAttestation memory second =
            _statement(guarantorId, signer, bytes32(uint256(52)), address(bond), 5);
        second.batchNumber += 1;
        _sign(first, privateKey);
        _sign(second, privateKey);
        vm.expectRevert(GuarantorBond.InvalidEquivocationEvidence.selector);
        bond.submitEquivocation(first, second);

        first.epoch = 4;
        second.epoch = 4;
        second.batchNumber = first.batchNumber;
        _sign(first, privateKey);
        _sign(second, privateKey);
        vm.expectRevert(GuarantorBond.InvalidEquivocationEvidence.selector);
        bond.submitEquivocation(first, second);
        require(bond.bondRecord(guarantorId).amount == 1 ether, "unauthorized-era evidence slashed bond");
    }

    function testGovernedRemovalStartsUnbonding() public {
        uint256 privateKey = 8;
        address signer = vm.addr(privateKey);
        bytes32 guarantorId = bytes32(uint256(8));
        GuarantorBond bond = _newBond();
        bond.activateGuarantor(guarantorId, signer, signer, 1, 1);
        _fundBond(bond, signer, guarantorId, 1 ether);
        require(bond.bondedActive(guarantorId, signer, 1), "not active");
        bond.removeGuarantor(guarantorId, 2, 2);
        vm.prank(signer);
        bond.beginUnbond(guarantorId, 1 ether);
        require(bond.bondedActive(guarantorId, signer, 1), "historical eligibility was erased");
        require(!bond.bondedActive(guarantorId, signer, 2), "removed signer remained active");
    }

    function testFundingCannotCreateOrBackdateMembership() public {
        GuarantorBond bond = _newBond();
        bytes32 guarantorId = bytes32(uint256(9));
        address signer = vm.addr(9);
        address funder = address(0xF00D);
        vm.expectRevert(GuarantorBond.InvalidBondAction.selector);
        vm.prank(funder);
        bond.depositBond(guarantorId, 1 ether);

        vm.expectRevert(GuarantorBond.Unauthorized.selector);
        vm.prank(funder);
        bond.activateGuarantor(guarantorId, signer, signer, 7, 1);

        bond.activateGuarantor(guarantorId, signer, signer, 7, 1);
        _fundBond(bond, funder, guarantorId, 1 ether);
        require(bond.bondRecord(guarantorId).amount == 1 ether, "permissionless funding not recorded");
        require(!bond.bondedActive(guarantorId, signer, 6), "membership was backdated");
        require(bond.bondedActive(guarantorId, signer, 7), "authorized activation missing");
    }

    function testRotationAndRemovalAreVersionedAndEpochBound() public {
        GuarantorBond bond = _newBond();
        bytes32 guarantorId = bytes32(uint256(10));
        address originalSigner = vm.addr(10);
        address rotatedSigner = vm.addr(11);
        bond.activateGuarantor(guarantorId, originalSigner, originalSigner, 2, 1);
        _fundBond(bond, address(0xB0A0), guarantorId, 1 ether);

        bond.rotateGuarantorSigner(guarantorId, rotatedSigner, 5, 2);
        require(bond.bondedActive(guarantorId, originalSigner, 4), "old signer history lost");
        require(!bond.bondedActive(guarantorId, originalSigner, 5), "old signer survived rotation");
        require(!bond.bondedActive(guarantorId, rotatedSigner, 4), "new signer activated early");
        require(bond.bondedActive(guarantorId, rotatedSigner, 5), "new signer not activated");

        vm.expectRevert(GuarantorBond.InvalidBondAction.selector);
        bond.removeGuarantor(guarantorId, 9, 2);
        bond.removeGuarantor(guarantorId, 9, 3);
        require(bond.membershipVersion() == 4 && bond.lastGovernanceSequence() == 3, "set version drift");
        require(bond.bondedActive(guarantorId, rotatedSigner, 8), "pre-removal history lost");
        require(!bond.bondedActive(guarantorId, rotatedSigner, 9), "removed signer remained active");
        vm.expectRevert(GuarantorBond.InvalidBondAction.selector);
        vm.prank(rotatedSigner);
        bond.beginUnbond(guarantorId, 1 ether);
        vm.prank(originalSigner);
        bond.beginUnbond(guarantorId, 1 ether);
        require(bond.bondRecord(guarantorId).pendingWithdrawal == 1 ether, "bond controller lost custody");
    }

    function testOldSignerEquivocationPreservesEpochHistoryAndExcludesCurrentMember() public {
        uint256 oldPrivateKey = 12;
        address oldSigner = vm.addr(oldPrivateKey);
        address newSigner = vm.addr(13);
        bytes32 guarantorId = bytes32(uint256(12));
        GuarantorBond bond = _newBond();
        bond.activateGuarantor(guarantorId, oldSigner, oldSigner, 1, 1);
        _fundBond(bond, oldSigner, guarantorId, 1 ether);
        bond.rotateGuarantorSigner(guarantorId, newSigner, 5, 2);

        CanonicalCheckpoint.GuarantorAttestation memory first =
            _statement(guarantorId, oldSigner, bytes32(uint256(21)), address(bond), 4);
        CanonicalCheckpoint.GuarantorAttestation memory second =
            _statement(guarantorId, oldSigner, bytes32(uint256(22)), address(bond), 4);
        _sign(first, oldPrivateKey);
        _sign(second, oldPrivateKey);
        vm.prank(address(0xCAFE));
        bond.submitEquivocation(first, second);

        GuarantorBond.BondRecord memory record = bond.bondRecord(guarantorId);
        (, uint64 oldActiveUntil,) = bond.signerAuthorization(guarantorId, oldSigner);
        (, uint64 newActiveUntil,) = bond.signerAuthorization(guarantorId, newSigner);
        require(
            record.jailed && record.removedEpoch == 0 && record.ejectedAtVersion == 4,
            "evidence exclusion boundary missing"
        );
        require(oldActiveUntil == 5 && newActiveUntil == 0, "signer history was rewritten");
        require(!bond.bondedActive(guarantorId, newSigner, 5), "slashed member remained eligible");
        require(bond.membershipVersion() == 4, "evidence exclusion was not versioned");
    }

    function testAdministrativeJailDoesNotShieldBondFromLaterSlash() public {
        address signer = vm.addr(14);
        bytes32 guarantorId = bytes32(uint256(14));
        GuarantorBond bond = _newBond();
        bond.activateGuarantor(guarantorId, signer, signer, 1, 1);
        _fundBond(bond, signer, guarantorId, 1 ether);
        bond.setGuarantorJailStatus(guarantorId, true, 2);
        bond.setSlashingAuthority(address(this));
        bond.slashForCheckpoint(guarantorId, bytes32(uint256(31)));
        GuarantorBond.BondRecord memory record = bond.bondRecord(guarantorId);
        require(record.amount == 0 && record.ejectedAtVersion == 4, "jailed bond escaped slash");
        require(bond.slashedBalance() == 1 ether, "jailed slash not conserved");
    }

    function testFeeOnTransferAndNativePaxAreRejected() public {
        BondToken token = _installToken(true);
        GuarantorBond bond = _deployBond();
        bytes32 guarantorId = bytes32(uint256(21));
        address controller = address(0x2100);
        bond.activateGuarantor(guarantorId, vm.addr(21), controller, 1, 1);
        token.mint(controller, 2 ether);
        vm.prank(controller);
        token.approve(address(bond), 2 ether);
        vm.expectRevert(GuarantorBond.TransferFailed.selector);
        vm.prank(controller);
        bond.depositBond(guarantorId, 1 ether);
        require(bond.totalBonded() == 0, "fee token changed accounting");
        require(token.balanceOf(address(bond)) == 0, "fee token transfer survived revert");

        vm.deal(address(this), 1 ether);
        (bool received,) = address(bond).call{value: 1}("");
        require(!received, "native PAX accepted");
        (received,) = address(bond).call{value: 1}(abi.encodeCall(GuarantorBond.depositBond, (guarantorId, uint256(1))));
        require(!received, "payable bond deposit accepted");
    }

    function testEveryEligibilityMutationAdvancesVersionWithoutConsumingGovernanceSequence() public {
        GuarantorBond bond = _newBond();
        bytes32 guarantorId = bytes32(uint256(22));
        address controller = address(0x2200);
        address signer = vm.addr(22);
        address rotated = vm.addr(23);
        bond.activateGuarantor(guarantorId, signer, controller, 1, 1);
        require(bond.membershipVersion() == 1 && bond.lastGovernanceSequence() == 1, "activation version");
        _fundBond(bond, controller, guarantorId, 5 ether);
        require(bond.membershipVersion() == 2 && bond.lastGovernanceSequence() == 1, "deposit version");
        vm.expectRevert(GuarantorBond.Unauthorized.selector);
        vm.prank(address(0xBAD));
        bond.syncCustodiedValue(100 ether);
        bond.syncCustodiedValue(100 ether);
        require(bond.membershipVersion() == 3 && bond.lastGovernanceSequence() == 1, "custody version");
        bond.setGuarantorJailStatus(guarantorId, true, 2);
        require(bond.membershipVersion() == 4 && bond.lastGovernanceSequence() == 2, "jail version");
        bond.setGuarantorJailStatus(guarantorId, false, 3);
        require(bond.membershipVersion() == 5 && bond.lastGovernanceSequence() == 3, "unjail version");
        bond.rotateGuarantorSigner(guarantorId, rotated, 2, 4);
        require(bond.membershipVersion() == 6 && bond.lastGovernanceSequence() == 4, "rotation version");
        bond.setUnresolvedSlashing(guarantorId, true);
        require(bond.membershipVersion() == 7 && bond.lastGovernanceSequence() == 4, "slashing flag version");
        bond.setUnresolvedSlashing(guarantorId, false);
        require(bond.membershipVersion() == 8 && bond.lastGovernanceSequence() == 4, "slashing clear version");
        bond.removeGuarantor(guarantorId, 3, 5);
        require(bond.membershipVersion() == 9 && bond.lastGovernanceSequence() == 5, "removal version");
        vm.prank(controller);
        bond.beginUnbond(guarantorId, 2 ether);
        vm.warp(block.timestamp + 7 days);
        vm.prank(controller);
        bond.finalizeUnbond(guarantorId);
        require(bond.membershipVersion() == 10 && bond.lastGovernanceSequence() == 5, "unbond version");
    }

    function testGenesisBondedSetRejectsPartialOmittedDuplicateAndUnsortedMembers() public {
        GuarantorBond bond = _newBond();
        bytes32 first = bytes32(uint256(31));
        bytes32 second = bytes32(uint256(32));
        bytes32 third = bytes32(uint256(33));
        bond.activateGuarantor(first, vm.addr(31), address(0x3100), 1, 1);
        bond.activateGuarantor(second, vm.addr(32), address(0x3200), 1, 2);
        bond.activateGuarantor(third, vm.addr(33), address(0x3300), 1, 3);
        _fundBond(bond, address(0x3100), first, 3 ether);
        _fundBond(bond, address(0x3200), second, 4 ether);
        _fundBond(bond, address(0x3300), third, 5 ether);

        bytes32[] memory ids = new bytes32[](2);
        ids[0] = first;
        ids[1] = second;
        vm.expectRevert(GuarantorBond.InvalidBondAction.selector);
        bond.sealGenesisBondedSet(ids);
        ids[1] = first;
        vm.expectRevert(GuarantorBond.InvalidBondAction.selector);
        bond.sealGenesisBondedSet(ids);
        ids[0] = second;
        ids[1] = first;
        vm.expectRevert(GuarantorBond.InvalidBondAction.selector);
        bond.sealGenesisBondedSet(ids);

        ids = new bytes32[](3);
        ids[0] = first;
        ids[1] = second;
        ids[2] = third;
        bond.sealGenesisBondedSet(ids);
        require(bond.genesisBondedSetCommitment() != bytes32(0), "genesis set not committed");
        require(bond.genesisBondedSetVersion() == bond.membershipVersion(), "genesis set version stale");
        vm.expectRevert(GuarantorBond.InvalidBondAction.selector);
        bond.sealGenesisBondedSet(ids);
    }

    function testUsdlConservationThroughUnbondSlashAndSweep() public {
        GuarantorBond bond = _newBond();
        BondToken token = BondToken(Constants.USDL_TOKEN);
        bytes32 first = bytes32(uint256(41));
        bytes32 second = bytes32(uint256(42));
        address firstController = address(0x4100);
        address secondController = address(0x4200);
        bond.activateGuarantor(first, vm.addr(41), firstController, 1, 1);
        bond.activateGuarantor(second, vm.addr(42), secondController, 1, 2);
        _fundBond(bond, firstController, first, 5 ether);
        _fundBond(bond, secondController, second, 7 ether);
        require(bond.totalBonded() == 12 ether && token.balanceOf(address(bond)) == 12 ether, "deposit conservation");

        bond.removeGuarantor(first, 2, 3);
        vm.prank(firstController);
        bond.beginUnbond(first, 5 ether);
        vm.warp(block.timestamp + 7 days);
        vm.prank(firstController);
        bond.finalizeUnbond(first);
        require(bond.totalBonded() == 7 ether && token.balanceOf(firstController) == 5 ether, "unbond conservation");

        bond.setSlashingAuthority(address(this));
        bond.slashForCheckpoint(second, bytes32(uint256(42)));
        require(bond.totalBonded() == 0 && bond.slashedBalance() == 7 ether, "slash conservation");
        bond.sweepSlashed(address(0xBEEF), 7 ether);
        require(
            bond.slashedBalance() == 0 && token.balanceOf(address(bond)) == 0
                && token.balanceOf(address(0xBEEF)) == 7 ether,
            "sweep conservation"
        );
    }

    function _newBond() private returns (GuarantorBond bond) {
        _installToken(false);
        return _deployBond();
    }

    function _deployBond() private returns (GuarantorBond bond) {
        return new GuarantorBond(
            address(this),
            address(this),
            Constants.USDL_TOKEN,
            address(this),
            Constants.USDL_ASSET_ID,
            Constants.PROTOCOL_VERSION,
            42,
            100,
            7 days,
            CONFIG,
            RELEASE
        );
    }

    function _installToken(bool feeOnTransfer) private returns (BondToken token) {
        address implementation = feeOnTransfer ? address(new FeeOnTransferBondToken()) : address(new BondToken());
        vm.etch(Constants.USDL_TOKEN, implementation.code);
        return BondToken(Constants.USDL_TOKEN);
    }

    function _fundBond(GuarantorBond bond, address payer, bytes32 guarantorId, uint256 amount) private {
        BondToken token = BondToken(Constants.USDL_TOKEN);
        token.mint(payer, amount);
        vm.prank(payer);
        token.approve(address(bond), amount);
        vm.prank(payer);
        bond.depositBond(guarantorId, amount);
    }

    function _statement(
        bytes32 guarantorId,
        address signer,
        bytes32 checkpointHash,
        address settlementContract,
        uint64 epoch
    ) private view returns (CanonicalCheckpoint.GuarantorAttestation memory statement) {
        statement = CanonicalCheckpoint.GuarantorAttestation({
            protocolVersion: 2,
            networkId: 42,
            paxeerChainId: uint64(block.chainid),
            settlementContract: settlementContract,
            epoch: epoch,
            checkpointId: checkpointHash,
            checkpointHash: checkpointHash,
            guarantorId: guarantorId,
            batchNumber: 12,
            dataAvailabilityRoot: bytes32(uint256(3)),
            replayed: true,
            dataAvailable: true,
            availabilityClassMask: 0x1f,
            attestedAt: 100,
            signer: signer,
            r: bytes32(0),
            s: bytes32(0),
            v: 0
        });
    }

    function _sign(CanonicalCheckpoint.GuarantorAttestation memory statement, uint256 privateKey) private {
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(privateKey, CanonicalCheckpoint.attestationHash(statement));
        statement.v = v;
        statement.r = r;
        statement.s = s;
    }
}
