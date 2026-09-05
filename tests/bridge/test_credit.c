#define _POSIX_C_SOURCE 200809L
#include "layerx/lxp_bridge_credit.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lx_asset.h"
#include "layerx/programs.h"
#include "files.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <string.h>

#define CHECK(expression) do { if (!(expression)) { \
    (void)fprintf(stderr, "credit check failed at line %d: %s\n", __LINE__, #expression); \
    return 1; } } while (0)

static int sign_activity(lxp_activity *activity, const uint8_t seed[32], uint8_t signature[64])
{
    uint8_t digest[32];
    size_t length = 64U;
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, seed, 32U);
    EVP_MD_CTX *context = EVP_MD_CTX_new();
    int ok = key != NULL && context != NULL &&
        lxp_activity_signing_preimage(activity, digest) == LXP_OK &&
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(context, signature, &length, digest, 32U) == 1 && length == 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    activity->signature = (lxp_byte_span){signature, 64U};
    return ok && lxp_activity_verify_signature(activity) == LXP_OK ? 0 : 1;
}

static int begin(lxp_module_ctx *ctx, lxp_kernel *kernel, lxp_arena *arena,
                  lxp_effect_buffer *effects, uint64_t sequence)
{
    CHECK(lxp_state_journal_open(kernel->state, sequence, kernel->journal) == LXP_OK);
    CHECK(lxp_module_ctx_init(ctx, kernel, LXP_MODULE_BRIDGE, 1U, 1U,
                              sequence, 100000U, arena, true) == LXP_OK);
    ctx->protocol_version = 3U;
    CHECK(lxp_effect_buffer_init(effects) == LXP_OK);
    CHECK(lxp_module_ctx_bind_effects(ctx, effects) == LXP_OK);
    return 0;
}

static int prove_leaf(const uint8_t *key, size_t key_length,
                      const uint8_t *value, size_t value_length,
                      const lxp_state_proof *proof, const uint8_t root[32])
{
    uint8_t bytes[1024];
    uint8_t digest[32];
    uint8_t pair[64];
    uint32_t index = proof->leaf_index;
    uint32_t count = proof->leaf_count;
    if (key_length + value_length > sizeof(bytes) - 8U || count == 0U || index >= count ||
        proof->depth > LXP_STATE_PROOF_MAX_DEPTH) return 1;
    for (size_t offset = 0U; offset < 4U; ++offset) {
        bytes[offset] = (uint8_t)(key_length >> (24U - offset * 8U));
        bytes[4U + offset] = (uint8_t)(value_length >> (24U - offset * 8U));
    }
    (void)memcpy(bytes + 8U, key, key_length);
    (void)memcpy(bytes + 8U + key_length, value, value_length);
    if (lxp_hash_domain(LXP_DOMAIN_STATE_LEAF, bytes, 8U + key_length + value_length, digest) != LXP_OK)
        return 1;
    for (size_t depth = 0U; depth < proof->depth; ++depth) {
        if (count <= 1U || ((index ^ 1U) >= count && memcmp(digest, proof->siblings[depth], 32U) != 0))
            return 1;
        (void)memcpy(pair + ((index & 1U) ? 32U : 0U), digest, 32U);
        (void)memcpy(pair + ((index & 1U) ? 0U : 32U), proof->siblings[depth], 32U);
        if (lxp_hash_domain(LXP_DOMAIN_STATE_NODE, pair, sizeof(pair), digest) != LXP_OK) return 1;
        index /= 2U;
        count = (count + 1U) / 2U;
    }
    return count == 1U && memcmp(digest, root, 32U) == 0 ? 0 : 1;
}

static int prove_balance(const lxp_kernel *kernel, const lx_account *account,
                         lxp_u128 expected, const uint8_t expected_root[32])
{
    uint8_t key[LX_ACCOUNT_STATE_LEAF_KEY_BYTES];
    uint8_t value[LX_ACCOUNT_STATE_LEAF_VALUE_MAX_BYTES];
    size_t length;
    uint8_t account_root[32];
    uint8_t universal_root[32];
    uint8_t root[32];
    const uint8_t universal_key[2] = {0U, 0U};
    const uint8_t tree_key[] = "account-tree";
    lxp_state_proof proof;
    CHECK(lxp_u128_cmp(account->balance, expected) == 0);
    CHECK(lx_account_state_leaf_material(account, key, value, &length) == LXP_OK);
    CHECK(lx_account_registry_proof(kernel->state->accounts, account->id, account_root, &proof) == LXP_OK);
    CHECK(prove_leaf(key, sizeof(key), value, length, &proof, account_root) == 0);
    CHECK(proof.depth > 0U);
    proof.siblings[0][0] ^= 1U;
    CHECK(prove_leaf(key, sizeof(key), value, length, &proof, account_root) != 0);
    proof.siblings[0][0] ^= 1U;
    value[0] ^= 1U;
    CHECK(prove_leaf(key, sizeof(key), value, length, &proof, account_root) != 0);
    CHECK(lxp_state_subtree_proof(kernel, 0U, tree_key, sizeof(tree_key) - 1U, universal_root, &proof) == LXP_OK);
    CHECK(prove_leaf(tree_key, sizeof(tree_key) - 1U, account_root, 32U, &proof, universal_root) == 0);
    CHECK(lxp_state_root_proof(kernel, 0U, root, &proof) == LXP_OK);
    CHECK(prove_leaf(universal_key, sizeof(universal_key), universal_root, 32U, &proof, root) == 0);
    CHECK(memcmp(root, expected_root, 32U) == 0);
    return 0;
}

int main(int argc, char **argv)
{
    uint8_t *manifest_bytes = NULL;
    uint8_t *activity_bytes = NULL;
    uint8_t *seed = NULL;
    size_t manifest_length = 0U;
    size_t activity_length = 0U;
    size_t seed_length = 0U;
    uint8_t *arena_bytes = malloc(4U * LXP_MAX_ACTIVITY_BYTES);
    lxp_genesis_manifest *manifest = malloc(sizeof(*manifest));
    lxp_state_store *state = malloc(sizeof(*state));
    lxp_state_journal *journal = calloc(1U, sizeof(*journal));
    lxp_kernel *kernel = malloc(sizeof(*kernel));
    lx_account_registry *accounts = malloc(sizeof(*accounts));
    lx_account_registry *before = malloc(sizeof(*before));
    lxp_module_ctx *ctx = malloc(sizeof(*ctx));
    lxp_effect_buffer *effects = malloc(sizeof(*effects));
    lxp_arena arena;
    lxp_activity activity;
    lxp_activity altered_activity;
    lxp_authority_resolved authority = {0};
    lxp_bridge_profile profile;
    lxp_bridge_profile changed_profile;
    lxp_bridge_credit credit;
    lxp_bridge_credit changed;
    uint8_t initial_root[32];
    uint8_t root[32];
    uint8_t nullifier[32];
    uint8_t signature[64];
    uint8_t name[LX_ACCOUNT_NAME_MAX];
    size_t name_length;
    bool present;
    size_t initial_kv;
    lxp_u128 amount;
    lxp_transfer_asset_state asset = {0};
    lx_asset_record asset_record = {0};
    lx_asset_runtime runtime = {0};
    lx_account *recipient;
    lx_account *reserve;
    lxp_receipt funding_receipt;
    lxp_u128 issued;
    uint8_t bridge_root[32];
    const uint8_t bridge_key[2] = {0U, LXP_MODULE_BRIDGE};
    lxp_state_proof proof;
    const uint8_t *value;
    size_t value_length;
    uint8_t supply_key[47] = "custody-issued:";
    uint8_t replay_key[50] = "deposit-nullifier:";
    if (argc != 4) {
        (void)fprintf(stderr, "usage: test-credit genesis.manifest signed-credit.activity actor-key\n");
        return 2;
    }
    CHECK(arena_bytes && manifest && state && journal && kernel && accounts && before && ctx && effects);
    CHECK(read_file(argv[1], LXP_GENESIS_MAX_ENCODED_BYTES, false, &manifest_bytes, &manifest_length) == 0);
    CHECK(read_file(argv[2], LXP_MAX_ACTIVITY_BYTES, false, &activity_bytes, &activity_length) == 0);
    CHECK(read_file(argv[3], 32U, true, &seed, &seed_length) == 0 && seed_length == 32U);
    CHECK(lxp_arena_init(&arena, arena_bytes, 4U * LXP_MAX_ACTIVITY_BYTES) == LXP_OK);
    CHECK(lxp_genesis_parse(manifest_bytes, manifest_length, LXP_GENESIS_INPUT_MANIFEST, manifest) == LXP_OK);
    CHECK(lxp_genesis_verify_signature(manifest, &arena) == LXP_OK);
    CHECK(lxp_bridge_genesis_profile(manifest, &profile, &present) == LXP_OK && present);
    CHECK(lxp_activity_decode(activity_bytes, activity_length, &activity) == LXP_OK);
    CHECK(lxp_activity_verify_signature(&activity) == LXP_OK);
    CHECK(activity.payload.length == sizeof(credit.bytes));
    (void)memcpy(credit.bytes, activity.payload.bytes, sizeof(credit.bytes));
    CHECK(lxp_bridge_credit_verify(&profile, &credit, manifest->network_id, 3U, nullifier) == LXP_OK);
    CHECK(lxp_u128_from_be(credit.bytes + 191U, &amount) == LXP_OK);
    for (size_t index = 0U; index < sizeof(credit.bytes); ++index) {
        changed = credit;
        changed.bytes[index] ^= 1U;
        CHECK(lxp_bridge_credit_verify(&profile, &changed, manifest->network_id, 3U, root) != LXP_OK);
    }
    for (size_t index = 0U; index < sizeof(profile.bytes); ++index) {
        changed_profile = profile;
        changed_profile.bytes[index] ^= 1U;
        CHECK(lxp_bridge_credit_verify(&changed_profile, &credit, manifest->network_id, 3U, root) != LXP_OK);
    }
    CHECK(lxp_bridge_credit_verify(&profile, &credit, manifest->network_id ^ 1U, 3U, root) != LXP_OK);
    CHECK(lxp_bridge_credit_verify(&profile, &credit, manifest->network_id, 2U, root) != LXP_OK);
    CHECK(lx_account_registry_init(accounts) == LXP_OK);
    CHECK(lxp_state_store_init(state, 1U) == LXP_OK);
    CHECK(lxp_state_store_bind_accounts(state, accounts) == LXP_OK);
    CHECK(lxp_kernel_create(kernel, state, journal, manifest, 1U) == LXP_OK);
    CHECK(lxp_kernel_register_module(kernel, programs_module_registration_v4()) == LXP_OK);
    CHECK(lxp_kernel_register_module(kernel, lx_asset_module_iface()) == LXP_OK);
    CHECK(lxp_kernel_register_module(kernel, lxp_bridge_module_iface()) == LXP_OK);
    CHECK(lxp_kernel_set_capabilities(kernel, NULL, lxp_kernel_canonical_ledger_apply) == LXP_OK);
    CHECK(lxp_genesis_materialize(manifest, &arena, kernel) == LXP_OK);
    (void)memcpy(asset.asset_id, profile.bytes + 97U, 32U);
    asset.registered = true;
    (void)memcpy(asset_record.asset_id, asset.asset_id, 32U);
    runtime.accounts = accounts;
    runtime.assets = &asset_record;
    runtime.asset_count = 1U;
    runtime.transfer_assets = &asset;
    runtime.transfer_asset_count = 1U;
    runtime.network_id = manifest->network_id;
    runtime.protocol_version = 3U;
    CHECK(lxp_kernel_bind_module_runtime(kernel, LXP_MODULE_ASSET, &runtime) == LXP_OK);
    CHECK(lxp_state_root(kernel, initial_root) == LXP_OK);
    CHECK(memcmp(initial_root, manifest->genesis_state_root, 32U) == 0);
    initial_kv = kernel->module_kv_count;
    *before = *accounts;
    for (size_t index = 0U; index < accounts->count; ++index)
        CHECK(lxp_u128_is_zero(accounts->accounts[index].balance));
    CHECK(activity.actor_did.length <= sizeof(name) - 11U);
    (void)memcpy(name, "agent:", 6U);
    (void)memcpy(name + 6U, activity.actor_did.bytes, activity.actor_did.length);
    name_length = 6U + activity.actor_did.length;
    (void)memcpy(name + name_length, ":main", 5U);
    name_length += 5U;
    CHECK(lx_account_id_from_string(name, name_length, authority.principal) == LXP_OK);
    authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(authority.verified_key, activity.authority.bytes, 32U);
    (void)memcpy(supply_key + 15U, profile.bytes + 97U, 32U);
    (void)memcpy(replay_key + 18U, nullifier, 32U);
    CHECK(lxp_module_ctx_init(ctx, kernel, LXP_MODULE_BRIDGE, 1U, 1U,
                              1U, 100000U, &arena, false) == LXP_OK);
    CHECK(lxp_ctx_account_find(ctx, profile.bytes + 129U, &reserve) == LXP_OK);
    CHECK(prove_balance(kernel, reserve, (lxp_u128){0U, 0U}, initial_root) == 0);
    CHECK(lxp_ctx_kv_get(ctx, supply_key, sizeof(supply_key), &value, &value_length) == LXP_OK &&
          value_length == 16U && lxp_ct_is_zero(value, value_length));
    CHECK(lxp_state_subtree_proof(kernel, LXP_MODULE_BRIDGE, supply_key, sizeof(supply_key), bridge_root, &proof) == LXP_OK);
    CHECK(prove_leaf(supply_key, sizeof(supply_key), value, value_length, &proof, bridge_root) == 0);
    CHECK(lxp_state_root_proof(kernel, LXP_MODULE_BRIDGE, root, &proof) == LXP_OK);
    CHECK(prove_leaf(bridge_key, sizeof(bridge_key), bridge_root, 32U, &proof, root) == 0);
    CHECK(memcmp(root, initial_root, 32U) == 0);
    for (unsigned int failure = 1U; failure <= 4U; ++failure) {
        CHECK(begin(ctx, kernel, &arena, effects, 1U) == 0);
        ctx->bridge_credit_fail_stage = failure;
        lxp_result result = lxp_ctx_bridge_credit(ctx, &activity, &authority, &credit);
        if (failure == 4U) {
            CHECK(result == LXP_OK);
            result = lxp_module_ctx_prepare_commit(ctx);
        }
        CHECK(result == LXP_ERR_IO);
        lxp_module_ctx_rollback(ctx);
        CHECK(lxp_state_journal_rollback(journal) == LXP_OK);
        CHECK(accounts->count == before->count && memcmp(accounts->accounts, before->accounts,
              accounts->count * sizeof(accounts->accounts[0])) == 0);
        CHECK(state->next_sequence == 1U && state->idempotency_count == 0U &&
              kernel->module_kv_count == initial_kv);
        CHECK(ctx->staged_count == 0U && ctx->staged_account_count == 0U && ctx->transfer_snapshot_count == 0U);
        CHECK(lxp_ctx_kv_get(ctx, supply_key, sizeof(supply_key), &value, &value_length) == LXP_OK &&
              value_length == 16U && lxp_ct_is_zero(value, value_length));
        CHECK(lxp_ctx_kv_get(ctx, replay_key, sizeof(replay_key), &value, &value_length) == LXP_ERR_UNKNOWN_FIELD);
        CHECK(lxp_state_root(kernel, root) == LXP_OK && memcmp(root, initial_root, 32U) == 0);
    }
    CHECK(begin(ctx, kernel, &arena, effects, 1U) == 0);
    CHECK(lxp_activity_id(activity_bytes, activity_length, ctx->activity_id) == LXP_OK);
    asset.paused = true;
    CHECK(lxp_ctx_bridge_credit(ctx, &activity, &authority, &credit) == LXP_ERR_ASSET_PAUSED);
    asset.paused = false;
    authority.principal[0] ^= 1U;
    CHECK(lxp_ctx_bridge_credit(ctx, &activity, &authority, &credit) == LXP_ERR_ACCOUNT_ID_MISMATCH);
    authority.principal[0] ^= 1U;
    authority.verified_key[0] ^= 1U;
    CHECK(lxp_ctx_bridge_credit(ctx, &activity, &authority, &credit) == LXP_ERR_UNAUTHORIZED_DEBIT);
    authority.verified_key[0] ^= 1U;
    CHECK(lxp_ctx_bridge_credit(ctx, &activity, &authority, &credit) == LXP_OK);
    CHECK(effects->count == 3U && effects->effects[0].monetary && effects->effects[1].body_length == 208U &&
          effects->effects[2].body_length == 112U && !ctx->ledger_receipt_present);
    (void)memset(&funding_receipt, 0, sizeof(funding_receipt));
    funding_receipt.protocol_version = 3U;
    funding_receipt.module_id = LXP_MODULE_BRIDGE;
    funding_receipt.module_version = 1U;
    funding_receipt.global_sequence = ctx->global_sequence;
    (void)memcpy(funding_receipt.activity_id, ctx->activity_id, 32U);
    (void)memcpy(funding_receipt.previous_state_root, initial_root, 32U);
    funding_receipt.effects = *effects;
    CHECK(lxp_bridge_credit_bind_receipt(&funding_receipt, ctx) == LXP_OK);
    CHECK(funding_receipt.operation == 0U && lxp_u128_is_zero(funding_receipt.from_balance_before) &&
          lxp_u128_is_zero(funding_receipt.amount));
    funding_receipt.from_balance_before = amount;
    CHECK(lxp_bridge_credit_bind_receipt(&funding_receipt, ctx) != LXP_OK);
    funding_receipt.from_balance_before = (lxp_u128){0U, 0U};
    for (size_t index = 32U; index < 112U; ++index) {
        effects->effects[2].body[index] ^= 1U;
        funding_receipt.effects = *effects;
        CHECK(lxp_bridge_credit_bind_receipt(&funding_receipt, ctx) != LXP_OK);
        effects->effects[2].body[index] ^= 1U;
    }
    for (size_t index = 176U; index < 208U; ++index) {
        effects->effects[1].body[index] ^= 1U;
        funding_receipt.effects = *effects;
        CHECK(lxp_bridge_credit_bind_receipt(&funding_receipt, ctx) != LXP_OK);
        effects->effects[1].body[index] ^= 1U;
    }
    funding_receipt.effects = *effects;
    CHECK(lxp_bridge_credit_bind_receipt(&funding_receipt, ctx) == LXP_OK);
    CHECK(lxp_module_ctx_prepare_commit(ctx) == LXP_OK);
    CHECK(lxp_state_journal_commit(journal) == LXP_OK);
    CHECK(lxp_module_ctx_commit(ctx) == LXP_OK);
    CHECK(accounts->count == before->count + 1U);
    CHECK(lxp_ctx_account_find(ctx, credit.bytes + 107U, &recipient) == LXP_OK);
    CHECK(lxp_u128_cmp(recipient->balance, amount) == 0 && recipient->has_authority_key &&
          memcmp(recipient->authority_key, activity.authority.bytes, 32U) == 0);
    CHECK(lxp_ctx_kv_get(ctx, replay_key, sizeof(replay_key), &value, &value_length) == LXP_OK &&
          value_length == sizeof(credit.bytes) && memcmp(value, credit.bytes, value_length) == 0);
    CHECK(lxp_state_root(kernel, initial_root) == LXP_OK);
    (void)memcpy(funding_receipt.resulting_state_root, initial_root, 32U);
    CHECK(prove_balance(kernel, reserve, (lxp_u128){0U, 0U}, funding_receipt.resulting_state_root) == 0);
    CHECK(prove_balance(kernel, recipient, amount, funding_receipt.resulting_state_root) == 0);
    CHECK(lxp_ctx_kv_get(ctx, supply_key, sizeof(supply_key), &value, &value_length) == LXP_OK && value_length == 16U);
    CHECK(lxp_u128_from_be(value, &issued) == LXP_OK && lxp_u128_cmp(issued, amount) == 0);
    CHECK(memcmp(value, funding_receipt.effects.effects[1].body + 192U, 16U) == 0);
    CHECK(lxp_state_subtree_proof(kernel, LXP_MODULE_BRIDGE, supply_key, sizeof(supply_key), bridge_root, &proof) == LXP_OK);
    CHECK(prove_leaf(supply_key, sizeof(supply_key), value, value_length, &proof, bridge_root) == 0);
    CHECK(lxp_state_root_proof(kernel, LXP_MODULE_BRIDGE, root, &proof) == LXP_OK);
    CHECK(prove_leaf(bridge_key, sizeof(bridge_key), bridge_root, 32U, &proof, root) == 0);
    CHECK(memcmp(root, funding_receipt.resulting_state_root, 32U) == 0);
    CHECK(begin(ctx, kernel, &arena, effects, state->next_sequence) == 0);
    altered_activity = activity;
    ++altered_activity.account_sequence;
    CHECK(sign_activity(&altered_activity, seed, signature) == 0);
    CHECK(lxp_ctx_bridge_credit(ctx, &altered_activity, &authority, &credit) == LXP_ERR_DEPOSIT_ALREADY_CREDITED);
    altered_activity.idempotency_key[0] ^= 1U;
    CHECK(sign_activity(&altered_activity, seed, signature) == 0);
    CHECK(lxp_ctx_bridge_credit(ctx, &altered_activity, &authority, &credit) == LXP_ERR_CONTEXT_MISMATCH);
    lxp_module_ctx_rollback(ctx);
    CHECK(lxp_state_journal_rollback(journal) == LXP_OK);
    CHECK(lxp_state_root(kernel, root) == LXP_OK && memcmp(root, initial_root, 32U) == 0);
    CHECK(lxp_state_store_destroy(state) == LXP_OK);
    lxp_secure_zero(seed, seed_length);
    free(seed);
    free(activity_bytes);
    free(manifest_bytes);
    free(effects);
    free(ctx);
    free(before);
    free(accounts);
    free(kernel);
    free(journal);
    free(state);
    free(manifest);
    free(arena_bytes);
    return 0;
}
