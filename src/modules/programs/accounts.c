#include "layerx/programs.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_state.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

enum {
    ACCOUNT_MAGIC_BYTES = 5,
    ACCOUNT_RECORD_V1_FIXED_BYTES = 99,
    ACCOUNT_RECORD_FIXED_BYTES = 139,
    ACCOUNT_EVENT_BYTES = 143,
    PROGRAM_RECORD_BYTES = 71,
    PROGRAM_OWNER_RECORD_BYTES = 33,
    PROGRAM_POLICY_IMMUTABLE = 0,
    PROGRAM_POLICY_AUTHORITY = 1
};

static const uint8_t account_magic[ACCOUNT_MAGIC_BYTES] = {
    'L', 'X', 'P', 'A', '1'
};
static const uint8_t account_domain[] =
    "LayerX/programs/program-account/v1";
static const uint8_t account_primary_prefix[] = "program-account\0p";
static const uint8_t account_reverse_prefix[] = "program-account\0r";
static const uint8_t program_prefix[] = "program\0";
static const uint8_t program_owner_prefix[] = "program-owner\0";

typedef struct programs_account_activity {
    uint8_t program_id[32];
    uint8_t asset_id[32];
    const uint8_t *seed;
    uint32_t seed_length;
} programs_account_activity;

typedef struct account_iter_state {
    const uint8_t *program_id;
    lx_programs_account_visit_fn visit;
    void *user;
} account_iter_state;

typedef struct value_account_iter_state {
    lxp_module_ctx *ctx;
    const uint8_t *receipt_digest;
    lx_programs_value_account_visit_fn visit;
    void *user;
} value_account_iter_state;

static lxp_result account_module_required(lxp_module_ctx *ctx)
{
    const lxp_module_registration *registration;
    lxp_result status;
    if (ctx == NULL || ctx->kernel == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_kernel_module_by_id(ctx->kernel, LXP_MODULE_PROGRAMS,
                                     ctx->epoch, &registration);
    if (status != LXP_OK) return status;
    return registration->abi_version == LX_PROGRAMS_ACCOUNT_ABI_VERSION ||
           registration->abi_version == LX_PROGRAMS_SANDBOX_ABI_VERSION ?
           LXP_OK : LXP_ERR_VERSION_UNSUPPORTED;
}

static uint32_t read_u32(const uint8_t bytes[4])
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | (uint32_t)bytes[3];
}

static uint16_t read_u16(const uint8_t bytes[2])
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | bytes[1]);
}

static uint64_t read_u64(const uint8_t bytes[8])
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index) value = (value << 8U) | bytes[index];
    return value;
}

static void write_u16(uint8_t bytes[2], uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

static void write_u32(uint8_t bytes[4], uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static void write_u64(uint8_t bytes[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        bytes[i] = (uint8_t)(value >> (56U - i * 8U));
}

static lxp_result seed_digest(const uint8_t *seed, size_t seed_length,
                              uint8_t digest[32])
{
    if ((seed == NULL && seed_length != 0U) || digest == NULL)
        return LXP_ERR_NON_CANONICAL;
    return lxp_hash_sha256(seed, seed_length, digest);
}

static lxp_result registration_event_material(
    const lx_programs_account_binding *binding,
    uint8_t body[ACCOUNT_EVENT_BYTES])
{
    uint8_t digest[32];
    size_t offset = 0U;
    lxp_result status;
    if (binding == NULL || body == NULL || binding->registered_sequence == 0U)
        return LXP_ERR_NON_CANONICAL;
    status = seed_digest(binding->seed, binding->seed_length, digest);
    if (status != LXP_OK) return status;
    (void)memcpy(body + offset, account_magic, sizeof(account_magic));
    offset += sizeof(account_magic);
    (void)memcpy(body + offset, binding->program_id, 32U); offset += 32U;
    (void)memcpy(body + offset, binding->account_id, 32U); offset += 32U;
    (void)memcpy(body + offset, binding->asset_id, 32U); offset += 32U;
    write_u16(body + offset, binding->seed_length); offset += 2U;
    (void)memcpy(body + offset, digest, 32U); offset += 32U;
    write_u64(body + offset, binding->registered_sequence); offset += 8U;
    return offset == ACCOUNT_EVENT_BYTES ? LXP_OK : LXP_FATAL_INVARIANT;
}

static lxp_result primary_key(const uint8_t program_id[32],
                              const uint8_t *seed, size_t seed_length,
                              uint8_t key[sizeof(account_primary_prefix) - 1U +
                                          64U])
{
    uint8_t digest[32];
    lxp_result status = seed_digest(seed, seed_length, digest);
    if (status != LXP_OK) return status;
    (void)memcpy(key, account_primary_prefix,
                 sizeof(account_primary_prefix) - 1U);
    (void)memcpy(key + sizeof(account_primary_prefix) - 1U,
                 program_id, 32U);
    (void)memcpy(key + sizeof(account_primary_prefix) - 1U + 32U,
                 digest, 32U);
    return LXP_OK;
}

static void reverse_key(const uint8_t account_id[32],
                        uint8_t key[sizeof(account_reverse_prefix) - 1U + 32U])
{
    (void)memcpy(key, account_reverse_prefix,
                 sizeof(account_reverse_prefix) - 1U);
    (void)memcpy(key + sizeof(account_reverse_prefix) - 1U,
                 account_id, 32U);
}

static lxp_result binding_encode(const lx_programs_account_binding *binding,
                                 uint8_t *record, size_t *record_length)
{
    size_t offset = 0U;
    if (binding == NULL || record == NULL || record_length == NULL ||
        binding->seed_length > LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES ||
        lxp_ct_is_zero(binding->program_id, 32U) ||
        lxp_ct_is_zero(binding->account_id, 32U) ||
        lxp_ct_is_zero(binding->asset_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    record[offset++] = 2U;
    (void)memcpy(record + offset, binding->program_id, 32U); offset += 32U;
    (void)memcpy(record + offset, binding->account_id, 32U); offset += 32U;
    (void)memcpy(record + offset, binding->asset_id, 32U); offset += 32U;
    write_u16(record + offset, binding->seed_length); offset += 2U;
    write_u64(record + offset, binding->registered_sequence); offset += 8U;
    (void)memcpy(record + offset, binding->registration_event_digest, 32U);
    offset += 32U;
    (void)memcpy(record + offset, binding->seed, binding->seed_length);
    offset += binding->seed_length;
    *record_length = offset;
    return LXP_OK;
}

static lxp_result binding_decode(const uint8_t *record, size_t record_length,
                                 lx_programs_account_binding *binding)
{
    uint16_t seed_length;
    size_t seed_offset;
    uint8_t derived[32];
    lxp_result status;
    if (record == NULL || binding == NULL ||
        record_length < ACCOUNT_RECORD_V1_FIXED_BYTES ||
        (record[0] != 1U && record[0] != 2U))
        return LXP_ERR_NON_CANONICAL;
    seed_length = read_u16(record + 97U);
    seed_offset = record[0] == 1U ? ACCOUNT_RECORD_V1_FIXED_BYTES :
                                   ACCOUNT_RECORD_FIXED_BYTES;
    if (seed_length > LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES ||
        record_length != seed_offset + seed_length)
        return LXP_ERR_LENGTH_LIMIT;
    (void)memset(binding, 0, sizeof(*binding));
    binding->record_version = record[0];
    (void)memcpy(binding->program_id, record + 1U, 32U);
    (void)memcpy(binding->account_id, record + 33U, 32U);
    (void)memcpy(binding->asset_id, record + 65U, 32U);
    binding->seed_length = seed_length;
    if (record[0] == 2U) {
        uint8_t event[ACCOUNT_EVENT_BYTES];
        uint8_t digest[32];
        binding->registered_sequence = read_u64(record + 99U);
        (void)memcpy(binding->registration_event_digest, record + 107U, 32U);
        (void)memcpy(binding->seed, record + seed_offset, seed_length);
        status = registration_event_material(binding, event);
        if (status == LXP_OK)
            status = lxp_hash_sha256(event, sizeof(event), digest);
        if (status != LXP_OK ||
            lxp_ct_memcmp(digest, binding->registration_event_digest, 32U) != 0)
            return status != LXP_OK ? status : LXP_FATAL_INVARIANT;
    } else {
        (void)memcpy(binding->seed, record + seed_offset, seed_length);
    }
    if (lxp_ct_is_zero(binding->program_id, 32U) ||
        lxp_ct_is_zero(binding->account_id, 32U) ||
        lxp_ct_is_zero(binding->asset_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_programs_account_derive(
        binding->program_id, binding->seed, binding->seed_length, derived);
    if (status != LXP_OK) return status;
    return lxp_ct_memcmp(derived, binding->account_id, 32U) == 0 ?
           LXP_OK : LXP_ERR_ACCOUNT_ID_MISMATCH;
}

static bool binding_equal(const lx_programs_account_binding *left,
                          const lx_programs_account_binding *right)
{
    return left->record_version == right->record_version &&
           left->seed_length == right->seed_length &&
           lxp_ct_memcmp(left->program_id, right->program_id, 32U) == 0 &&
           lxp_ct_memcmp(left->account_id, right->account_id, 32U) == 0 &&
           lxp_ct_memcmp(left->asset_id, right->asset_id, 32U) == 0 &&
           left->registered_sequence == right->registered_sequence &&
           lxp_ct_memcmp(left->registration_event_digest,
                         right->registration_event_digest, 32U) == 0 &&
           memcmp(left->seed, right->seed, left->seed_length) == 0;
}

static bool binding_origin_equal(const lx_programs_account_binding *left,
                                 const lx_programs_account_binding *right)
{
    return left->seed_length == right->seed_length &&
           lxp_ct_memcmp(left->program_id, right->program_id, 32U) == 0 &&
           lxp_ct_memcmp(left->account_id, right->account_id, 32U) == 0 &&
           memcmp(left->seed, right->seed, left->seed_length) == 0;
}

static lxp_result load_program(lxp_module_ctx *ctx,
                               const uint8_t program_id[32],
                               const uint8_t **record)
{
    uint8_t key[sizeof(program_prefix) - 1U + 32U];
    size_t record_length;
    lxp_result status;
    if (ctx == NULL || program_id == NULL || record == NULL ||
        lxp_ct_is_zero(program_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(key, program_prefix, sizeof(program_prefix) - 1U);
    (void)memcpy(key + sizeof(program_prefix) - 1U, program_id, 32U);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), record, &record_length);
    if (status != LXP_OK) return status;
    if (record_length != PROGRAM_RECORD_BYTES ||
        ((*record)[0] != PROGRAM_POLICY_IMMUTABLE &&
         (*record)[0] != PROGRAM_POLICY_AUTHORITY) ||
        ((*record)[0] == PROGRAM_POLICY_IMMUTABLE &&
         !lxp_ct_is_zero(*record + 1U, 32U)) ||
        ((*record)[0] == PROGRAM_POLICY_AUTHORITY &&
         lxp_ct_is_zero(*record + 1U, 32U)))
        return LXP_FATAL_INVARIANT;
    return LXP_OK;
}

static lxp_result deployed_program(lxp_module_ctx *ctx,
                                   const uint8_t program_id[32])
{
    uint16_t abi_version;
    lxp_result status = lxp_programs_program_abi(ctx, program_id,
                                                 &abi_version);
    if (status != LXP_OK) return status;
    if (abi_version != LX_PROGRAMS_ACCOUNT_ABI_VERSION)
        return LXP_ERR_VERSION_UNSUPPORTED;
    return lxp_programs_program_active(ctx, program_id);
}

static void owner_key(const uint8_t program_id[32],
                      uint8_t key[sizeof(program_owner_prefix) - 1U + 32U])
{
    (void)memcpy(key, program_owner_prefix,
                 sizeof(program_owner_prefix) - 1U);
    (void)memcpy(key + sizeof(program_owner_prefix) - 1U, program_id, 32U);
}

lxp_result lxp_programs_account_owner_bind(
    lxp_module_ctx *ctx, const uint8_t program_id[32], const uint8_t owner[32])
{
    uint8_t key[sizeof(program_owner_prefix) - 1U + 32U];
    uint8_t value[PROGRAM_OWNER_RECORD_BYTES];
    const uint8_t *existing;
    size_t existing_length;
    lxp_result status;
    if (ctx == NULL || program_id == NULL || owner == NULL ||
        lxp_ct_is_zero(program_id, 32U) || lxp_ct_is_zero(owner, 32U))
        return LXP_ERR_NON_CANONICAL;
    owner_key(program_id, key);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &existing,
                            &existing_length);
    if (status == LXP_OK)
        return existing_length == sizeof(value) && existing[0] == 1U &&
               lxp_ct_memcmp(existing + 1U, owner, 32U) == 0 ?
               LXP_OK : LXP_ERR_AUTH_SCOPE;
    if (status != LXP_ERR_UNKNOWN_FIELD) return status;
    value[0] = 1U;
    (void)memcpy(value + 1U, owner, 32U);
    return lxp_ctx_kv_put(ctx, key, sizeof(key), value, sizeof(value));
}

static lxp_result registration_authorized(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    const uint8_t principal[32])
{
    uint8_t key[sizeof(program_owner_prefix) - 1U + 32U];
    const uint8_t *program;
    const uint8_t *owner;
    size_t owner_length;
    lxp_result status;
    if (principal == NULL || lxp_ct_is_zero(principal, 32U))
        return LXP_ERR_AUTH_SCOPE;
    status = load_program(ctx, program_id, &program);
    if (status != LXP_OK) return status;
    owner_key(program_id, key);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &owner, &owner_length);
    if (status == LXP_OK) {
        if (owner_length != PROGRAM_OWNER_RECORD_BYTES || owner[0] != 1U ||
            lxp_ct_is_zero(owner + 1U, 32U))
            return LXP_FATAL_INVARIANT;
        return lxp_ct_memcmp(owner + 1U, principal, 32U) == 0 ?
               LXP_OK : LXP_ERR_AUTH_SCOPE;
    }
    if (status != LXP_ERR_UNKNOWN_FIELD) return status;
    if (program[0] != PROGRAM_POLICY_AUTHORITY)
        return LXP_ERR_AUTH_SCOPE;
    return lxp_ct_memcmp(program + 1U, principal, 32U) == 0 ?
           LXP_OK : LXP_ERR_AUTH_SCOPE;
}

static lxp_result registered_asset(lxp_module_ctx *ctx,
                                   const uint8_t asset_id[32])
{
    const lx_programs_transfer_runtime *runtime =
        (const lx_programs_transfer_runtime *)lxp_ctx_module_runtime(ctx);
    size_t i;
    if (runtime == NULL || runtime->accounts == NULL ||
        runtime->assets == NULL || runtime->asset_count == 0U)
        return LXP_ERR_MODULE_DISABLED;
    for (i = 0U; i < runtime->asset_count; ++i)
        if (lxp_ct_memcmp(runtime->assets[i].asset_id, asset_id, 32U) == 0)
            return runtime->assets[i].registered ? LXP_OK :
                                                   LXP_ERR_ASSET_MISMATCH;
    return LXP_ERR_ASSET_MISMATCH;
}

lxp_result lxp_programs_account_derive(
    const uint8_t program_id[32], const uint8_t *seed, size_t seed_length,
    uint8_t account_id[LX_PROGRAMS_ACCOUNT_ID_BYTES])
{
    uint8_t length[4];
    lxp_hash_context hash;
    lxp_result status;
    if (program_id == NULL || account_id == NULL ||
        (seed == NULL && seed_length != 0U) ||
        seed_length > LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES ||
        lxp_ct_is_zero(program_id, 32U))
        return seed_length > LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES ?
               LXP_ERR_LENGTH_LIMIT : LXP_ERR_NON_CANONICAL;
    write_u32(length, (uint32_t)seed_length);
    lxp_hash_init(&hash);
    status = lxp_hash_update(&hash, account_domain, sizeof(account_domain));
    if (status == LXP_OK) status = lxp_hash_update(&hash, program_id, 32U);
    if (status == LXP_OK) status = lxp_hash_update(&hash, length, sizeof(length));
    if (status == LXP_OK) status = lxp_hash_update(&hash, seed, seed_length);
    return status == LXP_OK ? lxp_hash_final(&hash, account_id) : status;
}

static lxp_result load_binding(lxp_module_ctx *ctx, const uint8_t *key,
                               size_t key_length,
                               lx_programs_account_binding *binding)
{
    const uint8_t *record;
    size_t record_length;
    lxp_result status = lxp_ctx_kv_get(ctx, key, key_length, &record,
                                       &record_length);
    if (status != LXP_OK) return status;
    return binding_decode(record, record_length, binding);
}

lxp_result lxp_programs_account_lookup(
    lxp_module_ctx *ctx, const uint8_t program_id[32], const uint8_t *seed,
    size_t seed_length, lx_programs_account_binding *binding,
    lx_account **account)
{
    uint8_t key[sizeof(account_primary_prefix) - 1U + 64U];
    uint8_t reverse[sizeof(account_reverse_prefix) - 1U + 32U];
    lx_programs_account_binding reverse_binding;
    lxp_result status;
    if (ctx == NULL || program_id == NULL || binding == NULL ||
        account == NULL || (seed == NULL && seed_length != 0U) ||
        seed_length > LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES)
        return LXP_ERR_NON_CANONICAL;
    status = primary_key(program_id, seed, seed_length, key);
    if (status == LXP_OK) status = load_binding(ctx, key, sizeof(key), binding);
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(binding->program_id, program_id, 32U) != 0 ||
        binding->seed_length != seed_length ||
        (seed_length != 0U &&
         memcmp(binding->seed, seed, seed_length) != 0))
        return LXP_ERR_CONTEXT_MISMATCH;
    reverse_key(binding->account_id, reverse);
    status = load_binding(ctx, reverse, sizeof(reverse), &reverse_binding);
    if (status != LXP_OK) return status;
    if (!binding_equal(binding, &reverse_binding))
        return LXP_ERR_CONTEXT_MISMATCH;
    return lxp_ctx_account_find(ctx, binding->account_id, account);
}

lxp_result lxp_programs_account_lookup_id(
    lxp_module_ctx *ctx, const uint8_t account_id[32],
    lx_programs_account_binding *binding, lx_account **account)
{
    uint8_t key[sizeof(account_reverse_prefix) - 1U + 32U];
    uint8_t primary[sizeof(account_primary_prefix) - 1U + 64U];
    lx_programs_account_binding primary_binding;
    lxp_result status;
    if (ctx == NULL || account_id == NULL || binding == NULL || account == NULL)
        return LXP_ERR_NON_CANONICAL;
    reverse_key(account_id, key);
    status = load_binding(ctx, key, sizeof(key), binding);
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(binding->account_id, account_id, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    status = primary_key(binding->program_id, binding->seed,
                         binding->seed_length, primary);
    if (status == LXP_OK)
        status = load_binding(ctx, primary, sizeof(primary), &primary_binding);
    if (status != LXP_OK) return status;
    if (!binding_equal(binding, &primary_binding))
        return LXP_ERR_CONTEXT_MISMATCH;
    return lxp_ctx_account_find(ctx, account_id, account);
}

static lxp_result visit_binding(const uint8_t *key, size_t key_length,
                                const uint8_t *value, size_t value_length,
                                void *user)
{
    account_iter_state *state = (account_iter_state *)user;
    lx_programs_account_binding binding;
    uint8_t expected[sizeof(account_primary_prefix) - 1U + 64U];
    lxp_result status;
    status = binding_decode(value, value_length, &binding);
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(binding.program_id, state->program_id, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    status = primary_key(binding.program_id, binding.seed,
                         binding.seed_length, expected);
    if (status != LXP_OK || key_length != sizeof(expected) ||
        memcmp(key, expected, sizeof(expected)) != 0)
        return status != LXP_OK ? status : LXP_ERR_CONTEXT_MISMATCH;
    return state->visit(&binding, state->user);
}

lxp_result lxp_programs_account_iter(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    lx_programs_account_visit_fn visit, void *user)
{
    uint8_t prefix[sizeof(account_primary_prefix) - 1U + 32U];
    account_iter_state state;
    if (ctx == NULL || program_id == NULL || visit == NULL ||
        lxp_ct_is_zero(program_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(prefix, account_primary_prefix,
                 sizeof(account_primary_prefix) - 1U);
    (void)memcpy(prefix + sizeof(account_primary_prefix) - 1U,
                 program_id, 32U);
    state.program_id = program_id;
    state.visit = visit;
    state.user = user;
    return lxp_ctx_kv_iter(ctx, prefix, sizeof(prefix), visit_binding, &state);
}

lxp_result lxp_programs_account_state_head_read(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    const uint8_t receipt_digest[32], lx_programs_account_state_head *head)
{
    static const uint8_t account_tree_key[] = "account-tree";
    const lx_programs_transfer_runtime *runtime;
    lxp_verified_receipt_facts receipt;
    uint8_t candidate_state_root[32];
    uint16_t abi_version;
    lxp_result status;
    if (ctx == NULL || program_id == NULL || receipt_digest == NULL ||
        head == NULL || lxp_ct_is_zero(program_id, 32U) ||
        lxp_ct_is_zero(receipt_digest, 32U) ||
        ctx->protocol_version != LXP_PROTOCOL_VERSION_OCCUPANCY ||
        ctx->staged_account_count != 0U || ctx->staged_count != 0U ||
        ctx->transfer_applied)
        return LXP_ERR_NON_CANONICAL;
    status = account_module_required(ctx);
    if (status != LXP_OK) return status;
    status = lxp_programs_program_abi(ctx, program_id, &abi_version);
    if (status != LXP_OK) return status;
    if (abi_version != LX_PROGRAMS_ACCOUNT_ABI_VERSION)
        return LXP_ERR_VERSION_UNSUPPORTED;
    runtime = (const lx_programs_transfer_runtime *)
        lxp_ctx_module_runtime(ctx);
    if (runtime == NULL || runtime->accounts == NULL ||
        ctx->kernel == NULL || ctx->kernel->state == NULL ||
        runtime->accounts != ctx->kernel->state->accounts ||
        ctx->kernel->state->next_sequence == 0U)
        return LXP_ERR_MODULE_DISABLED;
    (void)memset(head, 0, sizeof(*head));
    head->observed_sequence = ctx->kernel->state->next_sequence - 1U;
    (void)memcpy(head->receipt_digest, receipt_digest, 32U);
    status = lx_account_registry_root(runtime->accounts, head->account_root);
    if (status == LXP_OK)
        status = lxp_state_subtree_proof(
            ctx->kernel, 0U, account_tree_key, sizeof(account_tree_key) - 1U,
            head->universal_root, &head->account_tree_proof);
    if (status == LXP_OK)
        status = lxp_state_root_proof(
            ctx->kernel, 0U, head->state_root,
            &head->universal_root_proof);
    if (status == LXP_OK)
        status = lxp_state_subtree_root(
            ctx->kernel, LXP_MODULE_PROGRAMS, head->programs_root);
    if (status == LXP_OK)
        status = lxp_state_root_proof(
            ctx->kernel, LXP_MODULE_PROGRAMS, candidate_state_root,
            &head->programs_root_proof);
    if (status == LXP_OK &&
        lxp_ct_memcmp(candidate_state_root, head->state_root, 32U) != 0)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK)
        status = lxp_ctx_verified_receipt_facts(ctx, receipt_digest, &receipt);
    if (status == LXP_OK &&
        (receipt.result_code != LXP_OK ||
         receipt.global_sequence != head->observed_sequence ||
         receipt.timestamp == 0U ||
         lxp_ct_memcmp(receipt.receipt_digest, receipt_digest, 32U) != 0 ||
         lxp_ct_memcmp(receipt.resulting_state_root,
                       head->state_root, 32U) != 0))
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK) head->observed_at = receipt.timestamp;
    return status;
}

static lxp_result value_account_fill(
    lxp_module_ctx *ctx, const lx_programs_account_binding *binding,
    const uint8_t receipt_digest[32],
    lx_programs_value_account_view *view)
{
    const lx_programs_transfer_runtime *runtime;
    lx_programs_account_binding indexed;
    lxp_verified_receipt_facts receipt;
    lx_account *account;
    uint8_t primary[sizeof(account_primary_prefix) - 1U + 64U];
    uint8_t candidate_state_root[32];
    uint16_t abi_version;
    static const uint8_t account_tree_key[] = "account-tree";
    lxp_result status;
    if (ctx == NULL || binding == NULL || receipt_digest == NULL ||
        view == NULL || lxp_ct_is_zero(receipt_digest, 32U) ||
        ctx->protocol_version != LXP_PROTOCOL_VERSION_OCCUPANCY ||
        ctx->staged_account_count != 0U || ctx->staged_count != 0U ||
        ctx->transfer_applied)
        return LXP_ERR_NON_CANONICAL;
    runtime = (const lx_programs_transfer_runtime *)
        lxp_ctx_module_runtime(ctx);
    if (runtime == NULL || runtime->accounts == NULL ||
        ctx->kernel == NULL || ctx->kernel->state == NULL ||
        runtime->accounts != ctx->kernel->state->accounts)
        return LXP_ERR_MODULE_DISABLED;
    status = lxp_programs_program_abi(ctx, binding->program_id, &abi_version);
    if (status != LXP_OK) return status;
    if (abi_version != LX_PROGRAMS_ACCOUNT_ABI_VERSION)
        return LXP_ERR_VERSION_UNSUPPORTED;
    status = lxp_programs_account_lookup_id(
        ctx, binding->account_id, &indexed, &account);
    if (status != LXP_OK) return status;
    if (!binding_equal(binding, &indexed) || binding->record_version != 2U ||
        binding->registered_sequence == 0U ||
        lxp_ct_is_zero(binding->registration_event_digest, 32U) ||
        account->kind != LX_ACCOUNT_MODULE_VALUE || !account->has_asset ||
        lxp_ct_memcmp(account->id, binding->account_id, 32U) != 0 ||
        lxp_ct_memcmp(account->asset_id, binding->asset_id, 32U) != 0 ||
        account->created_at_sequence != binding->registered_sequence)
        return LXP_FATAL_INVARIANT;
    (void)memset(view, 0, sizeof(*view));
    view->binding = *binding;
    view->account = *account;
    view->balance.hi = account->balance.hi;
    view->balance.lo = account->balance.lo;
    view->frozen = account->frozen;
    if (ctx->kernel->state->next_sequence == 0U)
        return LXP_FATAL_INVARIANT;
    view->observed_sequence = ctx->kernel->state->next_sequence - 1U;
    (void)memcpy(view->receipt_digest, receipt_digest, 32U);
    status = lx_account_registry_proof(
        runtime->accounts, binding->account_id, view->account_root,
        &view->account_proof);
    if (status == LXP_OK)
        status = lxp_state_subtree_proof(
            ctx->kernel, 0U, account_tree_key, sizeof(account_tree_key) - 1U,
            view->universal_root, &view->account_tree_proof);
    if (status == LXP_OK)
        status = lxp_state_root_proof(
            ctx->kernel, 0U, view->state_root,
            &view->universal_root_proof);
    if (status == LXP_OK)
        status = primary_key(binding->program_id, binding->seed,
                             binding->seed_length, primary);
    if (status == LXP_OK)
        status = lxp_state_subtree_proof(
            ctx->kernel, LXP_MODULE_PROGRAMS, primary, sizeof(primary),
            view->programs_root, &view->binding_proof);
    if (status == LXP_OK)
        status = lxp_state_root_proof(
            ctx->kernel, LXP_MODULE_PROGRAMS, candidate_state_root,
            &view->programs_root_proof);
    if (status == LXP_OK &&
        lxp_ct_memcmp(candidate_state_root, view->state_root, 32U) != 0)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK)
        status = lxp_ctx_verified_receipt_facts(ctx, receipt_digest, &receipt);
    if (status == LXP_OK &&
        (receipt.result_code != LXP_OK ||
         receipt.global_sequence != view->observed_sequence ||
         receipt.timestamp == 0U ||
         lxp_ct_memcmp(receipt.receipt_digest, receipt_digest, 32U) != 0 ||
         lxp_ct_memcmp(receipt.resulting_state_root,
                       view->state_root, 32U) != 0))
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK) view->observed_at = receipt.timestamp;
    return status;
}

lxp_result lxp_programs_value_account_read(
    lxp_module_ctx *ctx, const uint8_t account_id[32],
    const uint8_t receipt_digest[32],
    lx_programs_value_account_view *view)
{
    lx_programs_account_binding binding;
    lx_account *account;
    lxp_result status;
    if (ctx == NULL || account_id == NULL || receipt_digest == NULL ||
        view == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = account_module_required(ctx);
    if (status == LXP_OK)
        status = lxp_programs_account_lookup_id(
            ctx, account_id, &binding, &account);
    if (status != LXP_OK) return status;
    return value_account_fill(ctx, &binding, receipt_digest, view);
}

static lxp_result visit_value_account(
    const lx_programs_account_binding *binding, void *user)
{
    value_account_iter_state *state = (value_account_iter_state *)user;
    lx_programs_value_account_view view;
    lxp_result status = value_account_fill(
        state->ctx, binding, state->receipt_digest, &view);
    return status == LXP_OK ? state->visit(&view, state->user) : status;
}

lxp_result lxp_programs_value_account_iter(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    const uint8_t receipt_digest[32],
    lx_programs_value_account_visit_fn visit, void *user)
{
    value_account_iter_state state;
    if (ctx == NULL || program_id == NULL || receipt_digest == NULL ||
        visit == NULL)
        return LXP_ERR_NON_CANONICAL;
    state.ctx = ctx;
    state.receipt_digest = receipt_digest;
    state.visit = visit;
    state.user = user;
    return lxp_programs_account_iter(
        ctx, program_id, visit_value_account, &state);
}

static lxp_result emit_registered(lxp_module_ctx *ctx,
                                  const lx_programs_account_binding *binding)
{
    uint8_t body[ACCOUNT_EVENT_BYTES];
    uint8_t digest[32];
    lxp_result status = registration_event_material(binding, body);
    if (status != LXP_OK) return status;
    status = lxp_hash_sha256(body, sizeof(body), digest);
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(digest, binding->registration_event_digest, 32U) != 0)
        return LXP_FATAL_INVARIANT;
    return lxp_ctx_emit_event(ctx, LX_PROGRAMS_EVENT_ACCOUNT_REGISTERED,
                              body, sizeof(body));
}

lxp_result lxp_programs_account_register(
    lxp_module_ctx *ctx, const uint8_t program_id[32], const uint8_t *seed,
    size_t seed_length, const uint8_t asset_id[32], lx_account **account,
    bool *created)
{
    lx_programs_account_binding binding;
    lx_programs_account_binding primary_binding;
    lx_programs_account_binding reverse_binding;
    uint8_t primary[sizeof(account_primary_prefix) - 1U + 64U];
    uint8_t reverse[sizeof(account_reverse_prefix) - 1U + 32U];
    uint8_t record[ACCOUNT_RECORD_FIXED_BYTES +
                   LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES];
    size_t record_length;
    lxp_result primary_status;
    lxp_result reverse_status;
    lxp_result status;
    bool account_created;
    if (ctx == NULL || program_id == NULL || asset_id == NULL ||
        account == NULL || created == NULL ||
        (seed == NULL && seed_length != 0U) ||
        seed_length > LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES ||
        ctx->protocol_version != LXP_PROTOCOL_VERSION_OCCUPANCY)
        return LXP_ERR_NON_CANONICAL;
    status = account_module_required(ctx);
    if (status == LXP_OK) status = deployed_program(ctx, program_id);
    if (status == LXP_OK) status = registered_asset(ctx, asset_id);
    if (status != LXP_OK) return status;
    (void)memset(&binding, 0, sizeof(binding));
    binding.record_version = 2U;
    (void)memcpy(binding.program_id, program_id, 32U);
    (void)memcpy(binding.asset_id, asset_id, 32U);
    binding.seed_length = (uint16_t)seed_length;
    if (seed_length != 0U)
        (void)memcpy(binding.seed, seed, seed_length);
    status = lxp_programs_account_derive(program_id, seed, seed_length,
                                         binding.account_id);
    if (status == LXP_OK)
        status = primary_key(program_id, seed, seed_length, primary);
    if (status != LXP_OK) return status;
    reverse_key(binding.account_id, reverse);
    primary_status = load_binding(ctx, primary, sizeof(primary),
                                  &primary_binding);
    reverse_status = load_binding(ctx, reverse, sizeof(reverse),
                                  &reverse_binding);
    if (primary_status != LXP_OK && primary_status != LXP_ERR_UNKNOWN_FIELD)
        return primary_status;
    if (reverse_status != LXP_OK && reverse_status != LXP_ERR_UNKNOWN_FIELD)
        return reverse_status;
    if (primary_status == LXP_OK || reverse_status == LXP_OK) {
        if (primary_status != LXP_OK || reverse_status != LXP_OK)
            return LXP_ERR_CONTEXT_MISMATCH;
        if (!binding_origin_equal(&binding, &primary_binding) ||
            !binding_origin_equal(&binding, &reverse_binding))
            return LXP_ERR_ACCOUNT_ID_MISMATCH;
        if (lxp_ct_memcmp(binding.asset_id, primary_binding.asset_id, 32U) != 0 ||
            lxp_ct_memcmp(binding.asset_id, reverse_binding.asset_id, 32U) != 0)
            return LXP_ERR_ASSET_MISMATCH;
        status = lxp_ctx_account_stage_module_value(
            ctx, binding.account_id, binding.asset_id, account,
            &account_created);
        if (status != LXP_OK) return status;
        if (account_created) return LXP_ERR_CONTEXT_MISMATCH;
        *created = false;
        return LXP_OK;
    }
    status = lxp_ctx_account_stage_module_value(
        ctx, binding.account_id, binding.asset_id, account, &account_created);
    if (status != LXP_OK) return status;
    if (!account_created) return LXP_ERR_CONTEXT_MISMATCH;
    binding.registered_sequence = lxp_ctx_global_sequence(ctx);
    {
        uint8_t event[ACCOUNT_EVENT_BYTES];
        status = registration_event_material(&binding, event);
        if (status == LXP_OK)
            status = lxp_hash_sha256(
                event, sizeof(event), binding.registration_event_digest);
    }
    if (status != LXP_OK) return status;
    status = binding_encode(&binding, record, &record_length);
    if (status == LXP_OK)
        status = lxp_ctx_kv_put(ctx, primary, sizeof(primary),
                                record, record_length);
    if (status == LXP_OK)
        status = lxp_ctx_kv_put(ctx, reverse, sizeof(reverse),
                                record, record_length);
    if (status == LXP_OK) status = emit_registered(ctx, &binding);
    if (status != LXP_OK) return status;
    *created = true;
    return LXP_OK;
}

lxp_result lxp_programs_account_decode(lxp_module_ctx *ctx,
                                       const uint8_t *payload,
                                       size_t payload_length,
                                       void **decoded)
{
    programs_account_activity *value;
    uint32_t seed_length;
    void *allocation;
    lxp_result status;
    if (ctx == NULL || payload == NULL || decoded == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (ctx->protocol_version != LXP_PROTOCOL_VERSION_OCCUPANCY)
        return LXP_ERR_VERSION_UNSUPPORTED;
    status = account_module_required(ctx);
    if (status != LXP_OK) return status;
    if (payload_length < 37U)
        return LXP_ERR_TRUNCATED;
    if (memcmp(payload + 32U, account_magic, ACCOUNT_MAGIC_BYTES) != 0)
        return LXP_ERR_INVALID_TAG;
    if (payload_length < 73U) return LXP_ERR_TRUNCATED;
    seed_length = read_u32(payload + 69U);
    if (seed_length > LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES)
        return LXP_ERR_LENGTH_LIMIT;
    if (payload_length != 73U + (size_t)seed_length)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value),
                                 _Alignof(programs_account_activity),
                                 &allocation);
    if (status != LXP_OK) return status;
    value = (programs_account_activity *)allocation;
    (void)memcpy(value->program_id, payload, 32U);
    (void)memcpy(value->asset_id, payload + 37U, 32U);
    value->seed_length = seed_length;
    value->seed = payload + 73U;
    *decoded = value;
    return LXP_OK;
}

lxp_result lxp_programs_account_validate(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded)
{
    const programs_account_activity *value =
        (const programs_account_activity *)decoded;
    uint8_t account_id[32];
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL ||
        ctx->protocol_version != LXP_PROTOCOL_VERSION_OCCUPANCY ||
        lxp_ct_is_zero(authority->principal, 32U) ||
        lxp_ct_is_zero(value->program_id, 32U) ||
        lxp_ct_is_zero(value->asset_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    status = registration_authorized(ctx, value->program_id,
                                     authority->principal);
    if (status == LXP_OK) status = registered_asset(ctx, value->asset_id);
    if (status == LXP_OK)
        status = lxp_programs_account_derive(
            value->program_id, value->seed, value->seed_length, account_id);
    if (status == LXP_OK)
        status = lxp_ctx_charge_gas(ctx, 73U + value->seed_length);
    return status;
}

lxp_result lxp_programs_account_execute(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects)
{
    const programs_account_activity *value =
        (const programs_account_activity *)decoded;
    lx_account *account;
    bool created;
    (void)activity;
    (void)effects;
    if (ctx == NULL || authority == NULL || value == NULL)
        return LXP_ERR_NON_CANONICAL;
    return lxp_programs_account_register(
        ctx, value->program_id, value->seed, value->seed_length,
        value->asset_id, &account, &created);
}
