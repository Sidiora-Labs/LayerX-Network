#!/usr/bin/env python3
"""Render the layerx-mirror publisher configuration from deployed identities.

Every chain identity is supplied by the caller from a real deployment: the
renderer carries only the bounded operational constants of
`config.example.json`, the handles and socket of the layerx-mirror-signer
container co-located with the publisher, and refuses any identity, endpoint or
path whose shape the publisher's own configuration loader would reject. The
signer handles and socket are overridable for a deployment that keeps the
publisher keys in an external signer.

The Ethereum mirror target is always rendered. The Solana target is rendered
from the `--solana-*` identities when they are given and left out of the
configuration entirely when none of them is, which the publisher reads as a
deployment that mirrors to the EVM chain only. A partial Solana target is
refused by name rather than half rendered.
"""

import argparse
import json
import pathlib
import sys

BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

ETHEREUM_CHUNK_BYTES = 24576
ETHEREUM_REQUIRED_CONFIRMATIONS = 12
ETHEREUM_MAXIMUM_REORG_DEPTH = 256
ETHEREUM_TRANSACTION_GAS_LIMIT = 30000000
ETHEREUM_MAXIMUM_FEE_PER_GAS = 100000000000
ETHEREUM_MAXIMUM_PRIORITY_FEE_PER_GAS = 5000000000
SOLANA_CHUNK_BYTES = 640
SOLANA_REQUIRED_ROOTED_SLOTS = 32
SOLANA_MAXIMUM_ANCESTRY = 4096
SIGNER_TIMEOUT_MS = 5000
SIGNER_SOCKET = "/run/mirror-signer/signer.sock"
ETHEREUM_KEY_HANDLE = "mirror/ethereum/beta"
SOLANA_KEY_HANDLE = "mirror/solana/beta"
POLL_INTERVAL_MS = 5000
FRAME_BYTES = 67108864
ARCHIVE_CHUNKS = 65536


def refuse(message):
    raise SystemExit("render-config: %s" % message)


def base58_decode(value, field):
    if not value or len(value) > 64:
        refuse("%s is not base58" % field)
    number = 0
    for character in value:
        index = BASE58_ALPHABET.find(character)
        if index < 0:
            refuse("%s is not base58" % field)
        number = number * 58 + index
    leading = len(value) - len(value.lstrip("1"))
    body = number.to_bytes((number.bit_length() + 7) // 8, "big") if number else b""
    return b"\x00" * leading + body


def base58(value, field):
    decoded = base58_decode(value, field)
    if len(decoded) != 32:
        refuse("%s must decode to 32 bytes, got %d" % (field, len(decoded)))
    if decoded == bytes(32):
        refuse("%s is the zero identity" % field)
    return value


def hexadecimal(value, field, length):
    text = value[2:] if value.startswith("0x") else value
    text = text.lower()
    if len(text) != length * 2 or any(character not in "0123456789abcdef" for character in text):
        refuse("%s must be %d hexadecimal bytes" % (field, length))
    if int(text, 16) == 0:
        refuse("%s is the zero identity" % field)
    return text


def absolute(value, field):
    if not value or not pathlib.PurePosixPath(value).is_absolute():
        refuse("%s must be an absolute container path, got %r" % (field, value))
    return value


def endpoints(values, field):
    parsed = []
    backends = set()
    origins = set()
    for value in values:
        parts = value.split(",")
        if len(parts) != 4:
            refuse("%s takes URL,BACKEND,CA_DER_PATH,TOKEN_PATH, got %r" % (field, value))
        url, backend, certificate, token = parts
        if not url.startswith("https://"):
            refuse("%s endpoint %r is not an https URL" % (field, url))
        if not backend or any(
            not (character.isalnum() or character in "-_.") for character in backend
        ):
            refuse("%s backend %r is not an independent backend name" % (field, backend))
        if backend in backends:
            refuse("%s repeats the independent backend %r" % (field, backend))
        origin = url.split("/", 3)[2].lower()
        if origin in origins:
            refuse("%s repeats the network origin %r" % (field, origin))
        backends.add(backend)
        origins.add(origin)
        parsed.append(
            {
                "url": url,
                "ca_certificate_der": absolute(certificate, "%s CA path" % field),
                "bearer_token_file": absolute(token, "%s token path" % field),
                "independent_backend": backend,
            }
        )
    if len(parsed) < 2:
        refuse("%s requires at least two independent backends" % field)
    quorum = len(parsed) // 2 + 1
    return {
        "endpoints": parsed,
        "quorum": quorum,
        "connect_timeout_ms": 3000,
        "request_timeout_ms": 15000,
        "maximum_response_bytes": 16777216,
    }


def signer(key_handle, public_key, socket, field):
    if not key_handle or len(key_handle) > 128 or "\x00" in key_handle:
        refuse("%s key handle is empty or too long" % field)
    return {
        "key_handle": key_handle,
        "public_key": public_key,
        "timeout_ms": SIGNER_TIMEOUT_MS,
        "transport": {"kind": "uds", "socket": absolute(socket, "%s signer socket" % field)},
    }


def main(argv):
    parser = argparse.ArgumentParser(add_help=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--state-directory", required=True)
    parser.add_argument("--first-batch-number", type=int, required=True)
    parser.add_argument("--status-listen", required=True)
    parser.add_argument("--lni-socket", required=True)
    parser.add_argument("--network-id", type=int, required=True)
    parser.add_argument("--protocol-version", type=int, required=True)
    parser.add_argument("--ethereum-endpoint", action="append", default=[])
    parser.add_argument("--ethereum-chain-id", type=int, required=True)
    parser.add_argument("--ethereum-genesis-hash", required=True)
    parser.add_argument("--ethereum-archive-contract", required=True)
    parser.add_argument("--ethereum-archive-code-hash", required=True)
    parser.add_argument("--ethereum-signer-key-handle", default=ETHEREUM_KEY_HANDLE)
    parser.add_argument("--ethereum-signer-public-key", required=True)
    parser.add_argument("--ethereum-signer-socket", default=SIGNER_SOCKET)
    parser.add_argument("--solana-endpoint", action="append", default=[])
    parser.add_argument("--solana-genesis-hash")
    parser.add_argument("--solana-archive-program")
    parser.add_argument("--solana-upgradeable-loader")
    parser.add_argument("--solana-program-data-account")
    parser.add_argument("--solana-program-code-hash")
    parser.add_argument("--solana-signer-key-handle", default=SOLANA_KEY_HANDLE)
    parser.add_argument("--solana-signer-public-key")
    parser.add_argument("--solana-signer-socket", default=SIGNER_SOCKET)
    arguments = parser.parse_args(argv)

    if arguments.first_batch_number < 1:
        refuse("--first-batch-number must be at least 1")
    if arguments.network_id < 1:
        refuse("--network-id must be a real LayerX network id")

    solana_inputs = {
        "--solana-endpoint": arguments.solana_endpoint,
        "--solana-genesis-hash": arguments.solana_genesis_hash,
        "--solana-archive-program": arguments.solana_archive_program,
        "--solana-upgradeable-loader": arguments.solana_upgradeable_loader,
        "--solana-program-data-account": arguments.solana_program_data_account,
        "--solana-program-code-hash": arguments.solana_program_code_hash,
        "--solana-signer-public-key": arguments.solana_signer_public_key,
    }
    missing = sorted(name for name, value in solana_inputs.items() if not value)
    solana_requested = len(missing) < len(solana_inputs)
    if solana_requested and missing:
        refuse(
            "the Solana mirror target needs every identity it publishes under; missing %s"
            % ", ".join(missing)
        )
    if solana_requested and arguments.ethereum_signer_key_handle == arguments.solana_signer_key_handle:
        refuse("the Ethereum and Solana signer key handles must differ")
    host, separator, port = arguments.status_listen.rpartition(":")
    if not separator or host not in ("127.0.0.1", "[::1]") or not port.isdigit() or int(port) == 0:
        refuse("--status-listen must be a loopback address with a port, got %r" % arguments.status_listen)

    ethereum_public_key = arguments.ethereum_signer_public_key
    stripped = ethereum_public_key[2:] if ethereum_public_key.startswith("0x") else ethereum_public_key
    if len(stripped) not in (66, 130):
        refuse("--ethereum-signer-public-key must be a SEC1 secp256k1 point")
    hexadecimal(stripped, "--ethereum-signer-public-key", len(stripped) // 2)

    config = {
        "state_directory": absolute(arguments.state_directory, "--state-directory"),
        "first_batch_number": arguments.first_batch_number,
        "poll_interval_ms": POLL_INTERVAL_MS,
        "status_listen": arguments.status_listen,
        "node": {
            "socket": absolute(arguments.lni_socket, "--lni-socket"),
            "expected_protocol_version": arguments.protocol_version,
            "expected_network_id": arguments.network_id,
            "maximum_frame_bytes": FRAME_BYTES,
            "maximum_connections": 2,
            "maximum_streams": 2,
            "maximum_queued_bytes": FRAME_BYTES,
            "deadline_ms": 30000,
            "maximum_archive_bytes": FRAME_BYTES,
            "maximum_archive_chunks": ARCHIVE_CHUNKS,
        },
        "ethereum": {
            "rpc": endpoints(arguments.ethereum_endpoint, "--ethereum-endpoint"),
            "chain_id": arguments.ethereum_chain_id,
            "genesis_hash_hex": hexadecimal(
                arguments.ethereum_genesis_hash, "--ethereum-genesis-hash", 32
            ),
            "archive_contract_hex": hexadecimal(
                arguments.ethereum_archive_contract, "--ethereum-archive-contract", 20
            ),
            "archive_code_hash_hex": hexadecimal(
                arguments.ethereum_archive_code_hash, "--ethereum-archive-code-hash", 32
            ),
            "required_confirmations": ETHEREUM_REQUIRED_CONFIRMATIONS,
            "maximum_reorg_depth": ETHEREUM_MAXIMUM_REORG_DEPTH,
            "chunk_bytes": ETHEREUM_CHUNK_BYTES,
            "transaction_gas_limit": ETHEREUM_TRANSACTION_GAS_LIMIT,
            "maximum_fee_per_gas": ETHEREUM_MAXIMUM_FEE_PER_GAS,
            "maximum_priority_fee_per_gas": ETHEREUM_MAXIMUM_PRIORITY_FEE_PER_GAS,
            "signer": signer(
                arguments.ethereum_signer_key_handle,
                stripped,
                arguments.ethereum_signer_socket,
                "ethereum",
            ),
        },
    }
    if solana_requested:
        config["solana"] = {
            "rpc": endpoints(arguments.solana_endpoint, "--solana-endpoint"),
            "genesis_hash_base58": base58(arguments.solana_genesis_hash, "--solana-genesis-hash"),
            "archive_program_base58": base58(
                arguments.solana_archive_program, "--solana-archive-program"
            ),
            "upgradeable_loader_base58": base58(
                arguments.solana_upgradeable_loader, "--solana-upgradeable-loader"
            ),
            "program_data_account_base58": base58(
                arguments.solana_program_data_account, "--solana-program-data-account"
            ),
            "program_code_hash_hex": hexadecimal(
                arguments.solana_program_code_hash, "--solana-program-code-hash", 32
            ),
            "required_rooted_slots": SOLANA_REQUIRED_ROOTED_SLOTS,
            "maximum_ancestry": SOLANA_MAXIMUM_ANCESTRY,
            "chunk_bytes": SOLANA_CHUNK_BYTES,
            "signer": signer(
                arguments.solana_signer_key_handle,
                base58(arguments.solana_signer_public_key, "--solana-signer-public-key"),
                arguments.solana_signer_socket,
                "solana",
            ),
        }
    output = pathlib.Path(arguments.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    with open(output, "w", encoding="utf-8") as handle:
        json.dump(config, handle, indent=2, sort_keys=False)
        handle.write("\n")
    output.chmod(0o600)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
