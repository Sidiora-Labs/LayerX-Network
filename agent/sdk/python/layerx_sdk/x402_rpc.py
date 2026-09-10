from __future__ import annotations

import json
import re
import urllib.request
import urllib.parse
from typing import Mapping

from .x402 import verify_payment_receipt
from .production import PlatformSdkError


def rpc_hex(value: object, size: int | None = None) -> bytes:
    if (
        not isinstance(value, str)
        or not 0 < len(value) <= 2097152
        or re.fullmatch(r"(?:[0-9a-f]{2})+", value) is None
        or size is not None
        and len(value) != size * 2
    ):
        raise ValueError("invalid-rpc-hex")
    return bytes.fromhex(value)


class PaymentRpcError(Exception):
    def __init__(self, code: int, data: object):
        super().__init__(f"JSON-RPC error {code}")
        self.code = code
        self.data = data


class _NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise ValueError("rpc-redirect")


class PaymentRpc:
    def __init__(self, endpoint: str, headers: Mapping[str, str] | None = None):
        url = urllib.parse.urlsplit(endpoint)
        if (
            url.username
            or url.password
            or url.fragment
            or url.query
            or url.path != "/rpc"
            or not url.hostname
            or url.scheme != "https"
            and not (
                url.scheme == "http"
                and url.hostname in ("localhost", "127.0.0.1", "::1")
            )
        ):
            raise ValueError("invalid-rpc-endpoint")
        self.endpoint = endpoint
        self.headers = dict(headers or {})
        self._id = 0
        self._opener = urllib.request.build_opener(_NoRedirect())

    def call(self, method: str, params: list) -> dict:
        self._id += 1
        request_id = self._id
        request = urllib.request.Request(
            self.endpoint,
            method="POST",
            headers={**self.headers, "Content-Type": "application/json"},
            data=json.dumps(
                {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}
            ).encode(),
        )
        with self._opener.open(request, timeout=30) as response:
            raw = response.read(8388609)
        if len(raw) > 8388608:
            raise ValueError("rpc-body-too-large")
        body = json.loads(raw)
        if (
            not isinstance(body, dict)
            or body.get("jsonrpc") != "2.0"
            or type(body.get("id")) is not int
            or body["id"] != request_id
            or ("error" in body) == ("result" in body)
        ):
            raise ValueError("invalid-rpc-envelope")
        if "error" in body:
            error = body["error"]
            if (
                not isinstance(error, dict)
                or type(error.get("code")) is not int
                or not isinstance(error.get("message"), str)
            ):
                raise ValueError("invalid-rpc-error")
            raise PaymentRpcError(error["code"], error.get("data"))
        if not isinstance(body["result"], dict):
            raise ValueError("invalid-rpc-result")
        return body["result"]

    def send(self, canonical_hex: str, commitment: str) -> dict:
        rpc_hex(canonical_hex)
        if len(canonical_hex) > 1048576 or commitment not in (
            "executed",
            "batched",
            "finalised",
        ):
            raise ValueError("invalid-rpc-submit")
        return self.call("lx_sendActivity", [canonical_hex, commitment])

    def receipt(self, activity_id: str) -> dict:
        rpc_hex(activity_id, 32)
        return self.call("lx_getReceipt", [activity_id])

    def status(self, activity_id: str) -> dict:
        rpc_hex(activity_id, 32)
        return self.call("lx_getActivityStatus", [activity_id])


def verify_rpc_payment(
    result,
    expected_activity,
    expected_payer,
    authorized,
    signatures,
    *,
    amount,
    asset,
    pay_to,
    commitment="executed",
    evidence=None,
):
    rpc_hex(expected_activity, 32)
    rpc_hex(expected_payer, 32)
    if result.get("activity_id") != expected_activity:
        raise ValueError("activity-mismatch")
    if result.get("state") == "pending":
        return None
    if result.get("state") not in (None, "completed"):
        raise ValueError("payment-refused")
    if "commitment" in result and result["commitment"] != commitment:
        raise ValueError("commitment-mismatch")
    try:
        verified = verify_payment_receipt(
            rpc_hex(result.get("receipt")),
            authorized,
            signatures,
            amount=amount,
            asset=asset,
            pay_to=pay_to,
            payer=expected_payer,
            commitment=commitment,
            evidence=evidence,
        )
    except PlatformSdkError as error:
        raise ValueError("payment-binding-mismatch") from error
    if (
        verified.receipt.activity_id.hex() != expected_activity
        or verified.receipt.from_account.hex() != expected_payer
    ):
        raise ValueError("payment-binding-mismatch")
    return verified


def rpc_batch_evidence(result, activity_id, receipt, network_id, authorization):
    from .x402 import PaymentCommitmentEvidence
    from .verifier import MerkleProof

    bundle = result["batch_evidence"]
    signed = bundle["signed_header"]
    proof = bundle["proof"]
    if (
        bundle["kind"] != "receipt"
        or bundle["activity_id"] != activity_id
        or rpc_hex(bundle["canonical_value"]) != receipt
        or rpc_hex(signed["public_key"], 32) != authorization.public_key
        or rpc_hex(signed["sequencer_id"], 32) != authorization.sequencer_id
        or type(proof["leaf_index"]) is not int
        or type(proof["leaf_count"]) is not int
        or not isinstance(proof["siblings"], list)
        or len(proof["siblings"]) > 64
    ):
        raise ValueError("batch-binding-mismatch")
    return PaymentCommitmentEvidence(
        network_id,
        rpc_hex(signed["canonical_header"]),
        rpc_hex(signed["signature"], 64),
        authorization,
        MerkleProof(
            proof["leaf_index"],
            proof["leaf_count"],
            tuple(rpc_hex(v, 32) for v in proof["siblings"]),
        ),
    )
