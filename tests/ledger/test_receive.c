#include "layerx/lxp_fee.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_transfer.h"

#include <openssl/evp.h>
#include <string.h>

static int public_from_seed(const uint8_t seed[32], uint8_t public_key[32])
{
    size_t length = 32U;
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                  seed, 32U);
    int result = key == NULL ||
        EVP_PKEY_get_raw_public_key(key, public_key, &length) != 1 || length != 32U;
    EVP_PKEY_free(key);
    return result;
}

static int sign_domain(const uint8_t seed[32], lxp_domain_tag_id domain,
                       const uint8_t *message, size_t message_length,
                       uint8_t signature[64])
{
    uint8_t digest[32];
    size_t signature_length = 64U;
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                  seed, 32U);
    EVP_MD_CTX *context;
    if (key == NULL || lxp_hash_domain(domain, message, message_length,
                                       digest) != LXP_OK) return 1;
    context = EVP_MD_CTX_new();
    if (context == NULL ||
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) != 1 ||
        EVP_DigestSign(context, signature, &signature_length, digest,
                       sizeof(digest)) != 1 || signature_length != 64U) return 1;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return 0;
}

static int sign_grant(lxp_payer_grant *grant, const uint8_t seed[32])
{
    uint8_t message[384];
    size_t length;
    if (lxp_grant_authorization_message(grant, message, sizeof(message), &length) !=
            LXP_OK || lxp_hash_authority(message, length, grant->grant_id) != LXP_OK)
        return 1;
    return sign_domain(seed, LXP_DOMAIN_AUTHORITY_HASH, message, length,
                       grant->signature);
}

static int sign_receive(lxp_receive *receive, const uint8_t seed[32])
{
    uint8_t message[512];
    size_t length;
    if (lxp_receive_authorization_message(receive, message, sizeof(message),
                                          &length) != LXP_OK) return 1;
    return sign_domain(seed, LXP_DOMAIN_SIGNATURE_PREIMAGE, message, length,
                       receive->receiver_authorization.signature);
}

int main(void)
{
    static const uint8_t payer_seed[32] = { 1U };
    static const uint8_t receiver_seed[32] = { 2U };
    lx_account_registry accounts;
    lx_account *from;
    lx_account *to;
    const char *from_name = "agent:did:key:payer:main";
    const char *to_name = "agent:did:key:merchant:main";
    uint8_t asset_id[32] = { 4U };
    uint8_t purpose_preimage[64];
    lxp_payer_grant grant;
    lxp_receive receive;
    lxp_receive decoded;
    uint8_t encoded[1024];
    size_t encoded_length;
    lxp_grant_store grants;
    lxp_send_store idempotency;
    lxp_transfer_asset_state asset;
    lxp_receive_environment environment;
    lxp_send_receipt_projection receipt;
    lxp_grant_state recurring;

    (void)memset(&receive, 0, sizeof(receive));
    if (lx_account_registry_init(&accounts) != LXP_OK ||
        lx_account_id_from_string((const uint8_t *)from_name, strlen(from_name),
                                  receive.from) != LXP_OK ||
        lx_account_id_from_string((const uint8_t *)to_name, strlen(to_name),
                                  receive.to) != LXP_OK ||
        lx_account_open(&accounts, (const uint8_t *)from_name, strlen(from_name),
                        receive.from, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &from) !=
            LXP_OK ||
        lx_account_open(&accounts, (const uint8_t *)to_name, strlen(to_name),
                        receive.to, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &to) != LXP_OK ||
        lxp_ledger_bootstrap_balance(from, asset_id, (lxp_u128){ 0U, 100U },
                                     0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(to, asset_id, (lxp_u128){ 0U, 0U }, 0U) !=
            LXP_OK ||
        public_from_seed(payer_seed, from->authority_key) != 0 ||
        public_from_seed(receiver_seed, to->authority_key) != 0) return 1;
    from->has_authority_key = true;
    to->has_authority_key = true;
    (void)memset(&grant, 0, sizeof(grant));
    (void)memcpy(grant.from, from->id, 32U);
    (void)memcpy(grant.recipient, to->id, 32U);
    (void)memcpy(grant.asset, asset_id, 32U);
    grant.per_draw_maximum = (lxp_u128){ 0U, 30U };
    grant.allowance = (lxp_u128){ 0U, 50U };
    grant.expiration = 100U;
    grant.purpose_hash[0] = 8U;
    grant.has_reference = true;
    grant.reference_hash[0] = 9U;
    grant.revocation_sequence = 5U;
    (void)memcpy(grant.public_key, from->authority_key, 32U);
    if (sign_grant(&grant, payer_seed) != 0) return 1;
    (void)memcpy(receive.asset, asset_id, 32U);
    receive.amount = (lxp_u128){ 0U, 30U };
    (void)memcpy(receive.grant_id, grant.grant_id, 32U);
    receive.idempotency_key[0] = 1U;
    (void)memcpy(purpose_preimage, grant.purpose_hash, 32U);
    (void)memcpy(purpose_preimage + 32U, grant.reference_hash, 32U);
    if (lxp_hash_context_value(purpose_preimage, sizeof(purpose_preimage),
                               receive.context_hash) != LXP_OK) return 1;
    receive.receiver_authorization.kind = LXP_AUTH_OWNER;
    (void)memcpy(receive.receiver_authorization.controller, to->id, 32U);
    (void)memcpy(receive.receiver_authorization.public_key, to->authority_key, 32U);
    (void)memcpy(receive.receiver_authorization.signed_context_hash,
                 receive.context_hash, 32U);
    receive.receiver_authorization.network_id = 7U;
    receive.receiver_authorization.protocol_version = LXP_PROTOCOL_VERSION;
    receive.payer_grant = grant;
    if (sign_receive(&receive, receiver_seed) != 0 ||
        lxp_receive_encode(&receive, encoded, sizeof(encoded), &encoded_length) !=
            LXP_OK ||
        lxp_receive_decode(encoded, encoded_length, &decoded) != LXP_OK ||
        memcmp(&receive, &decoded, sizeof(receive)) != 0 ||
        lxp_receive_decode(encoded, encoded_length - 1U, &decoded) !=
            LXP_ERR_MALFORMED_RECEIVE) return 1;
    (void)memset(&grants, 0, sizeof(grants));
    (void)memset(&idempotency, 0, sizeof(idempotency));
    (void)memset(&asset, 0, sizeof(asset));
    (void)memcpy(asset.asset_id, asset_id, 32U);
    asset.registered = true;
    environment = (lxp_receive_environment){ &accounts, &asset, 1U, &grants,
        &idempotency, 10U, 1U, 7U, LXP_PROTOCOL_VERSION };
    if (lxp_receive_execute(&receive, &environment, &receipt) !=
        LXP_ERR_NO_PAYER_GRANT) return 1;
    if (lxp_grant_store_put(&grants, &grant, from) != LXP_OK ||
        lxp_receive_execute(&receive, &environment, &receipt) != LXP_OK ||
        from->balance.lo != 70U || to->balance.lo != 30U ||
        to->next_sequence != 1U || grants.grants[0].drawn_total.lo != 30U ||
        !grants.grants[0].invoice_settled) return 1;
    receive.receiver_sequence = 1U;
    receive.idempotency_key[0] = 2U;
    if (sign_receive(&receive, receiver_seed) != 0 ||
        lxp_receive_execute(&receive, &environment, &receipt) !=
            LXP_ERR_INVOICE_ALREADY_SETTLED || from->balance.lo != 70U)
        return 1;
    receive.to[0] ^= 1U;
    if (lxp_receive_execute(&receive, &environment, &receipt) !=
        LXP_ERR_GRANT_SCOPE_VIOLATION) return 1;
    receive.to[0] ^= 1U;
    if (lxp_grant_revoke(&grants, grant.grant_id, 4U) !=
            LXP_ERR_STALE_REVOCATION ||
        lxp_grant_revoke(&grants, grant.grant_id, 5U) != LXP_OK) return 1;
    environment.global_sequence = 5U;
    if (lxp_receive_execute(&receive, &environment, &receipt) !=
        LXP_ERR_GRANT_REVOKED) return 1;
    (void)memset(&recurring, 0, sizeof(recurring));
    recurring.grant.recurring = true;
    recurring.grant.window_length = 10U;
    recurring.grant.per_draw_maximum = (lxp_u128){ 0U, 30U };
    recurring.grant.allowance = (lxp_u128){ 0U, 50U };
    if (lxp_grant_draw_record(&recurring, (lxp_u128){ 0U, 30U }, 12U) !=
            LXP_OK ||
        lxp_grant_draw_record(&recurring, (lxp_u128){ 0U, 21U }, 13U) !=
            LXP_ERR_GRANT_EXHAUSTED ||
        lxp_grant_draw_record(&recurring, (lxp_u128){ 0U, 30U }, 20U) !=
            LXP_OK || recurring.drawn_total.lo != 60U ||
        recurring.drawn_this_period.lo != 30U || recurring.window_start != 20U)
        return 1;
    grants.grants[0].invoice_settled = false;
    grants.grants[0].revoked = false;
    environment.global_sequence = 1U;
    if (lxp_ledger_bootstrap_balance(to, asset_id, (lxp_u128){ 0U, 30U },
                                     UINT64_MAX) != LXP_OK) return 1;
    receive.receiver_sequence = UINT64_MAX;
    receive.idempotency_key[0] = 3U;
    if (sign_receive(&receive, receiver_seed) != 0 ||
        lxp_receive_execute(&receive, &environment, &receipt) !=
            LXP_ERR_SEQUENCE_EXHAUSTED || from->balance.lo != 70U ||
        to->balance.lo != 30U || to->next_sequence != UINT64_MAX ||
        grants.grants[0].drawn_total.lo != 30U ||
        grants.grants[0].invoice_settled) return 1;
    if (lxp_ledger_bootstrap_balance(to, asset_id, (lxp_u128){ 0U, 30U }, 1U) !=
        LXP_OK) return 1;
    receive.receiver_sequence = 1U;
    receive.amount = (lxp_u128){ 0U, 20U };
    receive.idempotency_key[0] = 4U;
    if (sign_receive(&receive, receiver_seed) != 0 ||
        lxp_receive_execute(&receive, &environment, &receipt) != LXP_OK ||
        from->balance.lo != 50U || to->balance.lo != 50U ||
        to->next_sequence != 2U || grants.grants[0].drawn_total.lo != 50U ||
        !grants.grants[0].invoice_settled) return 1;
    {
        const size_t window = (size_t)LXP_SEND_STORE_CAPACITY;
        const size_t beyond = window + 2U;
        const uint64_t window_bytes =
            (uint64_t)window * (uint64_t)LXP_SEND_HISTORY_RECORD_BYTES;
        lxp_payer_grant bulk_grant;
        lxp_receive bulk;
        lxp_receive oldest_receive;
        lxp_send_store spilling;
        lxp_receive_environment spilling_environment;
        lxp_meter_ctx meter;
        lxp_send_receipt_projection oldest;
        lxp_send_receipt_projection replay;
        size_t total = 0U;
        size_t index;

        (void)memset(&bulk_grant, 0, sizeof(bulk_grant));
        (void)memcpy(bulk_grant.from, from->id, 32U);
        (void)memcpy(bulk_grant.recipient, to->id, 32U);
        (void)memcpy(bulk_grant.asset, asset_id, 32U);
        bulk_grant.per_draw_maximum = (lxp_u128){ 0U, 1U };
        bulk_grant.allowance = (lxp_u128){ 0U, 200U };
        bulk_grant.expiration = 100U;
        bulk_grant.purpose_hash[0] = 21U;
        bulk_grant.revocation_sequence = 50U;
        (void)memcpy(bulk_grant.public_key, from->authority_key, 32U);
        if (sign_grant(&bulk_grant, payer_seed) != 0 ||
            lxp_grant_store_put(&grants, &bulk_grant, from) != LXP_OK ||
            lxp_send_store_init(&spilling, &meter) != LXP_OK ||
            lxp_meter_init(&meter, UINT64_MAX, UINT64_MAX,
                           (lxp_u128){ 0U, 1U }, (lxp_u128){ 0U, UINT64_MAX },
                           1U, true) != LXP_OK ||
            lxp_ledger_bootstrap_balance(from, asset_id,
                                         (lxp_u128){ 0U, 200U }, 0U) !=
                LXP_OK ||
            lxp_ledger_bootstrap_balance(to, asset_id, (lxp_u128){ 0U, 0U },
                                         0U) != LXP_OK) return 1;
        bulk = receive;
        (void)memcpy(bulk.grant_id, bulk_grant.grant_id, 32U);
        bulk.payer_grant = bulk_grant;
        bulk.amount = (lxp_u128){ 0U, 1U };
        if (lxp_hash_context_value(bulk_grant.purpose_hash, 32U,
                                   bulk.context_hash) != LXP_OK) return 1;
        (void)memcpy(bulk.receiver_authorization.signed_context_hash,
                     bulk.context_hash, 32U);
        spilling_environment = (lxp_receive_environment){ &accounts, &asset,
            1U, &grants, &spilling, 10U, 1U, 7U, LXP_PROTOCOL_VERSION };
        (void)memset(&oldest, 0, sizeof(oldest));
        (void)memset(&oldest_receive, 0, sizeof(oldest_receive));
        for (index = 0U; index < beyond; ++index) {
            bulk.receiver_sequence = (uint64_t)index;
            bulk.idempotency_key[0] = 0U;
            bulk.idempotency_key[1] = (uint8_t)(index + 1U);
            if (sign_receive(&bulk, receiver_seed) != 0 ||
                lxp_receive_execute(&bulk, &spilling_environment, &receipt) !=
                    LXP_OK) return 1;
            if (index == 0U) {
                oldest = receipt;
                oldest_receive = bulk;
            }
        }
        if (lxp_send_store_total(&spilling, &total) != LXP_OK ||
            total != beyond || spilling.count != beyond - window ||
            spilling.history == NULL || spilling.history->count != window ||
            spilling.history->stored_bytes != window_bytes ||
            meter.net_storage_bytes != window_bytes ||
            from->balance.lo != 200U - beyond || to->balance.lo != beyond ||
            to->next_sequence != beyond) return 1;
        if (lxp_receive_execute(&oldest_receive, &spilling_environment,
                                &receipt) != LXP_ERR_SEQUENCE_REUSED ||
            from->balance.lo != 200U - beyond) return 1;
        bulk.receiver_sequence = (uint64_t)beyond;
        bulk.idempotency_key[0] = 0U;
        bulk.idempotency_key[1] = 1U;
        oldest.replayed = true;
        if (sign_receive(&bulk, receiver_seed) != 0 ||
            lxp_receive_execute(&bulk, &spilling_environment, &replay) !=
                LXP_ERR_IDEMPOTENT_REPLAY ||
            memcmp(&replay, &oldest, sizeof(replay)) != 0 ||
            from->balance.lo != 200U - beyond || to->balance.lo != beyond ||
            to->next_sequence != beyond) return 1;
        lxp_send_store_release(&spilling);
        if (spilling.count != 0U || spilling.history != NULL) return 1;
    }
    return 0;
}
