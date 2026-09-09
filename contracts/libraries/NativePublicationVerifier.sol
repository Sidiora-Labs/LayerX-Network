// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.24;
import {Ed25519Verifier} from "../crypto/Ed25519.sol";
import {NativeStateProof} from "./NativeStateProof.sol";
import {PaxeerWithdrawalCodec} from "./PaxeerWithdrawalCodec.sol";
import {CanonicalCheckpoint} from "./CanonicalCheckpoint.sol";

interface NativePublicationRegistry {
    function finalisedStateRoot(bytes32) external view returns (bytes32);
    function checkpointEpoch(bytes32) external view returns (uint64);
    function checkpointBatchNumber(bytes32) external view returns (uint64);
    function checkpointGuarantorSetVersion(bytes32) external view returns (uint64);
    function certificateCommitment(bytes32) external view returns (bytes32);
    function networkId() external view returns (uint32);
    function threshold() external view returns (uint16);
    function maximumAttestations() external view returns (uint16);
    function isRecordedAncestor(bytes32, bytes32) external view returns (bool);
}

contract NativePublicationVerifier {
    error InvalidWitnesses();
    bytes32 private constant REGISTERED_CERTIFICATE_DOMAIN = keccak256("LXP2/registered-guarantor-certificate/v2");
    Ed25519Verifier public immutable recipientVerifier = new Ed25519Verifier();

    function verify(bytes32 digest, bytes calldata vector, bool balances) external view {
        bytes memory domain =
            balances ? bytes("LXP/Paxeer/balance-witnesses/v2\x00") : bytes("LXP/Paxeer/withdrawal-witnesses/v2\x00");
        uint256 cursor = domain.length;
        if (vector.length < cursor + 4 || keccak256(vector[:cursor]) != keccak256(domain)) revert InvalidWitnesses();
        uint256 count = uint32(bytes4(vector[cursor:cursor + 4]));
        cursor += 4;
        if (count > 4096 || (balances && count == 0)) revert InvalidWitnesses();
        bytes32 previous;
        bytes32 previousAsset;
        address previousRecipient;
        for (uint256 i; i < count; ++i) {
            if (cursor + 4 > vector.length) revert InvalidWitnesses();
            uint256 size = uint32(bytes4(vector[cursor:cursor + 4]));
            cursor += 4;
            if (cursor + size > vector.length || size < (balances ? 100 : 64)) revert InvalidWitnesses();
            bytes calldata item = vector[cursor:cursor + size];
            cursor += size;
            bytes32 identity = bytes32(item[:32]);
            if (identity == bytes32(0)) revert InvalidWitnesses();
            if (balances) {
                bytes32 asset = bytes32(item[32:64]);
                address recipient = address(bytes20(item[80:100]));
                if (
                    i > 0
                        && (identity < previous
                            || (identity == previous
                                && (asset < previousAsset
                                    || (asset == previousAsset && recipient <= previousRecipient))))
                ) revert InvalidWitnesses();
                previousAsset = asset;
                previousRecipient = recipient;
            } else if (i > 0 && identity <= previous) {
                revert InvalidWitnesses();
            }
            previous = identity;
            _verifyNativeItem(digest, item, balances);
        }
        if (cursor != vector.length) revert InvalidWitnesses();
    }

    function _verifyNativeItem(bytes32 digest, bytes calldata item, bool balance) private view {
        NativePublicationRegistry registry = NativePublicationRegistry(msg.sender);
        bytes calldata wire = item[balance ? 100 : 64:];
        if (
            wire.length < 128 || bytes2(wire[:2]) != hex"0202" || bytes32(wire[2:34]) != digest
                || bytes32(wire[34:66]) != registry.finalisedStateRoot(digest)
                || uint64(bytes8(wire[66:74])) != registry.checkpointEpoch(digest)
                || uint64(bytes8(wire[74:82])) != registry.checkpointBatchNumber(digest)
                || uint64(bytes8(wire[114:122])) != 0 || uint16(bytes2(wire[122:124])) != 0
        ) revert InvalidWitnesses();
        uint256 count = uint32(bytes4(wire[124:128]));
        if (count < registry.threshold() || count > registry.maximumAttestations()) revert InvalidWitnesses();
        uint256 cursor = 128;
        CanonicalCheckpoint.GuarantorAttestation[] memory attestations =
            new CanonicalCheckpoint.GuarantorAttestation[](count);
        uint256[18] memory sizes = [uint256(2), 4, 8, 20, 8, 32, 32, 32, 8, 32, 1, 1, 1, 8, 20, 32, 32, 1];
        for (uint256 i; i < count; ++i) {
            bytes memory encoded = new bytes(18 * 32);
            for (uint256 field; field < 18; ++field) {
                uint256 size = sizes[field];
                if (cursor + size > wire.length) revert InvalidWitnesses();
                for (uint256 j; j < size; ++j) {
                    encoded[field * 32 + 32 - size + j] = wire[cursor + j];
                }
                cursor += size;
            }
            attestations[i] = abi.decode(encoded, (CanonicalCheckpoint.GuarantorAttestation));
        }
        if (
            registry.certificateCommitment(digest)
                    != sha256(
                        abi.encode(
                            REGISTERED_CERTIFICATE_DOMAIN,
                            digest,
                            registry.checkpointEpoch(digest),
                            registry.checkpointGuarantorSetVersion(digest),
                            attestations
                        )
                    ) || attestations[0].dataAvailabilityRoot != bytes32(wire[82:114]) || wire.length < cursor + 74
        ) revert InvalidWitnesses();
        bytes32 anchor = bytes32(wire[cursor:cursor + 32]);
        if (
            bytes32(wire[cursor + 32:cursor + 64]) != digest
                || uint32(bytes4(wire[cursor + 64:cursor + 68])) != registry.networkId()
                || !registry.isRecordedAncestor(anchor, digest)
        ) revert InvalidWitnesses();
        uint256 length = uint32(bytes4(wire[cursor + 68:cursor + 72]));
        cursor += 72;
        if (cursor + length + 2 > wire.length) revert InvalidWitnesses();
        bytes calldata witness = wire[cursor:cursor + length];
        cursor += length;
        uint256 signatureLength = uint16(bytes2(wire[cursor:cursor + 2]));
        cursor += 2;
        if (cursor + signatureLength != wire.length) revert InvalidWitnesses();
        if (balance) {
            if (signatureLength != 64) revert InvalidWitnesses();
            NativeStateProof.verifyBalance(
                recipientVerifier,
                witness,
                registry.finalisedStateRoot(digest),
                bytes32(item[:32]),
                bytes32(item[32:64]),
                uint128(bytes16(item[64:80])),
                registry.networkId(),
                address(bytes20(item[80:100])),
                anchor,
                wire[cursor:]
            );
        } else {
            if (signatureLength != 0 || witness.length < 237) revert InvalidWitnesses();
            bytes32 account = bytes32(witness[93:125]);
            bytes32 asset = bytes32(witness[125:157]);
            uint128 amount = uint128(bytes16(witness[157:173]));
            address recipient = address(bytes20(witness[185:205]));
            bytes32 id = bytes32(item[:32]);
            if (bytes32(item[32:64]) != PaxeerWithdrawalCodec.withdrawalLeaf(id, account, asset, amount, recipient)) {
                revert InvalidWitnesses();
            }
            NativeStateProof.verifyWithdrawal(
                witness,
                registry.finalisedStateRoot(digest),
                registry.networkId(),
                id,
                account,
                asset,
                amount,
                recipient,
                anchor,
                PaxeerWithdrawalCodec.nullifier(registry.networkId(), id, account, asset, amount, anchor)
            );
        }
    }
}
