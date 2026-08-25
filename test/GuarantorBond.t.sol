// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {CanonicalCheckpoint} from "../contracts/libraries/CanonicalCheckpoint.sol";
import {GuarantorBond} from "../contracts/GuarantorBond.sol";

interface BondVm {
    function addr(uint256 privateKey) external returns (address);
    function deal(address account, uint256 balance) external;
    function prank(address sender) external;
    function sign(uint256 privateKey, bytes32 digest) external returns (uint8 v, bytes32 r, bytes32 s);
    function expectRevert(bytes4 selector) external;
}

contract GuarantorBondTest {
    BondVm private constant vm = BondVm(address(uint160(uint256(keccak256("hevm cheat code")))));
    bytes32 private constant CONFIG = keccak256("bond-test-config");
    uint192 private constant RELEASE = uint192(1) << 128;

    function testPermissionlessPortableEquivocationSlashes() public {
        uint256 privateKey = 7;
        address signer = vm.addr(privateKey);
        bytes32 guarantorId = bytes32(uint256(7));
        GuarantorBond bond = new GuarantorBond(address(this), address(this), 100, 100 ether, 7 days, CONFIG, RELEASE);
        bond.activateGuarantor(guarantorId, signer, signer, 1, 1);
        vm.deal(signer, 2 ether);
        vm.prank(signer);
        bond.depositBond{value: 1 ether}(guarantorId);
        CanonicalCheckpoint.GuarantorAttestation memory first = _statement(guarantorId, signer, bytes32(uint256(1)));
        CanonicalCheckpoint.GuarantorAttestation memory second = _statement(guarantorId, signer, bytes32(uint256(2)));
        _sign(first, privateKey);
        _sign(second, privateKey);
        vm.prank(address(0xBEEF));
        bond.submitEquivocation(first, second, 4);
        GuarantorBond.BondRecord memory record = bond.bondRecord(guarantorId);
        require(record.jailed && record.amount == 0 && record.removedEpoch == 4, "not slashed");
        require(bond.slashedBalance() == 1 ether, "slash not conserved");
        require(bond.membershipVersion() == 2, "evidence removal was not versioned");
    }

    function testGovernedRemovalStartsUnbonding() public {
        uint256 privateKey = 8;
        address signer = vm.addr(privateKey);
        bytes32 guarantorId = bytes32(uint256(8));
        GuarantorBond bond = new GuarantorBond(address(this), address(this), 100, 100 ether, 7 days, CONFIG, RELEASE);
        bond.activateGuarantor(guarantorId, signer, signer, 1, 1);
        vm.deal(signer, 2 ether);
        vm.prank(signer);
        bond.depositBond{value: 1 ether}(guarantorId);
        require(bond.bondedActive(guarantorId, signer, 1), "not active");
        bond.removeGuarantor(guarantorId, 2, 2);
        vm.prank(signer);
        bond.beginUnbond(guarantorId, 1 ether);
        require(bond.bondedActive(guarantorId, signer, 1), "historical eligibility was erased");
        require(!bond.bondedActive(guarantorId, signer, 2), "removed signer remained active");
    }

    function testFundingCannotCreateOrBackdateMembership() public {
        GuarantorBond bond = new GuarantorBond(address(this), address(this), 100, 100 ether, 7 days, CONFIG, RELEASE);
        bytes32 guarantorId = bytes32(uint256(9));
        address signer = vm.addr(9);
        address funder = address(0xF00D);
        vm.deal(funder, 2 ether);

        vm.expectRevert(GuarantorBond.InvalidBondAction.selector);
        vm.prank(funder);
        bond.depositBond{value: 1 ether}(guarantorId);

        vm.expectRevert(GuarantorBond.Unauthorized.selector);
        vm.prank(funder);
        bond.activateGuarantor(guarantorId, signer, signer, 7, 1);

        bond.activateGuarantor(guarantorId, signer, signer, 7, 1);
        vm.prank(funder);
        bond.depositBond{value: 1 ether}(guarantorId);
        require(bond.bondRecord(guarantorId).amount == 1 ether, "permissionless funding not recorded");
        require(!bond.bondedActive(guarantorId, signer, 6), "membership was backdated");
        require(bond.bondedActive(guarantorId, signer, 7), "authorized activation missing");
    }

    function testRotationAndRemovalAreVersionedAndEpochBound() public {
        GuarantorBond bond = new GuarantorBond(address(this), address(this), 100, 100 ether, 7 days, CONFIG, RELEASE);
        bytes32 guarantorId = bytes32(uint256(10));
        address originalSigner = vm.addr(10);
        address rotatedSigner = vm.addr(11);
        bond.activateGuarantor(guarantorId, originalSigner, originalSigner, 2, 1);
        vm.deal(address(0xB0A0), 2 ether);
        vm.prank(address(0xB0A0));
        bond.depositBond{value: 1 ether}(guarantorId);

        bond.rotateGuarantorSigner(guarantorId, rotatedSigner, 5, 2);
        require(bond.bondedActive(guarantorId, originalSigner, 4), "old signer history lost");
        require(!bond.bondedActive(guarantorId, originalSigner, 5), "old signer survived rotation");
        require(!bond.bondedActive(guarantorId, rotatedSigner, 4), "new signer activated early");
        require(bond.bondedActive(guarantorId, rotatedSigner, 5), "new signer not activated");

        vm.expectRevert(GuarantorBond.InvalidBondAction.selector);
        bond.removeGuarantor(guarantorId, 9, 2);
        bond.removeGuarantor(guarantorId, 9, 3);
        require(bond.membershipVersion() == 3 && bond.lastGovernanceSequence() == 3, "set version drift");
        require(bond.bondedActive(guarantorId, rotatedSigner, 8), "pre-removal history lost");
        require(!bond.bondedActive(guarantorId, rotatedSigner, 9), "removed signer remained active");
        vm.expectRevert(GuarantorBond.InvalidBondAction.selector);
        vm.prank(rotatedSigner);
        bond.beginUnbond(guarantorId, 1 ether);
        vm.prank(originalSigner);
        bond.beginUnbond(guarantorId, 1 ether);
        require(bond.bondRecord(guarantorId).pendingWithdrawal == 1 ether, "bond controller lost custody");
    }

    function _statement(bytes32 guarantorId, address signer, bytes32 checkpointHash)
        private
        pure
        returns (CanonicalCheckpoint.GuarantorAttestation memory statement)
    {
        statement = CanonicalCheckpoint.GuarantorAttestation({
            checkpointId: bytes32(uint256(99)),
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
