from __future__ import annotations

import json
import unittest
from pathlib import Path

from layerx_sdk.account_derivation import (
    LAYERX_COIN_TYPE,
    AccountDerivationError,
    LayerXBindState,
    bind_nonce_call,
    bind_transaction_request,
    bound_did_call,
    decode_bind_nonce,
    decode_bound_did,
    derive_from_mnemonic,
    derive_from_wallet_signature,
    keccak256,
    key_derivation_hash,
    key_derivation_typed_data,
    normalize_wallet_signature,
    plan_layerx_bind,
    slip10_ed25519,
)

FIXTURE = json.loads(
    (Path(__file__).resolve().parents[4] / "platform/sdk/conformance/fixtures/account-derivation-v1.json").read_text()
)
ABANDON = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"


class AccountDerivationTest(unittest.TestCase):
    def refuses(self, code: str, run) -> None:
        with self.assertRaises(AccountDerivationError) as raised:
            run()
        self.assertEqual(raised.exception.code, code)

    def test_keccak_matches_published_digests(self) -> None:
        self.assertEqual(
            keccak256(b"").hex(), "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470"
        )
        self.assertEqual(
            keccak256(b"a" * 200).hex(),
            keccak256(b"a" * 136 + b"a" * 64).hex(),
        )

    def test_slip10_official_ed25519_vector(self) -> None:
        seed = bytes.fromhex("000102030405060708090a0b0c0d0e0f")
        self.assertEqual(
            slip10_ed25519(seed, ()).hex(), "2b4be7f19ee27bbf30c667b642d5f4aa69fd169872f8fc3059c08ebae2eb19e7"
        )
        self.assertEqual(
            slip10_ed25519(seed, (0, 1)).hex(), "b1d0bad404bf35da785a64ca1ac54b2617211d2777696fbffaf208f746ae84f2"
        )
        self.assertEqual(
            slip10_ed25519(seed, (0, 1, 2, 2, 1000000000)).hex(),
            "8f94d394a8e8fd6b1bc2f3f49f5c47e385281d5c17e65324b0f62483e37e8793",
        )

    def test_mnemonic_vectors_and_bind_calls(self) -> None:
        self.assertEqual(FIXTURE["layerx_coin_type"], LAYERX_COIN_TYPE)
        seen = 0
        for vector in FIXTURE["mnemonic_vectors"]:
            for expected in vector["accounts"]:
                derived = derive_from_mnemonic(
                    "  " + vector["mnemonic"].replace(" ", "  ") + "\n", vector["passphrase"], expected["index"]
                )
                self.assertEqual(derived.evm_address, expected["evm_address"])
                self.assertEqual(derived.layerx_public_key, expected["layerx_public_key"])
                self.assertEqual(derived.did, expected["did"])
                self.assertNotIn(derived.layerx_seed.hex(), repr(derived))
                bind = expected["bind"]
                plan = plan_layerx_bind(derived, bind["chain_id"], LayerXBindState(None, bind["nonce"]))
                self.assertEqual(plan.action, "bind")
                assert plan.call is not None
                self.assertEqual(plan.call.data, "0x" + bind["calldata"])
                self.assertEqual(plan.call.to, bind["to"])
                self.assertEqual(
                    bind_transaction_request(derived, plan.call),
                    {"from": derived.evm_address, "to": bind["to"], "data": "0x" + bind["calldata"], "value": "0x0"},
                )
                already = plan_layerx_bind(
                    derived, bind["chain_id"], LayerXBindState(derived.layerx_public_key.upper(), 1)
                )
                self.assertEqual(already.action, "already_bound")
                self.assertIsNone(already.call)
                self.refuses(
                    "bound_to_different_did",
                    lambda: plan_layerx_bind(derived, bind["chain_id"], LayerXBindState("aa" * 32, 1)),
                )
                seen += 1
        self.assertEqual(seen, 4)
        self.assertEqual(derive_from_mnemonic(ABANDON).evm_address, "0x9858EfFD232B4033E47d90003D41EC34EcaEda94")

    def test_bad_phrases_and_indices_are_refused(self) -> None:
        self.refuses("invalid_mnemonic", lambda: derive_from_mnemonic("abandon abandon abandon"))
        self.refuses("invalid_mnemonic", lambda: derive_from_mnemonic(" ".join(["abandon"] * 12)))
        self.refuses("index_out_of_range", lambda: derive_from_mnemonic(ABANDON, "", 0x80000000))

    def test_wallet_signature_vectors(self) -> None:
        wallet = FIXTURE["wallet_signature"]
        chain_id, address = FIXTURE["chain_id"], wallet["evm_address"]
        for vector in wallet["vectors"]:
            index = vector["index"]
            self.assertEqual(key_derivation_typed_data(chain_id, address, index), vector["typed_data"])
            self.assertEqual(key_derivation_hash(chain_id, address, index).hex(), vector["eip712_hash"])
            derived = derive_from_wallet_signature(chain_id, address, "0x" + vector["signature"], index)
            self.assertEqual(derived.did, vector["did"])
            self.assertEqual(derived.evm_address, address)
            self.assertIsNone(derived.evm_private_key)
            for equivalent in vector["equivalent_signatures"]:
                self.assertNotEqual(equivalent, vector["signature"])
                self.assertEqual(normalize_wallet_signature(equivalent).hex(), vector["signature"])
                self.assertEqual(
                    derive_from_wallet_signature(chain_id, address, bytes.fromhex(equivalent), index).did,
                    vector["did"],
                )
            self.refuses(
                "wallet_signer_mismatch",
                lambda: derive_from_wallet_signature(chain_id + 1, address, vector["signature"], index),
            )
            self.refuses(
                "invalid_wallet_signature",
                lambda: derive_from_wallet_signature(chain_id, address, vector["signature"][:128] + "1d", index),
            )
            self.refuses(
                "invalid_origin",
                lambda: key_derivation_typed_data(chain_id, address, index, 'https://Evil.example/"'),
            )

    def test_precompile_reads(self) -> None:
        address = "0x" + "11" * 20
        self.assertEqual(bind_nonce_call(address), "0xcedd9ba2" + "00" * 12 + "11" * 20)
        self.assertTrue(bound_did_call(address).startswith("0x357feed6"))
        self.assertEqual(decode_bind_nonce("0x" + "00" * 31 + "07"), 7)
        self.assertIsNone(decode_bound_did("0x" + "00" * 128))
        self.assertEqual(decode_bound_did("0x" + "00" * 64 + "ab" * 32 + "00" * 32), "ab" * 32)
        self.refuses("malformed_precompile_answer", lambda: decode_bind_nonce("0x01"))
        self.refuses("malformed_precompile_answer", lambda: decode_bound_did("0x" + "00" * 64))


if __name__ == "__main__":
    unittest.main()
