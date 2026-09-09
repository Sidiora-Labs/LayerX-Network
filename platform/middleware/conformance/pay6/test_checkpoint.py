import copy
import json
import unittest
from dataclasses import replace
from pathlib import Path

from test_commitment import CommitmentTests
from layerx_sdk.x402_checkpoint import RpcCheckpointAuthority, rpc_checkpoint_evidence
from layerx_sdk.x402 import PaymentCommitmentEvidence
from layerx_sdk.verifier import GuarantorKey, MerkleProof, SequencerAuthorization


class CheckpointTests(CommitmentTests):
    def setUp(self):
        super().setUp()
        self.fixture = json.loads(
            Path(__file__).with_name("checkpoint.json").read_text()
        )
        batch = json.loads(Path(__file__).with_name("batch.json").read_text())
        op = self.fixture["operator"]
        self.batch = PaymentCommitmentEvidence(
            7,
            bytes.fromhex(batch["header"]),
            bytes.fromhex(batch["signature"]),
            SequencerAuthorization(
                bytes.fromhex(batch["sequencer_id"]),
                self.authorized.sequencer_public_key,
                1,
                1,
            ),
            MerkleProof(0, 1, ()),
        )
        self.authority = RpcCheckpointAuthority(
            bytes.fromhex(op["context"]),
            (
                GuarantorKey(
                    bytes.fromhex(op["guarantor_id"]),
                    bytes.fromhex(op["public_key"]),
                    True,
                ),
            ),
            bytes.fromhex(self.fixture["checkpoint_evidence"]["checkpoint_id"]),
            op["chain_id"],
            bytes.fromhex(op["contract"]),
            bytes.fromhex(op["reference"]),
            True,
            op["required_guarantors"],
        )

    def test_finalised(self):
        evidence = rpc_checkpoint_evidence(self.fixture, self.batch, self.authority)
        self.verify(commitment="finalised", evidence=evidence)
        for change in (
            {"required_guarantors": 0},
            {"required_guarantors": 2},
            {"canonical_context": b"bad"},
            {"bonded_set": ()},
            {"expected_paxeer_chain_id": 778},
            {"availability_obtained": False},
        ):
            with self.assertRaises(Exception):
                evidence = rpc_checkpoint_evidence(
                    self.fixture, self.batch, replace(self.authority, **change)
                )
                self.verify(commitment="finalised", evidence=evidence)

    def test_wire_refusals(self):
        for field in ("checkpoint", "canonical_header", "context", "checkpoint_id"):
            for value in ("00", self.fixture["checkpoint_evidence"][field] + "00"):
                fixture = copy.deepcopy(self.fixture)
                fixture["checkpoint_evidence"][field] = value
                with self.assertRaises(Exception):
                    rpc_checkpoint_evidence(fixture, self.batch, self.authority)
        fixture = copy.deepcopy(self.fixture)
        raw = bytearray.fromhex(fixture["checkpoint_evidence"]["checkpoint"])
        raw[-114] ^= 1
        fixture["checkpoint_evidence"]["checkpoint"] = raw.hex()
        with self.assertRaises(Exception):
            evidence = rpc_checkpoint_evidence(fixture, self.batch, self.authority)
            self.verify(commitment="finalised", evidence=evidence)


if __name__ == "__main__":
    unittest.main()
