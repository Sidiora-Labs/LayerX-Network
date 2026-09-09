#include "layerx/lx_asset.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_state_proof.h"
#include <openssl/evp.h>
#include <stdio.h>
#include <stdlib.h>
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


#define CHECK(x) do { if (!(x)) { fprintf(stderr, "case %u line %d\n", mode, __LINE__); return 1; } } while (0)

static int run(unsigned mode, int vectors)
{
    static uint8_t arena_bytes[1048576];
    const uint8_t seed[32] = {1U};
    static uint8_t canonical[LXP_MAX_ACTIVITY_BYTES];
    uint8_t public_key[32], signature[64], digest[32];
    uint8_t before[32], after[32], payload[109] = {0};
    const uint8_t actor[] = "did:key:a";
    const char *names[] = {"agent:did:key:a:main", "system:paxeer-withdrawals"};
    lx_asset_record asset = {0};
    lxp_transfer_asset_state asset_state;
    lx_account_registry accounts;
    lx_account *from, *to;
    lx_account *opened[2];
    lx_asset_runtime runtime;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_effect_buffer effects;
    lxp_arena arena, wire_arena;
    lxp_byte_span wire;
    lxp_activity activity = {0};
    lxp_authority_resolved authority = {0};
    const lxp_module_registration *registration;
    uint64_t parameters = 1U;
    lxp_result status, result = LXP_OK;
    CHECK(sign_raw(seed, payload, 0U, signature, public_key) == 0);
    asset.asset_id[0] = 1U;
    asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    CHECK(lx_asset_transfer_state(&asset, &asset_state) == LXP_OK);
    CHECK(lx_account_registry_init(&accounts) == LXP_OK);
    for (size_t i = 0U; i < 2U; ++i) {
        uint8_t id[32];
        CHECK(lx_account_id_from_string((const uint8_t *)names[i], strlen(names[i]), id) == LXP_OK);
        CHECK(lx_account_open(&accounts, (const uint8_t *)names[i], strlen(names[i]), id,
            1U, i == 0U ? LX_ACCOUNT_OPEN_CREDIT : LX_ACCOUNT_OPEN_GENESIS, NULL, &opened[i]) == LXP_OK);
        CHECK(lxp_ledger_bootstrap_balance(opened[i], asset.asset_id,
            (lxp_u128){0U, i == 0U ? 100U : 0U}, 0U) == LXP_OK);
    }
    from = opened[0]; to = opened[1];
    memcpy(from->authority_key, public_key, 32U); from->has_authority_key = true;
    CHECK(lxp_state_store_init(&state, 0U) == LXP_OK);
    CHECK(lxp_state_store_bind_accounts(&state, &accounts) == LXP_OK);
    CHECK(lxp_state_store_require_account_root(&state) == LXP_OK);
    CHECK(lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) == LXP_OK);
    CHECK(lxp_kernel_register_module(&kernel, lx_asset_module_iface()) == LXP_OK);
    CHECK(lxp_kernel_set_capabilities(&kernel, NULL, lxp_kernel_canonical_ledger_apply) == LXP_OK);
    runtime = (lx_asset_runtime){&accounts, &asset, 1U, &asset_state, 1U, 7U, LXP_PROTOCOL_VERSION_OCCUPANCY};
    CHECK(lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_ASSET, &runtime) == LXP_OK);
    CHECK(lxp_kernel_module_for_activity(&kernel, LX_ASSET_WITHDRAW, 0U, &registration) == LXP_OK);
    memcpy(payload, asset.asset_id, 32U); payload[47] = 25U;
    payload[48] = 9U; payload[68] = 3U; payload[107] = 100U;
    activity.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    activity.network_id = 7U; activity.activity_type = LX_ASSET_WITHDRAW;
    activity.actor_did = (lxp_byte_span){actor, sizeof(actor) - 1U};
    activity.authority = (lxp_byte_span){public_key, 32U};
    activity.signature = (lxp_byte_span){signature, 64U};
    activity.payload = (lxp_byte_span){payload, 108U};
    activity.idempotency_key[0] = 1U;
    activity.fee_limit.lo = 100U;
    activity.timestamp_bound.not_after = 100U;
    authority.kind = LXP_AUTHORITY_OWNER;
    memcpy(authority.verified_key, public_key, 32U);
    if (mode == 2U) payload[48] = 0U;
    if (mode == 3U) payload[68] = 0U;
    if (mode == 4U) activity.fee_limit.lo = 99U;
    if (mode == 5U) activity.account_sequence = 1U;
    if (mode == 6U) payload[47] = 101U;
    if (mode == 7U) payload[0] = 2U;
    if (mode == 8U) activity.payload.length = 107U;
    if (mode == 9U) activity.payload.length = 109U;
    if (mode == 10U) { from->has_authority_key = false; memset(from->authority_key, 0, 32U); }
    if (mode == 11U) authority.verified_key[0] ^= 1U;
    if (mode == 12U) payload[47] = 0U;
    if (mode == 13U) { asset.paused = true; asset_state.paused = true; }
    if (mode == 15U) from->authority_key[0] ^= 1U;
    if (mode == 16U) activity.fee_limit.hi = 1U;
    if (mode == 17U) from->frozen = true;
    CHECK(lxp_hash_payload(activity.payload.bytes, activity.payload.length, activity.payload_hash) == LXP_OK);
    CHECK(lxp_activity_signing_preimage(&activity, digest) == LXP_OK);
    CHECK(sign_raw(seed, digest, 32U, signature, public_key) == 0);
    if (mode == 1U) signature[0] ^= 1U;
    if (mode == 14U) payload[48] ^= 1U;
    CHECK(lxp_arena_init(&wire_arena, canonical, sizeof(canonical)) == LXP_OK);
    CHECK(lxp_activity_encode(&activity, &wire_arena, &wire) == LXP_OK);
    CHECK(lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) == LXP_OK);
    CHECK(lxp_effect_buffer_init(&effects) == LXP_OK);
    CHECK(lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_ASSET, 10U, 0U, 1U, 100000U, &arena, true) == LXP_OK);
    ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY; ctx.batch_number = 1U;
    CHECK(lxp_activity_id(wire.bytes, wire.length, ctx.activity_id) == LXP_OK);
    CHECK(lxp_module_ctx_bind_effects(&ctx, &effects) == LXP_OK);
    CHECK(lxp_state_root(&kernel, before) == LXP_OK);
    status = lxp_kernel_dispatch(registration, &ctx, &activity, &authority, &effects, &result);
    if (mode != 0U) {
        CHECK(status != LXP_OK || result != LXP_OK);
        CHECK(from->balance.lo == 100U && to->balance.lo == 0U && from->next_sequence == 0U);
        CHECK(lxp_state_root(&kernel, after) == LXP_OK && memcmp(before, after, 32U) == 0);
    } else {
        uint8_t key[LX_WITHDRAWAL_STATE_KEY_BYTES], value[LX_WITHDRAWAL_STATE_VALUE_BYTES];
        lx_withdrawal_request request = {0};
        lx_withdrawal_record record;
        lxp_state_witness *proof = malloc(sizeof(*proof));
        CHECK(status == LXP_OK && result == LXP_OK);
        CHECK(from->balance.lo == 75U && to->balance.lo == 25U && from->next_sequence == 1U);
        CHECK(lxp_module_ctx_commit(&ctx) == LXP_OK);
        CHECK(lxp_state_root(&kernel, after) == LXP_OK && memcmp(before, after, 32U) != 0);
        request.network_id = 7U; request.amount.lo = 25U;
        memcpy(request.withdrawal_id, ctx.activity_id, 32U);
        memcpy(request.account_id, from->id, 32U); memcpy(request.asset_id, asset.asset_id, 32U);
        request.payout_recipient[12] = 9U; request.checkpoint_id[0] = 3U;
        CHECK(lx_withdrawal_state_encode(&request, key, value) == LXP_OK);
        CHECK(proof != NULL && lxp_state_proof_build(&kernel, LXP_MODULE_ASSET,
            (lxp_byte_span){key, sizeof(key)}, proof) == LXP_OK);
        CHECK(lxp_state_proof_verify(proof, after) == LXP_OK);
        CHECK(lx_withdrawal_state_decode(proof->key, proof->key_length, proof->value,
            proof->value_length, &record) == LXP_OK);
        CHECK(memcmp(value, proof->value, sizeof(value)) == 0);
        if (vectors != 0) {
            uint8_t *encoded = malloc(LXP_STATE_WITNESS_MAX_BYTES);
            size_t encoded_length = 0U;
            CHECK(encoded != NULL && lxp_state_proof_encode(proof, encoded,
                LXP_STATE_WITNESS_MAX_BYTES, &encoded_length) == LXP_OK);
            printf("{\"root\":\"0x");
            for (size_t i = 0U; i < 32U; ++i) printf("%02x", after[i]);
            printf("\",\"withdrawals_account\":\"0x");
            for (size_t i = 0U; i < 32U; ++i) printf("%02x", to->id[i]);
            printf("\",\"proof\":\"0x");
            for (size_t i = 0U; i < encoded_length; ++i) printf("%02x", encoded[i]);
            printf("\"}\n");
            free(encoded);
        }
        free(proof);
        CHECK(lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) == LXP_OK);
        CHECK(lxp_effect_buffer_init(&effects) == LXP_OK);
        CHECK(lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_ASSET, 10U, 0U, 2U, 100000U, &arena, true) == LXP_OK);
        ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY; ctx.batch_number = 2U;
        CHECK(lxp_activity_id(wire.bytes, wire.length, ctx.activity_id) == LXP_OK);
        CHECK(lxp_module_ctx_bind_effects(&ctx, &effects) == LXP_OK);
        CHECK(lxp_kernel_dispatch(registration, &ctx, &activity, &authority, &effects, &result) == LXP_OK);
        CHECK(result == LXP_ERR_WITHDRAWAL_ALREADY_SETTLED);
        CHECK(from->balance.lo == 75U && to->balance.lo == 25U && from->next_sequence == 1U);
        CHECK(lxp_state_root(&kernel, before) == LXP_OK && memcmp(before, after, 32U) == 0);
    }
    CHECK(lxp_state_store_destroy(&state) == LXP_OK);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc == 2 && strcmp(argv[1], "--vectors") == 0) return run(0U, 1);
    if (argc != 1) return 1;
    for (unsigned mode = 0U; mode < 18U; ++mode)
        if (run(mode, 0) != 0) return 1;
    return 0;
}
