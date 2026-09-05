import argparse
import json
import subprocess
import sys
import tempfile
from hashlib import sha256
from pathlib import Path

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey


ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / "agent/sdk/python"))

from layerx_sdk.program_wire import decode_and_verify_program_terminal
from layerx_sdk.verifier import AuthorizedReceiptBatch, verify_receipt_outcome


class Ed25519Signatures:
    def verify_ed25519(self, public_key, signature, digest):
        try:
            Ed25519PublicKey.from_public_bytes(public_key).verify(signature, digest)
            return True
        except (InvalidSignature, ValueError):
            return False


def canonical_hex(value, length=None):
    if not isinstance(value, str) or not value or len(value) > 2_097_152:
        raise ValueError("invalid native evidence length")
    decoded = bytes.fromhex(value)
    if decoded.hex() != value or (length is not None and len(decoded) != length):
        raise ValueError("noncanonical native evidence")
    return decoded


def fixture(raw):
    expected_fields = {
        "canonical_receipt_hex", "signed_activity_hex", "program_id_hex",
        "receipt_digest_hex", "terminal_payload_hex", "call_graph_hex",
        "batch_number", "network_id", "authorized_batch",
    }
    if set(raw) != expected_fields:
        raise ValueError("unexpected native execution fixture fields")
    batch = raw["authorized_batch"]
    batch_fields = (
        "batch_id_hex", "asset_hex", "previous_state_root_hex",
        "resulting_state_root_hex", "sequencer_public_key_hex",
    )
    if set(batch) != set(batch_fields):
        raise ValueError("unexpected batch authority fields")
    authority = AuthorizedReceiptBatch(*(canonical_hex(batch[key], 32) for key in batch_fields))
    canonical = canonical_hex(raw["canonical_receipt_hex"])
    verified = verify_receipt_outcome(canonical, authority, Ed25519Signatures(), protocol_version=3)
    receipt = verified.receipt
    outcome = receipt.program_outcome
    if (receipt.protocol_version, receipt.module_id, receipt.module_version, receipt.operation,
            receipt.result_code) != (3, 9, 4, 3, 0) or outcome is None or outcome.terminal_kind != 1 or outcome.abi_version != 2:
        raise ValueError("native producer did not execute a successful protocol-3 ABI2 CALL")
    if canonical_hex(raw["receipt_digest_hex"], 32) != verified.receipt_digest:
        raise ValueError("native receipt digest mismatch")
    activity = canonical_hex(raw["signed_activity_hex"])
    if sha256(b"LXP/v1/activity-id\0" + activity).digest() != receipt.activity_id:
        raise ValueError("native signed activity binding mismatch")
    terminal = canonical_hex(raw["terminal_payload_hex"])
    graph = canonical_hex(raw["call_graph_hex"])
    program_id = canonical_hex(raw["program_id_hex"], 32).hex()
    if (sha256(terminal).digest() != outcome.terminal_payload_root or
            sha256(graph).digest() != outcome.call_graph_root):
        raise ValueError("native artifact digest mismatch")
    decoded = decode_and_verify_program_terminal(terminal, graph, program_id, outcome, 3)
    execution = {
        "state": "executed",
        "activity_id": receipt.activity_id.hex(),
        "program_id": program_id,
        "guest_abi_version": outcome.abi_version,
        "module_version": receipt.module_version,
        "batch_id": receipt.batch_id.hex(),
        "global_sequence": str(receipt.global_sequence),
        "result_code": receipt.result_code,
        "state_root": receipt.resulting_state_root.hex(),
        "receipt": canonical.hex(),
        "receipt_digest": verified.receipt_digest.hex(),
        "terminal_payload": terminal.hex(),
        "call_graph": graph.hex(),
        "authority": {key.removesuffix("_hex"): batch[key] for key in batch_fields},
        "usage": dict(decoded.usage),
        "outcome": dict(decoded.outcome),
        "verification": "receipt-terminal-and-call-graph-verified",
    }
    expected = {
        "level": verified.level,
        "protocol_version": receipt.protocol_version,
        "module_id": receipt.module_id,
        "module_version": receipt.module_version,
        "operation": receipt.operation,
        "result_code": receipt.result_code,
        "global_sequence": receipt.global_sequence,
        "timestamp_ms": receipt.timestamp,
        "receipt_digest_hex": verified.receipt_digest.hex(),
        "activity_root_hex": receipt.activity_root.hex(),
        "transfer_set_root_hex": receipt.transfer_set_root.hex(),
    }
    for key in ("encoding_version", "runtime_version", "abi_version", "metering_schedule_version"):
        expected["program_outcome_" + key] = getattr(outcome, key)
    for key in ("occupancy_byte_batches", "occupancy_fee_units", "fee_units"):
        expected["program_outcome_" + key] = str(getattr(outcome, key))
    for key in ("occupancy_asset_id", "occupancy_evidence_digest", "occupancy_transfer_root",
                "call_graph_root", "terminal_payload_root"):
        expected["program_outcome_" + key + "_hex"] = getattr(outcome, key).hex()
    return {
        "name": "receipt-programs-executed-v3",
        "provenance": {
            "generator": "tests/programs/test_call_activity.c --dump-executed-v3",
            "packager": "platform/sdk/conformance/fixtures/generate_executed_program_fixture.py",
            "description": "Real C kernel admission, Rust Wasm runtime transfer execution, native ledger settlement and kernel receipt signing. Deterministic unit-test account bootstrap and Ed25519 authority; not production custody funding, external finality or checkpoint inclusion. The packager verifies and decodes original evidence; it does not construct outcomes, change receipt fields or sign receipts.",
        },
        **raw,
        "expected": expected,
        "execution_document": execution,
    }


def main():
    parser = argparse.ArgumentParser(description="Package real native execution evidence; requires cryptography.")
    parser.add_argument("--encoder", type=Path, required=True)
    parser.add_argument("--check", action="store_true")
    arguments = parser.parse_args()
    with tempfile.TemporaryFile() as output:
        subprocess.run([str(arguments.encoder.resolve()), "--dump-executed-v3"],
                       stdout=output, check=True, timeout=120)
        if output.tell() > 10_485_760:
            raise ValueError("native execution evidence exceeds fixture bound")
        output.seek(0)
        document = fixture(json.load(output))
    contents = json.dumps(document, indent=2) + "\n"
    destination = Path(__file__).with_name("receipt-programs-executed-v3.json")
    if arguments.check:
        if destination.read_text() != contents:
            raise ValueError(f"native execution fixture drift: {destination}")
    else:
        destination.write_text(contents)


if __name__ == "__main__":
    main()
