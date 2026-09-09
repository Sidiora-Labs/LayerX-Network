// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;

import {NativeStateProof} from "../contracts/libraries/NativeStateProof.sol";

interface NativeStateProofVm {
    function readFile(string calldata path) external view returns (string memory);
    function parseJsonBytes(string calldata json, string calldata key) external pure returns (bytes memory);
    function parseJsonBytes32(string calldata json, string calldata key) external pure returns (bytes32);
    function toString(uint256 value) external pure returns (string memory);
    function expectRevert(bytes4 selector) external;
}

contract NativeStateProofTest {
    NativeStateProofVm private constant vm =
        NativeStateProofVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    function verify(bytes calldata proof, uint16 moduleId, bytes32 root) external pure {
        NativeStateProof.verify(proof, moduleId, root);
    }

    function testNativeVectorsAndRefusals() external {
        string memory vectors = vm.readFile("contracts/config/native-state-proofs.json");
        for (uint256 i; i < 10; ++i) {
            string memory base = string.concat(".vectors[", vm.toString(i), "]");
            bytes memory proof = vm.parseJsonBytes(vectors, string.concat(base, ".proof"));
            bytes32 root = vm.parseJsonBytes32(vectors, string.concat(base, ".root"));
            uint16 moduleId = uint16(uint8(proof[2])) * 256 + uint16(uint8(proof[3]));
            this.verify(proof, moduleId, root);
            vm.expectRevert(NativeStateProof.RootMismatch.selector);
            this.verify(proof, moduleId, root ^ bytes32(uint256(1)));
            vm.expectRevert(NativeStateProof.WrongModule.selector);
            this.verify(proof, moduleId == 0 ? 1 : 0, root);
            proof[1] = 0x01;
            vm.expectRevert(NativeStateProof.WrongVersion.selector);
            this.verify(proof, moduleId, root);
            proof[1] = 0x02;
            proof[3] = 0x0a;
            vm.expectRevert(NativeStateProof.WrongModule.selector);
            this.verify(proof, 10, root);
            proof[3] = bytes1(uint8(moduleId));
            uint256 keyLength = uint32(bytes4(slice(proof, 4, 4)));
            uint256 valueAt = 8 + keyLength;
            uint256 valueLength = uint32(bytes4(slice(proof, valueAt, 4)));
            uint256 pathAt = valueAt + 4 + valueLength;
            bytes1 depth = proof[pathAt + 8];
            proof[pathAt + 8] = 0x21;
            vm.expectRevert(NativeStateProof.InvalidPath.selector);
            this.verify(proof, moduleId, root);
            proof[pathAt + 8] = depth;
            proof[pathAt + 4] = 0;
            proof[pathAt + 5] = 0;
            proof[pathAt + 6] = 0;
            proof[pathAt + 7] = 0;
            vm.expectRevert(NativeStateProof.InvalidPath.selector);
            this.verify(proof, moduleId, root);
        }
    }

    struct WithdrawalFact {
        uint32 network;
        bytes32 withdrawalId;
        bytes32 account;
        bytes32 asset;
        uint128 amount;
        address recipient;
        bytes32 anchor;
        bytes32 nullifier;
    }

    function verifyWithdrawal(bytes calldata proof, bytes32 root, WithdrawalFact calldata fact) external pure {
        NativeStateProof.verifyWithdrawal(
            proof,
            root,
            fact.network,
            fact.withdrawalId,
            fact.account,
            fact.asset,
            fact.amount,
            fact.recipient,
            fact.anchor,
            fact.nullifier
        );
    }

    function testNativeSignedWithdrawalVectorAndRefusals() external {
        string memory vector = vm.readFile("contracts/config/native-withdrawal-proof.json");
        bytes memory proof = vm.parseJsonBytes(vector, ".proof");
        bytes32 root = vm.parseJsonBytes32(vector, ".root");
        require(uint32(bytes4(slice(proof, 4, 4))) == 43, "native withdrawal key length");
        require(uint32(bytes4(slice(proof, 51, 4))) == 182, "native withdrawal value length");
        WithdrawalFact memory fact = WithdrawalFact({
            network: uint32(bytes4(slice(proof, 57, 4))),
            withdrawalId: bytes32(slice(proof, 61, 32)),
            account: bytes32(slice(proof, 93, 32)),
            asset: bytes32(slice(proof, 125, 32)),
            amount: uint128(bytes16(slice(proof, 157, 16))),
            recipient: address(bytes20(slice(proof, 185, 20))),
            anchor: bytes32(slice(proof, 205, 32)),
            nullifier: bytes32(slice(proof, 19, 32))
        });
        require(fact.network == 7 && fact.amount == 25, "native dispatch fixture facts");
        this.verifyWithdrawal(proof, root, fact);
        proof[1] = 0x01;
        vm.expectRevert(NativeStateProof.WrongVersion.selector);
        this.verifyWithdrawal(proof, root, fact);
        proof[1] = 0x02;
        proof[3] = 0x02;
        vm.expectRevert(NativeStateProof.WrongModule.selector);
        this.verifyWithdrawal(proof, root, fact);
        proof[3] = 0x01;
        vm.expectRevert(NativeStateProof.RootMismatch.selector);
        this.verifyWithdrawal(proof, root ^ bytes32(uint256(1)), fact);
        for (uint256 i; i < 8; ++i) {
            WithdrawalFact memory altered = abi.decode(abi.encode(fact), (WithdrawalFact));
            if (i == 0) altered.network ^= 1;
            else if (i == 1) altered.withdrawalId ^= bytes32(uint256(1));
            else if (i == 2) altered.account ^= bytes32(uint256(1));
            else if (i == 3) altered.asset ^= bytes32(uint256(1));
            else if (i == 4) altered.amount += 1;
            else if (i == 5) altered.recipient = address(uint160(altered.recipient) ^ 1);
            else if (i == 6) altered.anchor ^= bytes32(uint256(1));
            else altered.nullifier ^= bytes32(uint256(1));
            vm.expectRevert(NativeStateProof.InvalidEncoding.selector);
            this.verifyWithdrawal(proof, root, altered);
        }
        bytes1 depth = proof[245];
        proof[245] = 0x21;
        vm.expectRevert(NativeStateProof.InvalidPath.selector);
        this.verifyWithdrawal(proof, root, fact);
        proof[245] = depth;
        vm.expectRevert(NativeStateProof.InvalidPath.selector);
        this.verifyWithdrawal(slice(proof, 0, proof.length - 1), root, fact);
    }

    function slice(bytes memory input, uint256 start, uint256 length) private pure returns (bytes memory out) {
        out = new bytes(length);
        for (uint256 i; i < length; ++i) {
            out[i] = input[start + i];
        }
    }
}
