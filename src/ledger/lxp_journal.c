#include "layerx/lxp_transfer.h"

#include "layerx/lxp_hash.h"
#include "layerx/lxp_merkle.h"

#include "lxp_ledger_internal.h"

#include <string.h>

static lxp_result snapshot_add(lxp_ledger_journal *journal,
                               lx_account *account)
{
    lxp_ledger_journal_entry *entry;
    size_t i;
    for (i = 0U; i < journal->count; ++i)
        if (journal->entries[i].account == account) return LXP_OK;
    if (journal->count == LXP_MAX_TRANSFER_SET_LEGS * 2U)
        return LXP_ERR_ARENA_EXHAUSTED;
    entry = &journal->entries[journal->count++];
    entry->account = account;
    entry->balance_before = account->balance;
    (void)memcpy(entry->asset_id, account->asset_id, 32U);
    entry->has_asset = account->has_asset;
    entry->next_sequence = account->next_sequence;
    return LXP_OK;
}

lxp_result lxp_journal_open(lxp_transfer_leg *legs, size_t leg_count,
                            lxp_ledger_journal *journal)
{
    size_t i;
    lxp_result status;
    if (legs == NULL || journal == NULL || leg_count == 0U ||
        leg_count > LXP_MAX_TRANSFER_SET_LEGS) return LXP_ERR_TOO_MANY_LEGS;
    (void)memset(journal, 0, sizeof(*journal));
    journal->open = true;
    for (i = 0U; i < leg_count; ++i) {
        if (legs[i].from == NULL || legs[i].to == NULL) {
            journal->open = false;
            return LXP_ERR_NON_CANONICAL;
        }
        status = snapshot_add(journal, legs[i].from);
        if (status == LXP_OK) status = snapshot_add(journal, legs[i].to);
        if (status != LXP_OK) { journal->open = false; return status; }
    }
    return LXP_OK;
}

lxp_result lxp_journal_commit(lxp_ledger_journal *journal)
{
    if (journal == NULL || !journal->open) return LXP_ERR_NON_CANONICAL;
    journal->open = false;
    return LXP_OK;
}

lxp_result lxp_journal_rollback(lxp_ledger_journal *journal)
{
    return lxp_balance_restore_snapshot(journal);
}

static lxp_result accumulator_add(lxp_u256 *value, lxp_u128 amount)
{
    lxp_u256 addend = { { amount.lo, amount.hi, 0U, 0U } };
    lxp_u256 next;
    lxp_result status = lxp_u256_add(*value, addend, &next);
    if (status == LXP_OK) *value = next;
    return status;
}

static lxp_result source_authorities_check(
    const lxp_transfer_leg *legs, size_t leg_count,
    const lxp_transfer_context *context)
{
    size_t i;
    size_t j;
    if (context->source_authority_count == 0U)
        return context->source_authorities == NULL ? LXP_OK :
                                                    LXP_ERR_NON_CANONICAL;
    if (context->source_authorities == NULL ||
        context->source_authority_count > leg_count ||
        context->source_authority_count > LXP_MAX_TRANSFER_SET_LEGS)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < context->source_authority_count; ++i) {
        const lxp_transfer_source_authority *authority =
            &context->source_authorities[i];
        bool used = false;
        if (authority->debit_authority_kind < LXP_AUTH_OWNER ||
            authority->debit_authority_kind >
                LXP_AUTH_PROGRAM_SPEND)
            return LXP_ERR_NON_CANONICAL;
        for (j = 0U; j < i; ++j)
            if (memcmp(authority->authorized_from,
                       context->source_authorities[j].authorized_from,
                       32U) == 0)
                return LXP_ERR_NON_CANONICAL;
        for (j = 0U; j < leg_count; ++j)
            if (legs[j].from != NULL &&
                memcmp(authority->authorized_from, legs[j].from->id, 32U) == 0)
                used = true;
        if (!used) return LXP_ERR_UNAUTHORIZED_DEBIT;
    }
    for (i = 0U; i < leg_count; ++i) {
        size_t matches = 0U;
        if (legs[i].from == NULL) return LXP_ERR_NON_CANONICAL;
        for (j = 0U; j < context->source_authority_count; ++j)
            if (memcmp(context->source_authorities[j].authorized_from,
                       legs[i].from->id, 32U) == 0)
                ++matches;
        if (matches != 1U) return LXP_ERR_UNAUTHORIZED_DEBIT;
    }
    return LXP_OK;
}

static lxp_result program_spend_refuse(
    const lxp_transfer_leg *legs, size_t leg_count,
    lxp_transfer_context *context, const uint8_t transfer_set_root[32])
{
    size_t index;
    size_t program_spends = 0U;
    if (legs == NULL || context == NULL || transfer_set_root == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < context->source_authority_count; ++index) {
        const lxp_transfer_source_authority *authority =
            &context->source_authorities[index];
        if (authority->debit_authority_kind != LXP_AUTH_PROGRAM_SPEND)
            continue;
        ++program_spends;
    }
    if (program_spends == 0U)
        return context->program_spend_token == 0U ?
                   LXP_OK : LXP_ERR_NON_CANONICAL;
    (void)leg_count;
    (void)transfer_set_root;
    return LXP_ERR_UNAUTHORIZED_DEBIT;
}

lxp_result lxp_conservation_check(const lxp_transfer_leg *legs,
                                  size_t leg_count)
{
    size_t i;
    size_t j;
    if (legs == NULL || leg_count == 0U ||
        leg_count > LXP_MAX_TRANSFER_SET_LEGS) return LXP_ERR_TOO_MANY_LEGS;
    for (i = 0U; i < leg_count; ++i) {
        lxp_u256 debits = { { 0U, 0U, 0U, 0U } };
        lxp_u256 credits = { { 0U, 0U, 0U, 0U } };
        if (legs[i].from == NULL || legs[i].to == NULL ||
            legs[i].supply_mode > LXP_TRANSFER_DEBIT_ONLY)
            return LXP_ERR_NON_CANONICAL;
        for (j = 0U; j < leg_count; ++j) {
            lxp_result status;
            if (memcmp(legs[i].asset_id, legs[j].asset_id, 32U) != 0) continue;
            if (legs[j].supply_mode != LXP_TRANSFER_CREDIT_ONLY) {
                status = accumulator_add(&debits, legs[j].amount);
                if (status != LXP_OK) return status;
            }
            if (legs[j].supply_mode != LXP_TRANSFER_DEBIT_ONLY) {
                status = accumulator_add(&credits, legs[j].amount);
                if (status != LXP_OK) return status;
            }
        }
        if (memcmp(&debits, &credits, sizeof(debits)) != 0)
            return LXP_ERR_CONSERVATION;
    }
    return LXP_OK;
}

lxp_result lxp_transfer_set_root(const lxp_transfer_leg *legs,
                                 size_t leg_count, uint8_t root[32])
{
    uint8_t hashes[LXP_MAX_TRANSFER_SET_LEGS][32];
    uint8_t encoded[115];
    size_t i;
    size_t count;
    lxp_result status;
    if (legs == NULL || root == NULL || leg_count == 0U ||
        leg_count > LXP_MAX_TRANSFER_SET_LEGS) return LXP_ERR_TOO_MANY_LEGS;
    count = leg_count;
    for (i = 0U; i < count; ++i) {
        if (legs[i].from == NULL || legs[i].to == NULL ||
            legs[i].supply_mode > LXP_TRANSFER_DEBIT_ONLY)
            return LXP_ERR_NON_CANONICAL;
        encoded[0] = legs[i].supply_mode;
        (void)memcpy(encoded + 1U, legs[i].from->id, 32U);
        (void)memcpy(encoded + 33U, legs[i].to->id, 32U);
        (void)memcpy(encoded + 65U, legs[i].asset_id, 32U);
        status = lxp_u128_to_be(legs[i].amount, encoded + 97U);
        encoded[113] = (uint8_t)(legs[i].reason >> 8U);
        encoded[114] = (uint8_t)legs[i].reason;
        if (status == LXP_OK)
            status = lxp_merkle_leaf_hash(encoded, sizeof(encoded), hashes[i]);
        if (status != LXP_OK) return status;
    }
    while (count > 1U) {
        size_t next_count = (count + 1U) / 2U;
        for (i = 0U; i < next_count; ++i) {
            size_t right = i * 2U + 1U;
            if (right >= count) right = i * 2U;
            status = lxp_merkle_node_hash(hashes[i * 2U], hashes[right],
                                          hashes[i]);
            if (status != LXP_OK) return status;
        }
        count = next_count;
    }
    (void)memcpy(root, hashes[0], 32U);
    return LXP_OK;
}

lxp_result lxp_apply_transfer_set(lxp_transfer_leg *legs, size_t leg_count,
                                  lxp_transfer_context *context,
                                  lxp_transfer_set_result *result)
{
    lxp_ledger_journal journal;
    lxp_transfer_leg compact[LXP_MAX_TRANSFER_SET_LEGS];
    size_t original_index[LXP_MAX_TRANSFER_SET_LEGS];
    size_t compact_count = 0U;
    size_t i;
    lxp_result status;
    if (legs == NULL || context == NULL || result == NULL || leg_count == 0U ||
        leg_count > LXP_MAX_TRANSFER_SET_LEGS) return LXP_ERR_TOO_MANY_LEGS;
    (void)memset(result, 0, sizeof(*result));
    for (i = 0U; i < leg_count; ++i) {
        if (lxp_u128_is_zero(legs[i].amount)) continue;
        compact[compact_count] = legs[i];
        original_index[compact_count++] = i;
    }
    if (compact_count == 0U) return LXP_ERR_ZERO_AMOUNT;
    status = source_authorities_check(compact, compact_count, context);
    if (status != LXP_OK) return status;
    status = lxp_conservation_check(compact, compact_count);
    if (status != LXP_OK) return status;
    status = lxp_transfer_set_root(compact, compact_count,
                                   result->transfer_set_root);
    if (status != LXP_OK) return status;
    status = program_spend_refuse(compact, compact_count, context,
                                  result->transfer_set_root);
    if (status != LXP_OK) return status;
    status = lxp_journal_open(compact, compact_count, &journal);
    if (status != LXP_OK) return status;
    for (i = 0U; i < compact_count; ++i) {
        status = lxp_precondition_check(&compact[i], 1U, context);
        if (status == LXP_OK)
            status = lxp_balance_apply_leg(&compact[i], &result->legs[i]);
        if (status == LXP_OK && context->inject_failure &&
            i == context->failure_after_leg) status = LXP_ERR_IO;
        if (status != LXP_OK) {
            (void)lxp_journal_rollback(&journal);
            result->failed_leg = original_index[i];
            result->failure = status;
            result->leg_count = i;
            return status;
        }
    }
    if (context->debit_authority_kind !=
            LXP_AUTH_OCCUPANCY_RESPONSIBILITY)
        ++(context->sequence_account != NULL ? context->sequence_account :
                                              compact[0].from)->next_sequence;
    status = lxp_journal_commit(&journal);
    if (status != LXP_OK) return status;
    result->leg_count = compact_count;
    result->receipt_emitted = true;
    return LXP_OK;
}
