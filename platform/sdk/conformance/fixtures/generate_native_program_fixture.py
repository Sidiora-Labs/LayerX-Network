from hashlib import sha256
import json
from pathlib import Path
import struct
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
from cryptography.hazmat.primitives.serialization import Encoding, PublicFormat


def sized(value):
    return struct.pack(">I", len(value)) + value


entrypoint = b"layerx_call"
capabilities = b"\0\0"
access = b"LayerX/programs/access-declaration/v1\0\0"
resources = [1_000_000, 16_777_216, 1_048_576, 1_048_576, 64, 1_048_576, 4096]
payload = struct.pack(">32sHHIHII7Q", bytes([0x11]) * 32, 1, len(entrypoint), 0, len(capabilities), len(access), 16, *resources) + entrypoint + capabilities + access
key = Ed25519PrivateKey.from_private_bytes(bytes([0x33]) * 32)
public = key.public_key().public_bytes(Encoding.Raw, PublicFormat.Raw)
fields = (b"\1\0\3" + b"\2" + struct.pack(">I", 1) + b"\3" + struct.pack(">I", 0x90003)
          + b"\4" + sized(b"did:lxp:native-fixture") + b"\5" + sized(public)
          + b"\6" + bytes(8) + b"\7" + struct.pack(">QQ", 10, 20)
          + b"\10" + sized(bytes([0x44]) * 32) + b"\11" + (1000).to_bytes(16, "big")
          + b"\12" + sized(sha256(b"LXP/v1/payload-hash\0" + payload).digest()) + b"\13" + sized(payload))
unsigned = bytes.fromhex("000310010b") + fields
signature = key.sign(sha256(b"LXP/v1/signature-preimage\0" + unsigned).digest())
signed = bytes.fromhex("000310010c") + fields + b"\14" + sized(signature)
key.public_key().verify(signature, sha256(b"LXP/v1/signature-preimage\0" + unsigned).digest())
fixture = {"name": "native-program-call-v3", "provenance": "Canonical native106 payload and real Ed25519-signed codec vector; no execution claim", "payload_hex": payload.hex(), "signed_activity_hex": signed.hex(), "public_key_hex": public.hex(), "fee_limit": "1000", "activity_id_hex": sha256(b"LXP/v1/activity-id\0" + signed).hexdigest(), "idempotency_key_hex": (bytes([0x44]) * 32).hex(), "resources": [str(value) for value in resources]}
Path(__file__).with_name("native-program-call-v3.json").write_text(json.dumps(fixture, indent=2) + "\n")
