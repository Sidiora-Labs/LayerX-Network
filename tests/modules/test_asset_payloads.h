#ifndef TEST_ASSET_PAYLOADS_H
#define TEST_ASSET_PAYLOADS_H

#include <stdio.h>
#include <stdlib.h>

#define PAYLOAD_CHECK(expr) do { if (!(expr)) { \
    (void)fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #expr); \
    return 1; \
} } while (0)

static int decode_prefixes(lxp_module_ctx *ctx, uint16_t ordinal,
                            uint8_t *bytes, size_t length)
{
    const lxp_module_iface *iface = lx_asset_module_iface();
    size_t index;
    void *decoded = NULL;
    for (index = 0U; index < length; ++index) {
        PAYLOAD_CHECK(lxp_arena_reset(ctx->arena, 0U) == LXP_OK);
        PAYLOAD_CHECK(iface->decode(ctx, ordinal, bytes, index, &decoded) != LXP_OK);
    }
    PAYLOAD_CHECK(lxp_arena_reset(ctx->arena, 0U) == LXP_OK);
    PAYLOAD_CHECK(iface->decode(ctx, ordinal, bytes, length, &decoded) == LXP_OK);
    PAYLOAD_CHECK(decoded != NULL);
    PAYLOAD_CHECK(lxp_arena_reset(ctx->arena, 0U) == LXP_OK);
    PAYLOAD_CHECK(iface->decode(ctx, ordinal, bytes, length + 1U, &decoded) != LXP_OK);
    return 0;
}

static int test_asset_payloads(lxp_kernel *kernel)
{
    const lxp_module_iface *iface = lx_asset_module_iface();
    const uint32_t expected[] = {LX_ASSET_REGISTER, LX_ASSET_PAUSE,
        LX_ASSET_UNPAUSE, LX_ASSET_ACCOUNT_OPEN, LX_ASSET_SEND,
        LX_ASSET_RECEIVE, LX_ASSET_GRANT_ISSUE, LX_ASSET_GRANT_REVOKE,
        LX_ASSET_WITHDRAW, LX_ASSET_MINT, LX_ASSET_BURN};
    uint8_t arena_bytes[8192];
    uint8_t bytes[1024] = {0};
    uint8_t roundtrip[1024];
    lxp_arena arena;
    lxp_module_ctx *ctx = calloc(1U, sizeof(*ctx));
    lx_asset_register_payload registration;
    lx_asset_account_open_payload account;
    lx_asset_supply_payload supply;
    lx_asset_grant_revoke_payload revocation;
    lxp_receive receive = {0};
    lxp_payer_grant grant;
    void *decoded;
    size_t length;
    size_t roundtrip_length;
    size_t index;
    PAYLOAD_CHECK(ctx != NULL);
    PAYLOAD_CHECK(iface->activity_type_count == sizeof(expected) / sizeof(expected[0]));
    PAYLOAD_CHECK(memcmp(iface->activity_types, expected, sizeof(expected)) == 0);
    PAYLOAD_CHECK(lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) == LXP_OK);
    PAYLOAD_CHECK(lxp_module_ctx_init(ctx, kernel, LXP_MODULE_ASSET,
        0U, 0U, 0U, UINT64_MAX, &arena, false) == LXP_OK);
    PAYLOAD_CHECK(iface->decode(ctx, 12U, bytes, 0U, &decoded) == LXP_ERR_UNKNOWN_ACTIVITY);
    bytes[1] = 1U;
    bytes[2] = 7U;
    bytes[34] = 8U;
    bytes[66] = 1U;
    bytes[67] = 'x';
    bytes[68] = 1U;
    bytes[69] = 'n';
    bytes[70] = 38U;
    bytes[86] = 23U;
    bytes[87] = 1U;
    PAYLOAD_CHECK(decode_prefixes(ctx, 1U, bytes, 89U) == 0);
    PAYLOAD_CHECK(lx_asset_register_decode(bytes, 89U, &registration) == LXP_OK);
    PAYLOAD_CHECK(registration.asset_id[0] == 7U && registration.salt[0] == 8U &&
        registration.supply_cap.lo == 23U && registration.supply_cap.hi == 0U &&
        registration.decimals == 38U && registration.symbol[0] == 'x' &&
        registration.name[0] == 'n');
    bytes[1] = 2U;
    PAYLOAD_CHECK(lx_asset_register_decode(bytes, 89U, &registration) != LXP_OK);
    bytes[1] = 1U;
    bytes[66] = 0U;
    PAYLOAD_CHECK(lx_asset_register_decode(bytes, 89U, &registration) != LXP_OK);
    bytes[66] = 17U;
    PAYLOAD_CHECK(lx_asset_register_decode(bytes, 89U, &registration) != LXP_OK);
    bytes[66] = 1U;
    bytes[67] = 0x80U;
    PAYLOAD_CHECK(lx_asset_register_decode(bytes, 89U, &registration) != LXP_OK);
    bytes[67] = 'x';
    bytes[68] = 0U;
    PAYLOAD_CHECK(lx_asset_register_decode(bytes, 89U, &registration) != LXP_OK);
    bytes[68] = 33U;
    PAYLOAD_CHECK(lx_asset_register_decode(bytes, 89U, &registration) != LXP_OK);
    bytes[68] = 1U;
    bytes[69] = 0xffU;
    PAYLOAD_CHECK(lx_asset_register_decode(bytes, 89U, &registration) != LXP_OK);
    bytes[69] = 'n';
    bytes[70] = 39U;
    PAYLOAD_CHECK(lx_asset_register_decode(bytes, 89U, &registration) != LXP_OK);
    bytes[70] = 38U;
    bytes[87] = 3U;
    PAYLOAD_CHECK(lx_asset_register_decode(bytes, 89U, &registration) != LXP_OK);
    bytes[87] = 1U;
    bytes[88] = 1U;
    PAYLOAD_CHECK(lx_asset_register_decode(bytes, 90U, &registration) != LXP_OK);
    bytes[87] = 2U;
    bytes[88] = 128U;
    PAYLOAD_CHECK(decode_prefixes(ctx, 1U, bytes, 217U) == 0);
    bytes[88] = 129U;
    PAYLOAD_CHECK(lx_asset_register_decode(bytes, 218U, &registration) != LXP_OK);
    {
        const uint8_t invalid_utf8[][4] = {
            {0xc0U, 0x80U, 0U, 0U}, {0xedU, 0xa0U, 0x80U, 0U},
            {0xf4U, 0x90U, 0x80U, 0x80U}, {0xe2U, 0x82U, 0U, 0U}
        };
        const size_t sizes[] = {2U, 3U, 4U, 2U};
        for (index = 0U; index < 4U; ++index) {
            (void)memset(bytes, 0, sizeof(bytes));
            bytes[1] = 1U;
            bytes[66] = 1U;
            bytes[67] = 'x';
            bytes[68] = (uint8_t)sizes[index];
            (void)memcpy(bytes + 69U, invalid_utf8[index], sizes[index]);
            bytes[86U + sizes[index]] = 1U;
            PAYLOAD_CHECK(lx_asset_register_decode(bytes, 88U + sizes[index],
                                                   &registration) != LXP_OK);
        }
        (void)memset(bytes, 0, sizeof(bytes));
        bytes[1] = 1U;
        bytes[66] = 16U;
        (void)memset(bytes + 67U, 'x', 16U);
        bytes[83] = 32U;
        (void)memset(bytes + 84U, 'n', 32U);
        bytes[84] = 0xf0U;
        bytes[85] = 0x9fU;
        bytes[86] = 0x92U;
        bytes[87] = 0xb0U;
        bytes[133] = 1U;
        PAYLOAD_CHECK(decode_prefixes(ctx, 1U, bytes, 135U) == 0);
    }
    (void)memset(bytes, 0, sizeof(bytes));
    bytes[1] = 1U;
    bytes[2] = 7U;
    PAYLOAD_CHECK(decode_prefixes(ctx, 4U, bytes, 34U) == 0);
    PAYLOAD_CHECK(decode_prefixes(ctx, 2U, bytes, 34U) == 0);
    PAYLOAD_CHECK(decode_prefixes(ctx, 3U, bytes, 34U) == 0);
    PAYLOAD_CHECK(lx_asset_account_open_decode(bytes, 34U, &account) == LXP_OK &&
                  account.asset_id[0] == 7U);
    bytes[1] = 2U;
    PAYLOAD_CHECK(lx_asset_account_open_decode(bytes, 34U, &account) != LXP_OK);
    PAYLOAD_CHECK(lxp_arena_reset(ctx->arena, 0U) == LXP_OK);
    PAYLOAD_CHECK(iface->decode(ctx, 2U, bytes, 34U, &decoded) ==
                  LXP_ERR_NON_CANONICAL);
    PAYLOAD_CHECK(lxp_arena_reset(ctx->arena, 0U) == LXP_OK);
    PAYLOAD_CHECK(iface->decode(ctx, 3U, bytes, 34U, &decoded) ==
                  LXP_ERR_NON_CANONICAL);
    bytes[1] = 1U;
    bytes[34] = 1U;
    bytes[41] = 9U;
    PAYLOAD_CHECK(decode_prefixes(ctx, 8U, bytes, 42U) == 0);
    PAYLOAD_CHECK(lx_asset_grant_revoke_decode(bytes, 42U, &revocation) == LXP_OK &&
        revocation.revocation_sequence == UINT64_C(0x0100000000000009));
    bytes[1] = 2U;
    PAYLOAD_CHECK(lx_asset_grant_revoke_decode(bytes, 42U, &revocation) != LXP_OK);
    bytes[1] = 1U;
    bytes[81] = 1U;
    for (index = 10U; index <= 11U; ++index)
        PAYLOAD_CHECK(decode_prefixes(ctx, (uint16_t)index, bytes, 82U) == 0);
    PAYLOAD_CHECK(lx_asset_supply_decode(bytes, 82U, &supply) == LXP_OK &&
                  supply.amount.lo == 1U && supply.account_id[0] == 1U);
    bytes[81] = 0U;
    PAYLOAD_CHECK(lx_asset_supply_decode(bytes, 82U, &supply) == LXP_ERR_INVALID_AMOUNT);
    bytes[81] = 1U;
    bytes[1] = 2U;
    PAYLOAD_CHECK(lx_asset_supply_decode(bytes, 82U, &supply) != LXP_OK);
    receive.payer_grant.recurring = true;
    receive.payer_grant.has_reference = true;
    receive.payer_grant.allowance = (lxp_u128){3U, 4U};
    PAYLOAD_CHECK(lxp_receive_encode(&receive, bytes, sizeof(bytes), &length) == LXP_OK);
    PAYLOAD_CHECK(decode_prefixes(ctx, 6U, bytes, length) == 0);
    bytes[0] ^= 1U;
    PAYLOAD_CHECK(lxp_receive_decode(bytes, length, &receive) != LXP_OK);
    (void)memset(&grant, 0, sizeof(grant));
    grant.recurring = true;
    grant.has_reference = true;
    grant.allowance = (lxp_u128){3U, 4U};
    PAYLOAD_CHECK(lxp_payer_grant_encode(&grant, bytes, sizeof(bytes), &length) == LXP_OK);
    PAYLOAD_CHECK(decode_prefixes(ctx, 7U, bytes, length) == 0);
    PAYLOAD_CHECK(lxp_payer_grant_decode(bytes, length, &grant) == LXP_OK);
    PAYLOAD_CHECK(grant.recurring && grant.has_reference && grant.allowance.hi == 3U &&
                  grant.allowance.lo == 4U);
    PAYLOAD_CHECK(lxp_payer_grant_encode(&grant, roundtrip, sizeof(roundtrip),
                                        &roundtrip_length) == LXP_OK);
    PAYLOAD_CHECK(length == roundtrip_length && memcmp(bytes, roundtrip, length) == 0);
    bytes[160] = 2U;
    PAYLOAD_CHECK(lxp_payer_grant_decode(bytes, length, &grant) != LXP_OK);
    bytes[160] = 1U;
    bytes[209] = 2U;
    PAYLOAD_CHECK(lxp_payer_grant_decode(bytes, length, &grant) != LXP_OK);
    free(ctx);
    return 0;
}

#undef PAYLOAD_CHECK
#endif
