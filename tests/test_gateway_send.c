#include "layerx/lxp_gateway.h"
#include "layerx/lxp_hash.h"
#include "../src/network/lxp_gateway_internal.h"

#include <openssl/evp.h>
#include <sched.h>
#include <string.h>

typedef struct gateway_world {
    lx_asset_registry assets;
    lx_asset_record asset;
    lx_account_registry accounts;
    lx_account *payer;
    lx_account *payee;
    lxp_transfer_asset_state transfer_asset;
    lxp_send_store sends;
    lxp_send_environment environment;
    lxp_gateway_invoice_registry *invoices;
    lxp_gateway_settlement_context settlement;
} gateway_world;

typedef struct settlement_thread {
    const lxp_payment_requirement *requirement;
    const lxp_send *send;
    lxp_gateway_settlement_context *context;
    lxp_receipt receipt;
    lxp_result status;
} settlement_thread;

typedef struct invoice_lookup_thread {
    lx_account_registry *accounts;
    lxp_gateway_invoice_registry *registry;
    uint8_t invoice_id[32];
    uint8_t idempotency_key[32];
    lxp_receipt receipt;
    bool settled;
    lxp_result status;
} invoice_lookup_thread;

static void *settle_concurrently(void *argument)
{
    settlement_thread *thread = (settlement_thread *)argument;
    thread->status = lxp_gateway_send_settle(
        thread->requirement, thread->send, thread->context, &thread->receipt);
    return NULL;
}

static void *lookup_concurrently(void *argument)
{
    invoice_lookup_thread *thread = (invoice_lookup_thread *)argument;
    thread->status = lxp_gateway_invoice_state(
        thread->accounts, thread->registry, thread->invoice_id,
        thread->idempotency_key,
        &thread->receipt, &thread->settled);
    return NULL;
}

static int public_key_for(
    const uint8_t private_key[32], uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(
        EVP_PKEY_ED25519, NULL, private_key, 32U);
    size_t length = 32U;
    int ok = key != NULL && EVP_PKEY_get_raw_public_key(
        key, public_key, &length) == 1 && length == 32U;
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static int sign_raw(
    const uint8_t private_key[32], const uint8_t *message,
    size_t message_length, uint8_t signature[64])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(
        EVP_PKEY_ED25519, NULL, private_key, 32U);
    EVP_MD_CTX *context = EVP_MD_CTX_new();
    size_t signature_length = 64U;
    int ok = key != NULL && context != NULL &&
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(context, signature, &signature_length,
                       message, message_length) == 1 &&
        signature_length == 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static int sign_requirement(
    const uint8_t private_key[32], lxp_payment_requirement *requirement)
{
    uint8_t bytes[LXP_PAYMENT_REQUIREMENT_PREIMAGE_SIZE];
    size_t length = 0U;
    return lxp_payment_requirement_encode(
        requirement, false, bytes, sizeof(bytes), &length) != LXP_OK ||
        length != sizeof(bytes) ||
        sign_raw(private_key, bytes, length,
                 requirement->service_signature) != 0;
}

static int sign_send(
    const uint8_t private_key[32], lxp_send *send, uint8_t public_key[32])
{
    uint8_t message[512];
    uint8_t digest[32];
    size_t length = 0U;
    if (public_key_for(private_key, public_key) != 0) return 1;
    (void)memcpy(send->authorization.public_key, public_key, 32U);
    if (lxp_send_authorization_message(
            send, message, sizeof(message), &length) != LXP_OK ||
        lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE,
                        message, length, digest) != LXP_OK)
        return 1;
    return sign_raw(private_key, digest, sizeof(digest),
                    send->authorization.signature);
}

static int world_init(
    gateway_world *world,
    const uint8_t payer_public_key[32],
    const uint8_t service_public_key[32],
    const uint8_t sequencer_private_key[32],
    lxp_arena *arena,
    uint64_t timestamp)
{
    const char *payer_name = "agent:did:key:payer:main";
    const char *payee_name = "agent:did:key:service:main";
    lxp_result registry_status;
    (void)memset(world, 0, sizeof(*world));
    world->asset.asset_id[0] = 3U;
    (void)memcpy(world->asset.symbol, "USD", 4U);
    world->asset.symbol_length = 3U;
    world->asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    world->asset.custody_reference[0] = 1U;
    world->asset.custody_reference_length = 1U;
    if (lx_asset_registry_init(&world->assets, 0U) != LXP_OK ||
        lx_asset_register(
            &world->assets, &world->asset, 0U,
            (lxp_u128){0U, 0U}) != LXP_OK ||
        lx_account_registry_init(&world->accounts) != LXP_OK ||
        lx_asset_account_open(
            &world->assets, &world->accounts, world->asset.asset_id,
            (const uint8_t *)payer_name, strlen(payer_name), 1U,
            LX_ACCOUNT_OPEN_CREDIT, NULL, &world->payer) != LXP_OK ||
        lx_asset_account_open(
            &world->assets, &world->accounts, world->asset.asset_id,
            (const uint8_t *)payee_name, strlen(payee_name), 1U,
            LX_ACCOUNT_OPEN_CREDIT, NULL, &world->payee) != LXP_OK ||
        lxp_ledger_bootstrap_balance(
            world->payer, world->asset.asset_id,
            (lxp_u128){0U, 100U}, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(
            world->payee, world->asset.asset_id,
            (lxp_u128){0U, 0U}, 0U) != LXP_OK ||
        lx_asset_transfer_state(
            &world->asset, &world->transfer_asset) != LXP_OK)
        return 1;
    world->invoices = lxp_gateway_invoice_registry_create(
        &world->accounts, &registry_status);
    if (world->invoices == NULL || registry_status != LXP_OK) return 1;
    (void)memcpy(world->payer->authority_key, payer_public_key, 32U);
    world->payer->has_authority_key = true;
    world->environment = (lxp_send_environment){
        &world->accounts, &world->transfer_asset, 1U,
        &world->sends, timestamp, 42U, LXP_PROTOCOL_VERSION
    };
    world->settlement.assets = &world->assets;
    world->settlement.send_environment = &world->environment;
    world->settlement.invoices = world->invoices;
    world->settlement.service_public_key = service_public_key;
    world->settlement.sequencer_private_key = sequencer_private_key;
    world->settlement.global_sequence = 1U;
    world->settlement.batch_id[0] = 0x88U;
    world->settlement.arena = arena;
    return 0;
}

int main(void)
{
    uint8_t payer_private_key[32] = {1U};
    uint8_t service_private_key[32] = {2U};
    uint8_t sequencer_private_key[32] = {3U};
    uint8_t payer_public_key[32];
    uint8_t service_public_key[32];
    static uint8_t arena_a_bytes[LXP_MAX_ACTIVITY_BYTES + 4096U];
    static uint8_t arena_b_bytes[LXP_MAX_ACTIVITY_BYTES + 4096U];
    static uint8_t arena_c_bytes[LXP_MAX_ACTIVITY_BYTES + 4096U];
    lxp_arena arena_a;
    lxp_arena arena_b;
    lxp_arena arena_c;
    static gateway_world gateway;
    static gateway_world direct;
    static gateway_world concurrent;
    lxp_payment_requirement requirement;
    lxp_send send;
    lxp_send decoded;
    lxp_receipt gateway_receipt;
    lxp_receipt direct_receipt;
    lxp_receipt replay_receipt;
    uint8_t encoded_send[512];
    size_t encoded_send_length = 0U;
    lxp_byte_span gateway_bytes;
    lxp_byte_span direct_bytes;
    size_t arena_a_mark;
    size_t arena_b_mark;
    lxp_result gateway_status;
    lxp_result direct_status;
    lxp_gateway_transaction_boundary boundary;
    lx_account payer_before;
    lx_account payee_before;

    if (public_key_for(payer_private_key, payer_public_key) != 0 ||
        public_key_for(service_private_key, service_public_key) != 0 ||
        lxp_arena_init(&arena_a, arena_a_bytes, sizeof(arena_a_bytes)) !=
            LXP_OK ||
        lxp_arena_init(&arena_b, arena_b_bytes, sizeof(arena_b_bytes)) !=
            LXP_OK ||
        lxp_arena_init(&arena_c, arena_c_bytes, sizeof(arena_c_bytes)) !=
            LXP_OK ||
        world_init(&gateway, payer_public_key, service_public_key,
                   sequencer_private_key, &arena_a, 100U) != 0 ||
        world_init(&direct, payer_public_key, service_public_key,
                   sequencer_private_key, &arena_b, 100U) != 0 ||
        world_init(&concurrent, payer_public_key, service_public_key,
                   sequencer_private_key, &arena_c, 100U) != 0)
        return 1;
    (void)memset(&requirement, 0, sizeof(requirement));
    requirement.network_id = 42U;
    (void)memcpy(requirement.recipient, gateway.payee->id, 32U);
    (void)memcpy(requirement.asset, gateway.asset.asset_id, 32U);
    requirement.amount = (lxp_u128){0U, 25U};
    requirement.invoice_id[0] = 0x44U;
    requirement.purpose_hash[0] = 0x55U;
    requirement.expiry = 200U;
    requirement.acceptable_conditions =
        (UINT32_C(1) << LXP_CONDITION_NOT_BEFORE) |
        (UINT32_C(1) << LXP_CONDITION_NOT_AFTER);
    if (sign_requirement(service_private_key, &requirement) != 0) return 1;
    (void)memset(&send, 0, sizeof(send));
    (void)memcpy(send.from, gateway.payer->id, 32U);
    (void)memcpy(send.to, requirement.recipient, 32U);
    (void)memcpy(send.asset, requirement.asset, 32U);
    send.amount = requirement.amount;
    send.idempotency_key[0] = 0x66U;
    send.expires_at = 180U;
    (void)memcpy(send.context_hash, requirement.purpose_hash, 32U);
    send.condition_count = 1U;
    send.conditions[0] =
        (lxp_send_condition){LXP_CONDITION_NOT_BEFORE, 90U};
    send.authorization.kind = LXP_AUTH_OWNER;
    (void)memcpy(send.authorization.controller, send.from, 32U);
    (void)memcpy(send.authorization.signed_context_hash,
                 send.context_hash, 32U);
    send.authorization.network_id = 42U;
    send.authorization.protocol_version = LXP_PROTOCOL_VERSION;
    if (sign_send(payer_private_key, &send, payer_public_key) != 0 ||
        lxp_send_encode(
            &send, encoded_send, sizeof(encoded_send),
            &encoded_send_length) != LXP_OK ||
        lxp_send_decode(encoded_send, encoded_send_length, &decoded) != LXP_OK)
        return 1;
    {
        lx_account_registry alternate_accounts;
        lx_account_registry race_accounts;
        lxp_gateway_invoice_registry *alternate_invoices = NULL;
        lxp_gateway_invoice_registry *competing_invoices = NULL;
        lxp_gateway_invoice_registry *race_invoices = NULL;
        lxp_gateway_invoice_registry *replacement_invoices = NULL;
        lxp_gateway_settlement_context mismatched = gateway.settlement;
        invoice_lookup_thread lookup;
        pthread_t lookup_worker;
        lxp_receipt unused_receipt;
        lxp_result create_status;
        bool settled = false;
        (void)memset(&alternate_accounts, 0, sizeof(alternate_accounts));
        (void)memset(&race_accounts, 0, sizeof(race_accounts));
        (void)memset(&lookup, 0, sizeof(lookup));
        if (lxp_gateway_invoice_state(
                NULL, alternate_invoices, requirement.invoice_id,
                send.idempotency_key, &unused_receipt,
                &settled) != LXP_ERR_NON_CANONICAL ||
            lx_account_registry_init(&alternate_accounts) != LXP_OK)
            return 1;
        alternate_invoices = lxp_gateway_invoice_registry_create(
            &alternate_accounts, &create_status);
        if (alternate_invoices == NULL || create_status != LXP_OK)
            return 1;
        competing_invoices = lxp_gateway_invoice_registry_create(
            &alternate_accounts, &create_status);
        if (competing_invoices != NULL ||
            create_status != LXP_ERR_SEQUENCE_REUSED)
            return 1;
        if (lx_account_registry_init(&race_accounts) != LXP_OK)
            return 1;
        race_invoices = lxp_gateway_invoice_registry_create(
            &race_accounts, &create_status);
        if (race_invoices == NULL || create_status != LXP_OK)
            return 1;
        lookup.accounts = &race_accounts;
        lookup.registry = race_invoices;
        lxp_gateway_registry_test_pause_before_activation();
        if (pthread_create(
                &lookup_worker, NULL, lookup_concurrently, &lookup) != 0)
            return 1;
        while (!lxp_gateway_registry_test_activation_paused())
            (void)sched_yield();
        if (lxp_gateway_invoice_registry_destroy(
                &race_accounts, &race_invoices) != LXP_ERR_IO ||
            race_invoices == NULL)
            return 1;
        lxp_gateway_registry_test_release_activation();
        if (pthread_join(lookup_worker, NULL) != 0 ||
            lookup.status != LXP_OK || lookup.settled ||
            lxp_gateway_invoice_registry_destroy(
                &race_accounts, &race_invoices) != LXP_OK ||
            race_invoices != NULL)
            return 1;
        replacement_invoices = lxp_gateway_invoice_registry_create(
            &race_accounts, &create_status);
        if (replacement_invoices == NULL || create_status != LXP_OK ||
            lxp_gateway_invoice_registry_destroy(
                &race_accounts, &replacement_invoices) != LXP_OK)
            return 1;
        mismatched.invoices = alternate_invoices;
        if (lxp_gateway_send_settle(
                &requirement, &send, &mismatched,
                &unused_receipt) != LXP_ERR_NON_CANONICAL ||
            gateway.payer->balance.lo != 100U ||
            gateway.payee->balance.lo != 0U)
            return 1;
        if (lxp_gateway_registry_enter(
                alternate_invoices, &alternate_accounts) != LXP_OK)
            return 1;
        if (lxp_gateway_invoice_registry_destroy(
                &alternate_accounts, &alternate_invoices) != LXP_ERR_IO ||
            lxp_gateway_registry_leave(alternate_invoices) != LXP_OK)
            return 1;
        alternate_invoices->records[0].receipt.signature[0] = 0xa5U;
        alternate_invoices->count = 1U;
        if (lxp_gateway_invoice_registry_destroy(
                &alternate_accounts, &alternate_invoices) != LXP_OK ||
                alternate_invoices != NULL ||
            lxp_gateway_invoice_registry_destroy(
                &alternate_accounts,
                &alternate_invoices) != LXP_ERR_NON_CANONICAL)
            return 1;
        replacement_invoices = lxp_gateway_invoice_registry_create(
            &alternate_accounts, &create_status);
        if (replacement_invoices == NULL || create_status != LXP_OK ||
            lxp_gateway_invoice_registry_destroy(
                &alternate_accounts, &replacement_invoices) != LXP_OK)
            return 1;
    }
    payer_before = *gateway.payer;
    payee_before = *gateway.payee;
    for (boundary = LXP_GATEWAY_AFTER_BALANCE_WRITE;
         boundary <= LXP_GATEWAY_AFTER_INVOICE_WRITE;
         boundary = (lxp_gateway_transaction_boundary)((unsigned)boundary + 1U)) {
        lxp_receipt zero_receipt;
        lxp_send_store_record zero_send;
        lxp_gateway_invoice_record zero_invoice;
        size_t mark = lxp_arena_mark(&arena_a);
        (void)memset(&zero_receipt, 0, sizeof(zero_receipt));
        (void)memset(&zero_send, 0, sizeof(zero_send));
        (void)memset(&zero_invoice, 0, sizeof(zero_invoice));
        (void)memset(&gateway_receipt, 0xa5, sizeof(gateway_receipt));
        lxp_gateway_send_test_fail_after(boundary);
        if (lxp_gateway_send_settle(
                &requirement, &send, &gateway.settlement,
                &gateway_receipt) != LXP_ERR_IO ||
            memcmp(gateway.payer, &payer_before, sizeof(payer_before)) != 0 ||
            memcmp(gateway.payee, &payee_before, sizeof(payee_before)) != 0 ||
            gateway.sends.count != 0U || gateway.invoices->count != 0U ||
            memcmp(&gateway.sends.records[0], &zero_send,
                   sizeof(zero_send)) != 0 ||
            memcmp(&gateway.invoices->records[0], &zero_invoice,
                   sizeof(zero_invoice)) != 0 ||
            lxp_arena_mark(&arena_a) != mark ||
            memcmp(&gateway_receipt, &zero_receipt,
                   sizeof(gateway_receipt)) != 0)
            return 1;
    }
    {
        size_t capacity = arena_a.capacity;
        lxp_receipt zero_receipt;
        lxp_send_store_record zero_send;
        lxp_gateway_invoice_record zero_invoice;
        (void)memset(&zero_receipt, 0, sizeof(zero_receipt));
        (void)memset(&zero_send, 0, sizeof(zero_send));
        (void)memset(&zero_invoice, 0, sizeof(zero_invoice));
        arena_a.capacity = arena_a.offset;
        if (lxp_gateway_send_settle(
                &requirement, &send, &gateway.settlement,
                &gateway_receipt) != LXP_ERR_ARENA_EXHAUSTED ||
            memcmp(gateway.payer, &payer_before, sizeof(payer_before)) != 0 ||
            memcmp(gateway.payee, &payee_before, sizeof(payee_before)) != 0 ||
            gateway.sends.count != 0U || gateway.invoices->count != 0U ||
            memcmp(&gateway.sends.records[0], &zero_send,
                   sizeof(zero_send)) != 0 ||
            memcmp(&gateway.invoices->records[0], &zero_invoice,
                   sizeof(zero_invoice)) != 0 ||
            memcmp(&gateway_receipt, &zero_receipt,
                   sizeof(gateway_receipt)) != 0)
            return 1;
        arena_a.capacity = capacity;
    }
    gateway_status = lxp_gateway_send_settle(
        &requirement, &send, &gateway.settlement, &gateway_receipt);
    direct_status = lxp_gateway_send_settle(
        &requirement, &decoded, &direct.settlement, &direct_receipt);
    if (gateway_status != LXP_OK || direct_status != LXP_OK ||
        gateway.payer->balance.lo != 75U ||
        gateway.payee->balance.lo != 25U ||
        direct.payer->balance.lo != 75U || direct.payee->balance.lo != 25U ||
        memcmp(&gateway_receipt, &direct_receipt,
               sizeof(gateway_receipt)) != 0)
        return 1;
    {
        settlement_thread threads[2];
        pthread_t workers[2];
        size_t index;
        size_t successes = 0U;
        size_t replays = 0U;
        (void)memset(threads, 0, sizeof(threads));
        for (index = 0U; index < 2U; ++index) {
            threads[index].requirement = &requirement;
            threads[index].send = &send;
            threads[index].context = &concurrent.settlement;
            if (pthread_create(&workers[index], NULL, settle_concurrently,
                               &threads[index]) != 0)
                return 1;
        }
        for (index = 0U; index < 2U; ++index) {
            if (pthread_join(workers[index], NULL) != 0) return 1;
            if (threads[index].status == LXP_OK) ++successes;
            else if (threads[index].status == LXP_ERR_IDEMPOTENT_REPLAY)
                ++replays;
            else return 1;
        }
        if (successes != 1U || replays != 1U ||
            memcmp(&threads[0].receipt, &threads[1].receipt,
                   sizeof(threads[0].receipt)) != 0 ||
            concurrent.payer->balance.lo != 75U ||
            concurrent.payee->balance.lo != 25U ||
            concurrent.sends.count != 1U || concurrent.invoices->count != 1U)
            return 1;
    }
    arena_a_mark = lxp_arena_mark(&arena_a);
    arena_b_mark = lxp_arena_mark(&arena_b);
    if (lxp_gateway_receipt_return(
            &gateway_receipt, &arena_a, &gateway_bytes) != LXP_OK ||
        lxp_gateway_receipt_return(
            &direct_receipt, &arena_b, &direct_bytes) != LXP_OK ||
        gateway_bytes.length != direct_bytes.length ||
        memcmp(gateway_bytes.bytes, direct_bytes.bytes,
               gateway_bytes.length) != 0 ||
        memcmp(gateway_receipt.resulting_state_root,
               direct_receipt.resulting_state_root, 32U) != 0)
        return 1;
    (void)lxp_arena_reset(&arena_a, arena_a_mark);
    (void)lxp_arena_reset(&arena_b, arena_b_mark);
    if (lxp_gateway_send_settle(
            &requirement, &send, &gateway.settlement,
            &replay_receipt) != LXP_ERR_IDEMPOTENT_REPLAY ||
        memcmp(&gateway_receipt, &replay_receipt,
               sizeof(gateway_receipt)) != 0 ||
        gateway.payer->balance.lo != 75U || gateway.payee->balance.lo != 25U)
        return 1;
    direct.environment.batch_timestamp = 201U;
    decoded.sequence = 1U;
    decoded.idempotency_key[0] = 0x77U;
    if (sign_send(payer_private_key, &decoded, payer_public_key) != 0 ||
        lxp_gateway_send_settle(
            &requirement, &decoded, &direct.settlement,
            &direct_receipt) != LXP_ERR_EXPIRED ||
        direct.payer->balance.lo != 75U || direct.payee->balance.lo != 25U)
        return 1;
    return lxp_gateway_invoice_registry_destroy(
               &gateway.accounts, &gateway.invoices) != LXP_OK ||
           lxp_gateway_invoice_registry_destroy(
               &direct.accounts, &direct.invoices) != LXP_OK ||
           lxp_gateway_invoice_registry_destroy(
               &concurrent.accounts, &concurrent.invoices) != LXP_OK;
}
