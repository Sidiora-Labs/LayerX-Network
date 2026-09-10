from layerx_sdk import Client, IdempotentMutation, SubmissionExecuted, SubmissionState


def settle_402(client: Client, request: IdempotentMutation[bytes]) -> str:
    state = client.call("submit", request)
    if isinstance(state, SubmissionExecuted):
        return state.receipt_ref
    kind = state.kind if isinstance(state, SubmissionState.__args__) else "Invalid"
    raise RuntimeError(f"settlement_failed:{kind}")
