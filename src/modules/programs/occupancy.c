#include "occupancy.h"
#include "occupancy_evidence.h"
#include "storage.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_fee.h"
#include "layerx/lxp_hash.h"

#include <limits.h>
#include <string.h>

enum {
    OCCUPANCY_CHUNK_BYTES = 1000,
    OCCUPANCY_HEAD_BYTES = 175,
    OCCUPANCY_LEDGER_KEY_BYTES = 15,
    OCCUPANCY_SCHEDULE_BYTES = 4 + 7 * 8 + 32,
    OCCUPANCY_PAYER_BYTES = 32 + 16 + 16 + 16 + 1
};

static const uint8_t occupancy_head_key[] = "progocc/head/v3";
static const uint8_t occupancy_final_key[] = "progocc/final/v3";
static const uint8_t occupancy_accounts_key[] = "progocc/accounts/v1";
static const uint8_t occupancy_ledger_prefix[] = "progocc/l/v3/";
static const uint8_t occupancy_receipt_domain[] =
    "LXP/programs/occupancy-receipt/v2\0";
#define OCCUPANCY_RECEIPT_DOMAIN_BYTES \
    (sizeof(occupancy_receipt_domain) - 1U)

static uint32_t read_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | (uint32_t)bytes[3];
}

static uint64_t read_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

static void write_u16(uint8_t *bytes, uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
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
    size_t i;
    for (i = 0U; i < 8U; ++i)
        bytes[i] = (uint8_t)(value >> (56U - 8U * i));
}

static void schedule_prices(const lx_programs_fee_schedule *schedule,
                            uint64_t prices[7])
{
    prices[0] = schedule->cpu;
    prices[1] = schedule->memory_byte;
    prices[2] = schedule->storage_read_byte;
    prices[3] = schedule->storage_write_byte;
    prices[4] = schedule->output_value;
    prices[5] = schedule->output_byte;
    prices[6] = schedule->occupancy_byte_batch;
}

static lxp_result resolve_parameters(
    const lx_programs_transfer_runtime *runtime, uint32_t parameter_version,
    lx_programs_fee_schedule *schedule, uint8_t asset_id[32])
{
    lxp_result status;
    if (runtime == NULL || schedule == NULL || asset_id == NULL ||
        parameter_version == 0U ||
        runtime->resolve_occupancy_parameters == NULL)
        return LXP_ERR_MODULE_DISABLED;
    (void)memset(schedule, 0, sizeof(*schedule));
    (void)memset(asset_id, 0, 32U);
    status = runtime->resolve_occupancy_parameters(
        runtime->occupancy_parameter_context, parameter_version,
        schedule, asset_id);
    if (status != LXP_OK) return status;
    if (schedule->version != parameter_version ||
        schedule->occupancy_byte_batch == 0U ||
        lxp_ct_is_zero(asset_id, 32U))
        return LXP_ERR_VERSION_UNSUPPORTED;
    return LXP_OK;
}

static void ledger_key(uint16_t index,
                       uint8_t key[OCCUPANCY_LEDGER_KEY_BYTES])
{
    (void)memcpy(key, occupancy_ledger_prefix,
                 sizeof(occupancy_ledger_prefix) - 1U);
    key[OCCUPANCY_LEDGER_KEY_BYTES - 2U] = (uint8_t)(index >> 8U);
    key[OCCUPANCY_LEDGER_KEY_BYTES - 1U] = (uint8_t)index;
}

static lxp_result load_ledger(lxp_programs_occupancy_bridge *bridge)
{
    const uint8_t *head;
    size_t head_length;
    uint32_t ledger_length;
    uint16_t chunks;
    uint8_t *ledger;
    uint32_t cursor = 0U;
    uint8_t digest[32];
    uint16_t index;
    lxp_result status = lxp_ctx_kv_get(
        bridge->ctx, occupancy_head_key, sizeof(occupancy_head_key) - 1U,
        &head, &head_length);
    if (status == LXP_ERR_UNKNOWN_FIELD) return LXP_OK;
    if (status != LXP_OK) return status;
    if (head_length != OCCUPANCY_HEAD_BYTES ||
        memcmp(head, "LXOC3", 5U) != 0)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    ledger_length = read_u32(head + 5U);
    chunks = (uint16_t)(((uint16_t)head[9] << 8U) | head[10]);
    if (ledger_length == 0U ||
        ledger_length > LXP_PROGRAMS_OCCUPANCY_MAX_LEDGER_BYTES ||
        chunks == 0U || chunks > LXP_PROGRAMS_OCCUPANCY_MAX_CHUNKS ||
        chunks != (ledger_length + OCCUPANCY_CHUNK_BYTES - 1U) /
                      OCCUPANCY_CHUNK_BYTES)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    status = lxp_ctx_arena_alloc(bridge->ctx, ledger_length, 1U,
                                 (void **)&ledger);
    if (status != LXP_OK) return status;
    for (index = 0U; index < chunks; ++index) {
        uint8_t key[OCCUPANCY_LEDGER_KEY_BYTES];
        const uint8_t *chunk;
        size_t chunk_length;
        size_t expected = ledger_length - cursor;
        if (expected > OCCUPANCY_CHUNK_BYTES) expected = OCCUPANCY_CHUNK_BYTES;
        ledger_key(index, key);
        status = lxp_ctx_kv_get(bridge->ctx, key, sizeof(key),
                                &chunk, &chunk_length);
        if (status != LXP_OK || chunk_length != expected)
            return LXP_FATAL_REPLAY_DIVERGENCE;
        (void)memcpy(ledger + cursor, chunk, chunk_length);
        cursor += (uint32_t)chunk_length;
    }
    status = lxp_hash_sha256(ledger, ledger_length, digest);
    if (status != LXP_OK || lxp_ct_memcmp(digest, head + 111U, 32U) != 0)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    bridge->current_ledger = ledger;
    bridge->current_ledger_length = ledger_length;
    bridge->current_ledger_chunks = chunks;
    bridge->current_batch = read_u64(head + 11U);
    bridge->current_schedule_version = read_u32(head + 19U);
    for (index = 0U; index < 7U; ++index)
        bridge->current_schedule_prices[index] =
            read_u64(head + 23U + (size_t)index * 8U);
    (void)memcpy(bridge->current_asset_id, head + 79U, 32U);
    if (bridge->current_batch == 0U ||
        bridge->current_schedule_version == 0U ||
        bridge->current_schedule_prices[6] == 0U ||
        lxp_ct_is_zero(bridge->current_asset_id, 32U))
        return LXP_FATAL_REPLAY_DIVERGENCE;
    return LXP_OK;
}

typedef struct activation_sum {
    uint64_t persistent_bytes;
} activation_sum;

static lxp_result activation_cell(
    void *user, const uint8_t *key, uint16_t key_length,
    const uint8_t *value, uint32_t value_length)
{
    activation_sum *sum = (activation_sum *)user;
    uint64_t cell_bytes;
    (void)value;
    if (sum == NULL || key == NULL || key_length == 0U)
        return LXP_FATAL_INVARIANT;
    cell_bytes = (uint64_t)key_length + (uint64_t)value_length;
    if (UINT64_MAX - sum->persistent_bytes < cell_bytes)
        return LXP_ERR_OVERFLOW;
    sum->persistent_bytes += cell_bytes;
    return LXP_OK;
}

static lxp_result activation_visit(
    const uint8_t *key, size_t key_length, const uint8_t *value,
    size_t value_length, void *user)
{
    lxp_programs_occupancy_bridge *bridge =
        (lxp_programs_occupancy_bridge *)user;
    lxp_programs_occupancy_activation_position *position;
    activation_sum sum = {0U};
    const uint8_t *owner_record;
    size_t owner_record_length;
    uint8_t owner_key[40] = {'p','r','o','g','r','a','m',0};
    uint8_t namespace_length;
    lxp_result status;
    (void)value;
    if (bridge == NULL || key == NULL || value == NULL ||
        value_length != 38U || key_length < 8U ||
        memcmp(key, "progstor", 8U) != 0 ||
        (key_length != 41U && key_length != 73U) ||
        bridge->activation_count == LXP_PROGRAMS_OCCUPANCY_MAX_POSITIONS)
        return LXP_FATAL_INVARIANT;
    namespace_length = (uint8_t)(key_length - 8U);
    if (lxp_ct_is_zero(key + 8U, 32U) ||
        (namespace_length == 33U && key[40] != 1U) ||
        (namespace_length == 65U &&
         (key[40] != 0U || lxp_ct_is_zero(key + 41U, 32U))))
        return LXP_FATAL_INVARIANT;
    position = &bridge->activation_positions[bridge->activation_count];
    position->namespace_length = namespace_length;
    (void)memcpy(position->namespace_bytes, key + 8U, namespace_length);
    if (namespace_length == 65U) {
        (void)memcpy(position->payer, key + 41U, 32U);
    } else {
        (void)memcpy(owner_key + 8U, key + 8U, 32U);
        status = lxp_ctx_kv_get(bridge->ctx, owner_key, sizeof(owner_key),
                                &owner_record, &owner_record_length);
        if (status != LXP_OK) return status;
        if (owner_record_length != 71U ||
            lxp_ct_is_zero(owner_record + 1U, 32U))
            return LXP_FATAL_INVARIANT;
        (void)memcpy(position->payer, owner_record + 1U, 32U);
    }
    status = lxp_programs_storage_import(
        bridge->ctx, position->namespace_bytes, namespace_length,
        activation_cell, &sum);
    if (status != LXP_OK) return status;
    if (sum.persistent_bytes == 0U) return LXP_FATAL_INVARIANT;
    position->persistent_bytes = sum.persistent_bytes;
    ++bridge->activation_count;
    return LXP_OK;
}

static lxp_result load_activation_positions(
    lxp_programs_occupancy_bridge *bridge)
{
    static const uint8_t prefix[] = "progstor";
    return lxp_ctx_kv_iter(bridge->ctx, prefix, sizeof(prefix) - 1U,
                           activation_visit, bridge);
}

lxp_result lxp_programs_occupancy_bridge_init(
    lxp_programs_occupancy_bridge *bridge, lxp_module_ctx *ctx)
{
    if (bridge == NULL || ctx == NULL ||
        ctx->module_id != LXP_MODULE_PROGRAMS || !ctx->mutable)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(bridge, 0, sizeof(*bridge));
    bridge->ctx = ctx;
    {
        const uint8_t *final;
        size_t final_length;
        lxp_result status = lxp_ctx_kv_get(
            ctx, occupancy_final_key, sizeof(occupancy_final_key) - 1U,
            &final, &final_length);
        if (status == LXP_OK) {
            if (final_length != 84U) return LXP_FATAL_REPLAY_DIVERGENCE;
            bridge->finalized_batch = read_u64(final);
            bridge->global_sequence = read_u64(final + 8U);
        } else if (status == LXP_ERR_UNKNOWN_FIELD) {
            bridge->uninitialized = true;
        } else return status;
    }
    {
        lxp_result status = load_ledger(bridge);
        if (status == LXP_OK && bridge->current_ledger_length == 0U)
            status = load_activation_positions(bridge);
        if (status == LXP_OK && bridge->current_ledger_length != 0U &&
            bridge->uninitialized) {
            if (bridge->current_batch == 0U)
                status = LXP_FATAL_REPLAY_DIVERGENCE;
            else {
                bridge->finalized_batch = bridge->current_batch - 1U;
                bridge->uninitialized = false;
            }
        }
        if (status == LXP_OK && bridge->current_ledger_length != 0U &&
            bridge->current_batch != bridge->finalized_batch &&
            (bridge->finalized_batch == UINT64_MAX ||
             bridge->current_batch != bridge->finalized_batch + 1U))
            status = LXP_FATAL_REPLAY_DIVERGENCE;
        return status;
    }
}

lxp_result lxp_programs_occupancy_bind_call(
    lxp_programs_occupancy_bridge *bridge, const uint8_t root_program[32],
    const uint64_t budget[LX_PROGRAMS_CALL_BUDGET_FIELDS])
{
    const lxp_call_admission_facts *admission;
    lxp_u128 execution_ceiling = {0U, 0U};
    size_t index;
    if (bridge == NULL || bridge->ctx == NULL || root_program == NULL ||
        budget == NULL || bridge->call_authorized || bridge->begun ||
        lxp_ct_is_zero(root_program, 32U))
        return LXP_ERR_NON_CANONICAL;
    admission = lxp_ctx_call_admission(bridge->ctx);
    if (admission == NULL || !admission->present ||
        lxp_ct_is_zero(admission->payer, 32U) ||
        lxp_ct_is_zero(admission->activity_binding, 32U))
        return LXP_FATAL_INVARIANT;
    for (index = 0U; index < 6U; ++index) {
        lxp_u256 product;
        lxp_u128 component;
        lxp_result status = lxp_u128_mul(
            (lxp_u128){0U, budget[index]},
            (lxp_u128){0U, admission->fee_schedule_prices[index]}, &product);
        if (status != LXP_OK || product.words[2] != 0U ||
            product.words[3] != 0U)
            return status == LXP_OK ? LXP_ERR_OVERFLOW : status;
        component = (lxp_u128){product.words[1], product.words[0]};
        status = lxp_u128_add(execution_ceiling, component,
                              &execution_ceiling);
        if (status != LXP_OK) return status;
    }
    if (lxp_u128_sub(admission->signed_fee_limit, execution_ceiling,
                     &bridge->authorized_responsibility_ceiling) != LXP_OK)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    (void)memcpy(bridge->authorized_root_program, root_program, 32U);
    (void)memcpy(bridge->authorized_payer, admission->payer, 32U);
    (void)memcpy(bridge->authorized_activity_binding,
                 admission->activity_binding, 32U);
    bridge->call_authorized = true;
    return LXP_OK;
}

lxp_result layerx_programs_occupancy_activation_count(uint64_t token)
{
    const lxp_programs_occupancy_bridge *bridge =
        (const lxp_programs_occupancy_bridge *)(uintptr_t)token;
    if (bridge == NULL)
        return LXP_ERR_NON_CANONICAL;
    return (lxp_result)bridge->activation_count;
}

lxp_result layerx_programs_occupancy_activation_record_length(
    uint64_t token, uint16_t index)
{
    const lxp_programs_occupancy_bridge *bridge =
        (const lxp_programs_occupancy_bridge *)(uintptr_t)token;
    if (bridge == NULL || index >= bridge->activation_count)
        return LXP_ERR_TRUNCATED;
    return (lxp_result)(1U +
        bridge->activation_positions[index].namespace_length + 32U + 8U);
}

lxp_result layerx_programs_occupancy_activation_record_byte(
    uint64_t token, uint16_t index, uint16_t offset)
{
    const lxp_programs_occupancy_bridge *bridge =
        (const lxp_programs_occupancy_bridge *)(uintptr_t)token;
    const lxp_programs_occupancy_activation_position *position;
    uint16_t length;
    if (bridge == NULL || index >= bridge->activation_count)
        return LXP_ERR_TRUNCATED;
    position = &bridge->activation_positions[index];
    length = (uint16_t)(1U + position->namespace_length + 32U + 8U);
    if (offset >= length) return LXP_ERR_TRUNCATED;
    if (offset == 0U) return (lxp_result)position->namespace_length;
    --offset;
    if (offset < position->namespace_length)
        return (lxp_result)position->namespace_bytes[offset];
    offset = (uint16_t)(offset - position->namespace_length);
    if (offset < 32U) return (lxp_result)position->payer[offset];
    offset = (uint16_t)(offset - 32U);
    return (lxp_result)(uint8_t)(position->persistent_bytes >>
                                 (56U - 8U * offset));
}

lxp_result layerx_programs_occupancy_ledger_length(uint64_t token)
{
    const lxp_programs_occupancy_bridge *bridge =
        (const lxp_programs_occupancy_bridge *)(uintptr_t)token;
    if (bridge == NULL || bridge->ctx == NULL ||
        bridge->current_ledger_length > INT32_MAX)
        return LXP_ERR_NON_CANONICAL;
    return (lxp_result)bridge->current_ledger_length;
}

lxp_result layerx_programs_occupancy_ledger_byte(uint64_t token,
                                                 uint32_t offset)
{
    const lxp_programs_occupancy_bridge *bridge =
        (const lxp_programs_occupancy_bridge *)(uintptr_t)token;
    if (bridge == NULL || offset >= bridge->current_ledger_length)
        return LXP_ERR_TRUNCATED;
    return (lxp_result)bridge->current_ledger[offset];
}

lxp_result layerx_programs_occupancy_output_begin(
    uint64_t token, uint64_t batch_number, uint32_t parameter_version,
    uint32_t schedule_version,
    uint32_t ledger_length, uint32_t evidence_length, uint16_t payer_count,
    uint64_t byte_batches_hi, uint64_t byte_batches_lo,
    uint64_t fee_units_hi, uint64_t fee_units_lo,
    uint64_t paid_hi, uint64_t paid_lo,
    uint64_t arrears_hi, uint64_t arrears_lo)
{
    lxp_programs_occupancy_bridge *bridge =
        (lxp_programs_occupancy_bridge *)(uintptr_t)token;
    lx_programs_transfer_runtime *runtime;
    lx_programs_fee_schedule schedule;
    uint8_t asset_id[32];
    uint64_t prices[7];
    lxp_result status;
    if (bridge == NULL || bridge->ctx == NULL || bridge->begun ||
        batch_number == 0U || parameter_version == 0U ||
        ledger_length == 0U ||
        ledger_length > LXP_PROGRAMS_OCCUPANCY_MAX_LEDGER_BYTES ||
        evidence_length == 0U ||
        evidence_length > LXP_PROGRAMS_OCCUPANCY_MAX_EVIDENCE_BYTES ||
        payer_count > LXP_PROGRAMS_OCCUPANCY_MAX_PAYERS)
        return LXP_ERR_NON_CANONICAL;
    runtime = (lx_programs_transfer_runtime *)
        lxp_ctx_module_runtime(bridge->ctx);
    status = resolve_parameters(runtime, parameter_version, &schedule,
                                asset_id);
    if (status != LXP_OK) return status;
    status = lxp_state_store_bind_accounts(bridge->ctx->kernel->state,
                                           runtime->accounts);
    if (status == LXP_OK)
        status = lxp_state_journal_require_account_root(
            bridge->ctx->kernel->journal);
    if (status != LXP_OK) return status;
    if (schedule.version != schedule_version)
        return LXP_ERR_VERSION_UNSUPPORTED;
    if (bridge->uninitialized) {
        bridge->finalized_batch = batch_number - 1U;
        bridge->current_batch = bridge->finalized_batch;
        bridge->uninitialized = false;
    }
    if (bridge->finalized_batch == UINT64_MAX ||
        batch_number != bridge->finalized_batch + 1U ||
        batch_number < bridge->current_batch)
        return LXP_ERR_BATCH_GAP;
    schedule_prices(&schedule, prices);
    if (bridge->current_ledger_length != 0U) {
        if (lxp_ct_memcmp(bridge->current_asset_id, asset_id, 32U) != 0)
            return LXP_ERR_CONTEXT_MISMATCH;
        if (bridge->current_schedule_version == schedule_version) {
            if (memcmp(bridge->current_schedule_prices, prices,
                       sizeof(prices)) != 0)
                return LXP_ERR_CONTEXT_MISMATCH;
        } else if (bridge->current_batch == batch_number ||
                   bridge->current_schedule_version == UINT32_MAX ||
                   schedule_version != bridge->current_schedule_version + 1U) {
            return LXP_ERR_VERSION_UNSUPPORTED;
        }
    }
    status = lxp_ctx_arena_alloc(bridge->ctx, ledger_length, 1U,
                                 (void **)&bridge->next_ledger);
    if (status == LXP_OK)
        status = lxp_ctx_arena_alloc(bridge->ctx, evidence_length, 1U,
                                     (void **)&bridge->evidence);
    if (status != LXP_OK) return status;
    bridge->batch_number = batch_number;
    bridge->global_sequence = lxp_ctx_global_sequence(bridge->ctx);
    bridge->parameter_version = parameter_version;
    bridge->schedule_version = schedule_version;
    (void)memcpy(bridge->resolved_schedule_prices, prices, sizeof(prices));
    (void)memcpy(bridge->resolved_asset_id, asset_id, 32U);
    bridge->next_ledger_length = ledger_length;
    bridge->evidence_length = evidence_length;
    bridge->payer_count = payer_count;
    bridge->byte_batches = (lxp_u128){byte_batches_hi, byte_batches_lo};
    bridge->fee_units = (lxp_u128){fee_units_hi, fee_units_lo};
    bridge->paid_fee_units = (lxp_u128){paid_hi, paid_lo};
    bridge->arrears_fee_units = (lxp_u128){arrears_hi, arrears_lo};
    bridge->begun = true;
    return LXP_OK;
}

lxp_result layerx_programs_occupancy_output_payer(
    uint64_t token, uint16_t index,
    uint64_t p0, uint64_t p1, uint64_t p2, uint64_t p3,
    uint64_t due_hi, uint64_t due_lo,
    uint64_t paid_hi, uint64_t paid_lo,
    uint64_t arrears_hi, uint64_t arrears_lo, uint8_t frozen)
{
    lxp_programs_occupancy_bridge *bridge =
        (lxp_programs_occupancy_bridge *)(uintptr_t)token;
    lxp_programs_occupancy_payer *payer;
    lxp_u128 accounted;
    uint64_t words[4] = {p0, p1, p2, p3};
    size_t word;
    lxp_result status;
    if (bridge == NULL || !bridge->begun || bridge->applied ||
        index != bridge->payers_written || index >= bridge->payer_count ||
        frozen > 1U || (due_hi == 0U && due_lo == 0U))
        return LXP_ERR_NON_CANONICAL;
    payer = &bridge->payers[index];
    for (word = 0U; word < 4U; ++word)
        write_u64(payer->principal + word * 8U, words[word]);
    if (lxp_ct_is_zero(payer->principal, 32U) ||
        (index != 0U && memcmp(bridge->payers[index - 1U].principal,
                               payer->principal, 32U) >= 0))
        return LXP_ERR_NON_CANONICAL;
    payer->due = (lxp_u128){due_hi, due_lo};
    payer->paid = (lxp_u128){paid_hi, paid_lo};
    payer->arrears = (lxp_u128){arrears_hi, arrears_lo};
    payer->frozen = frozen != 0U;
    status = lxp_u128_add(payer->paid, payer->arrears, &accounted);
    if (status != LXP_OK || lxp_u128_cmp(accounted, payer->due) != 0 ||
        payer->frozen != !lxp_u128_is_zero(payer->arrears))
        return LXP_ERR_NON_CANONICAL;
    ++bridge->payers_written;
    return LXP_OK;
}

lxp_result layerx_programs_occupancy_output_byte(
    uint64_t token, uint16_t section, uint32_t offset, uint8_t byte)
{
    lxp_programs_occupancy_bridge *bridge =
        (lxp_programs_occupancy_bridge *)(uintptr_t)token;
    uint8_t *bytes;
    uint32_t length;
    uint32_t *written;
    if (bridge == NULL || !bridge->begun || bridge->applied)
        return LXP_ERR_NON_CANONICAL;
    if (section == 0U) {
        bytes = bridge->next_ledger;
        length = bridge->next_ledger_length;
        written = &bridge->next_ledger_written;
    } else if (section == 1U) {
        bytes = bridge->evidence;
        length = bridge->evidence_length;
        written = &bridge->evidence_written;
    } else return LXP_ERR_UNKNOWN_FIELD;
    if (offset != *written || offset >= length) return LXP_ERR_TRUNCATED;
    bytes[offset] = byte;
    ++*written;
    return LXP_OK;
}

static lx_account *ordinary_account(lx_account_registry *registry,
                                    const uint8_t principal[32])
{
    size_t index;
    if (registry == NULL) return NULL;
    for (index = 0U; index < registry->count; ++index)
        if (registry->accounts[index].kind == LX_ACCOUNT_AGENT_MAIN &&
            memcmp(registry->accounts[index].id, principal, 32U) == 0)
            return &registry->accounts[index];
    return NULL;
}

lxp_result layerx_programs_occupancy_payer_available(
    uint64_t token, uint64_t p0, uint64_t p1, uint64_t p2, uint64_t p3,
    uint64_t fee_hi, uint64_t fee_lo)
{
    lxp_programs_occupancy_bridge *bridge =
        (lxp_programs_occupancy_bridge *)(uintptr_t)token;
    lx_programs_transfer_runtime *runtime;
    lx_account *account;
    uint8_t principal[32];
    uint64_t words[4] = {p0, p1, p2, p3};
    lxp_u128 balance;
    size_t index;
    lxp_result status;
    if (bridge == NULL || bridge->ctx == NULL ||
        (fee_hi == 0U && fee_lo == 0U))
        return LXP_ERR_NON_CANONICAL;
    runtime = (lx_programs_transfer_runtime *)
        lxp_ctx_module_runtime(bridge->ctx);
    if (runtime == NULL || runtime->accounts == NULL || runtime->assets == NULL ||
        runtime->asset_count == 0U) return LXP_ERR_MODULE_DISABLED;
    for (index = 0U; index < 4U; ++index)
        write_u64(principal + index * 8U, words[index]);
    account = ordinary_account(runtime->accounts, principal);
    if (account == NULL) return LXP_ERR_INSUFFICIENT_BALANCE;
    if (account->frozen) return LXP_ERR_INSUFFICIENT_BALANCE;
    {
        lx_account *treasury;
        size_t asset_index;
        status = lxp_fee_treasury_account(runtime->accounts, &treasury);
        if (status != LXP_OK) return status;
        if (treasury->frozen) return LXP_ERR_INSUFFICIENT_BALANCE;
        if (treasury->has_asset &&
            memcmp(treasury->asset_id, bridge->resolved_asset_id, 32U) != 0)
            return LXP_ERR_INSUFFICIENT_BALANCE;
        {
            lxp_u128 credited;
            if (lxp_u128_add(treasury->balance,
                             (lxp_u128){fee_hi, fee_lo},
                             &credited) != LXP_OK)
                return LXP_ERR_INSUFFICIENT_BALANCE;
        }
        for (asset_index = 0U; asset_index < runtime->asset_count; ++asset_index)
            if (memcmp(runtime->assets[asset_index].asset_id,
                       bridge->resolved_asset_id, 32U) == 0)
                break;
        if (asset_index == runtime->asset_count)
            return LXP_ERR_ASSET_MISMATCH;
        if (!runtime->assets[asset_index].registered)
            return LXP_ERR_ASSET_MISMATCH;
        if (runtime->assets[asset_index].paused)
            return LXP_ERR_INSUFFICIENT_BALANCE;
    }
    status = lxp_state_balance_get(account, bridge->resolved_asset_id,
                                   &balance);
    if (status != LXP_OK) return status == LXP_ERR_ASSET_MISMATCH ?
                                 LXP_ERR_INSUFFICIENT_BALANCE : status;
    return lxp_u128_cmp(balance, (lxp_u128){fee_hi, fee_lo}) < 0 ?
           LXP_ERR_INSUFFICIENT_BALANCE : LXP_OK;
}

static lxp_result apply_payer(
    lxp_programs_occupancy_bridge *bridge,
    const lxp_programs_occupancy_payer *payer,
    uint8_t aggregate_root[32])
{
    lx_programs_transfer_runtime *runtime =
        (lx_programs_transfer_runtime *)lxp_ctx_module_runtime(bridge->ctx);
    lx_account *from;
    lx_account *treasury;
    lxp_transfer_set set;
    lxp_receipt receipt;
    uint8_t material[64];
    lxp_result status;
    if (lxp_u128_is_zero(payer->paid)) return LXP_OK;
    if (runtime == NULL || runtime->accounts == NULL || runtime->assets == NULL ||
        runtime->asset_count == 0U ||
        lxp_ct_is_zero(bridge->resolved_asset_id, 32U))
        return LXP_ERR_MODULE_DISABLED;
    from = ordinary_account(runtime->accounts, payer->principal);
    if (from == NULL) return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    status = lxp_fee_treasury_account(runtime->accounts, &treasury);
    if (status != LXP_OK) return status;
    (void)memset(&set, 0, sizeof(set));
    set.leg_count = 1U;
    set.legs[0].from = from;
    set.legs[0].to = treasury;
    (void)memcpy(set.legs[0].asset_id, bridge->resolved_asset_id, 32U);
    set.legs[0].amount = payer->paid;
    set.legs[0].reason = LXP_REASON_STORAGE_OCCUPANCY;
    set.legs[0].supply_mode = LXP_TRANSFER_CONSERVED;
    set.context.assets = runtime->assets;
    set.context.asset_count = runtime->asset_count;
    (void)memcpy(set.context.authorized_from, payer->principal, 32U);
    set.context.protocol_system_capability = true;
    set.context.origin_module_id = LXP_MODULE_PROGRAMS;
    set.context.debit_authority_kind = LXP_AUTH_OCCUPANCY_RESPONSIBILITY;
    (void)memset(&receipt, 0, sizeof(receipt));
    status = lxp_ctx_emit_programs_maintenance_transfer_set(
        bridge->ctx, &set, &receipt);
    if (status != LXP_OK) return status;
    (void)memcpy(material, aggregate_root, 32U);
    (void)memcpy(material + 32U, receipt.transfer_set_root, 32U);
    return lxp_hash_domain(LXP_DOMAIN_TRANSFER_SET, material, sizeof(material),
                           aggregate_root);
}

static lxp_result persist_output(lxp_programs_occupancy_bridge *bridge,
                                 const uint8_t evidence_digest[32],
                                 const uint8_t aggregate_root[32])
{
    uint32_t computed_chunks;
    uint16_t chunks;
    uint16_t maximum_chunks;
    uint16_t index;
    uint32_t cursor = 0U;
    uint8_t head[OCCUPANCY_HEAD_BYTES];
    uint8_t ledger_digest[32];
    uint8_t schedule_bytes[OCCUPANCY_SCHEDULE_BYTES];
    uint64_t prices[7];
    lxp_result status;
    computed_chunks = (bridge->next_ledger_length +
                       OCCUPANCY_CHUNK_BYTES - 1U) / OCCUPANCY_CHUNK_BYTES;
    if (computed_chunks == 0U ||
        computed_chunks > LXP_PROGRAMS_OCCUPANCY_MAX_CHUNKS)
        return LXP_ERR_LENGTH_LIMIT;
    chunks = (uint16_t)computed_chunks;
    maximum_chunks = chunks > bridge->current_ledger_chunks ?
                     chunks : bridge->current_ledger_chunks;
    if ((uint32_t)maximum_chunks + 3U > LXP_MODULE_MAX_STAGED_WRITES)
        return LXP_ERR_ARENA_EXHAUSTED;
    for (index = 0U; index < chunks; ++index) {
        uint8_t key[OCCUPANCY_LEDGER_KEY_BYTES];
        size_t length = bridge->next_ledger_length - cursor;
        if (length > OCCUPANCY_CHUNK_BYTES) length = OCCUPANCY_CHUNK_BYTES;
        ledger_key(index, key);
        status = lxp_ctx_kv_put(bridge->ctx, key, sizeof(key),
                                bridge->next_ledger + cursor, length);
        if (status != LXP_OK) return status;
        cursor += (uint32_t)length;
    }
    for (index = chunks; index < bridge->current_ledger_chunks; ++index) {
        uint8_t key[OCCUPANCY_LEDGER_KEY_BYTES];
        ledger_key(index, key);
        status = lxp_ctx_kv_del(bridge->ctx, key, sizeof(key));
        if (status != LXP_OK) return status;
    }
    status = lxp_hash_sha256(bridge->next_ledger,
                             bridge->next_ledger_length, ledger_digest);
    if (status != LXP_OK) return status;
    {
        uint8_t account_root[32];
        status = lx_account_registry_root(
            ((lx_programs_transfer_runtime *)
                lxp_ctx_module_runtime(bridge->ctx))->accounts,
            account_root);
        if (status == LXP_OK)
            status = lxp_ctx_kv_put(
                bridge->ctx, occupancy_accounts_key,
                sizeof(occupancy_accounts_key) - 1U,
                account_root, sizeof(account_root));
        if (status != LXP_OK) return status;
    }
    (void)memcpy(prices, bridge->resolved_schedule_prices, sizeof(prices));
    write_u32(schedule_bytes, bridge->schedule_version);
    for (index = 0U; index < 7U; ++index)
        write_u64(schedule_bytes + 4U + (size_t)index * 8U, prices[index]);
    (void)memcpy(schedule_bytes + 60U, bridge->resolved_asset_id, 32U);
    (void)memset(head, 0, sizeof(head));
    (void)memcpy(head, "LXOC3", 5U);
    write_u32(head + 5U, bridge->next_ledger_length);
    write_u16(head + 9U, chunks);
    write_u64(head + 11U, bridge->batch_number);
    write_u32(head + 19U, bridge->schedule_version);
    for (index = 0U; index < 7U; ++index)
        write_u64(head + 23U + (size_t)index * 8U, prices[index]);
    (void)memcpy(head + 79U, bridge->resolved_asset_id, 32U);
    (void)memcpy(head + 111U, ledger_digest, 32U);
    (void)memcpy(head + 143U, evidence_digest, 32U);
    status = lxp_ctx_kv_put(bridge->ctx, occupancy_head_key,
                            sizeof(occupancy_head_key) - 1U,
                            head, sizeof(head));
    if (status != LXP_OK) return status;
    if (bridge->finalizing) {
        uint8_t final[84];
        write_u64(final, bridge->batch_number);
        write_u64(final + 8U, bridge->global_sequence);
        write_u32(final + 16U, bridge->parameter_version);
        (void)memcpy(final + 20U, evidence_digest, 32U);
        (void)memcpy(final + 52U, aggregate_root, 32U);
        status = lxp_ctx_kv_put(bridge->ctx, occupancy_final_key,
                                sizeof(occupancy_final_key) - 1U,
                                final, sizeof(final));
        if (status != LXP_OK) return status;
    }
    (void)memset(&bridge->receipt, 0, sizeof(bridge->receipt));
    bridge->receipt.batch_number = bridge->batch_number;
    bridge->receipt.global_sequence = bridge->global_sequence;
    bridge->receipt.parameter_version = bridge->parameter_version;
    bridge->receipt.schedule_version = bridge->schedule_version;
    (void)memcpy(bridge->receipt.schedule_prices, prices, sizeof(prices));
    (void)memcpy(bridge->receipt.occupancy_asset_id,
                 bridge->resolved_asset_id, 32U);
    bridge->receipt.byte_batches = bridge->byte_batches;
    bridge->receipt.fee_units = bridge->fee_units;
    bridge->receipt.paid_fee_units = bridge->paid_fee_units;
    bridge->receipt.arrears_fee_units = bridge->arrears_fee_units;
    bridge->receipt.payer_count = bridge->payer_count;
    for (index = 0U; index < bridge->payer_count; ++index) {
        lxp_programs_occupancy_payer_receipt *target =
            &bridge->receipt.payers[index];
        (void)memcpy(target->principal, bridge->payers[index].principal, 32U);
        target->due = bridge->payers[index].due;
        target->paid = bridge->payers[index].paid;
        target->arrears = bridge->payers[index].arrears;
        target->frozen = bridge->payers[index].frozen;
    }
    status = lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH, schedule_bytes,
                             sizeof(schedule_bytes),
                             bridge->receipt.schedule_commitment);
    if (status != LXP_OK) return status;
    bridge->receipt.settlement_evidence =
        (lxp_byte_span){bridge->evidence, bridge->evidence_length};
    (void)memcpy(bridge->receipt.settlement_evidence_digest,
                 evidence_digest, 32U);
    (void)memcpy(bridge->receipt.ledger_root, ledger_digest, 32U);
    (void)memcpy(bridge->receipt.transfer_set_root, aggregate_root, 32U);
    return LXP_OK;
}

lxp_result layerx_programs_occupancy_output_apply(uint64_t token)
{
    lxp_programs_occupancy_bridge *bridge =
        (lxp_programs_occupancy_bridge *)(uintptr_t)token;
    lxp_u128 paid_total = {0U, 0U};
    lxp_u128 arrears_total = {0U, 0U};
    uint8_t evidence_digest[32];
    uint8_t aggregate_root[32] = {0};
    uint16_t index;
    lxp_result status;
    if (bridge == NULL || !bridge->begun || bridge->applied ||
        bridge->next_ledger_written != bridge->next_ledger_length ||
        bridge->evidence_written != bridge->evidence_length ||
        bridge->payers_written != bridge->payer_count)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_programs_occupancy_validate_output(bridge);
    if (status != LXP_OK) return status;
    for (index = 0U; index < bridge->payer_count; ++index) {
        status = lxp_u128_add(paid_total, bridge->payers[index].paid,
                              &paid_total);
        if (status == LXP_OK)
            status = lxp_u128_add(arrears_total, bridge->payers[index].arrears,
                                  &arrears_total);
        if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    }
    if (lxp_u128_cmp(paid_total, bridge->paid_fee_units) != 0 ||
        lxp_u128_cmp(arrears_total, bridge->arrears_fee_units) != 0)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_hash_sha256(bridge->evidence, bridge->evidence_length,
                             evidence_digest);
    if (status != LXP_OK) return status;
    for (index = 0U; index < bridge->payer_count; ++index) {
        status = apply_payer(bridge, &bridge->payers[index], aggregate_root);
        if (status != LXP_OK) return status;
    }
    status = persist_output(bridge, evidence_digest, aggregate_root);
    if (status == LXP_OK) bridge->applied = true;
    return status;
}

lxp_result lxp_programs_finalize_occupancy_batch(
    lxp_kernel *kernel, uint64_t batch_number, uint64_t batch_timestamp_ms,
    uint64_t global_sequence, uint32_t parameter_version, lxp_arena *arena,
    lxp_programs_occupancy_receipt *receipt, lxp_byte_span *encoded)
{
    lx_programs_transfer_runtime *runtime;
    lx_programs_fee_schedule schedule;
    uint8_t occupancy_asset_id[32];
    lxp_programs_occupancy_bridge bridge;
    lxp_module_ctx ctx;
    lxp_result status;
    if (kernel == NULL || batch_number == 0U || global_sequence == UINT64_MAX ||
        parameter_version == 0U || arena == NULL || receipt == NULL ||
        encoded == NULL || kernel->state == NULL || kernel->journal == NULL)
        return LXP_ERR_NON_CANONICAL;
    runtime = (lx_programs_transfer_runtime *)
        kernel->module_runtime[LXP_MODULE_PROGRAMS];
    status = resolve_parameters(runtime, parameter_version, &schedule,
                                occupancy_asset_id);
    if (status != LXP_OK) return status;
    status = lxp_state_store_bind_accounts(kernel->state, runtime->accounts);
    if (status != LXP_OK) return status;
    status = lxp_state_journal_open(kernel->state, global_sequence,
                                    kernel->journal);
    if (status != LXP_OK) return status;
    status = lxp_state_journal_require_account_root(kernel->journal);
    if (status != LXP_OK) {
        (void)lxp_state_journal_rollback(kernel->journal);
        return status;
    }
    status = lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_PROGRAMS,
                                 batch_timestamp_ms, kernel->epoch,
                                 global_sequence, UINT64_MAX, arena, true);
    if (status != LXP_OK) {
        (void)lxp_state_journal_rollback(kernel->journal);
        return status;
    }
    ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    ctx.batch_number = batch_number;
    status = lxp_programs_occupancy_bridge_init(&bridge, &ctx);
    if (status == LXP_OK && bridge.uninitialized) {
        bridge.finalized_batch = batch_number - 1U;
        bridge.current_batch = bridge.finalized_batch;
        bridge.uninitialized = false;
    }
    if (status == LXP_OK && bridge.finalized_batch == UINT64_MAX)
        status = LXP_ERR_OVERFLOW;
    if (status == LXP_OK && batch_number != bridge.finalized_batch + 1U)
        status = batch_number <= bridge.finalized_batch ?
                 LXP_ERR_IDEMPOTENT_REPLAY : LXP_ERR_BATCH_GAP;
    bridge.finalizing = true;
    if (status == LXP_OK)
        status = layerx_programs_occupancy_finalize_rust(
            (uint64_t)(uintptr_t)&bridge, batch_number, parameter_version,
            schedule.version, schedule.cpu, schedule.memory_byte,
            schedule.storage_read_byte, schedule.storage_write_byte,
            schedule.output_value, schedule.output_byte,
            schedule.occupancy_byte_batch);
    if (status == LXP_OK && !bridge.applied) status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK) status = lxp_module_ctx_prepare_commit(&ctx);
    if (status != LXP_OK) {
        lxp_module_ctx_rollback(&ctx);
        (void)lxp_state_journal_rollback(kernel->journal);
        return status;
    }
    (void)memcpy(bridge.receipt.previous_state_root,
                 kernel->current_state_root, 32U);
    status = lxp_module_ctx_preview_state_root(
        &ctx, kernel->journal, bridge.receipt.resulting_state_root);
    if (status == LXP_OK)
        status = lxp_programs_occupancy_receipt_encode(
            &bridge.receipt, arena, encoded);
    if (status == LXP_OK) {
        status = lxp_state_journal_commit(kernel->journal);
        if (status != LXP_OK && !kernel->journal->open) {
            lxp_result committed_status = lxp_module_ctx_commit(&ctx);
            if (committed_status == LXP_OK) {
                (void)memcpy(kernel->current_state_root,
                             bridge.receipt.resulting_state_root, 32U);
                *receipt = bridge.receipt;
            }
            return committed_status == LXP_OK ? status :
                                                LXP_FATAL_INVARIANT;
        }
    }
    if (status == LXP_OK) status = lxp_module_ctx_commit(&ctx);
    if (status != LXP_OK) {
        if (ctx.commit_prepared) lxp_module_ctx_rollback(&ctx);
        if (kernel->journal->open)
            (void)lxp_state_journal_rollback(kernel->journal);
        return status;
    }
    (void)memcpy(kernel->current_state_root,
                 bridge.receipt.resulting_state_root, 32U);
    *receipt = bridge.receipt;
    return LXP_OK;
}

static lxp_result receipt_length(const lxp_programs_occupancy_receipt *receipt,
                                 size_t *length)
{
    size_t payer_bytes;
    size_t total = OCCUPANCY_RECEIPT_DOMAIN_BYTES + 8U + 8U + 4U + 4U +
                   56U + 32U +
                   4U * 16U + 2U + 32U + 4U + 5U * 32U;
    if (receipt->payer_count > LXP_PROGRAMS_OCCUPANCY_MAX_PAYERS ||
        receipt->settlement_evidence.bytes == NULL ||
        receipt->settlement_evidence.length == 0U ||
        receipt->settlement_evidence.length >
            LXP_PROGRAMS_OCCUPANCY_MAX_EVIDENCE_BYTES)
        return LXP_ERR_LENGTH_LIMIT;
    payer_bytes = (size_t)receipt->payer_count * OCCUPANCY_PAYER_BYTES;
    if (payer_bytes > SIZE_MAX - total ||
        receipt->settlement_evidence.length > SIZE_MAX - total - payer_bytes)
        return LXP_ERR_LENGTH_LIMIT;
    *length = total + payer_bytes + receipt->settlement_evidence.length;
    return LXP_OK;
}

static lxp_result receipt_validate_fields(
    const lxp_programs_occupancy_receipt *receipt)
{
    uint8_t schedule_bytes[OCCUPANCY_SCHEDULE_BYTES];
    uint8_t digest[32];
    lxp_u128 paid_total = {0U, 0U};
    lxp_u128 arrears_total = {0U, 0U};
    uint16_t index;
    lxp_result status;
    if (receipt == NULL || receipt->batch_number == 0U ||
        receipt->parameter_version == 0U ||
        receipt->schedule_version != receipt->parameter_version ||
        lxp_ct_is_zero(receipt->occupancy_asset_id, 32U) ||
        lxp_ct_is_zero(receipt->schedule_commitment, 32U) ||
        lxp_ct_is_zero(receipt->settlement_evidence_digest, 32U) ||
        lxp_ct_is_zero(receipt->ledger_root, 32U) ||
        lxp_ct_is_zero(receipt->resulting_state_root, 32U) ||
        receipt->payer_count > LXP_PROGRAMS_OCCUPANCY_MAX_PAYERS ||
        receipt->settlement_evidence.bytes == NULL ||
        receipt->settlement_evidence.length == 0U ||
        receipt->settlement_evidence.length >
            LXP_PROGRAMS_OCCUPANCY_MAX_EVIDENCE_BYTES)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < receipt->payer_count; ++index) {
        const lxp_programs_occupancy_payer_receipt *payer =
            &receipt->payers[index];
        lxp_u128 accounted;
        if (lxp_ct_is_zero(payer->principal, 32U) ||
            (index != 0U && memcmp(receipt->payers[index - 1U].principal,
                                   payer->principal, 32U) >= 0))
            return LXP_ERR_NON_CANONICAL;
        status = lxp_u128_add(payer->paid, payer->arrears, &accounted);
        if (status != LXP_OK || lxp_u128_cmp(accounted, payer->due) != 0 ||
            payer->frozen != !lxp_u128_is_zero(payer->arrears))
            return LXP_ERR_NON_CANONICAL;
        status = lxp_u128_add(paid_total, payer->paid, &paid_total);
        if (status == LXP_OK)
            status = lxp_u128_add(arrears_total, payer->arrears,
                                  &arrears_total);
        if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    }
    if (lxp_u128_cmp(paid_total, receipt->paid_fee_units) != 0 ||
        lxp_u128_cmp(arrears_total, receipt->arrears_fee_units) != 0 ||
        (lxp_u128_is_zero(paid_total) !=
         lxp_ct_is_zero(receipt->transfer_set_root, 32U)))
        return LXP_ERR_NON_CANONICAL;
    write_u32(schedule_bytes, receipt->schedule_version);
    for (index = 0U; index < 7U; ++index)
        write_u64(schedule_bytes + 4U + (size_t)index * 8U,
                  receipt->schedule_prices[index]);
    (void)memcpy(schedule_bytes + 60U, receipt->occupancy_asset_id, 32U);
    status = lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH, schedule_bytes,
                             sizeof(schedule_bytes), digest);
    if (status == LXP_OK && lxp_ct_memcmp(
            digest, receipt->schedule_commitment, 32U) != 0)
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_hash_sha256(receipt->settlement_evidence.bytes,
                                 receipt->settlement_evidence.length, digest);
    if (status == LXP_OK && lxp_ct_memcmp(
            digest, receipt->settlement_evidence_digest, 32U) != 0)
        status = LXP_ERR_PAYLOAD_HASH_MISMATCH;
    if (status == LXP_OK)
        status = lxp_programs_occupancy_validate_receipt_evidence(receipt);
    return status;
}

lxp_result lxp_programs_occupancy_receipt_encode(
    const lxp_programs_occupancy_receipt *receipt, lxp_arena *arena,
    lxp_byte_span *encoded)
{
    uint8_t *bytes;
    size_t length;
    size_t offset = 0U;
    uint16_t index;
    lxp_result status;
    if (arena == NULL || encoded == NULL) return LXP_ERR_NON_CANONICAL;
    status = receipt_validate_fields(receipt);
    if (status == LXP_OK) status = receipt_length(receipt, &length);
    if (status == LXP_OK)
        status = lxp_arena_alloc(arena, length, 1U, (void **)&bytes);
    if (status != LXP_OK) return status;
    (void)memcpy(bytes + offset, occupancy_receipt_domain,
                 OCCUPANCY_RECEIPT_DOMAIN_BYTES);
    offset += OCCUPANCY_RECEIPT_DOMAIN_BYTES;
    write_u64(bytes + offset, receipt->batch_number); offset += 8U;
    write_u64(bytes + offset, receipt->global_sequence); offset += 8U;
    write_u32(bytes + offset, receipt->parameter_version); offset += 4U;
    write_u32(bytes + offset, receipt->schedule_version); offset += 4U;
    for (index = 0U; index < 7U; ++index) {
        write_u64(bytes + offset, receipt->schedule_prices[index]);
        offset += 8U;
    }
    (void)memcpy(bytes + offset, receipt->occupancy_asset_id, 32U); offset += 32U;
    lxp_u128_to_be(receipt->byte_batches, bytes + offset); offset += 16U;
    lxp_u128_to_be(receipt->fee_units, bytes + offset); offset += 16U;
    lxp_u128_to_be(receipt->paid_fee_units, bytes + offset); offset += 16U;
    lxp_u128_to_be(receipt->arrears_fee_units, bytes + offset); offset += 16U;
    write_u16(bytes + offset, receipt->payer_count); offset += 2U;
    for (index = 0U; index < receipt->payer_count; ++index) {
        const lxp_programs_occupancy_payer_receipt *payer = &receipt->payers[index];
        (void)memcpy(bytes + offset, payer->principal, 32U); offset += 32U;
        lxp_u128_to_be(payer->due, bytes + offset); offset += 16U;
        lxp_u128_to_be(payer->paid, bytes + offset); offset += 16U;
        lxp_u128_to_be(payer->arrears, bytes + offset); offset += 16U;
        bytes[offset++] = payer->frozen ? 1U : 0U;
    }
    (void)memcpy(bytes + offset, receipt->schedule_commitment, 32U); offset += 32U;
    write_u32(bytes + offset, (uint32_t)receipt->settlement_evidence.length); offset += 4U;
    (void)memcpy(bytes + offset, receipt->settlement_evidence.bytes,
                 receipt->settlement_evidence.length);
    offset += receipt->settlement_evidence.length;
    (void)memcpy(bytes + offset, receipt->settlement_evidence_digest, 32U); offset += 32U;
    (void)memcpy(bytes + offset, receipt->ledger_root, 32U); offset += 32U;
    (void)memcpy(bytes + offset, receipt->transfer_set_root, 32U); offset += 32U;
    (void)memcpy(bytes + offset, receipt->previous_state_root, 32U); offset += 32U;
    (void)memcpy(bytes + offset, receipt->resulting_state_root, 32U); offset += 32U;
    encoded->bytes = bytes;
    encoded->length = offset;
    return offset == length ? LXP_OK : LXP_FATAL_INVARIANT;
}

lxp_result lxp_programs_occupancy_receipt_decode(
    const uint8_t *bytes, size_t length,
    lxp_programs_occupancy_receipt *receipt)
{
    size_t offset = 0U;
    uint32_t evidence_length;
    uint16_t index;
    uint8_t schedule_bytes[OCCUPANCY_SCHEDULE_BYTES];
    uint8_t digest[32];
    lxp_u128 paid_total = {0U, 0U};
    lxp_u128 arrears_total = {0U, 0U};
    lxp_result status;
    const size_t minimum = OCCUPANCY_RECEIPT_DOMAIN_BYTES + 8U + 8U +
                           4U + 4U + 56U +
                           32U + 4U * 16U + 2U + 32U + 4U + 5U * 32U;
    if (bytes == NULL || receipt == NULL || length < minimum ||
        memcmp(bytes, occupancy_receipt_domain,
               OCCUPANCY_RECEIPT_DOMAIN_BYTES) != 0)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(receipt, 0, sizeof(*receipt));
    offset += OCCUPANCY_RECEIPT_DOMAIN_BYTES;
    receipt->batch_number = read_u64(bytes + offset); offset += 8U;
    receipt->global_sequence = read_u64(bytes + offset); offset += 8U;
    receipt->parameter_version = read_u32(bytes + offset); offset += 4U;
    receipt->schedule_version = read_u32(bytes + offset); offset += 4U;
    for (index = 0U; index < 7U; ++index) {
        receipt->schedule_prices[index] = read_u64(bytes + offset);
        offset += 8U;
    }
    (void)memcpy(receipt->occupancy_asset_id, bytes + offset, 32U); offset += 32U;
    status = lxp_u128_from_be(bytes + offset, &receipt->byte_batches);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_from_be(bytes + offset, &receipt->fee_units);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_from_be(bytes + offset, &receipt->paid_fee_units);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_from_be(bytes + offset, &receipt->arrears_fee_units);
    if (status != LXP_OK) return status;
    offset += 16U;
    receipt->payer_count = (uint16_t)(((uint16_t)bytes[offset] << 8U) |
                                      bytes[offset + 1U]);
    offset += 2U;
    if (receipt->payer_count > LXP_PROGRAMS_OCCUPANCY_MAX_PAYERS ||
        (size_t)receipt->payer_count * OCCUPANCY_PAYER_BYTES > length - offset)
        return LXP_ERR_LENGTH_LIMIT;
    for (index = 0U; index < receipt->payer_count; ++index) {
        lxp_programs_occupancy_payer_receipt *payer = &receipt->payers[index];
        lxp_u128 accounted;
        (void)memcpy(payer->principal, bytes + offset, 32U); offset += 32U;
        status = lxp_u128_from_be(bytes + offset, &payer->due);
        if (status != LXP_OK) return status;
        offset += 16U;
        status = lxp_u128_from_be(bytes + offset, &payer->paid);
        if (status != LXP_OK) return status;
        offset += 16U;
        status = lxp_u128_from_be(bytes + offset, &payer->arrears);
        if (status != LXP_OK) return status;
        offset += 16U;
        payer->frozen = bytes[offset++] != 0U;
        if (lxp_ct_is_zero(payer->principal, 32U) ||
            (index != 0U && memcmp(receipt->payers[index - 1U].principal,
                                   payer->principal, 32U) >= 0) ||
            (bytes[offset - 1U] > 1U))
            return LXP_ERR_NON_CANONICAL;
        status = lxp_u128_add(payer->paid, payer->arrears, &accounted);
        if (status != LXP_OK || lxp_u128_cmp(accounted, payer->due) != 0 ||
            payer->frozen != !lxp_u128_is_zero(payer->arrears))
            return LXP_ERR_NON_CANONICAL;
        status = lxp_u128_add(paid_total, payer->paid, &paid_total);
        if (status == LXP_OK)
            status = lxp_u128_add(arrears_total, payer->arrears,
                                  &arrears_total);
        if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    }
    if (length - offset < 32U + 4U + 5U * 32U)
        return LXP_ERR_TRUNCATED;
    (void)memcpy(receipt->schedule_commitment, bytes + offset, 32U); offset += 32U;
    evidence_length = read_u32(bytes + offset); offset += 4U;
    if (evidence_length == 0U ||
        evidence_length > LXP_PROGRAMS_OCCUPANCY_MAX_EVIDENCE_BYTES ||
        evidence_length > length - offset ||
        length - offset - evidence_length != 5U * 32U)
        return LXP_ERR_LENGTH_LIMIT;
    receipt->settlement_evidence = (lxp_byte_span){bytes + offset, evidence_length};
    offset += evidence_length;
    (void)memcpy(receipt->settlement_evidence_digest, bytes + offset, 32U); offset += 32U;
    (void)memcpy(receipt->ledger_root, bytes + offset, 32U); offset += 32U;
    (void)memcpy(receipt->transfer_set_root, bytes + offset, 32U); offset += 32U;
    (void)memcpy(receipt->previous_state_root, bytes + offset, 32U); offset += 32U;
    (void)memcpy(receipt->resulting_state_root, bytes + offset, 32U); offset += 32U;
    write_u32(schedule_bytes, receipt->schedule_version);
    for (index = 0U; index < 7U; ++index)
        write_u64(schedule_bytes + 4U + (size_t)index * 8U,
                  receipt->schedule_prices[index]);
    (void)memcpy(schedule_bytes + 60U, receipt->occupancy_asset_id, 32U);
    status = lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH, schedule_bytes,
                             sizeof(schedule_bytes), digest);
    if (status == LXP_OK &&
        lxp_ct_memcmp(digest, receipt->schedule_commitment, 32U) != 0)
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_hash_sha256(receipt->settlement_evidence.bytes,
                                 receipt->settlement_evidence.length, digest);
    if (status == LXP_OK && lxp_ct_memcmp(
            digest, receipt->settlement_evidence_digest, 32U) != 0)
        status = LXP_ERR_PAYLOAD_HASH_MISMATCH;
    if (status != LXP_OK) return status;
    if (offset != length || receipt->batch_number == 0U ||
        receipt->parameter_version == 0U ||
        receipt->schedule_version != receipt->parameter_version ||
        lxp_ct_is_zero(receipt->occupancy_asset_id, 32U) ||
        lxp_ct_is_zero(receipt->ledger_root, 32U) ||
        lxp_ct_is_zero(receipt->resulting_state_root, 32U) ||
        lxp_u128_cmp(paid_total, receipt->paid_fee_units) != 0 ||
        lxp_u128_cmp(arrears_total, receipt->arrears_fee_units) != 0 ||
        (lxp_u128_is_zero(receipt->paid_fee_units) !=
         lxp_ct_is_zero(receipt->transfer_set_root, 32U)))
        return LXP_ERR_NON_CANONICAL;
    return receipt_validate_fields(receipt);
}

lxp_result lxp_programs_replay_finalize(
    void *context, const lxp_batch_header *header, uint32_t parameter_version,
    uint64_t system_sequence, const uint8_t previous_state_root[32],
    lxp_arena *arena, lxp_replay_activity_output *output)
{
    lxp_kernel *kernel = (lxp_kernel *)context;
    lxp_programs_occupancy_receipt receipt;
    lxp_byte_span encoded;
    lxp_result status;
    if (kernel == NULL || header == NULL || previous_state_root == NULL ||
        arena == NULL || output == NULL ||
        header->protocol_version != LXP_PROTOCOL_VERSION_OCCUPANCY)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_ct_memcmp(kernel->current_state_root,
                      previous_state_root, 32U) != 0)
        return LXP_ERR_ROOT_MISMATCH;
    status = lxp_programs_finalize_occupancy_batch(
        kernel, header->batch_number, header->timestamp_ms,
        system_sequence, parameter_version, arena, &receipt, &encoded);
    if (status != LXP_OK) return status;
    (void)memset(output, 0, sizeof(*output));
    output->result_code = LXP_OK;
    output->fee_charged = receipt.paid_fee_units;
    output->effects = receipt.settlement_evidence;
    output->canonical_receipt = encoded;
    (void)memcpy(output->resulting_state_root,
                 receipt.resulting_state_root, 32U);
    return LXP_OK;
}

lxp_result lxp_programs_replay_engine_bind(lxp_replay_engine *engine,
                                           lxp_kernel *kernel)
{
    if (engine == NULL || kernel == NULL)
        return LXP_ERR_NON_CANONICAL;
    return lxp_replay_engine_register_batch_finalizer(
        engine, lxp_programs_replay_finalize, kernel);
}
