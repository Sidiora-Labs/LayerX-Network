#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_ledger.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef struct vector {
    const char *name;
    lx_account_kind kind;
    const char *identifier;
} vector;

static const vector vectors[] = {
    { "agent:did:key:alice:asset:0300000000000000000000000000000000000000000000000000000000000000", LX_ACCOUNT_AGENT_MAIN, "cd99e00b8c6897b6ba6e250492de75103b65fcb5e7c48313bbb9183237a2c3e7" },
    { "agent:did:key:alice:main", LX_ACCOUNT_AGENT_MAIN, "efc9802f76722dfc48ebfed35bfd8b20dbc2775fe2f027d6cbd595aff1307454" },
    { "agent:did:key:alice:budget:daily", LX_ACCOUNT_AGENT_BUDGET, "6f27c8a878c055eeb4056e5bf32864cbf113748af82e6b72e2060c39b4a18829" },
    { "agent:did:key:alice:escrow:order-7", LX_ACCOUNT_AGENT_ESCROW, "a781282c755f899aeff3881c53d68b14ed796f97c5f05d9b5b9da90d7c7aecc2" },
    { "agent:did:key:alice:stream:salary", LX_ACCOUNT_AGENT_STREAM, "5ab8f69df1347e5fe0f8f8fbe2db3d7e3d84d5af2c6641033aef5b08ff95a636" },
    { "agent:did:key:alice:margin:btc-usd", LX_ACCOUNT_AGENT_MARGIN, "4fe1c45e2186de26cb1b5970605e3b30e37b56b8ec21e08f3b9c4c721dd091b3" },
    { "system:liquidity:btc-usd", LX_ACCOUNT_SYSTEM_LIQUIDITY, "b3b2bb3b51a162524acd6f07af8f0f3993c35144c6fa1ad41493c9384c5c42de" },
    { "system:funding:btc-usd:long", LX_ACCOUNT_SYSTEM_FUNDING_LONG, "fb75d9a061bf420ddf370b14d320d78b972d448959f6b4b46bb00b0a6b6d5869" },
    { "system:funding:btc-usd:short", LX_ACCOUNT_SYSTEM_FUNDING_SHORT, "db2afd847c8e447f9cdab3034072e2ff8bf8b2c49e65dccaede6906b0d94bbe3" },
    { "system:insurance", LX_ACCOUNT_SYSTEM_INSURANCE, "4c16a537b274bd21ce39ebc501c464f4c7ac6a60d7eb4d396948570cbb764568" },
    { "system:fees", LX_ACCOUNT_SYSTEM_FEES, "dbf940aa4c1f587b73f3b65da0dec92760e0bd0187f1f33c659ea043caebdcdf" },
    { "system:paxeer-reserve", LX_ACCOUNT_SYSTEM_PAXEER_RESERVE, "6e0e5cca5cfaa1b20ddd1c6174321eeaf00dd74bce2adcd78b61e15aa9e26f7c" },
    { "system:paxeer-withdrawals", LX_ACCOUNT_SYSTEM_PAXEER_WITHDRAWALS, "36e5bfab4a0143c723aaac6a61d6eae554e684f94cfff2a24f5fdf0574c468f7" }
};

static int nibble(char value)
{
    if (value >= '0' && value <= '9') return value - '0';
    return value - 'a' + 10;
}

static void decode_hex(const char *hex, uint8_t out[32])
{
    size_t i;
    for (i = 0U; i < 32U; ++i)
        out[i] = (uint8_t)((nibble(hex[i * 2U]) << 4) |
                           nibble(hex[i * 2U + 1U]));
}

int main(void)
{
    lx_account_registry registry;
    lx_account *account;
    uint8_t actual[32];
    uint8_t expected[32];
    uint8_t mismatch[32] = { 0U };
    size_t i;
    char directory[] = "/tmp/lxp-ledger-account-XXXXXX";
    char path[128];
    lxp_log log;
    lxp_log_record_header header;
    uint8_t body[1024];
    const char *lazy = "agent:did:key:bob:budget:food";
    const char *system = "system:fees";

    for (i = 0U; i < sizeof(vectors) / sizeof(vectors[0]); ++i) {
        lx_account_kind kind;
        decode_hex(vectors[i].identifier, expected);
        if (lx_account_id_from_string((const uint8_t *)vectors[i].name,
                                      strlen(vectors[i].name), actual) != LXP_OK ||
            memcmp(actual, expected, sizeof(actual)) != 0 ||
            lx_account_kind_of((const uint8_t *)vectors[i].name,
                               strlen(vectors[i].name), &kind) != LXP_OK ||
            kind != vectors[i].kind) return 1;
    }
    if (lx_account_id_from_string((const uint8_t *)"agent::main", 11U,
                                  actual) != LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE ||
        lx_account_id_from_string((const uint8_t *)"agent:ALICE:main", 16U,
                                  actual) != LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE)
        return 1;
    if (lx_account_registry_init(&registry) != LXP_OK ||
        mkdtemp(directory) == NULL ||
        lxp_log_segment_create(&log, directory, 0U, 4096U) != LXP_OK)
        return 1;
    if (snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0)
        return 1;
    if (lx_account_id_from_string((const uint8_t *)lazy, strlen(lazy), actual) !=
        LXP_OK || lx_account_open(&registry, (const uint8_t *)lazy, strlen(lazy),
                                 mismatch, 7U, LX_ACCOUNT_OPEN_CREDIT, &log,
                                 &account) != LXP_ERR_ACCOUNT_ID_MISMATCH ||
        lx_account_open(&registry, (const uint8_t *)lazy, strlen(lazy), actual,
                        7U, LX_ACCOUNT_OPEN_CREDIT, &log, &account) != LXP_OK ||
        registry.count != 1U || account->created_at_sequence != 7U ||
        !lxp_u128_is_zero(account->balance) ||
        lxp_log_read(&log, 0U, &header, body, sizeof(body)) != LXP_OK ||
        header.record_kind != LXP_LOG_ACTIVITY || header.global_sequence != 7U)
        return 1;
    account->has_open_reference = true;
    if (lx_account_close(&registry, actual) != LXP_ERR_ACCOUNT_NOT_EMPTY)
        return 1;
    account->has_open_reference = false;
    if (lx_account_close(&registry, actual) != LXP_OK || registry.count != 0U)
        return 1;
    if (lx_account_id_from_string((const uint8_t *)system, strlen(system),
                                  actual) != LXP_OK ||
        lx_account_open(&registry, (const uint8_t *)system, strlen(system), actual,
                        8U, LX_ACCOUNT_OPEN_CREDIT, NULL, &account) !=
            LXP_ERR_UNAUTHORIZED_DEBIT ||
        lx_account_open(&registry, (const uint8_t *)system, strlen(system), actual,
                        8U, LX_ACCOUNT_OPEN_GENESIS, NULL, &account) != LXP_OK)
        return 1;
    if (lxp_log_close(&log) != LXP_OK || unlink(path) != 0 ||
        rmdir(directory) != 0) return 1;
    {
        static const uint8_t hex[] = "0123456789abcdef";
        uint8_t asset_id[32];
        uint8_t current_name[LX_ASSET_ISSUANCE_NAME_BYTES];
        uint8_t expected_id[32];
        uint8_t retired[79];
        lx_account_name parsed_name;
        lx_account retired_account;
        lx_account already_current;
        lx_account_registry rewrite_registry;
        size_t renamed = 0U;
        size_t index;
        (void)memset(asset_id, 0x11, sizeof(asset_id));
        (void)memcpy(retired, "asset:", 6U);
        for (index = 0U; index < 32U; ++index) {
            retired[6U + index * 2U] = hex[asset_id[index] >> 4U];
            retired[7U + index * 2U] = hex[asset_id[index] & 15U];
        }
        (void)memcpy(retired + 70U, ":issuance", 9U);
        if (lx_account_id_from_string(retired, sizeof(retired), actual) !=
                LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE ||
            lx_account_name_parse(retired, sizeof(retired), &parsed_name) !=
                LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE ||
            lx_asset_issuance_name(asset_id, current_name, expected_id) !=
                LXP_OK)
            return 1;
        (void)memset(&retired_account, 0, sizeof(retired_account));
        (void)memcpy(retired_account.name, retired, sizeof(retired));
        retired_account.name_length = (uint16_t)sizeof(retired);
        (void)memcpy(retired_account.id, expected_id, sizeof(expected_id));
        retired_account.kind = LX_ACCOUNT_MODULE_VALUE;
        retired_account.balance = (lxp_u128){0U, 100U};
        (void)memcpy(retired_account.asset_id, asset_id, sizeof(asset_id));
        retired_account.has_asset = true;
        retired_account.next_sequence = 4U;
        retired_account.created_at_sequence = 2U;
        if (lx_account_validate_canonical(&retired_account) != LXP_OK)
            return 1;
        retired_account.id[0] ^= 1U;
        if (lx_account_validate_canonical(&retired_account) !=
                LXP_ERR_ACCOUNT_ID_MISMATCH ||
            lx_account_rewrite_retired_issuance(&retired_account) !=
                LXP_ERR_ACCOUNT_ID_MISMATCH)
            return 1;
        retired_account.id[0] ^= 1U;
        if (lx_account_rewrite_retired_issuance(&retired_account) != LXP_OK ||
            retired_account.name_length != LX_ASSET_ISSUANCE_NAME_BYTES ||
            memcmp(retired_account.name, current_name,
                   LX_ASSET_ISSUANCE_NAME_BYTES) != 0 ||
            memcmp(retired_account.id, expected_id, sizeof(expected_id)) != 0 ||
            lxp_u128_cmp(retired_account.balance, (lxp_u128){0U, 100U}) != 0 ||
            memcmp(retired_account.asset_id, asset_id, sizeof(asset_id)) != 0 ||
            retired_account.next_sequence != 4U ||
            retired_account.created_at_sequence != 2U ||
            retired_account.kind != LX_ACCOUNT_MODULE_VALUE ||
            lx_account_validate_canonical(&retired_account) != LXP_OK ||
            lx_account_rewrite_retired_issuance(&retired_account) !=
                LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE)
            return 1;
        already_current = retired_account;
        if (lx_account_registry_init(&rewrite_registry) != LXP_OK)
            return 1;
        rewrite_registry.accounts[0] = already_current;
        rewrite_registry.count = 1U;
        if (lx_account_registry_rewrite_retired_issuance(
                &rewrite_registry, &renamed) !=
                LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE ||
            renamed != 0U)
            return 1;
        (void)memset(&retired_account, 0, sizeof(retired_account));
        (void)memcpy(retired_account.name, retired, sizeof(retired));
        retired_account.name_length = (uint16_t)sizeof(retired);
        (void)memcpy(retired_account.id, expected_id, sizeof(expected_id));
        retired_account.kind = LX_ACCOUNT_MODULE_VALUE;
        retired_account.balance = (lxp_u128){0U, 100U};
        (void)memcpy(retired_account.asset_id, asset_id, sizeof(asset_id));
        retired_account.has_asset = true;
        retired_account.next_sequence = 4U;
        retired_account.created_at_sequence = 2U;
        rewrite_registry.accounts[0] = retired_account;
        rewrite_registry.accounts[1] = already_current;
        rewrite_registry.count = 2U;
        if (lx_account_registry_rewrite_retired_issuance(
                &rewrite_registry, &renamed) != LXP_OK ||
            renamed != 1U ||
            rewrite_registry.accounts[0].name_length !=
                LX_ASSET_ISSUANCE_NAME_BYTES ||
            memcmp(rewrite_registry.accounts[0].name, current_name,
                   LX_ASSET_ISSUANCE_NAME_BYTES) != 0 ||
            memcmp(rewrite_registry.accounts[0].id, expected_id,
                   sizeof(expected_id)) != 0 ||
            memcmp(rewrite_registry.accounts[1].id, already_current.id,
                   sizeof(already_current.id)) != 0)
            return 1;
    }
    return 0;
}
