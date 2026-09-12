#include "layerx/lxp_authority.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_fee.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_transfer.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <string.h>

#define CHECK(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "send allowance check failed at line %d\n", __LINE__); \
    return 1; } } while (0)

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

static int sign_send(lxp_send *send, const uint8_t seed[32])
{
    uint8_t message[512];
    size_t length;
    if (public_from_seed(seed, send->authorization.public_key) != 0 ||
        lxp_send_authorization_message(send, message, sizeof(message),
                                       &length) != LXP_OK) return 1;
    return sign_domain(seed, LXP_DOMAIN_SIGNATURE_PREIMAGE, message, length,
                       send->authorization.signature);
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

static int open_account(lx_account_registry *registry, const char *name,
                        const uint8_t asset_id[32], uint64_t balance,
                        const uint8_t seed[32], lx_account **account)
{
    uint8_t id[32];
    size_t length = strlen(name);
    CHECK(lx_account_id_from_string((const uint8_t *)name, length, id) ==
          LXP_OK);
    CHECK(lx_account_open(registry, (const uint8_t *)name, length, id, 1U,
                          LX_ACCOUNT_OPEN_CREDIT, NULL, account) == LXP_OK);
    CHECK(lxp_ledger_bootstrap_balance(*account, asset_id,
                                       (lxp_u128){ 0U, balance }, 0U) == LXP_OK);
    CHECK(public_from_seed(seed, (*account)->authority_key) == 0);
    (*account)->has_authority_key = true;
    return 0;
}

/* A delegated capability over one asset: at most 50 per activity and 70 in
 * total. The standalone send and receive entries present origin module 0, so
 * the scope has to admit that module for the debit to bind. */
static void metered_scope(lxp_authority_scope *scope, const uint8_t asset_id[32])
{
    (void)memset(scope, 0, sizeof(*scope));
    scope->module_mask = UINT64_C(1);
    scope->activity_ordinal_min = 1U;
    scope->activity_ordinal_max = 7U;
    (void)memcpy(scope->asset_id, asset_id, 32U);
    scope->maximum_per_activity = (lxp_u128){ 0U, 50U };
    scope->maximum_total = (lxp_u128){ 0U, 70U };
    scope->purpose_hash[0] = 0x77U;
}

static int build_send(lxp_send *send, const lx_account *from,
                      const lx_account *to, const uint8_t asset_id[32],
                      uint64_t amount, uint64_t sequence, uint8_t marker,
                      uint8_t kind, const uint8_t seed[32])
{
    (void)memset(send, 0, sizeof(*send));
    (void)memcpy(send->from, from->id, 32U);
    (void)memcpy(send->to, to->id, 32U);
    (void)memcpy(send->asset, asset_id, 32U);
    send->amount = (lxp_u128){ 0U, amount };
    send->sequence = sequence;
    send->idempotency_key[0] = marker;
    send->expires_at = 20U;
    send->context_hash[0] = marker;
    send->authorization.kind = kind;
    (void)memcpy(send->authorization.controller, send->from, 32U);
    (void)memcpy(send->authorization.signed_context_hash, send->context_hash,
                 32U);
    send->authorization.network_id = 7U;
    send->authorization.protocol_version = LXP_PROTOCOL_VERSION;
    return sign_send(send, seed);
}

static int delegated_send(void)
{
    static const uint8_t delegate_seed[32] = { 2U };
    static const uint8_t bob_seed[32] = { 3U };
    lx_account_registry registry;
    lx_account *alice;
    lx_account *bob;
    uint8_t asset_id[32] = { 3U };
    lxp_transfer_asset_state asset;
    lxp_send_store store;
    lxp_send_environment environment;
    lxp_send_receipt_projection receipt;
    lxp_authority_scope scope;
    lxp_transfer_allowance allowance;
    lxp_send send;

    CHECK(lx_account_registry_init(&registry) == LXP_OK);
    if (open_account(&registry, "agent:did:key:alice:main", asset_id, 100U,
                     delegate_seed, &alice) != 0) return 1;
    if (open_account(&registry, "agent:did:key:bob:main", asset_id, 0U,
                     bob_seed, &bob) != 0) return 1;
    (void)memset(&asset, 0, sizeof(asset));
    (void)memcpy(asset.asset_id, asset_id, 32U);
    asset.registered = true;
    (void)memset(&store, 0, sizeof(store));
    environment = (lxp_send_environment){ &registry, &asset, 1U, &store,
                                          10U, 7U, LXP_PROTOCOL_VERSION, NULL };

    /* A declared delegated debit without the grant it draws against is
     * refused before anything moves. */
    CHECK(build_send(&send, alice, bob, asset_id, 40U, 0U, 1U,
                     LXP_AUTH_DELEGATED_CAPABILITY, delegate_seed) == 0);
    CHECK(lxp_send_execute(&send, &environment, &receipt) ==
          LXP_ERR_AUTH_ALLOWANCE);
    CHECK(alice->balance.lo == 100U && bob->balance.lo == 0U);
    CHECK(alice->next_sequence == 0U && store.count == 0U);

    metered_scope(&scope, asset_id);
    (void)memset(&allowance, 0, sizeof(allowance));
    allowance.scope = &scope;
    allowance.kind = LXP_AUTHORITY_DELEGATED_CAPABILITY;
    (void)memcpy(allowance.grantor, alice->id, 32U);
    allowance.grant_id[0] = 0x11U;
    environment.allowance = &allowance;

    /* The presented grant has to carry the declared kind. */
    allowance.kind = LXP_AUTHORITY_SESSION_KEY;
    CHECK(lxp_send_execute(&send, &environment, &receipt) == LXP_ERR_AUTH_SCOPE);
    allowance.kind = LXP_AUTHORITY_DELEGATED_CAPABILITY;

    /* The debited account has to be the grantor. */
    (void)memcpy(allowance.grantor, bob->id, 32U);
    CHECK(lxp_send_execute(&send, &environment, &receipt) ==
          LXP_ERR_UNAUTHORIZED_DEBIT);
    (void)memcpy(allowance.grantor, alice->id, 32U);

    /* The scope binds one asset and the modules it names. */
    scope.asset_id[0] ^= 1U;
    CHECK(lxp_send_execute(&send, &environment, &receipt) ==
          LXP_ERR_ASSET_MISMATCH);
    scope.asset_id[0] ^= 1U;
    scope.module_mask = UINT64_C(1) << LXP_MODULE_ASSET;
    CHECK(lxp_send_execute(&send, &environment, &receipt) == LXP_ERR_AUTH_SCOPE);
    scope.module_mask = UINT64_C(1);

    /* A debit over the per-activity cap leaves the scope and balances
     * untouched. */
    CHECK(build_send(&send, alice, bob, asset_id, 60U, 0U, 2U,
                     LXP_AUTH_DELEGATED_CAPABILITY, delegate_seed) == 0);
    CHECK(lxp_send_execute(&send, &environment, &receipt) ==
          LXP_ERR_GRANT_EXHAUSTED);
    CHECK(lxp_u128_is_zero(scope.spent_total));
    CHECK(alice->balance.lo == 100U && bob->balance.lo == 0U);
    CHECK(alice->next_sequence == 0U && store.count == 0U);

    /* A debit inside the scope moves the balance and charges the scope. */
    CHECK(build_send(&send, alice, bob, asset_id, 40U, 0U, 1U,
                     LXP_AUTH_DELEGATED_CAPABILITY, delegate_seed) == 0);
    CHECK(lxp_send_execute(&send, &environment, &receipt) == LXP_OK);
    CHECK(alice->balance.lo == 60U && bob->balance.lo == 40U);
    CHECK(alice->next_sequence == 1U && store.count == 1U);
    CHECK(scope.spent_total.lo == 40U);
    CHECK(receipt.from_after.lo == 60U && receipt.to_after.lo == 40U);

    /* The total cap is enforced against the charged scope. */
    CHECK(build_send(&send, alice, bob, asset_id, 40U, 1U, 3U,
                     LXP_AUTH_DELEGATED_CAPABILITY, delegate_seed) == 0);
    CHECK(lxp_send_execute(&send, &environment, &receipt) ==
          LXP_ERR_GRANT_EXHAUSTED);
    CHECK(scope.spent_total.lo == 40U);
    CHECK(alice->balance.lo == 60U && bob->balance.lo == 40U);
    CHECK(alice->next_sequence == 1U && store.count == 1U);

    CHECK(build_send(&send, alice, bob, asset_id, 30U, 1U, 4U,
                     LXP_AUTH_DELEGATED_CAPABILITY, delegate_seed) == 0);
    CHECK(lxp_send_execute(&send, &environment, &receipt) == LXP_OK);
    CHECK(alice->balance.lo == 30U && bob->balance.lo == 70U);
    CHECK(alice->next_sequence == 2U && store.count == 2U);
    CHECK(scope.spent_total.lo == 70U);

    CHECK(build_send(&send, alice, bob, asset_id, 1U, 2U, 5U,
                     LXP_AUTH_DELEGATED_CAPABILITY, delegate_seed) == 0);
    CHECK(lxp_send_execute(&send, &environment, &receipt) ==
          LXP_ERR_GRANT_EXHAUSTED);
    CHECK(scope.spent_total.lo == 70U);
    CHECK(alice->balance.lo == 30U && alice->next_sequence == 2U);

    /* An owner debit may not ride on a delegated grant either: the declared
     * kind and the presented grant have to agree. */
    CHECK(build_send(&send, alice, bob, asset_id, 1U, 2U, 6U,
                     LXP_AUTH_OWNER, delegate_seed) == 0);
    CHECK(lxp_send_execute(&send, &environment, &receipt) == LXP_ERR_AUTH_SCOPE);
    CHECK(alice->balance.lo == 30U && alice->next_sequence == 2U);
    CHECK(store.count == 2U);
    lx_account_registry_release(&registry);
    return 0;
}

static int delegated_receive(void)
{
    static const uint8_t payer_seed[32] = { 4U };
    static const uint8_t merchant_seed[32] = { 5U };
    lx_account_registry registry;
    lx_account *payer;
    lx_account *merchant;
    uint8_t asset_id[32] = { 4U };
    uint8_t purpose_preimage[64];
    lxp_payer_grant grant;
    lxp_receive receive;
    lxp_grant_store grants;
    lxp_send_store idempotency;
    lxp_transfer_asset_state asset;
    lxp_receive_environment environment;
    lxp_send_receipt_projection receipt;
    lxp_authority_scope scope;
    lxp_transfer_allowance allowance;

    CHECK(lx_account_registry_init(&registry) == LXP_OK);
    if (open_account(&registry, "agent:did:key:payer:main", asset_id, 100U,
                     payer_seed, &payer) != 0) return 1;
    if (open_account(&registry, "agent:did:key:merchant:main", asset_id, 0U,
                     merchant_seed, &merchant) != 0) return 1;
    (void)memset(&grant, 0, sizeof(grant));
    (void)memcpy(grant.from, payer->id, 32U);
    (void)memcpy(grant.recipient, merchant->id, 32U);
    (void)memcpy(grant.asset, asset_id, 32U);
    grant.per_draw_maximum = (lxp_u128){ 0U, 30U };
    grant.allowance = (lxp_u128){ 0U, 50U };
    grant.expiration = 100U;
    grant.purpose_hash[0] = 8U;
    grant.has_reference = true;
    grant.reference_hash[0] = 9U;
    grant.revocation_sequence = 5U;
    (void)memcpy(grant.public_key, payer->authority_key, 32U);
    CHECK(sign_grant(&grant, payer_seed) == 0);
    (void)memset(&receive, 0, sizeof(receive));
    (void)memcpy(receive.from, payer->id, 32U);
    (void)memcpy(receive.to, merchant->id, 32U);
    (void)memcpy(receive.asset, asset_id, 32U);
    receive.amount = (lxp_u128){ 0U, 30U };
    (void)memcpy(receive.grant_id, grant.grant_id, 32U);
    receive.idempotency_key[0] = 1U;
    (void)memcpy(purpose_preimage, grant.purpose_hash, 32U);
    (void)memcpy(purpose_preimage + 32U, grant.reference_hash, 32U);
    CHECK(lxp_hash_context_value(purpose_preimage, sizeof(purpose_preimage),
                                 receive.context_hash) == LXP_OK);
    receive.receiver_authorization.kind = LXP_AUTH_DELEGATED_CAPABILITY;
    (void)memcpy(receive.receiver_authorization.controller, merchant->id, 32U);
    (void)memcpy(receive.receiver_authorization.public_key,
                 merchant->authority_key, 32U);
    (void)memcpy(receive.receiver_authorization.signed_context_hash,
                 receive.context_hash, 32U);
    receive.receiver_authorization.network_id = 7U;
    receive.receiver_authorization.protocol_version = LXP_PROTOCOL_VERSION;
    receive.payer_grant = grant;
    CHECK(sign_receive(&receive, merchant_seed) == 0);
    (void)memset(&grants, 0, sizeof(grants));
    (void)memset(&idempotency, 0, sizeof(idempotency));
    (void)memset(&asset, 0, sizeof(asset));
    (void)memcpy(asset.asset_id, asset_id, 32U);
    asset.registered = true;
    CHECK(lxp_grant_store_put(&grants, &grant, payer) == LXP_OK);
    environment = (lxp_receive_environment){ &registry, &asset, 1U, &grants,
        &idempotency, 10U, 1U, 7U, LXP_PROTOCOL_VERSION, NULL };

    /* A declared delegated draw without its grant scope is refused and the
     * payer grant's drawn total is restored. */
    CHECK(lxp_receive_execute(&receive, &environment, &receipt) ==
          LXP_ERR_AUTH_ALLOWANCE);
    CHECK(payer->balance.lo == 100U && merchant->balance.lo == 0U);
    CHECK(merchant->next_sequence == 0U && idempotency.count == 0U);
    CHECK(lxp_u128_is_zero(grants.grants[0].drawn_total));
    CHECK(!grants.grants[0].invoice_settled);

    /* The scope has to be the payer's: the draw debits the payer, not the
     * receiver that signed it. */
    metered_scope(&scope, asset_id);
    (void)memset(&allowance, 0, sizeof(allowance));
    allowance.scope = &scope;
    allowance.kind = LXP_AUTHORITY_DELEGATED_CAPABILITY;
    (void)memcpy(allowance.grantor, merchant->id, 32U);
    allowance.grant_id[0] = 0x22U;
    environment.allowance = &allowance;
    CHECK(lxp_receive_execute(&receive, &environment, &receipt) ==
          LXP_ERR_UNAUTHORIZED_DEBIT);
    CHECK(payer->balance.lo == 100U && merchant->balance.lo == 0U);
    CHECK(lxp_u128_is_zero(grants.grants[0].drawn_total));
    CHECK(lxp_u128_is_zero(scope.spent_total));

    (void)memcpy(allowance.grantor, payer->id, 32U);
    CHECK(lxp_receive_execute(&receive, &environment, &receipt) == LXP_OK);
    CHECK(payer->balance.lo == 70U && merchant->balance.lo == 30U);
    CHECK(merchant->next_sequence == 1U && idempotency.count == 1U);
    CHECK(grants.grants[0].drawn_total.lo == 30U);
    CHECK(grants.grants[0].invoice_settled);
    CHECK(scope.spent_total.lo == 30U);
    lx_account_registry_release(&registry);
    return 0;
}

int main(void)
{
    if (delegated_send() != 0) return 1;
    if (delegated_receive() != 0) return 1;
    return 0;
}
