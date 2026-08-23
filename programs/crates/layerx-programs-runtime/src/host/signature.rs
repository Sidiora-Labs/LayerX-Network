//! Signature verification and recovery host-function registration.

use wasmi::{Caller, Linker};

use crate::crypto::signature::{
    recover_secp256k1, verify_ed25519, verify_secp256k1, SignatureAlgorithm, SignatureRefusal,
    SECP256K1_UNCOMPRESSED_PUBLIC_KEY_BYTES,
};
use crate::execute::ExecutionFault;

use super::memory::{read_guest, write_guest};
use super::{linker_fault, RuntimeState, ABI_MODULE, STATUS_BOUNDS, STATUS_INVALID, STATUS_METER};

/// Status code returned when signature verification fails.
const STATUS_VERIFY_FAILED: i32 = -6;

pub(super) fn register(linker: &mut Linker<RuntimeState>) -> Result<(), ExecutionFault> {
    linker
        .func_wrap(
            ABI_MODULE,
            "signature_verify",
            |mut caller: Caller<'_, RuntimeState>,
             algorithm: i32,
             message_pointer: i32,
             message_length: i32,
             public_key_pointer: i32,
             public_key_length: i32,
             signature_pointer: i32,
             signature_length: i32|
             -> i32 {
                let algorithm = match SignatureAlgorithm::decode(algorithm as u32) {
                    Ok(algorithm) => algorithm,
                    Err(_) => return STATUS_INVALID,
                };

                if let Err(status) = charge_signature_fuel(&mut caller, algorithm) {
                    return status;
                }

                let message = match read_guest(&caller, message_pointer, message_length, 1024) {
                    Ok(message) => message,
                    Err(status) => return status,
                };

                let public_key =
                    match read_guest(&caller, public_key_pointer, public_key_length, 128) {
                        Ok(public_key) => public_key,
                        Err(status) => return status,
                    };

                let signature =
                    match read_guest(&caller, signature_pointer, signature_length, 128) {
                        Ok(signature) => signature,
                        Err(status) => return status,
                    };

                let result = match algorithm {
                    SignatureAlgorithm::Ed25519 => {
                        verify_ed25519(&message, &public_key, &signature)
                    }
                    SignatureAlgorithm::Secp256k1Verify => {
                        verify_secp256k1(&message, &public_key, &signature)
                    }
                    SignatureAlgorithm::Secp256k1Recover => return STATUS_INVALID,
                };

                match result {
                    Ok(()) => 0,
                    Err(SignatureRefusal::InvalidAlgorithm) => STATUS_INVALID,
                    Err(SignatureRefusal::InvalidMessageLength) => STATUS_INVALID,
                    Err(SignatureRefusal::MalformedPublicKey) => STATUS_INVALID,
                    Err(SignatureRefusal::MalformedSignature) => STATUS_INVALID,
                    Err(SignatureRefusal::VerificationFailed) => STATUS_VERIFY_FAILED,
                    Err(SignatureRefusal::InvalidRecoveryId)
                    | Err(SignatureRefusal::RecoveryFailed) => STATUS_INVALID,
                }
            },
        )
        .map_err(|error| linker_fault(&error))?;

    linker
        .func_wrap(
            ABI_MODULE,
            "signature_recover",
            |mut caller: Caller<'_, RuntimeState>,
             message_digest_pointer: i32,
             message_digest_length: i32,
             signature_pointer: i32,
             signature_length: i32,
             recovery_id: i32,
             output_pointer: i32,
             output_capacity: i32|
             -> i32 {
                if let Err(status) =
                    charge_signature_fuel(&mut caller, SignatureAlgorithm::Secp256k1Recover)
                {
                    return status;
                }

                let message_digest =
                    match read_guest(&caller, message_digest_pointer, message_digest_length, 64) {
                        Ok(digest) => digest,
                        Err(status) => return status,
                    };

                let signature =
                    match read_guest(&caller, signature_pointer, signature_length, 128) {
                        Ok(signature) => signature,
                        Err(status) => return status,
                    };

                if recovery_id < 0 || recovery_id > 3 {
                    return STATUS_INVALID;
                }

                let public_key =
                    match recover_secp256k1(&message_digest, &signature, recovery_id as u8) {
                        Ok(public_key) => public_key,
                        Err(SignatureRefusal::InvalidMessageLength) => return STATUS_INVALID,
                        Err(SignatureRefusal::MalformedSignature) => return STATUS_INVALID,
                        Err(SignatureRefusal::InvalidRecoveryId) => return STATUS_INVALID,
                        Err(SignatureRefusal::RecoveryFailed) => return STATUS_VERIFY_FAILED,
                        Err(_) => return STATUS_INVALID,
                    };

                if output_capacity < 0
                    || (output_capacity as usize) < SECP256K1_UNCOMPRESSED_PUBLIC_KEY_BYTES
                {
                    return STATUS_BOUNDS;
                }

                if let Err(status) = write_guest(&mut caller, output_pointer, &public_key) {
                    return status;
                }

                i32::try_from(SECP256K1_UNCOMPRESSED_PUBLIC_KEY_BYTES).unwrap_or(STATUS_BOUNDS)
            },
        )
        .map_err(|error| linker_fault(&error))?;

    Ok(())
}

fn charge_signature_fuel(
    caller: &mut Caller<'_, RuntimeState>,
    algorithm: SignatureAlgorithm,
) -> Result<(), i32> {
    let fuel = algorithm.fuel_coefficient();
    caller
        .data_mut()
        .meter_mut()
        .charge_cpu(fuel)
        .map_err(|_| STATUS_METER)
}
