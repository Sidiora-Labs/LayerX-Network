from __future__ import annotations

import json
import unittest
from pathlib import Path
from typing import Any

from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from cryptography.hazmat.primitives.kdf.hkdf import HKDF

from layerx_sdk import (
    XWEB_ENVELOPE_INFO,
    XWEB_KIND_API,
    XWEB_LEVEL_MAJORITY,
    XWEB_LEVEL_SINGLE,
    XWEB_PRECOMPILE,
    XWebApiCall,
    XWebApiError,
    XWebApiHeader,
    XWebAttestor,
    XWebAttestorSet,
    XWebEnvelopeRandomness,
    build_xweb_api_request,
    decode_xweb_api_payload,
    decode_xweb_attestors,
    encode_xweb_api_payload,
    encode_xweb_credential,
    seal_xweb_envelope,
    xweb_api_origin,
    xweb_api_request_call,
    xweb_attestor_address,
    xweb_get_attestors_call_data,
)
from layerx_sdk.account_derivation import keccak256

_TESTDATA = Path(__file__).resolve().parents[4] / "modules" / "xweb" / "types" / "testdata"
_ENVELOPES: dict[str, Any] = json.loads((_TESTDATA / "envelope-vectors.json").read_text())
_API: dict[str, Any] = json.loads((_TESTDATA / "api-vectors.json").read_text())


def _bytes(value: str) -> bytes:
    return bytes.fromhex(value[2:])


def _hex(value: bytes) -> str:
    return "0x" + value.hex()


def _label_key(label: str) -> bytes:
    return keccak256(label.encode("ascii"))


def _envelope(name: str) -> dict[str, Any]:
    return next(vector for vector in _ENVELOPES["vectors"] if vector["name"] == name)


def _headers(entries: list[dict[str, str]]) -> tuple[XWebApiHeader, ...]:
    return tuple(XWebApiHeader(entry["name"], entry["value"]) for entry in entries)


def _randomness(vector: dict[str, Any]) -> XWebEnvelopeRandomness:
    return XWebEnvelopeRandomness(_label_key(vector["ephemeral_key_label"]), _bytes(vector["nonce"]))


def _compressed(private_key: bytes) -> bytes:
    key = ec.derive_private_key(int.from_bytes(private_key, "big"), ec.SECP256K1())
    return key.public_key().public_bytes(serialization.Encoding.X962, serialization.PublicFormat.CompressedPoint)


_ONE = _envelope("attestor-1-api-key")
_TWO = _envelope("attestor-2-api-key")
_SET = XWebAttestorSet(
    (
        XWebAttestor(_ONE["attestor"], "pax1attestorone", _bytes(_ONE["attestor_public_key"])),
        XWebAttestor(_TWO["attestor"], "pax1attestortwo", _bytes(_TWO["attestor_public_key"])),
    ),
    2,
)
_PRICE = XWebApiCall("GET", "https://paxeer.app/api/v1/price?asset=PAX", pointers=("/data/price",))
_API_KEY = (XWebApiHeader("X-Api-Key", "paxeer-vector-credential"),)


class XWebApiVectorTests(unittest.TestCase):
    def assert_refused(self, fragment: str, run, *args: Any, **kwargs: Any) -> None:
        with self.assertRaises(XWebApiError) as caught:
            run(*args, **kwargs)
        self.assertIn(fragment, str(caught.exception))

    def test_shared_constants(self) -> None:
        self.assertEqual(_ENVELOPES["hkdf_info"].encode("ascii"), XWEB_ENVELOPE_INFO)
        self.assertEqual(_API["kind"], XWEB_KIND_API)
        self.assertEqual(_API["levels"]["majority"], XWEB_LEVEL_MAJORITY)
        self.assertEqual(_API["levels"]["single"], XWEB_LEVEL_SINGLE)
        self.assertEqual(XWEB_PRECOMPILE, "0x0000000000000000000000000000000000001019")

    def test_envelope_vectors(self) -> None:
        for vector in _ENVELOPES["vectors"]:
            with self.subTest(vector["name"]):
                self.assertEqual(_hex(_compressed(_label_key(vector["attestor_key_label"]))), vector["attestor_public_key"])
                self.assertEqual(xweb_attestor_address(vector["attestor_public_key"]), vector["attestor"])
                plaintext = encode_xweb_credential(_headers(vector["credential"]))
                self.assertEqual(_hex(plaintext), vector["plaintext"])

                ephemeral = ec.derive_private_key(
                    int.from_bytes(_label_key(vector["ephemeral_key_label"]), "big"), ec.SECP256K1()
                )
                self.assertEqual(_hex(_compressed(_label_key(vector["ephemeral_key_label"]))), vector["ephemeral_public_key"])
                recipient = ec.EllipticCurvePublicKey.from_encoded_point(
                    ec.SECP256K1(), _bytes(vector["attestor_public_key"])
                )
                shared = ephemeral.exchange(ec.ECDH(), recipient)
                self.assertEqual(_hex(shared), vector["shared_x"])
                key = HKDF(
                    algorithm=hashes.SHA256(), length=32, salt=_bytes(vector["ephemeral_public_key"]), info=XWEB_ENVELOPE_INFO
                ).derive(shared)
                self.assertEqual(_hex(key), vector["aes_key"])
                self.assertEqual(vector["attestor"] + vector["origin"].encode("ascii").hex(), vector["aad"])

                sealed = seal_xweb_envelope(vector["attestor_public_key"], vector["origin"], plaintext, _randomness(vector))
                self.assertEqual(_hex(sealed), vector["envelope"])
                self.assertEqual(_hex(sealed[20 + 33 + 12 :]), vector["ciphertext"])
                opened = AESGCM(key).decrypt(_bytes(vector["nonce"]), sealed[20 + 33 + 12 :], _bytes(vector["aad"]))
                self.assertEqual(_hex(opened), vector["plaintext"])

    def test_fresh_randomness(self) -> None:
        plaintext = _bytes(_ONE["plaintext"])
        first = seal_xweb_envelope(_ONE["attestor_public_key"], _ONE["origin"], plaintext)
        second = seal_xweb_envelope(_ONE["attestor_public_key"], _ONE["origin"], plaintext)
        self.assertEqual(len(first), len(_bytes(_ONE["envelope"])))
        self.assertEqual(_hex(first[:20]), _ONE["attestor"])
        self.assertNotEqual(first[20:65], second[20:65])
        self.assert_refused("plaintext is 0 bytes", seal_xweb_envelope, _ONE["attestor_public_key"], _ONE["origin"], b"")
        self.assert_refused(
            "not a secp256k1 scalar",
            seal_xweb_envelope,
            _ONE["attestor_public_key"],
            _ONE["origin"],
            plaintext,
            XWebEnvelopeRandomness(bytes(32), _bytes(_ONE["nonce"])),
        )
        self.assert_refused(
            "not a point on secp256k1", seal_xweb_envelope, "0x02" + "00" * 32, _ONE["origin"], plaintext
        )

    def test_api_vectors(self) -> None:
        for vector in _API["vectors"]:
            with self.subTest(vector["name"]):
                named = [_envelope(name) for name in vector["envelopes"]]
                by_attestor = {entry["attestor"]: _randomness(entry) for entry in named}
                call = XWebApiCall(
                    vector["method"],
                    vector["url"],
                    headers=_headers(vector["headers"]),
                    body=vector["body"],
                    pointers=tuple(vector["pointers"]),
                    single=vector["attestor"] if vector["level"] == XWEB_LEVEL_SINGLE else None,
                )
                built = build_xweb_api_request(
                    call,
                    _SET,
                    credential=_headers(named[0]["credential"]) if named else None,
                    randomness=by_attestor.__getitem__,
                )
                self.assertEqual(_hex(built.payload), vector["payload"])
                self.assertEqual(built.payload_hash, vector["payload_hash"])
                self.assertEqual(_hex(keccak256(built.payload)), vector["payload_hash"])
                self.assertEqual(built.origin, vector["origin"])
                self.assertEqual(built.level, vector["level"])
                self.assertEqual(built.attestor, vector["attestor"])
                self.assertEqual([_hex(entry.envelope) for entry in built.envelopes], [entry["envelope"] for entry in named])

                decoded = decode_xweb_api_payload(_bytes(vector["payload"]))
                self.assertEqual(decoded.method, vector["method"])
                self.assertEqual(decoded.level, vector["level"])
                self.assertEqual(decoded.attestor, vector["attestor"])
                self.assertEqual(decoded.url, vector["url"])
                self.assertEqual(decoded.headers, _headers(vector["headers"]))
                self.assertEqual(decoded.body, vector["body"].encode("utf-8"))
                self.assertEqual(decoded.pointers, tuple(vector["pointers"]))
                self.assertEqual(_hex(encode_xweb_api_payload(decoded)), vector["payload"])
                self.assertEqual(xweb_api_origin(vector["url"]), vector["origin"])

    def test_refusal_vectors(self) -> None:
        for refusal in _API["refusals"]:
            with self.subTest(refusal["name"]):
                self.assert_refused(refusal["refuses"], decode_xweb_api_payload, _bytes(refusal["payload"]))

    def test_builder_refusals(self) -> None:
        self.assert_refused(
            "the single level names",
            build_xweb_api_request,
            XWebApiCall("GET", _PRICE.url, pointers=_PRICE.pointers, single="0x" + "00" * 19 + "aa"),
            _SET,
        )
        unkeyed = XWebAttestorSet((_SET.attestors[0], XWebAttestor(_TWO["attestor"], "pax1attestortwo", b"")), 2)
        self.assert_refused(
            "1 of 2 attestors take credential envelopes", build_xweb_api_request, _PRICE, unkeyed, credential=_API_KEY
        )
        swapped = XWebAttestorSet(
            (XWebAttestor(_ONE["attestor"], "pax1attestorone", _bytes(_TWO["attestor_public_key"])),), 1
        )
        self.assert_refused("belongs to", build_xweb_api_request, _PRICE, swapped, credential=_API_KEY)
        self.assert_refused(
            "repeats",
            build_xweb_api_request,
            _PRICE,
            _SET,
            credential=(XWebApiHeader("Accept", "x"), XWebApiHeader("accept", "y")),
        )
        self.assert_refused(
            "repeats a public header",
            build_xweb_api_request,
            XWebApiCall("GET", _PRICE.url, headers=(XWebApiHeader("X-Api-Key", "public"),)),
            _SET,
            credential=_API_KEY,
        )
        self.assert_refused("credential carries no header", build_xweb_api_request, _PRICE, _SET, credential=())
        self.assert_refused(
            "GET carries", build_xweb_api_request, XWebApiCall("GET", "https://paxeer.app/status", body="x"), _SET
        )
        self.assert_refused("is not https", build_xweb_api_request, XWebApiCall("GET", "http://paxeer.app/status"), _SET)
        self.assert_refused(
            "only lower-case letters", build_xweb_api_request, XWebApiCall("GET", "https://Paxeer.app/status"), _SET
        )
        self.assert_refused(
            "does not start with /",
            build_xweb_api_request,
            XWebApiCall("GET", "https://paxeer.app/status", pointers=("data",)),
            _SET,
        )
        padded = tuple(XWebApiHeader(f"X-Pad-{index}", "p" * 1024) for index in range(5))
        self.assert_refused(
            "payload is",
            build_xweb_api_request,
            XWebApiCall("POST", "https://paxeer.app/api/v1/quote", headers=padded, body="x" * 4000),
            _SET,
            max_payload_bytes=8192,
        )

    def test_majority_envelopes_with_fresh_randomness(self) -> None:
        built = build_xweb_api_request(_PRICE, _SET, credential=_API_KEY)
        self.assertEqual([entry.attestor for entry in built.envelopes], [_ONE["attestor"], _TWO["attestor"]])
        self.assertEqual(len(decode_xweb_api_payload(built.payload).envelopes), 2)
        self.assertNotIn(b"paxeer-vector-credential", built.payload)

    def test_request_call(self) -> None:
        vector = next(entry for entry in _API["vectors"] if entry["name"] == "example-consumer")
        payload = _bytes(vector["payload"])
        call = xweb_api_request_call(payload, 200_000, 1_000)
        self.assertEqual(call.to, XWEB_PRECOMPILE)
        self.assertEqual(call.value, 1_000)
        self.assertTrue(call.data.startswith(_hex(keccak256(b"request(uint8,bytes,uint64)")[:4])))
        words = bytes.fromhex(call.data[10:])
        self.assertEqual(int.from_bytes(words[0:32], "big"), 3)
        self.assertEqual(int.from_bytes(words[32:64], "big"), 96)
        self.assertEqual(int.from_bytes(words[64:96], "big"), 200_000)
        length = int.from_bytes(words[96:128], "big")
        self.assertEqual(words[128 : 128 + length], payload)
        self.assertEqual(len(words), 128 + -(-length // 32) * 32)
        self.assert_refused("callback gas", xweb_api_request_call, payload, 0, 1)

    def test_get_attestors(self) -> None:
        self.assertEqual(xweb_get_attestors_call_data(), _hex(keccak256(b"getAttestors()")[:4]))

        def word(value: int) -> bytes:
            return value.to_bytes(32, "big")

        def dynamic(data: bytes) -> bytes:
            return word(len(data)) + data + b"\x00" * (-len(data) % 32)

        def entry(signer: str, payout: str, public_key: bytes) -> bytes:
            payout_tail = dynamic(payout.encode("utf-8"))
            return word(int(signer, 16)) + word(96) + word(96 + len(payout_tail)) + payout_tail + dynamic(public_key)

        first = entry(_ONE["attestor"], "pax1attestorone", _bytes(_ONE["attestor_public_key"]))
        second = entry(_TWO["attestor"], "pax1attestortwo", b"")
        answer = word(64) + word(2) + word(2) + word(64) + word(64 + len(first)) + first + second
        decoded = decode_xweb_attestors(_hex(answer))
        self.assertEqual(decoded.required, 2)
        self.assertEqual(
            decoded.attestors,
            (
                XWebAttestor(_ONE["attestor"], "pax1attestorone", _bytes(_ONE["attestor_public_key"])),
                XWebAttestor(_TWO["attestor"], "pax1attestortwo", b""),
            ),
        )
        mismatched = word(64) + word(1) + word(1) + word(32) + entry(
            _TWO["attestor"], "pax1attestortwo", _bytes(_ONE["attestor_public_key"])
        )
        self.assert_refused("belongs to", decode_xweb_attestors, _hex(mismatched))
        self.assert_refused("ends before byte", decode_xweb_attestors, _hex(answer[:99]))


if __name__ == "__main__":
    unittest.main()
