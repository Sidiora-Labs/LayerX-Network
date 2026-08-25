#include "layerx/lxp_kernel.h"

#include <stdint.h>
#include <string.h>

static bool supply_bad;

static lxp_result supply(const lxp_kernel *kernel)
{
    (void)kernel;
    return supply_bad ? LXP_ERR_CONSERVATION : LXP_OK;
}

static lxp_result genesis(lxp_module_ctx *ctx, const uint8_t *bytes, size_t n)
{ (void)ctx; (void)bytes; (void)n; return LXP_OK; }
static lxp_result decode(lxp_module_ctx *ctx, uint16_t ordinal,
                         const uint8_t *bytes, size_t n, void **decoded)
{ (void)ctx; (void)ordinal; (void)bytes; (void)n; *decoded = NULL; return LXP_OK; }
static lxp_result validate(lxp_module_ctx *ctx, const lxp_activity *activity,
                           const lxp_authority_resolved *authority,
                           const void *decoded)
{ (void)ctx; (void)activity; (void)authority; (void)decoded; return LXP_OK; }
static lxp_result execute(lxp_module_ctx *ctx, const lxp_activity *activity,
                          const lxp_authority_resolved *authority,
                          const void *decoded, lxp_effect_buffer *effects)
{ (void)ctx; (void)activity; (void)authority; (void)decoded; (void)effects;
  return LXP_OK; }
static lxp_result epoch(lxp_module_ctx *ctx, uint64_t number, uint64_t timestamp)
{ (void)ctx; (void)number; (void)timestamp; return LXP_OK; }
static lxp_result module_root(lxp_module_ctx *ctx, uint8_t root[32])
{ (void)ctx; (void)memset(root, 0, 32U); return LXP_OK; }

static const uint32_t program_types[] = { UINT32_C(0x00090001) };
static const lxp_module_iface program_iface = {
    9U, 1U, "programs", program_types, 1U, genesis, decode, validate,
    execute, epoch, epoch, module_root, NULL
};

static int prepare(lxp_kernel *kernel, lxp_state_store *store,
                   lxp_state_journal *journal, lxp_module_ctx *ctx,
                   lxp_arena *arena, uint8_t *arena_bytes, bool reverse)
{
    static const uint32_t types[] = { UINT32_C(0x00010001) };
    static const lxp_module_iface iface = { 1U, 1U, "asset", types, 1U,
        genesis, decode, validate, execute, epoch, epoch, module_root, NULL };
    static uint64_t parameters = 1U;
    uint8_t first = reverse ? 2U : 1U;
    uint8_t second = reverse ? 1U : 2U;
    uint8_t first_value = (uint8_t)(first + 10U);
    uint8_t second_value = (uint8_t)(second + 10U);
    uint8_t cell_a[32] = { 1U };
    uint8_t cell_b[32] = { 2U };
    if (lxp_state_store_init(store, 0U) != LXP_OK ||
        lxp_arena_init(arena, arena_bytes, 128U) != LXP_OK ||
        lxp_kernel_create(kernel, store, journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(kernel, &iface) != LXP_OK ||
        lxp_kernel_set_supply_checker(kernel, supply) != LXP_OK ||
        lxp_module_ctx_init(ctx, kernel, 1U, 1U, 0U, 0U, 10U, arena, true) !=
            LXP_OK || lxp_ctx_kv_put(ctx, &first, 1U, &first_value, 1U) !=
            LXP_OK || lxp_ctx_kv_put(ctx, &second, 1U, &second_value, 1U) !=
            LXP_OK || lxp_module_ctx_commit(ctx) != LXP_OK ||
        lxp_state_journal_open(store, 0U, journal) != LXP_OK ||
        lxp_state_journal_set(journal, reverse ? cell_b : cell_a,
                              (lxp_u128){ 0U, reverse ? 2U : 1U }) != LXP_OK ||
        lxp_state_journal_set(journal, reverse ? cell_a : cell_b,
                              (lxp_u128){ 0U, reverse ? 1U : 2U }) != LXP_OK ||
        lxp_state_journal_commit(journal) != LXP_OK) return 1;
    return 0;
}

int main(void)
{
    lxp_kernel first;
    lxp_kernel second;
    lxp_state_store first_store;
    lxp_state_store second_store;
    lxp_state_journal first_journal;
    lxp_state_journal second_journal;
    lxp_module_ctx first_ctx;
    lxp_module_ctx second_ctx;
    lxp_arena first_arena;
    lxp_arena second_arena;
    uint8_t first_bytes[128];
    uint8_t second_bytes[128];
    uint8_t first_root[32];
    uint8_t second_root[32];
    uint8_t legacy_root[32];
    uint8_t chained[32];
    if (prepare(&first, &first_store, &first_journal, &first_ctx, &first_arena,
                first_bytes, false) != 0 ||
        prepare(&second, &second_store, &second_journal, &second_ctx,
                &second_arena, second_bytes, true) != 0 ||
        lxp_state_root(&first, first_root) != LXP_OK ||
        lxp_state_root(&second, second_root) != LXP_OK ||
        memcmp(first_root, second_root, 32U) != 0)
        return 1;
    (void)memcpy(legacy_root, first_root, sizeof(legacy_root));
    if (
        lxp_kernel_register_module(&first, &program_iface) != LXP_OK ||
        lxp_state_root(&first, first_root) != LXP_OK ||
        memcmp(first_root, legacy_root, 32U) == 0 ||
        lxp_state_root_chain(first_root, second_root, 1U, chained) != LXP_OK ||
        memcmp(chained, first_root, 32U) == 0) return 1;
    first.modules[first.module_count - 1U].enabled_epoch = 1U;
    if (lxp_state_root(&first, chained) != LXP_OK ||
        memcmp(chained, first_root, 32U) == 0) return 1;
    first.modules[first.module_count - 1U].enabled_epoch = 0U;
    ++first.modules[first.module_count - 1U].activity_types[0];
    if (lxp_state_root(&first, chained) != LXP_OK ||
        memcmp(chained, first_root, 32U) == 0) return 1;
    --first.modules[first.module_count - 1U].activity_types[0];
    first.modules[first.module_count - 1U].module_id = 10U;
    if (lxp_state_root(&first, chained) != LXP_ERR_UNKNOWN_MODULE) return 1;
    first.modules[first.module_count - 1U].module_id = 9U;
    first.module_count = LXP_KERNEL_MAX_MODULE_REGISTRATIONS + 1U;
    if (lxp_state_root(&first, chained) != LXP_ERR_LENGTH_LIMIT) return 1;
    first.module_count = 2U;
    second.module_kv[second.module_kv_count].module_id = 0U;
    ++second.module_kv_count;
    if (lxp_state_root(&second, chained) != LXP_ERR_UNKNOWN_MODULE) return 1;
    --second.module_kv_count;
    second.module_kv[second.module_kv_count].module_id = 9U;
    second.module_kv[second.module_kv_count].key_length = 1U;
    second.module_kv[second.module_kv_count].key[0] = 9U;
    second.module_kv[second.module_kv_count].value_length = 1U;
    second.module_kv[second.module_kv_count].value[0] = 9U;
    ++second.module_kv_count;
    if (lxp_state_root(&second, chained) != LXP_ERR_UNKNOWN_MODULE) return 1;
    --second.module_kv_count;
    {
        lxp_state_store *saved_state = first.state;
        first.state = NULL;
        if (lxp_state_root(&first, chained) != LXP_FATAL_INVARIANT)
            return 1;
        first.state = saved_state;
    }
    first.blob_count = LXP_KERNEL_MAX_BLOBS + 1U;
    if (lxp_state_root(&first, chained) != LXP_ERR_LENGTH_LIMIT) return 1;
    first.blob_count = 0U;
    first.blob_total_bytes = 1U;
    if (lxp_state_root(&first, chained) != LXP_FATAL_INVARIANT) return 1;
    first.blob_total_bytes = 0U;
    first_store.count = LXP_STATE_MAX_CELLS + 1U;
    if (lxp_state_root(&first, chained) != LXP_ERR_LENGTH_LIMIT) return 1;
    first_store.count = 2U;
    first_store.idempotency_count = LXP_STATE_MAX_IDEMPOTENCY + 1U;
    if (lxp_state_root(&first, chained) != LXP_ERR_LENGTH_LIMIT) return 1;
    first_store.idempotency_count = 0U;
    first.module_kv_count = LXP_KERNEL_MAX_MODULE_KV + 1U;
    if (lxp_state_root(&first, chained) != LXP_ERR_LENGTH_LIMIT) return 1;
    first.module_kv_count = 2U;
    first.blob_count = 1U;
    first.blobs[0].length = 1U;
    first.blobs[0].bytes = NULL;
    first.blob_total_bytes = 1U;
    if (lxp_state_root(&first, chained) != LXP_FATAL_INVARIANT) return 1;
    first.blob_count = 0U;
    first.blob_total_bytes = 0U;
    supply_bad = true;
    if (lxp_state_root(&first, chained) != LXP_FATAL_SUPPLY_MISMATCH ||
        lxp_state_store_destroy(&first_store) != LXP_OK ||
        lxp_state_store_destroy(&second_store) != LXP_OK) return 1;
    return 0;
}
