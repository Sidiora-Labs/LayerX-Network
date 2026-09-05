import argparse
import copy
import contextlib
import socket
import subprocess
import tempfile
import time
from pathlib import Path
import unittest

from custody_credit import Rpc, account_from_proof, agreed_block, attest, common_finalized, decode_rlp, eth_hash, header, quantity, receipt_bytes, rlp, rpc_pair, trie_root, unhex, verified_code, verified_receipts


@contextlib.contextmanager
def local_fork(source, block):
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    with tempfile.TemporaryDirectory() as directory:
        process = subprocess.Popen(["anvil", "--host", "127.0.0.1", "--port", str(port),
                    "--fork-url", source.url, "--fork-block-number", str(quantity(block["number"])),
                    "--no-mining", "--silent"], cwd=directory,
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            rpc = Rpc(f"http://127.0.0.1:{port}")
            deadline = time.monotonic() + 30
            while True:
                if process.poll() is not None or time.monotonic() >= deadline:
                    raise ValueError("real fork startup failed")
                try:
                    if "anvil" in rpc.call("web3_clientVersion", []).lower():
                        break
                except (OSError, ValueError):
                    time.sleep(0.1)
            selected = agreed_block([rpc], block["number"])
            if unhex(selected["hash"], 32) != unhex(block["hash"], 32):
                raise ValueError("fork source block mismatch")
            yield rpc
        finally:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


class RealCustodyEvidence(unittest.TestCase):
    arguments = None

    @classmethod
    def setUpClass(cls):
        cls.rpcs = rpc_pair(cls.arguments.rpc)
        receipt = cls.rpcs[0].call("eth_getTransactionReceipt", [cls.arguments.transaction])
        cls.block = agreed_block(cls.rpcs, receipt["blockHash"], True)
        cls.receipts = verified_receipts(cls.rpcs, cls.block)

    def test_real_receipts_reconstruct_header_root(self):
        self.assertEqual(trie_root([receipt_bytes(value) for value in self.receipts]),
                         unhex(self.block["receiptsRoot"], 32))

    def test_changed_header_refused(self):
        for field in ("parentHash", "stateRoot", "transactionsRoot", "receiptsRoot"):
            block = copy.deepcopy(self.block)
            changed = bytearray(unhex(block[field], 32))
            changed[0] ^= 1
            block[field] = "0x" + changed.hex()
            with self.assertRaises(ValueError):
                header(block)

    def test_changed_receipt_breaks_inclusion(self):
        receipts = copy.deepcopy(self.receipts)
        index = next(index for index, value in enumerate(receipts) if value["logs"])
        changed = bytearray(unhex(receipts[index]["logs"][0]["address"], 20))
        changed[0] ^= 1
        receipts[index]["logs"][0]["address"] = "0x" + changed.hex()
        self.assertNotEqual(trie_root([receipt_bytes(value) for value in receipts]),
                            unhex(self.block["receiptsRoot"], 32))

    def test_removed_log_refused(self):
        changed = copy.deepcopy(next(value for value in self.receipts if value["logs"]))
        changed["logs"][0]["removed"] = True
        with self.assertRaises(ValueError):
            receipt_bytes(changed)

    def test_runtime_account_proof_and_mutation(self):
        profile = Path(self.arguments.profile).read_bytes()
        vault = "0x" + profile[13:33].hex()
        code = verified_code(self.rpcs, vault, self.block)
        evidence = self.rpcs[0].call("eth_getProof", [vault, [],
                    {"blockHash": self.block["hash"], "requireCanonical": True}])
        state_root = unhex(self.block["stateRoot"], 32)
        account = account_from_proof(state_root, unhex(vault, 20), evidence["accountProof"])
        self.assertEqual(account[3], eth_hash(code))
        for index, node in enumerate(evidence["accountProof"]):
            changed = list(evidence["accountProof"])
            damaged = bytearray(unhex(node))
            damaged[-1] ^= 1
            changed[index] = "0x" + damaged.hex()
            with self.assertRaises(ValueError):
                account_from_proof(state_root, unhex(vault, 20), changed)
        for node in evidence["accountProof"]:
            encoded = unhex(node)
            self.assertEqual(rlp(decode_rlp(encoded)), encoded)
            with self.assertRaises(ValueError):
                decode_rlp(encoded + b"\x00")
            prefix = encoded[0]
            if prefix <= 247:
                payload = encoded[1:]
                noncanonical = b"\xf8" + bytes([len(payload)]) + payload
            else:
                width = prefix - 247
                noncanonical = bytes([prefix + 1]) + b"\x00" + encoded[1:]
                self.assertLess(width, 8)
            with self.assertRaises(ValueError):
                decode_rlp(noncanonical)
        with self.assertRaises(ValueError):
            account_from_proof(state_root, unhex(vault, 20), evidence["accountProof"] * 2)

    def test_real_divergent_finalized_forks_refused(self):
        anchor = common_finalized(self.rpcs)
        with local_fork(self.rpcs[0], anchor) as first, local_fork(self.rpcs[0], anchor) as second:
            for offset, rpc in enumerate((first, second), start=1):
                rpc.call("evm_setNextBlockTimestamp", [quantity(anchor["timestamp"]) + offset * 1000],
                         allow_missing=True)
                rpc.call("anvil_mine", ["0xa0", "0x1"], allow_missing=True)
                first_mined = agreed_block([rpc], hex(quantity(anchor["number"]) + 1))
                self.assertEqual(quantity(first_mined["timestamp"]),
                                 quantity(anchor["timestamp"]) + offset * 1000)
            tips = [rpc.call("eth_getBlockByNumber", ["finalized", False]) for rpc in (first, second)]
            for tip in tips:
                header(tip)
                self.assertGreater(quantity(tip["number"]), quantity(anchor["number"]))
            self.assertEqual(quantity(tips[0]["number"]), quantity(tips[1]["number"]))
            self.assertNotEqual(unhex(tips[0]["hash"], 32), unhex(tips[1]["hash"], 32))
            with self.assertRaisesRegex(ValueError, "block quorum disagreement"):
                common_finalized([first, second])

    def test_wrong_expected_amount_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            args = argparse.Namespace(**vars(self.arguments))
            args.expected_amount += 1
            args.output = str(Path(directory) / "refused")
            with self.assertRaises(ValueError):
                attest(args)
            self.assertFalse(Path(args.output).exists())

    def test_real_attestation_and_wrong_beneficiary(self):
        with tempfile.TemporaryDirectory() as directory:
            args = argparse.Namespace(**vars(self.arguments))
            args.output = str(Path(directory) / "credit")
            attest(args)
            self.assertEqual(len(Path(args.output).read_bytes()), 427)
            args.output = str(Path(directory) / "refused")
            changed = bytearray(unhex(args.beneficiary, 32))
            changed[0] ^= 1
            args.beneficiary = "0x" + changed.hex()
            with self.assertRaises(ValueError):
                attest(args)
            self.assertFalse(Path(args.output).exists())

    def test_wrong_chain_genesis_and_runtime_refused(self):
        original = Path(self.arguments.profile).read_bytes()
        for offset in (5, 13, 33, 65, 161, 169, 201, 205):
            with tempfile.TemporaryDirectory() as directory:
                args = argparse.Namespace(**vars(self.arguments))
                changed = bytearray(original)
                changed[offset] ^= 1
                args.profile = str(Path(directory) / "profile")
                Path(args.profile).write_bytes(changed)
                args.output = str(Path(directory) / "refused")
                with self.assertRaises(ValueError):
                    attest(args)
                self.assertFalse(Path(args.output).exists())


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--rpc", action="append", required=True)
    parser.add_argument("--profile", required=True)
    parser.add_argument("--network-id", type=int, required=True)
    parser.add_argument("--transaction", required=True)
    parser.add_argument("--beneficiary", required=True)
    parser.add_argument("--beneficiary-key", required=True)
    parser.add_argument("--expected-amount", type=int, required=True)
    parser.add_argument("--attestor-key", required=True)
    RealCustodyEvidence.arguments = parser.parse_args()
    unittest.main(argv=["test_evidence"], verbosity=2)
