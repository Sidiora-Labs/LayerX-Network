#include "layerx/lxp_kernel.h"

#include "layerx/lx_web.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lxp_state_proof.h"

#include <stdbool.h>
#include <stdint.h>
#include <string.h>

static size_t begin_calls;
static size_t end_calls;
static uint64_t last_begin_epoch;
static uint64_t last_end_epoch;
static lxp_result begin_status = LXP_OK;
static size_t stage_bulk;
static size_t bulk_serial;

/* Stages `count` module writes under keys no earlier hook has used, so each
 * one is a genuine addition to the kernel's committed table rather than an
 * overwrite of a slot it already holds. */
static lxp_result stage_distinct_keys(lxp_module_ctx *ctx, size_t count)
{
    uint8_t key[9];
    uint8_t value[1];
    size_t i;
    size_t digit;
    value[0] = 1U;
    key[0] = (uint8_t)'k';
    for (i = 0U; i < count; ++i) {
        size_t serial = bulk_serial;
        lxp_result status;
        for (digit = sizeof(key); digit > 1U; --digit) {
            key[digit - 1U] = (uint8_t)('0' + (int)(serial % 10U));
            serial /= 10U;
        }
        ++bulk_serial;
        status = lxp_ctx_kv_put(ctx, key, sizeof(key), value, sizeof(value));
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
}

static lxp_result genesis(lxp_module_ctx *ctx, const uint8_t *manifest,
                          size_t length)
{
    (void)ctx;
    (void)manifest;
    (void)length;
    return LXP_OK;
}

static lxp_result decode(lxp_module_ctx *ctx, uint16_t ordinal,
                         const uint8_t *payload, size_t length, void **decoded)
{
    (void)ctx;
    (void)ordinal;
    (void)payload;
    (void)length;
    *decoded = NULL;
    return LXP_OK;
}

static lxp_result validate(lxp_module_ctx *ctx, const lxp_activity *activity,
                           const lxp_authority_resolved *authority,
                           const void *decoded)
{
    (void)ctx;
    (void)activity;
    (void)authority;
    (void)decoded;
    return LXP_OK;
}

static lxp_result execute(lxp_module_ctx *ctx, const lxp_activity *activity,
                          const lxp_authority_resolved *authority,
                          const void *decoded, lxp_effect_buffer *effects)
{
    (void)ctx;
    (void)activity;
    (void)authority;
    (void)decoded;
    (void)effects;
    return LXP_OK;
}

/* Records every epoch hook the kernel drives. epoch_begin stages one module
 * write keyed "epoch" so that the test can see the transition commit it and a
 * failing transition roll it back; while stage_bulk is set both hooks instead
 * stage that many fresh keys, which is how the capacity checks fill the
 * kernel's table and then ask one transition to overrun it. */
static lxp_result epoch_hook(lxp_module_ctx *ctx, uint64_t number,
                             uint64_t timestamp, bool begin)
{
    static const uint8_t key[] = "epoch";
    uint8_t value[8];
    size_t i;
    if (ctx == NULL || number != lxp_ctx_epoch(ctx) ||
        timestamp != lxp_ctx_batch_timestamp_ms(ctx))
        return LXP_ERR_TIMESTAMP_REGRESSION;
    if (begin) {
        ++begin_calls;
        last_begin_epoch = number;
        if (begin_status != LXP_OK) return begin_status;
    } else {
        ++end_calls;
        last_end_epoch = number;
    }
    if (stage_bulk != 0U) return stage_distinct_keys(ctx, stage_bulk);
    if (!begin) return LXP_OK;
    for (i = 0U; i < 8U; ++i)
        value[i] = (uint8_t)(number >> ((7U - i) * 8U));
    return lxp_ctx_kv_put(ctx, key, sizeof(key) - 1U, value, sizeof(value));
}

static lxp_result epoch_begin(lxp_module_ctx *ctx, uint64_t number,
                              uint64_t timestamp)
{
    return epoch_hook(ctx, number, timestamp, true);
}

static lxp_result epoch_end(lxp_module_ctx *ctx, uint64_t number,
                            uint64_t timestamp)
{
    return epoch_hook(ctx, number, timestamp, false);
}

static lxp_result state_root(lxp_module_ctx *ctx, uint8_t root[32])
{
    (void)ctx;
    (void)memset(root, 0, 32U);
    return LXP_OK;
}

static lxp_module_iface make_iface(uint32_t version,
                                   const uint32_t *types, size_t count)
{
    lxp_module_iface iface;
    (void)memset(&iface, 0, sizeof(iface));
    iface.module_id = LXP_MODULE_ASSET;
    iface.abi_version = version;
    iface.name = "asset";
    iface.activity_types = types;
    iface.activity_type_count = count;
    iface.genesis = genesis;
    iface.decode = decode;
    iface.validate = validate;
    iface.execute = execute;
    iface.epoch_begin = epoch_begin;
    iface.epoch_end = epoch_end;
    iface.state_root = state_root;
    return iface;
}

/* The kernel at epoch 4 with the abi 2 registration active. The transition
 * drives epoch_end for the departing epoch and epoch_begin for the arriving
 * one inside one journal: one sequence is consumed, the staged module write
 * lands and the root is recomputed; a failing hook leaves nothing behind. */
static int transition_checks(lxp_kernel *kernel, lxp_state_store *store,
                             lxp_state_journal *journal)
{
    static uint8_t arena_bytes[16384];
    lxp_arena arena;
    uint8_t root[32];
    uint8_t held[32];
    uint64_t sequence = store->next_sequence;
    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK)
        return 1;
    if (lxp_kernel_epoch_transition(kernel, 5U, 100U, &arena) != LXP_OK ||
        end_calls != 1U || last_end_epoch != 4U ||
        begin_calls != 1U || last_begin_epoch != 5U ||
        kernel->epoch != 5U || store->next_sequence != sequence + 1U ||
        journal->open || kernel->module_kv_count != 1U ||
        kernel->module_kv[0].module_id != LXP_MODULE_ASSET ||
        kernel->module_kv[0].value_length != 8U ||
        kernel->module_kv[0].value[7] != 5U ||
        lxp_state_root(kernel, root) != LXP_OK ||
        memcmp(root, kernel->current_state_root, 32U) != 0)
        return 1;
    (void)memcpy(held, root, 32U);
    if (lxp_kernel_epoch_transition(kernel, 5U, 200U, &arena) !=
            LXP_ERR_IDEMPOTENT_REPLAY ||
        lxp_kernel_epoch_transition(kernel, 4U, 200U, &arena) !=
            LXP_ERR_TIMESTAMP_REGRESSION ||
        lxp_kernel_epoch_transition(kernel, 6U, 0U, &arena) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_kernel_epoch_transition(kernel, 6U, 200U, NULL) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_kernel_epoch_transition(NULL, 6U, 200U, &arena) !=
            LXP_ERR_NON_CANONICAL ||
        end_calls != 1U || begin_calls != 1U || kernel->epoch != 5U ||
        store->next_sequence != sequence + 1U || journal->open)
        return 1;
    begin_status = LXP_ERR_GAS_EXHAUSTED;
    if (lxp_kernel_epoch_transition(kernel, 6U, 200U, &arena) !=
            LXP_ERR_GAS_EXHAUSTED ||
        end_calls != 2U || last_end_epoch != 5U || begin_calls != 2U ||
        kernel->epoch != 5U || store->next_sequence != sequence + 1U ||
        journal->open || kernel->module_kv_count != 1U ||
        kernel->module_kv[0].value[7] != 5U ||
        lxp_state_root(kernel, root) != LXP_OK ||
        memcmp(root, held, 32U) != 0 ||
        memcmp(kernel->current_state_root, held, 32U) != 0)
        return 1;
    begin_status = LXP_OK;
    if (lxp_kernel_epoch_transition(kernel, 6U, 200U, &arena) != LXP_OK ||
        end_calls != 3U || begin_calls != 3U || last_begin_epoch != 6U ||
        kernel->epoch != 6U || store->next_sequence != sequence + 2U ||
        journal->open || kernel->module_kv_count != 1U ||
        kernel->module_kv[0].value[7] != 6U ||
        lxp_state_root(kernel, root) != LXP_OK ||
        memcmp(root, kernel->current_state_root, 32U) != 0 ||
        memcmp(root, held, 32U) == 0)
        return 1;
    return 0;
}

/* Both hooks of a transition stage their writes before either of them
 * commits, so the capacity guarantee each context takes against the kernel's
 * current table is not enough on its own. Three transitions carry the table to
 * 385 of its 512 entries; the fourth stages 64 additions in each hook, so each
 * hook clears its own guarantee against the 127 entries that remain while the
 * pair would drive the table one entry past its end. The transition has to
 * refuse before the journal commits and leave the epoch, the sequence, the
 * table and the root exactly as they were. */
static int capacity_checks(lxp_kernel *kernel, lxp_state_store *store,
                           lxp_state_journal *journal)
{
    static uint8_t arena_bytes[16384];
    lxp_arena arena;
    uint8_t root[32];
    uint8_t held[32];
    uint64_t epoch = kernel->epoch;
    uint64_t sequence = store->next_sequence;
    size_t fill;
    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK)
        return 1;
    stage_bulk = 64U;
    for (fill = 0U; fill < 3U; ++fill) {
        ++epoch;
        if (lxp_kernel_epoch_transition(kernel, epoch, 300U, &arena) !=
                LXP_OK ||
            kernel->epoch != epoch || journal->open)
            return 1;
    }
    if (kernel->module_kv_count != 385U ||
        store->next_sequence != sequence + 3U ||
        lxp_state_root(kernel, held) != LXP_OK ||
        memcmp(kernel->current_state_root, held, 32U) != 0)
        return 1;
    if (lxp_kernel_epoch_transition(kernel, epoch + 1U, 400U, &arena) !=
            LXP_ERR_ARENA_EXHAUSTED ||
        kernel->epoch != epoch || kernel->module_kv_count != 385U ||
        store->next_sequence != sequence + 3U || journal->open ||
        lxp_state_root(kernel, root) != LXP_OK ||
        memcmp(root, held, 32U) != 0 ||
        memcmp(kernel->current_state_root, held, 32U) != 0)
        return 1;
    stage_bulk = 0U;
    return 0;
}

static lxp_module_iface named_iface(uint16_t module_id, const char *name,
                                    const uint32_t *types)
{
    lxp_module_iface iface = make_iface(1U, types, 1U);
    iface.module_id = module_id;
    iface.name = name;
    return iface;
}

static uint32_t read_be(const uint8_t *bytes, size_t width)
{
    uint32_t value = 0U;
    size_t i;
    for (i = 0U; i < width; ++i) value = (value << 8U) | bytes[i];
    return value;
}

/* Genesis names the web module through its enable flag: a manifest that sets
 * it selects the web registration last, after every module below it, and the
 * registered kernel resolves both web activities. */
static int web_genesis_checks(void)
{
    static const uint8_t version_key[32] = "parameter-version";
    static lxp_genesis_manifest manifest;
    static lxp_kernel kernel;
    static lxp_state_store store;
    static lxp_state_journal journal;
    lxp_genesis_module_plan plan;
    const lxp_module_registration *registration;
    uint8_t key[32];
    uint8_t expected[32] = "module-enable:web";
    uint64_t parameters = 1U;
    if (lxp_genesis_module_enable_key(LXP_MODULE_WEB, key) != LXP_OK ||
        memcmp(key, expected, 32U) != 0 ||
        lxp_genesis_module_enable_key(LXP_MODULE_RESERVED_COUNT + 1U, key) !=
            LXP_ERR_UNKNOWN_MODULE)
        return 1;
    (void)memset(&manifest, 0, sizeof(manifest));
    manifest.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    manifest.network_id = 42U;
    manifest.genesis_timestamp_ms = UINT64_C(1700000000000);
    manifest.parameters[0].module_id = LXP_MODULE_GOVERNANCE;
    (void)memcpy(manifest.parameters[0].key, expected, 32U);
    manifest.parameters[0].value[31] = 1U;
    manifest.parameters[1].module_id = LXP_MODULE_GOVERNANCE;
    (void)memcpy(manifest.parameters[1].key, version_key, 32U);
    manifest.parameters[1].value[31] = 1U;
    manifest.parameter_count = 2U;
    manifest.guarantor_count = 1U;
    manifest.guarantors[0].guarantor_id[0] = 1U;
    manifest.guarantors[0].public_key[0] = 2U;
    manifest.guarantors[0].public_key[32] = 3U;
    if (lxp_genesis_module_plan_resolve(&manifest, &plan) != LXP_OK ||
        plan.count < 2U || plan.modules[plan.count - 1U] != lx_web_module_iface())
        return 1;
    (void)memset(&journal, 0, sizeof(journal));
    if (lxp_state_store_init(&store, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &store, &journal, &parameters, 0U) !=
            LXP_OK ||
        lxp_genesis_module_plan_register(&plan, &kernel) != LXP_OK ||
        lxp_genesis_module_plan_matches(&plan, &kernel) != LXP_OK ||
        lxp_kernel_module_for_activity(&kernel, LX_WEB_OBSERVATION_ACTIVITY, 0U,
                                       &registration) != LXP_OK ||
        registration->module_id != LXP_MODULE_WEB ||
        lxp_kernel_module_for_activity(&kernel, LX_WEB_ATTESTOR_SET_ACTIVITY,
                                       0U, &registration) != LXP_OK ||
        registration->module_id != LXP_MODULE_WEB ||
        lxp_state_store_destroy(&store) != LXP_OK)
        return 1;
    manifest.parameters[0].value[31] = 0U;
    if (lxp_genesis_module_plan_resolve(&manifest, &plan) != LXP_OK ||
        plan.modules[plan.count - 1U] == lx_web_module_iface())
        return 1;
    return 0;
}

/* The public handover genesis lists every registered module by id, so module
 * 11 takes the last slot of the ordered table while module 12 is refused by
 * registration, lookup and runtime binding alike. */
static int web_handover_checks(void)
{
    static const uint32_t governance_types[] = { UINT32_C(0x00070001) };
    static const uint32_t twelve_types[] = { UINT32_C(0x000C0001) };
    static const uint8_t authority_key[32] = "handover-authority";
    static lxp_kernel kernel;
    static lxp_state_store store;
    static lxp_state_journal journal;
    static lx_web_store web;
    static uint8_t arena_bytes[LXP_STATE_WITNESS_MAX_BYTES + 8192U];
    lxp_module_iface governance = named_iface(LXP_MODULE_GOVERNANCE,
                                              "governance", governance_types);
    lxp_module_iface twelve = named_iface(LXP_MODULE_RESERVED_COUNT + 1U,
                                          "twelve", twelve_types);
    const lxp_module_registration *registration;
    lxp_module_kv_entry *entry;
    lxp_byte_span encoded;
    lxp_arena arena;
    uint8_t state_root[32];
    const uint8_t *tail;
    uint64_t parameters = 1U;
    (void)memset(&journal, 0, sizeof(journal));
    (void)memset(&web, 0, sizeof(web));
    if (lxp_state_store_init(&store, 1U) != LXP_OK ||
        lxp_kernel_create(&kernel, &store, &journal, &parameters, 0U) !=
            LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_web_module_iface()) != LXP_OK ||
        lxp_kernel_register_module(&kernel, &governance) != LXP_OK ||
        lxp_kernel_register_module(&kernel, &twelve) !=
            LXP_ERR_UNKNOWN_MODULE ||
        lxp_kernel_module_by_id(&kernel, LXP_MODULE_RESERVED_COUNT + 1U, 0U,
                                &registration) != LXP_ERR_UNKNOWN_MODULE ||
        lxp_kernel_module_for_activity(&kernel, twelve_types[0], 0U,
                                       &registration) !=
            LXP_ERR_UNKNOWN_MODULE ||
        lxp_kernel_module_by_id(&kernel, LXP_MODULE_WEB, 0U, &registration) !=
            LXP_OK ||
        lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_WEB, &web) !=
            LXP_ERR_NON_CANONICAL)
        return 1;
    web.network_id = 42U;
    if (lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_WEB, &web) !=
            LXP_OK ||
        kernel.module_runtime[LXP_MODULE_WEB] != &web ||
        lxp_kernel_bind_module_runtime(&kernel,
                                       LXP_MODULE_RESERVED_COUNT + 1U, &web) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_kernel_set_epoch(&kernel, 1U) != LXP_OK)
        return 1;
    kernel.handover.enabled = true;
    kernel.handover.network_id = 42U;
    (void)memset(kernel.handover.governance_public_key, 0x42, 32U);
    (void)memset(kernel.handover.genesis_authorization.public_key, 0x24, 32U);
    entry = &kernel.module_kv[0];
    (void)memset(entry, 0, sizeof(*entry));
    entry->module_id = LXP_MODULE_GOVERNANCE;
    entry->key_length = 32U;
    (void)memcpy(entry->key, authority_key, 32U);
    entry->value_length = 32U;
    (void)memcpy(entry->value, kernel.handover.governance_public_key, 32U);
    kernel.module_kv_count = 1U;
    if (lxp_state_root(&kernel, state_root) != LXP_OK ||
        lxp_genesis_receipt_state_root(42U, state_root,
                                       kernel.current_state_root) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_handover_genesis_trust_encode(&kernel, &arena, &encoded) !=
            LXP_OK ||
        encoded.length < 2U * (2U + 4U) + 3U * 4U + 4U)
        return 1;
    /* The module list closes the encoding: the count, governance then web in
     * module order although web registered first. */
    tail = encoded.bytes + encoded.length - (2U * (2U + 4U) + 3U * 4U + 4U);
    if (read_be(tail, 4U) != 2U ||
        read_be(tail + 4U, 2U) != LXP_MODULE_GOVERNANCE ||
        read_be(tail + 6U, 4U) != 1U ||
        read_be(tail + 10U, 4U) != governance_types[0] ||
        read_be(tail + 14U, 2U) != LXP_MODULE_WEB ||
        read_be(tail + 16U, 4U) != 2U ||
        read_be(tail + 20U, 4U) != LX_WEB_OBSERVATION_ACTIVITY ||
        read_be(tail + 24U, 4U) != LX_WEB_ATTESTOR_SET_ACTIVITY ||
        lxp_state_store_destroy(&store) != LXP_OK)
        return 1;
    return 0;
}

int main(void)
{
    static const uint32_t v1_types[] = { UINT32_C(0x00010001),
                                         UINT32_C(0x00010002) };
    static const uint32_t v2_types[] = { UINT32_C(0x00010001),
                                         UINT32_C(0x00010003) };
    static const uint32_t unsorted[] = { UINT32_C(0x00010002),
                                         UINT32_C(0x00010001) };
    lxp_state_store store;
    lxp_state_journal journal;
    lxp_kernel kernel;
    uint64_t parameters = 1U;
    lxp_module_iface v1 = make_iface(1U, v1_types, 2U);
    lxp_module_iface v2 = make_iface(2U, v2_types, 2U);
    lxp_module_iface bad = make_iface(3U, unsorted, 2U);
    lxp_module_iface unterminated = make_iface(3U, v1_types, 2U);
    char unterminated_name[LXP_MODULE_MAX_NAME + 1U];
    const lxp_module_registration *registration;
    (void)memset(unterminated_name, 'a', sizeof(unterminated_name));
    (void)memset(&journal, 0, sizeof(journal));
    unterminated.name = unterminated_name;
    if (lxp_state_store_init(&store, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &store, &journal, &parameters, 0U) !=
            LXP_OK ||
        lxp_kernel_register_module(&kernel, &v1) != LXP_OK ||
        lxp_kernel_module_for_activity(&kernel, v1_types[1], 0U,
                                       &registration) != LXP_OK ||
        registration->abi_version != 1U ||
        lxp_kernel_module_for_activity(&kernel, UINT32_C(0x00010003), 0U,
                                       &registration) !=
            LXP_ERR_UNKNOWN_ACTIVITY ||
        lxp_kernel_module_for_activity(&kernel, UINT32_C(0x00020001), 0U,
                                       &registration) !=
            LXP_ERR_MODULE_DISABLED ||
        lxp_kernel_register_module(&kernel, &bad) !=
            LXP_ERR_UNSORTED_SEQUENCE ||
        lxp_kernel_register_module(&kernel, &unterminated) !=
            LXP_ERR_LENGTH_LIMIT ||
        lxp_kernel_set_epoch(&kernel, 4U) != LXP_OK ||
        lxp_kernel_set_epoch(&kernel, 4U) != LXP_OK ||
        kernel.epoch != 4U ||
        lxp_kernel_register_module(&kernel, &v2) != LXP_OK ||
        lxp_kernel_module_for_activity(&kernel, v1_types[1], 3U,
                                       &registration) != LXP_OK ||
        registration->abi_version != 1U ||
        lxp_kernel_module_for_activity(&kernel, v2_types[1], 4U,
                                       &registration) != LXP_OK ||
        registration->abi_version != 2U ||
        lxp_kernel_module_for_activity(&kernel, v1_types[1], 4U,
                                       &registration) !=
            LXP_ERR_UNKNOWN_ACTIVITY ||
        lxp_kernel_set_epoch(&kernel, 3U) != LXP_ERR_TIMESTAMP_REGRESSION ||
        kernel.epoch != 4U) return 1;
    if (transition_checks(&kernel, &store, &journal) != 0) return 1;
    if (capacity_checks(&kernel, &store, &journal) != 0) return 1;
    if (lxp_state_store_destroy(&store) != LXP_OK) return 1;
    if (web_genesis_checks() != 0 || web_handover_checks() != 0) return 1;
    return 0;
}
