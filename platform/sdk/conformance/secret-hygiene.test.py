import json
import pytest
from layerx_sdk import (
    SecretBytes,
    PlatformSdkError,
    SdkErrorCode,
    IdempotencyKey,
    ProtocolAmount,
)


class TestSecretBytesHygiene:
    def test_redacts_str(self):
        secret = SecretBytes(b"\x01\x02\x03\x04")
        assert str(secret) == "[REDACTED]"

    def test_redacts_repr(self):
        secret = SecretBytes(b"\x01\x02\x03\x04")
        assert repr(secret) == "SecretBytes([REDACTED])"

    def test_prevents_serialization(self):
        secret = SecretBytes(b"\x42\x43\x44")
        with pytest.raises(TypeError, match="cannot be serialised"):
            import pickle
            pickle.dumps(secret)

    def test_zeroizes_on_del(self):
        secret = SecretBytes(b"\x2a\x2b\x2c")
        captured = None
        def capture(view):
            nonlocal captured
            captured = bytes(view)
        secret.use(capture)
        assert captured[0] == 0x2a
        del secret

    def test_refuses_empty_input(self):
        with pytest.raises(PlatformSdkError) as exc_info:
            SecretBytes(b"")
        assert exc_info.value.code == SdkErrorCode.INVALID_ARGUMENT

    def test_never_exposes_material_through_error_serialization(self):
        secret = SecretBytes(b"\xde\xad\xbe\xef")
        secret.destroy()
        try:
            secret.use(lambda v: None)
        except PlatformSdkError as error:
            serialized = json.dumps(list(error.to_dict().values()))
            assert "de" not in serialized
            assert "ad" not in serialized
            assert "be" not in serialized
            assert "ef" not in serialized
            assert "dead" not in serialized
            assert "beef" not in serialized

    def test_never_logs_key_material_when_stringified(self):
        secret = SecretBytes(b"\x01\x02\xff")
        log_payload = json.dumps({"operation": "sign", "key": str(secret)})
        assert "sign" in log_payload
        assert "[REDACTED]" in log_payload
        assert "01" not in log_payload
        assert "02" not in log_payload
        assert "ff" not in log_payload


class TestIdempotencyKeyHygiene:
    def test_constructs_valid_keys(self):
        key = IdempotencyKey("valid-key-123")
        assert key is not None

    def test_refuses_empty_keys(self):
        with pytest.raises(PlatformSdkError) as exc_info:
            IdempotencyKey("")
        assert exc_info.value.code == SdkErrorCode.INVALID_ARGUMENT

    def test_refuses_overlong_keys(self):
        overlong = "a" * 256
        with pytest.raises(PlatformSdkError):
            IdempotencyKey(overlong)

    def test_refuses_nul_containing_keys(self):
        with pytest.raises(PlatformSdkError):
            IdempotencyKey("has\0null")

    def test_never_leaks_key_material_through_error_serialization(self):
        try:
            IdempotencyKey("")
        except PlatformSdkError as error:
            serialized = json.dumps(error.to_dict())
            assert "invalid-argument" in serialized


class TestProtocolAmountHygiene:
    def test_constructs_integer_amounts(self):
        amount = ProtocolAmount(12345)
        assert amount == 12345

    def test_parses_decimal_strings(self):
        amount = ProtocolAmount("67890")
        assert amount == 67890

    def test_refuses_negative_amounts(self):
        with pytest.raises(PlatformSdkError):
            ProtocolAmount(-1)

    def test_refuses_amounts_exceeding_u128(self):
        too_large = 340282366920938463463374607431768211456
        with pytest.raises(PlatformSdkError):
            ProtocolAmount(too_large)

    def test_refuses_floating_point_representation(self):
        with pytest.raises(PlatformSdkError):
            ProtocolAmount("123.45")

    def test_refuses_scientific_notation(self):
        with pytest.raises(PlatformSdkError):
            ProtocolAmount("1e10")

    def test_makes_floating_point_amounts_structurally_impossible(self):
        amount = ProtocolAmount(100)
        assert isinstance(amount, int)

    def test_refuses_bool_input(self):
        with pytest.raises(PlatformSdkError):
            ProtocolAmount(True)


class TestErrorHygiene:
    def test_never_includes_request_details_in_error_messages(self):
        error = PlatformSdkError(
            SdkErrorCode.TRANSPORT_FAILURE,
            "safe",
            request_id="req-secret-12345",
        )
        assert "req-secret-12345" not in str(error)
        assert str(error) == "The request could not reach the service."

    def test_serializes_only_safe_machine_codes(self):
        error = PlatformSdkError(
            SdkErrorCode.CAPABILITY_REFUSAL,
            "never",
            protocol_result_code=4001,
        )
        serialized = error.to_dict()
        assert serialized["code"] == "capability-refusal"
        assert serialized["retry"] == "never"
        assert serialized["protocol_result_code"] == 4001

    def test_never_includes_session_tokens_in_serialized_errors(self):
        error = PlatformSdkError(
            SdkErrorCode.DEADLINE,
            "safe",
            request_id="token-Bearer-abc123",
        )
        serialized = json.dumps(error.to_dict())
        assert "deadline" in serialized
        assert "token-Bearer-abc123" in serialized
