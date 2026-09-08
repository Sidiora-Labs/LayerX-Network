use crate::evm_types::SendPlanAuthorization;
use crate::wire::{Error, Result};
use sha2::{Digest, Sha256};

pub(crate) fn digest(
    value: &SendPlanAuthorization,
    binding: [u8; 32],
    network: u32,
    protocol: u16,
    now: u64,
) -> Result<[u8; 32]> {
    if value.binding_digest != binding
        || value.network != network
        || value.protocol != protocol
        || value.plan_id == [0; 32]
        || value.action_key == [0; 32]
        || value.principal.is_empty()
        || value.principal.len() > 128
        || value.tenant.is_empty()
        || value.tenant.len() > 128
        || value.from == [0; 32]
        || value.to == [0; 32]
        || value.asset == [0; 32]
        || value.amount == 0
        || value.idempotency_key == [0; 32]
        || value.context == [0; 32]
        || value.expires_at == 0
        || value.not_before > now
        || value.not_after < now
        || value.not_after <= value.not_before
    {
        return Err(Error::Refused);
    }
    let mut message = Vec::new();
    message.extend(0x5301_u16.to_be_bytes());
    message.extend(value.from);
    message.extend(value.to);
    message.extend(value.asset);
    message.extend(value.amount.to_be_bytes());
    message.extend(value.sequence.to_be_bytes());
    message.extend(value.idempotency_key);
    message.extend(value.expires_at.to_be_bytes());
    message.extend(value.context);
    message.extend([0, 1]);
    message.extend(value.from);
    message.extend(value.context);
    message.extend(value.network.to_be_bytes());
    message.extend(value.protocol.to_be_bytes());
    let mut hash = Sha256::new();
    hash.update(b"LXP/v1/signature-preimage\0");
    hash.update(message);
    Ok(hash.finalize().into())
}
