#include "layerx/programs.h"

#include "layerx/lxp_crypto.h"

#include <stdlib.h>
#include <string.h>

enum { STATE_RECORD_MAX_ITEMS = 4096 };

typedef struct record_values {
    lx_programs_value_account_view *values;
    size_t capacity;
    size_t count;
} record_values;

typedef struct record_writer {
    uint8_t *bytes;
    size_t capacity;
    size_t length;
    lxp_result status;
} record_writer;

typedef struct writer_count {
    record_writer *writer;
    const uint8_t *program_id;
    size_t count;
} writer_count;

static void put(record_writer *writer, const void *bytes, size_t length)
{
    if (writer->status != LXP_OK) return;
    if ((bytes == NULL && length != 0U) ||
        length > writer->capacity - writer->length) {
        writer->status = LXP_ERR_LENGTH_LIMIT;
        return;
    }
    (void)memcpy(writer->bytes + writer->length, bytes, length);
    writer->length += length;
}

static void put_u8(record_writer *writer, uint8_t value)
{
    put(writer, &value, 1U);
}

static void put_u16(record_writer *writer, uint16_t value)
{
    uint8_t bytes[2] = {(uint8_t)(value >> 8U), (uint8_t)value};
    put(writer, bytes, sizeof(bytes));
}

static void put_u32(record_writer *writer, uint32_t value)
{
    uint8_t bytes[4] = {
        (uint8_t)(value >> 24U), (uint8_t)(value >> 16U),
        (uint8_t)(value >> 8U), (uint8_t)value};
    put(writer, bytes, sizeof(bytes));
}

static void put_u64(record_writer *writer, uint64_t value)
{
    uint8_t bytes[8];
    size_t index;
    for (index = 0U; index < sizeof(bytes); ++index)
        bytes[index] = (uint8_t)(value >> (56U - index * 8U));
    put(writer, bytes, sizeof(bytes));
}

static void put_u128(record_writer *writer, lxp_u128 value)
{
    put_u64(writer, value.hi);
    put_u64(writer, value.lo);
}

static void put_proof(record_writer *writer, const lxp_state_proof *proof)
{
    size_t index;
    if (proof == NULL || proof->depth > LXP_STATE_PROOF_MAX_DEPTH) {
        writer->status = LXP_ERR_NON_CANONICAL;
        return;
    }
    put_u32(writer, proof->leaf_index);
    put_u32(writer, proof->leaf_count);
    put_u8(writer, proof->depth);
    for (index = 0U; index < proof->depth; ++index)
        put(writer, proof->siblings[index], 32U);
}

static void put_binding(record_writer *writer,
                        const lx_programs_account_binding *binding)
{
    put_u8(writer, binding->record_version);
    put(writer, binding->program_id, 32U);
    put(writer, binding->account_id, 32U);
    put(writer, binding->asset_id, 32U);
    put_u16(writer, binding->seed_length);
    put(writer, binding->seed, binding->seed_length);
    put_u64(writer, binding->registered_sequence);
    put(writer, binding->registration_event_digest, 32U);
}

static lxp_result collect_value(const lx_programs_value_account_view *value,
                                void *context)
{
    record_values *values = (record_values *)context;
    if (value == NULL || values == NULL || values->count == values->capacity)
        return LXP_ERR_LENGTH_LIMIT;
    values->values[values->count++] = *value;
    return LXP_OK;
}

static lxp_result count_route(const lx_programs_exit_route_view *route,
                              void *context)
{
    writer_count *state = (writer_count *)context;
    if (route == NULL || state == NULL ||
        state->count == STATE_RECORD_MAX_ITEMS)
        return LXP_ERR_LENGTH_LIMIT;
    ++state->count;
    return LXP_OK;
}

static lxp_result write_route(const lx_programs_exit_route_view *route,
                              void *context)
{
    writer_count *state = (writer_count *)context;
    if (route == NULL || state == NULL || state->writer == NULL ||
        state->program_id == NULL ||
        lxp_ct_memcmp(route->program_id, state->program_id, 32U) != 0 ||
        route->seed_length > LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES)
        return LXP_ERR_NON_CANONICAL;
    put(state->writer, route->account_id, 32U);
    put(state->writer, route->asset_id, 32U);
    put(state->writer, route->destination, 32U);
    put_u16(state->writer, route->seed_length);
    put(state->writer, route->seed, route->seed_length);
    ++state->count;
    return state->writer->status;
}

static lxp_result count_history(
    const lx_programs_wind_down_history_view *history, void *context)
{
    writer_count *state = (writer_count *)context;
    if (history == NULL || state == NULL ||
        state->count == STATE_RECORD_MAX_ITEMS)
        return LXP_ERR_LENGTH_LIMIT;
    ++state->count;
    return LXP_OK;
}

static lxp_result write_history(
    const lx_programs_wind_down_history_view *history, void *context)
{
    writer_count *state = (writer_count *)context;
    if (history == NULL || state == NULL || state->writer == NULL ||
        state->program_id == NULL ||
        lxp_ct_memcmp(history->program_id, state->program_id, 32U) != 0 ||
        lxp_ct_memcmp(history->exit_program, state->program_id, 32U) != 0 ||
        history->prior < LX_PROGRAMS_LIFECYCLE_ACTIVE ||
        history->prior > LX_PROGRAMS_LIFECYCLE_TOMBSTONED ||
        history->current < LX_PROGRAMS_LIFECYCLE_ACTIVE ||
        history->current > LX_PROGRAMS_LIFECYCLE_TOMBSTONED)
        return LXP_ERR_NON_CANONICAL;
    put_u8(state->writer, (uint8_t)history->prior);
    put_u8(state->writer, (uint8_t)history->current);
    put(state->writer, history->authority, 32U);
    put_u64(state->writer, history->effective_sequence);
    put(state->writer, history->exit_program, 32U);
    put_u64(state->writer, history->deadline);
    put_u32(state->writer, history->live_value_account_count);
    ++state->count;
    return state->writer->status;
}

static int account_pointer_compare(const void *left, const void *right)
{
    const lx_programs_value_account_view *const *a =
        (const lx_programs_value_account_view *const *)left;
    const lx_programs_value_account_view *const *b =
        (const lx_programs_value_account_view *const *)right;
    return memcmp((*a)->account.id, (*b)->account.id, 32U);
}

static bool same_proof(const lxp_state_proof *left,
                       const lxp_state_proof *right)
{
    return left->leaf_index == right->leaf_index &&
           left->leaf_count == right->leaf_count &&
           left->depth == right->depth &&
           left->depth <= LXP_STATE_PROOF_MAX_DEPTH &&
           memcmp(left->siblings, right->siblings,
                  (size_t)left->depth * 32U) == 0;
}

lxp_result lxp_programs_state_record_encode(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    const uint8_t receipt_digest[32], lxp_arena *arena,
    lxp_byte_span *encoded)
{
    static const uint8_t magic[5] = {'L', 'X', 'P', 'S', '1'};
    lx_programs_account_state_head head;
    lx_programs_wind_down_view wind_down;
    lx_programs_value_account_view *value_memory;
    const lx_programs_value_account_view **account_order;
    record_values values;
    writer_count routes = {NULL, program_id, 0U};
    writer_count history = {NULL, program_id, 0U};
    record_writer writer;
    void *memory = NULL;
    size_t mark;
    size_t index;
    lxp_result status;
    if (ctx == NULL || program_id == NULL || receipt_digest == NULL ||
        arena == NULL || encoded == NULL || lxp_ct_is_zero(program_id, 32U) ||
        lxp_ct_is_zero(receipt_digest, 32U))
        return LXP_ERR_NON_CANONICAL;
    encoded->bytes = NULL;
    encoded->length = 0U;
    mark = lxp_arena_mark(arena);
    status = lxp_arena_alloc(
        arena, STATE_RECORD_MAX_ITEMS * sizeof(*value_memory),
        _Alignof(lx_programs_value_account_view), &memory);
    if (status != LXP_OK) return status;
    value_memory = (lx_programs_value_account_view *)memory;
    values.values = value_memory;
    values.capacity = STATE_RECORD_MAX_ITEMS;
    values.count = 0U;
    status = lxp_programs_account_state_head_read(
        ctx, program_id, receipt_digest, &head);
    if (status == LXP_OK)
        status = lxp_programs_value_account_iter(
            ctx, program_id, receipt_digest, collect_value, &values);
    if (status == LXP_OK)
        status = lxp_programs_exit_route_iter(
            ctx, program_id, count_route, &routes);
    if (status == LXP_OK)
        status = lxp_programs_wind_down_history_iter(
            ctx, program_id, count_history, &history);
    (void)memset(&wind_down, 0, sizeof(wind_down));
    if (status == LXP_OK) {
        status = lxp_programs_wind_down_read(ctx, program_id, &wind_down);
        if (status == LXP_ERR_UNKNOWN_FIELD) {
            wind_down.status = LX_PROGRAMS_LIFECYCLE_ACTIVE;
            status = LXP_OK;
        }
    }
    if (status != LXP_OK || values.count > UINT16_MAX ||
        routes.count > UINT16_MAX || history.count > UINT16_MAX) {
        (void)lxp_arena_reset(arena, mark);
        return status == LXP_OK ? LXP_ERR_LENGTH_LIMIT : status;
    }
    if (wind_down.status != LX_PROGRAMS_LIFECYCLE_ACTIVE &&
        (lxp_ct_memcmp(wind_down.program_id, program_id, 32U) != 0 ||
         lxp_ct_memcmp(wind_down.exit_program, program_id, 32U) != 0 ||
         wind_down.deadline == 0U || wind_down.effective_sequence == 0U ||
         wind_down.value_account_count != (uint32_t)values.count)) {
        (void)lxp_arena_reset(arena, mark);
        return LXP_ERR_NON_CANONICAL;
    }
    for (index = 0U; index < values.count; ++index) {
        const lx_programs_value_account_view *value = &values.values[index];
        if (lxp_ct_memcmp(value->binding.program_id, program_id, 32U) != 0 ||
            lxp_ct_memcmp(value->binding.account_id, value->account.id, 32U) != 0 ||
            lxp_ct_memcmp(value->binding.asset_id,
                          value->account.asset_id, 32U) != 0 ||
            value->binding.registered_sequence !=
                value->account.created_at_sequence ||
            lxp_u128_cmp(value->balance, value->account.balance) != 0 ||
            value->frozen != value->account.frozen ||
            value->observed_sequence != head.observed_sequence ||
            value->observed_at != head.observed_at ||
            lxp_ct_memcmp(value->receipt_digest,
                          head.receipt_digest, 32U) != 0 ||
            lxp_ct_memcmp(value->state_root, head.state_root, 32U) != 0 ||
            lxp_ct_memcmp(value->account_root, head.account_root, 32U) != 0 ||
            lxp_ct_memcmp(value->universal_root,
                          head.universal_root, 32U) != 0 ||
            lxp_ct_memcmp(value->programs_root,
                          head.programs_root, 32U) != 0 ||
            !same_proof(&value->account_tree_proof,
                        &head.account_tree_proof) ||
            !same_proof(&value->universal_root_proof,
                        &head.universal_root_proof) ||
            !same_proof(&value->programs_root_proof,
                        &head.programs_root_proof)) {
            (void)lxp_arena_reset(arena, mark);
            return LXP_ERR_NON_CANONICAL;
        }
    }
    status = lxp_arena_alloc(arena, values.count * sizeof(*account_order),
                             _Alignof(const lx_programs_value_account_view *),
                             &memory);
    if (status != LXP_OK) {
        (void)lxp_arena_reset(arena, mark);
        return status;
    }
    account_order = (const lx_programs_value_account_view **)memory;
    for (index = 0U; index < values.count; ++index)
        account_order[index] = &values.values[index];
    qsort(account_order, values.count, sizeof(account_order[0]),
          account_pointer_compare);
    status = lxp_arena_alloc(arena, arena->capacity - arena->offset, 1U,
                             &memory);
    if (status != LXP_OK) {
        (void)lxp_arena_reset(arena, mark);
        return status;
    }
    writer.bytes = (uint8_t *)memory;
    writer.capacity = arena->capacity - ((uint8_t *)memory - arena->buffer);
    writer.length = 0U;
    writer.status = LXP_OK;
    put(&writer, magic, sizeof(magic));
    put(&writer, program_id, 32U);
    put_u8(&writer, (uint8_t)wind_down.status);
    put_u16(&writer, (uint16_t)values.count);
    for (index = 0U; index < values.count; ++index)
        put_binding(&writer, &values.values[index].binding);
    put_u16(&writer, (uint16_t)routes.count);
    routes.writer = &writer;
    routes.count = 0U;
    if (writer.status == LXP_OK)
        writer.status = lxp_programs_exit_route_iter(
            ctx, program_id, write_route, &routes);
    put_u16(&writer, (uint16_t)history.count);
    history.writer = &writer;
    history.count = 0U;
    if (writer.status == LXP_OK)
        writer.status = lxp_programs_wind_down_history_iter(
            ctx, program_id, write_history, &history);
    put_u16(&writer, LX_PROGRAMS_ACCOUNT_ABI_VERSION);
    put(&writer, head.receipt_digest, 32U);
    put(&writer, head.state_root, 32U);
    put(&writer, head.universal_root, 32U);
    put(&writer, head.programs_root, 32U);
    put(&writer, head.account_root, 32U);
    put_u64(&writer, head.observed_sequence);
    put_u64(&writer, head.observed_at);
    put_proof(&writer, &head.account_tree_proof);
    put_proof(&writer, &head.universal_root_proof);
    put_proof(&writer, &head.programs_root_proof);
    put_u16(&writer, (uint16_t)values.count);
    for (index = 0U; index < values.count; ++index) {
        put_binding(&writer, &values.values[index].binding);
        put_proof(&writer, &values.values[index].binding_proof);
    }
    put_u16(&writer, (uint16_t)values.count);
    for (index = 0U; index < values.count; ++index) {
        const lx_programs_value_account_view *value = account_order[index];
        const lx_account *account = &value->account;
        put(&writer, account->id, 32U);
        put_u16(&writer, account->name_length);
        put(&writer, account->name, account->name_length);
        put_u8(&writer, (uint8_t)account->kind);
        put_u128(&writer, account->balance);
        put(&writer, account->asset_id, 32U);
        put_u8(&writer, account->has_asset ? 1U : 0U);
        put_u64(&writer, account->next_sequence);
        put_u64(&writer, account->created_at_sequence);
        put_u8(&writer, account->frozen ? 1U : 0U);
        put_u8(&writer, account->has_open_reference ? 1U : 0U);
        put(&writer, account->authority_key, 32U);
        put_u8(&writer, account->has_authority_key ? 1U : 0U);
        put_proof(&writer, &value->account_proof);
    }
    if (writer.status != LXP_OK) {
        (void)lxp_arena_reset(arena, mark);
        return writer.status;
    }
    encoded->bytes = writer.bytes;
    encoded->length = writer.length;
    return LXP_OK;
}
