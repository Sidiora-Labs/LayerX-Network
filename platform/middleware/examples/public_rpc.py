import argparse
import hashlib
import json
import os
import urllib.request
import urllib.parse
from pathlib import Path

from layerx_sdk.x402_http import decode_header, validate_required
from layerx_sdk.x402_rpc import PaymentRpc, rpc_hex, verify_rpc_payment, _NoRedirect
from layerx_sdk.verifier import AuthorizedReceiptBatch


def main():
    parser = argparse.ArgumentParser(
        description="Read public RPC sequence, claim faucet funding, or verify a signed exact payment."
    )
    parser.add_argument("--rpc", default=os.environ.get("LAYERX_RPC_URL"))
    parser.add_argument("--did", default=os.environ.get("LAYERX_DID"))
    parser.add_argument("--faucet", default=os.environ.get("LAYERX_FAUCET_URL"))
    parser.add_argument("--public-key")
    parser.add_argument("--claim-key")
    parser.add_argument("--activity")
    parser.add_argument("--offer")
    parser.add_argument("--authority")
    parser.add_argument("--payer")
    args = parser.parse_args()
    if not args.rpc or not args.did:
        parser.error("RPC URL and DID are required through arguments or environment")
    if urllib.parse.urlsplit(args.rpc).port == 18545 or (
        args.faucet and urllib.parse.urlsplit(args.faucet).port == 18545
    ):
        raise ValueError("persistent-host-chain-forbidden")
    token = os.environ.get("LAYERX_RPC_TOKEN")
    rpc = PaymentRpc(args.rpc, {"Authorization": f"Bearer {token}"} if token else {})
    if args.faucet:
        url = urllib.parse.urlsplit(args.faucet)
        if (
            url.username
            or url.password
            or url.query
            or url.fragment
            or url.path != "/v1/faucet/claims"
            or url.scheme != "https"
            and not (
                url.scheme == "http"
                and url.hostname in ("localhost", "127.0.0.1", "::1")
            )
        ):
            raise ValueError("invalid-faucet-url")
        rpc_hex(args.public_key, 32)
        faucet_token = os.environ.get("LAYERX_FAUCET_TOKEN")
        if not args.claim_key or not faucet_token:
            raise ValueError("faucet-claim-key-and-token-required")
        request = urllib.request.Request(
            args.faucet,
            method="POST",
            headers={
                "Content-Type": "application/json",
                "Authorization": f"Bearer {faucet_token}",
                "Idempotency-Key": args.claim_key,
            },
            data=json.dumps({"did": args.did, "public_key": args.public_key}).encode(),
        )
        with urllib.request.build_opener(_NoRedirect()).open(
            request, timeout=30
        ) as response:
            print(
                f"Faucet HTTP status: {response.status}; confirm the account balance before spending."
            )
    print(json.dumps({"sequence": rpc.call("lx_getSequence", [args.did])}))
    if args.activity:
        if not args.offer or not args.authority or not args.payer:
            raise ValueError("offer-authority-and-payer-required")
        required = validate_required(
            decode_header(Path(args.offer).read_text().strip())
        )
        offer = next(
            v
            for v in required["accepts"]
            if v["scheme"] in ("exact", "metered", "subscription")
            and v.get("extra", {}).get("layerx", {}).get("commitment", "executed")
            == "executed"
        )
        canonical = Path(args.activity).read_text().strip()
        activity = hashlib.sha256(
            b"LXP/v1/activity-id\0" + rpc_hex(canonical)
        ).hexdigest()
        configured = json.loads(Path(args.authority).read_text())
        names = {
            "batch_id": "batchId",
            "asset": "asset",
            "previous_state_root": "previousStateRoot",
            "resulting_state_root": "resultingStateRoot",
            "sequencer_public_key": "sequencerPublicKey",
        }
        authority = AuthorizedReceiptBatch(
            **{key: rpc_hex(configured[name], 32) for key, name in names.items()}
        )
        from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey
        from cryptography.exceptions import InvalidSignature

        class Signatures:
            def verify_ed25519(self, public_key, signature, digest):
                try:
                    Ed25519PublicKey.from_public_bytes(public_key).verify(
                        signature, digest
                    )
                    return True
                except (InvalidSignature, ValueError):
                    return False

        result = rpc.send(canonical, "executed")
        verified = verify_rpc_payment(
            result,
            activity,
            args.payer,
            authority,
            Signatures(),
            amount=offer["amount"],
            asset=offer["asset"],
            pay_to=offer["payTo"],
        )
        if verified is None:
            print(json.dumps({"state": "pending", "activity_id": activity}))
            return 2
        if offer["scheme"] != "exact" and (
            verified.receipt.module_id != 1 or verified.receipt.operation != 6
        ):
            raise ValueError("grant-receive-receipt-required")
        digest = hashlib.sha256(
            b"LXP/v1/merkle-leaf\0" + verified.canonical_bytes
        ).hexdigest()
        print(json.dumps({"transaction": "lxp:" + digest, "commitment": "executed"}))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
