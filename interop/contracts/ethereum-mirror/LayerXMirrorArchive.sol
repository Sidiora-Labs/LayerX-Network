// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity 0.8.30;

/// @notice Immutable, permissionless publication of public LayerX archive
/// chunks. The contract has no custody, withdrawal, payable or token surface.
contract LayerXMirrorArchive {
    uint256 public constant MAX_ARCHIVE_BYTES = 64 * 1024 * 1024;
    uint256 public constant MAX_CHUNK_BYTES = 24 * 1024;
    uint256 public constant MAX_CHUNKS = 65_536;
    address public immutable publisher;

    constructor(address publisher_) {
        if (publisher_ == address(0)) revert InvalidPublisher();
        publisher = publisher_;
    }

    struct ArchiveManifest {
        uint32 networkId;
        uint64 batchNumber;
        bytes32 checkpointId;
        uint64 totalBytes;
        uint32 totalChunks;
        bytes32 archiveDigest;
        bytes32 expectedChunkChain;
        bytes32 observedChunkChain;
        uint64 receivedBytes;
        uint32 nextChunk;
        bool finalized;
    }

    mapping(bytes32 => ArchiveManifest) private _manifests;
    mapping(bytes32 => mapping(uint32 => bytes)) private _chunks;

    event ManifestOpened(
        bytes32 indexed commitment,
        uint32 networkId,
        uint64 batchNumber,
        uint64 totalBytes,
        uint32 totalChunks,
        bytes32 archiveDigest
    );
    event ChunkStored(
        bytes32 indexed commitment,
        uint32 index,
        bytes32 chunkDigest,
        uint32 chunkBytes
    );
    event ArchiveFinalized(bytes32 indexed commitment, bytes32 archiveDigest);

    error InvalidManifest();
    error ManifestConflict();
    error ChunkOrder();
    error ChunkConflict();
    error IncompleteArchive();
    error InvalidPublisher();

    modifier onlyPublisher() {
        if (msg.sender != publisher) revert InvalidPublisher();
        _;
    }

    function begin(
        bytes32 commitment,
        uint32 networkId,
        uint64 batchNumber,
        bytes32 checkpointId,
        uint64 totalBytes,
        uint32 totalChunks,
        bytes32 archiveDigest,
        bytes32 expectedChunkChain
    ) external onlyPublisher {
        if (
            commitment == bytes32(0) || networkId == 0 || batchNumber == 0 ||
            totalBytes == 0 || totalBytes > MAX_ARCHIVE_BYTES ||
            totalChunks == 0 || totalChunks > MAX_CHUNKS ||
            archiveDigest == bytes32(0) || expectedChunkChain == bytes32(0)
        ) revert InvalidManifest();
        ArchiveManifest storage existing = _manifests[commitment];
        if (existing.totalBytes != 0) {
            if (
                existing.networkId != networkId || existing.batchNumber != batchNumber ||
                existing.checkpointId != checkpointId || existing.totalBytes != totalBytes ||
                existing.totalChunks != totalChunks || existing.archiveDigest != archiveDigest ||
                existing.expectedChunkChain != expectedChunkChain
            ) revert ManifestConflict();
            return;
        }
        _manifests[commitment] = ArchiveManifest({
            networkId: networkId,
            batchNumber: batchNumber,
            checkpointId: checkpointId,
            totalBytes: totalBytes,
            totalChunks: totalChunks,
            archiveDigest: archiveDigest,
            expectedChunkChain: expectedChunkChain,
            observedChunkChain: bytes32(0),
            receivedBytes: 0,
            nextChunk: 0,
            finalized: false
        });
        emit ManifestOpened(
            commitment, networkId, batchNumber, totalBytes, totalChunks, archiveDigest
        );
    }

    function append(bytes32 commitment, uint32 index, bytes calldata value) external onlyPublisher {
        ArchiveManifest storage archive = _manifests[commitment];
        if (archive.totalBytes == 0 || archive.finalized) revert InvalidManifest();
        if (value.length == 0 || value.length > MAX_CHUNK_BYTES) revert InvalidManifest();
        bytes32 digest = sha256(value);
        if (index < archive.nextChunk) {
            if (sha256(_chunks[commitment][index]) != digest) revert ChunkConflict();
            return;
        }
        if (index != archive.nextChunk || index >= archive.totalChunks) revert ChunkOrder();
        uint256 nextBytes = uint256(archive.receivedBytes) + value.length;
        if (nextBytes > archive.totalBytes) revert InvalidManifest();
        _chunks[commitment][index] = value;
        archive.observedChunkChain = keccak256(
            abi.encodePacked(archive.observedChunkChain, index, digest, uint32(value.length))
        );
        archive.receivedBytes = uint64(nextBytes);
        archive.nextChunk = index + 1;
        emit ChunkStored(commitment, index, digest, uint32(value.length));
    }

    function finalize(bytes32 commitment) external onlyPublisher {
        ArchiveManifest storage archive = _manifests[commitment];
        if (archive.finalized) return;
        if (
            archive.totalBytes == 0 || archive.nextChunk != archive.totalChunks ||
            archive.receivedBytes != archive.totalBytes ||
            archive.observedChunkChain != archive.expectedChunkChain
        ) revert IncompleteArchive();
        archive.finalized = true;
        emit ArchiveFinalized(commitment, archive.archiveDigest);
    }

    function manifest(bytes32 commitment)
        external
        view
        returns (uint64 totalBytes, uint32 totalChunks, bytes32 archiveDigest, bool finalized)
    {
        ArchiveManifest storage archive = _manifests[commitment];
        return (
            archive.totalBytes, archive.totalChunks, archive.archiveDigest, archive.finalized
        );
    }

    function chunk(bytes32 commitment, uint32 index) external view returns (bytes memory) {
        return _chunks[commitment][index];
    }
}
