import base64
import copy
import json
from pathlib import Path
import re
import sys

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / "agent/sdk/python"))

from layerx_sdk.verifier import (
    AuthorizedReceiptBatch,
    ReceiptVerificationError,
    verify_receipt_outcome,
)


class Ed25519Signatures:
    def verify_ed25519(self, public_key, signature, digest):
        try:
            Ed25519PublicKey.from_public_bytes(public_key).verify(signature, digest)
            return True
        except (InvalidSignature, ValueError):
            return False


FIELDS = {
    "batchId": "batch_id_hex",
    "asset": "asset_hex",
    "previousStateRoot": "previous_state_root_hex",
    "resultingStateRoot": "resulting_state_root_hex",
    "sequencerPublicKey": "sequencer_public_key_hex",
}


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON member")
        result[key] = value
    return result


def canonical_base64(value, length=None):
    if not isinstance(value, str) or re.fullmatch(r"[A-Za-z0-9_-]+", value) is None:
        raise ValueError("noncanonical base64url")
    decoded = base64.urlsafe_b64decode(value + "=" * (-len(value) % 4))
    if base64.urlsafe_b64encode(decoded).rstrip(b"=").decode() != value:
        raise ValueError("noncanonical base64url")
    if length is not None and len(decoded) != length:
        raise ValueError("wrong field length")
    return decoded


def verify(document, fixture):
    if set(document) != set(FIELDS) | {
        "format", "verificationLevel", "canonicalReceipt", "receiptDigest"
    }:
        raise ValueError("unexpected portable fields")
    if document["format"] != "layerx-receipt-proof-v1" or \
            document["verificationLevel"] != "sequencer-signed":
        raise ValueError("unsupported portable evidence")
    trusted = fixture["authorized_batch"]
    for field, authority in FIELDS.items():
        if canonical_base64(document[field], 32) != bytes.fromhex(trusted[authority]):
            raise ValueError("portable authority mismatch")
    receipt = canonical_base64(document["canonicalReceipt"])
    if len(receipt) > 1_048_576:
        raise ValueError("receipt length limit")
    authority = AuthorizedReceiptBatch(
        *(bytes.fromhex(trusted[field]) for field in FIELDS.values())
    )
    verified = verify_receipt_outcome(
        receipt, authority, Ed25519Signatures(),
        protocol_version=fixture["expected"]["protocol_version"],
    )
    if verified.receipt_digest != canonical_base64(document["receiptDigest"], 32):
        raise ValueError("portable digest mismatch")
    expected = fixture["expected"]
    if verified.receipt_digest.hex() != expected["receipt_digest_hex"] or \
            verified.receipt.result_code != expected["result_code"] or \
            verified.receipt.amount != int(expected["amount"]):
        raise ValueError("independent native result mismatch")
    return verified.receipt_digest.hex()


def main():
    fixture = json.loads(Path(sys.argv[1]).read_text(), object_pairs_hook=unique_object)
    encoded = sys.stdin.buffer.read(1_500_001)
    if len(encoded) > 1_500_000:
        raise ValueError("portable length limit")
    document = json.loads(encoded, object_pairs_hook=unique_object)
    digest = verify(document, fixture)
    mutations = []
    canonical = canonical_base64(document["canonicalReceipt"])
    for index in (0, len(canonical) // 2, len(canonical) - 1):
        changed = bytearray(canonical)
        changed[index] ^= 1
        altered = copy.deepcopy(document)
        altered["canonicalReceipt"] = base64.urlsafe_b64encode(changed).rstrip(b"=").decode()
        mutations.append(altered)
    for field in (*FIELDS, "receiptDigest"):
        altered = copy.deepcopy(document)
        altered[field] = base64.urlsafe_b64encode(bytes([0x91]) * 32).rstrip(b"=").decode()
        mutations.append(altered)
    for field, value in (("format", "unknown"), ("verificationLevel", "checkpoint"),
                         ("canonicalReceipt", 42), ("unrecognised", True),
                         ("asset", document["asset"] + "=")):
        altered = copy.deepcopy(document)
        altered[field] = value
        mutations.append(altered)
    for altered in mutations:
        try:
            verify(altered, fixture)
        except (ReceiptVerificationError, ValueError, TypeError):
            continue
        raise AssertionError("independent verifier accepted a substitution")
    print(json.dumps({"receipt_digest": digest, "mutations_refused": len(mutations)}))


if __name__ == "__main__":
    main()
