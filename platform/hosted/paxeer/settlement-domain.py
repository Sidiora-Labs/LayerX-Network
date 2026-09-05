#!/usr/bin/env python3
"""Helpers for deploy-contracts.sh: secp256k1 public-key handling and the
checkpoint settlement document update for one settlement domain."""

import json
import os
import tempfile
import pathlib
import subprocess
import sys

P = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F
SCHEMA = "layerx/checkpoint-settlement/1"


def unhex(text):
    return bytes.fromhex(text[2:] if text.startswith("0x") else text)


def hexstr(data):
    return "0x" + data.hex()


def decompress(compressed):
    if len(compressed) != 33 or compressed[0] not in (2, 3):
        raise SystemExit("public key must be a 33-byte compressed secp256k1 point")
    x = int.from_bytes(compressed[1:], "big")
    if x >= P:
        raise SystemExit("public key x coordinate is not a field element")
    y = pow((x * x * x + 7) % P, (P + 1) // 4, P)
    if (y * y - (x * x * x + 7)) % P != 0:
        raise SystemExit("public key is not on the curve")
    if (y & 1) != (compressed[0] & 1):
        y = P - y
    return x.to_bytes(32, "big") + y.to_bytes(32, "big")


def keccak(data):
    return unhex(
        subprocess.run(["cast", "keccak", hexstr(data)], check=True, capture_output=True, text=True).stdout.strip()
    )


def signer_of(compressed):
    return keccak(decompress(compressed))[12:]


def compress(uncompressed):
    raw = unhex(uncompressed)
    if len(raw) == 65 and raw[0] == 4:
        raw = raw[1:]
    if len(raw) != 64:
        raise SystemExit("uncompressed public key must be 64 bytes")
    prefix = 0x02 if raw[63] % 2 == 0 else 0x03
    return bytes([prefix]) + raw[:32]


def validate_guarantors(guarantors):
    previous = b""
    signers = set()
    for entry in guarantors:
        guarantor_id = unhex(entry["guarantor_id"])
        signer = unhex(entry["signer"])
        public_key = unhex(entry["public_key"])
        if len(guarantor_id) != 32 or guarantor_id <= previous:
            raise SystemExit("guarantor identifiers must be 32 bytes and strictly ascending")
        if len(signer) != 20 or signer in signers:
            raise SystemExit("guarantor signers must be unique 20-byte addresses")
        if signer_of(public_key) != signer:
            raise SystemExit(f"guarantor {entry['guarantor_id']} signer does not match its public key")
        previous = guarantor_id
        signers.add(signer)


def write_domain(path, name, domain):
    document = json.loads(path.read_text())
    if document.get("schema") != SCHEMA:
        raise SystemExit(f"{path} is not a {SCHEMA} document")
    domains = document["settlement_domains"]
    if name == "vectors":
        raise SystemExit("the vectors domain is fixture data and is never rewritten")
    if "vectors" not in domains:
        raise SystemExit("the settlement document must keep its vectors domain")
    before = json.dumps(domains["vectors"], sort_keys=True)
    validate_guarantors(domain["guarantor_set"])
    if len(domain["guarantor_set"]) < document["finality_policy"]["certificate_threshold"]:
        raise SystemExit("guarantor set cannot meet the certificate threshold")
    if int(domain["paxeer_chain_id"]) == 0 or int(domain["network_id"]) == 0:
        raise SystemExit("chain id and network id must be positive")
    for key in ("settlement_contract", "guarantor_bond"):
        if len(unhex(domain[key])) != 20 or unhex(domain[key]) == bytes(20):
            raise SystemExit(f"{key} must be a non-zero address")
    protocol_version = domain.get("protocol_version", document["protocol_version"])
    if type(protocol_version) is not int or protocol_version not in (2, 3):
        raise SystemExit("unsupported settlement protocol version")
    domains[name] = {
        "paxeer_chain_id": int(domain["paxeer_chain_id"]),
        "network_id": int(domain["network_id"]),
        "settlement_contract": domain["settlement_contract"].lower(),
        "guarantor_bond": domain["guarantor_bond"].lower(),
        "guarantor_set": [
            {
                "guarantor_id": entry["guarantor_id"].lower(),
                "signer": entry["signer"].lower(),
                "public_key": entry["public_key"].lower(),
            }
            for entry in domain["guarantor_set"]
        ],
    }
    if "protocol_version" in domain:
        domains[name]["protocol_version"] = protocol_version
        domains[name]["header_encoding_prefix"] = "0x" + protocol_version.to_bytes(2, "big").hex() + "17010f"
    if json.dumps(domains["vectors"], sort_keys=True) != before:
        raise SystemExit("the vectors domain changed")
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode='w', dir=path.parent, delete=False) as output:
            temporary = pathlib.Path(output.name)
            output.write(json.dumps(document, indent=2) + "\n")
            output.flush()
            os.fsync(output.fileno())
        os.chmod(temporary, path.stat().st_mode & 0o777)
        os.replace(temporary, path)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main(argv):
    if len(argv) >= 3 and argv[1] == "signer":
        sys.stdout.write(hexstr(signer_of(unhex(argv[2]))) + "\n")
        return 0
    if len(argv) >= 3 and argv[1] == "compress":
        sys.stdout.write(hexstr(compress(argv[2])) + "\n")
        return 0
    if len(argv) >= 4 and argv[1] == "write":
        write_domain(pathlib.Path(argv[2]), argv[3], json.load(sys.stdin))
        return 0
    sys.stderr.write(
        "usage: settlement-domain.py signer <compressed-public-key>\n"
        "       settlement-domain.py compress <uncompressed-public-key>\n"
        "       settlement-domain.py write <checkpoint-settlement.json> <domain> < domain.json\n"
    )
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
