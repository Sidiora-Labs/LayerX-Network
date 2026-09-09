import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import subprocess

import pytest


DIRECTORY = Path(__file__).resolve().parent
DOMAIN = b"LayerX/sequencer-trust-history/v1\0"
SPEC = importlib.util.spec_from_file_location("sequencer_pins", DIRECTORY / "sequencer-pins.py")
PINS = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PINS)


@pytest.fixture
def generated(tmp_path):
    key = subprocess.run(["openssl", "genpkey", "-algorithm", "ed25519"],
                         check=True, capture_output=True).stdout
    public = subprocess.run(["openssl", "pkey", "-pubout", "-outform", "DER"],
                            input=key, check=True, capture_output=True).stdout[-32:]
    identity = hashlib.sha256(b"layerx-sequencer:" + public.hex().encode()).hexdigest()
    history = tmp_path / "trust-history"
    subprocess.run(["bash", "-c", 'source "$1"; encode_trust_history "$2" "$3" "$4"',
                    "trust-test", str(DIRECTORY / "beta-cluster.sh"), str(history),
                    identity, public.hex()], check=True, capture_output=True)
    (tmp_path / "sequencer-public-key").write_text(public.hex())
    return tmp_path, identity, public.hex()


def test_generated_protocol_three_history_provisions_exact_pins(generated):
    directory, identity, public = generated
    history = (directory / "trust-history").read_bytes()
    version, network, epoch = struct.unpack_from(">HIQ", history, len(DOMAIN) + 4)
    assert version == 3 and network > 0 and epoch == 1
    output = directory / "authorization.json"
    PINS.provision(directory, output)
    assert json.loads(output.read_text()) == {"layerx": {
        "sequencer_id": identity, "sequencer_public_key": public,
        "sequencer_first_batch": "1", "sequencer_last_batch": str(1 << 40)}}
    assert (directory / "sequencer-id").read_text() == identity
    assert (directory / "sequencer-first-batch").read_text() == "1"
    assert (directory / "sequencer-last-batch").read_text() == str(1 << 40)


@pytest.mark.parametrize("version", [0, 1, 2, 4, 65535])
def test_pin_provision_refuses_every_other_protocol(generated, version):
    directory, _, _ = generated
    history = bytearray((directory / "trust-history").read_bytes())
    struct.pack_into(">H", history, len(DOMAIN) + 4, version)
    (directory / "trust-history").write_bytes(history)
    assert_refused(directory)


def assert_refused(directory):
    output = directory / "authorization.json"
    with pytest.raises(AssertionError):
        PINS.provision(directory, output)
    assert not output.exists()
    for name in ("sequencer-id", "sequencer-first-batch", "sequencer-last-batch"):
        assert not (directory / name).exists()


@pytest.mark.parametrize("mutation", ["domain", "count", "trailing", "truncated", "network",
                                     "epoch", "first", "last", "revoked", "revoked_at", "key"])
def test_pin_provision_preserves_history_refusals(generated, mutation):
    directory, _, _ = generated
    history = bytearray((directory / "trust-history").read_bytes())
    start = len(DOMAIN) + 4
    offsets = {"network": (start + 2, ">I", 0), "epoch": (start + 6, ">Q", 0),
               "first": (start + 78, ">Q", 0), "last": (start + 86, ">Q", 0),
               "revoked": (start + 94, ">B", 1), "revoked_at": (start + 95, ">Q", 1)}
    if mutation in offsets:
        offset, encoding, value = offsets[mutation]
        struct.pack_into(encoding, history, offset, value)
    elif mutation == "domain":
        history[0] ^= 1
    elif mutation == "count":
        struct.pack_into(">H", history, len(DOMAIN), 2)
    elif mutation == "trailing":
        history.append(0)
    elif mutation == "truncated":
        history.pop()
    else:
        history[start + 46] ^= 1
    (directory / "trust-history").write_bytes(history)
    assert_refused(directory)
