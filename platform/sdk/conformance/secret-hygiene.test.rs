#[cfg(test)]
mod secret_hygiene_tests {
    use layerx_sdk::production::{
        IdempotencyKey, ProductionError, ProtocolAmount, RetryClass, SdkErrorCode, SecretBytes,
    };

    #[test]
    fn secret_bytes_redacts_debug() {
        let secret = SecretBytes::new(&[1, 2, 3, 4]).unwrap();
        let debug = format!("{secret:?}");
        assert_eq!(debug, "SecretBytes([REDACTED])");
        assert!(!debug.contains("1"));
        assert!(!debug.contains("2"));
    }

    #[test]
    fn secret_bytes_zeroizes_on_drop() {
        let secret = SecretBytes::new(&[42, 43, 44]).unwrap();
        let mut captured = vec![];
        secret.expose_to(|bytes| captured.extend_from_slice(bytes));
        assert_eq!(captured[0], 42);
        drop(secret);
    }

    #[test]
    fn secret_bytes_refuses_empty_input() {
        let result = SecretBytes::new(&[]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, SdkErrorCode::InvalidArgument);
    }

    #[test]
    fn secret_bytes_never_exposes_material_through_error_serialization() {
        let secret = SecretBytes::new(&[0xde, 0xad, 0xbe, 0xef]).unwrap();
        drop(secret);
        let error = ProductionError::new(SdkErrorCode::InternalFault, RetryClass::Never);
        let serialized = format!("{error}");
        assert!(!serialized.contains("de"));
        assert!(!serialized.contains("ad"));
        assert!(!serialized.contains("be"));
        assert!(!serialized.contains("ef"));
        assert!(!serialized.contains("dead"));
        assert!(!serialized.contains("beef"));
    }

    #[test]
    fn secret_bytes_never_logs_key_material_when_formatted() {
        let secret = SecretBytes::new(&[0x01, 0x02, 0xff]).unwrap();
        let log_output = format!("operation: sign, key: {secret:?}");
        assert!(log_output.contains("sign"));
        assert!(log_output.contains("[REDACTED]"));
        assert!(!log_output.contains("01"));
        assert!(!log_output.contains("02"));
        assert!(!log_output.contains("ff"));
    }

    #[test]
    fn idempotency_key_constructs_valid_keys() {
        let key = IdempotencyKey::new("valid-key-123");
        assert!(key.is_ok());
        assert_eq!(key.unwrap().as_str(), "valid-key-123");
    }

    #[test]
    fn idempotency_key_refuses_empty_keys() {
        let key = IdempotencyKey::new("");
        assert!(key.is_err());
        assert_eq!(key.unwrap_err().code, SdkErrorCode::InvalidArgument);
    }

    #[test]
    fn idempotency_key_refuses_overlong_keys() {
        let overlong = "a".repeat(256);
        let key = IdempotencyKey::new(overlong);
        assert!(key.is_err());
    }

    #[test]
    fn idempotency_key_refuses_nul_containing_keys() {
        let key = IdempotencyKey::new("has\0null");
        assert!(key.is_err());
    }

    #[test]
    fn idempotency_key_never_leaks_key_material_through_error_serialization() {
        let result = IdempotencyKey::new("");
        assert!(result.is_err());
        let error = result.unwrap_err();
        let serialized = error.code.machine_code();
        assert_eq!(serialized, "invalid-argument");
    }

    #[test]
    fn protocol_amount_constructs_integer_amounts() {
        let amount = ProtocolAmount::new(12345);
        assert_eq!(amount.get(), 12345);
    }

    #[test]
    fn protocol_amount_accepts_valid_u128() {
        let amount = ProtocolAmount::new(u128::MAX);
        assert_eq!(amount.get(), u128::MAX);
    }

    #[test]
    fn protocol_amount_makes_floating_point_amounts_structurally_impossible() {
        let amount = ProtocolAmount::new(100);
        assert_eq!(amount.get(), 100u128);
    }

    #[test]
    fn error_hygiene_never_includes_request_details_in_error_messages() {
        let error = ProductionError {
            code: SdkErrorCode::TransportFailure,
            retry: RetryClass::Safe,
            protocol_result_code: None,
            retry_after_ms: None,
        };
        let message = format!("{error}");
        assert_eq!(message, "transport-failure");
    }

    #[test]
    fn error_hygiene_serializes_only_safe_machine_codes() {
        let error = ProductionError {
            code: SdkErrorCode::CapabilityRefusal,
            retry: RetryClass::Never,
            protocol_result_code: Some(4001),
            retry_after_ms: None,
        };
        assert_eq!(error.code.machine_code(), "capability-refusal");
        assert_eq!(error.protocol_result_code, Some(4001));
    }

    #[test]
    fn error_hygiene_never_includes_session_tokens_in_formatted_errors() {
        let error = ProductionError {
            code: SdkErrorCode::Deadline,
            retry: RetryClass::Safe,
            protocol_result_code: None,
            retry_after_ms: None,
        };
        let formatted = format!("{error:?}");
        assert!(formatted.contains("Deadline"));
        assert!(!formatted.contains("Bearer"));
    }
}
