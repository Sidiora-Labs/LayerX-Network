from __future__ import annotations
import re
from .x402_receive import decode_receive


def validate_grant_draw(
    wire: bytes, offer: dict, idempotency_key: str, network_id: int, now: int
) -> dict:
    receive = decode_receive(wire)
    grant = receive["payer_grant"]
    terms = offer["extra"]["layerx"]

    def integer(value, bits):
        return (
            isinstance(value, str)
            and re.fullmatch(r"0|[1-9][0-9]{0,38}", value) is not None
            and int(value) < 1 << bits
        )

    if (
        not integer(offer["amount"], 128)
        or int(offer["amount"]) == 0
        or terms["commitment"] not in ("executed", "batched", "finalised")
        or not isinstance(terms["purposeHash"], str)
        or re.fullmatch(r"[0-9a-f]{64}", terms["purposeHash"]) is None
        or terms["purposeHash"] == "00" * 32
        or receive["asset"] != offer["asset"]
        or receive["to"] != offer["payTo"]
        or receive["amount"] != offer["amount"]
        or receive["idempotency_key"] != idempotency_key
        or receive["grant_id"] != grant["grant_id"]
        or receive["from"] != grant["from"]
        or receive["to"] != grant["recipient"]
        or receive["asset"] != grant["asset"]
        or grant["purpose_hash"] != terms["purposeHash"]
        or int(receive["amount"]) > int(grant["per_draw_maximum"])
        or int(receive["amount"]) > int(grant["allowance"])
        or type(now) is not int
        or not 0 <= now < int(grant["expiration"])
        or type(network_id) is not int
        or receive["receiver_authorization"]["network_id"] != network_id
        or receive["receiver_authorization"]["controller"] != receive["to"]
        or receive["receiver_authorization"]["signed_context_hash"]
        != receive["context_hash"]
        or grant["has_reference"]
        or grant["reference_hash"] != "00" * 32
    ):
        raise ValueError("invalid-grant-draw")
    if offer["scheme"] == "metered":
        if (
            grant["recurring"]
            or grant["window_length"] != "0"
            or "windowSeconds" in terms
        ):
            raise ValueError("invalid-metered-grant")
    elif offer["scheme"] == "subscription":
        if (
            not grant["recurring"]
            or not integer(terms.get("windowSeconds"), 64)
            or terms["windowSeconds"] == "0"
            or terms["windowSeconds"] != grant["window_length"]
        ):
            raise ValueError("invalid-subscription-grant")
    else:
        raise ValueError("unsupported-grant-scheme")
    return receive
