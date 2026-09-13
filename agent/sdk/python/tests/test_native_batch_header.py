from __future__ import annotations

import importlib.util
import json
import unittest
from dataclasses import replace
from hashlib import sha256
from pathlib import Path

from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.asymmetric import ec, utils
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat

from layerx_sdk import PlatformSdkError
from layerx_sdk.verifier import (
    CheckpointAttestation,
    CheckpointCertificate,
    CheckpointVerificationInput,
    GuarantorKey,
    MerkleProof,
    SequencerAuthorization,
    _attestation_message,
    decode_batch_header,
    verify_batch_inclusion,
    verify_checkpoint,
)

ROOT = Path(__file__).resolve().parents[4]
SIGNATURES_PATH = ROOT / "platform/integrations/fastapi/layerx_fastapi/signatures.py"
SPEC = importlib.util.spec_from_file_location("native_header_signatures", SIGNATURES_PATH)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("real signature verifier is unavailable")
SIGNATURES = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SIGNATURES)


def inclusion_proof(leaves: tuple[bytes, ...], index: int) -> MerkleProof:
    level = [sha256(b"LXP/v1/merkle-leaf\0" + leaf).digest() for leaf in leaves]
    siblings = []
    position = index
    while len(level) > 1:
        siblings.append(level[position ^ 1] if position ^ 1 < len(level) else level[position])
        level = [
            sha256(b"LXP/v1/merkle-internal\0" + level[i] + level[min(i + 1, len(level) - 1)]).digest()
            for i in range(0, len(level), 2)
        ]
        position //= 2
    return MerkleProof(index, len(leaves), tuple(siblings))


def native_batches():
    for directory, names in [
        ("custody/daemon-credit-receipt", ("credit.receipt", "maintenance.receipt")),
        ("programs/maintained-multicall", ("receipt-0", "receipt-1", "maintenance.receipt")),
    ]:
        root = ROOT / "tests/fixtures" / directory
        canonical = (root / "header").read_bytes()
        header = decode_batch_header(canonical)
        yield (
            canonical,
            (root / "header.signature").read_bytes(),
            SequencerAuthorization(header.sequencer_id, (root / "sequencer.public").read_bytes(),
                                   header.batch_number, header.batch_number),
            tuple((root / name).read_bytes() for name in names),
        )


class NativeBatchHeaderTest(unittest.TestCase):
    def setUp(self):
        self.signatures = SIGNATURES.LayerXSignatureVerifier()

    def test_native_first_batch_and_multiple_call_receipts_verify_unchanged(self):
        for canonical, signature, authority, leaves in native_batches():
            header = decode_batch_header(canonical)
            self.assertEqual(len(canonical), 354)
            self.assertEqual(header.protocol_version, 3)
            self.assertEqual(header.last_sequence - header.first_sequence + 1, len(leaves))
            for index, leaf in enumerate(leaves):
                verified = verify_batch_inclusion(
                    "receipt", leaf, inclusion_proof(leaves, index), canonical,
                    signature, authority, self.signatures, protocol_version=3)
                self.assertEqual(verified.header, header)
                self.assertEqual(verified.root, header.receipt_merkle_root)
                self.assertEqual(verified.level, "batch-included")
                self.assertEqual(verified.header_digest, sha256(b"LXP/v1/batch-header\0" + canonical).digest())

    def test_native_header_inclusion_refuses_domain_authority_and_evidence_changes(self):
        canonical, signature, authority, leaves = next(native_batches())
        proof = inclusion_proof(leaves, 0)
        for protocol in (1, 2, 4):
            with self.subTest(protocol=protocol), self.assertRaises(PlatformSdkError):
                verify_batch_inclusion("receipt", leaves[0], proof, canonical, signature,
                                       authority, self.signatures, protocol_version=protocol)
        for changed in (
            replace(authority, public_key=bytes(32)),
            replace(authority, sequencer_id=bytes(32)),
            replace(authority, first_batch_number=2),
            replace(authority, last_batch_number=0),
        ):
            with self.assertRaises(PlatformSdkError):
                verify_batch_inclusion("receipt", leaves[0], proof, canonical, signature,
                                       changed, self.signatures, protocol_version=3)
        for index in (8, 13, 22, 31, 40, 50, 87, 124, 161, 198, 235, 272, 309, 322, 353):
            changed = bytearray(canonical)
            changed[index] ^= 1
            with self.subTest(header_byte=index), self.assertRaises(PlatformSdkError):
                verify_batch_inclusion("receipt", leaves[0], proof, bytes(changed), signature,
                                       authority, self.signatures, protocol_version=3)
        for leaf, path, signed in (
            (leaves[0][:-1] + bytes([leaves[0][-1] ^ 1]), proof, signature),
            (leaves[0], inclusion_proof(leaves, 1), signature),
            (leaves[0], proof, bytes(64)),
            (leaves[0], MerkleProof(0, 2, ()), signature),
        ):
            with self.assertRaises(PlatformSdkError):
                verify_batch_inclusion("receipt", leaf, path, canonical, signed,
                                       authority, self.signatures, protocol_version=3)

    def test_header_codec_requires_matching_supported_versions_and_exact_fields(self):
        canonical, _, _, _ = next(native_batches())
        for size in range(len(canonical)):
            with self.subTest(size=size), self.assertRaises(PlatformSdkError):
                decode_batch_header(canonical[:size])
        with self.assertRaises(PlatformSdkError):
            decode_batch_header(canonical + b"\0")
        for outer in (0, 1, 2, 3, 4, 65535):
            for inner in (0, 1, 2, 3, 4, 65535):
                changed = bytearray(canonical)
                changed[:2] = outer.to_bytes(2, "big")
                changed[6:8] = inner.to_bytes(2, "big")
                with self.subTest(outer=outer, inner=inner):
                    if outer == inner and outer in (1, 2, 3):
                        self.assertEqual(decode_batch_header(bytes(changed)).protocol_version, outer)
                    else:
                        with self.assertRaises(PlatformSdkError):
                            decode_batch_header(bytes(changed))
        for index in (2, 3, 4, 5, 8, 13, 22, 31, 40, 49, 50, 86, 123, 160, 197, 234, 271, 308, 317):
            changed = bytearray(canonical)
            changed[index] ^= 1
            with self.subTest(field_byte=index), self.assertRaises(PlatformSdkError):
                decode_batch_header(bytes(changed))

    def test_published_protocol_two_checkpoint_vector_keeps_its_digest(self):
        vector = json.loads((ROOT / "tests/vectors/checkpoint/fresh.json").read_text())
        canonical = bytes.fromhex(vector["header"]["bytes"].removeprefix("0x"))
        header = decode_batch_header(canonical)
        self.assertEqual(header.protocol_version, 2)
        checkpoint = bytes.fromhex(vector["expected_digest"].removeprefix("0x"))
        attestations, bonded = [], []
        for index, item in enumerate(vector["attestations"], 1):
            raw = lambda name: bytes.fromhex(item[name].removeprefix("0x"))
            message = raw("message")
            attestation = CheckpointAttestation(
                header.protocol_version, header.network_id, int.from_bytes(message[6:14], "big"),
                message[14:34], header.epoch, checkpoint, checkpoint, raw("guarantor_id"),
                header.batch_number, header.data_availability_root, item["replayed"],
                item["data_possessed"], item["availability_class_mask"], item["attested_at_ms"],
                raw("signer"), raw("signature"), item["signature_v"])
            self.assertEqual(_attestation_message(attestation), message)
            public = ec.derive_private_key(index, ec.SECP256K1()).public_key().public_bytes(
                Encoding.X962, PublicFormat.CompressedPoint)
            attestations.append(attestation)
            bonded.append(GuarantorKey(attestation.guarantor_id, public, True))
        verification = CheckpointVerificationInput(
            CheckpointCertificate(canonical, bytes.fromhex(vector["certificate"]["validity_proof"].removeprefix("0x")),
                                  tuple(attestations), vector["certificate"]["threshold"]),
            tuple(bonded), checkpoint, attestations[0].paxeer_chain_id,
            attestations[0].settlement_contract, None, True)
        verified = verify_checkpoint(verification, self.signatures)
        self.assertEqual(verified.checkpoint_id, checkpoint)
        self.assertEqual((verified.achieved, verified.required), (3, 2))
        with self.assertRaises(PlatformSdkError):
            verify_checkpoint(verification, self.signatures, protocol_version=3)

    def test_native_protocol_three_checkpoint_requires_actual_matching_attestation(self):
        canonical, _, _, _ = next(native_batches())
        header = decode_batch_header(canonical)
        checkpoint = sha256(b"LXP/v2/checkpoint-certificate\0" + canonical + bytes(4)).digest()
        key = ec.generate_private_key(ec.SECP256K1())
        public = key.public_key().public_bytes(Encoding.X962, PublicFormat.CompressedPoint)
        uncompressed = key.public_key().public_bytes(Encoding.X962, PublicFormat.UncompressedPoint)
        signer = SIGNATURES._keccak256(uncompressed[1:])[-20:]
        attestation = CheckpointAttestation(
            3, header.network_id, 125, bytes.fromhex("11" * 20), header.epoch,
            checkpoint, checkpoint, sha256(public).digest(), header.batch_number,
            header.data_availability_root, True, True, 31, header.timestamp_ms,
            signer, b"", 27)
        digest = sha256(b"LXP/v2/guarantor-attestation\0" + _attestation_message(attestation)).digest()
        r, s = utils.decode_dss_signature(key.sign(digest, ec.ECDSA(utils.Prehashed(hashes.SHA256()))))
        order = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEBAAEDCE6AF48A03BBFD25E8CD0364141
        signature = r.to_bytes(32, "big") + min(s, order - s).to_bytes(32, "big")
        recoveries = [v for v in (27, 28) if self.signatures.verify_recoverable_secp256k1(
            public, signature, v, signer, digest)]
        self.assertEqual(len(recoveries), 1)
        attestation = replace(attestation, signature=signature, signature_v=recoveries[0])
        certificate = CheckpointCertificate(canonical, b"", (attestation,), 1)
        verification = CheckpointVerificationInput(
            certificate, (GuarantorKey(attestation.guarantor_id, public, True),), checkpoint,
            125, attestation.settlement_contract, None, True)
        verified = verify_checkpoint(verification, self.signatures, protocol_version=3)
        self.assertEqual(verified.header, header)
        self.assertEqual(verified.level, "checkpoint-finalised")
        for changed in (
            replace(verification, availability_obtained=False),
            replace(verification, expected_paxeer_chain_id=126),
            replace(verification, registered_checkpoint_id=bytes(32)),
            replace(verification, certificate=replace(certificate, threshold=2)),
            replace(verification, certificate=replace(certificate, attestations=(replace(attestation, protocol_version=2),))),
            replace(verification, certificate=replace(certificate, attestations=(replace(attestation, network_id=header.network_id + 1),))),
            replace(verification, certificate=replace(certificate, attestations=(replace(attestation, signature=bytes(64)),))),
        ):
            with self.assertRaises(PlatformSdkError):
                verify_checkpoint(changed, self.signatures, protocol_version=3)


if __name__ == "__main__":
    unittest.main()
