#include "layerx/lx_asset.h"

#include <string.h>

static lxp_result add(lxp_u128 *sum, lxp_u128 value)
{
    lxp_u128 next;
    lxp_result status = lxp_u128_add(*sum, value, &next);
    if (status == LXP_OK) *sum = next;
    return status == LXP_OK ? LXP_OK : LXP_FATAL_SUPPLY_MISMATCH;
}

static lxp_u128 *bucket(lx_asset_reserve_report_record *report,
                        lx_account_kind kind)
{
    switch (kind) {
    case LX_ACCOUNT_AGENT_MAIN: return &report->agent_main;
    case LX_ACCOUNT_AGENT_ESCROW: return &report->escrow;
    case LX_ACCOUNT_AGENT_BUDGET: return &report->budget;
    case LX_ACCOUNT_AGENT_STREAM: return &report->stream;
    case LX_ACCOUNT_AGENT_MARGIN: return &report->margin;
    case LX_ACCOUNT_SYSTEM_LIQUIDITY:
    case LX_ACCOUNT_SYSTEM_FUNDING_LONG:
    case LX_ACCOUNT_SYSTEM_FUNDING_SHORT: return &report->liquidity;
    case LX_ACCOUNT_SYSTEM_INSURANCE: return &report->insurance;
    case LX_ACCOUNT_SYSTEM_FEES: return &report->fees;
    case LX_ACCOUNT_SYSTEM_PAXEER_WITHDRAWALS: return &report->withdrawals;
    case LX_ACCOUNT_SYSTEM_PAXEER_RESERVE: return &report->reserve;
    default: return &report->other_system;
    }
}

lxp_result lx_asset_reserve_report(
    const lx_account_registry *accounts,
    const lx_asset_custody_attestation *attestation,
    lx_asset_reserve_report_record *report)
{
    size_t i;
    lxp_result status = LXP_OK;
    if (accounts == NULL || attestation == NULL || report == NULL ||
        accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY ||
        !attestation->finalized) return LXP_FATAL_SUPPLY_MISMATCH;
    (void)memset(report, 0, sizeof(*report));
    (void)memcpy(report->asset_id, attestation->asset_id, 32U);
    for (i = 0U; status == LXP_OK && i < accounts->count; ++i) {
        const lx_account *account = &accounts->accounts[i];
        if (!account->has_asset ||
            memcmp(account->asset_id, attestation->asset_id, 32U) != 0) continue;
        if (account->kind == LX_ACCOUNT_AGENT_ESCROW) {
            size_t line = report->escrow_line_count;
            if (line == LX_ASSET_RESERVE_LINE_CAPACITY)
                return LXP_FATAL_SUPPLY_MISMATCH;
            (void)memcpy(report->escrow_lines[line].account_id,
                         account->id, 32U);
            report->escrow_lines[line].kind = account->kind;
            report->escrow_lines[line].balance = account->balance;
            ++report->escrow_line_count;
        }
        status = add(bucket(report, account->kind), account->balance);
        if (status == LXP_OK) status = add(&report->raw_total, account->balance);
        if (status == LXP_OK &&
            account->kind != LX_ACCOUNT_SYSTEM_PAXEER_RESERVE)
            status = add(&report->circulating, account->balance);
    }
    if (status == LXP_OK)
        status = lxp_u128_sub(report->raw_total, attestation->settled_out,
                              &report->effective_total);
    if (status == LXP_OK)
        status = lxp_u128_sub(attestation->custody_amount,
                              attestation->settled_out,
                              &report->expected_backing);
    return status == LXP_OK ? LXP_OK : LXP_FATAL_SUPPLY_MISMATCH;
}

lxp_result lx_asset_reserve_reconcile(
    const lx_account_registry *accounts,
    const lx_asset_custody_attestation *attestation,
    lx_asset_reserve_report_record *report)
{
    lxp_result status = lx_asset_reserve_report(accounts, attestation, report);
    if (status != LXP_OK ||
        lxp_u128_cmp(report->circulating, report->expected_backing) > 0 ||
        lxp_u128_cmp(report->effective_total, report->expected_backing) != 0)
        return LXP_FATAL_SUPPLY_MISMATCH;
    return LXP_OK;
}

lxp_result lx_asset_reserve_report_encode(
    const lx_asset_reserve_report_record *report,
    uint8_t *bytes, size_t capacity, size_t *length)
{
    const lxp_u128 *fields[15];
    size_t cursor = 0U;
    size_t i;
    if (report == NULL || bytes == NULL || length == NULL ||
        report->escrow_line_count > LX_ASSET_RESERVE_LINE_CAPACITY ||
        capacity < 274U + report->escrow_line_count * 48U)
        return LXP_ERR_LENGTH_LIMIT;
    fields[0] = &report->agent_main;
    fields[1] = &report->escrow;
    fields[2] = &report->budget;
    fields[3] = &report->stream;
    fields[4] = &report->margin;
    fields[5] = &report->liquidity;
    fields[6] = &report->insurance;
    fields[7] = &report->fees;
    fields[8] = &report->withdrawals;
    fields[9] = &report->other_system;
    fields[10] = &report->reserve;
    fields[11] = &report->raw_total;
    fields[12] = &report->circulating;
    fields[13] = &report->effective_total;
    fields[14] = &report->expected_backing;
    (void)memcpy(bytes, report->asset_id, 32U);
    cursor = 32U;
    for (i = 0U; i < 15U; ++i) {
        lxp_result status = lxp_u128_to_be(*fields[i], bytes + cursor);
        if (status != LXP_OK) return status;
        cursor += 16U;
    }
    bytes[cursor++] = (uint8_t)(report->escrow_line_count >> 8U);
    bytes[cursor++] = (uint8_t)report->escrow_line_count;
    for (i = 0U; i < report->escrow_line_count; ++i) {
        lxp_result status;
        (void)memcpy(bytes + cursor, report->escrow_lines[i].account_id, 32U);
        cursor += 32U;
        status = lxp_u128_to_be(report->escrow_lines[i].balance,
                                bytes + cursor);
        if (status != LXP_OK) return status;
        cursor += 16U;
    }
    *length = cursor;
    return LXP_OK;
}

lxp_result lx_asset_supply_check(
    const lx_asset_registry *assets, const lx_account_registry *accounts,
    const lx_asset_custody_attestation *attestations,
    size_t attestation_count)
{
    size_t i;
    if (assets == NULL || accounts == NULL || attestations == NULL ||
        assets->count > LX_ASSET_REGISTRY_CAPACITY ||
        accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY ||
        attestation_count > LX_ASSET_REGISTRY_CAPACITY)
        return LXP_FATAL_SUPPLY_MISMATCH;
    for (i = 0U; i < assets->count; ++i) {
        lx_asset_reserve_report_record report;
        size_t j;
        for (j = 0U; j < attestation_count; ++j)
            if (memcmp(assets->assets[i].asset_id,
                       attestations[j].asset_id, 32U) == 0) break;
        if (j == attestation_count ||
            lx_asset_reserve_reconcile(accounts, &attestations[j], &report) !=
                LXP_OK ||
            lxp_u128_cmp(assets->assets[i].total_units, report.raw_total) != 0)
            return LXP_FATAL_SUPPLY_MISMATCH;
    }
    return LXP_OK;
}
