// SPDX-License-Identifier: Apache-2.0
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
    function payloadOfLength(uint256 length) private pure returns (bytes memory payload) {
        payload = new bytes(length);
        for (uint256 i = 0; i < length; ++i) payload[i] = bytes1(uint8(i));
    }

    function chunkOf(bytes memory payload, uint256 offset, uint256 size) private pure returns (bytes memory chunk) {
        uint256 length = payload.length - offset;
        if (length > size) length = size;
        chunk = new bytes(length);
        for (uint256 i = 0; i < length; ++i) chunk[i] = payload[offset + i];
    }

    function openAndAppend(
        LayerXMirrorArchive archive,
        bytes32 commitment,
        bytes memory payload,
        uint256 chunkBytes,
        bytes32 digest
    ) private {
        uint32 count = uint32((payload.length + chunkBytes - 1) / chunkBytes);
        bytes32 chain;
        for (uint32 i = 0; i < count; ++i) {
            bytes memory chunk = chunkOf(payload, uint256(i) * chunkBytes, chunkBytes);
            chain = keccak256(abi.encodePacked(chain, i, sha256(chunk), uint32(chunk.length)));
        }
        archive.begin(commitment, 1, 7, bytes32(0), uint64(payload.length), count, digest, chain);
        for (uint32 i = 0; i < count; ++i) {
            bytes memory chunk = chunkOf(payload, uint256(i) * chunkBytes, chunkBytes);
            archive.append(commitment, i, chunk);
            archive.append(commitment, i, chunk);
            require(keccak256(archive.chunk(commitment, i)) == keccak256(chunk), "stored chunk differs");
            archive.begin(commitment, 1, 7, bytes32(0), uint64(payload.length), count, digest, chain);
        }
    }

    function requireRefusal(bool success, bytes memory reason, bytes4 expected) private pure {
        require(!success && reason.length >= 4, "operation did not refuse");
        bytes4 selector;
        assembly ("memory-safe") {
            selector := mload(add(reason, 32))
        }
        require(selector == expected, "unexpected refusal");
    }

    function testRejectsIncorrectArchiveDigestWithCorrectChunkChain() public {
        LayerXMirrorArchive archive = new LayerXMirrorArchive(address(this));
        bytes memory payload = payloadOfLength(193);
        bytes32 commitment = keccak256("incorrect-digest");
        bytes32 wrongDigest = bytes32(uint256(sha256(payload)) ^ 1);
        openAndAppend(archive, commitment, payload, 63, wrongDigest);
        (bool success, bytes memory reason) = address(archive).call(abi.encodeCall(archive.finalize, (commitment)));
        requireRefusal(success, reason, LayerXMirrorArchive.ArchiveDigestMismatch.selector);
        (, , bytes32 storedDigest, bool finalized) = archive.manifest(commitment);
        require(storedDigest == wrongDigest && !finalized, "invalid archive finalized");
        (success, reason) = address(archive).call(abi.encodeCall(archive.finalize, (commitment)));
        requireRefusal(success, reason, LayerXMirrorArchive.ArchiveDigestMismatch.selector);
    }

    function testMultiChunkPaddingBoundariesAndRetries() public {
        LayerXMirrorArchive archive = new LayerXMirrorArchive(address(this));
        uint256[13] memory lengths = [uint256(1), 55, 56, 63, 64, 65, 119, 120, 127, 128, 129, 193, 1025];
        for (uint256 i = 0; i < lengths.length; ++i) {
            bytes memory payload = payloadOfLength(lengths[i]);
            bytes32 commitment = keccak256(abi.encodePacked("padding", lengths[i]));
            openAndAppend(archive, commitment, payload, 31, sha256(payload));
            archive.finalize(commitment);
            archive.finalize(commitment);
            (, , bytes32 digest, bool finalized) = archive.manifest(commitment);
            require(digest == sha256(payload) && finalized, "SHA256 padding mismatch");
        }
    }

    function testMaximumChunkAndRemainder() public {
        LayerXMirrorArchive archive = new LayerXMirrorArchive(address(this));
        bytes memory payload = payloadOfLength(archive.MAX_CHUNK_BYTES() + 63);
        bytes32 commitment = keccak256("maximum-chunk");
        openAndAppend(archive, commitment, payload, archive.MAX_CHUNK_BYTES(), sha256(payload));
        archive.finalize(commitment);
        (, , bytes32 digest, bool finalized) = archive.manifest(commitment);
        require(digest == sha256(payload) && finalized, "maximum chunk digest mismatch");
    }

    function testFailedAppendDoesNotAdvanceDigest() public {
        LayerXMirrorArchive archive = new LayerXMirrorArchive(address(this));
        bytes memory first = payloadOfLength(63);
        bytes memory second = payloadOfLength(65);
        bytes memory payload = bytes.concat(first, second);
        bytes32 commitment = keccak256("failed-append");
        bytes32 chain = keccak256(abi.encodePacked(bytes32(0), uint32(0), sha256(first), uint32(first.length)));
        chain = keccak256(abi.encodePacked(chain, uint32(1), sha256(second), uint32(second.length)));
        archive.begin(commitment, 1, 7, bytes32(0), uint64(payload.length), 2, sha256(payload), chain);
        (bool success, bytes memory reason) = address(archive).call(abi.encodeCall(archive.append, (commitment, 1, second)));
        requireRefusal(success, reason, LayerXMirrorArchive.ChunkOrder.selector);
        archive.append(commitment, 0, first);
        (success, reason) = address(archive).call(abi.encodeCall(archive.append, (commitment, 0, second)));
        requireRefusal(success, reason, LayerXMirrorArchive.ChunkConflict.selector);
        (success, reason) = address(archive).call(abi.encodeCall(archive.finalize, (commitment)));
        requireRefusal(success, reason, LayerXMirrorArchive.IncompleteArchive.selector);
        bytes memory oversize = payloadOfLength(archive.MAX_CHUNK_BYTES() + 1);
        (success, reason) = address(archive).call(abi.encodeCall(archive.append, (commitment, 1, oversize)));
        requireRefusal(success, reason, LayerXMirrorArchive.InvalidManifest.selector);
        archive.append(commitment, 0, first);
        archive.append(commitment, 1, second);
        archive.finalize(commitment);
        (, , bytes32 digest, bool finalized) = archive.manifest(commitment);
        require(digest == sha256(payload) && finalized, "refusal advanced digest");
    }

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
