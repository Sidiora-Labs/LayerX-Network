import concurrent.futures
import sqlite3
import tempfile
import unittest
from pathlib import Path

from test_commitment import CommitmentTests
from layerx_sdk.x402_agent import PaymentBudget, AgentGrantMiddleware
from layerx_sdk.x402_draw import PreparedGrantDraws
from layerx_sdk.x402_rpc import PaymentRpc
from layerx_sdk.x402_http import ConfiguredReceiptAuthority
from layerx_sdk.x402_receive import decode_receive
from layerx_sdk.x402_http import PaymentEvidence


class AgentBudgetTests(CommitmentTests):
    def test_budget_receipt_commit_and_replay(self):
        offer = {
            "scheme": "exact",
            "network": "layerx:testnet",
            "amount": self.offer["amount"],
            "asset": self.offer["asset"],
            "payTo": self.offer["pay_to"],
            "maxTimeoutSeconds": 30,
        }
        evidence = PaymentEvidence(self.wire, self.authorized)
        with tempfile.TemporaryDirectory() as directory:
            path = str(Path(directory) / "budgets.sqlite")
            budget = PaymentBudget(
                path, "tenant", offer["asset"], int(offer["amount"]) * 2
            )
            key, digest = "01" * 32, "02" * 32
            with concurrent.futures.ThreadPoolExecutor() as pool:
                list(pool.map(lambda _: budget.reserve(key, digest, offer), range(8)))
            for _ in range(2):
                budget.verify_and_commit(key, digest, offer, evidence, self.signatures)
            with self.assertRaises(ValueError):
                budget.reserve(key, "03" * 32, offer)
            second = "04" * 32
            budget.reserve(second, digest, offer)
            with self.assertRaises(sqlite3.IntegrityError):
                budget.verify_and_commit(
                    second, digest, offer, evidence, self.signatures
                )
            with self.assertRaises(ValueError):
                budget.reserve("05" * 32, digest, offer)
            with self.assertRaises(ValueError):
                PaymentBudget(path, "tenant", offer["asset"], 1)
            with sqlite3.connect(path) as db:
                self.assertEqual(
                    db.execute(
                        "SELECT count(*) FROM payment_reservations WHERE receipt_digest IS NOT NULL"
                    ).fetchone(),
                    (1,),
                )

    def test_agent_draw_preserves_unknown_reservation(self):
        receive = bytes.fromhex(
            Path(__file__).with_name("receive.hex").read_text().splitlines()[0]
        )
        canonical = bytes.fromhex(
            Path(__file__).with_name("draw.hex").read_text().splitlines()[0]
        )
        r = decode_receive(receive)
        key, digest = "04" + "00" * 31, "ab" * 32
        offer = {
            "scheme": "subscription",
            "network": "layerx:testnet",
            "amount": r["amount"],
            "asset": r["asset"],
            "payTo": r["to"],
            "maxTimeoutSeconds": 30,
            "extra": {
                "layerx": {
                    "commitment": "executed",
                    "purposeHash": r["payer_grant"]["purpose_hash"],
                    "windowSeconds": "3600",
                }
            },
        }
        with tempfile.TemporaryDirectory() as directory:
            path = str(Path(directory) / "budget.sqlite")
            budget = PaymentBudget(path, "tenant", r["asset"], int(r["amount"]))
            draws = PreparedGrantDraws(
                str(Path(directory) / "draws.sqlite"),
                "did:lxp:pay6-receiver",
                7,
                PaymentRpc("http://127.0.0.1:1/rpc"),
                ConfiguredReceiptAuthority(self.authorized),
                self.signatures,
            )
            draws.register(
                "payer", digest, canonical, receive, key, "subscription:period:1"
            )
            agent = AgentGrantMiddleware(budget, draws, self.signatures)
            for _ in range(2):
                self.assertIsNone(
                    agent(
                        "payer",
                        digest,
                        {"receive": receive.hex(), "idempotencyKey": key},
                        offer,
                    )
                )
            with sqlite3.connect(path) as db:
                self.assertEqual(
                    db.execute(
                        "SELECT amount, receipt_digest FROM payment_reservations"
                    ).fetchall(),
                    [(r["amount"], None)],
                )

    def test_budget_requires_requested_commitment(self):
        offer = {
            "scheme": "exact",
            "network": "layerx:testnet",
            "amount": self.offer["amount"],
            "asset": self.offer["asset"],
            "payTo": self.offer["pay_to"],
            "maxTimeoutSeconds": 30,
            "extra": {"layerx": {"commitment": "finalised"}},
        }
        with tempfile.TemporaryDirectory() as directory:
            path = str(Path(directory) / "budgets.sqlite")
            budget = PaymentBudget(path, "tenant", offer["asset"], int(offer["amount"]))
            key, digest = "01" * 32, "02" * 32
            budget.reserve(key, digest, offer)
            with self.assertRaises(Exception):
                budget.verify_and_commit(
                    key,
                    digest,
                    offer,
                    PaymentEvidence(self.wire, self.authorized),
                    self.signatures,
                )
            with sqlite3.connect(path) as db:
                self.assertEqual(
                    db.execute(
                        "SELECT receipt_digest FROM payment_reservations"
                    ).fetchone(),
                    (None,),
                )


if __name__ == "__main__":
    unittest.main()
