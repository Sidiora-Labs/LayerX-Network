#include "lxp_test_harness.h"

#include "layerx/lxp_activity.h"
#include "layerx/lxp_arena.h"
#include "layerx/lxp_authority.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_module.h"

#include <stdlib.h>
#include <string.h>

#define TEST_ACTIVITY UINT32_C(0x00070005)

enum {
    TEST_BATCH_TIMESTAMP = 1000,
    TEST_MATURE_TIMESTAMP = 1300,
    TEST_NOT_BEFORE = 900,
    TEST_NOT_AFTER = 1100,
    TEST_LATE_NOT_AFTER = 2000,
    TEST_LATE_BOUND = 1400,
    TEST_WINDOW = 300000,
    TEST_SEQUENCE = 12,
    TEST_REVOCATION_SEQUENCE = 7,
    TEST_EARLIEST_SEQUENCE = 40,
    TEST_EARLIEST_TIMESTAMP = 1200,
    TEST_SIGNERS = 3,
    TEST_THRESHOLD = 2
};

static const uint64_t declared_mask = (UINT64_C(1) << LXP_MODULE_ASSET) |
                                      (UINT64_C(1) << LXP_MODULE_GOVERNANCE);

static const char fixture_owner[] =
    "0001200101000000201112131415161718191a1b1c1d1e1f20212223242526272829"
    "2a2b2c2d2e2f30000000204142434445464748494a4b4c4d4e4f5051525354555657"
    "58595a5b5c5d5e5f6001000000207172737475767778797a7b7c7d7e7f8081828384"
    "85868788898a8b8c8d8e8f900102030405060709000103e900000020a1a2a3a4a5a6"
    "a7a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc00000000000000001"
    "00000000000003e90000000000000001000000000000138900000000000000000000"
    "0000000000080000000000000065000000000000000000000000000007d100000000"
    "000000000000000000000004000000000000138900000020d1d2d3d4d5d6d7d8d9da"
    "dbdcdddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeeff000000000000003e900000000"
    "000007d100000000000000080000000000000000000000004002030405060708090a"
    "0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c"
    "2d2e2f303132333435363738393a3b3c3d3e3f4041";

static const char fixture_session_key[] =
    "00012001010000002012131415161718191a1b1c1d1e1f202122232425262728292a"
    "2b2c2d2e2f30310000002042434445464748494a4b4c4d4e4f505152535455565758"
    "595a5b5c5d5e5f6061020000002072737475767778797a7b7c7d7e7f808182838485"
    "868788898a8b8c8d8e8f9091010203040506070a000203ea00000020a2a3a4a5a6a7"
    "a8a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c10000000000000002"
    "00000000000003ea0000000000000002000000000000138a00000000000000000000"
    "0000000000090000000000000066000000000000000000000000000007d200000000"
    "000000000000000000000005000000000000138a00000020d2d3d4d5d6d7d8d9dadb"
    "dcdddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f100000000000003ea00000000"
    "000007d2000000000000000900000000000000000000000040030405060708090a0b"
    "0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d"
    "2e2f303132333435363738393a3b3c3d3e3f404142";

static const char fixture_delegated_capability[] =
    "000120010100000020131415161718191a1b1c1d1e1f202122232425262728292a2b"
    "2c2d2e2f30313200000020434445464748494a4b4c4d4e4f50515253545556575859"
    "5a5b5c5d5e5f6061620300000020737475767778797a7b7c7d7e7f80818283848586"
    "8788898a8b8c8d8e8f909192010203040506070b000303eb00000020a3a4a5a6a7a8"
    "a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c20000000000000003"
    "00000000000003eb0000000000000003000000000000138b00000000000000000000"
    "00000000000a0000000000000067000000000000000000000000000007d300000000"
    "000000000000000000000006000000000000138b00000020d3d4d5d6d7d8d9dadbdc"
    "dddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f200000000000003eb00000000"
    "000007d3000000000000000a000000000000000000000000400405060708090a0b0c"
    "0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e"
    "2f303132333435363738393a3b3c3d3e3f40414243";

static const char fixture_budget_allowance[] =
    "0001200101000000201415161718191a1b1c1d1e1f202122232425262728292a2b2c"
    "2d2e2f30313233000000204445464748494a4b4c4d4e4f505152535455565758595a"
    "5b5c5d5e5f6061626304000000207475767778797a7b7c7d7e7f8081828384858687"
    "88898a8b8c8d8e8f90919293010203040506070c000403ec00000020a4a5a6a7a8a9"
    "aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c30000000000000004"
    "00000000000003ec0000000000000004000000000000138c00000000000000000000"
    "00000000000b0000000000000068000000000000000000000000000007d400000000"
    "000000000000000000000007000000000000138c00000020d4d5d6d7d8d9dadbdcdd"
    "dedfe0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f2f300000000000003ec00000000"
    "000007d4000000000000000b0000000000000000000000004005060708090a0b0c0d"
    "0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f"
    "303132333435363738393a3b3c3d3e3f4041424344";

static const char fixture_escrow[] =
    "00012001010000002015161718191a1b1c1d1e1f202122232425262728292a2b2c2d"
    "2e2f30313233340000002045464748494a4b4c4d4e4f505152535455565758595a5b"
    "5c5d5e5f6061626364050000002075767778797a7b7c7d7e7f808182838485868788"
    "898a8b8c8d8e8f9091929394010203040506070d000503ed00000020a5a6a7a8a9aa"
    "abacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c3c40000000000000005"
    "00000000000003ed0000000000000005000000000000138d00000000000000000000"
    "00000000000c0000000000000069000000000000000000000000000007d500000000"
    "000000000000000000000008000000000000138d00000020d5d6d7d8d9dadbdcddde"
    "dfe0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f2f3f400000000000003ed00000000"
    "000007d5000000000000000c01000000000000109200000040060708090a0b0c0d0e"
    "0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f30"
    "3132333435363738393a3b3c3d3e3f404142434445";

static const char fixture_protocol_module[] =
    "000120010100000020161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e"
    "2f30313233343500000020464748494a4b4c4d4e4f505152535455565758595a5b5c"
    "5d5e5f6061626364650600000020767778797a7b7c7d7e7f80818283848586878889"
    "8a8b8c8d8e8f909192939495010203040506070e000603ee00000020a6a7a8a9aaab"
    "acadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c3c4c50000000000000006"
    "00000000000003ee0000000000000006000000000000138e00000000000000000000"
    "00000000000d000000000000006a000000000000000000000000000007d600000000"
    "000000000000000000000009000000000000138e00000020d6d7d8d9dadbdcdddedf"
    "e0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f2f3f4f500000000000003ee00000000"
    "000007d6000000000000000d000000000000000000000000400708090a0b0c0d0e0f"
    "101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f3031"
    "32333435363738393a3b3c3d3e3f40414243444546";

static const char fixture_fee_budget[] =
    "000120010200000020131415161718191a1b1c1d1e1f202122232425262728292a2b"
    "2c2d2e2f30313200000020434445464748494a4b4c4d4e4f50515253545556575859"
    "5a5b5c5d5e5f6061620300000020737475767778797a7b7c7d7e7f80818283848586"
    "8788898a8b8c8d8e8f909192010203040506070b000303eb00000020a3a4a5a6a7a8"
    "a9aaabacadaeafb0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c20000000000000003"
    "00000000000003eb0000000000000003000000000000138b00000000000000000000"
    "00000000000a0000000000000067000000000000000000000000000007d300000000"
    "000000000000000000000006000000000000138b00000020d3d4d5d6d7d8d9dadbdc"
    "dddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeeff0f1f200000000000003eb00000000"
    "000007d3000000000000000a000000000000000000000000400405060708090a0b0c"
    "0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e"
    "2f303132333435363738393a3b3c3d3e3f404142430000002055565758595a5b5c5d"
    "5e5f606162636465666768696a6b6c6d6e6f70717273740000000000000000000000"
    "00000000040000000000000000000000000000006400000000000000000000000000"
    "000014000000000000000a0000000000000000000000000000002800000000000000"
    "00000000000000000800000000000003ff";

static const char fixture_authentication_only[] =
    "00012001030000002012131415161718191a1b1c1d1e1f202122232425262728292a"
    "2b2c2d2e2f30310000002012131415161718191a1b1c1d1e1f202122232425262728"
    "292a2b2c2d2e2f3031020000002072737475767778797a7b7c7d7e7f808182838485"
    "868788898a8b8c8d8e8f909100000000000000000000000000000020000000000000"
    "00000000000000000000000000000000000000000000000000000000000000000000"
    "00000000000000000000000000000000000000000000000000000000000000000000"
    "00000000000000000000000000000000000000000000000000000000000000000000"
    "00000000000000000000000000000000000000000000002000000000000000000000"
    "0000000000000000000000000000000000000000000000000000000003ea00000000"
    "000007d2000000000000000900000000000000000000000040030405060708090a0b"
    "0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d"
    "2e2f303132333435363738393a3b3c3d3e3f40414201";

static void fill_pattern(uint8_t *bytes, size_t length, uint8_t base,
                         uint8_t kind)
{
    size_t index;
    for (index = 0U; index < length; ++index)
        bytes[index] = (uint8_t)(base + kind + (uint8_t)index);
}

static void build_fixture(uint8_t kind, lxp_authority_grant *grant)
{
    (void)memset(grant, 0, sizeof(*grant));
    fill_pattern(grant->grantor, 32U, 0x10U, kind);
    fill_pattern(grant->grantee, 32U, 0x40U, kind);
    fill_pattern(grant->key, 32U, 0x70U, kind);
    grant->kind = (lxp_authority_kind)kind;
    grant->scope.module_mask = UINT64_C(0x0102030405060708) + kind;
    grant->scope.activity_ordinal_min = kind;
    grant->scope.activity_ordinal_max = (uint16_t)(1000U + kind);
    fill_pattern(grant->scope.asset_id, 32U, 0xA0U, kind);
    grant->scope.maximum_per_activity.hi = kind;
    grant->scope.maximum_per_activity.lo = 1000U + kind;
    grant->scope.maximum_total.hi = kind;
    grant->scope.maximum_total.lo = 5000U + kind;
    grant->scope.spent_total.lo = 7U + kind;
    grant->scope.period_length = 100U + kind;
    grant->scope.maximum_per_period.lo = 2000U + kind;
    grant->scope.spent_this_period.lo = 3U + kind;
    grant->scope.period_start = 5000U + kind;
    fill_pattern(grant->scope.purpose_hash, 32U, 0xD0U, kind);
    grant->not_before = 1000U + kind;
    grant->not_after = 2000U + kind;
    grant->grantor_revocation_sequence = 7U + kind;
    grant->revoked = kind == 5U;
    grant->revoked_at_sequence = kind == 5U ? 4242U : 0U;
    fill_pattern(grant->grantor_signature, 64U, 0x01U, kind);
}

static void build_fee_fixture(lxp_authority_grant *grant)
{
    size_t index;
    build_fixture((uint8_t)LXP_AUTHORITY_DELEGATED_CAPABILITY, grant);
    grant->fee_budget.present = true;
    for (index = 0U; index < 32U; ++index)
        grant->fee_budget.asset_id[index] = (uint8_t)(0x55U + index);
    grant->fee_budget.maximum_per_activity.lo = 4U;
    grant->fee_budget.maximum_total.lo = 100U;
    grant->fee_budget.spent_total.lo = 20U;
    grant->fee_budget.period_length = 10U;
    grant->fee_budget.maximum_per_period.lo = 40U;
    grant->fee_budget.spent_this_period.lo = 8U;
    grant->fee_budget.period_start = grant->not_before + 20U;
}

static void build_authentication_fixture(lxp_authority_grant *grant)
{
    build_fixture((uint8_t)LXP_AUTHORITY_SESSION_KEY, grant);
    (void)memcpy(grant->grantee, grant->grantor, 32U);
    (void)memset(&grant->scope, 0, sizeof(grant->scope));
    grant->revoked = false;
    grant->revoked_at_sequence = 0U;
    grant->authentication_only = true;
}

static int hex_value(char digit, uint8_t *value)
{
    if (digit >= '0' && digit <= '9') {
        *value = (uint8_t)(digit - '0');
        return 0;
    }
    if (digit >= 'a' && digit <= 'f') {
        *value = (uint8_t)(10 + (digit - 'a'));
        return 0;
    }
    return 1;
}

static int hex_decode(const char *text, uint8_t *out, size_t capacity,
                      size_t *length)
{
    size_t digits = strlen(text);
    size_t index;
    if ((digits % 2U) != 0U || digits / 2U > capacity) return 1;
    for (index = 0U; index < digits; index += 2U) {
        uint8_t high;
        uint8_t low;
        if (hex_value(text[index], &high) != 0 ||
            hex_value(text[index + 1U], &low) != 0) return 1;
        out[index / 2U] = (uint8_t)((high << 4U) | low);
    }
    *length = digits / 2U;
    return 0;
}

static int encodes_as(const char *hex, const lxp_authority_grant *grant)
{
    uint8_t storage[4096];
    uint8_t expected[1024];
    uint8_t reencoded[1024];
    lxp_authority_grant decoded;
    lxp_arena arena;
    lxp_byte_span encoded;
    size_t length;
    size_t produced;
    if (hex_decode(hex, expected, sizeof(expected), &length) != 0) return 1;
    if (LXP_ASSERT_RESULT(LXP_OK,
                          lxp_arena_init(&arena, storage, sizeof(storage))) != 0)
        return 1;
    if (LXP_ASSERT_RESULT(LXP_OK, lxp_grant_encode(grant, &arena, &encoded)) != 0)
        return 1;
    if (LXP_ASSERT_U64((uint64_t)length, (uint64_t)encoded.length) != 0) return 1;
    if (LXP_ASSERT_BYTES(expected, encoded.bytes, length) != 0) return 1;
    if (LXP_ASSERT_RESULT(LXP_OK,
                          lxp_grant_decode(expected, length, &decoded)) != 0)
        return 1;
    if (LXP_ASSERT_RESULT(LXP_OK, lxp_arena_reset(&arena, 0U)) != 0) return 1;
    if (LXP_ASSERT_RESULT(LXP_OK,
                          lxp_grant_encode(&decoded, &arena, &encoded)) != 0)
        return 1;
    produced = encoded.length;
    if (produced > sizeof(reencoded)) return 1;
    (void)memcpy(reencoded, encoded.bytes, produced);
    if (LXP_ASSERT_U64((uint64_t)length, (uint64_t)produced) != 0) return 1;
    return LXP_ASSERT_BYTES(expected, reencoded, length);
}

static int existing_grants_byte_identical(void)
{
    lxp_authority_grant grant;
    int failed = 0;
    build_fixture((uint8_t)LXP_AUTHORITY_OWNER, &grant);
    failed |= encodes_as(fixture_owner, &grant);
    build_fixture((uint8_t)LXP_AUTHORITY_SESSION_KEY, &grant);
    failed |= encodes_as(fixture_session_key, &grant);
    build_fixture((uint8_t)LXP_AUTHORITY_DELEGATED_CAPABILITY, &grant);
    failed |= encodes_as(fixture_delegated_capability, &grant);
    build_fixture((uint8_t)LXP_AUTHORITY_BUDGET_ALLOWANCE, &grant);
    failed |= encodes_as(fixture_budget_allowance, &grant);
    build_fixture((uint8_t)LXP_AUTHORITY_ESCROW, &grant);
    failed |= encodes_as(fixture_escrow, &grant);
    build_fixture((uint8_t)LXP_AUTHORITY_PROTOCOL_MODULE, &grant);
    failed |= encodes_as(fixture_protocol_module, &grant);
    build_fee_fixture(&grant);
    failed |= encodes_as(fixture_fee_budget, &grant);
    build_authentication_fixture(&grant);
    failed |= encodes_as(fixture_authentication_only, &grant);
    return failed;
}

typedef struct authority_env {
    lxp_kernel *kernel;
    lxp_identity identity;
} authority_env;

static void declare_module(lxp_kernel *kernel, uint16_t module_id,
                           const uint32_t *activity_types, size_t count)
{
    lxp_module_registration *registration =
        &kernel->modules[kernel->module_count];
    (void)memset(registration, 0, sizeof(*registration));
    registration->module_id = module_id;
    registration->abi_version = 1U;
    registration->activity_type_count = count;
    (void)memcpy(registration->activity_types, activity_types,
                 count * sizeof(activity_types[0]));
    registration->enabled_epoch = kernel->epoch;
    registration->disabled_epoch = UINT64_MAX;
    registration->enabled = true;
    ++kernel->module_count;
}

static int env_init(authority_env *env)
{
    static const uint32_t asset_types[] = { UINT32_C(0x00010001),
                                            UINT32_C(0x00010002) };
    static const uint32_t governance_types[] = { UINT32_C(0x00070001),
                                                 UINT32_C(0x00070005),
                                                 UINT32_C(0x00070006) };
    size_t index;
    env->kernel = calloc(1U, sizeof(*env->kernel));
    if (env->kernel == NULL) return 1;
    env->kernel->epoch = 4U;
    declare_module(env->kernel, LXP_MODULE_ASSET, asset_types,
                   sizeof(asset_types) / sizeof(asset_types[0]));
    declare_module(env->kernel, LXP_MODULE_GOVERNANCE, governance_types,
                   sizeof(governance_types) / sizeof(governance_types[0]));
    (void)memset(&env->identity, 0, sizeof(env->identity));
    for (index = 0U; index < 32U; ++index) {
        env->identity.did_id[index] = (uint8_t)(0x40U + index);
        env->identity.primary_key[index] = (uint8_t)(0x80U + index);
    }
    env->identity.revocation_sequence = TEST_REVOCATION_SEQUENCE;
    return 0;
}

static void env_free(authority_env *env)
{
    free(env->kernel);
    env->kernel = NULL;
}

static lxp_result env_install(authority_env *env, lxp_authority_grant *grant)
{
    uint8_t storage[4096];
    lxp_arena arena;
    lxp_byte_span encoded;
    lxp_module_kv_entry *entry;
    lxp_result status = lxp_grant_id_compute(grant, grant->grant_id);
    if (status != LXP_OK) return status;
    status = lxp_arena_init(&arena, storage, sizeof(storage));
    if (status != LXP_OK) return status;
    status = lxp_grant_encode(grant, &arena, &encoded);
    if (status != LXP_OK) return status;
    if (encoded.length > (size_t)LXP_MODULE_MAX_VALUE_BYTES ||
        env->kernel->module_kv_count >= (size_t)LXP_KERNEL_MAX_MODULE_KV)
        return LXP_ERR_LENGTH_LIMIT;
    entry = &env->kernel->module_kv[env->kernel->module_kv_count];
    (void)memset(entry, 0, sizeof(*entry));
    entry->module_id = LXP_MODULE_GOVERNANCE;
    entry->key_length = 33U;
    entry->key[0] = 5U;
    (void)memcpy(entry->key + 1U, grant->grant_id, 32U);
    entry->value_length = (uint32_t)encoded.length;
    (void)memcpy(entry->value, encoded.bytes, encoded.length);
    ++env->kernel->module_kv_count;
    return LXP_OK;
}

static void env_retire(authority_env *env)
{
    if (env->kernel->module_kv_count != 0U) --env->kernel->module_kv_count;
}

static void build_resolvable(const authority_env *env, lxp_authority_kind kind,
                             const uint8_t key[32], uint64_t not_after,
                             lxp_authority_grant *grant)
{
    (void)memset(grant, 0, sizeof(*grant));
    (void)memcpy(grant->grantor, env->identity.did_id, 32U);
    (void)memcpy(grant->grantee, env->identity.did_id, 32U);
    (void)memcpy(grant->key, key, 32U);
    grant->kind = kind;
    grant->scope.module_mask = UINT64_C(1) << LXP_MODULE_GOVERNANCE;
    grant->scope.activity_ordinal_min = 1U;
    grant->scope.activity_ordinal_max = 6U;
    grant->not_before = TEST_NOT_BEFORE;
    grant->not_after = not_after;
    grant->grantor_revocation_sequence = TEST_REVOCATION_SEQUENCE;
}

static void build_signer_set(lxp_authority_grant *grant, uint8_t signers,
                             uint8_t threshold, uint8_t approvals)
{
    uint8_t index;
    grant->scope.signer_threshold = threshold;
    grant->scope.signer_count = signers;
    for (index = 0U; index < signers; ++index)
        fill_pattern(grant->scope.signers[index], 32U,
                     (uint8_t)(0x20U + (uint8_t)(0x10U * index)), 0U);
    grant->scope.approval_count = approvals;
    for (index = 0U; index < approvals; ++index)
        (void)memcpy(grant->scope.approvals[index], grant->scope.signers[index],
                     32U);
}

static void build_activity(lxp_activity *activity, const authority_env *env,
                           const uint8_t key[32], uint64_t not_after)
{
    (void)memset(activity, 0, sizeof(*activity));
    activity->protocol_version = 1U;
    activity->activity_type = TEST_ACTIVITY;
    activity->actor_did.bytes = env->identity.did_id;
    activity->actor_did.length = 32U;
    activity->authority.bytes = key;
    activity->authority.length = 32U;
    activity->timestamp_bound.not_before = TEST_NOT_BEFORE;
    activity->timestamp_bound.not_after = not_after;
}

static lxp_result env_resolve(const authority_env *env, const uint8_t key[32],
                              uint64_t batch_timestamp, uint64_t bound_not_after,
                              uint64_t global_sequence,
                              lxp_authority_grant *grant,
                              lxp_authority_resolved *resolved)
{
    lxp_activity activity;
    build_activity(&activity, env, key, bound_not_after);
    return lxp_authority_resolve_activity(env->kernel, &env->identity, &activity,
                                          false, true, batch_timestamp,
                                          TEST_WINDOW, global_sequence, grant,
                                          resolved);
}

static int multisig_resolution(void)
{
    authority_env env;
    lxp_authority_grant grant;
    lxp_authority_grant loaded;
    lxp_authority_grant decoded;
    lxp_authority_resolved resolved;
    lxp_arena arena;
    lxp_byte_span encoded;
    uint8_t storage[4096];
    uint8_t key[32];
    uint8_t held[1024];
    size_t length;
    int failed = 0;
    if (env_init(&env) != 0) return 1;
    fill_pattern(key, 32U, 0xE0U, 0U);
    build_resolvable(&env, LXP_AUTHORITY_KIND_MULTISIG, key, TEST_NOT_AFTER,
                     &grant);
    build_signer_set(&grant, TEST_SIGNERS, TEST_THRESHOLD, 1U);
    failed |= LXP_ASSERT_RESULT(LXP_OK, env_install(&env, &grant));
    failed |= LXP_ASSERT_RESULT(LXP_ERR_AUTH_THRESHOLD_UNMET,
                                env_resolve(&env, key, TEST_BATCH_TIMESTAMP,
                                            TEST_NOT_AFTER, TEST_SEQUENCE,
                                            &loaded, &resolved));
    env_retire(&env);
    build_signer_set(&grant, TEST_SIGNERS, TEST_THRESHOLD, TEST_THRESHOLD);
    failed |= LXP_ASSERT_RESULT(LXP_OK, env_install(&env, &grant));
    failed |= LXP_ASSERT_RESULT(LXP_OK,
                                env_resolve(&env, key, TEST_BATCH_TIMESTAMP,
                                            TEST_NOT_AFTER, TEST_SEQUENCE,
                                            &loaded, &resolved));
    failed |= LXP_ASSERT_U64((uint64_t)LXP_AUTHORITY_KIND_MULTISIG,
                             (uint64_t)resolved.kind);
    failed |= LXP_ASSERT_BYTES(grant.grant_id, resolved.grant_id, 32U);
    failed |= LXP_ASSERT_U64((uint64_t)TEST_THRESHOLD,
                             (uint64_t)loaded.scope.approval_count);
    failed |= LXP_ASSERT_RESULT(LXP_OK,
                                lxp_arena_init(&arena, storage, sizeof(storage)));
    failed |= LXP_ASSERT_RESULT(LXP_OK, lxp_grant_encode(&grant, &arena, &encoded));
    length = encoded.length;
    if (length > sizeof(held)) {
        env_free(&env);
        return 1;
    }
    (void)memcpy(held, encoded.bytes, length);
    failed |= LXP_ASSERT_U64((uint64_t)LXP_AUTHORITY_GRANT_VERSION_EXTENDED,
                             (uint64_t)held[4]);
    failed |= LXP_ASSERT_RESULT(LXP_OK, lxp_grant_decode(held, length, &decoded));
    failed |= LXP_ASSERT_U64((uint64_t)TEST_SIGNERS,
                             (uint64_t)decoded.scope.signer_count);
    failed |= LXP_ASSERT_U64((uint64_t)TEST_THRESHOLD,
                             (uint64_t)decoded.scope.signer_threshold);
    failed |= LXP_ASSERT_BYTES(grant.scope.signers, decoded.scope.signers,
                               sizeof(grant.scope.signers));
    failed |= LXP_ASSERT_BYTES(grant.scope.approvals, decoded.scope.approvals,
                               sizeof(grant.scope.approvals));
    (void)memcpy(decoded.scope.approvals[1], decoded.scope.approvals[0], 32U);
    failed |= LXP_ASSERT_RESULT(LXP_ERR_AUTH_DUPLICATE_SIGNER,
                                lxp_authority_resolve(&decoded,
                                                      env.identity.did_id,
                                                      TEST_ACTIVITY,
                                                      declared_mask, 1U, 6U,
                                                      true, &resolved));
    failed |= LXP_ASSERT_RESULT(LXP_OK, lxp_arena_reset(&arena, 0U));
    failed |= LXP_ASSERT_RESULT(LXP_ERR_AUTH_DUPLICATE_SIGNER,
                                lxp_grant_encode(&decoded, &arena, &encoded));
    decoded = grant;
    (void)memcpy(decoded.scope.signers[1], decoded.scope.signers[0], 32U);
    failed |= LXP_ASSERT_RESULT(LXP_ERR_AUTH_DUPLICATE_SIGNER,
                                lxp_authority_resolve(&decoded,
                                                      env.identity.did_id,
                                                      TEST_ACTIVITY,
                                                      declared_mask, 1U, 6U,
                                                      true, &resolved));
    decoded = grant;
    decoded.scope.approval_count = 1U;
    (void)memset(decoded.scope.approvals[1], 0, 32U);
    failed |= LXP_ASSERT_RESULT(LXP_ERR_AUTH_THRESHOLD_UNMET,
                                lxp_authority_resolve(&decoded,
                                                      env.identity.did_id,
                                                      TEST_ACTIVITY,
                                                      declared_mask, 1U, 6U,
                                                      true, &resolved));
    env_free(&env);
    return failed;
}

static int timelock_resolution(void)
{
    authority_env env;
    lxp_authority_grant grant;
    lxp_authority_grant loaded;
    lxp_authority_grant decoded;
    lxp_authority_resolved resolved;
    lxp_arena arena;
    lxp_byte_span encoded;
    uint8_t storage[4096];
    uint8_t key[32];
    uint8_t held[1024];
    size_t length;
    int failed = 0;
    if (env_init(&env) != 0) return 1;
    fill_pattern(key, 32U, 0xC0U, 0U);
    build_resolvable(&env, LXP_AUTHORITY_KIND_TIMELOCK, key, TEST_NOT_AFTER,
                     &grant);
    grant.scope.earliest_sequence = TEST_EARLIEST_SEQUENCE;
    failed |= LXP_ASSERT_RESULT(LXP_OK, env_install(&env, &grant));
    failed |= LXP_ASSERT_RESULT(LXP_ERR_AUTH_NOT_MATURE,
                                env_resolve(&env, key, TEST_BATCH_TIMESTAMP,
                                            TEST_NOT_AFTER, TEST_SEQUENCE,
                                            &loaded, &resolved));
    failed |= LXP_ASSERT_RESULT(LXP_OK,
                                env_resolve(&env, key, TEST_BATCH_TIMESTAMP,
                                            TEST_NOT_AFTER,
                                            TEST_EARLIEST_SEQUENCE, &loaded,
                                            &resolved));
    failed |= LXP_ASSERT_U64((uint64_t)LXP_AUTHORITY_KIND_TIMELOCK,
                             (uint64_t)resolved.kind);
    failed |= LXP_ASSERT_BYTES(grant.grant_id, resolved.grant_id, 32U);
    failed |= LXP_ASSERT_RESULT(LXP_OK,
                                lxp_arena_init(&arena, storage, sizeof(storage)));
    failed |= LXP_ASSERT_RESULT(LXP_OK, lxp_grant_encode(&grant, &arena, &encoded));
    length = encoded.length;
    if (length > sizeof(held)) {
        env_free(&env);
        return 1;
    }
    (void)memcpy(held, encoded.bytes, length);
    failed |= LXP_ASSERT_U64((uint64_t)LXP_AUTHORITY_GRANT_VERSION_EXTENDED,
                             (uint64_t)held[4]);
    failed |= LXP_ASSERT_RESULT(LXP_OK, lxp_grant_decode(held, length, &decoded));
    failed |= LXP_ASSERT_U64((uint64_t)TEST_EARLIEST_SEQUENCE,
                             decoded.scope.earliest_sequence);
    env_retire(&env);
    fill_pattern(key, 32U, 0x90U, 0U);
    build_resolvable(&env, LXP_AUTHORITY_KIND_TIMELOCK, key, TEST_LATE_NOT_AFTER,
                     &grant);
    grant.scope.earliest_timestamp = TEST_EARLIEST_TIMESTAMP;
    failed |= LXP_ASSERT_RESULT(LXP_OK, env_install(&env, &grant));
    failed |= LXP_ASSERT_RESULT(LXP_ERR_AUTH_NOT_MATURE,
                                env_resolve(&env, key, TEST_BATCH_TIMESTAMP,
                                            TEST_NOT_AFTER, TEST_SEQUENCE,
                                            &loaded, &resolved));
    failed |= LXP_ASSERT_RESULT(LXP_OK,
                                env_resolve(&env, key, TEST_MATURE_TIMESTAMP,
                                            TEST_LATE_BOUND, TEST_SEQUENCE,
                                            &loaded, &resolved));
    failed |= LXP_ASSERT_U64((uint64_t)LXP_AUTHORITY_KIND_TIMELOCK,
                             (uint64_t)resolved.kind);
    failed |= LXP_ASSERT_RESULT(LXP_ERR_AUTH_NOT_MATURE,
                                lxp_authority_is_live(&grant,
                                                      TEST_REVOCATION_SEQUENCE,
                                                      TEST_BATCH_TIMESTAMP,
                                                      TEST_SEQUENCE));
    failed |= LXP_ASSERT_RESULT(LXP_OK,
                                lxp_authority_is_live(&grant,
                                                      TEST_REVOCATION_SEQUENCE,
                                                      TEST_MATURE_TIMESTAMP,
                                                      TEST_SEQUENCE));
    grant.scope.earliest_timestamp = 0U;
    failed |= LXP_ASSERT_RESULT(LXP_OK, lxp_arena_reset(&arena, 0U));
    failed |= LXP_ASSERT_RESULT(LXP_ERR_MALFORMED_GRANT,
                                lxp_grant_encode(&grant, &arena, &encoded));
    failed |= LXP_ASSERT_RESULT(LXP_ERR_MALFORMED_GRANT,
                                lxp_authority_is_live(&grant,
                                                      TEST_REVOCATION_SEQUENCE,
                                                      TEST_MATURE_TIMESTAMP,
                                                      TEST_SEQUENCE));
    env_free(&env);
    return failed;
}

int main(int argc, char **argv)
{
    int list_only = argc == 2 && strcmp(argv[1], "--list") == 0;
    if (lxp_test_register("authority.existing-grants-byte-identical",
                          existing_grants_byte_identical) != 0) return 1;
    if (lxp_test_register("authority.multisig-resolution",
                          multisig_resolution) != 0) return 1;
    if (lxp_test_register("authority.timelock-resolution",
                          timelock_resolution) != 0) return 1;
    return lxp_test_run_all(list_only);
}
