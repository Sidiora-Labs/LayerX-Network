#include "layerx/programs.h"

#include "layerx/lxp_fee.h"
#include "layerx/lxp_kernel.h"

#include <stdlib.h>
#include <string.h>

typedef struct programs_fee_token {
    lx_account *actor;
    lx_account *treasury;
    lxp_u128 actor_balance;
    lxp_u128 treasury_balance;
    uint8_t actor_asset[32];
    uint8_t treasury_asset[32];
    uint64_t actor_sequence;
    uint64_t treasury_sequence;
    bool actor_has_asset;
    bool treasury_has_asset;
} programs_fee_token;

static lx_account *principal_account(lx_account_registry *registry,
                                     const uint8_t principal[32])
{
    size_t index;
    if (registry == NULL || principal == NULL) return NULL;
    for (index = 0U; index < registry->count; ++index)
        if (registry->accounts[index].kind == LX_ACCOUNT_AGENT_MAIN &&
            memcmp(registry->accounts[index].id, principal, 32U) == 0)
            return &registry->accounts[index];
    return NULL;
}

static lxp_result prepare_fee(lxp_kernel *kernel, const lxp_activity *activity,
                              const lxp_authority_resolved *authority,
                              lxp_u128 fee, void **transaction)
{
    lx_programs_transfer_runtime *runtime;
    programs_fee_token *token;
    lx_account *actor;
    lx_account *treasury;
    lxp_transfer_context context;
    lxp_transfer_result transfer;
    lxp_receipt receipt;
    lxp_result status;
    if (kernel == NULL || activity == NULL || authority == NULL ||
        transaction == NULL || lxp_u128_is_zero(fee))
        return LXP_ERR_NON_CANONICAL;
    *transaction = NULL;
    runtime = (lx_programs_transfer_runtime *)
        kernel->module_runtime[LXP_MODULE_PROGRAMS];
    if (runtime == NULL || runtime->accounts == NULL ||
        runtime->assets == NULL || runtime->asset_count == 0U)
        return LXP_FATAL_INVARIANT;
    actor = principal_account(runtime->accounts, authority->principal);
    if (actor == NULL) return LXP_ERR_AUTH_SCOPE;
    if (!actor->has_asset) return LXP_ERR_ASSET_MISMATCH;
    status = lxp_fee_treasury_account(runtime->accounts, &treasury);
    if (status != LXP_OK) return status;
    token = (programs_fee_token *)malloc(sizeof(*token));
    if (token == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    token->actor = actor;
    token->treasury = treasury;
    token->actor_balance = actor->balance;
    token->treasury_balance = treasury->balance;
    (void)memcpy(token->actor_asset, actor->asset_id, 32U);
    (void)memcpy(token->treasury_asset, treasury->asset_id, 32U);
    token->actor_sequence = actor->next_sequence;
    token->treasury_sequence = treasury->next_sequence;
    token->actor_has_asset = actor->has_asset;
    token->treasury_has_asset = treasury->has_asset;
    (void)memset(&context, 0, sizeof(context));
    context.assets = runtime->assets;
    context.asset_count = runtime->asset_count;
    (void)memcpy(context.authorized_from, authority->principal, 32U);
    context.actor_sequence = actor->next_sequence;
    context.protocol_system_capability = true;
    context.sequence_account = treasury;
    context.origin_module_id = LXP_MODULE_PROGRAMS;
    context.debit_authority_kind = LXP_AUTH_OWNER;
    (void)memset(&receipt, 0, sizeof(receipt));
    status = lxp_fee_charge(actor, treasury, actor->asset_id, fee,
                            activity->fee_limit, &context, &receipt,
                            &transfer);
    if (status != LXP_OK) {
        free(token);
        return status;
    }
    *transaction = token;
    return LXP_OK;
}

static void commit_fee(lxp_kernel *kernel, void *transaction)
{
    (void)kernel;
    free(transaction);
}

static void rollback_fee(lxp_kernel *kernel, void *transaction)
{
    programs_fee_token *token = (programs_fee_token *)transaction;
    (void)kernel;
    if (token == NULL) return;
    token->actor->balance = token->actor_balance;
    token->treasury->balance = token->treasury_balance;
    (void)memcpy(token->actor->asset_id, token->actor_asset, 32U);
    (void)memcpy(token->treasury->asset_id, token->treasury_asset, 32U);
    token->actor->next_sequence = token->actor_sequence;
    token->treasury->next_sequence = token->treasury_sequence;
    token->actor->has_asset = token->actor_has_asset;
    token->treasury->has_asset = token->treasury_has_asset;
    free(token);
}

lxp_result lxp_programs_bind_fee_transaction(lxp_kernel *kernel)
{
    static const lxp_kernel_fee_transaction transaction = {
        prepare_fee, commit_fee, rollback_fee
    };
    return lxp_kernel_set_fee_transaction(kernel, &transaction);
}
