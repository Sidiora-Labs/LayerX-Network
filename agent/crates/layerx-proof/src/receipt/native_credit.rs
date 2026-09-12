use super::{
    encode_unsigned, receipt_digest, verify_sequencer_signature, AuthorizedBatch, Evidence,
    ReceiptCheck, VerificationFailure, VerifiedReceipt,
};
use layerx_wire::receipt::ProtocolReceipt;

fn refused() -> VerificationFailure {
    VerificationFailure::at(ReceiptCheck::ReceiptShape)
}

pub(super) fn verify(
    bytes: &[u8],
    authorized: &AuthorizedBatch,
) -> Result<VerifiedReceipt, VerificationFailure> {
    let receipt = verify_sequencer_signature(bytes, authorized.sequencer_public_key())?;
    let protocol = receipt.protocol().ok_or_else(refused)?;
    verify_shape(protocol)?;
    if protocol.batch_id() != authorized.batch_id() {
        return Err(VerificationFailure::at(ReceiptCheck::BatchId));
    }
    if authorized.asset() != [0; 32] {
        return Err(VerificationFailure::at(ReceiptCheck::Asset));
    }
    if protocol.previous_state_root() != authorized.previous_state_root() {
        return Err(VerificationFailure::at(ReceiptCheck::PreviousStateRoot));
    }
    if protocol.resulting_state_root() != authorized.resulting_state_root() {
        return Err(VerificationFailure::at(ReceiptCheck::ResultingStateRoot));
    }
    if protocol.result_code() == 0 {
        verify_effects(protocol)?;
    } else if !protocol.effects().is_empty() {
        return Err(refused());
    }
    let unsigned = encode_unsigned(&receipt).map_err(|_| refused())?;
    let digest = receipt_digest(&unsigned).map_err(|_| refused())?;
    Ok(VerifiedReceipt {
        receipt,
        canonical_bytes: bytes.to_vec(),
        evidence: Evidence::sequencer(digest),
    })
}

fn verify_shape(receipt: &ProtocolReceipt) -> Result<(), VerificationFailure> {
    if receipt.protocol_version() != 3
        || receipt.module_id() != 8
        || receipt.module_version() != 1
        || receipt.operation() != 0
        || receipt.amount() != 0
        || receipt.asset() != [0; 32]
        || receipt.from() != [0; 32]
        || receipt.to() != [0; 32]
        || receipt.debit_sequence() != 0
        || receipt.debit_balance_before() != 0
        || receipt.debit_balance_after() != 0
        || receipt.credit_balance_before() != 0
        || receipt.credit_balance_after() != 0
        || receipt.authorization_hash() != [0; 32]
        || receipt.context_hash() != [0; 32]
        || receipt.transfer_set_root() != [0; 32]
    {
        return Err(refused());
    }
    Ok(())
}

fn amount(bytes: &[u8], offset: usize) -> Result<u128, VerificationFailure> {
    Ok(u128::from_be_bytes(
        bytes
            .get(offset..offset + 16)
            .ok_or_else(refused)?
            .try_into()
            .map_err(|_| refused())?,
    ))
}

fn sequence(bytes: &[u8], offset: usize) -> Result<u64, VerificationFailure> {
    Ok(u64::from_be_bytes(
        bytes
            .get(offset..offset + 8)
            .ok_or_else(refused)?
            .try_into()
            .map_err(|_| refused())?,
    ))
}

fn verify_effects(receipt: &ProtocolReceipt) -> Result<(), VerificationFailure> {
    let [transfer, credit, balances] = receipt.effects() else {
        return Err(refused());
    };
    if transfer.module_id() != 8
        || transfer.kind() != 2
        || !transfer.monetary()
        || transfer.event_type() != 0
        || !transfer.body().is_empty()
        || transfer.transfer_set_root() == [0; 32]
        || credit.module_id() != 8
        || credit.kind() != 3
        || credit.monetary()
        || credit.event_type() != 1
        || credit.body().len() != 208
        || credit.transfer_set_root() != [0; 32]
        || balances.module_id() != 8
        || balances.kind() != 3
        || balances.monetary()
        || balances.event_type() != 2
        || balances.body().len() != 112
        || balances.transfer_set_root() != [0; 32]
    {
        return Err(refused());
    }
    let value = amount(credit.body(), 96)?;
    if value == 0
        || [0..32, 32..64, 64..96, 112..144, 144..176]
            .into_iter()
            .any(|range| credit.body()[range].iter().all(|byte| *byte == 0))
        || balances.body()[..32].iter().all(|byte| *byte == 0)
        || amount(credit.body(), 176)?.checked_add(value) != Some(amount(credit.body(), 192)?)
        || amount(balances.body(), 32)? != amount(balances.body(), 48)?
        || amount(balances.body(), 64)?.checked_add(value) != Some(amount(balances.body(), 80)?)
        || sequence(balances.body(), 96)?.checked_add(1) != Some(sequence(balances.body(), 104)?)
    {
        return Err(refused());
    }
    Ok(())
}
