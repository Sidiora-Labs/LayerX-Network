#include "layerx/programs.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_u128.h"

#include <limits.h>
#include <string.h>

enum {
    PROGRAMS_FEE_PRICE_FIELDS = LX_PROGRAMS_FEE_PRICE_FIELDS,
    PROGRAMS_FEE_RECORD_BYTES = 217,
    PROGRAMS_FEE_PENDING_BYTES = 197,
    PROGRAMS_FEE_PROPOSAL_BYTES = 149
};

static const uint8_t fee_active_key[] = "progfee/active/v1";
static const uint8_t fee_pending_key[] = "progfee/pending/v1";
static const uint8_t fee_history_prefix[] = "progfee/history/v1/";
static const uint8_t fee_record_magic[5] = {'L', 'X', 'F', 'R', '1'};
static const uint8_t fee_pending_magic[5] = {'L', 'X', 'F', 'P', '1'};
static const uint8_t fee_proposal_magic[5] = {'L', 'X', 'F', 'G', '1'};

typedef struct programs_fee_demand_policy {
    uint64_t target_occupancy_byte_batches;
    uint64_t response_denominator;
    uint64_t maximum_change_numerator;
    uint64_t maximum_change_denominator;
    uint64_t minimum_fee_units_per_occupancy_byte_batch;
    uint64_t maximum_fee_units_per_occupancy_byte_batch;
} programs_fee_demand_policy;

typedef struct programs_fee_record {
    lx_programs_fee_schedule schedule;
    uint8_t occupancy_asset_id[32];
    programs_fee_demand_policy demand;
    uint64_t activation_batch;
    uint64_t last_occupancy_batch;
    uint64_t governance_sequence;
    uint8_t governance_receipt_digest[32];
    lxp_u128 observed_occupancy_byte_batches;
} programs_fee_record;

typedef struct programs_fee_pending {
    lx_programs_fee_schedule proposed_schedule;
    uint8_t occupancy_asset_id[32];
    programs_fee_demand_policy demand;
    uint64_t activation_batch;
    uint64_t staged_batch;
    uint64_t governance_sequence;
    uint8_t governance_receipt_digest[32];
} programs_fee_pending;

typedef struct programs_fee_governance_activity {
    lx_programs_fee_schedule proposed;
    uint8_t occupancy_asset_id[32];
    programs_fee_demand_policy demand;
    uint64_t activation_batch;
    lxp_receipt governance_receipt;
} programs_fee_governance_activity;

static uint32_t read_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) |
           ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | (uint32_t)bytes[3];
}

static uint64_t read_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index)
        value = (value << 8U) | bytes[index];
    return value;
}

static void write_u32(uint8_t *bytes, uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static void write_u64(uint8_t *bytes, uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> (56U - 8U * index));
}

static void schedule_prices(const lx_programs_fee_schedule *schedule,
                            uint64_t prices[PROGRAMS_FEE_PRICE_FIELDS])
{
    prices[0] = schedule->cpu;
    prices[1] = schedule->memory_byte;
    prices[2] = schedule->storage_read_byte;
    prices[3] = schedule->storage_write_byte;
    prices[4] = schedule->output_value;
    prices[5] = schedule->output_byte;
    prices[6] = schedule->occupancy_byte_batch;
}

static void schedule_from_prices(lx_programs_fee_schedule *schedule,
                                 const uint64_t prices[PROGRAMS_FEE_PRICE_FIELDS])
{
    schedule->cpu = prices[0];
    schedule->memory_byte = prices[1];
    schedule->storage_read_byte = prices[2];
    schedule->storage_write_byte = prices[3];
    schedule->output_value = prices[4];
    schedule->output_byte = prices[5];
    schedule->occupancy_byte_batch = prices[6];
}

static bool schedule_prices_valid(const lx_programs_fee_schedule *schedule)
{
    uint64_t prices[PROGRAMS_FEE_PRICE_FIELDS];
    size_t index;
    if (schedule == NULL) return false;
    schedule_prices(schedule, prices);
    for (index = 0U; index < PROGRAMS_FEE_PRICE_FIELDS; ++index)
        if (prices[index] == 0U) return false;
    return true;
}

static bool demand_policy_valid(const programs_fee_demand_policy *policy)
{
    return policy != NULL && policy->target_occupancy_byte_batches != 0U &&
        policy->response_denominator != 0U &&
        policy->maximum_change_numerator != 0U &&
        policy->maximum_change_denominator != 0U &&
        policy->maximum_change_numerator <=
            policy->maximum_change_denominator &&
        policy->minimum_fee_units_per_occupancy_byte_batch != 0U &&
        policy->minimum_fee_units_per_occupancy_byte_batch <=
            policy->maximum_fee_units_per_occupancy_byte_batch;
}

static bool record_valid(const programs_fee_record *record)
{
    return record != NULL && record->schedule.version != 0U &&
        schedule_prices_valid(&record->schedule) &&
        demand_policy_valid(&record->demand) &&
        record->schedule.occupancy_byte_batch >=
            record->demand.minimum_fee_units_per_occupancy_byte_batch &&
        record->schedule.occupancy_byte_batch <=
            record->demand.maximum_fee_units_per_occupancy_byte_batch &&
        record->activation_batch != 0U &&
        record->last_occupancy_batch < UINT64_MAX &&
        record->governance_sequence != 0U &&
        !lxp_ct_is_zero(record->occupancy_asset_id, 32U) &&
        !lxp_ct_is_zero(record->governance_receipt_digest, 32U);
}

static bool pending_valid(const programs_fee_pending *pending)
{
    return pending != NULL && pending->proposed_schedule.version == 0U &&
        schedule_prices_valid(&pending->proposed_schedule) &&
        demand_policy_valid(&pending->demand) &&
        pending->proposed_schedule.occupancy_byte_batch >=
            pending->demand.minimum_fee_units_per_occupancy_byte_batch &&
        pending->proposed_schedule.occupancy_byte_batch <=
            pending->demand.maximum_fee_units_per_occupancy_byte_batch &&
        pending->staged_batch != 0U &&
        pending->activation_batch > pending->staged_batch &&
        pending->governance_sequence != 0U &&
        !lxp_ct_is_zero(pending->occupancy_asset_id, 32U) &&
        !lxp_ct_is_zero(pending->governance_receipt_digest, 32U);
}

static void encode_policy(uint8_t *bytes,
                          const programs_fee_demand_policy *policy)
{
    write_u64(bytes, policy->target_occupancy_byte_batches);
    write_u64(bytes + 8U, policy->response_denominator);
    write_u64(bytes + 16U, policy->maximum_change_numerator);
    write_u64(bytes + 24U, policy->maximum_change_denominator);
    write_u64(bytes + 32U,
              policy->minimum_fee_units_per_occupancy_byte_batch);
    write_u64(bytes + 40U,
              policy->maximum_fee_units_per_occupancy_byte_batch);
}

static void decode_policy(const uint8_t *bytes,
                          programs_fee_demand_policy *policy)
{
    policy->target_occupancy_byte_batches = read_u64(bytes);
    policy->response_denominator = read_u64(bytes + 8U);
    policy->maximum_change_numerator = read_u64(bytes + 16U);
    policy->maximum_change_denominator = read_u64(bytes + 24U);
    policy->minimum_fee_units_per_occupancy_byte_batch =
        read_u64(bytes + 32U);
    policy->maximum_fee_units_per_occupancy_byte_batch =
        read_u64(bytes + 40U);
}

static void encode_prices(uint8_t *bytes,
                          const lx_programs_fee_schedule *schedule)
{
    uint64_t prices[PROGRAMS_FEE_PRICE_FIELDS];
    size_t index;
    schedule_prices(schedule, prices);
    for (index = 0U; index < PROGRAMS_FEE_PRICE_FIELDS; ++index)
        write_u64(bytes + index * 8U, prices[index]);
}

static void decode_prices(const uint8_t *bytes,
                          lx_programs_fee_schedule *schedule)
{
    uint64_t prices[PROGRAMS_FEE_PRICE_FIELDS];
    size_t index;
    for (index = 0U; index < PROGRAMS_FEE_PRICE_FIELDS; ++index)
        prices[index] = read_u64(bytes + index * 8U);
    schedule_from_prices(schedule, prices);
}

static lxp_result record_encode(const programs_fee_record *record,
                                uint8_t encoded[PROGRAMS_FEE_RECORD_BYTES])
{
    size_t offset = 0U;
    if (!record_valid(record) || encoded == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(encoded + offset, fee_record_magic,
                 sizeof(fee_record_magic));
    offset += sizeof(fee_record_magic);
    write_u32(encoded + offset, record->schedule.version); offset += 4U;
    encode_prices(encoded + offset, &record->schedule); offset += 56U;
    (void)memcpy(encoded + offset, record->occupancy_asset_id, 32U);
    offset += 32U;
    encode_policy(encoded + offset, &record->demand); offset += 48U;
    write_u64(encoded + offset, record->activation_batch); offset += 8U;
    write_u64(encoded + offset, record->last_occupancy_batch); offset += 8U;
    write_u64(encoded + offset, record->governance_sequence); offset += 8U;
    (void)memcpy(encoded + offset, record->governance_receipt_digest, 32U);
    offset += 32U;
    write_u64(encoded + offset, record->observed_occupancy_byte_batches.hi);
    offset += 8U;
    write_u64(encoded + offset, record->observed_occupancy_byte_batches.lo);
    offset += 8U;
    return offset == PROGRAMS_FEE_RECORD_BYTES ? LXP_OK :
                                                LXP_FATAL_INVARIANT;
}

static lxp_result record_decode(const uint8_t *encoded, size_t length,
                                programs_fee_record *record)
{
    size_t offset = 0U;
    if (encoded == NULL || record == NULL ||
        length != PROGRAMS_FEE_RECORD_BYTES ||
        memcmp(encoded, fee_record_magic, sizeof(fee_record_magic)) != 0)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(record, 0, sizeof(*record));
    offset += sizeof(fee_record_magic);
    record->schedule.version = read_u32(encoded + offset); offset += 4U;
    decode_prices(encoded + offset, &record->schedule); offset += 56U;
    (void)memcpy(record->occupancy_asset_id, encoded + offset, 32U);
    offset += 32U;
    decode_policy(encoded + offset, &record->demand); offset += 48U;
    record->activation_batch = read_u64(encoded + offset); offset += 8U;
    record->last_occupancy_batch = read_u64(encoded + offset); offset += 8U;
    record->governance_sequence = read_u64(encoded + offset); offset += 8U;
    (void)memcpy(record->governance_receipt_digest, encoded + offset, 32U);
    offset += 32U;
    record->observed_occupancy_byte_batches.hi = read_u64(encoded + offset);
    offset += 8U;
    record->observed_occupancy_byte_batches.lo = read_u64(encoded + offset);
    offset += 8U;
    return offset == length && record_valid(record) ? LXP_OK :
                                                     LXP_FATAL_REPLAY_DIVERGENCE;
}

static lxp_result pending_encode(
    const programs_fee_pending *pending,
    uint8_t encoded[PROGRAMS_FEE_PENDING_BYTES])
{
    size_t offset = 0U;
    if (!pending_valid(pending) || encoded == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(encoded + offset, fee_pending_magic,
                 sizeof(fee_pending_magic));
    offset += sizeof(fee_pending_magic);
    encode_prices(encoded + offset, &pending->proposed_schedule); offset += 56U;
    (void)memcpy(encoded + offset, pending->occupancy_asset_id, 32U);
    offset += 32U;
    encode_policy(encoded + offset, &pending->demand); offset += 48U;
    write_u64(encoded + offset, pending->activation_batch); offset += 8U;
    write_u64(encoded + offset, pending->staged_batch); offset += 8U;
    write_u64(encoded + offset, pending->governance_sequence); offset += 8U;
    (void)memcpy(encoded + offset, pending->governance_receipt_digest, 32U);
    offset += 32U;
    return offset == PROGRAMS_FEE_PENDING_BYTES ? LXP_OK :
                                                 LXP_FATAL_INVARIANT;
}

static lxp_result pending_decode(const uint8_t *encoded, size_t length,
                                 programs_fee_pending *pending)
{
    size_t offset = 0U;
    if (encoded == NULL || pending == NULL ||
        length != PROGRAMS_FEE_PENDING_BYTES ||
        memcmp(encoded, fee_pending_magic, sizeof(fee_pending_magic)) != 0)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(pending, 0, sizeof(*pending));
    offset += sizeof(fee_pending_magic);
    decode_prices(encoded + offset, &pending->proposed_schedule); offset += 56U;
    (void)memcpy(pending->occupancy_asset_id, encoded + offset, 32U);
    offset += 32U;
    decode_policy(encoded + offset, &pending->demand); offset += 48U;
    pending->activation_batch = read_u64(encoded + offset); offset += 8U;
    pending->staged_batch = read_u64(encoded + offset); offset += 8U;
    pending->governance_sequence = read_u64(encoded + offset); offset += 8U;
    (void)memcpy(pending->governance_receipt_digest, encoded + offset, 32U);
    offset += 32U;
    return offset == length && pending_valid(pending) ? LXP_OK :
                                                      LXP_FATAL_REPLAY_DIVERGENCE;
}

static lxp_result proposal_encode(
    const lx_programs_fee_schedule *proposed,
    const uint8_t occupancy_asset_id[32],
    const programs_fee_demand_policy *demand, uint64_t activation_batch,
    uint8_t encoded[PROGRAMS_FEE_PROPOSAL_BYTES])
{
    size_t offset = 0U;
    if (proposed == NULL || proposed->version != 0U ||
        !schedule_prices_valid(proposed) || occupancy_asset_id == NULL ||
        lxp_ct_is_zero(occupancy_asset_id, 32U) ||
        !demand_policy_valid(demand) ||
        proposed->occupancy_byte_batch <
            demand->minimum_fee_units_per_occupancy_byte_batch ||
        proposed->occupancy_byte_batch >
            demand->maximum_fee_units_per_occupancy_byte_batch ||
        activation_batch == 0U || encoded == NULL)
        return LXP_ERR_PARAMETER_BOUNDS;
    (void)memcpy(encoded + offset, fee_proposal_magic,
                 sizeof(fee_proposal_magic));
    offset += sizeof(fee_proposal_magic);
    encode_prices(encoded + offset, proposed); offset += 56U;
    (void)memcpy(encoded + offset, occupancy_asset_id, 32U); offset += 32U;
    encode_policy(encoded + offset, demand); offset += 48U;
    write_u64(encoded + offset, activation_batch); offset += 8U;
    return offset == PROGRAMS_FEE_PROPOSAL_BYTES ? LXP_OK :
                                                  LXP_FATAL_INVARIANT;
}

static lxp_result governance_receipt_commits_proposal(
    const lxp_receipt *receipt,
    const uint8_t proposal[PROGRAMS_FEE_PROPOSAL_BYTES])
{
    size_t matches = 0U;
    size_t index;
    if (receipt == NULL || proposal == NULL ||
        receipt->effects.count > LXP_MAX_EFFECTS)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < receipt->effects.count; ++index) {
        const lxp_effect *effect = &receipt->effects.effects[index];
        if (effect->module_id == LXP_MODULE_GOVERNANCE &&
            effect->kind == LXP_EFFECT_STATE && !effect->monetary &&
            lxp_ct_is_zero(effect->transfer_set_root, 32U) &&
            effect->body_length == PROGRAMS_FEE_PROPOSAL_BYTES &&
            memcmp(effect->body, proposal,
                   (size_t)PROGRAMS_FEE_PROPOSAL_BYTES) == 0)
            ++matches;
    }
    return matches == 1U ? LXP_OK : LXP_ERR_AUTH_SCOPE;
}

static void history_key(uint32_t version,
                        uint8_t key[sizeof(fee_history_prefix) - 1U + 4U])
{
    (void)memcpy(key, fee_history_prefix, sizeof(fee_history_prefix) - 1U);
    write_u32(key + sizeof(fee_history_prefix) - 1U, version);
}

static int genesis_manifest_order(
    uint16_t module_id, const uint8_t key[32],
    const lxp_genesis_module_value *right)
{
    if (module_id < right->module_id) return -1;
    if (module_id > right->module_id) return 1;
    return memcmp(key, right->key, 32U);
}

static lxp_result genesis_manifest_insert(
    lxp_genesis_manifest *manifest, const uint8_t *key, size_t key_length,
    const uint8_t *value, size_t value_length)
{
    uint8_t padded_key[32] = {0U};
    size_t location = 0U;
    if (manifest == NULL || key == NULL || key_length == 0U ||
        key_length > sizeof(padded_key) || value == NULL ||
        value_length == 0U || value_length > LXP_GENESIS_MODULE_VALUE_BYTES ||
        manifest->module_value_count == LXP_GENESIS_MAX_MODULE_VALUES)
        return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(padded_key, key, key_length);
    while (location < manifest->module_value_count &&
           genesis_manifest_order(LXP_MODULE_PROGRAMS, padded_key,
                                  &manifest->module_values[location]) > 0)
        ++location;
    if (location < manifest->module_value_count &&
        genesis_manifest_order(LXP_MODULE_PROGRAMS, padded_key,
                               &manifest->module_values[location]) == 0)
        return LXP_ERR_SEQUENCE_REUSED;
    (void)memmove(&manifest->module_values[location + 1U],
                  &manifest->module_values[location],
                  (manifest->module_value_count - location) *
                      sizeof(manifest->module_values[0]));
    (void)memset(&manifest->module_values[location], 0,
                 sizeof(manifest->module_values[location]));
    manifest->module_values[location].module_id = LXP_MODULE_PROGRAMS;
    (void)memcpy(manifest->module_values[location].key, padded_key, 32U);
    (void)memcpy(manifest->module_values[location].value, value, value_length);
    manifest->module_values[location].value_length = value_length;
    ++manifest->module_value_count;
    return LXP_OK;
}

static bool genesis_manifest_contains(
    const lxp_genesis_manifest *manifest, const uint8_t *key,
    size_t key_length)
{
    uint8_t padded_key[32] = {0U};
    size_t index;
    if (manifest == NULL || key == NULL || key_length > sizeof(padded_key))
        return false;
    (void)memcpy(padded_key, key, key_length);
    for (index = 0U; index < manifest->module_value_count; ++index)
        if (manifest->module_values[index].module_id == LXP_MODULE_PROGRAMS &&
            memcmp(manifest->module_values[index].key, padded_key, 32U) == 0)
            return true;
    return false;
}

static bool genesis_key_matches(
    const lxp_genesis_module_value *value, const uint8_t *key,
    size_t key_length)
{
    return value != NULL && key != NULL && key_length <= 32U &&
        value->module_id == LXP_MODULE_PROGRAMS &&
        memcmp(value->key, key, key_length) == 0 &&
        lxp_ct_is_zero(value->key + key_length, 32U - key_length);
}

static bool genesis_record_valid(
    const programs_fee_record *record, const uint8_t signer_digest[32])
{
    return record_valid(record) && record->schedule.version == 1U &&
        record->activation_batch == 1U && record->last_occupancy_batch == 0U &&
        record->governance_sequence == 1U &&
        lxp_u128_is_zero(record->observed_occupancy_byte_batches) &&
        lxp_ct_memcmp(record->governance_receipt_digest,
                      signer_digest, 32U) == 0;
}

lxp_result lxp_programs_fee_genesis_append(
    lxp_genesis_manifest *manifest,
    const lx_programs_fee_genesis_parameters *parameters)
{
    programs_fee_record record;
    uint8_t encoded[PROGRAMS_FEE_RECORD_BYTES];
    uint8_t key[sizeof(fee_history_prefix) - 1U + 4U];
    uint8_t signer_digest[32];
    size_t original_count;
    lxp_result status;
    if (manifest == NULL || parameters == NULL ||
        parameters->schedule.version != 1U ||
        manifest->module_value_count > LXP_GENESIS_MAX_MODULE_VALUES - 2U ||
        lxp_ct_is_zero(manifest->signer_public_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&record, 0, sizeof(record));
    record.schedule = parameters->schedule;
    (void)memcpy(record.occupancy_asset_id,
                 parameters->occupancy_asset_id, 32U);
    record.demand = (programs_fee_demand_policy){
        parameters->target_occupancy_byte_batches,
        parameters->response_denominator,
        parameters->maximum_change_numerator,
        parameters->maximum_change_denominator,
        parameters->minimum_fee_units_per_occupancy_byte_batch,
        parameters->maximum_fee_units_per_occupancy_byte_batch
    };
    record.activation_batch = 1U;
    record.governance_sequence = 1U;
    status = lxp_hash_payload(manifest->signer_public_key, 32U,
                              signer_digest);
    if (status == LXP_OK)
        (void)memcpy(record.governance_receipt_digest, signer_digest, 32U);
    if (status == LXP_OK) status = record_encode(&record, encoded);
    if (status != LXP_OK) return status;
    history_key(1U, key);
    if (genesis_manifest_contains(manifest, fee_active_key,
                                  sizeof(fee_active_key) - 1U) ||
        genesis_manifest_contains(manifest, key, sizeof(key)))
        return LXP_ERR_SEQUENCE_REUSED;
    original_count = manifest->module_value_count;
    status = genesis_manifest_insert(
        manifest, fee_active_key, sizeof(fee_active_key) - 1U,
        encoded, sizeof(encoded));
    if (status == LXP_OK)
        status = genesis_manifest_insert(manifest, key, sizeof(key),
                                         encoded, sizeof(encoded));
    if (status != LXP_OK) manifest->module_value_count = original_count;
    return status;
}

lxp_result lxp_programs_fee_genesis_validate(
    const lxp_genesis_manifest *manifest)
{
    const lxp_genesis_module_value *active = NULL;
    const lxp_genesis_module_value *history = NULL;
    uint8_t key[sizeof(fee_history_prefix) - 1U + 4U];
    uint8_t signer_digest[32];
    size_t index;
    lxp_result status;
    if (manifest == NULL || lxp_ct_is_zero(manifest->signer_public_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_hash_payload(manifest->signer_public_key, 32U,
                              signer_digest);
    if (status != LXP_OK) return status;
    history_key(1U, key);
    for (index = 0U; index < manifest->module_value_count; ++index) {
        const lxp_genesis_module_value *value = &manifest->module_values[index];
        programs_fee_record decoded;
        bool is_active = genesis_key_matches(
            value, fee_active_key, sizeof(fee_active_key) - 1U);
        bool is_history = genesis_key_matches(value, key, sizeof(key));
        if (!is_active && !is_history) continue;
        if ((is_active && active != NULL) || (is_history && history != NULL))
            return LXP_ERR_SEQUENCE_REUSED;
        status = record_decode(value->value, value->value_length, &decoded);
        if (status != LXP_OK || !genesis_record_valid(&decoded, signer_digest))
            return status == LXP_OK ? LXP_ERR_NON_CANONICAL : status;
        if (is_active) active = value;
        else history = value;
    }
    if (active == NULL || history == NULL ||
        memcmp(active->value, history->value,
               PROGRAMS_FEE_RECORD_BYTES) != 0)
        return LXP_ERR_UNKNOWN_FIELD;
    return LXP_OK;
}

static lxp_result active_record(lxp_module_ctx *ctx,
                                programs_fee_record *record)
{
    const uint8_t *encoded;
    size_t length;
    lxp_result status;
    if (ctx == NULL || record == NULL || ctx->module_id != LXP_MODULE_PROGRAMS)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_kv_get(ctx, fee_active_key,
                            sizeof(fee_active_key) - 1U, &encoded, &length);
    return status == LXP_OK ? record_decode(encoded, length, record) : status;
}

static lxp_result pending_record(lxp_module_ctx *ctx,
                                 programs_fee_pending *pending)
{
    const uint8_t *encoded;
    size_t length;
    lxp_result status;
    if (ctx == NULL || pending == NULL || ctx->module_id != LXP_MODULE_PROGRAMS)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_kv_get(ctx, fee_pending_key,
                            sizeof(fee_pending_key) - 1U, &encoded, &length);
    return status == LXP_OK ? pending_decode(encoded, length, pending) : status;
}

static lxp_result verified_governance_receipt(
    lxp_module_ctx *ctx, const lxp_receipt *receipt,
    const uint8_t proposal[PROGRAMS_FEE_PROPOSAL_BYTES], uint8_t digest[32])
{
    lxp_verified_receipt_facts facts;
    size_t arena_mark;
    lxp_result status;
    lxp_result reset_status;
    if (ctx == NULL || receipt == NULL || proposal == NULL || digest == NULL ||
        ctx->arena == NULL ||
        ctx->module_id != LXP_MODULE_PROGRAMS || ctx->global_sequence == 0U ||
        receipt->module_id != LXP_MODULE_GOVERNANCE ||
        receipt->result_code != LXP_OK || receipt->global_sequence == 0U ||
        receipt->global_sequence >= ctx->global_sequence ||
        receipt->timestamp == 0U || receipt->program_outcome.present ||
        lxp_ct_is_zero(receipt->resulting_state_root, 32U))
        return LXP_ERR_AUTH_SCOPE;
    arena_mark = lxp_arena_mark(ctx->arena);
    status = lxp_receipt_digest(receipt, ctx->arena, digest);
    reset_status = lxp_arena_reset(ctx->arena, arena_mark);
    if (reset_status != LXP_OK) return LXP_FATAL_INVARIANT;
    if (status != LXP_OK) return status;
    status = lxp_ctx_verified_receipt_facts(ctx, digest, &facts);
    if (status != LXP_OK) return status;
    if (facts.result_code != LXP_OK ||
        facts.global_sequence != receipt->global_sequence ||
        facts.timestamp != receipt->timestamp ||
        lxp_ct_memcmp(facts.receipt_digest, digest, 32U) != 0 ||
        lxp_ct_memcmp(facts.resulting_state_root,
                      receipt->resulting_state_root, 32U) != 0)
        return LXP_ERR_ROOT_MISMATCH;
    return governance_receipt_commits_proposal(receipt, proposal);
}

typedef struct receipt_reuse_check {
    const uint8_t *digest;
    bool found;
} receipt_reuse_check;

static lxp_result receipt_reuse_visit(
    const uint8_t *key, size_t key_length, const uint8_t *value,
    size_t value_length, void *user)
{
    receipt_reuse_check *check = (receipt_reuse_check *)user;
    programs_fee_record record;
    lxp_result status;
    if (key == NULL || value == NULL || check == NULL ||
        check->digest == NULL ||
        key_length != sizeof(fee_history_prefix) - 1U + 4U)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    status = record_decode(value, value_length, &record);
    if (status != LXP_OK) return status;
    if (memcmp(key, fee_history_prefix,
               sizeof(fee_history_prefix) - 1U) != 0 ||
        read_u32(key + sizeof(fee_history_prefix) - 1U) !=
            record.schedule.version)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    if (lxp_ct_memcmp(record.governance_receipt_digest,
                      check->digest, 32U) == 0)
        check->found = true;
    return LXP_OK;
}

static lxp_result governance_receipt_unused(lxp_module_ctx *ctx,
                                            const uint8_t digest[32])
{
    receipt_reuse_check check = {digest, false};
    lxp_result status = lxp_ctx_kv_iter(
        ctx, fee_history_prefix, sizeof(fee_history_prefix) - 1U,
        receipt_reuse_visit, &check);
    if (status != LXP_OK) return status;
    return check.found ? LXP_ERR_SEQUENCE_REUSED : LXP_OK;
}

lxp_result lxp_programs_fee_governance_decode(
    lxp_module_ctx *ctx, const uint8_t *payload, size_t payload_length,
    void **decoded)
{
    programs_fee_governance_activity *value;
    programs_fee_demand_policy demand;
    lx_programs_fee_schedule proposed;
    uint8_t canonical[PROGRAMS_FEE_PROPOSAL_BYTES];
    uint32_t receipt_length;
    void *allocation;
    lxp_result status;
    if (ctx == NULL || decoded == NULL || payload == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (payload_length < PROGRAMS_FEE_PROPOSAL_BYTES + 4U)
        return LXP_ERR_TRUNCATED;
    receipt_length = read_u32(payload + PROGRAMS_FEE_PROPOSAL_BYTES);
    if (receipt_length == 0U ||
        (size_t)receipt_length !=
            payload_length - PROGRAMS_FEE_PROPOSAL_BYTES - 4U)
        return LXP_ERR_NON_CANONICAL;
    if (memcmp(payload, fee_proposal_magic, sizeof(fee_proposal_magic)) != 0)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&proposed, 0, sizeof(proposed));
    decode_prices(payload + sizeof(fee_proposal_magic), &proposed);
    decode_policy(payload + sizeof(fee_proposal_magic) + 56U + 32U,
                  &demand);
    status = proposal_encode(
        &proposed, payload + sizeof(fee_proposal_magic) + 56U, &demand,
        read_u64(payload + sizeof(fee_proposal_magic) + 56U + 32U + 48U),
        canonical);
    if (status != LXP_OK) return status;
    if (memcmp(canonical, payload, sizeof(canonical)) != 0)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value),
                                 _Alignof(programs_fee_governance_activity),
                                 &allocation);
    if (status != LXP_OK) return status;
    value = (programs_fee_governance_activity *)allocation;
    (void)memset(value, 0, sizeof(*value));
    value->proposed = proposed;
    (void)memcpy(value->occupancy_asset_id,
                 payload + sizeof(fee_proposal_magic) + 56U, 32U);
    value->demand = demand;
    value->activation_batch = read_u64(
        payload + sizeof(fee_proposal_magic) + 56U + 32U + 48U);
    status = lxp_receipt_decode(
        payload + PROGRAMS_FEE_PROPOSAL_BYTES + 4U,
        (size_t)receipt_length, true, &value->governance_receipt);
    if (status != LXP_OK) return status;
    *decoded = value;
    return LXP_OK;
}

lxp_result lxp_programs_fee_governance_validate(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded)
{
    const programs_fee_governance_activity *value =
        (const programs_fee_governance_activity *)decoded;
    uint8_t proposal[PROGRAMS_FEE_PROPOSAL_BYTES];
    uint8_t receipt_digest[32];
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL ||
        activity->activity_type != LX_PROGRAMS_FEE_GOVERNANCE)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_ct_is_zero(authority->principal, sizeof(authority->principal)))
        return LXP_ERR_AUTH_SCOPE;
    status = proposal_encode(&value->proposed, value->occupancy_asset_id,
                             &value->demand, value->activation_batch,
                             proposal);
    if (status == LXP_OK)
        status = verified_governance_receipt(
            ctx, &value->governance_receipt, proposal, receipt_digest);
    if (status != LXP_OK) return status;
    return lxp_ctx_charge_gas(
        ctx, PROGRAMS_FEE_PROPOSAL_BYTES +
                 (size_t)value->governance_receipt.effects.count);
}

lxp_result lxp_programs_fee_governance_execute(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects)
{
    const programs_fee_governance_activity *value =
        (const programs_fee_governance_activity *)decoded;
    (void)effects;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL ||
        activity->activity_type != LX_PROGRAMS_FEE_GOVERNANCE ||
        lxp_ct_is_zero(authority->principal, sizeof(authority->principal)))
        return LXP_ERR_NON_CANONICAL;
    return lxp_programs_fee_governance_stage(
        ctx, &value->proposed, value->occupancy_asset_id,
        value->demand.target_occupancy_byte_batches,
        value->demand.response_denominator,
        value->demand.maximum_change_numerator,
        value->demand.maximum_change_denominator,
        value->demand.minimum_fee_units_per_occupancy_byte_batch,
        value->demand.maximum_fee_units_per_occupancy_byte_batch,
        value->activation_batch, &value->governance_receipt);
}

static lxp_result maximum_price_change(
    uint64_t price, const programs_fee_demand_policy *policy,
    uint64_t *maximum_change)
{
    lxp_u128 quotient;
    lxp_u128 remainder;
    lxp_result status;
    if (!demand_policy_valid(policy) || maximum_change == NULL || price == 0U)
        return LXP_ERR_PARAMETER_BOUNDS;
    status = lxp_u128_mul_div_floor(
        (lxp_u128){0U, price},
        (lxp_u128){0U, policy->maximum_change_numerator},
        (lxp_u128){0U, policy->maximum_change_denominator},
        &quotient, &remainder);
    if (status != LXP_OK) return status;
    if (quotient.hi != 0U) return LXP_ERR_OVERFLOW;
    *maximum_change = quotient.lo;
    return LXP_OK;
}

static lxp_result price_change_within_bound(
    uint64_t previous, uint64_t proposed,
    const programs_fee_demand_policy *policy)
{
    uint64_t maximum_change;
    uint64_t change = previous > proposed ? previous - proposed :
                                             proposed - previous;
    lxp_result status = maximum_price_change(previous, policy,
                                              &maximum_change);
    if (status != LXP_OK) return status;
    return change <= maximum_change ? LXP_OK : LXP_ERR_PARAMETER_BOUNDS;
}

static int u256_compare(lxp_u256 left, lxp_u256 right)
{
    size_t index = 4U;
    while (index != 0U) {
        --index;
        if (left.words[index] != right.words[index])
            return left.words[index] < right.words[index] ? -1 : 1;
    }
    return 0;
}

static lxp_result capped_proportional_change(
    uint64_t current_price, lxp_u128 deviation, lxp_u128 response_divisor,
    uint64_t maximum_change, uint64_t *change)
{
    lxp_u256 observed_product;
    uint64_t lower = 0U;
    uint64_t upper = maximum_change;
    lxp_result status;
    if (current_price == 0U || lxp_u128_is_zero(response_divisor) ||
        maximum_change > current_price || change == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_mul((lxp_u128){0U, current_price}, deviation,
                          &observed_product);
    if (status != LXP_OK) return status;
    /* Cross-multiplication in u256 preserves full-range u128 occupancy while
     * the search remains bounded by the governed u64 movement cap. */
    while (lower < upper) {
        uint64_t distance = upper - lower;
        uint64_t candidate = lower + distance / 2U + distance % 2U;
        lxp_u256 required_product;
        status = lxp_u128_mul((lxp_u128){0U, candidate}, response_divisor,
                              &required_product);
        if (status != LXP_OK) return status;
        if (u256_compare(required_product, observed_product) <= 0)
            lower = candidate;
        else
            upper = candidate - 1U;
    }
    *change = lower;
    return LXP_OK;
}

/* Stages unversioned fee coefficients in Programs protocol state. Every price
 * is fee units per the resource named by lx_programs_fee_schedule; occupancy
 * target is byte-batches. The prior verified Governance receipt must contain
 * exactly one nonmonetary state effect with body LXFG1 || seven big-endian
 * prices || occupancy asset || six policy integers || activation batch. */
lxp_result lxp_programs_fee_governance_stage(
    lxp_module_ctx *ctx, const lx_programs_fee_schedule *proposed,
    const uint8_t occupancy_asset_id[32],
    uint64_t target_occupancy_byte_batches,
    uint64_t response_denominator,
    uint64_t maximum_change_numerator,
    uint64_t maximum_change_denominator,
    uint64_t minimum_fee_units_per_occupancy_byte_batch,
    uint64_t maximum_fee_units_per_occupancy_byte_batch,
    uint64_t activation_batch, const lxp_receipt *governance_receipt)
{
    programs_fee_pending pending;
    programs_fee_pending existing;
    programs_fee_record current;
    programs_fee_demand_policy demand;
    uint8_t encoded[PROGRAMS_FEE_PENDING_BYTES];
    uint8_t proposal[PROGRAMS_FEE_PROPOSAL_BYTES];
    uint8_t receipt_digest[32];
    lxp_result status;
    if (ctx == NULL || proposed == NULL || occupancy_asset_id == NULL ||
        !ctx->mutable || ctx->module_id != LXP_MODULE_PROGRAMS ||
        ctx->batch_number == 0U || proposed->version != 0U ||
        activation_batch <= ctx->batch_number)
        return LXP_ERR_NON_CANONICAL;
    demand = (programs_fee_demand_policy){
        target_occupancy_byte_batches,
        response_denominator,
        maximum_change_numerator,
        maximum_change_denominator,
        minimum_fee_units_per_occupancy_byte_batch,
        maximum_fee_units_per_occupancy_byte_batch
    };
    status = proposal_encode(proposed, occupancy_asset_id, &demand,
                             activation_batch, proposal);
    if (status != LXP_OK) return status;
    status = pending_record(ctx, &existing);
    if (status == LXP_OK) return LXP_ERR_SEQUENCE_REUSED;
    if (status != LXP_ERR_UNKNOWN_FIELD) return status;
    status = verified_governance_receipt(ctx, governance_receipt, proposal,
                                         receipt_digest);
    if (status == LXP_OK) {
        status = active_record(ctx, &current);
        if (status == LXP_OK && governance_receipt->global_sequence <=
                                current.governance_sequence)
            return LXP_ERR_SEQUENCE_MISMATCH;
        if (status == LXP_ERR_UNKNOWN_FIELD) status = LXP_OK;
    }
    if (status == LXP_OK)
        status = governance_receipt_unused(ctx, receipt_digest);
    if (status != LXP_OK) return status;
    (void)memset(&pending, 0, sizeof(pending));
    pending.proposed_schedule = *proposed;
    (void)memcpy(pending.occupancy_asset_id, occupancy_asset_id, 32U);
    pending.demand = demand;
    pending.activation_batch = activation_batch;
    pending.staged_batch = ctx->batch_number;
    pending.governance_sequence = governance_receipt->global_sequence;
    (void)memcpy(pending.governance_receipt_digest, receipt_digest, 32U);
    status = pending_encode(&pending, encoded);
    return status == LXP_OK ? lxp_ctx_kv_put(
        ctx, fee_pending_key, sizeof(fee_pending_key) - 1U,
        encoded, sizeof(encoded)) : status;
}

/* Reads the receipt-backed proposal visible in state before activation. The
 * returned schedule has version zero because consensus assigns its effective
 * version only at activation, after all intervening demand revisions. */
lxp_result lxp_programs_fee_governance_pending(
    lxp_module_ctx *ctx, lx_programs_fee_schedule *proposed,
    uint64_t *activation_batch, uint8_t governance_receipt_digest[32])
{
    programs_fee_pending pending;
    lxp_result status;
    if (proposed == NULL || activation_batch == NULL ||
        governance_receipt_digest == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = pending_record(ctx, &pending);
    if (status != LXP_OK) return status;
    *proposed = pending.proposed_schedule;
    *activation_batch = pending.activation_batch;
    (void)memcpy(governance_receipt_digest,
                 pending.governance_receipt_digest, 32U);
    return LXP_OK;
}

/* Activates exactly at the pending batch boundary and appends, rather than
 * replacing, the newly assigned schedule version. Missed activation is a
 * replay divergence; early activation remains not-yet-valid. */
lxp_result lxp_programs_fee_governance_activate(lxp_module_ctx *ctx,
                                                uint64_t batch_number)
{
    programs_fee_pending pending;
    programs_fee_record current;
    programs_fee_record next;
    uint8_t active_encoded[PROGRAMS_FEE_RECORD_BYTES];
    uint8_t history_encoded[PROGRAMS_FEE_RECORD_BYTES];
    uint8_t key[sizeof(fee_history_prefix) - 1U + 4U];
    const uint8_t *existing;
    size_t existing_length;
    lxp_result status;
    if (ctx == NULL || !ctx->mutable || ctx->module_id != LXP_MODULE_PROGRAMS ||
        batch_number == 0U || ctx->batch_number != batch_number)
        return LXP_ERR_NON_CANONICAL;
    status = pending_record(ctx, &pending);
    if (status != LXP_OK) return status;
    if (batch_number < pending.activation_batch) return LXP_ERR_NOT_YET_VALID;
    if (batch_number > pending.activation_batch)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    status = active_record(ctx, &current);
    if (status == LXP_OK) {
        if (current.schedule.version == UINT32_MAX)
            return LXP_ERR_OVERFLOW;
        if (pending.governance_sequence <= current.governance_sequence)
            return LXP_FATAL_REPLAY_DIVERGENCE;
        status = price_change_within_bound(
            current.schedule.occupancy_byte_batch,
            pending.proposed_schedule.occupancy_byte_batch,
            &current.demand);
        if (status != LXP_OK) return status;
    } else if (status != LXP_ERR_UNKNOWN_FIELD) {
        return status;
    }
    (void)memset(&next, 0, sizeof(next));
    next.schedule = pending.proposed_schedule;
    next.schedule.version = status == LXP_ERR_UNKNOWN_FIELD ? 1U :
                                                     current.schedule.version + 1U;
    (void)memcpy(next.occupancy_asset_id, pending.occupancy_asset_id, 32U);
    next.demand = pending.demand;
    next.activation_batch = batch_number;
    next.last_occupancy_batch = batch_number - 1U;
    next.governance_sequence = pending.governance_sequence;
    (void)memcpy(next.governance_receipt_digest,
                 pending.governance_receipt_digest, 32U);
    history_key(next.schedule.version, key);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &existing, &existing_length);
    if (status == LXP_OK) return LXP_FATAL_REPLAY_DIVERGENCE;
    if (status != LXP_ERR_UNKNOWN_FIELD) return status;
    status = record_encode(&next, history_encoded);
    if (status == LXP_OK) status = record_encode(&next, active_encoded);
    if (status == LXP_OK)
        status = lxp_ctx_kv_put(ctx, key, sizeof(key), history_encoded,
                                sizeof(history_encoded));
    if (status == LXP_OK)
        status = lxp_ctx_kv_put(ctx, fee_active_key,
                                sizeof(fee_active_key) - 1U,
                                active_encoded, sizeof(active_encoded));
    if (status == LXP_OK)
        status = lxp_ctx_kv_del(ctx, fee_pending_key,
                                sizeof(fee_pending_key) - 1U);
    return status;
}

static lxp_result kernel_history_record(
    const lxp_kernel *kernel, uint32_t version, programs_fee_record *record)
{
    uint8_t key[sizeof(fee_history_prefix) - 1U + 4U];
    size_t index;
    if (kernel == NULL || record == NULL || version == 0U ||
        kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV)
        return LXP_ERR_NON_CANONICAL;
    history_key(version, key);
    for (index = 0U; index < kernel->module_kv_count; ++index) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[index];
        if (entry->module_id == LXP_MODULE_PROGRAMS &&
            entry->key_length == sizeof(key) &&
            memcmp(entry->key, key, sizeof(key)) == 0)
            return record_decode(entry->value, entry->value_length, record);
    }
    return LXP_ERR_VERSION_UNSUPPORTED;
}

static lxp_result kernel_active_record(const lxp_kernel *kernel,
                                       programs_fee_record *record)
{
    size_t index;
    if (kernel == NULL || record == NULL ||
        kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < kernel->module_kv_count; ++index) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[index];
        if (entry->module_id == LXP_MODULE_PROGRAMS &&
            entry->key_length == sizeof(fee_active_key) - 1U &&
            memcmp(entry->key, fee_active_key,
                   sizeof(fee_active_key) - 1U) == 0)
            return record_decode(entry->value, entry->value_length, record);
    }
    return LXP_ERR_VERSION_UNSUPPORTED;
}

static const lxp_module_kv_entry *genesis_kernel_entry(
    const lxp_kernel *kernel, const uint8_t *key, size_t key_length)
{
    size_t index;
    if (kernel == NULL || key == NULL || key_length == 0U ||
        kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV)
        return NULL;
    for (index = 0U; index < kernel->module_kv_count; ++index) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[index];
        if (entry->module_id == LXP_MODULE_PROGRAMS &&
            entry->key_length == key_length &&
            memcmp(entry->key, key, key_length) == 0)
            return entry;
    }
    return NULL;
}

static int genesis_kernel_order(
    uint16_t module_id, const uint8_t *key, size_t key_length,
    const lxp_module_kv_entry *right)
{
    size_t common;
    int order;
    if (module_id < right->module_id) return -1;
    if (module_id > right->module_id) return 1;
    common = key_length < right->key_length ? key_length : right->key_length;
    order = memcmp(key, right->key, common);
    if (order != 0) return order;
    return key_length < right->key_length ? -1 :
           key_length > right->key_length ? 1 : 0;
}

static lxp_result genesis_kernel_insert(
    lxp_kernel *kernel, const uint8_t *key, size_t key_length,
    const uint8_t *value, size_t value_length)
{
    size_t location = 0U;
    if (kernel == NULL || key == NULL || key_length == 0U ||
        key_length > LXP_MODULE_MAX_KEY_BYTES || value == NULL ||
        value_length == 0U || value_length > LXP_MODULE_MAX_VALUE_BYTES ||
        kernel->module_kv_count == LXP_KERNEL_MAX_MODULE_KV)
        return LXP_ERR_LENGTH_LIMIT;
    while (location < kernel->module_kv_count &&
           genesis_kernel_order(LXP_MODULE_PROGRAMS, key, key_length,
                                &kernel->module_kv[location]) > 0)
        ++location;
    if (location < kernel->module_kv_count &&
        genesis_kernel_order(LXP_MODULE_PROGRAMS, key, key_length,
                             &kernel->module_kv[location]) == 0)
        return LXP_ERR_SEQUENCE_REUSED;
    (void)memmove(&kernel->module_kv[location + 1U],
                  &kernel->module_kv[location],
                  (kernel->module_kv_count - location) *
                      sizeof(kernel->module_kv[0]));
    (void)memset(&kernel->module_kv[location], 0,
                 sizeof(kernel->module_kv[location]));
    kernel->module_kv[location].module_id = LXP_MODULE_PROGRAMS;
    kernel->module_kv[location].key_length = (uint16_t)key_length;
    kernel->module_kv[location].value_length = (uint32_t)value_length;
    (void)memcpy(kernel->module_kv[location].key, key, key_length);
    (void)memcpy(kernel->module_kv[location].value, value, value_length);
    ++kernel->module_kv_count;
    return LXP_OK;
}

lxp_result lxp_programs_fee_genesis_project(
    const lxp_genesis_manifest *manifest, lxp_arena *arena,
    lxp_kernel *kernel)
{
    const lxp_genesis_module_value *active = NULL;
    const lxp_genesis_module_value *history = NULL;
    const lxp_module_kv_entry *existing_active;
    const lxp_module_kv_entry *existing_history;
    uint8_t key[sizeof(fee_history_prefix) - 1U + 4U];
    size_t index;
    lxp_result status;
    if (manifest == NULL || arena == NULL || kernel == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_genesis_verify_signature(manifest, arena);
    if (status == LXP_OK) status = lxp_programs_fee_genesis_validate(manifest);
    if (status != LXP_OK) return status;
    history_key(1U, key);
    for (index = 0U; index < manifest->module_value_count; ++index) {
        const lxp_genesis_module_value *value = &manifest->module_values[index];
        if (genesis_key_matches(value, fee_active_key,
                                sizeof(fee_active_key) - 1U))
            active = value;
        else if (genesis_key_matches(value, key, sizeof(key)))
            history = value;
    }
    if (active == NULL || history == NULL)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    existing_active = genesis_kernel_entry(
        kernel, fee_active_key, sizeof(fee_active_key) - 1U);
    existing_history = genesis_kernel_entry(kernel, key, sizeof(key));
    if ((existing_active == NULL) != (existing_history == NULL))
        return LXP_FATAL_REPLAY_DIVERGENCE;
    if (existing_active != NULL) {
        if (existing_active->value_length != active->value_length ||
            existing_history->value_length != history->value_length ||
            memcmp(existing_active->value, active->value,
                   active->value_length) != 0 ||
            memcmp(existing_history->value, history->value,
                   history->value_length) != 0)
            return LXP_FATAL_REPLAY_DIVERGENCE;
        return LXP_OK;
    }
    if (kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV - 2U)
        return LXP_ERR_LENGTH_LIMIT;
    status = genesis_kernel_insert(
        kernel, fee_active_key, sizeof(fee_active_key) - 1U,
        active->value, active->value_length);
    if (status == LXP_OK)
        status = genesis_kernel_insert(kernel, key, sizeof(key),
                                       history->value,
                                       history->value_length);
    return status;
}

/* Canonical replay selector: only the exact state-retained version is
 * returned. Unknown versions never fall back to the active node schedule. */
lxp_result lxp_programs_fee_schedule_at(
    lxp_module_ctx *ctx, uint32_t recorded_version,
    lx_programs_fee_schedule *schedule, uint8_t occupancy_asset_id[32])
{
    uint8_t key[sizeof(fee_history_prefix) - 1U + 4U];
    const uint8_t *encoded;
    size_t length;
    programs_fee_record record;
    lxp_result status;
    if (ctx == NULL || schedule == NULL || occupancy_asset_id == NULL ||
        recorded_version == 0U || ctx->module_id != LXP_MODULE_PROGRAMS)
        return LXP_ERR_NON_CANONICAL;
    history_key(recorded_version, key);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &encoded, &length);
    if (status == LXP_ERR_UNKNOWN_FIELD) return LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK) status = record_decode(encoded, length, &record);
    if (status != LXP_OK) return status;
    if (record.schedule.version != recorded_version)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    *schedule = record.schedule;
    (void)memcpy(occupancy_asset_id, record.occupancy_asset_id, 32U);
    return LXP_OK;
}

/* Returns the active protocol-state schedule, never a compiled default. */
lxp_result lxp_programs_fee_schedule_current(
    lxp_module_ctx *ctx, lx_programs_fee_schedule *schedule,
    uint8_t occupancy_asset_id[32])
{
    programs_fee_record record;
    lxp_result status;
    if (schedule == NULL || occupancy_asset_id == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = active_record(ctx, &record);
    if (status != LXP_OK) return status;
    *schedule = record.schedule;
    (void)memcpy(occupancy_asset_id, record.occupancy_asset_id, 32U);
    return LXP_OK;
}

static lxp_result validate_occupancy_receipt(
    lxp_module_ctx *ctx, const programs_fee_record *current,
    const lxp_programs_occupancy_receipt *receipt)
{
    uint64_t prices[PROGRAMS_FEE_PRICE_FIELDS];
    size_t index;
    if (ctx == NULL || current == NULL || receipt == NULL ||
        receipt->batch_number == 0U ||
        receipt->schedule_version != current->schedule.version ||
        receipt->batch_number != current->last_occupancy_batch + 1U ||
        lxp_ct_memcmp(receipt->occupancy_asset_id,
                      current->occupancy_asset_id, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    schedule_prices(&current->schedule, prices);
    for (index = 0U; index < PROGRAMS_FEE_PRICE_FIELDS; ++index)
        if (receipt->schedule_prices[index] != prices[index])
            return LXP_ERR_CONTEXT_MISMATCH;
    if (lxp_u128_is_zero(receipt->byte_batches) &&
        !lxp_u128_is_zero(receipt->fee_units))
        return LXP_ERR_CONTEXT_MISMATCH;
    return LXP_OK;
}

static lxp_result adjusted_occupancy_price(
    const programs_fee_record *current, lxp_u128 observed,
    uint64_t *price, uint64_t *maximum_change, uint64_t *applied_change)
{
    lxp_u128 target;
    lxp_u128 deviation;
    lxp_u128 response_divisor;
    lxp_u128 response_remainder;
    lxp_u128 maximum;
    lxp_u128 maximum_remainder;
    uint64_t change;
    uint64_t proposed;
    lxp_result status;
    if (!record_valid(current) || price == NULL || maximum_change == NULL ||
        applied_change == NULL)
        return LXP_ERR_NON_CANONICAL;
    target = (lxp_u128){0U, current->demand.target_occupancy_byte_batches};
    if (lxp_u128_cmp(observed, target) >= 0)
        status = lxp_u128_sub(observed, target, &deviation);
    else
        status = lxp_u128_sub(target, observed, &deviation);
    if (status != LXP_OK) return status;
    status = lxp_u128_mul_div_floor(
        target, (lxp_u128){0U, current->demand.response_denominator},
        (lxp_u128){0U, 1U}, &response_divisor, &response_remainder);
    if (status == LXP_OK)
        status = lxp_u128_mul_div_floor(
            (lxp_u128){0U, current->schedule.occupancy_byte_batch},
            (lxp_u128){0U, current->demand.maximum_change_numerator},
            (lxp_u128){0U, current->demand.maximum_change_denominator},
            &maximum, &maximum_remainder);
    if (status != LXP_OK) return status;
    if (maximum.hi != 0U) return LXP_ERR_OVERFLOW;
    *maximum_change = maximum.lo;
    status = capped_proportional_change(
        current->schedule.occupancy_byte_batch, deviation,
        response_divisor, maximum.lo, &change);
    if (status != LXP_OK) return status;
    proposed = current->schedule.occupancy_byte_batch;
    if (lxp_u128_cmp(observed, target) > 0) {
        uint64_t room = current->demand
            .maximum_fee_units_per_occupancy_byte_batch - proposed;
        proposed += change > room ? room : change;
    } else if (lxp_u128_cmp(observed, target) < 0) {
        uint64_t room = proposed - current->demand
            .minimum_fee_units_per_occupancy_byte_batch;
        proposed -= change > room ? room : change;
    }
    *applied_change = proposed > current->schedule.occupancy_byte_batch ?
        proposed - current->schedule.occupancy_byte_batch :
        current->schedule.occupancy_byte_batch - proposed;
    if (*applied_change > *maximum_change)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    *price = proposed;
    return LXP_OK;
}

/* Consumes the canonical occupancy receipt for one completed protocol batch
 * and derives the price effective for the next batch. Only integer protocol
 * evidence is used, and the postcondition rechecks the governed movement cap. */
lxp_result lxp_programs_fee_governance_observe_batch(
    lxp_module_ctx *ctx, const lxp_programs_occupancy_receipt *receipt)
{
    programs_fee_record current;
    programs_fee_record next;
    programs_fee_pending pending;
    uint8_t active_encoded[PROGRAMS_FEE_RECORD_BYTES];
    uint8_t history_encoded[PROGRAMS_FEE_RECORD_BYTES];
    uint8_t key[sizeof(fee_history_prefix) - 1U + 4U];
    const uint8_t *existing;
    size_t existing_length;
    uint64_t price;
    uint64_t maximum_change;
    uint64_t applied_change;
    bool schedule_changed = false;
    lxp_result status;
    if (ctx == NULL || receipt == NULL || !ctx->mutable ||
        ctx->module_id != LXP_MODULE_PROGRAMS ||
        ctx->batch_number != receipt->batch_number ||
        receipt->batch_number == UINT64_MAX)
        return LXP_ERR_NON_CANONICAL;
    status = active_record(ctx, &current);
    if (status == LXP_OK)
        status = validate_occupancy_receipt(ctx, &current, receipt);
    if (status == LXP_OK)
        status = adjusted_occupancy_price(
            &current, receipt->byte_batches, &price,
            &maximum_change, &applied_change);
    if (status != LXP_OK) return status;
    (void)maximum_change;
    next = current;
    next.last_occupancy_batch = receipt->batch_number;
    next.observed_occupancy_byte_batches = receipt->byte_batches;
    (void)memset(&pending, 0, sizeof(pending));
    status = pending_record(ctx, &pending);
    if (status == LXP_OK && pending.activation_batch == receipt->batch_number + 1U) {
        if (current.schedule.version == UINT32_MAX ||
            pending.governance_sequence <= current.governance_sequence)
            return LXP_FATAL_REPLAY_DIVERGENCE;
        next.schedule = pending.proposed_schedule;
        next.schedule.version = current.schedule.version + 1U;
        (void)memcpy(next.occupancy_asset_id, pending.occupancy_asset_id, 32U);
        next.demand = pending.demand;
        next.activation_batch = pending.activation_batch;
        next.governance_sequence = pending.governance_sequence;
        schedule_changed = true;
        (void)memcpy(next.governance_receipt_digest,
                     pending.governance_receipt_digest, 32U);
        price = next.schedule.occupancy_byte_batch;
        applied_change = price > current.schedule.occupancy_byte_batch ?
            price - current.schedule.occupancy_byte_batch :
            current.schedule.occupancy_byte_batch - price;
        status = price_change_within_bound(
            current.schedule.occupancy_byte_batch, price, &current.demand);
        if (status != LXP_OK) return status;
    } else if (status == LXP_OK) {
        if (pending.activation_batch <= receipt->batch_number)
            return LXP_FATAL_REPLAY_DIVERGENCE;
        status = LXP_ERR_UNKNOWN_FIELD;
    }
    if (status != LXP_ERR_UNKNOWN_FIELD) {
        if (status != LXP_OK) return status;
    } else {
        if (applied_change != 0U) {
            next.schedule.version = current.schedule.version + 1U;
            next.schedule.occupancy_byte_batch = price;
            next.activation_batch = receipt->batch_number + 1U;
            schedule_changed = true;
        }
        status = LXP_OK;
    }
    if (schedule_changed) {
        if (current.schedule.version == UINT32_MAX) return LXP_ERR_OVERFLOW;
        history_key(next.schedule.version, key);
        status = lxp_ctx_kv_get(ctx, key, sizeof(key), &existing,
                                &existing_length);
        if (status == LXP_OK) return LXP_FATAL_REPLAY_DIVERGENCE;
        if (status != LXP_ERR_UNKNOWN_FIELD) return status;
        status = record_encode(&next, history_encoded);
        if (status == LXP_OK)
            status = lxp_ctx_kv_put(ctx, key, sizeof(key), history_encoded,
                                    sizeof(history_encoded));
        if (status != LXP_OK) return status;
    }
    if (pending.activation_batch == receipt->batch_number + 1U &&
        status == LXP_OK)
        status = lxp_ctx_kv_del(ctx, fee_pending_key,
                                sizeof(fee_pending_key) - 1U);
    if (status != LXP_OK) return status;
    status = record_encode(&next, active_encoded);
    return status == LXP_OK ? lxp_ctx_kv_put(
        ctx, fee_active_key, sizeof(fee_active_key) - 1U,
        active_encoded, sizeof(active_encoded)) : status;
}

/* Exact-version protocol-state selector for replay and admission integrations.
 * `version` is receipt-recorded and never replaced by the active version. */
lxp_result lxp_programs_fee_governance_resolve_runtime(
    void *context, uint32_t version, lx_programs_fee_schedule *schedule,
    uint8_t occupancy_asset_id[32])
{
    programs_fee_record record;
    lxp_result status;
    if (schedule == NULL || occupancy_asset_id == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = version == 0U ?
        kernel_active_record((const lxp_kernel *)context, &record) :
        kernel_history_record((const lxp_kernel *)context, version, &record);
    if (status != LXP_OK) return status;
    if (version != 0U && record.schedule.version != version)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    *schedule = record.schedule;
    (void)memcpy(occupancy_asset_id, record.occupancy_asset_id, 32U);
    return LXP_OK;
}
