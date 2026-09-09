#include "layerx/lxp_receipt.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_storage.h"

#include <string.h>

lxp_result lxp_balance_writer_guard(bool through_ledger_primitive)
{
    return through_ledger_primitive ? LXP_OK : LXP_ERR_BALANCE_BYPASS;
}

lxp_result lxp_ledger_receipt_build(lxp_receipt *receipt,
                                    const lxp_ledger_receipt_input *input)
{
    lxp_u128 expected_from;
    lxp_u128 expected_to;
    if (receipt == NULL || input == NULL || input->operation == 0U ||
        input->leg_count == 0U ||
        lxp_ct_is_zero(input->transfer_set_root, 32U))
        return LXP_ERR_NON_CANONICAL;
    if (input->leg_count == 1U) {
        if (lxp_u128_sub(input->from_balance_before, input->amount,
                         &expected_from) != LXP_OK ||
            lxp_u128_add(input->to_balance_before, input->amount,
                         &expected_to) != LXP_OK ||
            lxp_u128_cmp(expected_from, input->from_balance_after) != 0 ||
            lxp_u128_cmp(expected_to, input->to_balance_after) != 0)
            return LXP_FATAL_INVARIANT;
    }
    (void)memset(receipt, 0, sizeof(*receipt));
    receipt->protocol_version = LXP_PROTOCOL_VERSION;
    (void)memcpy(receipt->activity_id, input->transaction_id, 32U);
    receipt->global_sequence = input->global_sequence;
    receipt->result_code = LXP_OK;
    receipt->operation = input->operation;
    (void)memcpy(receipt->asset, input->asset, 32U);
    receipt->amount = input->amount;
    (void)memcpy(receipt->from, input->from, 32U);
    receipt->from_balance_before = input->from_balance_before;
    receipt->from_balance_after = input->from_balance_after;
    receipt->from_sequence = input->from_sequence;
    (void)memcpy(receipt->to, input->to, 32U);
    receipt->to_balance_before = input->to_balance_before;
    receipt->to_balance_after = input->to_balance_after;
    receipt->supply_binding_version = input->supply_binding_version;
    receipt->total_units_before = input->total_units_before;
    receipt->total_units_after = input->total_units_after;
    (void)memcpy(receipt->transfer_set_root, input->transfer_set_root, 32U);
    (void)memcpy(receipt->authorization_hash, input->authorization_hash, 32U);
    (void)memcpy(receipt->context_hash, input->context_hash, 32U);
    (void)memcpy(receipt->previous_state_root, input->previous_state_root, 32U);
    (void)memcpy(receipt->resulting_state_root, input->resulting_state_root, 32U);
    (void)memcpy(receipt->batch_id, input->batch_id, 32U);
    receipt->timestamp = input->timestamp;
    return LXP_OK;
}

lxp_result lxp_ledger_receipt_issue(lxp_receipt *receipt,
                                    const lxp_ledger_receipt_input *input,
                                    const uint8_t private_key[32],
                                    lxp_arena *arena, lxp_log *log)
{
    lxp_byte_span encoded;
    size_t mark;
    lxp_result status;
    if (private_key == NULL || arena == NULL || log == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_ledger_receipt_build(receipt, input);
    if (status != LXP_OK) return status;
    status = lxp_receipt_sign(receipt, private_key, arena);
    if (status != LXP_OK) return status;
    mark = lxp_arena_mark(arena);
    status = lxp_receipt_encode(receipt, true, arena, &encoded);
    if (status == LXP_OK && encoded.length > UINT32_MAX)
        status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK)
        status = lxp_log_append(log, LXP_LOG_RECEIPT, input->global_sequence,
                                encoded.bytes, (uint32_t)encoded.length, NULL);
    (void)lxp_arena_reset(arena, mark);
    return status;
}
