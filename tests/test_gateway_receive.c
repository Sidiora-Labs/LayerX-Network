#include "layerx/lxp_gateway.h"
#include "layerx/lxp_hash.h"
#include "../src/network/lxp_gateway_internal.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <string.h>

typedef struct receive_world {
    lx_asset_registry assets;
    lx_asset_record asset;
    lx_account_registry accounts;
    lx_account *payer;
    lx_account *service;
    lxp_transfer_asset_state transfer_asset;
    lxp_grant_store grants;
    lxp_send_store receive_idempotency;
    lxp_send_store send_idempotency;
    lxp_receive_environment receive_environment;
    lxp_send_environment send_environment;
    lxp_gateway_invoice_registry *invoices;
    lxp_gateway_receive_context receive_context;
    lxp_gateway_settlement_context send_context;
} receive_world;

typedef struct gateway_race_call {
    const lxp_payment_requirement *requirement;
    const lxp_receive *receive;
    const lxp_send *send;
    receive_world *world;
    lxp_receipt receipt;
    lxp_result status;
} gateway_race_call;

static void *claim_concurrently(void *argument)
{
    gateway_race_call *call = (gateway_race_call *)argument;
    call->status = lxp_gateway_receive_claim(
        call->requirement, call->receive, &call->world->receive_context,
        &call->receipt);
    return NULL;
}

static void *send_concurrently(void *argument)
{
    gateway_race_call *call = (gateway_race_call *)argument;
    call->status = lxp_gateway_send_settle(
        call->requirement, call->send, &call->world->send_context,
        &call->receipt);
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

static int sign_domain(
    const uint8_t private_key[32], lxp_domain_tag_id domain,
    const uint8_t *message, size_t message_length, uint8_t signature[64])
{
    uint8_t digest[32];
    return lxp_hash_domain(domain, message, message_length, digest) != LXP_OK ||
        sign_raw(private_key, digest, sizeof(digest), signature) != 0;
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

static int sign_grant(
    const uint8_t private_key[32], lxp_payer_grant *grant)
{
    uint8_t message[384];
    size_t length = 0U;
    return lxp_grant_authorization_message(
        grant, message, sizeof(message), &length) != LXP_OK ||
        lxp_hash_authority(message, length, grant->grant_id) != LXP_OK ||
        sign_domain(private_key, LXP_DOMAIN_AUTHORITY_HASH,
                    message, length, grant->signature) != 0;
}

static int sign_receive(
    const uint8_t private_key[32], lxp_receive *receive)
{
    uint8_t message[512];
    size_t length = 0U;
    return lxp_receive_authorization_message(
        receive, message, sizeof(message), &length) != LXP_OK ||
        sign_domain(private_key, LXP_DOMAIN_SIGNATURE_PREIMAGE,
                    message, length,
                    receive->receiver_authorization.signature) != 0;
}

static int sign_send(
    const uint8_t private_key[32], lxp_send *send,
    const uint8_t public_key[32])
{
    uint8_t message[512];
    size_t length = 0U;
    (void)memcpy(send->authorization.public_key, public_key, 32U);
    return lxp_send_authorization_message(
        send, message, sizeof(message), &length) != LXP_OK ||
        sign_domain(private_key, LXP_DOMAIN_SIGNATURE_PREIMAGE,
                    message, length, send->authorization.signature) != 0;
}

static int world_init(
    receive_world *world, const uint8_t payer_public_key[32],
    const uint8_t service_public_key[32],
    const uint8_t sequencer_private_key[32], lxp_arena *arena)
{
    const char *payer_name = "agent:did:key:payer:main";
    const char *service_name = "agent:did:key:service:main";
    lxp_result registry_status;
    (void)memset(world, 0, sizeof(*world));
    world->asset.asset_id[0] = 4U;
    (void)memcpy(world->asset.symbol, "USD", 4U);
    world->asset.symbol_length = 3U;
    world->asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    world->asset.custody_reference[0] = 1U;
    world->asset.custody_reference_length = 1U;
    if (lx_asset_registry_init(&world->assets, 0U) != LXP_OK ||
        lx_asset_register(&world->assets, &world->asset, 0U,
                          (lxp_u128){0U, 0U}) != LXP_OK ||
        lx_account_registry_init(&world->accounts) != LXP_OK ||
        lx_asset_account_open(
            &world->assets, &world->accounts, world->asset.asset_id,
            (const uint8_t *)payer_name, strlen(payer_name), 1U,
            LX_ACCOUNT_OPEN_CREDIT, NULL, &world->payer) != LXP_OK ||
        lx_asset_account_open(
            &world->assets, &world->accounts, world->asset.asset_id,
            (const uint8_t *)service_name, strlen(service_name), 1U,
            LX_ACCOUNT_OPEN_CREDIT, NULL, &world->service) != LXP_OK ||
        lxp_ledger_bootstrap_balance(
            world->payer, world->asset.asset_id,
            (lxp_u128){0U, 100U}, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(
            world->service, world->asset.asset_id,
            (lxp_u128){0U, 0U}, 0U) != LXP_OK ||
        lx_asset_transfer_state(
            &world->asset, &world->transfer_asset) != LXP_OK)
        return 1;
    world->invoices = lxp_gateway_invoice_registry_create(
        &world->accounts, &registry_status);
    if (world->invoices == NULL || registry_status != LXP_OK) return 1;
    (void)memcpy(world->payer->authority_key, payer_public_key, 32U);
    world->payer->has_authority_key = true;
    (void)memcpy(world->service->authority_key, service_public_key, 32U);
    world->service->has_authority_key = true;
    world->receive_environment = (lxp_receive_environment){
        &world->accounts, &world->transfer_asset, 1U, &world->grants,
        &world->receive_idempotency, 100U, 1U, 42U,
        LXP_PROTOCOL_VERSION
    };
    world->send_environment = (lxp_send_environment){
        &world->accounts, &world->transfer_asset, 1U,
        &world->send_idempotency, 100U, 42U, LXP_PROTOCOL_VERSION
    };
    world->receive_context = (lxp_gateway_receive_context){
        &world->assets, &world->receive_environment, world->invoices,
        service_public_key, sequencer_private_key, 7U, {0U}, arena
    };
    world->receive_context.batch_id[0] = 0x88U;
    world->send_context = (lxp_gateway_settlement_context){
        &world->assets, &world->send_environment, world->invoices,
        service_public_key, sequencer_private_key, 8U, {0U}, arena
    };
    world->send_context.batch_id[0] = 0x89U;
    return 0;
}

int main(void)
{
    static const uint8_t payer_private_key[32] = {1U};
    static const uint8_t service_private_key[32] = {2U};
    static const uint8_t sequencer_private_key[32] = {3U};
    uint8_t payer_public_key[32];
    uint8_t service_public_key[32];
    uint8_t sequencer_public_key[32];
    static uint8_t arena_bytes[LXP_MAX_ACTIVITY_BYTES + 4096U];
    static uint8_t race_arena_bytes[LXP_MAX_ACTIVITY_BYTES + 4096U];
    lxp_arena arena;
    lxp_arena race_arena;
    static receive_world world;
    static receive_world race_world;
    lxp_payment_requirement requirement;
    lxp_payment_requirement altered_requirement;
    lxp_payer_grant grant;
    lxp_receive receive;
    lxp_receive altered_receive;
    lxp_send send;
    lxp_receipt receive_receipt;
    lxp_receipt replay_receipt;
    uint8_t purpose_preimage[64];
    lxp_result status;
    lxp_gateway_transaction_boundary boundary;
    lx_account payer_before;
    lx_account service_before;

    if (public_key_for(payer_private_key, payer_public_key) != 0 ||
        public_key_for(service_private_key, service_public_key) != 0 ||
        public_key_for(sequencer_private_key, sequencer_public_key) != 0 ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_arena_init(
            &race_arena, race_arena_bytes, sizeof(race_arena_bytes)) !=
                LXP_OK ||
        world_init(&world, payer_public_key, service_public_key,
                   sequencer_private_key, &arena) != 0 ||
        world_init(&race_world, payer_public_key, service_public_key,
                   sequencer_private_key, &race_arena) != 0)
        return 1;
    (void)memset(&requirement, 0, sizeof(requirement));
    requirement.network_id = 42U;
    (void)memcpy(requirement.recipient, world.service->id, 32U);
    (void)memcpy(requirement.asset, world.asset.asset_id, 32U);
    requirement.amount = (lxp_u128){0U, 30U};
    requirement.invoice_id[0] = 9U;
    requirement.purpose_hash[0] = 8U;
    requirement.expiry = 200U;
    requirement.acceptable_conditions =
        UINT32_C(1) << LXP_CONDITION_NOT_BEFORE;
    if (sign_requirement(service_private_key, &requirement) != 0) return 1;

    (void)memset(&grant, 0, sizeof(grant));
    (void)memcpy(grant.from, world.payer->id, 32U);
    (void)memcpy(grant.recipient, world.service->id, 32U);
    (void)memcpy(grant.asset, requirement.asset, 32U);
    grant.per_draw_maximum = (lxp_u128){0U, 30U};
    grant.allowance = (lxp_u128){0U, 50U};
    grant.expiration = 180U;
    (void)memcpy(grant.purpose_hash, requirement.purpose_hash, 32U);
    grant.has_reference = true;
    (void)memcpy(grant.reference_hash, requirement.invoice_id, 32U);
    grant.revocation_sequence = 5U;
    (void)memcpy(grant.public_key, payer_public_key, 32U);
    if (sign_grant(payer_private_key, &grant) != 0) return 1;
    {
        lxp_payer_grant same;
        size_t account_count = world.accounts.count;
        (void)memset(&same, 0xa5, sizeof(same));
        (void)memcpy(same.grant_id, grant.grant_id, 32U);
        (void)memcpy(same.from, grant.from, 32U);
        (void)memcpy(same.recipient, grant.recipient, 32U);
        (void)memcpy(same.asset, grant.asset, 32U);
        same.per_draw_maximum = grant.per_draw_maximum;
        same.allowance = grant.allowance;
        same.recurring = grant.recurring;
        same.window_length = grant.window_length;
        same.expiration = grant.expiration;
        (void)memcpy(same.purpose_hash, grant.purpose_hash, 32U);
        same.has_reference = grant.has_reference;
        (void)memcpy(same.reference_hash, grant.reference_hash, 32U);
        same.revocation_sequence = grant.revocation_sequence;
        (void)memcpy(same.public_key, grant.public_key, 32U);
        (void)memcpy(same.signature, grant.signature, 64U);
        if (memcmp(&same, &grant, sizeof(grant)) == 0 ||
            lxp_gateway_registry_enter(
                world.invoices, &world.accounts) != LXP_OK ||
            lxp_gateway_grant_present_test_locked(
                &grant, &world.accounts, &world.grants) != LXP_OK ||
            lxp_gateway_grant_present_test_locked(
                &same, &world.accounts, &world.grants) != LXP_OK ||
            world.grants.count != 1U ||
            lxp_gateway_registry_leave(world.invoices) != LXP_OK)
            return 1;
        (void)memset(&world.grants, 0, sizeof(world.grants));
        if (lxp_gateway_registry_enter(
                world.invoices, &world.accounts) != LXP_OK)
            return 1;
        world.grants.count = LXP_GRANT_STORE_CAPACITY + 1U;
        if (lxp_gateway_grant_present_test_locked(
                &grant, &world.accounts, &world.grants) !=
                    LXP_ERR_MALFORMED_GRANT)
            return 1;
        world.grants.count = 0U;
        world.accounts.count = LX_ACCOUNT_REGISTRY_CAPACITY + 1U;
        if (lxp_gateway_grant_present_test_locked(
                &grant, &world.accounts, &world.grants) !=
                    LXP_ERR_MALFORMED_GRANT)
            return 1;
        world.accounts.count = account_count;
        if (lxp_gateway_registry_leave(world.invoices) != LXP_OK)
            return 1;
    }

    (void)memset(&receive, 0, sizeof(receive));
    (void)memcpy(receive.from, world.payer->id, 32U);
    (void)memcpy(receive.to, world.service->id, 32U);
    (void)memcpy(receive.asset, requirement.asset, 32U);
    receive.amount = requirement.amount;
    (void)memcpy(receive.grant_id, grant.grant_id, 32U);
    receive.idempotency_key[0] = 0x66U;
    (void)memcpy(purpose_preimage, requirement.purpose_hash, 32U);
    (void)memcpy(purpose_preimage + 32U, requirement.invoice_id, 32U);
    if (lxp_hash_context_value(
            purpose_preimage, sizeof(purpose_preimage),
            receive.context_hash) != LXP_OK)
        return 1;
    receive.receiver_authorization.kind = LXP_AUTH_OWNER;
    (void)memcpy(receive.receiver_authorization.controller,
                 world.service->id, 32U);
    (void)memcpy(receive.receiver_authorization.public_key,
                 service_public_key, 32U);
    (void)memcpy(receive.receiver_authorization.signed_context_hash,
                 receive.context_hash, 32U);
    receive.receiver_authorization.network_id = 42U;
    receive.receiver_authorization.protocol_version = LXP_PROTOCOL_VERSION;
    receive.payer_grant = grant;
    if (sign_receive(service_private_key, &receive) != 0) return 1;
    payer_before = *world.payer;
    service_before = *world.service;
    altered_receive = receive;
    altered_receive.amount.lo = 31U;
    if (lxp_gateway_receive_claim(
            &requirement, &altered_receive, &world.receive_context,
            &replay_receipt) != LXP_ERR_GRANT_SCOPE_VIOLATION ||
        world.grants.count != 0U || world.payer->balance.lo != 100U ||
        world.service->balance.lo != 0U)
        return 1;
    for (boundary = LXP_GATEWAY_AFTER_GRANT_WRITE;
         boundary <= LXP_GATEWAY_AFTER_INVOICE_WRITE;
         boundary = (lxp_gateway_transaction_boundary)((unsigned)boundary + 1U)) {
        lxp_receipt zero_receipt;
        lxp_grant_state zero_grant;
        lxp_send_store_record zero_idempotency;
        lxp_gateway_invoice_record zero_invoice;
        size_t mark = lxp_arena_mark(&arena);
        (void)memset(&zero_receipt, 0, sizeof(zero_receipt));
        (void)memset(&zero_grant, 0, sizeof(zero_grant));
        (void)memset(&zero_idempotency, 0, sizeof(zero_idempotency));
        (void)memset(&zero_invoice, 0, sizeof(zero_invoice));
        (void)memset(&receive_receipt, 0xa5, sizeof(receive_receipt));
        lxp_gateway_receive_test_fail_after(boundary);
        if (lxp_gateway_receive_claim(
                &requirement, &receive, &world.receive_context,
                &receive_receipt) != LXP_ERR_IO ||
            memcmp(world.payer, &payer_before, sizeof(payer_before)) != 0 ||
            memcmp(world.service, &service_before,
                   sizeof(service_before)) != 0 || world.grants.count != 0U ||
            world.receive_idempotency.count != 0U ||
            world.invoices->count != 0U || lxp_arena_mark(&arena) != mark ||
            memcmp(&world.grants.grants[0], &zero_grant,
                   sizeof(zero_grant)) != 0 ||
            memcmp(&world.receive_idempotency.records[0], &zero_idempotency,
                   sizeof(zero_idempotency)) != 0 ||
            memcmp(&world.invoices->records[0], &zero_invoice,
                   sizeof(zero_invoice)) != 0 ||
            memcmp(&receive_receipt, &zero_receipt,
                   sizeof(receive_receipt)) != 0)
            return 1;
    }
    {
        size_t capacity = arena.capacity;
        lxp_receipt zero_receipt;
        lxp_grant_state zero_grant;
        lxp_send_store_record zero_idempotency;
        lxp_gateway_invoice_record zero_invoice;
        (void)memset(&zero_receipt, 0, sizeof(zero_receipt));
        (void)memset(&zero_grant, 0, sizeof(zero_grant));
        (void)memset(&zero_idempotency, 0, sizeof(zero_idempotency));
        (void)memset(&zero_invoice, 0, sizeof(zero_invoice));
        arena.capacity = arena.offset;
        if (lxp_gateway_receive_claim(
                &requirement, &receive, &world.receive_context,
                &receive_receipt) != LXP_ERR_ARENA_EXHAUSTED ||
            memcmp(world.payer, &payer_before, sizeof(payer_before)) != 0 ||
            memcmp(world.service, &service_before,
                   sizeof(service_before)) != 0 || world.grants.count != 0U ||
            world.receive_idempotency.count != 0U ||
            world.invoices->count != 0U ||
            memcmp(&world.grants.grants[0], &zero_grant,
                   sizeof(zero_grant)) != 0 ||
            memcmp(&world.receive_idempotency.records[0], &zero_idempotency,
                   sizeof(zero_idempotency)) != 0 ||
            memcmp(&world.invoices->records[0], &zero_invoice,
                   sizeof(zero_invoice)) != 0 ||
            memcmp(&receive_receipt, &zero_receipt,
                   sizeof(receive_receipt)) != 0)
            return 1;
        arena.capacity = capacity;
    }
    status = lxp_gateway_receive_claim(
        &requirement, &receive, &world.receive_context, &receive_receipt);
    if (status != LXP_OK ||
        world.payer->balance.lo != 70U || world.service->balance.lo != 30U ||
        world.grants.count != 1U ||
        world.grants.grants[0].drawn_total.lo != 30U ||
        !world.grants.grants[0].invoice_settled ||
        receive_receipt.operation != (uint8_t)LX_ASSET_RECEIVE ||
        lxp_receipt_verify(
            &receive_receipt, sequencer_public_key, &arena) != LXP_OK) {
        (void)fprintf(stderr, "initial claim failed: %d\n", (int)status);
        return 1;
    }
    if (lxp_gateway_receive_claim(
            &requirement, &receive, &world.receive_context,
            &replay_receipt) != LXP_ERR_IDEMPOTENT_REPLAY ||
        memcmp(&receive_receipt, &replay_receipt,
               sizeof(receive_receipt)) != 0 ||
        world.payer->balance.lo != 70U || world.service->balance.lo != 30U ||
        world.grants.grants[0].drawn_total.lo != 30U ||
        world.receive_idempotency.count != 1U || world.invoices->count != 1U)
        return 1;
    {
        lxp_grant_state existing = world.grants.grants[0];
        lxp_grant_state empty;
        (void)memset(&empty, 0, sizeof(empty));
        altered_receive = receive;
        altered_receive.idempotency_key[0] = 0x7fU;
        altered_receive.payer_grant.grant_id[0] ^= 1U;
        if (lxp_gateway_receive_claim(
                &requirement, &altered_receive, &world.receive_context,
                &replay_receipt) != LXP_ERR_GRANT_SCOPE_VIOLATION ||
            world.grants.count != 1U ||
            memcmp(&world.grants.grants[0], &existing,
                   sizeof(existing)) != 0 ||
            memcmp(&world.grants.grants[1], &empty, sizeof(empty)) != 0 ||
            world.payer->balance.lo != 70U ||
            world.service->balance.lo != 30U ||
            world.receive_idempotency.count != 1U ||
            world.invoices->count != 1U)
            return 1;
    }

    (void)memset(&send, 0, sizeof(send));
    (void)memcpy(send.from, world.payer->id, 32U);
    (void)memcpy(send.to, requirement.recipient, 32U);
    (void)memcpy(send.asset, requirement.asset, 32U);
    send.amount = requirement.amount;
    (void)memcpy(send.idempotency_key, receive.idempotency_key, 32U);
    send.expires_at = 180U;
    (void)memcpy(send.context_hash, requirement.purpose_hash, 32U);
    send.authorization.kind = LXP_AUTH_OWNER;
    (void)memcpy(send.authorization.controller, world.payer->id, 32U);
    (void)memcpy(send.authorization.signed_context_hash,
                 send.context_hash, 32U);
    send.authorization.network_id = 42U;
    send.authorization.protocol_version = LXP_PROTOCOL_VERSION;
    if (sign_send(payer_private_key, &send, payer_public_key) != 0)
        return 1;
    {
        gateway_race_call calls[2];
        pthread_t workers[2];
        size_t successes = 0U;
        size_t replays = 0U;
        (void)memset(calls, 0, sizeof(calls));
        calls[0].requirement = &requirement;
        calls[0].receive = &receive;
        calls[0].world = &race_world;
        calls[1].requirement = &requirement;
        calls[1].send = &send;
        calls[1].world = &race_world;
        if (pthread_create(
                &workers[0], NULL, claim_concurrently, &calls[0]) != 0 ||
            pthread_create(
                &workers[1], NULL, send_concurrently, &calls[1]) != 0)
            return 1;
        if (pthread_join(workers[0], NULL) != 0 ||
            pthread_join(workers[1], NULL) != 0)
            return 1;
        successes += calls[0].status == LXP_OK ? 1U : 0U;
        successes += calls[1].status == LXP_OK ? 1U : 0U;
        replays += calls[0].status == LXP_ERR_IDEMPOTENT_REPLAY ? 1U : 0U;
        replays += calls[1].status == LXP_ERR_IDEMPOTENT_REPLAY ? 1U : 0U;
        if (successes != 1U || replays != 1U ||
            memcmp(&calls[0].receipt, &calls[1].receipt,
                   sizeof(calls[0].receipt)) != 0 ||
            race_world.payer->balance.lo != 70U ||
            race_world.service->balance.lo != 30U ||
            race_world.invoices->count != 1U ||
            race_world.receive_idempotency.count +
                race_world.send_idempotency.count != 1U)
            return 1;
    }
    if (lxp_gateway_send_settle(
            &requirement, &send, &world.send_context,
            &replay_receipt) != LXP_ERR_IDEMPOTENT_REPLAY ||
        memcmp(&receive_receipt, &replay_receipt,
               sizeof(receive_receipt)) != 0 ||
        world.payer->balance.lo != 70U || world.service->balance.lo != 30U)
        return 1;

    altered_receive = receive;
    altered_receive.idempotency_key[0] = 0x67U;
    altered_receive.to[0] ^= 1U;
    if (lxp_gateway_receive_claim(
            &requirement, &altered_receive, &world.receive_context,
            &replay_receipt) != LXP_ERR_GRANT_SCOPE_VIOLATION ||
        world.payer->balance.lo != 70U || world.service->balance.lo != 30U)
        return 1;
    altered_receive = receive;
    altered_receive.idempotency_key[0] = 0x68U;
    altered_receive.amount.lo = 31U;
    if (lxp_gateway_receive_claim(
            &requirement, &altered_receive, &world.receive_context,
            &replay_receipt) != LXP_ERR_GRANT_SCOPE_VIOLATION ||
        world.payer->balance.lo != 70U || world.service->balance.lo != 30U)
        return 1;

    altered_requirement = requirement;
    altered_requirement.purpose_hash[0] ^= 1U;
    if (sign_requirement(
            service_private_key, &altered_requirement) != 0)
        return 1;
    altered_receive = receive;
    altered_receive.idempotency_key[0] = 0x69U;
    if (lxp_gateway_receive_claim(
            &altered_requirement, &altered_receive, &world.receive_context,
            &replay_receipt) != LXP_ERR_PURPOSE_MISMATCH ||
        world.payer->balance.lo != 70U || world.service->balance.lo != 30U)
        return 1;

    world.receive_environment.batch_timestamp = 181U;
    altered_receive = receive;
    altered_receive.idempotency_key[0] = 0x6aU;
    if (lxp_gateway_receive_claim(
            &requirement, &altered_receive, &world.receive_context,
            &replay_receipt) != LXP_ERR_GRANT_EXPIRED ||
        world.payer->balance.lo != 70U || world.service->balance.lo != 30U)
        return 1;
    world.receive_environment.batch_timestamp = 100U;
    if (lxp_grant_revoke(&world.grants, grant.grant_id, 5U) != LXP_OK)
        return 1;
    world.receive_environment.global_sequence = 5U;
    altered_receive.idempotency_key[0] = 0x6bU;
    if (lxp_gateway_receive_claim(
            &requirement, &altered_receive, &world.receive_context,
            &replay_receipt) != LXP_ERR_GRANT_REVOKED ||
        world.payer->balance.lo != 70U || world.service->balance.lo != 30U)
        return 1;
    return lxp_gateway_invoice_registry_destroy(&world.invoices) != LXP_OK ||
           lxp_gateway_invoice_registry_destroy(
               &race_world.invoices) != LXP_OK;
}
