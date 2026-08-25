#include "layerx/lxp_kernel.h"

#include <stdint.h>
#include <string.h>

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

static lxp_result epoch(lxp_module_ctx *ctx, uint64_t number, uint64_t timestamp)
{
    (void)ctx;
    (void)number;
    (void)timestamp;
    return LXP_OK;
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
    iface.epoch_begin = epoch;
    iface.epoch_end = epoch;
    iface.state_root = state_root;
    return iface;
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
        lxp_state_store_destroy(&store) != LXP_OK) return 1;
    return 0;
}
