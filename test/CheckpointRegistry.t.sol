// SPDX-License-Identifier: LicenseRef-Centra-ai-Protocol
pragma solidity ^0.8.24;

import {CanonicalCheckpoint} from "../contracts/libraries/CanonicalCheckpoint.sol";
import {CheckpointRegistry} from "../contracts/CheckpointRegistry.sol";
import {GuarantorBond} from "../contracts/GuarantorBond.sol";
import {Constants} from "../contracts/libraries/Constants.sol";

interface CheckpointVm {
    function addr(uint256 privateKey) external returns (address);
    function deal(address account, uint256 balance) external;
    function prank(address sender) external;
    function sign(uint256 privateKey, bytes32 digest) external returns (uint8 v, bytes32 r, bytes32 s);
    function warp(uint256 timestamp) external;
    function expectRevert(bytes4 selector) external;
    function etch(address target, bytes calldata code) external;
    function readFile(string calldata path) external view returns (string memory);
    function keyExistsJson(string calldata json, string calldata key) external pure returns (bool);
    function parseJsonUint(string calldata json, string calldata key) external pure returns (uint256);
    function parseJsonBool(string calldata json, string calldata key) external pure returns (bool);
    function parseJsonString(string calldata json, string calldata key) external pure returns (string memory);
    function parseJsonBytes(string calldata json, string calldata key) external pure returns (bytes memory);
    function parseJsonBytes32(string calldata json, string calldata key) external pure returns (bytes32);
    function parseJsonAddress(string calldata json, string calldata key) external pure returns (address);
    function toString(uint256 value) external pure returns (string memory);
}

contract CheckpointToken {
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

    function transferFrom(address sender, address recipient, uint256 amount) external returns (bool) {
        allowance[sender][msg.sender] -= amount;
        balanceOf[sender] -= amount;
        balanceOf[recipient] += amount;
        return true;
    }
}

contract CheckpointRegistryTest {
    CheckpointVm private constant vm = CheckpointVm(address(uint160(uint256(keccak256("hevm cheat code")))));

    GuarantorBond private bond;
    CheckpointRegistry private registry;
    uint256[3] private keys = [uint256(1), uint256(2), uint256(3)];
    bytes32 private constant GENESIS_RECEIPT_ROOT = bytes32(uint256(0x11) << 248);
    bytes32 private constant GENESIS_CANONICAL_STATE_ROOT = bytes32(uint256(0x12) << 248);
    bytes32 private constant GENESIS_MANIFEST_DIGEST = bytes32(uint256(0x13) << 248);
    bytes32 private constant CONFIG = keccak256("checkpoint-test-config");
    uint192 private constant RELEASE = uint192(1) << 128;
    string private constant SETTLEMENT_PATH = "contracts/config/checkpoint-settlement.json";
    string private constant VECTOR_DIRECTORY = "tests/vectors/checkpoint/";
    uint256 private constant VECTOR_GUARANTORS = 3;

    string private settlement;
    uint64 private declaredDelaySeconds;
    uint16 private declaredThreshold;
    uint256 private declaredHeaderLength;
    bytes private declaredHeaderPrefix;
    address private declaredSettlementContract;

    function setUp() public {
        vm.warp(1000);
        _loadDeclaredSettlement();
        CheckpointToken implementation = new CheckpointToken();
        vm.etch(Constants.USDL_TOKEN, address(implementation).code);
        bond = new GuarantorBond(
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
        CheckpointToken token = CheckpointToken(Constants.USDL_TOKEN);
        for (uint256 i = 0; i < keys.length; ++i) {
            address signer = vm.addr(keys[i]);
            bond.activateGuarantor(bytes32(i + 1), signer, signer, 1, uint64(i + 1));
            token.mint(signer, 2 ether);
            vm.prank(signer);
            token.approve(address(bond), 2 ether);
            vm.prank(signer);
            bond.depositBond(bytes32(i + 1), 2 ether);
        }
        require(address(bond) == declaredSettlementContract, "bond differs from the declared settlement contract");
        registry = new CheckpointRegistry(
            bond,
            Constants.PROTOCOL_VERSION,
            42,
            declaredThreshold,
            4,
            declaredDelaySeconds,
            5 minutes,
            GENESIS_MANIFEST_DIGEST,
            GENESIS_CANONICAL_STATE_ROOT,
            GENESIS_RECEIPT_ROOT,
            CONFIG,
            RELEASE
        );
    }

    function _loadDeclaredSettlement() private {
        settlement = vm.readFile(SETTLEMENT_PATH);
        require(_same(vm.parseJsonString(settlement, ".schema"), "layerx/checkpoint-settlement/1"), "settlement schema");
        require(vm.parseJsonUint(settlement, ".protocol_version") == Constants.PROTOCOL_VERSION, "declared protocol");
        require(
            _sameBytes(
                _nulTerminated(vm.parseJsonString(settlement, ".checkpoint_certificate_domain")),
                CanonicalCheckpoint.CHECKPOINT_DOMAIN
            ),
            "declared checkpoint domain differs from CanonicalCheckpoint.CHECKPOINT_DOMAIN"
        );
        require(
            _sameBytes(
                _nulTerminated(vm.parseJsonString(settlement, ".guarantor_attestation_domain")),
                CanonicalCheckpoint.ATTESTATION_DOMAIN
            ),
            "declared attestation domain differs from CanonicalCheckpoint.ATTESTATION_DOMAIN"
        );
        declaredHeaderPrefix = vm.parseJsonBytes(settlement, ".header_encoding_prefix");
        require(_sameBytes(declaredHeaderPrefix, hex"000217010f"), "declared header prefix");
        declaredHeaderLength = vm.parseJsonUint(settlement, ".header_length");
        require(declaredHeaderLength == CanonicalCheckpoint.HEADER_LENGTH, "declared header length");
        uint256 delaySeconds = vm.parseJsonUint(settlement, ".finality_policy.maximum_attestation_delay_seconds");
        require(delaySeconds > 0 && delaySeconds <= type(uint64).max, "declared delay");
        declaredDelaySeconds = uint64(delaySeconds);
        uint256 threshold = vm.parseJsonUint(settlement, ".finality_policy.certificate_threshold");
        require(threshold > 0 && threshold <= VECTOR_GUARANTORS, "declared threshold");
        declaredThreshold = uint16(threshold);
        require(
            vm.parseJsonUint(settlement, ".settlement_domains.vectors.paxeer_chain_id") == block.chainid,
            "declared paxeer chain id differs from the forge chain"
        );
        require(vm.parseJsonUint(settlement, ".settlement_domains.vectors.network_id") == 42, "declared network id");
        declaredSettlementContract = vm.parseJsonAddress(settlement, ".settlement_domains.vectors.settlement_contract");
        for (uint256 i = 0; i < VECTOR_GUARANTORS; ++i) {
            string memory entry = string.concat(".settlement_domains.vectors.guarantor_set[", vm.toString(i), "]");
            require(
                vm.parseJsonBytes32(settlement, string.concat(entry, ".guarantor_id")) == bytes32(i + 1),
                "declared guarantor id"
            );
            require(
                vm.parseJsonAddress(settlement, string.concat(entry, ".signer")) == vm.addr(keys[i]),
                "declared guarantor signer"
            );
        }
        require(!vm.keyExistsJson(settlement, ".settlement_domains.vectors.guarantor_set[3]"), "declared set size");
    }

    function testDeclaredSettlementDrivesRegistryFreshnessPolicy() public view {
        require(
            registry.maximumAttestationDelayMilliseconds() == uint256(declaredDelaySeconds) * 1_000,
            "registry delay differs from the declared value"
        );
        require(
            registry.maximumAttestationDelay() == declaredDelaySeconds,
            "registry delay seconds differ from the declared value"
        );
        require(registry.threshold() == declaredThreshold, "registry threshold differs from the declared value");
        require(registry.networkId() == 42, "registry network id");
    }

    function testVectorFresh() public {
        _runVector("fresh");
    }

    function testVectorTooEarly() public {
        _runVector("too_early");
    }

    function testVectorTooLate() public {
        _runVector("too_late");
    }

    function testVectorBoundaryLow() public {
        _runVector("boundary_low");
    }

    function testVectorBoundaryHigh() public {
        _runVector("boundary_high");
    }

    function _runVector(string memory name) private {
        string memory vector = vm.readFile(string.concat(VECTOR_DIRECTORY, name, ".json"));
        require(_same(vm.parseJsonString(vector, ".schema"), "layerx/checkpoint-vector/1"), "vector schema");
        require(_same(vm.parseJsonString(vector, ".case"), name), "vector case");
        require(_same(vm.parseJsonString(vector, ".settlement_domain"), "vectors"), "vector settlement domain");
        CanonicalCheckpoint.HeaderCommitments memory header = _vectorHeaderFromJson(vector);
        bytes memory encoded = registry.canonicalHeader(header);
        require(encoded.length == declaredHeaderLength, "vector header length");
        require(
            _sameBytes(encoded, vm.parseJsonBytes(vector, ".header.bytes")),
            "Solidity header encoding differs from the vector bytes"
        );
        require(_startsWith(encoded, declaredHeaderPrefix), "Solidity header prefix differs from the declared prefix");
        bytes memory validityProof = vm.parseJsonBytes(vector, ".certificate.validity_proof");
        require(vm.parseJsonUint(vector, ".certificate.threshold") == declaredThreshold, "vector threshold");
        bytes32 digest = registry.checkpointHash(header, validityProof);
        require(
            digest == vm.parseJsonBytes32(vector, ".expected_digest"),
            "Solidity checkpoint identity differs from the expected digest"
        );
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _vectorAttestations(vector, header, digest);
        string memory rejection = vm.parseJsonString(vector, ".expected_rejection");
        if (_same(vm.parseJsonString(vector, ".expected_outcome"), "accept")) {
            require(_same(rejection, "none"), "accept vector names a rejection");
            registry.registerCheckpoint(header, validityProof, attestations);
            require(registry.checkpointAtBatch(header.batchNumber) == digest, "vector checkpoint not registered");
            require(registry.checkpointTimestamp(digest) == header.timestamp, "vector timestamp not recorded");
        } else {
            require(_same(vm.parseJsonString(vector, ".expected_outcome"), "reject"), "vector outcome");
            require(_same(rejection, "not_yet_valid") || _same(rejection, "expired"), "vector rejection");
            vm.expectRevert(CheckpointRegistry.InvalidCertificate.selector);
            registry.registerCheckpoint(header, validityProof, attestations);
            require(registry.checkpointAtBatch(header.batchNumber) == bytes32(0), "rejected vector was registered");
        }
    }

    function _vectorHeaderFromJson(string memory vector)
        private
        view
        returns (CanonicalCheckpoint.HeaderCommitments memory header)
    {
        header.protocolVersion = uint16(vm.parseJsonUint(vector, ".header.protocol_version"));
        header.networkId = uint32(vm.parseJsonUint(vector, ".header.network_id"));
        header.epoch = uint64(vm.parseJsonUint(vector, ".header.epoch"));
        header.batchNumber = uint64(vm.parseJsonUint(vector, ".header.batch_number"));
        header.firstSequence = uint64(vm.parseJsonUint(vector, ".header.first_sequence"));
        header.lastSequence = uint64(vm.parseJsonUint(vector, ".header.last_sequence"));
        header.previousStateRoot = vm.parseJsonBytes32(vector, ".header.previous_state_root");
        header.resultingStateRoot = vm.parseJsonBytes32(vector, ".header.resulting_state_root");
        header.activityMerkleRoot = vm.parseJsonBytes32(vector, ".header.activity_merkle_root");
        header.receiptMerkleRoot = vm.parseJsonBytes32(vector, ".header.receipt_merkle_root");
        header.eventMerkleRoot = vm.parseJsonBytes32(vector, ".header.event_merkle_root");
        header.dataAvailabilityRoot = vm.parseJsonBytes32(vector, ".header.data_availability_root");
        header.oracleRoot = vm.parseJsonBytes32(vector, ".header.oracle_root");
        header.timestamp = uint64(vm.parseJsonUint(vector, ".header.timestamp_ms"));
        header.sequencerId = vm.parseJsonBytes32(vector, ".header.sequencer_id");
        require(header.protocolVersion == Constants.PROTOCOL_VERSION, "vector protocol version");
        require(header.networkId == 42, "vector network id");
    }

    function _vectorAttestations(
        string memory vector,
        CanonicalCheckpoint.HeaderCommitments memory header,
        bytes32 digest
    ) private view returns (CanonicalCheckpoint.GuarantorAttestation[] memory attestations) {
        require(!vm.keyExistsJson(vector, ".attestations[3]"), "vector attestation count");
        attestations = new CanonicalCheckpoint.GuarantorAttestation[](VECTOR_GUARANTORS);
        for (uint256 i = 0; i < VECTOR_GUARANTORS; ++i) {
            attestations[i] = _vectorAttestation(vector, header, digest, i);
        }
    }

    function _vectorAttestation(
        string memory vector,
        CanonicalCheckpoint.HeaderCommitments memory header,
        bytes32 digest,
        uint256 index
    ) private view returns (CanonicalCheckpoint.GuarantorAttestation memory attestation) {
        string memory entry = string.concat(".attestations[", vm.toString(index), "]");
        bytes memory signature = vm.parseJsonBytes(vector, string.concat(entry, ".signature"));
        require(signature.length == 64, "vector signature width");
        attestation.protocolVersion = header.protocolVersion;
        attestation.networkId = header.networkId;
        attestation.paxeerChainId = uint64(block.chainid);
        attestation.settlementContract = declaredSettlementContract;
        attestation.epoch = header.epoch;
        attestation.checkpointId = digest;
        attestation.checkpointHash = digest;
        attestation.guarantorId = vm.parseJsonBytes32(vector, string.concat(entry, ".guarantor_id"));
        attestation.batchNumber = header.batchNumber;
        attestation.dataAvailabilityRoot = header.dataAvailabilityRoot;
        attestation.replayed = vm.parseJsonBool(vector, string.concat(entry, ".replayed"));
        attestation.dataAvailable = vm.parseJsonBool(vector, string.concat(entry, ".data_possessed"));
        attestation.availabilityClassMask =
            uint8(vm.parseJsonUint(vector, string.concat(entry, ".availability_class_mask")));
        attestation.attestedAt = uint64(vm.parseJsonUint(vector, string.concat(entry, ".attested_at_ms")));
        attestation.signer = vm.parseJsonAddress(vector, string.concat(entry, ".signer"));
        (attestation.r, attestation.s) = _splitSignature(signature);
        attestation.v = uint8(vm.parseJsonUint(vector, string.concat(entry, ".signature_v")));
        _checkVectorAttestation(vector, entry, index, attestation);
    }

    function _checkVectorAttestation(
        string memory vector,
        string memory entry,
        uint256 index,
        CanonicalCheckpoint.GuarantorAttestation memory attestation
    ) private view {
        string memory declared = string.concat(".settlement_domains.vectors.guarantor_set[", vm.toString(index), "]");
        require(
            attestation.guarantorId == vm.parseJsonBytes32(settlement, string.concat(declared, ".guarantor_id")),
            "vector guarantor differs from the declared set"
        );
        require(
            attestation.signer == vm.parseJsonAddress(settlement, string.concat(declared, ".signer")),
            "vector signer differs from the declared set"
        );
        bytes32 attestationDigest = CanonicalCheckpoint.attestationHash(attestation);
        require(
            attestationDigest == vm.parseJsonBytes32(vector, string.concat(entry, ".digest")),
            "Solidity attestation digest differs from the vector digest"
        );
        require(
            sha256(
                abi.encodePacked(
                    CanonicalCheckpoint.ATTESTATION_DOMAIN, vm.parseJsonBytes(vector, string.concat(entry, ".message"))
                )
            ) == attestationDigest,
            "Solidity attestation digest differs from the declared domain over the vector message"
        );
        require(
            ecrecover(attestationDigest, attestation.v, attestation.r, attestation.s) == attestation.signer,
            "vector signature does not recover the declared signer"
        );
    }

    function _splitSignature(bytes memory signature) private pure returns (bytes32 r, bytes32 s) {
        assembly {
            r := mload(add(signature, 32))
            s := mload(add(signature, 64))
        }
    }

    function _nulTerminated(string memory text) private pure returns (bytes memory) {
        return bytes.concat(bytes(text), hex"00");
    }

    function _same(string memory left, string memory right) private pure returns (bool) {
        return keccak256(bytes(left)) == keccak256(bytes(right));
    }

    function _sameBytes(bytes memory left, bytes memory right) private pure returns (bool) {
        return keccak256(left) == keccak256(right);
    }

    function _startsWith(bytes memory data, bytes memory prefix) private pure returns (bool) {
        if (data.length < prefix.length) return false;
        for (uint256 i = 0; i < prefix.length; ++i) {
            if (data[i] != prefix[i]) return false;
        }
        return true;
    }

    function testCanonicalHeaderMatchesCVector() public view {
        CanonicalCheckpoint.HeaderCommitments memory header = _vectorHeader();
        bytes memory encoded = registry.canonicalHeader(header);
        require(encoded.length == 354, "header length");
        require(encoded[0] == bytes1(0) && encoded[1] == bytes1(uint8(2)), "outer header version");
        require(
            registry.checkpointHash(header, "") == 0xf5d35dfd948812aac72e8bc5bd87c57be8377c891e9b9fec6d84350cdd8ff743,
            "C checkpoint vector mismatch"
        );
    }

    function testCanonicalAttestationMatchesProtocolV2GoldenVector() public pure {
        CanonicalCheckpoint.GuarantorAttestation memory attestation = CanonicalCheckpoint.GuarantorAttestation({
            protocolVersion: 2,
            networkId: 42,
            paxeerChainId: 31_337,
            settlementContract: address(0x1111111111111111111111111111111111111111),
            epoch: 1,
            checkpointId: bytes32(uint256(0xaa) << 248),
            checkpointHash: bytes32(uint256(0xaa) << 248),
            guarantorId: bytes32(uint256(1) << 248),
            batchNumber: 1,
            dataAvailabilityRoot: bytes32(uint256(0x66) << 248),
            replayed: true,
            dataAvailable: true,
            availabilityClassMask: 0x1f,
            attestedAt: 1001,
            signer: address(0),
            r: bytes32(0),
            s: bytes32(0),
            v: 0
        });
        require(
            CanonicalCheckpoint.attestationHash(attestation)
                == 0xc8c4749f2a9ee3d933696bebb1c0543e028c9fe22aac54f4ff33c124b2bbf991,
            "C attestation vector mismatch"
        );
    }

    function testInitialContinuityUsesGenesisReceiptRoot() public view {
        require(registry.protocolVersion() == 2, "protocol v2 not active");
        require(registry.latestFinalisedStateRoot() == GENESIS_RECEIPT_ROOT, "genesis receipt root");
        require(registry.genesisManifestDigest() == GENESIS_MANIFEST_DIGEST, "genesis manifest digest");
        require(registry.genesisCanonicalStateRoot() == GENESIS_CANONICAL_STATE_ROOT, "canonical root");
        require(registry.genesisReceiptRoot() == GENESIS_RECEIPT_ROOT, "receipt root");
        require(registry.genesisCheckpointId() == registry.derivedGenesisCheckpointId(), "genesis checkpoint id");
    }

    function testFirstCheckpointRejectsCanonicalRootAndAcceptsReceiptRoot() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        header.previousStateRoot = GENESIS_CANONICAL_STATE_ROOT;
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        vm.expectRevert(CheckpointRegistry.StateRootDiscontinuity.selector);
        registry.registerCheckpoint(header, "", attestations);

        header.previousStateRoot = GENESIS_RECEIPT_ROOT;
        digest = registry.checkpointHash(header, "");
        attestations = _attestations(header, digest, 2);
        registry.registerCheckpoint(header, "", attestations);
        require(registry.checkpointAtBatch(1) == digest, "receipt-root continuity rejected");
    }

    function testGenesisCheckpointIdentityBindsDistinctRootsManifestAndConfig() public {
        CheckpointRegistry other = new CheckpointRegistry(
            bond,
            Constants.PROTOCOL_VERSION,
            42,
            declaredThreshold,
            4,
            declaredDelaySeconds,
            5 minutes,
            keccak256("other-manifest"),
            GENESIS_CANONICAL_STATE_ROOT,
            GENESIS_RECEIPT_ROOT,
            keccak256("other-config"),
            RELEASE
        );
        require(other.genesisCheckpointId() != registry.genesisCheckpointId(), "genesis identity not bound");

        vm.expectRevert(CheckpointRegistry.InvalidConfiguration.selector);
        new CheckpointRegistry(
            bond,
            Constants.PROTOCOL_VERSION,
            42,
            declaredThreshold,
            4,
            declaredDelaySeconds,
            5 minutes,
            GENESIS_RECEIPT_ROOT,
            GENESIS_RECEIPT_ROOT,
            GENESIS_RECEIPT_ROOT,
            CONFIG,
            RELEASE
        );
    }

    function testCanonicalMillisecondTimestampsMeetSecondBasedWallClockBounds() public {
        vm.warp(1_700_000_000);
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        header.timestamp = 1_700_000_000_123;
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        attestations[0].attestedAt = header.timestamp + (declaredDelaySeconds * 1_000);
        (attestations[0].v, attestations[0].r, attestations[0].s) =
            vm.sign(keys[0], CanonicalCheckpoint.attestationHash(attestations[0]));
        registry.registerCheckpoint(header, "", attestations);
        require(registry.checkpointTimestamp(digest) == 1_700_000_000_123, "milliseconds were truncated");
    }

    function testRejectsMillisecondTimestampBeyondSecondConfiguredDrift() public {
        vm.warp(1_700_000_000);
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        header.timestamp = 1_700_000_000_000 + ((5 minutes + 1) * 1_000);
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        vm.expectRevert(CheckpointRegistry.InvalidHeader.selector);
        registry.registerCheckpoint(header, "", attestations);
    }

    function testRejectsMillisecondAttestationBeyondSecondConfiguredDelay() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        attestations[0].attestedAt = header.timestamp + ((declaredDelaySeconds + 1) * 1_000);
        (attestations[0].v, attestations[0].r, attestations[0].s) =
            vm.sign(keys[0], CanonicalCheckpoint.attestationHash(attestations[0]));
        vm.expectRevert(CheckpointRegistry.InvalidCertificate.selector);
        registry.registerCheckpoint(header, "", attestations);
    }

    function testProtocolThreeRejectsLegacyHeaderAndBindsCertificateVersion() public {
        CheckpointRegistry legacyRegistry = registry;
        bond = new GuarantorBond(
            address(this),
            address(this),
            Constants.USDL_TOKEN,
            address(this),
            Constants.USDL_ASSET_ID,
            3,
            42,
            100,
            7 days,
            CONFIG,
            RELEASE
        );
        CheckpointToken token = CheckpointToken(Constants.USDL_TOKEN);
        for (uint256 i = 0; i < keys.length; ++i) {
            address signer = vm.addr(keys[i]);
            bond.activateGuarantor(bytes32(i + 1), signer, signer, 1, uint64(i + 1));
            token.mint(signer, 2 ether);
            vm.prank(signer);
            token.approve(address(bond), 2 ether);
            vm.prank(signer);
            bond.depositBond(bytes32(i + 1), 2 ether);
        }
        registry = new CheckpointRegistry(
            bond,
            3,
            42,
            declaredThreshold,
            4,
            declaredDelaySeconds,
            5 minutes,
            GENESIS_MANIFEST_DIGEST,
            GENESIS_CANONICAL_STATE_ROOT,
            GENESIS_RECEIPT_ROOT,
            CONFIG,
            RELEASE
        );
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes32 legacyDigest = legacyRegistry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory empty = new CanonicalCheckpoint.GuarantorAttestation[](0);
        vm.expectRevert(CheckpointRegistry.InvalidHeader.selector);
        registry.registerCheckpoint(header, "", empty);
        header.protocolVersion = 3;
        vm.expectRevert(CheckpointRegistry.InvalidHeader.selector);
        legacyRegistry.registerCheckpoint(header, "", empty);
        bytes32 digest = registry.checkpointHash(header, "");
        require(digest != legacyDigest, "header versions alias");
        bytes memory encoded = registry.canonicalHeader(header);
        require(encoded.length == 354 && uint8(encoded[0]) == 0 && uint8(encoded[1]) == 3, "versioned header prefix");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        bytes32 selectedAttestation = CanonicalCheckpoint.attestationHash(attestations[0]);
        attestations[0].protocolVersion = 2;
        require(
            CanonicalCheckpoint.attestationHash(attestations[0]) != selectedAttestation, "attestation versions alias"
        );
        vm.expectRevert(CheckpointRegistry.InvalidCertificate.selector);
        registry.registerCheckpoint(header, "", attestations);
        attestations[0].protocolVersion = 3;
        registry.registerCheckpoint(header, "", attestations);
        require(registry.latestFinalisedStateRoot() == header.resultingStateRoot, "version3 root not advanced");
        require(registry.protocolVersion() == 3 && bond.protocolVersion() == 3, "immutable version mismatch");
    }

    function testRegistersThresholdCertificate() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        registry.registerCheckpoint(header, "", attestations);
        require(registry.latestFinalisedStateRoot() == header.resultingStateRoot, "root not advanced");
        require(registry.checkpointAtBatch(1) == digest, "batch checkpoint absent");
        require(registry.checkpointGuarantorSetVersion(digest) == 6, "guarantor-set version absent");
        require(
            registry.certificateCommitment(digest)
                == sha256(
                    abi.encode(
                        keccak256("LXP2/registered-guarantor-certificate/v2"),
                        digest,
                        header.epoch,
                        uint64(6),
                        attestations
                    )
                ),
            "certificate not bound to guarantor-set version"
        );
    }

    function testRejectsSignatureOutsideGovernedMembership() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        attestations[1].guarantorId = bytes32(uint256(4));
        attestations[1].signer = vm.addr(4);
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(4, CanonicalCheckpoint.attestationHash(attestations[1]));
        attestations[1].v = v;
        attestations[1].r = r;
        attestations[1].s = s;
        vm.expectRevert(CheckpointRegistry.InvalidCertificate.selector);
        registry.registerCheckpoint(header, "", attestations);
    }

    function testRejectsForeignNetworkAndPaxeerDomainAttestation() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        attestations[1].networkId = 43;
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(2, CanonicalCheckpoint.attestationHash(attestations[1]));
        attestations[1].v = v;
        attestations[1].r = r;
        attestations[1].s = s;
        vm.expectRevert(CheckpointRegistry.InvalidCertificate.selector);
        registry.registerCheckpoint(header, "", attestations);

        attestations = _attestations(header, digest, 2);
        attestations[1].paxeerChainId += 1;
        (v, r, s) = vm.sign(2, CanonicalCheckpoint.attestationHash(attestations[1]));
        attestations[1].v = v;
        attestations[1].r = r;
        attestations[1].s = s;
        vm.expectRevert(CheckpointRegistry.InvalidCertificate.selector);
        registry.registerCheckpoint(header, "", attestations);

        attestations = _attestations(header, digest, 2);
        attestations[1].settlementContract = address(0xF0E1);
        (v, r, s) = vm.sign(2, CanonicalCheckpoint.attestationHash(attestations[1]));
        attestations[1].v = v;
        attestations[1].r = r;
        attestations[1].s = s;
        vm.expectRevert(CheckpointRegistry.InvalidCertificate.selector);
        registry.registerCheckpoint(header, "", attestations);
    }

    function testRejectsDuplicateGuarantor() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        attestations[1] = attestations[0];
        vm.expectRevert(CheckpointRegistry.InvalidCertificate.selector);
        registry.registerCheckpoint(header, "", attestations);
    }

    function testRejectsStateRootDiscontinuity() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        header.previousStateRoot = keccak256("unrecorded-state-root");
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        vm.expectRevert(CheckpointRegistry.StateRootDiscontinuity.selector);
        registry.registerCheckpoint(header, "", attestations);
    }

    function testRegisteredCertificateRequiresExactRecordedSet() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        registry.registerCheckpoint(header, "", attestations);

        require(
            registry.verifyRegisteredCertificate(
                digest,
                header.resultingStateRoot,
                header.epoch,
                header.batchNumber,
                header.dataAvailabilityRoot,
                attestations
            ),
            "recorded certificate rejected"
        );
        require(
            !registry.verifyRegisteredCertificate(
                digest,
                header.resultingStateRoot,
                header.epoch + 1,
                header.batchNumber,
                header.dataAvailabilityRoot,
                attestations
            ),
            "caller-selected membership epoch accepted"
        );

        attestations[0].attestedAt += 1;
        require(
            !registry.verifyRegisteredCertificate(
                digest,
                header.resultingStateRoot,
                header.epoch,
                header.batchNumber,
                header.dataAvailabilityRoot,
                attestations
            ),
            "mutated certificate accepted"
        );
    }

    function testRecordedCertificateSurvivesLaterMembershipAndCustodyMutations() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        registry.registerCheckpoint(header, "", attestations);
        _requireRecordedCertificate(header, digest, attestations);

        bytes32 guarantorId = bytes32(uint256(1));
        address originalSigner = vm.addr(1);
        bond.setUnresolvedSlashing(guarantorId, true);
        _requireRecordedCertificate(header, digest, attestations);
        bond.setUnresolvedSlashing(guarantorId, false);
        bond.setGuarantorJailStatus(guarantorId, true, 4);
        _requireRecordedCertificate(header, digest, attestations);
        bond.setGuarantorJailStatus(guarantorId, false, 5);
        bond.rotateGuarantorSigner(guarantorId, vm.addr(4), 2, 6);
        _requireRecordedCertificate(header, digest, attestations);
        bond.removeGuarantor(guarantorId, 3, 7);
        _requireRecordedCertificate(header, digest, attestations);

        vm.prank(originalSigner);
        bond.beginUnbond(guarantorId, 2 ether);
        _requireRecordedCertificate(header, digest, attestations);
        vm.warp(block.timestamp + 7 days);
        vm.prank(originalSigner);
        bond.finalizeUnbond(guarantorId);
        _requireRecordedCertificate(header, digest, attestations);
        bond.syncCustodiedValue(1_000 ether);
        _requireRecordedCertificate(header, digest, attestations);

        _slashRotatedOldSigner(attestations[1]);
        _requireRecordedCertificate(header, digest, attestations);
        require(registry.isRecordedCertificate(digest, attestations), "recorded path used mutable eligibility");
    }

    function testRecordedCertificateSurvivesLaterVersionedSlash() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        registry.registerCheckpoint(header, "", attestations);
        bond.setSlashingAuthority(address(this));
        bond.slashForCheckpoint(bytes32(uint256(1)), digest);
        require(bond.membershipVersion() == 7, "slash did not advance guarantor-set version");
        require(bond.bondRecord(bytes32(uint256(1))).removedEpoch == 0, "slash invented removal epoch");
        require(bond.bondRecord(bytes32(uint256(1))).ejectedAtVersion == 7, "slash boundary not recorded");
        _requireRecordedCertificate(header, digest, attestations);
        require(registry.checkpointGuarantorSetVersion(digest) == 6, "recorded set version mutated");
    }

    function testOnlySlashingAuthorityCanInvalidateWithoutErasingHistory() public {
        CanonicalCheckpoint.HeaderCommitments memory header = _header();
        bytes32 digest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations = _attestations(header, digest, 2);
        registry.registerCheckpoint(header, "", attestations);

        vm.expectRevert(CheckpointRegistry.ChallengeAuthorityOnly.selector);
        vm.prank(address(0xBEEF));
        registry.invalidateCheckpoint(digest);

        bond.setSlashingAuthority(address(this));
        registry.invalidateCheckpoint(digest);
        require(registry.explicitlyInvalidated(digest), "invalidation not recorded");
        require(!registry.isCanonicalCheckpoint(digest), "invalid checkpoint remained canonical");
        require(registry.finalisedStateRoot(digest) == header.resultingStateRoot, "state-root history erased");
        require(registry.checkpointAtBatch(header.batchNumber) == digest, "batch history erased");
        require(registry.latestCanonicalCheckpointHash() == bytes32(0), "nonexistent predecessor invented");

        header.epoch = 2;
        header.batchNumber = 2;
        header.firstSequence = header.lastSequence + 1;
        header.lastSequence = header.firstSequence;
        header.previousStateRoot = header.resultingStateRoot;
        header.resultingStateRoot = bytes32(uint256(0x99) << 248);
        header.timestamp += 1;
        bytes32 descendantDigest = registry.checkpointHash(header, "");
        CanonicalCheckpoint.GuarantorAttestation[] memory descendantAttestations =
            _attestations(header, descendantDigest, 2);
        vm.expectRevert(CheckpointRegistry.CanonicalChainInvalidated.selector);
        registry.registerCheckpoint(header, "", descendantAttestations);
    }

    function testPayloadSizeIndependentOfActivityCount() public view {
        CanonicalCheckpoint.HeaderCommitments memory first = _header();
        CanonicalCheckpoint.HeaderCommitments memory second = first;
        second.lastSequence = 10_000_000;
        require(
            registry.canonicalHeader(first).length == registry.canonicalHeader(second).length, "count leaked into size"
        );
    }

    function _header() private pure returns (CanonicalCheckpoint.HeaderCommitments memory header) {
        header = CanonicalCheckpoint.HeaderCommitments({
            protocolVersion: 2,
            networkId: 42,
            epoch: 1,
            batchNumber: 1,
            firstSequence: 1,
            lastSequence: 1_000_000,
            previousStateRoot: GENESIS_RECEIPT_ROOT,
            resultingStateRoot: bytes32(uint256(0x22) << 248),
            activityMerkleRoot: bytes32(uint256(0x33) << 248),
            receiptMerkleRoot: bytes32(uint256(0x44) << 248),
            eventMerkleRoot: bytes32(uint256(0x55) << 248),
            dataAvailabilityRoot: bytes32(uint256(0x66) << 248),
            oracleRoot: bytes32(uint256(0x77) << 248),
            timestamp: 1_000_000,
            sequencerId: bytes32(uint256(0x88) << 248)
        });
    }

    function _vectorHeader() private pure returns (CanonicalCheckpoint.HeaderCommitments memory header) {
        header = _header();
        header.timestamp = 1000;
    }

    function _requireRecordedCertificate(
        CanonicalCheckpoint.HeaderCommitments memory header,
        bytes32 digest,
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations
    ) private view {
        require(
            registry.verifyRegisteredCertificate(
                digest,
                header.resultingStateRoot,
                header.epoch,
                header.batchNumber,
                header.dataAvailabilityRoot,
                attestations
            ),
            "later state invalidated recorded certificate"
        );
    }

    function _slashRotatedOldSigner(CanonicalCheckpoint.GuarantorAttestation memory first) private {
        bytes32 guarantorId = bytes32(uint256(2));
        address rotatedSigner = vm.addr(5);
        bond.rotateGuarantorSigner(guarantorId, rotatedSigner, 4, 8);
        CanonicalCheckpoint.GuarantorAttestation memory second =
            abi.decode(abi.encode(first), (CanonicalCheckpoint.GuarantorAttestation));
        second.checkpointHash = keccak256("conflicting-old-signer-checkpoint");
        second.checkpointId = second.checkpointHash;
        (uint8 v, bytes32 r, bytes32 s) = vm.sign(2, CanonicalCheckpoint.attestationHash(second));
        second.v = v;
        second.r = r;
        second.s = s;
        bond.submitEquivocation(first, second);
        require(bond.bondRecord(guarantorId).ejectedAtVersion == 16, "old signer did not eject member");
        require(!bond.bondedActive(guarantorId, rotatedSigner, 4), "rotated member survived old-key slash");
    }

    function _attestations(CanonicalCheckpoint.HeaderCommitments memory header, bytes32 digest, uint256 count)
        private
        returns (CanonicalCheckpoint.GuarantorAttestation[] memory attestations)
    {
        attestations = new CanonicalCheckpoint.GuarantorAttestation[](count);
        for (uint256 i = 0; i < count; ++i) {
            attestations[i] = CanonicalCheckpoint.GuarantorAttestation({
                protocolVersion: header.protocolVersion,
                networkId: header.networkId,
                paxeerChainId: uint64(block.chainid),
                settlementContract: address(bond),
                epoch: header.epoch,
                checkpointId: digest,
                checkpointHash: digest,
                guarantorId: bytes32(i + 1),
                batchNumber: header.batchNumber,
                dataAvailabilityRoot: header.dataAvailabilityRoot,
                replayed: true,
                dataAvailable: true,
                availabilityClassMask: 0x1f,
                attestedAt: header.timestamp + 1,
                signer: vm.addr(keys[i]),
                r: bytes32(0),
                s: bytes32(0),
                v: 0
            });
            bytes32 attestationDigest = CanonicalCheckpoint.attestationHash(attestations[i]);
            (uint8 v, bytes32 r, bytes32 s) = vm.sign(keys[i], attestationDigest);
            attestations[i].v = v;
            attestations[i].r = r;
            attestations[i].s = s;
        }
    }
}
