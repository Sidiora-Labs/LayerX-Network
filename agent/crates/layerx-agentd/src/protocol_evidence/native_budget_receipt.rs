use crate::budget::NativeBudgetError as Error;
use layerx_wire::activity::Activity;
use layerx_wire::receipt::ProtocolReceipt;

pub(super) fn event(receipt: &ProtocolReceipt, kind: u16, body: &[u8]) -> Result<(), Error> {
    let [effect] = receipt.effects() else {
        return Err(Error::Receipt);
    };
    if effect.module_id() != 3
        || effect.ordinal() != 0
        || effect.kind() != 3
        || effect.event_type() != kind
        || effect.monetary()
        || effect.transfer_set_root() != [0; 32]
        || effect.body() != body
    {
        return Err(Error::Receipt);
    }
    Ok(())
}

pub(super) fn spend(receipt: &ProtocolReceipt, activity: &Activity) -> Result<(), Error> {
    if receipt.protocol_version() != 3
        || receipt.module_id() != 3
        || receipt.module_version() != 1
        || receipt.parameter_version() == 0
        || receipt.operation() != 0
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
        || receipt.program_outcome().is_some()
        || receipt.total_units().is_some()
        || receipt.fee_charged() > activity.fee_limit()
    {
        return Err(Error::Receipt);
    }
    if receipt.result_code() == 0 {
        event(
            receipt,
            6,
            activity.payload().get(2..82).ok_or(Error::Activity)?,
        )
    } else if receipt.effects().is_empty() {
        Ok(())
    } else {
        Err(Error::Receipt)
    }
}
