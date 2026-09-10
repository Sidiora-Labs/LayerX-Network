from layerx_sdk import (
    Client,
    IdempotentMutation,
    SubmissionExecuted,
    SubmissionState,
    VerificationLevel,
    VerifiedRead,
    require_verified,
)


def payment_with_verification(
    client: Client,
    request: IdempotentMutation[bytes],
) -> VerifiedRead[SubmissionState]:
    result = client.call("submit", request)
    if not isinstance(result, VerifiedRead):
        raise TypeError("invalid_submit_response")
    verified = require_verified(VerificationLevel.SEQUENCER_SIGNED, result)
    if isinstance(verified.value, SubmissionExecuted):
        return verified
    raise RuntimeError(f"payment_not_executed:{verified.value.kind}")
