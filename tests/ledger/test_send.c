#include "layerx/lxp_crypto.h"
#include "layerx/lxp_fee.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_transfer.h"

#include <openssl/evp.h>
#include <string.h>

int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size);

static int sign_send(lxp_send *send, const uint8_t seed[32],
                     uint8_t public_key[32])
{
    uint8_t message[512];
    uint8_t digest[32];
    size_t message_length;
    size_t public_length = 32U;
    size_t signature_length = 64U;
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                  seed, 32U);
    EVP_MD_CTX *context;
    if (key == NULL ||
        EVP_PKEY_get_raw_public_key(key, public_key, &public_length) != 1 ||
        public_length != 32U) return 1;
    (void)memcpy(send->authorization.public_key, public_key, 32U);
    if (lxp_send_authorization_message(send, message, sizeof(message),
                                       &message_length) != LXP_OK ||
        lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, message, message_length,
                        digest) != LXP_OK) return 1;
    context = EVP_MD_CTX_new();
    if (context == NULL ||
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) != 1 ||
        EVP_DigestSign(context, send->authorization.signature,
                       &signature_length, digest, sizeof(digest)) != 1 ||
        signature_length != 64U) return 1;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return 0;
}

int main(void)
{
    static const uint8_t seed[32] = {
        0x9dU,0x61U,0xb1U,0x9dU,0xefU,0xfdU,0x5aU,0x60U,
        0xbaU,0x84U,0x4aU,0xf4U,0x92U,0xecU,0x2cU,0xc4U,
        0x44U,0x49U,0xc5U,0x69U,0x7bU,0x32U,0x69U,0x19U,
        0x70U,0x3bU,0xacU,0x03U,0x1cU,0xaeU,0x7fU,0x60U
    };
    lx_account_registry registry;
    lx_account *from;
    lx_account *to;
    const char *from_name = "agent:did:key:alice:main";
    const char *to_name = "agent:did:key:bob:main";
    uint8_t asset_id[32] = { 3U };
    uint8_t public_key[32];
    lxp_send send;
    lxp_send decoded;
    uint8_t encoded[512];
    size_t encoded_length;
    lxp_transfer_asset_state asset;
    lxp_send_store store;
    lxp_send_environment environment;
    lxp_send_receipt_projection receipt;
    lxp_send_receipt_projection first;

    if (lx_account_registry_init(&registry) != LXP_OK ||
        lx_account_id_from_string((const uint8_t *)from_name, strlen(from_name),
                                  send.from) != LXP_OK ||
        lx_account_id_from_string((const uint8_t *)to_name, strlen(to_name),
                                  send.to) != LXP_OK ||
        lx_account_open(&registry, (const uint8_t *)from_name, strlen(from_name),
                        send.from, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &from) !=
            LXP_OK ||
        lx_account_open(&registry, (const uint8_t *)to_name, strlen(to_name),
                        send.to, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &to) != LXP_OK ||
        lxp_ledger_bootstrap_balance(from, asset_id, (lxp_u128){ 0U, 100U },
                                     0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(to, asset_id, (lxp_u128){ 0U, 0U }, 0U) !=
            LXP_OK) return 1;
    (void)memset(&send, 0, sizeof(send));
    (void)memcpy(send.from, from->id, 32U);
    (void)memcpy(send.to, to->id, 32U);
    (void)memcpy(send.asset, asset_id, 32U);
    send.amount = (lxp_u128){ 0U, 25U };
    send.sequence = 0U;
    send.idempotency_key[0] = 9U;
    send.expires_at = 20U;
    send.context_hash[0] = 5U;
    send.condition_count = 2U;
    send.conditions[0] = (lxp_send_condition){ LXP_CONDITION_NOT_BEFORE, 5U };
    send.conditions[1] = (lxp_send_condition){ LXP_CONDITION_NOT_AFTER, 15U };
    send.authorization.kind = LXP_AUTH_OWNER;
    (void)memcpy(send.authorization.controller, send.from, 32U);
    (void)memcpy(send.authorization.signed_context_hash, send.context_hash, 32U);
    send.authorization.network_id = 7U;
    send.authorization.protocol_version = LXP_PROTOCOL_VERSION;
    if (sign_send(&send, seed, public_key) != 0) return 1;
    (void)memcpy(from->authority_key, public_key, 32U);
    from->has_authority_key = true;
    if (lxp_send_encode(&send, encoded, sizeof(encoded), &encoded_length) !=
            LXP_OK ||
        lxp_send_decode(encoded, encoded_length, &decoded) != LXP_OK ||
        memcmp(&send, &decoded, sizeof(send)) != 0 ||
        lxp_send_decode(encoded, encoded_length - 1U, &decoded) !=
            LXP_ERR_MALFORMED_SEND ||
        lxp_send_decode(encoded, encoded_length + 1U, &decoded) !=
            LXP_ERR_MALFORMED_SEND ||
        LLVMFuzzerTestOneInput(encoded, encoded_length) != 0) return 1;
    (void)memset(&asset, 0, sizeof(asset));
    (void)memcpy(asset.asset_id, asset_id, 32U);
    asset.registered = true;
    (void)memset(&store, 0, sizeof(store));
    environment = (lxp_send_environment){ &registry, &asset, 1U, &store,
                                          10U, 7U, LXP_PROTOCOL_VERSION };
    if (lxp_send_execute(&send, &environment, &receipt) != LXP_OK ||
        from->balance.lo != 75U || to->balance.lo != 25U ||
        from->next_sequence != 1U) return 1;
    first = receipt;
    if (lxp_send_execute(&send, &environment, &receipt) !=
            LXP_ERR_SEQUENCE_REUSED || from->balance.lo != 75U ||
        to->balance.lo != 25U) return 1;
    send.sequence = 1U;
    send.amount = (lxp_u128){ 0U, 10U };
    if (sign_send(&send, seed, public_key) != 0 ||
        lxp_send_execute(&send, &environment, &receipt) !=
            LXP_ERR_IDEMPOTENT_REPLAY || !receipt.replayed ||
        memcmp(receipt.transfer_set_root, first.transfer_set_root, 32U) != 0 ||
        from->balance.lo != 75U || to->balance.lo != 25U) return 1;
    send.idempotency_key[0] = 10U;
    send.authorization.kind = 7U;
    if (lxp_send_validate(&send, &environment) !=
        LXP_ERR_UNKNOWN_AUTHORITY_KIND) return 1;
    send.authorization.kind = LXP_AUTH_OWNER;
    send.context_hash[0] ^= 1U;
    if (lxp_send_validate(&send, &environment) != LXP_ERR_CONTEXT_MISMATCH)
        return 1;
    send.context_hash[0] ^= 1U;
    if (lxp_ledger_bootstrap_balance(from, asset_id, (lxp_u128){ 0U, 75U },
                                     UINT64_MAX) != LXP_OK) return 1;
    send.sequence = UINT64_MAX;
    send.idempotency_key[0] = 11U;
    if (sign_send(&send, seed, public_key) != 0 ||
        lxp_send_validate(&send, &environment) != LXP_ERR_SEQUENCE_EXHAUSTED ||
        lxp_send_execute(&send, &environment, &receipt) !=
            LXP_ERR_SEQUENCE_EXHAUSTED ||
        from->balance.lo != 75U || to->balance.lo != 25U ||
        from->next_sequence != UINT64_MAX || store.count != 1U) return 1;
    if (lxp_ledger_bootstrap_balance(from, asset_id, (lxp_u128){ 0U, 75U },
                                     1U) != LXP_OK) return 1;
    send.sequence = 1U;
    send.idempotency_key[0] = 12U;
    if (sign_send(&send, seed, public_key) != 0 ||
        lxp_send_execute(&send, &environment, &receipt) != LXP_OK ||
        from->balance.lo != 65U || to->balance.lo != 35U ||
        from->next_sequence != 2U) return 1;
    {
        const size_t window = (size_t)LXP_SEND_STORE_CAPACITY;
        const size_t beyond = window + 3U;
        const uint64_t window_bytes =
            (uint64_t)window * (uint64_t)LXP_SEND_HISTORY_RECORD_BYTES;
        lxp_send_store spilling;
        lxp_send_environment spilling_environment;
        lxp_meter_ctx meter;
        lxp_send_receipt_projection oldest;
        lxp_send_receipt_projection replay;
        lxp_send oldest_send;
        size_t total = 0U;
        size_t index;

        if (lxp_send_store_init(&spilling, &meter) != LXP_OK ||
            lxp_meter_init(&meter, UINT64_MAX, UINT64_MAX,
                           (lxp_u128){ 0U, 1U }, (lxp_u128){ 0U, UINT64_MAX },
                           1U, true) != LXP_OK ||
            lxp_ledger_bootstrap_balance(from, asset_id,
                                         (lxp_u128){ 0U, 1000U }, 0U) !=
                LXP_OK ||
            lxp_ledger_bootstrap_balance(to, asset_id, (lxp_u128){ 0U, 0U },
                                         0U) != LXP_OK) return 1;
        spilling_environment = (lxp_send_environment){ &registry, &asset, 1U,
            &spilling, 10U, 7U, LXP_PROTOCOL_VERSION };
        (void)memset(&oldest, 0, sizeof(oldest));
        (void)memset(&oldest_send, 0, sizeof(oldest_send));
        send.amount = (lxp_u128){ 0U, 1U };
        for (index = 0U; index < beyond; ++index) {
            send.sequence = (uint64_t)index;
            send.idempotency_key[0] = 0U;
            send.idempotency_key[1] = (uint8_t)(index + 1U);
            if (sign_send(&send, seed, public_key) != 0 ||
                lxp_send_execute(&send, &spilling_environment, &receipt) !=
                    LXP_OK) return 1;
            if (index == 0U) {
                oldest = receipt;
                oldest_send = send;
            }
        }
        if (lxp_send_store_total(&spilling, &total) != LXP_OK ||
            total != beyond || spilling.count != beyond - window ||
            spilling.history == NULL || spilling.history->count != window ||
            spilling.history->stored_bytes != window_bytes ||
            meter.net_storage_bytes != window_bytes ||
            from->balance.lo != 1000U - beyond || to->balance.lo != beyond ||
            from->next_sequence != beyond) return 1;
        if (lxp_send_execute(&oldest_send, &spilling_environment, &receipt) !=
                LXP_ERR_SEQUENCE_REUSED ||
            from->balance.lo != 1000U - beyond) return 1;
        send.sequence = (uint64_t)beyond;
        send.amount = (lxp_u128){ 0U, 7U };
        send.idempotency_key[0] = 0U;
        send.idempotency_key[1] = 1U;
        oldest.replayed = true;
        if (sign_send(&send, seed, public_key) != 0 ||
            lxp_send_execute(&send, &spilling_environment, &replay) !=
                LXP_ERR_IDEMPOTENT_REPLAY ||
            memcmp(&replay, &oldest, sizeof(replay)) != 0 ||
            from->balance.lo != 1000U - beyond || to->balance.lo != beyond ||
            from->next_sequence != beyond) return 1;
        lxp_send_store_release(&spilling);
        if (spilling.count != 0U || spilling.history != NULL) return 1;
    }
    {
        const size_t window = (size_t)LXP_SEND_STORE_CAPACITY;
        const uint64_t window_bytes =
            (uint64_t)window * (uint64_t)LXP_SEND_HISTORY_RECORD_BYTES;
        lxp_send_store metered;
        lxp_send_environment metered_environment;
        lxp_meter_ctx meter;
        size_t spilled = 0U;
        size_t index;

        if (lxp_send_store_init(&metered, &meter) != LXP_OK ||
            lxp_meter_init(&meter, UINT64_MAX, window_bytes - 1U,
                           (lxp_u128){ 0U, 1U }, (lxp_u128){ 0U, UINT64_MAX },
                           1U, true) != LXP_OK ||
            lxp_ledger_bootstrap_balance(from, asset_id,
                                         (lxp_u128){ 0U, 1000U }, 0U) !=
                LXP_OK ||
            lxp_ledger_bootstrap_balance(to, asset_id, (lxp_u128){ 0U, 0U },
                                         0U) != LXP_OK) return 1;
        metered_environment = (lxp_send_environment){ &registry, &asset, 1U,
            &metered, 10U, 7U, LXP_PROTOCOL_VERSION };
        send.amount = (lxp_u128){ 0U, 1U };
        for (index = 0U; index < window; ++index) {
            send.sequence = (uint64_t)index;
            send.idempotency_key[0] = 1U;
            send.idempotency_key[1] = (uint8_t)(index + 1U);
            if (sign_send(&send, seed, public_key) != 0 ||
                lxp_send_execute(&send, &metered_environment, &receipt) !=
                    LXP_OK) return 1;
        }
        send.sequence = (uint64_t)window;
        send.idempotency_key[0] = 2U;
        send.idempotency_key[1] = 1U;
        if (sign_send(&send, seed, public_key) != 0 ||
            lxp_send_execute(&send, &metered_environment, &receipt) !=
                LXP_ERR_GAS_EXHAUSTED ||
            metered.count != window || metered.history == NULL ||
            metered.history->count != 0U ||
            metered.history->stored_bytes != 0U ||
            lxp_send_store_total(&metered, &spilled) != LXP_OK ||
            spilled != window ||
            from->balance.lo != 1000U - window || to->balance.lo != window ||
            from->next_sequence != window) return 1;
        lxp_send_store_release(&metered);
    }
    {
        const size_t window = (size_t)LXP_SEND_STORE_CAPACITY;
        lxp_send_store unmetered;
        lxp_send_environment unmetered_environment;
        size_t total = 0U;
        size_t index;

        if (lxp_send_store_init(&unmetered, NULL) != LXP_OK ||
            lxp_ledger_bootstrap_balance(from, asset_id,
                                         (lxp_u128){ 0U, 1000U }, 0U) !=
                LXP_OK ||
            lxp_ledger_bootstrap_balance(to, asset_id, (lxp_u128){ 0U, 0U },
                                         0U) != LXP_OK) return 1;
        unmetered_environment = (lxp_send_environment){ &registry, &asset, 1U,
            &unmetered, 10U, 7U, LXP_PROTOCOL_VERSION };
        send.amount = (lxp_u128){ 0U, 1U };
        for (index = 0U; index < window; ++index) {
            send.sequence = (uint64_t)index;
            send.idempotency_key[0] = 3U;
            send.idempotency_key[1] = (uint8_t)(index + 1U);
            if (sign_send(&send, seed, public_key) != 0 ||
                lxp_send_execute(&send, &unmetered_environment, &receipt) !=
                    LXP_OK) return 1;
        }
        send.sequence = (uint64_t)window;
        send.idempotency_key[0] = 4U;
        send.idempotency_key[1] = 1U;
        if (sign_send(&send, seed, public_key) != 0 ||
            lxp_send_execute(&send, &unmetered_environment, &receipt) !=
                LXP_ERR_ARENA_EXHAUSTED ||
            lxp_send_store_total(&unmetered, &total) != LXP_OK ||
            total != window || unmetered.count != window ||
            unmetered.history != NULL ||
            from->balance.lo != 1000U - window || to->balance.lo != window ||
            from->next_sequence != window) return 1;
        lxp_send_store_release(&unmetered);
    }
    return 0;
}
