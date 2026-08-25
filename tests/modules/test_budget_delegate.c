#include "layerx/lx_budget.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <openssl/evp.h>
#include <string.h>

static lxp_result apply_capability(lxp_kernel *kernel,
                                   const lxp_transfer_set *set,
                                   lxp_receipt *receipt)
{
    lxp_transfer_set_result result;
    lxp_transfer_context context = set->context;
    lxp_result status;
    (void)kernel;
    status = lxp_apply_transfer_set((lxp_transfer_leg *)set->legs,
                                    set->leg_count, &context, &result);
    if (status == LXP_OK)
        (void)memcpy(receipt->transfer_set_root, result.transfer_set_root, 32U);
    return status;
}

static int sign_grant(lxp_payer_grant *grant, const uint8_t seed[32])
{
    uint8_t message[384];
    uint8_t digest[32];
    size_t message_length;
    size_t signature_length = 64U;
    size_t public_length = 32U;
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                  seed, 32U);
    EVP_MD_CTX *context = EVP_MD_CTX_new();
    int failed = key == NULL || context == NULL ||
        EVP_PKEY_get_raw_public_key(key, grant->public_key,
                                    &public_length) != 1 ||
        lxp_grant_authorization_message(grant, message, sizeof(message),
                                        &message_length) != LXP_OK ||
        lxp_hash_authority(message, message_length, grant->grant_id) != LXP_OK ||
        lxp_hash_domain(LXP_DOMAIN_AUTHORITY_HASH, message, message_length,
                        digest) != LXP_OK ||
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) != 1 ||
        EVP_DigestSign(context, grant->signature, &signature_length,
                       digest, sizeof(digest)) != 1 ||
        signature_length != 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return failed;
}

int main(void)
{
    static const uint8_t seed[32] = { 9U };
    lx_account budget_account;
    lx_account recipient;
    lx_account other;
    lx_account grantor;
    lx_asset_record asset;
    lxp_transfer_asset_state asset_state;
    lx_budget_store store;
    lx_budget_delegate_capability capability;
    lx_budget_delegate_spend_request delegated;
    lx_budget_pull_request pull;
    lxp_payer_grant grant;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    lxp_receipt receipt;
    uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;
    uint8_t delegate_a[32] = { 2U };
    uint8_t delegate_b[32] = { 1U };

    (void)memset(&budget_account, 0, sizeof(budget_account));
    (void)memset(&recipient, 0, sizeof(recipient));
    (void)memset(&other, 0, sizeof(other));
    (void)memset(&grantor, 0, sizeof(grantor));
    (void)memset(&asset, 0, sizeof(asset));
    (void)memset(&store, 0, sizeof(store));
    budget_account.id[0] = 3U; budget_account.kind = LX_ACCOUNT_AGENT_BUDGET;
    recipient.id[0] = 4U; recipient.kind = LX_ACCOUNT_AGENT_MAIN;
    other.id[0] = 5U; other.kind = LX_ACCOUNT_AGENT_MAIN;
    grantor.id[0] = 6U; grantor.kind = LX_ACCOUNT_AGENT_MAIN;
    asset.asset_id[0] = 7U;
    if (lxp_ledger_bootstrap_balance(&budget_account, asset.asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(&recipient, asset.asset_id,
                                     (lxp_u128){ 0U, 0U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(&other, asset.asset_id,
                                     (lxp_u128){ 0U, 0U }, 0U) != LXP_OK ||
        lx_asset_transfer_state(&asset, &asset_state) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_budget_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_BUDGET, 100U, 0U, 1U,
                            1000U, &arena, true) != LXP_OK)
        return 1;
    store.count = 1U;
    store.records[0].budget_id[0] = 8U;
    (void)memcpy(store.records[0].owner, grantor.id, 32U);
    (void)memcpy(store.records[0].budget_account, budget_account.id, 32U);
    (void)memcpy(store.records[0].asset_id, asset.asset_id, 32U);
    store.records[0].per_period_limit = (lxp_u128){ 0U, 100U };
    store.records[0].configured_period_limit = (lxp_u128){ 0U, 100U };
    store.records[0].period_start = 1U;
    store.records[0].period_length = 1000U;
    store.records[0].expiry = 1000U;
    store.records[0].revocation_sequence = 5U;
    store.records[0].purpose_hash[0] = 9U;
    if (lx_budget_delegate_add_execute(&store.records[0], delegate_a) != LXP_OK ||
        lx_budget_delegate_add_execute(&store.records[0], delegate_b) != LXP_OK ||
        memcmp(store.records[0].delegates[0], delegate_b, 32U) != 0 ||
        memcmp(store.records[0].delegates[1], delegate_a, 32U) != 0)
        return 1;
    store.records[0].delegate_count = LX_BUDGET_MAX_DELEGATES + 1U;
    if (lx_budget_delegate_add_execute(&store.records[0], delegate_a) !=
            LXP_ERR_NON_CANONICAL ||
        lx_budget_delegate_remove_execute(&store.records[0], delegate_a) !=
            LXP_ERR_NON_CANONICAL)
        return 1;
    store.records[0].delegate_count = 2U;
    (void)memset(&capability, 0, sizeof(capability));
    (void)memcpy(capability.holder, delegate_a, 32U);
    (void)memcpy(capability.asset_id, asset.asset_id, 32U);
    (void)memcpy(capability.recipient, recipient.id, 32U);
    (void)memcpy(capability.purpose_hash, store.records[0].purpose_hash, 32U);
    capability.maximum_per_spend = (lxp_u128){ 0U, 20U };
    capability.maximum_total = (lxp_u128){ 0U, 30U };
    capability.expiry = 200U;
    capability.revocation_sequence = 5U;
    (void)memset(&delegated, 0, sizeof(delegated));
    delegated.spend.store = &store;
    delegated.spend.budget_id = store.records[0].budget_id;
    delegated.spend.budget_account = &budget_account;
    delegated.spend.recipient = &recipient;
    delegated.spend.asset = &asset;
    delegated.spend.amount = (lxp_u128){ 0U, 20U };
    delegated.spend.context.assets = &asset_state;
    delegated.spend.context.asset_count = 1U;
    delegated.spend.context.sequence_account = &budget_account;
    (void)memcpy(delegated.spend.context.authorized_from,
                 budget_account.id, 32U);
    delegated.submitter = delegate_a;
    delegated.capability = &capability;
    if (lx_budget_delegate_spend_execute(&ctx, &delegated, &receipt) != LXP_OK ||
        capability.consumed.lo != 20U || budget_account.balance.lo != 80U)
        return 1;
    delegated.spend.amount = (lxp_u128){ 0U, 11U };
    delegated.spend.context.actor_sequence = budget_account.next_sequence;
    if (lx_budget_delegate_spend_execute(&ctx, &delegated, &receipt) !=
            LXP_ERR_UNAUTHORIZED_DELEGATE || capability.consumed.lo != 20U)
        return 1;
    capability.expiry = 99U;
    delegated.spend.amount = (lxp_u128){ 0U, 1U };
    if (lx_budget_delegate_spend_execute(&ctx, &delegated, &receipt) !=
        LXP_ERR_UNAUTHORIZED_DELEGATE) return 1;
    capability.expiry = 200U;
    (void)memcpy(capability.recipient, other.id, 32U);
    if (lx_budget_delegate_spend_execute(&ctx, &delegated, &receipt) !=
        LXP_ERR_UNAUTHORIZED_DELEGATE) return 1;
    (void)memcpy(capability.recipient, recipient.id, 32U);
    capability.revoked = true;
    if (lx_budget_delegate_spend_execute(&ctx, &delegated, &receipt) !=
        LXP_ERR_UNAUTHORIZED_DELEGATE) return 1;
    capability.revoked = false;
    if (lx_budget_delegate_remove_execute(&store.records[0], delegate_a) != LXP_OK ||
        lx_budget_delegate_spend_execute(&ctx, &delegated, &receipt) !=
            LXP_ERR_UNAUTHORIZED_DELEGATE)
        return 1;

    (void)memset(&grant, 0, sizeof(grant));
    (void)memcpy(grant.from, grantor.id, 32U);
    (void)memcpy(grant.recipient, recipient.id, 32U);
    (void)memcpy(grant.asset, asset.asset_id, 32U);
    grant.per_draw_maximum = (lxp_u128){ 0U, 10U };
    grant.allowance = (lxp_u128){ 0U, 20U };
    grant.expiration = 200U;
    grant.revocation_sequence = 5U;
    (void)memcpy(grant.purpose_hash, store.records[0].purpose_hash, 32U);
    if (sign_grant(&grant, seed) != 0) return 1;
    (void)memcpy(grantor.authority_key, grant.public_key, 32U);
    grantor.has_authority_key = true;
    (void)memset(&pull, 0, sizeof(pull));
    pull.spend = delegated.spend;
    pull.spend.amount = (lxp_u128){ 0U, 10U };
    pull.grant = &grant;
    pull.grantor = &grantor;
    if (lx_budget_pull_execute(&ctx, &pull, &receipt) != LXP_OK ||
        budget_account.balance.lo != 70U || recipient.balance.lo != 30U)
        return 1;
    pull.spend.amount = (lxp_u128){ 0U, 11U };
    pull.spend.context.actor_sequence = budget_account.next_sequence;
    if (lx_budget_pull_execute(&ctx, &pull, &receipt) !=
            LXP_ERR_GRANT_SCOPE_VIOLATION ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}
