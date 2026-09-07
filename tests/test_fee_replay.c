#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_fee.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_storage.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

enum { TEST_RECORDS = 4, WIRE_BYTES = 56, ENTRY_BYTES = 100 };

typedef struct fee_wire {
    uint64_t epoch;
    uint32_t activity_type;
    lxp_fee_meter meter;
    lxp_u128 fee_limit;
    lxp_result execution_result;
} fee_wire;

typedef struct fee_fixture {
    lxp_param_table parameters;
    lx_account_registry registry;
    lx_account *actor;
    lx_account *treasury;
    lxp_transfer_asset_state asset;
    lxp_transfer_context transfer_context;
    uint8_t asset_id[32];
    uint8_t previous_root[32];
} fee_fixture;

typedef struct replay_context {
    fee_fixture *fixture;
    lxp_fee_replay_entry *replayed;
    lxp_fee_replay_entry *logged;
    size_t activity_count;
    size_t receipt_count;
} replay_context;

static void put_u32(uint8_t out[4], uint32_t value)
{
    out[0] = (uint8_t)(value >> 24U);
    out[1] = (uint8_t)(value >> 16U);
    out[2] = (uint8_t)(value >> 8U);
    out[3] = (uint8_t)value;
}

static void put_u64(uint8_t out[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        out[7U - i] = (uint8_t)(value >> (i * 8U));
}

static uint32_t get_u32(const uint8_t in[4])
{
    return ((uint32_t)in[0] << 24U) | ((uint32_t)in[1] << 16U) |
           ((uint32_t)in[2] << 8U) | in[3];
}

static uint64_t get_u64(const uint8_t in[8])
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | in[i];
    return value;
}

static void wire_encode(const fee_wire *wire, uint8_t out[WIRE_BYTES])
{
    uint32_t result_bits;
    put_u64(out, wire->epoch);
    put_u32(out + 8U, wire->activity_type);
    put_u64(out + 12U, wire->meter.canonical_encoded_bytes);
    put_u64(out + 20U, wire->meter.execution_units);
    put_u64(out + 28U, wire->meter.storage_units);
    (void)lxp_u128_to_be(wire->fee_limit, out + 36U);
    (void)memcpy(&result_bits, &wire->execution_result, sizeof(result_bits));
    put_u32(out + 52U, result_bits);
}

static lxp_result wire_decode(const uint8_t *bytes, size_t length,
                              fee_wire *wire)
{
    uint32_t result_bits;
    if (bytes == NULL || wire == NULL || length != WIRE_BYTES)
        return LXP_ERR_MALFORMED_ENVELOPE;
    (void)memset(wire, 0, sizeof(*wire));
    wire->epoch = get_u64(bytes);
    wire->activity_type = get_u32(bytes + 8U);
    wire->meter.canonical_encoded_bytes = get_u64(bytes + 12U);
    wire->meter.execution_units = get_u64(bytes + 20U);
    wire->meter.storage_units = get_u64(bytes + 28U);
    if (lxp_u128_from_be(bytes + 36U, &wire->fee_limit) != LXP_OK)
        return LXP_ERR_MALFORMED_ENVELOPE;
    result_bits = get_u32(bytes + 52U);
    (void)memcpy(&wire->execution_result, &result_bits, sizeof(result_bits));
    return lxp_result_is_fatal(wire->execution_result) ?
           LXP_ERR_MALFORMED_ENVELOPE : LXP_OK;
}

static void entry_encode(const lxp_fee_replay_entry *entry,
                         uint8_t out[ENTRY_BYTES])
{
    (void)lxp_u128_to_be(entry->fee_charged, out);
    (void)lxp_u128_to_be(entry->actor_fee_debit, out + 16U);
    (void)lxp_u128_to_be(entry->treasury_fee_credit, out + 32U);
    (void)lxp_u128_to_be(entry->treasury_balance, out + 48U);
    (void)memcpy(out + 64U, entry->resulting_state_root, 32U);
    put_u32(out + 96U, entry->parameter_version);
}

static lxp_result entry_decode(const uint8_t *bytes, size_t length,
                               lxp_fee_replay_entry *entry)
{
    if (bytes == NULL || entry == NULL || length != ENTRY_BYTES)
        return LXP_ERR_MALFORMED_ENVELOPE;
    (void)memset(entry, 0, sizeof(*entry));
    if (lxp_u128_from_be(bytes, &entry->fee_charged) != LXP_OK ||
        lxp_u128_from_be(bytes + 16U, &entry->actor_fee_debit) != LXP_OK ||
        lxp_u128_from_be(bytes + 32U, &entry->treasury_fee_credit) != LXP_OK ||
        lxp_u128_from_be(bytes + 48U, &entry->treasury_balance) != LXP_OK)
        return LXP_ERR_MALFORMED_ENVELOPE;
    (void)memcpy(entry->resulting_state_root, bytes + 64U, 32U);
    entry->parameter_version = get_u32(bytes + 96U);
    return entry->parameter_version == 0U ? LXP_ERR_MALFORMED_ENVELOPE : LXP_OK;
}

static int add_parameter(lxp_param_table *table, const char *key,
                         uint64_t value)
{
    return lxp_param_set_bounds(
        table, (lxp_byte_span){(const uint8_t *)key, strlen(key)}, 1U,
        0U, UINT32_MAX, value, 1U) == LXP_OK ? 0 : 1;
}

static lxp_result fixture_init(fee_fixture *fixture)
{
    static const uint8_t actor_name[] = "agent:fee-replay:main";
    static const uint8_t treasury_name[] = "system:fees";
    uint8_t actor_id[32];
    uint8_t treasury_id[32];
    uint8_t proposal_id[32] = {1U};
    lxp_result status;
    (void)memset(fixture, 0, sizeof(*fixture));
    status = lxp_param_table_init(&fixture->parameters);
    if (status != LXP_OK ||
        add_parameter(&fixture->parameters, "fee.base", 2U) != 0 ||
        add_parameter(&fixture->parameters, "fee.activity", 0U) != 0 ||
        add_parameter(&fixture->parameters, "fee.byte", 1U) != 0 ||
        add_parameter(&fixture->parameters, "fee.exec", 1U) != 0 ||
        add_parameter(&fixture->parameters, "fee.storage", 1U) != 0 ||
        add_parameter(&fixture->parameters, "fee.multiplier_bps", 10000U) != 0)
        return LXP_ERR_PARAMETER_BOUNDS;
    status = lxp_param_apply_ordered(
        &fixture->parameters,
        (lxp_byte_span){(const uint8_t *)"fee.base", 8U},
        7U, 5U, proposal_id, true);
    if (status != LXP_OK) return status;
    fixture->asset_id[0] = 0x40U;
    fixture->asset_id[1] = 0x32U;
    status = lx_account_registry_init(&fixture->registry);
    if (status == LXP_OK)
        status = lx_account_id_from_string(actor_name,
                                           sizeof(actor_name) - 1U, actor_id);
    if (status == LXP_OK)
        status = lx_account_id_from_string(treasury_name,
                                           sizeof(treasury_name) - 1U,
                                           treasury_id);
    if (status == LXP_OK)
        status = lx_account_open(&fixture->registry, actor_name,
                                 sizeof(actor_name) - 1U, actor_id, 1U,
                                 LX_ACCOUNT_OPEN_GENESIS, NULL,
                                 &fixture->actor);
    if (status == LXP_OK)
        status = lx_account_open(&fixture->registry, treasury_name,
                                 sizeof(treasury_name) - 1U, treasury_id, 2U,
                                 LX_ACCOUNT_OPEN_GENESIS, NULL,
                                 &fixture->treasury);
    if (status == LXP_OK)
        status = lxp_ledger_bootstrap_balance(
            fixture->actor, fixture->asset_id, (lxp_u128){0U, 100000U}, 0U);
    if (status == LXP_OK)
        status = lxp_ledger_bootstrap_balance(
            fixture->treasury, fixture->asset_id, (lxp_u128){0U, 50U}, 0U);
    if (status != LXP_OK) return status;
    (void)memcpy(fixture->asset.asset_id, fixture->asset_id, 32U);
    fixture->asset.registered = true;
    fixture->transfer_context.assets = &fixture->asset;
    fixture->transfer_context.asset_count = 1U;
    (void)memcpy(fixture->transfer_context.authorized_from,
                 fixture->actor->id, 32U);
    fixture->transfer_context.origin_module_id = 1U;
    fixture->transfer_context.debit_authority_kind = LXP_AUTH_OWNER;
    return LXP_OK;
}

static lxp_result execute_wire(fee_fixture *fixture, const fee_wire *wire,
                               uint64_t sequence,
                               lxp_fee_replay_entry *entry)
{
    lxp_fee_params schedule;
    uint32_t parameter_version;
    lxp_u128 computed_fee;
    lxp_fee_policy_decision admission;
    lxp_fee_policy_decision policy;
    lxp_transfer_result transfer;
    lxp_receipt receipt;
    uint8_t material[112];
    size_t offset = 0U;
    lxp_result status = lxp_fee_schedule(
        &fixture->parameters, wire->epoch, NULL, &schedule,
        &parameter_version);
    if (status == LXP_OK)
        status = lxp_fee_compute(&schedule, wire->activity_type, wire->meter,
                                 &computed_fee);
    if (status == LXP_OK)
        status = lxp_fee_admission_check(
            (lxp_admission_result){LXP_OK, true, true, true}, wire->fee_limit,
            fixture->actor->balance, &admission);
    if (status == LXP_OK)
        status = lxp_fee_rejection_policy(
            &admission, wire->execution_result, computed_fee, wire->fee_limit,
            &policy);
    if (status != LXP_OK || !policy.assign_global_sequence ||
        !policy.consume_account_sequence)
        return status != LXP_OK ? status : LXP_FATAL_INVARIANT;
    (void)memset(&receipt, 0, sizeof(receipt));
    fixture->transfer_context.actor_sequence = fixture->actor->next_sequence;
    if (policy.charge_fee)
        status = lxp_fee_charge(
            fixture->actor, fixture->treasury, fixture->asset_id,
            policy.fee_charged, wire->fee_limit, &fixture->transfer_context,
            &receipt, &transfer);
    if (status != LXP_OK) return status;
    (void)memset(entry, 0, sizeof(*entry));
    entry->fee_charged = policy.fee_charged;
    entry->actor_fee_debit = policy.fee_charged;
    entry->treasury_fee_credit = policy.fee_charged;
    entry->treasury_balance = fixture->treasury->balance;
    entry->parameter_version = parameter_version;
    (void)memcpy(material + offset, fixture->previous_root, 32U);
    offset += 32U;
    (void)lxp_u128_to_be(fixture->actor->balance, material + offset);
    offset += 16U;
    (void)lxp_u128_to_be(fixture->treasury->balance, material + offset);
    offset += 16U;
    (void)lxp_u128_to_be(policy.fee_charged, material + offset);
    offset += 16U;
    put_u64(material + offset, sequence);
    offset += 8U;
    put_u32(material + offset, parameter_version);
    offset += 4U;
    {
        uint32_t result_bits;
        (void)memcpy(&result_bits, &policy.result_code, sizeof(result_bits));
        put_u32(material + offset, result_bits);
        offset += 4U;
    }
    status = lxp_hash_domain(LXP_DOMAIN_RECEIPT, material, offset,
                             entry->resulting_state_root);
    if (status == LXP_OK)
        (void)memcpy(fixture->previous_root, entry->resulting_state_root, 32U);
    return status;
}

static lxp_result replay_record(void *opaque,
                                const lxp_log_record_header *header,
                                const uint8_t *body)
{
    replay_context *context = (replay_context *)opaque;
    if (header->record_kind == (uint8_t)LXP_LOG_ACTIVITY) {
        fee_wire wire;
        lxp_result status;
        if (context->activity_count == TEST_RECORDS)
            return LXP_ERR_LENGTH_LIMIT;
        status = wire_decode(body, header->body_length, &wire);
        if (status == LXP_OK)
            status = execute_wire(
                context->fixture, &wire, header->global_sequence,
                &context->replayed[context->activity_count]);
        if (status == LXP_OK) ++context->activity_count;
        return status;
    }
    if (header->record_kind == (uint8_t)LXP_LOG_RECEIPT) {
        lxp_result status;
        if (context->receipt_count >= context->activity_count ||
            context->receipt_count == TEST_RECORDS)
            return LXP_FATAL_REPLAY_DIVERGENCE;
        status = entry_decode(body, header->body_length,
                              &context->logged[context->receipt_count]);
        if (status == LXP_OK) ++context->receipt_count;
        return status;
    }
    return LXP_FATAL_REPLAY_DIVERGENCE;
}

static int policy_boundaries(void)
{
    lxp_fee_policy_decision admission;
    lxp_fee_policy_decision policy;
    if (lxp_fee_admission_check(
            (lxp_admission_result){LXP_ERR_BAD_SIGNATURE, false, false, false},
            (lxp_u128){0U, 10U}, (lxp_u128){0U, 100U}, &admission) != LXP_OK ||
        admission.result_code != LXP_ERR_BAD_SIGNATURE ||
        admission.assign_global_sequence || admission.consume_account_sequence ||
        admission.charge_fee ||
        lxp_fee_rejection_policy(
            &admission, LXP_OK, (lxp_u128){0U, 5U},
            (lxp_u128){0U, 10U}, &policy) != LXP_OK ||
        policy.result_code != LXP_ERR_BAD_SIGNATURE ||
        policy.assign_global_sequence || policy.consume_account_sequence ||
        policy.charge_fee)
        return 1;
    if (lxp_fee_admission_check(
            (lxp_admission_result){LXP_OK, true, true, true},
            (lxp_u128){0U, 10U}, (lxp_u128){0U, 9U}, &admission) != LXP_OK ||
        admission.result_code != LXP_ERR_FEE_UNPAYABLE ||
        admission.assign_global_sequence || admission.consume_account_sequence)
        return 1;
    if (lxp_fee_admission_check(
            (lxp_admission_result){LXP_OK, true, true, true},
            (lxp_u128){0U, 10U}, (lxp_u128){0U, 10U}, &admission) != LXP_OK ||
        lxp_fee_rejection_policy(
            &admission, LXP_OK, (lxp_u128){0U, 10U},
            (lxp_u128){0U, 10U}, &policy) != LXP_OK ||
        policy.result_code != LXP_OK || policy.fee_charged.lo != 10U ||
        !policy.assign_global_sequence || !policy.consume_account_sequence ||
        !policy.charge_fee || !policy.apply_module_effects)
        return 1;
    if (lxp_fee_rejection_policy(
            &admission, LXP_OK, (lxp_u128){0U, 11U},
            (lxp_u128){0U, 10U}, &policy) != LXP_OK ||
        policy.result_code != LXP_ERR_FEE_LIMIT ||
        policy.fee_charged.lo != 10U || !policy.charge_fee ||
        policy.apply_module_effects)
        return 1;
    if (lxp_fee_rejection_policy(
            &admission, LXP_ERR_AGREEMENT_STATE, (lxp_u128){0U, 9U},
            (lxp_u128){0U, 10U}, &policy) != LXP_OK ||
        policy.result_code != LXP_ERR_AGREEMENT_STATE ||
        policy.fee_charged.lo != 9U || !policy.consume_account_sequence ||
        !policy.charge_fee || policy.apply_module_effects)
        return 1;
    return 0;
}

int main(void)
{
    static fee_fixture committed_fixture;
    static fee_fixture replay_fixture;
    static lxp_fee_replay_entry committed[TEST_RECORDS];
    static lxp_fee_replay_entry replayed[TEST_RECORDS];
    static lxp_fee_replay_entry logged[TEST_RECORDS];
    static const fee_wire wires[TEST_RECORDS] = {
        {1U, 1U, {5U, 2U, 1U, false, 0U, {0U, 0U}}, {0U, 1000U}, LXP_OK},
        {2U, 2U, {7U, 4U, 3U, false, 0U, {0U, 0U}}, {0U, 1000U},
         LXP_ERR_AGREEMENT_STATE},
        {6U, 3U, {11U, 6U, 5U, false, 0U, {0U, 0U}}, {0U, 1000U}, LXP_OK},
        {8U, 4U, {13U, 8U, 7U, false, 0U, {0U, 0U}}, {0U, 1000U},
         LXP_ERR_MARKET_HALTED}
    };
    char directory[] = "/tmp/lxp-fee-replay-XXXXXX";
    char path[128];
    lxp_log log;
    replay_context replay_context_value;
    size_t i;
    if (policy_boundaries() != 0 || fixture_init(&committed_fixture) != LXP_OK ||
        mkdtemp(directory) == NULL ||
        lxp_log_segment_create(&log, directory, 0U, 16384U) != LXP_OK)
        return 1;
    for (i = 0U; i < TEST_RECORDS; ++i) {
        uint8_t wire_bytes[WIRE_BYTES];
        uint8_t entry_bytes[ENTRY_BYTES];
        wire_encode(&wires[i], wire_bytes);
        if (execute_wire(&committed_fixture, &wires[i], i,
                         &committed[i]) != LXP_OK)
            return 1;
        entry_encode(&committed[i], entry_bytes);
        if (lxp_log_append(&log, LXP_LOG_ACTIVITY, i, wire_bytes,
                           WIRE_BYTES, NULL) != LXP_OK ||
            lxp_log_append(&log, LXP_LOG_RECEIPT, i, entry_bytes,
                           ENTRY_BYTES, NULL) != LXP_OK)
            return 1;
    }
    if (lxp_log_write_boundary(&log) != LXP_OK ||
        lxp_log_close(&log) != LXP_OK ||
        snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0 ||
        lxp_log_open(&log, path) != LXP_OK ||
        fixture_init(&replay_fixture) != LXP_OK)
        return 1;
    (void)memset(&replay_context_value, 0, sizeof(replay_context_value));
    replay_context_value.fixture = &replay_fixture;
    replay_context_value.replayed = replayed;
    replay_context_value.logged = logged;
    if (lxp_log_recover(&log, replay_record, &replay_context_value) != LXP_OK ||
        replay_context_value.activity_count != TEST_RECORDS ||
        replay_context_value.receipt_count != TEST_RECORDS ||
        lxp_fee_replay_check(committed, logged, TEST_RECORDS,
                             (lxp_u128){0U, 50U}) != LXP_OK ||
        lxp_fee_replay_check(committed, replayed, TEST_RECORDS,
                             (lxp_u128){0U, 50U}) != LXP_OK)
        return 1;
    replayed[2].resulting_state_root[0] ^= 1U;
    if (lxp_fee_replay_check(committed, replayed, TEST_RECORDS,
                             (lxp_u128){0U, 50U}) !=
        LXP_FATAL_REPLAY_DIVERGENCE)
        return 1;
    replayed[2] = committed[2];
    replayed[2].treasury_fee_credit.lo += 1U;
    if (lxp_fee_replay_check(committed, replayed, TEST_RECORDS,
                             (lxp_u128){0U, 50U}) !=
        LXP_FATAL_SUPPLY_MISMATCH)
        return 1;
    if (lxp_log_close(&log) != LXP_OK || unlink(path) != 0 ||
        rmdir(directory) != 0)
        return 1;
    return 0;
}
