#include "layerx/lx_asset.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_kernel.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "test_asset_payloads.h"

#define PAUSE_CHECK(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "pause dispatch check failed at line %d\n", __LINE__); \
    return 1; } } while (0)

typedef struct pause_fixture {
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lx_account_registry accounts;
    lx_asset_record genesis_asset;
    lxp_transfer_asset_state genesis_state;
    lx_asset_runtime runtime;
    lxp_module_ctx ctx;
    lxp_effect_buffer effects;
    lxp_activity activity;
    lxp_authority_resolved authority;
    lxp_arena arena;
    uint64_t parameters;
    uint8_t public_key[32];
    uint8_t signature[64];
    uint8_t payload[512];
} pause_fixture;

static uint8_t pause_arena_bytes[1048576];
static uint8_t pause_canonical[LXP_MAX_ACTIVITY_BYTES];

static int pause_sign(const uint8_t seed[32], const uint8_t *message,
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

static int pause_record(const pause_fixture *f, const uint8_t id[32],
                        lx_asset_record *record)
{
    uint8_t key[38];
    size_t index;
    (void)memcpy(key, "asset:", 6U);
    (void)memcpy(key + 6U, id, 32U);
    for (index = 0U; index < f->kernel.module_kv_count; ++index) {
        const lxp_module_kv_entry *entry = &f->kernel.module_kv[index];
        if (entry->module_id != LXP_MODULE_ASSET || entry->key_length != 38U ||
            memcmp(entry->key, key, 38U) != 0) continue;
        return lx_asset_record_decode(entry->value, entry->value_length,
                                      record) == LXP_OK ? 0 : 1;
    }
    return 1;
}

static int pause_submit_receipt(pause_fixture *f, const uint8_t seed[32],
                                const uint8_t *did, size_t did_length,
                                uint32_t activity_type, const uint8_t *payload,
                                size_t payload_length,
                                const uint8_t idempotency[32],
                                uint64_t sequence, bool journal,
                                lxp_result *result,
                                lxp_ledger_receipt_input *receipt)
{
    const lxp_module_registration *registration;
    lxp_arena wire_arena;
    lxp_byte_span wire;
    uint8_t digest[32] = {0};
    uint8_t actor[32];
    PAUSE_CHECK(payload_length <= sizeof(f->payload));
    PAUSE_CHECK(pause_sign(seed, digest, 32U, f->signature, f->public_key) == 0);
    PAUSE_CHECK(lxp_did_id_derive(did, did_length, actor) == LXP_OK);
    (void)memcpy(f->payload, payload, payload_length);
    (void)memset(&f->activity, 0, sizeof(f->activity));
    f->activity.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    f->activity.network_id = 7U;
    f->activity.activity_type = activity_type;
    f->activity.actor_did = (lxp_byte_span){did, did_length};
    f->activity.authority = (lxp_byte_span){f->public_key, 32U};
    f->activity.signature = (lxp_byte_span){f->signature, 64U};
    f->activity.payload = (lxp_byte_span){f->payload, payload_length};
    (void)memcpy(f->activity.idempotency_key, idempotency, 32U);
    f->activity.fee_limit.lo = 1000U;
    f->activity.timestamp_bound.not_after = 100U;
    PAUSE_CHECK(lxp_hash_payload(f->payload, payload_length,
                                 f->activity.payload_hash) == LXP_OK);
    PAUSE_CHECK(lxp_activity_signing_preimage(&f->activity, digest) == LXP_OK);
    PAUSE_CHECK(pause_sign(seed, digest, 32U, f->signature, f->public_key) == 0);
    (void)memset(&f->authority, 0, sizeof(f->authority));
    f->authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(f->authority.verified_key, f->public_key, 32U);
    (void)memcpy(f->authority.actor, actor, 32U);
    (void)memcpy(f->authority.principal, actor, 32U);
    PAUSE_CHECK(lxp_kernel_module_for_activity(&f->kernel, activity_type, 0U,
                                               &registration) == LXP_OK);
    PAUSE_CHECK(lxp_arena_init(&wire_arena, pause_canonical,
                               sizeof(pause_canonical)) == LXP_OK);
    PAUSE_CHECK(lxp_activity_encode(&f->activity, &wire_arena, &wire) == LXP_OK);
    PAUSE_CHECK(lxp_arena_init(&f->arena, pause_arena_bytes,
                               sizeof(pause_arena_bytes)) == LXP_OK);
    PAUSE_CHECK(lxp_effect_buffer_init(&f->effects) == LXP_OK);
    if (journal)
        PAUSE_CHECK(lxp_state_journal_open(&f->state, sequence,
                                           &f->journal) == LXP_OK);
    PAUSE_CHECK(lxp_module_ctx_init(&f->ctx, &f->kernel, LXP_MODULE_ASSET, 10U,
                                    0U, sequence, 100000U, &f->arena,
                                    true) == LXP_OK);
    f->ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    f->ctx.batch_number = sequence;
    PAUSE_CHECK(lxp_activity_id(wire.bytes, wire.length,
                                f->ctx.activity_id) == LXP_OK);
    PAUSE_CHECK(lxp_module_ctx_bind_effects(&f->ctx, &f->effects) == LXP_OK);
    *result = LXP_FATAL_INVARIANT;
    PAUSE_CHECK(lxp_kernel_dispatch(registration, &f->ctx, &f->activity,
                                    &f->authority, &f->effects,
                                    result) == LXP_OK);
    if (receipt != NULL) *receipt = f->ctx.ledger_receipt;
    if (*result == LXP_OK) {
        if (journal)
            PAUSE_CHECK(lxp_state_journal_commit(&f->journal) == LXP_OK);
        PAUSE_CHECK(lxp_module_ctx_commit(&f->ctx) == LXP_OK);
    } else if (journal) {
        PAUSE_CHECK(lxp_state_journal_rollback(&f->journal) == LXP_OK);
    }
    return 0;
}

static int pause_submit(pause_fixture *f, const uint8_t seed[32],
                        const uint8_t *did, size_t did_length,
                        uint32_t activity_type, const uint8_t *payload,
                        size_t payload_length, const uint8_t idempotency[32],
                        uint64_t sequence, bool journal, lxp_result *result)
{
    return pause_submit_receipt(f, seed, did, did_length, activity_type,
                                payload, payload_length, idempotency,
                                sequence, journal, result, NULL);
}

static int test_pause_dispatch(void)
{
    static const uint8_t issuer_seed[32] = {11U};
    static const uint8_t intruder_seed[32] = {12U};
    static const uint8_t issuer_did[] = "did:key:pause-issuer";
    static const uint8_t intruder_did[] = "did:key:pause-intruder";
    const char *holder_name = "agent:did:key:holder:main";
    const char *other_name = "agent:did:key:other:main";
    pause_fixture *f = (pause_fixture *)calloc(1U, sizeof(*f));
    lx_asset_record record;
    lxp_hash_context hash;
    lxp_send send;
    lx_account *holder;
    lx_account *other;
    uint8_t issuer_actor[32];
    uint8_t issuer_key[32];
    uint8_t scratch[64];
    uint8_t zero[32] = {0};
    uint8_t asset_id[32];
    uint8_t salt[32] = {8U};
    uint8_t holder_id[32];
    uint8_t other_id[32];
    uint8_t register_payload[89] = {0};
    uint8_t pause_payload[34] = {0};
    uint8_t supply_payload[82] = {0};
    uint8_t send_payload[512] = {0};
    uint8_t idempotency[32] = {0};
    size_t send_length = 0U;
    uint64_t sequence = 1U;
    lxp_result result = LXP_OK;

    PAUSE_CHECK(f != NULL);
    PAUSE_CHECK(pause_sign(issuer_seed, zero, 32U, scratch, issuer_key) == 0);
    PAUSE_CHECK(lxp_did_id_derive(issuer_did, sizeof(issuer_did) - 1U,
                                  issuer_actor) == LXP_OK);
    lxp_hash_init(&hash);
    PAUSE_CHECK(lxp_hash_update(&hash, (const uint8_t *)"LX:ASSET:v1",
                                11U) == LXP_OK);
    PAUSE_CHECK(lxp_hash_update(&hash, issuer_actor, 32U) == LXP_OK);
    PAUSE_CHECK(lxp_hash_update(&hash, salt, 32U) == LXP_OK);
    PAUSE_CHECK(lxp_hash_final(&hash, asset_id) == LXP_OK);

    PAUSE_CHECK(lx_account_registry_init(&f->accounts) == LXP_OK);
    PAUSE_CHECK(lx_account_id_from_string((const uint8_t *)holder_name,
                                          strlen(holder_name),
                                          holder_id) == LXP_OK);
    PAUSE_CHECK(lx_account_id_from_string((const uint8_t *)other_name,
                                          strlen(other_name),
                                          other_id) == LXP_OK);
    PAUSE_CHECK(lx_account_open(&f->accounts, (const uint8_t *)holder_name,
                                strlen(holder_name), holder_id, 1U,
                                LX_ACCOUNT_OPEN_CREDIT, NULL,
                                &holder) == LXP_OK);
    PAUSE_CHECK(lx_account_open(&f->accounts, (const uint8_t *)other_name,
                                strlen(other_name), other_id, 1U,
                                LX_ACCOUNT_OPEN_CREDIT, NULL,
                                &other) == LXP_OK);
    PAUSE_CHECK(lxp_ledger_bootstrap_balance(holder, asset_id,
                                             (lxp_u128){0U, 10U}, 0U) == LXP_OK);
    PAUSE_CHECK(lxp_ledger_bootstrap_balance(other, asset_id,
                                             (lxp_u128){0U, 0U}, 0U) == LXP_OK);
    holder->has_authority_key = true;
    (void)memcpy(holder->authority_key, issuer_key, 32U);

    f->genesis_asset.asset_id[0] = 0x5aU;
    f->genesis_asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    PAUSE_CHECK(lx_asset_transfer_state(&f->genesis_asset,
                                        &f->genesis_state) == LXP_OK);
    f->parameters = 1U;
    PAUSE_CHECK(lxp_state_store_init(&f->state, 1U) == LXP_OK);
    PAUSE_CHECK(lxp_state_store_bind_accounts(&f->state, &f->accounts) == LXP_OK);
    PAUSE_CHECK(lxp_state_store_require_account_root(&f->state) == LXP_OK);
    PAUSE_CHECK(lxp_kernel_create(&f->kernel, &f->state, &f->journal,
                                  &f->parameters, 0U) == LXP_OK);
    PAUSE_CHECK(lxp_kernel_register_module(&f->kernel,
                                           lx_asset_module_iface()) == LXP_OK);
    PAUSE_CHECK(lxp_kernel_set_capabilities(
        &f->kernel, NULL, lxp_kernel_canonical_ledger_apply) == LXP_OK);
    f->runtime = (lx_asset_runtime){&f->accounts, &f->genesis_asset, 1U,
        &f->genesis_state, 1U, 7U, LXP_PROTOCOL_VERSION_OCCUPANCY};
    PAUSE_CHECK(lxp_kernel_bind_module_runtime(&f->kernel, LXP_MODULE_ASSET,
                                               &f->runtime) == LXP_OK);

    register_payload[1] = 1U;
    (void)memcpy(register_payload + 2U, asset_id, 32U);
    (void)memcpy(register_payload + 34U, salt, 32U);
    register_payload[66] = 1U;
    register_payload[67] = 'T';
    register_payload[68] = 1U;
    register_payload[69] = 'T';
    PAUSE_CHECK(lxp_u128_to_be((lxp_u128){0U, 100U},
                               register_payload + 71U) == LXP_OK);
    register_payload[87] = 1U;
    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit(f, issuer_seed, issuer_did,
                             sizeof(issuer_did) - 1U, LX_ASSET_REGISTER,
                             register_payload, sizeof(register_payload),
                             idempotency, sequence++, true, &result) == 0);
    PAUSE_CHECK(result == LXP_OK);
    PAUSE_CHECK(pause_record(f, asset_id, &record) == 0);
    PAUSE_CHECK(!record.paused &&
                memcmp(record.issuer_did32, issuer_actor, 32U) == 0);

    pause_payload[1] = 1U;
    (void)memcpy(pause_payload + 2U, asset_id, 32U);
    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit(f, intruder_seed, intruder_did,
                             sizeof(intruder_did) - 1U, LX_ASSET_PAUSE,
                             pause_payload, sizeof(pause_payload), idempotency,
                             sequence++, false, &result) == 0);
    PAUSE_CHECK(result == LXP_ERR_UNAUTHORIZED_DEBIT);
    PAUSE_CHECK(pause_record(f, asset_id, &record) == 0 && !record.paused);

    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit(f, issuer_seed, issuer_did,
                             sizeof(issuer_did) - 1U, LX_ASSET_PAUSE,
                             pause_payload, sizeof(pause_payload), idempotency,
                             sequence++, false, &result) == 0);
    PAUSE_CHECK(result == LXP_OK);
    PAUSE_CHECK(pause_record(f, asset_id, &record) == 0 && record.paused);

    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit(f, issuer_seed, issuer_did,
                             sizeof(issuer_did) - 1U, LX_ASSET_PAUSE,
                             pause_payload, sizeof(pause_payload), idempotency,
                             sequence++, false, &result) == 0);
    PAUSE_CHECK(result == LXP_ERR_PAUSED_SCOPE);
    PAUSE_CHECK(pause_record(f, asset_id, &record) == 0 && record.paused);

    supply_payload[1] = 1U;
    (void)memcpy(supply_payload + 2U, asset_id, 32U);
    (void)memcpy(supply_payload + 34U, holder_id, 32U);
    PAUSE_CHECK(lxp_u128_to_be((lxp_u128){0U, 1U},
                               supply_payload + 66U) == LXP_OK);
    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit(f, issuer_seed, issuer_did,
                             sizeof(issuer_did) - 1U, LX_ASSET_MINT,
                             supply_payload, sizeof(supply_payload),
                             idempotency, sequence++, false, &result) == 0);
    PAUSE_CHECK(result == LXP_ERR_ASSET_PAUSED);
    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit(f, issuer_seed, issuer_did,
                             sizeof(issuer_did) - 1U, LX_ASSET_BURN,
                             supply_payload, sizeof(supply_payload),
                             idempotency, sequence++, false, &result) == 0);
    PAUSE_CHECK(result == LXP_ERR_ASSET_PAUSED);

    (void)memset(&send, 0, sizeof(send));
    (void)memcpy(send.from, holder_id, 32U);
    (void)memcpy(send.to, other_id, 32U);
    (void)memcpy(send.asset, asset_id, 32U);
    send.amount.lo = 1U;
    send.expires_at = 100U;
    send.idempotency_key[0] = 7U;
    send.authorization.kind = LXP_AUTH_OWNER;
    send.authorization.network_id = 7U;
    send.authorization.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    (void)memcpy(send.authorization.controller, holder_id, 32U);
    (void)memcpy(send.authorization.public_key, issuer_key, 32U);
    PAUSE_CHECK(lxp_send_encode(&send, send_payload, sizeof(send_payload),
                                &send_length) == LXP_OK);
    PAUSE_CHECK(pause_submit(f, issuer_seed, issuer_did,
                             sizeof(issuer_did) - 1U, LX_ASSET_SEND,
                             send_payload, send_length, send.idempotency_key,
                             sequence++, false, &result) == 0);
    PAUSE_CHECK(result == LXP_ERR_ASSET_PAUSED);

    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit(f, intruder_seed, intruder_did,
                             sizeof(intruder_did) - 1U, LX_ASSET_UNPAUSE,
                             pause_payload, sizeof(pause_payload), idempotency,
                             sequence++, false, &result) == 0);
    PAUSE_CHECK(result == LXP_ERR_UNAUTHORIZED_DEBIT);
    PAUSE_CHECK(pause_record(f, asset_id, &record) == 0 && record.paused);

    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit(f, issuer_seed, issuer_did,
                             sizeof(issuer_did) - 1U, LX_ASSET_UNPAUSE,
                             pause_payload, sizeof(pause_payload), idempotency,
                             sequence++, false, &result) == 0);
    PAUSE_CHECK(result == LXP_OK);
    PAUSE_CHECK(pause_record(f, asset_id, &record) == 0 && !record.paused);

    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit(f, issuer_seed, issuer_did,
                             sizeof(issuer_did) - 1U, LX_ASSET_UNPAUSE,
                             pause_payload, sizeof(pause_payload), idempotency,
                             sequence++, false, &result) == 0);
    PAUSE_CHECK(result == LXP_ERR_PAUSED_SCOPE);

    PAUSE_CHECK(pause_submit(f, issuer_seed, issuer_did,
                             sizeof(issuer_did) - 1U, LX_ASSET_SEND,
                             send_payload, send_length, send.idempotency_key,
                             sequence++, false, &result) == 0);
    PAUSE_CHECK(result != LXP_ERR_ASSET_PAUSED && result != LXP_OK);
    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit(f, issuer_seed, issuer_did,
                             sizeof(issuer_did) - 1U, LX_ASSET_MINT,
                             supply_payload, sizeof(supply_payload),
                             idempotency, sequence++, false, &result) == 0);
    PAUSE_CHECK(result != LXP_ERR_ASSET_PAUSED);

    pause_payload[1] = 2U;
    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit(f, issuer_seed, issuer_did,
                             sizeof(issuer_did) - 1U, LX_ASSET_PAUSE,
                             pause_payload, sizeof(pause_payload), idempotency,
                             sequence++, false, &result) == 0);
    PAUSE_CHECK(result == LXP_ERR_NON_CANONICAL);
    pause_payload[1] = 1U;
    pause_payload[0] = 1U;
    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit(f, issuer_seed, issuer_did,
                             sizeof(issuer_did) - 1U, LX_ASSET_UNPAUSE,
                             pause_payload, sizeof(pause_payload), idempotency,
                             sequence++, false, &result) == 0);
    PAUSE_CHECK(result == LXP_ERR_NON_CANONICAL);
    pause_payload[0] = 0U;
    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit(f, issuer_seed, issuer_did,
                             sizeof(issuer_did) - 1U, LX_ASSET_PAUSE,
                             pause_payload, sizeof(pause_payload) - 1U,
                             idempotency, sequence++, false, &result) == 0);
    PAUSE_CHECK(result == LXP_ERR_NON_CANONICAL);
    pause_payload[2] ^= 1U;
    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit(f, issuer_seed, issuer_did,
                             sizeof(issuer_did) - 1U, LX_ASSET_PAUSE,
                             pause_payload, sizeof(pause_payload), idempotency,
                             sequence++, false, &result) == 0);
    PAUSE_CHECK(result == LXP_ERR_ASSET_MISMATCH);

    PAUSE_CHECK(lxp_state_store_destroy(&f->state) == LXP_OK);
    free(f);
    return 0;
}

static int test_supply_binding_dispatch(void)
{
    static const uint8_t issuer_seed[32] = {13U};
    static const uint8_t issuer_did[] = "did:key:binding-issuer";
    pause_fixture *f = (pause_fixture *)calloc(1U, sizeof(*f));
    lx_asset_record record;
    lxp_ledger_receipt_input receipt;
    lxp_hash_context hash;
    uint8_t issuer_actor[32];
    uint8_t issuer_key[32];
    uint8_t scratch[64];
    uint8_t zero[32] = {0};
    uint8_t asset_id[32];
    uint8_t salt[32] = {9U};
    uint8_t register_payload[89] = {0};
    uint8_t pause_payload[34] = {0};
    uint8_t idempotency[32] = {0};
    uint64_t sequence = 1U;
    lxp_result result = LXP_OK;

    PAUSE_CHECK(f != NULL);
    PAUSE_CHECK(pause_sign(issuer_seed, zero, 32U, scratch, issuer_key) == 0);
    PAUSE_CHECK(lxp_did_id_derive(issuer_did, sizeof(issuer_did) - 1U,
                                  issuer_actor) == LXP_OK);
    lxp_hash_init(&hash);
    PAUSE_CHECK(lxp_hash_update(&hash, (const uint8_t *)"LX:ASSET:v1",
                                11U) == LXP_OK);
    PAUSE_CHECK(lxp_hash_update(&hash, issuer_actor, 32U) == LXP_OK);
    PAUSE_CHECK(lxp_hash_update(&hash, salt, 32U) == LXP_OK);
    PAUSE_CHECK(lxp_hash_final(&hash, asset_id) == LXP_OK);

    PAUSE_CHECK(lx_account_registry_init(&f->accounts) == LXP_OK);
    f->genesis_asset.asset_id[0] = 0x5bU;
    f->genesis_asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    PAUSE_CHECK(lx_asset_transfer_state(&f->genesis_asset,
                                        &f->genesis_state) == LXP_OK);
    f->parameters = 1U;
    PAUSE_CHECK(lxp_state_store_init(&f->state, 1U) == LXP_OK);
    PAUSE_CHECK(lxp_state_store_bind_accounts(&f->state, &f->accounts) == LXP_OK);
    PAUSE_CHECK(lxp_state_store_require_account_root(&f->state) == LXP_OK);
    PAUSE_CHECK(lxp_kernel_create(&f->kernel, &f->state, &f->journal,
                                  &f->parameters, 0U) == LXP_OK);
    PAUSE_CHECK(lxp_kernel_register_module(&f->kernel,
                                           lx_asset_module_iface()) == LXP_OK);
    PAUSE_CHECK(lxp_kernel_set_capabilities(
        &f->kernel, NULL, lxp_kernel_canonical_ledger_apply) == LXP_OK);
    f->runtime = (lx_asset_runtime){&f->accounts, &f->genesis_asset, 1U,
        &f->genesis_state, 1U, 7U, LXP_PROTOCOL_VERSION_OCCUPANCY};
    PAUSE_CHECK(lxp_kernel_bind_module_runtime(&f->kernel, LXP_MODULE_ASSET,
                                               &f->runtime) == LXP_OK);

    register_payload[1] = 1U;
    (void)memcpy(register_payload + 2U, asset_id, 32U);
    (void)memcpy(register_payload + 34U, salt, 32U);
    register_payload[66] = 1U;
    register_payload[67] = 'B';
    register_payload[68] = 1U;
    register_payload[69] = 'B';
    PAUSE_CHECK(lxp_u128_to_be((lxp_u128){0U, 100U},
                               register_payload + 71U) == LXP_OK);
    register_payload[87] = 1U;
    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit_receipt(f, issuer_seed, issuer_did,
                                     sizeof(issuer_did) - 1U, LX_ASSET_REGISTER,
                                     register_payload, sizeof(register_payload),
                                     idempotency, sequence++, true, &result,
                                     &receipt) == 0);
    PAUSE_CHECK(result == LXP_OK);
    PAUSE_CHECK(pause_record(f, asset_id, &record) == 0 && !record.paused);
    PAUSE_CHECK(receipt.supply_binding_version == 1U && receipt.operation == 1U &&
                memcmp(receipt.asset, asset_id, 32U) == 0 &&
                lxp_u128_is_zero(receipt.total_units_before) &&
                lxp_u128_is_zero(receipt.total_units_after));

    pause_payload[1] = 1U;
    (void)memcpy(pause_payload + 2U, asset_id, 32U);
    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit_receipt(f, issuer_seed, issuer_did,
                                     sizeof(issuer_did) - 1U, LX_ASSET_PAUSE,
                                     pause_payload, sizeof(pause_payload),
                                     idempotency, sequence++, false, &result,
                                     &receipt) == 0);
    PAUSE_CHECK(result == LXP_OK);
    PAUSE_CHECK(pause_record(f, asset_id, &record) == 0 && record.paused);
    PAUSE_CHECK(receipt.supply_binding_version == 1U && receipt.operation == 2U &&
                memcmp(receipt.asset, asset_id, 32U) == 0 &&
                lxp_u128_cmp(receipt.total_units_before,
                             record.total_units) == 0 &&
                lxp_u128_cmp(receipt.total_units_after,
                             record.total_units) == 0);

    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit_receipt(f, issuer_seed, issuer_did,
                                     sizeof(issuer_did) - 1U, LX_ASSET_PAUSE,
                                     pause_payload, sizeof(pause_payload),
                                     idempotency, sequence++, false, &result,
                                     &receipt) == 0);
    PAUSE_CHECK(result == LXP_ERR_PAUSED_SCOPE);
    PAUSE_CHECK(receipt.supply_binding_version == 0U &&
                lxp_u128_is_zero(receipt.total_units_before) &&
                lxp_u128_is_zero(receipt.total_units_after));

    idempotency[0] = (uint8_t)sequence;
    PAUSE_CHECK(pause_submit_receipt(f, issuer_seed, issuer_did,
                                     sizeof(issuer_did) - 1U, LX_ASSET_UNPAUSE,
                                     pause_payload, sizeof(pause_payload),
                                     idempotency, sequence++, false, &result,
                                     &receipt) == 0);
    PAUSE_CHECK(result == LXP_OK);
    PAUSE_CHECK(pause_record(f, asset_id, &record) == 0 && !record.paused);
    PAUSE_CHECK(receipt.supply_binding_version == 1U && receipt.operation == 3U &&
                memcmp(receipt.asset, asset_id, 32U) == 0 &&
                lxp_u128_cmp(receipt.total_units_before,
                             record.total_units) == 0 &&
                lxp_u128_cmp(receipt.total_units_after,
                             record.total_units) == 0);

    PAUSE_CHECK(lxp_state_store_destroy(&f->state) == LXP_OK);
    free(f);
    return 0;
}

static int withdraw_submit(pause_fixture *f, const uint8_t seed[32],
                           const uint8_t *did, size_t did_length,
                           const uint8_t principal[32], uint64_t sequence,
                           uint8_t activity_id[32], lxp_result *result,
                           lxp_ledger_receipt_input *receipt)
{
    const lxp_module_registration *registration;
    lxp_arena wire_arena;
    lxp_byte_span wire;
    uint8_t digest[32] = {0};
    uint8_t actor[32];
    PAUSE_CHECK(pause_sign(seed, digest, 32U, f->signature, f->public_key) == 0);
    PAUSE_CHECK(lxp_did_id_derive(did, did_length, actor) == LXP_OK);
    (void)memset(&f->activity, 0, sizeof(f->activity));
    f->activity.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    f->activity.network_id = 7U;
    f->activity.activity_type = LX_ASSET_WITHDRAW;
    f->activity.actor_did = (lxp_byte_span){did, did_length};
    f->activity.authority = (lxp_byte_span){f->public_key, 32U};
    f->activity.signature = (lxp_byte_span){f->signature, 64U};
    f->activity.payload = (lxp_byte_span){f->payload, 108U};
    f->activity.idempotency_key[0] = 1U;
    f->activity.fee_limit.lo = 100U;
    f->activity.timestamp_bound.not_after = 100U;
    PAUSE_CHECK(lxp_hash_payload(f->payload, 108U,
                                 f->activity.payload_hash) == LXP_OK);
    PAUSE_CHECK(lxp_activity_signing_preimage(&f->activity, digest) == LXP_OK);
    PAUSE_CHECK(pause_sign(seed, digest, 32U, f->signature, f->public_key) == 0);
    (void)memset(&f->authority, 0, sizeof(f->authority));
    f->authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(f->authority.verified_key, f->public_key, 32U);
    (void)memcpy(f->authority.actor, actor, 32U);
    (void)memcpy(f->authority.principal, principal, 32U);
    PAUSE_CHECK(lxp_kernel_module_for_activity(&f->kernel, LX_ASSET_WITHDRAW,
                                               0U, &registration) == LXP_OK);
    PAUSE_CHECK(lxp_arena_init(&wire_arena, pause_canonical,
                               sizeof(pause_canonical)) == LXP_OK);
    PAUSE_CHECK(lxp_activity_encode(&f->activity, &wire_arena, &wire) == LXP_OK);
    PAUSE_CHECK(lxp_arena_init(&f->arena, pause_arena_bytes,
                               sizeof(pause_arena_bytes)) == LXP_OK);
    PAUSE_CHECK(lxp_effect_buffer_init(&f->effects) == LXP_OK);
    PAUSE_CHECK(lxp_module_ctx_init(&f->ctx, &f->kernel, LXP_MODULE_ASSET, 10U,
                                    0U, sequence, 100000U, &f->arena,
                                    true) == LXP_OK);
    f->ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    f->ctx.batch_number = sequence;
    PAUSE_CHECK(lxp_activity_id(wire.bytes, wire.length,
                                f->ctx.activity_id) == LXP_OK);
    (void)memcpy(activity_id, f->ctx.activity_id, 32U);
    PAUSE_CHECK(lxp_module_ctx_bind_effects(&f->ctx, &f->effects) == LXP_OK);
    *result = LXP_FATAL_INVARIANT;
    PAUSE_CHECK(lxp_kernel_dispatch(registration, &f->ctx, &f->activity,
                                    &f->authority, &f->effects,
                                    result) == LXP_OK);
    *receipt = f->ctx.ledger_receipt;
    if (*result == LXP_OK)
        PAUSE_CHECK(lxp_module_ctx_commit(&f->ctx) == LXP_OK);
    return 0;
}

static int test_withdraw_dispatch(void)
{
    static const uint8_t seed[32] = {14U};
    static const uint8_t actor_did[] = "did:key:withdrawer";
    const char *names[] = {"agent:did:key:withdrawer:main",
                           "system:paxeer-withdrawals"};
    pause_fixture *f = (pause_fixture *)calloc(1U, sizeof(*f));
    lx_account *opened[2];
    lx_withdrawal_request request;
    lxp_ledger_receipt_input receipt;
    uint8_t digest[32] = {0};
    uint8_t activity_id[32];
    uint8_t key[LX_WITHDRAWAL_STATE_KEY_BYTES];
    uint8_t value[LX_WITHDRAWAL_STATE_VALUE_BYTES];
    uint8_t before[32];
    uint8_t after[32];
    size_t index;
    lxp_result result = LXP_OK;

    PAUSE_CHECK(f != NULL);
    PAUSE_CHECK(pause_sign(seed, digest, 32U, f->signature, f->public_key) == 0);
    f->genesis_asset.asset_id[0] = 0x5cU;
    f->genesis_asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    PAUSE_CHECK(lx_asset_transfer_state(&f->genesis_asset,
                                        &f->genesis_state) == LXP_OK);
    PAUSE_CHECK(lx_account_registry_init(&f->accounts) == LXP_OK);
    for (index = 0U; index < 2U; ++index) {
        uint8_t id[32];
        PAUSE_CHECK(lx_account_id_from_string((const uint8_t *)names[index],
                                              strlen(names[index]),
                                              id) == LXP_OK);
        PAUSE_CHECK(lx_account_open(&f->accounts, (const uint8_t *)names[index],
                                    strlen(names[index]), id, 1U,
                                    index == 0U ? LX_ACCOUNT_OPEN_CREDIT :
                                                  LX_ACCOUNT_OPEN_GENESIS,
                                    NULL, &opened[index]) == LXP_OK);
        PAUSE_CHECK(lxp_ledger_bootstrap_balance(
            opened[index], f->genesis_asset.asset_id,
            (lxp_u128){0U, index == 0U ? 100U : 0U}, 0U) == LXP_OK);
    }
    (void)memcpy(opened[0]->authority_key, f->public_key, 32U);
    opened[0]->has_authority_key = true;
    f->parameters = 1U;
    PAUSE_CHECK(lxp_state_store_init(&f->state, 0U) == LXP_OK);
    PAUSE_CHECK(lxp_state_store_bind_accounts(&f->state, &f->accounts) == LXP_OK);
    PAUSE_CHECK(lxp_state_store_require_account_root(&f->state) == LXP_OK);
    PAUSE_CHECK(lxp_kernel_create(&f->kernel, &f->state, &f->journal,
                                  &f->parameters, 0U) == LXP_OK);
    PAUSE_CHECK(lxp_kernel_register_module(&f->kernel,
                                           lx_asset_module_iface()) == LXP_OK);
    PAUSE_CHECK(lxp_kernel_set_capabilities(
        &f->kernel, NULL, lxp_kernel_canonical_ledger_apply) == LXP_OK);
    f->runtime = (lx_asset_runtime){&f->accounts, &f->genesis_asset, 1U,
        &f->genesis_state, 1U, 7U, LXP_PROTOCOL_VERSION_OCCUPANCY};
    PAUSE_CHECK(lxp_kernel_bind_module_runtime(&f->kernel, LXP_MODULE_ASSET,
                                               &f->runtime) == LXP_OK);

    (void)memset(f->payload, 0, sizeof(f->payload));
    (void)memcpy(f->payload, f->genesis_asset.asset_id, 32U);
    f->payload[47] = 25U;
    f->payload[48] = 9U;
    f->payload[68] = 3U;
    f->payload[107] = 100U;

    PAUSE_CHECK(lxp_state_root(&f->kernel, before) == LXP_OK);
    PAUSE_CHECK(withdraw_submit(f, seed, actor_did, sizeof(actor_did) - 1U,
                                opened[1]->id, 1U, activity_id, &result,
                                &receipt) == 0);
    PAUSE_CHECK(result == LXP_ERR_CONTEXT_MISMATCH);
    PAUSE_CHECK(opened[0]->balance.lo == 100U && opened[1]->balance.lo == 0U &&
                opened[0]->next_sequence == 0U);
    PAUSE_CHECK(lxp_state_root(&f->kernel, after) == LXP_OK &&
                memcmp(before, after, 32U) == 0);

    PAUSE_CHECK(withdraw_submit(f, seed, actor_did, sizeof(actor_did) - 1U,
                                opened[0]->id, 1U, activity_id, &result,
                                &receipt) == 0);
    PAUSE_CHECK(result == LXP_OK);
    PAUSE_CHECK(opened[0]->balance.lo == 75U && opened[1]->balance.lo == 25U &&
                opened[0]->next_sequence == 1U);
    PAUSE_CHECK(receipt.supply_binding_version == 0U &&
                lxp_u128_is_zero(receipt.total_units_before) &&
                lxp_u128_is_zero(receipt.total_units_after));
    PAUSE_CHECK(lxp_state_root(&f->kernel, after) == LXP_OK &&
                memcmp(before, after, 32U) != 0);

    (void)memset(&request, 0, sizeof(request));
    request.network_id = 7U;
    request.amount.lo = 25U;
    (void)memcpy(request.withdrawal_id, activity_id, 32U);
    (void)memcpy(request.account_id, opened[0]->id, 32U);
    (void)memcpy(request.asset_id, f->genesis_asset.asset_id, 32U);
    request.payout_recipient[12] = 9U;
    request.checkpoint_id[0] = 3U;
    PAUSE_CHECK(lx_withdrawal_state_encode(&request, key, value) == LXP_OK);
    for (index = 0U; index < f->kernel.module_kv_count; ++index) {
        const lxp_module_kv_entry *entry = &f->kernel.module_kv[index];
        if (entry->module_id == LXP_MODULE_ASSET &&
            entry->key_length == sizeof(key) &&
            memcmp(entry->key, key, sizeof(key)) == 0) break;
    }
    PAUSE_CHECK(index < f->kernel.module_kv_count &&
                f->kernel.module_kv[index].value_length == sizeof(value) &&
                memcmp(f->kernel.module_kv[index].value, value,
                       sizeof(value)) == 0);

    PAUSE_CHECK(withdraw_submit(f, seed, actor_did, sizeof(actor_did) - 1U,
                                opened[0]->id, 2U, activity_id, &result,
                                &receipt) == 0);
    PAUSE_CHECK(result == LXP_ERR_WITHDRAWAL_ALREADY_SETTLED);
    PAUSE_CHECK(opened[0]->balance.lo == 75U && opened[1]->balance.lo == 25U &&
                opened[0]->next_sequence == 1U);
    PAUSE_CHECK(lxp_state_root(&f->kernel, before) == LXP_OK &&
                memcmp(before, after, 32U) == 0);

    PAUSE_CHECK(lxp_state_store_destroy(&f->state) == LXP_OK);
    free(f);
    return 0;
}

#undef PAUSE_CHECK

int main(void)
{
    lx_asset_registry registry;
    lx_asset_record record;
    lx_asset_record decoded;
    lx_asset_record *found;
    uint8_t encoded[256];
    size_t encoded_length;
    lxp_u128 amount;
    lxp_transfer_asset_state asset_state;
    lx_account_registry accounts;
    lx_account *from;
    lx_account *to;
    const char *from_name = "agent:did:key:a:main";
    const char *to_name = "agent:did:key:b:main";
    uint8_t from_id[32];
    uint8_t to_id[32];
    lxp_transfer_leg leg;
    lxp_transfer_context context;
    lxp_transfer_result transfer_result;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    uint64_t parameters = 1U;
    const lxp_module_iface *iface = lx_asset_module_iface();
    const lxp_module_registration *registration;

    (void)memset(&record, 0, sizeof(record));
    record.asset_id[0] = 1U;
    (void)memcpy(record.symbol, "USDC", 5U);
    record.symbol_length = 4U;
    record.decimals = 6U;
    (void)memcpy(record.name, "USD Coin", 8U);
    record.name_length = 8U;
    record.issuer_kind = 2U;
    record.issuer_did32[0] = 1U;
    record.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    (void)memcpy(record.custody_reference, "paxeer:usdc", 11U);
    record.custody_reference_length = 11U;
    if (lx_asset_registry_init(&registry, 4U) != LXP_OK ||
        lx_asset_register(&registry, &record, 4U, (lxp_u128){ 0U, 3U }) !=
            LXP_OK || registry.next_sequence != 5U ||
        registry.fees_charged.lo != 3U ||
        lx_asset_register(&registry, &record, 5U, (lxp_u128){ 0U, 3U }) !=
            LXP_ERR_ASSET_ALREADY_REGISTERED || registry.next_sequence != 6U ||
        registry.fees_charged.lo != 6U || registry.count != 1U ||
        lx_asset_lookup(&registry, record.asset_id, &found) != LXP_OK ||
        found->decimals != 6U ||
        lx_asset_record_encode(found, encoded, sizeof(encoded), &encoded_length) !=
            LXP_OK || lx_asset_record_decode(encoded, encoded_length, &decoded) !=
            LXP_OK || memcmp(decoded.asset_id, record.asset_id, 32U) != 0 ||
        strcmp(decoded.symbol, "USDC") != 0 ||
        decoded.custody_reference_length != 11U) return 1;
    encoded[encoded_length - 49U] = 0U;
    if (lx_asset_record_decode(encoded, encoded_length, &decoded) !=
        LXP_ERR_NON_CANONICAL) return 1;
    encoded[encoded_length - 49U] = 2U;
    record.issuer_kind = 0U;
    if (lx_asset_record_encode(&record, encoded, sizeof(encoded),
                               &encoded_length) != LXP_ERR_NON_CANONICAL)
        return 1;
    record.issuer_kind = 2U;
    if (lx_asset_amount_decode((const uint8_t *)"1000000", 7U, &amount) !=
            LXP_OK || amount.lo != 1000000U || amount.hi != 0U ||
        lx_asset_amount_decode((const uint8_t *)"1.0", 3U, &amount) !=
            LXP_ERR_INVALID_AMOUNT ||
        lx_asset_amount_decode((const uint8_t *)"1e6", 3U, &amount) !=
            LXP_ERR_INVALID_AMOUNT ||
        lx_asset_amount_decode((const uint8_t *)"01", 2U, &amount) !=
            LXP_ERR_INVALID_AMOUNT) return 1;
    if (iface == NULL || iface->module_id != LXP_MODULE_ASSET ||
        iface->activity_type_count != 11U ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, iface) != LXP_OK ||
        lxp_kernel_module_for_activity(&kernel, LX_ASSET_GRANT_REVOKE, 0U,
                                       &registration) != LXP_OK ||
        lxp_kernel_module_for_activity(&kernel, LX_ASSET_WITHDRAW, 0U,
                                       &registration) != LXP_OK ||
        registration->activity_type_count != 11U) return 1;
    if (test_asset_payloads(&kernel) != 0) return 1;
    if (lx_account_registry_init(&accounts) != LXP_OK ||
        lx_account_id_from_string((const uint8_t *)from_name, strlen(from_name),
                                  from_id) != LXP_OK ||
        lx_account_id_from_string((const uint8_t *)to_name, strlen(to_name),
                                  to_id) != LXP_OK ||
        lx_account_open(&accounts, (const uint8_t *)from_name, strlen(from_name),
                        from_id, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &from) != LXP_OK ||
        lx_account_open(&accounts, (const uint8_t *)to_name, strlen(to_name),
                        to_id, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &to) != LXP_OK ||
        lxp_ledger_bootstrap_balance(from, record.asset_id,
                                     (lxp_u128){ 0U, 10U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(to, record.asset_id,
                                     (lxp_u128){ 0U, 0U }, 0U) != LXP_OK ||
        lx_asset_pause(&registry, record.asset_id) != LXP_OK ||
        lx_asset_transfer_state(found, &asset_state) != LXP_OK) return 1;
    (void)memset(&leg, 0, sizeof(leg));
    leg.from = from;
    leg.to = to;
    (void)memcpy(leg.asset_id, record.asset_id, 32U);
    leg.amount = (lxp_u128){ 0U, 1U };
    (void)memset(&context, 0, sizeof(context));
    context.assets = &asset_state;
    context.asset_count = 1U;
    (void)memcpy(context.authorized_from, from_id, 32U);
    if (lxp_apply_transfer(&leg, &context, &transfer_result) !=
        LXP_ERR_ASSET_PAUSED) return 1;
    if (lx_asset_unpause(&registry, record.asset_id) != LXP_OK ||
        lxp_state_store_destroy(&state) != LXP_OK) return 1;
    if (test_pause_dispatch() != 0) return 1;
    if (test_supply_binding_dispatch() != 0) return 1;
    if (test_withdraw_dispatch() != 0) return 1;
    return 0;
}
