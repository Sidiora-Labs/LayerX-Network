#include "layerx/lx_asset.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_receipt.h"

#include <openssl/evp.h>
#include <string.h>

static int sign_raw(const uint8_t seed[32], const uint8_t *message,
                    size_t message_length, uint8_t signature[64],
                    uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                  seed, 32U);
    EVP_MD_CTX *context = EVP_MD_CTX_new();
    size_t public_length = 32U;
    size_t signature_length = 64U;
    int ok = key != NULL && context != NULL &&
             EVP_PKEY_get_raw_public_key(key, public_key, &public_length) == 1 &&
             public_length == 32U &&
             EVP_DigestSignInit(context, NULL, NULL, NULL, key) == 1 &&
             EVP_DigestSign(context, signature, &signature_length,
                            message, message_length) == 1 &&
             signature_length == 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static int context_for_send(lxp_send *send)
{
    uint8_t material[32U + 32U + 32U + 16U + 32U];
    if (lxp_u128_to_be(send->amount, material + 96U) != LXP_OK) return 1;
    (void)memcpy(material, send->from, 32U);
    (void)memcpy(material + 32U, send->to, 32U);
    (void)memcpy(material + 64U, send->asset, 32U);
    (void)memcpy(material + 112U, send->idempotency_key, 32U);
    return lxp_hash_context_value(material, sizeof(material),
                                  send->context_hash) == LXP_OK ? 0 : 1;
}

static int sign_send(lxp_send *send, const uint8_t seed[32],
                     uint8_t public_key[32])
{
    uint8_t message[512];
    uint8_t digest[32];
    size_t message_length = 0U;
    (void)memcpy(send->authorization.signed_context_hash,
                 send->context_hash, 32U);
    if (lxp_send_authorization_message(send, message, sizeof(message),
                                       &message_length) != LXP_OK ||
        lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, message,
                        message_length, digest) != LXP_OK)
        return 1;
    if (sign_raw(seed, digest, sizeof(digest),
                 send->authorization.signature, public_key) != 0)
        return 1;
    (void)memcpy(send->authorization.public_key, public_key, 32U);
    return 0;
}

static lxp_result dispatch_send(const lxp_module_registration *registration,
                                lxp_module_ctx *ctx,
                                lxp_activity *activity,
                                const lxp_authority_resolved *authority,
                                const lxp_send *send,
                                lxp_effect_buffer *effects,
                                lxp_result *module_result)
{
    uint8_t encoded[512];
    size_t encoded_length = 0U;
    lxp_result status = lxp_send_encode(send, encoded, sizeof(encoded),
                                        &encoded_length);
    if (status != LXP_OK) return status;
    activity->payload = (lxp_byte_span){ encoded, encoded_length };
    return lxp_kernel_dispatch(registration, ctx, activity, authority,
                               effects, module_result);
}

static int balances(const lx_account *from, const lx_account *to,
                    uint64_t from_value, uint64_t to_value)
{
    return from->balance.hi == 0U && from->balance.lo == from_value &&
           to->balance.hi == 0U && to->balance.lo == to_value;
}

static int init_ctx(lxp_module_ctx *ctx, lxp_kernel *kernel, lxp_arena *arena,
                    lxp_effect_buffer *effects, uint64_t global_sequence,
                    uint8_t activity_marker)
{
    if (lxp_effect_buffer_init(effects) != LXP_OK ||
        lxp_module_ctx_init(ctx, kernel, LXP_MODULE_ASSET, 10U, 0U,
                            global_sequence, 1000U, arena, true) != LXP_OK)
        return 1;
    ctx->protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    ctx->batch_number = 1U;
    ctx->activity_id[0] = activity_marker;
    return lxp_module_ctx_bind_effects(ctx, effects) == LXP_OK ? 0 : 1;
}

int main(void)
{
    static const uint8_t seed[32] = {
        0x9dU, 0x61U, 0xb1U, 0x9dU, 0xefU, 0xfdU, 0x5aU, 0x60U,
        0xbaU, 0x84U, 0x4aU, 0xf4U, 0x92U, 0xecU, 0x2cU, 0xc4U,
        0x44U, 0x49U, 0xc5U, 0x69U, 0x7bU, 0x32U, 0x69U, 0x19U,
        0x70U, 0x3bU, 0xacU, 0x03U, 0x1cU, 0xaeU, 0x7fU, 0x60U
    };
    static const uint8_t actor_did[] = "did:key:a";
    lx_asset_record asset;
    lxp_transfer_asset_state asset_state;
    lx_account_registry accounts;
    lx_account *from;
    lx_account *to;
    const char *from_name = "agent:did:key:a:main";
    const char *to_name = "agent:did:key:b:main";
    uint8_t from_id[32];
    uint8_t to_id[32];
    uint8_t public_key[32];
    lx_asset_transfer_request request;
    lxp_transfer_source_authority source_authority;
    lx_asset_runtime runtime;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_receipt receipt;
    lxp_effect_buffer effects;
    lxp_arena arena;
    uint8_t arena_bytes[4096];
    uint8_t root_before[32];
    uint8_t root_after[32];
    uint64_t parameters = 1U;
    const lxp_module_registration *registration;
    lxp_send send;
    lxp_activity activity;
    lxp_authority_resolved authority;
    lxp_result module_result = LXP_OK;

    (void)memset(&asset, 0, sizeof(asset));
    asset.asset_id[0] = 1U;
    asset.symbol_length = 1U;
    (void)memcpy(asset.symbol, "A", 2U);
    asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    asset.custody_reference[0] = 1U;
    asset.custody_reference_length = 1U;
    if (lx_asset_transfer_state(&asset, &asset_state) != LXP_OK ||
        lx_account_registry_init(&accounts) != LXP_OK ||
        lx_account_id_from_string((const uint8_t *)from_name, strlen(from_name),
                                  from_id) != LXP_OK ||
        lx_account_id_from_string((const uint8_t *)to_name, strlen(to_name),
                                  to_id) != LXP_OK ||
        lx_account_open(&accounts, (const uint8_t *)from_name,
                        strlen(from_name), from_id, 1U,
                        LX_ACCOUNT_OPEN_CREDIT, NULL, &from) != LXP_OK ||
        lx_account_open(&accounts, (const uint8_t *)to_name, strlen(to_name),
                        to_id, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &to) != LXP_OK ||
        lxp_ledger_bootstrap_balance(from, asset.asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(to, asset.asset_id,
                                     (lxp_u128){ 0U, 0U }, 0U) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_state_store_bind_accounts(&state, &accounts) != LXP_OK ||
        lxp_state_store_require_account_root(&state) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_asset_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL,
                                    lxp_kernel_canonical_ledger_apply) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK)
        return 1;
    runtime = (lx_asset_runtime){ &accounts, &asset, 1U, &asset_state, 1U,
                                  7U, LXP_PROTOCOL_VERSION_OCCUPANCY };
    if (lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_ASSET, &runtime) !=
            LXP_OK ||
        lxp_kernel_module_for_activity(&kernel, LX_ASSET_SEND, 0U,
                                       &registration) != LXP_OK ||
        lxp_state_root(&kernel, root_before) != LXP_OK ||
        init_ctx(&ctx, &kernel, &arena, &effects, 1U, 1U) != 0)
        return 1;

    (void)memset(&request, 0, sizeof(request));
    (void)memset(&source_authority, 0, sizeof(source_authority));
    request.from = from;
    request.to = to;
    request.asset = &asset;
    request.amount = (lxp_u128){ 0U, 25U };
    request.context.assets = &asset_state;
    request.context.asset_count = 1U;
    request.context.sequence_account = from;
    request.context.debit_authority_kind = LXP_AUTH_OWNER;
    source_authority.debit_authority_kind = LXP_AUTH_OWNER;
    (void)memcpy(request.context.authorized_from, from_id, 32U);
    (void)memcpy(source_authority.authorized_from, from_id, 32U);
    request.context.source_authorities = &source_authority;
    request.context.source_authority_count = 1U;
    if (lx_asset_validate(&request) != LXP_OK || !balances(from, to, 100U, 0U) ||
        lx_asset_send_execute(&ctx, &request, &receipt) != LXP_OK ||
        !balances(from, to, 75U, 25U) || from->next_sequence != 1U ||
        effects.count != 1U || !effects.effects[0].monetary ||
        effects.effects[0].kind != LXP_EFFECT_TRANSFER ||
        memcmp(effects.effects[0].transfer_set_root,
               receipt.transfer_set_root, 32U) != 0 ||
        lxp_state_root(&kernel, root_after) != LXP_OK ||
        memcmp(root_before, root_after, 32U) == 0)
        return 1;
    lxp_module_ctx_rollback(&ctx);
    if (!balances(from, to, 100U, 0U) || from->next_sequence != 0U ||
        lxp_state_root(&kernel, root_after) != LXP_OK ||
        memcmp(root_before, root_after, 32U) != 0)
        return 1;

    request.direct_balance_write = true;
    if (lx_asset_validate(&request) != LXP_ERR_BALANCE_BYPASS ||
        !balances(from, to, 100U, 0U))
        return 1;
    request.direct_balance_write = false;
    request.amount = (lxp_u128){ 0U, 101U };
    if (lx_asset_validate(&request) != LXP_ERR_INSUFFICIENT_BALANCE ||
        !balances(from, to, 100U, 0U))
        return 1;
    if (lxp_ledger_bootstrap_balance(
            to, asset.asset_id,
            (lxp_u128){ UINT64_MAX, UINT64_MAX }, 0U) != LXP_OK)
        return 1;
    request.amount = (lxp_u128){ 0U, 1U };
    if (lx_asset_validate(&request) != LXP_ERR_OVERFLOW ||
        from->balance.lo != 100U || to->balance.lo != UINT64_MAX ||
        lxp_ledger_bootstrap_balance(to, asset.asset_id,
                                     (lxp_u128){ 0U, 0U }, 0U) != LXP_OK)
        return 1;
    request.amount = (lxp_u128){ 0U, 0U };
    if (lx_asset_validate(&request) != LXP_ERR_INVALID_AMOUNT ||
        !balances(from, to, 100U, 0U))
        return 1;
    request.amount = (lxp_u128){ 0U, 1U };
    asset.asset_id[0] = 2U;
    if (lx_asset_validate(&request) != LXP_ERR_ASSET_MISMATCH ||
        !balances(from, to, 100U, 0U))
        return 1;
    asset.asset_id[0] = 1U;
    request.payer_grant = NULL;
    if (init_ctx(&ctx, &kernel, &arena, &effects, 1U, 2U) != 0 ||
        lx_asset_receive_execute(&ctx, &request, &receipt) !=
            LXP_ERR_NO_PAYER_GRANT)
        return 1;

    (void)memset(&send, 0, sizeof(send));
    (void)memcpy(send.from, from_id, 32U);
    (void)memcpy(send.to, to_id, 32U);
    (void)memcpy(send.asset, asset.asset_id, 32U);
    send.amount = (lxp_u128){ 0U, 25U };
    send.sequence = 0U;
    send.idempotency_key[0] = 9U;
    send.expires_at = 20U;
    send.authorization.kind = LXP_AUTH_OWNER;
    (void)memcpy(send.authorization.controller, from_id, 32U);
    send.authorization.network_id = 7U;
    send.authorization.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    if (context_for_send(&send) != 0 || sign_send(&send, seed, public_key) != 0)
        return 1;
    (void)memcpy(from->authority_key, public_key, 32U);
    from->has_authority_key = true;
    if (lxp_state_root(&kernel, root_before) != LXP_OK) return 1;
    (void)memset(&activity, 0, sizeof(activity));
    activity.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    activity.network_id = 7U;
    activity.activity_type = LX_ASSET_SEND;
    activity.actor_did = (lxp_byte_span){ actor_did, sizeof(actor_did) - 1U };
    (void)memcpy(activity.idempotency_key, send.idempotency_key, 32U);
    (void)memset(&authority, 0, sizeof(authority));
    authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(authority.verified_key, public_key, 32U);
    if (init_ctx(&ctx, &kernel, &arena, &effects, 1U, 3U) != 0 ||
        dispatch_send(registration, &ctx, &activity, &authority, &send,
                      &effects, &module_result) != LXP_OK ||
        module_result != LXP_OK || !ctx.ledger_receipt_present ||
        !balances(from, to, 75U, 25U) || from->next_sequence != 1U ||
        effects.count != 1U || !effects.effects[0].monetary ||
        memcmp(effects.effects[0].transfer_set_root,
               ctx.ledger_receipt.transfer_set_root, 32U) != 0 ||
        ctx.ledger_receipt.amount.hi != 0U ||
        ctx.ledger_receipt.amount.lo != 25U ||
        ctx.ledger_receipt.from_sequence != 0U ||
        ctx.ledger_receipt.from_balance_before.lo != 100U ||
        ctx.ledger_receipt.from_balance_after.lo != 75U ||
        ctx.ledger_receipt.to_balance_before.lo != 0U ||
        ctx.ledger_receipt.to_balance_after.lo != 25U ||
        lxp_module_ctx_commit(&ctx) != LXP_OK ||
        lxp_state_root(&kernel, root_after) != LXP_OK ||
        memcmp(root_before, root_after, 32U) == 0)
        return 1;

    if (init_ctx(&ctx, &kernel, &arena, &effects, 2U, 4U) != 0 ||
        dispatch_send(registration, &ctx, &activity, &authority, &send,
                      &effects, &module_result) != LXP_OK ||
        module_result != LXP_ERR_SEQUENCE_MISMATCH ||
        !balances(from, to, 75U, 25U) || from->next_sequence != 1U ||
        effects.count != 0U || ctx.ledger_receipt_present)
        return 1;

    send.sequence = 1U;
    send.amount = (lxp_u128){ 0U, 76U };
    send.idempotency_key[0] = 10U;
    (void)memcpy(activity.idempotency_key, send.idempotency_key, 32U);
    if (context_for_send(&send) != 0 || sign_send(&send, seed, public_key) != 0 ||
        init_ctx(&ctx, &kernel, &arena, &effects, 2U, 5U) != 0 ||
        dispatch_send(registration, &ctx, &activity, &authority, &send,
                      &effects, &module_result) != LXP_OK ||
        module_result != LXP_ERR_INSUFFICIENT_BALANCE ||
        !balances(from, to, 75U, 25U) || from->next_sequence != 1U ||
        effects.count != 0U || ctx.ledger_receipt_present)
        return 1;

    send.amount = (lxp_u128){ 0U, 10U };
    if (context_for_send(&send) != 0 || sign_send(&send, seed, public_key) != 0)
        return 1;
    send.authorization.signature[0] ^= 1U;
    if (init_ctx(&ctx, &kernel, &arena, &effects, 2U, 6U) != 0 ||
        dispatch_send(registration, &ctx, &activity, &authority, &send,
                      &effects, &module_result) != LXP_OK ||
        module_result != LXP_ERR_UNAUTHORIZED_DEBIT ||
        !balances(from, to, 75U, 25U) || from->next_sequence != 1U ||
        effects.count != 0U || ctx.ledger_receipt_present)
        return 1;

    if (sign_send(&send, seed, public_key) != 0) return 1;
    send.sequence = 2U;
    if (sign_send(&send, seed, public_key) != 0 ||
        init_ctx(&ctx, &kernel, &arena, &effects, 2U, 7U) != 0 ||
        dispatch_send(registration, &ctx, &activity, &authority, &send,
                      &effects, &module_result) != LXP_OK ||
        module_result != LXP_ERR_SEQUENCE_MISMATCH ||
        !balances(from, to, 75U, 25U) || from->next_sequence != 1U ||
        effects.count != 0U || ctx.ledger_receipt_present)
        return 1;

    send.sequence = 1U;
    if (sign_send(&send, seed, public_key) != 0) return 1;
    activity.idempotency_key[0] ^= 1U;
    if (init_ctx(&ctx, &kernel, &arena, &effects, 2U, 8U) != 0 ||
        dispatch_send(registration, &ctx, &activity, &authority, &send,
                      &effects, &module_result) != LXP_OK ||
        module_result != LXP_ERR_UNAUTHORIZED_DEBIT ||
        !balances(from, to, 75U, 25U) || from->next_sequence != 1U ||
        effects.count != 0U || ctx.ledger_receipt_present ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}
