use layerx_crypto::send::SendDebit;
use layerx_wire::{encode::Encoder, WireError};

/// # Errors
/// Refuses a non-owner or conditional debit and retains every original encoder bound.
pub fn owner_send_authorization(debit: &SendDebit) -> Result<Vec<u8>, WireError> {
    if debit.authorization_kind != 1 || !debit.conditions.is_empty() {
        return Err(WireError {
            result: layerx_types::result::KnownResult::NonCanonical.into(),
            offset: 0,
        });
    }
    let mut message = Encoder::new(512);
    message.u16(0x5301)?;
    for value in [debit.from, debit.to, debit.asset] {
        message.fixed(&value)?;
    }
    message.u128(debit.amount)?;
    message.u64(debit.source_sequence)?;
    message.fixed(&debit.idempotency_key)?;
    message.u64(debit.expires_at)?;
    message.fixed(&debit.context_hash)?;
    message.u8(0)?;
    message.u8(1)?;
    message.fixed(&debit.from)?;
    message.fixed(&debit.context_hash)?;
    message.u32(debit.network_id)?;
    message.u16(debit.protocol_version)?;
    Ok(message.finish())
}
