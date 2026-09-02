#!/usr/bin/env python3
"""Regenerates the cross-language checkpoint vectors from the declared
checkpoint settlement configuration. Signatures are produced with
`cast wallet sign --no-hash` over the v2 guarantor-attestation digest."""

import hashlib
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
SETTLEMENT = ROOT / "contracts" / "config" / "checkpoint-settlement.json"
OUTPUT = ROOT / "tests" / "vectors" / "checkpoint"
PRIVATE_KEYS = [1, 2, 3]
STRUCTURE_TAG = 0x1701
HEADER_FIELDS = 15
VALIDITY_PROOF = b"PROOF"
ALL_AVAILABILITY_CLASSES = 0x1F

HEADER = {
    "epoch": 1,
    "batch_number": 1,
    "first_sequence": 1,
    "last_sequence": 1_000_000,
    "previous_state_root": bytes([0x11]) + bytes(31),
    "resulting_state_root": bytes([0x22]) + bytes(31),
    "activity_merkle_root": bytes([0x33]) + bytes(31),
    "receipt_merkle_root": bytes([0x44]) + bytes(31),
    "event_merkle_root": bytes([0x55]) + bytes(31),
    "data_availability_root": bytes([0x66]) + bytes(31),
    "oracle_root": bytes([0x77]) + bytes(31),
    "timestamp_ms": 1_000_000,
    "sequencer_id": bytes([0x88]) + bytes(31),
}


def hexstr(data):
    return "0x" + data.hex()


def unhex(text):
    return bytes.fromhex(text[2:] if text.startswith("0x") else text)


def cast(*args):
    return subprocess.run(["cast", *args], check=True, capture_output=True, text=True).stdout.strip()


def private_key_hex(value):
    return "0x" + value.to_bytes(32, "big").hex()


def compressed_public_key(value):
    raw = unhex(cast("wallet", "public-key", "--private-key", private_key_hex(value)))
    assert len(raw) == 64
    prefix = 0x02 if raw[63] % 2 == 0 else 0x03
    return bytes([prefix]) + raw[:32]


def encode_header(protocol_version, network_id):
    out = bytearray()
    out += protocol_version.to_bytes(2, "big")
    out += STRUCTURE_TAG.to_bytes(2, "big")
    out += bytes([HEADER_FIELDS])
    ordered = [
        (1, protocol_version.to_bytes(2, "big")),
        (2, network_id.to_bytes(4, "big")),
        (3, HEADER["epoch"].to_bytes(8, "big")),
        (4, HEADER["batch_number"].to_bytes(8, "big")),
        (5, HEADER["first_sequence"].to_bytes(8, "big")),
        (6, HEADER["last_sequence"].to_bytes(8, "big")),
        (7, (32).to_bytes(4, "big") + HEADER["previous_state_root"]),
        (8, (32).to_bytes(4, "big") + HEADER["resulting_state_root"]),
        (9, (32).to_bytes(4, "big") + HEADER["activity_merkle_root"]),
        (10, (32).to_bytes(4, "big") + HEADER["receipt_merkle_root"]),
        (11, (32).to_bytes(4, "big") + HEADER["event_merkle_root"]),
        (12, (32).to_bytes(4, "big") + HEADER["data_availability_root"]),
        (13, (32).to_bytes(4, "big") + HEADER["oracle_root"]),
        (14, HEADER["timestamp_ms"].to_bytes(8, "big")),
        (15, (32).to_bytes(4, "big") + HEADER["sequencer_id"]),
    ]
    for tag, value in ordered:
        out += bytes([tag]) + value
    return bytes(out)


def attestation_message(settlement, domain, protocol_version, checkpoint_id, guarantor_id, attested_at_ms):
    out = bytearray()
    out += protocol_version.to_bytes(2, "big")
    out += domain["network_id"].to_bytes(4, "big")
    out += domain["paxeer_chain_id"].to_bytes(8, "big")
    out += unhex(domain["settlement_contract"])
    out += HEADER["epoch"].to_bytes(8, "big")
    out += checkpoint_id
    out += checkpoint_id
    out += guarantor_id
    out += HEADER["batch_number"].to_bytes(8, "big")
    out += HEADER["data_availability_root"]
    out += bytes([1, 1, ALL_AVAILABILITY_CLASSES])
    out += attested_at_ms.to_bytes(8, "big")
    assert len(out) == 189
    return bytes(out)


def sign(value, digest):
    raw = unhex(cast("wallet", "sign", "--no-hash", "--private-key", private_key_hex(value), hexstr(digest)))
    assert len(raw) == 65 and raw[64] in (27, 28)
    return raw[:64], raw[64]


def main():
    settlement = json.loads(SETTLEMENT.read_text())
    assert settlement["schema"] == "layerx/checkpoint-settlement/1"
    protocol_version = settlement["protocol_version"]
    checkpoint_domain = settlement["checkpoint_certificate_domain"].encode() + b"\0"
    attestation_domain = settlement["guarantor_attestation_domain"].encode() + b"\0"
    prefix = unhex(settlement["header_encoding_prefix"])
    delay_ms = settlement["finality_policy"]["maximum_attestation_delay_seconds"] * 1000
    threshold = settlement["finality_policy"]["certificate_threshold"]
    domain = settlement["settlement_domains"]["vectors"]
    guarantors = domain["guarantor_set"]
    assert len(guarantors) == len(PRIVATE_KEYS)
    for value, guarantor in zip(PRIVATE_KEYS, guarantors):
        assert unhex(guarantor["signer"]) == unhex(cast("wallet", "address", "--private-key", private_key_hex(value)))
        assert unhex(guarantor["public_key"]) == compressed_public_key(value)

    header_bytes = encode_header(protocol_version, domain["network_id"])
    assert len(header_bytes) == settlement["header_length"]
    assert header_bytes.startswith(prefix)
    checkpoint_id = hashlib.sha256(
        checkpoint_domain + header_bytes + len(VALIDITY_PROOF).to_bytes(4, "big") + VALIDITY_PROOF
    ).digest()
    timestamp = HEADER["timestamp_ms"]
    cases = [
        ("fresh", timestamp + 1_000, "accept", "none"),
        ("too_early", timestamp - 1, "reject", "not_yet_valid"),
        ("too_late", timestamp + delay_ms + 1, "reject", "expired"),
        ("boundary_low", timestamp, "accept", "none"),
        ("boundary_high", timestamp + delay_ms, "accept", "none"),
    ]
    for name, attested_at_ms, outcome, rejection in cases:
        attestations = []
        for value, guarantor in zip(PRIVATE_KEYS, guarantors):
            guarantor_id = unhex(guarantor["guarantor_id"])
            message = attestation_message(settlement, domain, protocol_version, checkpoint_id, guarantor_id, attested_at_ms)
            digest = hashlib.sha256(attestation_domain + message).digest()
            signature, v = sign(value, digest)
            attestations.append({
                "guarantor_id": hexstr(guarantor_id),
                "replayed": True,
                "data_possessed": True,
                "availability_class_mask": ALL_AVAILABILITY_CLASSES,
                "attested_at_ms": attested_at_ms,
                "signer": guarantor["signer"],
                "signature": hexstr(signature),
                "signature_v": v,
                "message": hexstr(message),
                "digest": hexstr(digest),
            })
        vector = {
            "schema": "layerx/checkpoint-vector/1",
            "case": name,
            "settlement_domain": "vectors",
            "expected_outcome": outcome,
            "expected_rejection": rejection,
            "header": {
                "protocol_version": protocol_version,
                "network_id": domain["network_id"],
                "epoch": HEADER["epoch"],
                "batch_number": HEADER["batch_number"],
                "first_sequence": HEADER["first_sequence"],
                "last_sequence": HEADER["last_sequence"],
                "previous_state_root": hexstr(HEADER["previous_state_root"]),
                "resulting_state_root": hexstr(HEADER["resulting_state_root"]),
                "activity_merkle_root": hexstr(HEADER["activity_merkle_root"]),
                "receipt_merkle_root": hexstr(HEADER["receipt_merkle_root"]),
                "event_merkle_root": hexstr(HEADER["event_merkle_root"]),
                "data_availability_root": hexstr(HEADER["data_availability_root"]),
                "oracle_root": hexstr(HEADER["oracle_root"]),
                "timestamp_ms": timestamp,
                "sequencer_id": hexstr(HEADER["sequencer_id"]),
                "bytes": hexstr(header_bytes),
            },
            "certificate": {
                "validity_proof": hexstr(VALIDITY_PROOF),
                "threshold": threshold,
            },
            "expected_digest": hexstr(checkpoint_id),
            "attestations": attestations,
        }
        (OUTPUT / f"{name}.json").write_text(json.dumps(vector, indent=2) + "\n")
        print(name, outcome, rejection, attested_at_ms)
    return 0


if __name__ == "__main__":
    sys.exit(main())
