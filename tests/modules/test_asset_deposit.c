#include "layerx/lx_asset.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_receipt.h"

#include <inttypes.h>
#include <openssl/evp.h>
#include <stdio.h>
#include <string.h>

static int check_equal(intmax_t actual, intmax_t expected,
                       const char *assertion, int line)
{
    if (actual == expected) return 1;
    (void)fprintf(stderr,
                  "%s:%d: assertion failed: %s; actual=%" PRIdMAX
                  "; expected=%" PRIdMAX "; exit_code=1\n",
                  __FILE__, line, assertion, actual, expected);
    return 0;
}

#define CHECK_EQ(actual, expected) \
    check_equal((intmax_t)(actual), (intmax_t)(expected), \
                #actual " == " #expected, __LINE__)

static int root_register(
    lx_checkpoint_registry **registry, const lx_deposit_proof *proof,
    const uint8_t deposit_root[32])
{
    static const uint8_t private_key[32] = {9U};
    lx_paxeer_deposit_root_registration registration;
    uint8_t public_key[32];
    uint8_t message[192];
    size_t public_key_length = 32U;
    size_t message_length;
    size_t signature_length = 64U;
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(
        EVP_PKEY_ED25519, NULL, private_key, 32U);
    EVP_MD_CTX *context = EVP_MD_CTX_new();
    int ok = CHECK_EQ(key != NULL, 1) && CHECK_EQ(context != NULL, 1) &&
        CHECK_EQ(EVP_PKEY_get_raw_public_key(
            key, public_key, &public_key_length), 1) &&
        CHECK_EQ(public_key_length, 32U);
    (void)memset(&registration, 0, sizeof(registration));
    (void)memcpy(registration.checkpoint_id, proof->checkpoint_id, 32U);
    registration.checkpoint_state_root[0] = 3U;
    (void)memcpy(registration.deposit_root, deposit_root, 32U);
    (void)memcpy(registration.custody_reference,
                 proof->custody_reference, 32U);
    registration.network_id = proof->network_id;
    registration.protocol_version = proof->protocol_version;
    ok = ok && CHECK_EQ(lx_paxeer_deposit_root_message(
        &registration, message, sizeof(message), &message_length), LXP_OK) &&
        CHECK_EQ(EVP_DigestSignInit(context, NULL, NULL, NULL, key), 1) &&
        CHECK_EQ(EVP_DigestSign(context, registration.signature, &signature_length,
                       message, message_length), 1) &&
        CHECK_EQ(signature_length, 64U) &&
        CHECK_EQ(lx_checkpoint_registry_create(
            public_key, proof->network_id, proof->protocol_version,
            registry), LXP_OK) &&
        CHECK_EQ(lx_checkpoint_registry_register_deposit_root(
            *registry, &registration), LXP_OK);
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

int main(void)
{
    lx_asset_registry assets;
    lx_asset_record asset;
    lx_account_registry accounts;
    lx_account *reserve;
    lx_account *agent;
    const char *reserve_name = "system:paxeer-reserve";
    const char *agent_name = "agent:did:key:a:main";
    lx_checkpoint_registry *checkpoints = NULL;
    lx_deposit_proof proof;
    lx_asset_transfer_request request;
    lxp_transfer_source_authority authority;
    lxp_transfer_asset_state asset_state;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_receipt receipt;
    lxp_arena arena;
    uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;
    lxp_u128 total;

    (void)memset(&asset, 0, sizeof(asset));
    asset.asset_id[0] = 1U;
    (void)memcpy(asset.symbol, "A", 2U);
    asset.symbol_length = 1U;
    asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    asset.custody_reference[0] = 5U;
    asset.custody_reference_length = 32U;
    if (!CHECK_EQ(lx_asset_registry_init(&assets, 0U), LXP_OK) ||
        !CHECK_EQ(lx_asset_register(&assets, &asset, 0U,
                                    (lxp_u128){ 0U, 0U }), LXP_OK) ||
        !CHECK_EQ(lx_account_registry_init(&accounts), LXP_OK) ||
        !CHECK_EQ(lx_asset_account_open(&assets, &accounts, asset.asset_id,
                              (const uint8_t *)reserve_name,
                              strlen(reserve_name), 1U, LX_ACCOUNT_OPEN_GENESIS,
                              NULL, &reserve), LXP_OK) ||
        !CHECK_EQ(lx_asset_account_open(&assets, &accounts, asset.asset_id,
                              (const uint8_t *)agent_name, strlen(agent_name), 1U,
                              LX_ACCOUNT_OPEN_CREDIT, NULL, &agent), LXP_OK) ||
        !CHECK_EQ(lxp_ledger_bootstrap_balance(reserve, asset.asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U), LXP_OK) ||
        !CHECK_EQ(lx_asset_transfer_state(&asset, &asset_state), LXP_OK) ||
        !CHECK_EQ(lxp_state_store_init(&state, 0U), LXP_OK) ||
        !CHECK_EQ(lxp_kernel_create(&kernel, &state, &journal,
                                    &parameters, 0U), LXP_OK) ||
        !CHECK_EQ(lxp_kernel_register_module(
            &kernel, lx_asset_module_iface()), LXP_OK) ||
        !CHECK_EQ(lxp_kernel_set_capabilities(
            &kernel, NULL, lxp_kernel_canonical_ledger_apply), LXP_OK) ||
        !CHECK_EQ(lxp_arena_init(&arena, arena_bytes,
                                 sizeof(arena_bytes)), LXP_OK) ||
        !CHECK_EQ(lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_ASSET, 10U, 0U, 1U,
                            1000U, &arena, true), LXP_OK)) return 1;
    (void)memset(&proof, 0, sizeof(proof));
    proof.deposit_id[0] = 4U;
    proof.custody_reference[0] = 5U;
    (void)memcpy(proof.asset_id, asset.asset_id, 32U);
    proof.amount = (lxp_u128){ 0U, 25U };
    proof.checkpoint_id[0] = 2U;
    proof.network_id = 7U;
    proof.protocol_version = LXP_PROTOCOL_VERSION;
    proof.inclusion_proof.leaf_count = 1U;
    {
        uint8_t deposit_root[32];
        if (!CHECK_EQ(lx_paxeer_deposit_leaf_hash(&proof, deposit_root), LXP_OK) ||
            !CHECK_EQ(root_register(&checkpoints, &proof, deposit_root), 0))
            return 1;
    }
    (void)memset(&request, 0, sizeof(request));
    request.from = reserve;
    request.to = agent;
    request.asset = &asset;
    request.amount = proof.amount;
    request.context.assets = &asset_state;
    request.context.asset_count = 1U;
    request.context.protocol_system_capability = true;
    if (!CHECK_EQ(lx_asset_deposit_credit(&ctx, &request, &proof, checkpoints,
                                8U, LXP_PROTOCOL_VERSION, &receipt),
                  LXP_ERR_DEPOSIT_PROOF_NOT_FINAL) ||
        !CHECK_EQ(reserve->balance.lo, 100U))
        return 1;
    if (!CHECK_EQ(lx_asset_deposit_credit(&ctx, &request, &proof, checkpoints,
                                7U, LXP_PROTOCOL_VERSION, &receipt),
                  LXP_ERR_NON_CANONICAL) ||
        !CHECK_EQ(reserve->balance.lo, 100U) ||
        !CHECK_EQ(agent->balance.lo, 0U) ||
        !CHECK_EQ(ctx.staged_count, 0U)) return 1;
    (void)memset(&authority, 0, sizeof(authority));
    (void)memcpy(authority.authorized_from, reserve->id, 32U);
    authority.debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    authority.protocol_system_capability = true;
    request.context.debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    request.context.source_authorities = &authority;
    request.context.source_authority_count = 1U;
    if (!CHECK_EQ(lx_asset_deposit_credit(&ctx, &request, &proof, checkpoints,
                                7U, LXP_PROTOCOL_VERSION, &receipt), LXP_OK) ||
        !CHECK_EQ(reserve->balance.lo, 75U) || !CHECK_EQ(agent->balance.lo, 25U) ||
        !CHECK_EQ(lx_asset_total_units(&assets, &accounts,
                                       asset.asset_id, &total), LXP_OK) ||
        !CHECK_EQ(total.lo, 100U)) return 1;
    if (!CHECK_EQ(lx_asset_deposit_credit(&ctx, &request, &proof, checkpoints,
                                7U, LXP_PROTOCOL_VERSION, &receipt),
                  LXP_ERR_DEPOSIT_ALREADY_CREDITED) ||
        !CHECK_EQ(reserve->balance.lo, 75U) ||
        !CHECK_EQ(agent->balance.lo, 25U)) return 1;
    if (!CHECK_EQ(lx_checkpoint_registry_destroy(&checkpoints), LXP_OK) ||
        !CHECK_EQ(lxp_state_store_destroy(&state), LXP_OK)) return 1;
    return 0;
}
