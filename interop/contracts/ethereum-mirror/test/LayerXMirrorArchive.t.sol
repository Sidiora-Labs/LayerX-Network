// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity 0.8.30;

import {LayerXMirrorArchive} from "../LayerXMirrorArchive.sol";

contract MirrorUnauthorizedCaller {
    function begin(LayerXMirrorArchive archive) external {
        archive.begin(
            keccak256("unauthorized-commitment"),
            1,
            1,
            keccak256("checkpoint"),
            1,
            1,
            keccak256("archive"),
            keccak256("chain")
        );
    }
}

contract LayerXMirrorArchiveTest {
    function testArchiveRoundTripAndIdempotence() public {
        LayerXMirrorArchive archive = new LayerXMirrorArchive(address(this));
        bytes memory payload = hex"0102030405";
        bytes32 commitment = keccak256("commitment");
        bytes32 digest = sha256(payload);
        bytes32 expectedChain = keccak256(abi.encodePacked(bytes32(0), uint32(0), digest, uint32(payload.length)));

        archive.begin(commitment, 1, 7, keccak256("checkpoint"), uint64(payload.length), 1, digest, expectedChain);
        archive.begin(commitment, 1, 7, keccak256("checkpoint"), uint64(payload.length), 1, digest, expectedChain);
        archive.append(commitment, 0, payload);
        archive.append(commitment, 0, payload);
        archive.finalize(commitment);
        archive.finalize(commitment);

        (uint64 totalBytes, uint32 totalChunks, bytes32 archiveDigest, bool finalized) = archive.manifest(commitment);
        require(totalBytes == payload.length, "total bytes");
        require(totalChunks == 1, "total chunks");
        require(archiveDigest == digest, "archive digest");
        require(finalized, "not finalized");
        require(keccak256(archive.chunk(commitment, 0)) == keccak256(payload), "chunk mismatch");
    }

    function testOnlyPublisherCanOpenArchive() public {
        LayerXMirrorArchive archive = new LayerXMirrorArchive(address(this));
        MirrorUnauthorizedCaller caller = new MirrorUnauthorizedCaller();
        (bool success, bytes memory reason) =
            address(caller).call(abi.encodeCall(MirrorUnauthorizedCaller.begin, (archive)));
        require(!success && reason.length >= 4, "unauthorized publication accepted");
        bytes4 selector;
        assembly ("memory-safe") {
            selector := mload(add(reason, 32))
        }
        require(selector == LayerXMirrorArchive.InvalidPublisher.selector, "wrong refusal");
    }

    function testMirrorRejectsValue() public {
        LayerXMirrorArchive archive = new LayerXMirrorArchive(address(this));
        (bool success,) = address(archive).call{value: 1}("");
        require(!success, "value accepted");
    }

    receive() external payable {}
}
