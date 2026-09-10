import hashlib
import json
from pathlib import Path

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, utils
from test_commitment import SIGNATURES
from layerx_sdk.verifier import decode_batch_header

HERE = Path(__file__).parent
batch = json.loads((HERE / "batch.json").read_text())
header = bytes.fromhex(batch["header"])
h = decode_batch_header(header)


def u(n, size):
    return n.to_bytes(size, "big")


validity = b""
checkpoint_id = hashlib.sha256(
    b"LXP/v2/checkpoint-certificate\0" + header + u(0, 4)
).digest()
contract = bytes.fromhex("11" * 20)
chain = 777
key = ec.derive_private_key(7, ec.SECP256K1())
public = key.public_key().public_bytes(
    serialization.Encoding.X962, serialization.PublicFormat.CompressedPoint
)
uncompressed = key.public_key().public_bytes(
    serialization.Encoding.X962, serialization.PublicFormat.UncompressedPoint
)[1:]
signer = SIGNATURES._keccak256(uncompressed)[12:]
guard = u(1, 32)
message = (
    u(h.protocol_version, 2)
    + u(h.network_id, 4)
    + u(chain, 8)
    + contract
    + u(h.epoch, 8)
    + checkpoint_id * 2
    + guard
    + u(h.batch_number, 8)
    + h.data_availability_root
    + bytes([1, 1, 31])
    + u(1000, 8)
)
digest = hashlib.sha256(b"LXP/v2/guarantor-attestation\0" + message).digest()
r, s = utils.decode_dss_signature(
    key.sign(digest, ec.ECDSA(utils.Prehashed(hashes.SHA256())))
)
s = min(s, SIGNATURES._SECP256K1_N - s)
signature = u(r, 32) + u(s, 32)
v = next(
    v
    for v in (27, 28)
    if SIGNATURES.verify_recoverable_secp256k1(public, signature, v, signer, digest)
)
reference = (
    u(1, 2)
    + u(chain, 8)
    + contract
    + checkpoint_id
    + bytes([3]) * 32
    + u(1, 8)
    + u(1000, 8)
)
checkpoint = (
    u(1, 2)
    + u(len(header), 4)
    + header
    + u(0, 4)
    + u(1, 1)
    + message
    + signer
    + signature
    + u(v, 1)
    + u(1, 1)
    + u(len(reference), 2)
    + reference
)
context = (
    u(1, 2)
    + u(0, 8)
    + u(1, 8)
    + u(1, 8)
    + u(1, 1)
    + guard
    + public
    + u(100, 16)
    + u(1, 8)
    + u(0, 8)
    + u(0, 8)
    + u(1, 1)
    + public
    + u(1, 8)
    + u(0, 8)
    + u(1, 8)
    + u(4, 1)
    + u(h.epoch, 8)
    + u(900, 8)
    + u(2000, 8)
    + u(1000, 8)
    + u(1, 1)
    + u(1, 16)
    + u(1, 1)
    + checkpoint_id
    + h.resulting_state_root
    + u(h.batch_number, 8)
    + u(chain, 8)
    + contract
    + u(len(reference), 2)
    + reference
)
print(
    json.dumps(
        {
            "checkpoint_evidence": {
                "checkpoint_id": checkpoint_id.hex(),
                "checkpoint": checkpoint.hex(),
                "context": context.hex(),
                "canonical_header": header.hex(),
            },
            "operator": {
                "context": context.hex(),
                "guarantor_id": guard.hex(),
                "public_key": public.hex(),
                "required_guarantors": 1,
                "chain_id": chain,
                "contract": contract.hex(),
                "reference": reference.hex(),
            },
        },
        indent=2,
    )
)
