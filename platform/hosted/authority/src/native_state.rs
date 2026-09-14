use layerx_proof::receipt::{verify_sequencer_signature, AuthorizedBatch, ReceiptCheck};
use layerx_wire::receipt::{Effect, ProtocolReceipt};

use crate::EvidenceRefusal;

pub(super) fn selected(receipt: &ProtocolReceipt) -> bool {
    receipt.operation() == 0
        && ((2..=6).contains(&receipt.module_id())
            || (receipt.module_id() == 7
                && receipt
                    .effects()
                    .iter()
                    .any(|effect| effect.event_type() == 0x7109)))
}

fn refuse(check: ReceiptCheck) -> EvidenceRefusal {
    EvidenceRefusal::Receipt(check)
}

fn projection(receipt: &ProtocolReceipt) -> Result<(), EvidenceRefusal> {
    if receipt.protocol_version() != 3 {
        return Err(refuse(ReceiptCheck::ProtocolVersion));
    }
    if !(2..=7).contains(&receipt.module_id()) || receipt.module_version() != 1 {
        return Err(refuse(ReceiptCheck::Module));
    }
    if receipt.operation() != 0
        || receipt.asset() != [0; 32]
        || receipt.amount() != 0
        || receipt.from() != [0; 32]
        || receipt.to() != [0; 32]
        || receipt.debit_sequence() != 0
        || receipt.debit_balance_before() != 0
        || receipt.debit_balance_after() != 0
        || receipt.credit_balance_before() != 0
        || receipt.credit_balance_after() != 0
        || receipt.transfer_set_root() != [0; 32]
        || receipt.authorization_hash() != [0; 32]
        || receipt.context_hash() != [0; 32]
        || receipt.total_units().is_some()
        || receipt.program_outcome().is_some()
        || receipt.global_sequence() == 0
        || receipt.timestamp() == 0
        || receipt.activity_root() == [0; 32]
        || receipt.resulting_state_root() == [0; 32]
    {
        return Err(refuse(ReceiptCheck::ReceiptShape));
    }
    Ok(())
}

fn event_length(module: u16, event: u16) -> Option<usize> {
    match (module, event) {
        (2, 1..=7) => Some(67),
        (3, 1 | 6) => Some(80),
        (3, 2) => Some(48),
        (3, 3 | 7) => Some(56),
        (3, 4 | 5) => Some(64),
        (4, 1) => Some(177),
        (4, 2) => Some(49),
        (4, 3) => Some(72),
        (4, 4) => Some(65),
        (4, 5 | 6) => Some(56),
        (4, 7) | (6, 0x0601 | 0x0605) => Some(64),
        (5, 1..=13) => Some(73),
        (5, 14) => Some(50),
        (6, 0x0602 | 0x0606 | 0x0607) => Some(65),
        (6, 0x0603) => Some(88),
        (6, 0x0604 | 0x0608 | 0x060b) => Some(81),
        (6, 0x0609) => Some(98),
        (6, 0x060a) => Some(80),
        (7, 0x7109) => Some(32),
        _ => None,
    }
}

fn event(receipt: &ProtocolReceipt, effect: &Effect) -> Result<(), EvidenceRefusal> {
    if effect.module_id() != receipt.module_id()
        || effect.ordinal() != 0
        || effect.kind() != 3
        || effect.monetary()
        || effect.transfer_set_root() != [0; 32]
        || event_length(receipt.module_id(), effect.event_type()) != Some(effect.body().len())
        || effect.body().get(..32).is_none_or(|id| id == [0; 32])
    {
        return Err(refuse(ReceiptCheck::ReceiptShape));
    }
    if receipt.module_id() == 2
        && (effect.body()[32..34] != effect.event_type().to_be_bytes()
            || !(1..=7).contains(&effect.body()[34]))
    {
        return Err(refuse(ReceiptCheck::ReceiptShape));
    }
    Ok(())
}

pub(super) fn verify(bytes: &[u8], authorised: &AuthorizedBatch) -> Result<(), EvidenceRefusal> {
    let receipt = verify_sequencer_signature(bytes, authorised.sequencer_public_key())
        .map_err(|failure| refuse(failure.check))?;
    let receipt = receipt.protocol().ok_or(EvidenceRefusal::ReceiptShape)?;
    projection(receipt)?;
    if receipt.batch_id() != authorised.batch_id() {
        return Err(refuse(ReceiptCheck::BatchId));
    }
    if authorised.asset() != receipt.asset() {
        return Err(refuse(ReceiptCheck::Asset));
    }
    if receipt.previous_state_root() != authorised.previous_state_root() {
        return Err(refuse(ReceiptCheck::PreviousStateRoot));
    }
    if receipt.resulting_state_root() != authorised.resulting_state_root() {
        return Err(refuse(ReceiptCheck::ResultingStateRoot));
    }
    if receipt.result_code() < 0 {
        if !receipt.effects().is_empty() {
            return Err(refuse(ReceiptCheck::ReceiptShape));
        }
        return Ok(());
    }
    if receipt.result_code() != 0 {
        return Err(refuse(ReceiptCheck::ResultCode));
    }
    let [effect] = receipt.effects() else {
        return Err(refuse(ReceiptCheck::ReceiptShape));
    };
    event(receipt, effect)
}
