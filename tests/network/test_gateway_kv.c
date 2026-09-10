#include "layerx/lxp_gateway.h"
#include "layerx/lxp_hash.h"
#include "../../src/network/lxp_gateway_internal.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <string.h>

enum {
    SETTLEMENT_COUNT = 300,
    STARTING_BALANCE = 100000,
    PAYMENT_AMOUNT = 1
};

typedef struct kv_world {
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
    lxp_meter_ctx meter;
} kv_world;

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
    kv_world *world,
    const uint8_t payer_public_key[32],
    const uint8_t service_public_key[32],
    const uint8_t sequencer_private_key[32],
    lxp_arena *arena,
    uint64_t storage_ceiling)
{
    const char *payer_name = "agent:did:key:payer-kv:main";
    const char *payee_name = "agent:did:key:service-kv:main";
    lxp_result registry_status;
    (void)memset(world, 0, sizeof(*world));
    world->asset.asset_id[0] = 3U;
    (void)memcpy(world->asset.symbol, "USD", 4U);
    world->asset.symbol_length = 3U;
    world->asset.name[0] = (uint8_t)'A';
    world->asset.name_length = 1U;
    world->asset.issuer_kind = 2U;
    world->asset.issuer_did32[0] = 1U;
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
            (lxp_u128){0U, (uint64_t)STARTING_BALANCE}, 0U) != LXP_OK ||
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
        &world->sends, 100U, 42U, LXP_PROTOCOL_VERSION
    };
    if (lxp_meter_init(
            &world->meter, UINT64_MAX, storage_ceiling,
            (lxp_u128){0U, 1U}, (lxp_u128){0U, UINT64_MAX}, 1U,
            true) != LXP_OK)
        return 1;
    world->settlement.assets = &world->assets;
    world->settlement.send_environment = &world->environment;
    world->settlement.invoices = world->invoices;
    world->settlement.service_public_key = service_public_key;
    world->settlement.sequencer_private_key = sequencer_private_key;
    world->settlement.global_sequence = 1U;
    world->settlement.batch_id[0] = 0x88U;
    world->settlement.arena = arena;
    world->settlement.meter = &world->meter;
    return 0;
}

static void requirement_for(
    lxp_payment_requirement *requirement, const kv_world *world,
    uint32_t index)
{
    (void)memset(requirement, 0, sizeof(*requirement));
    requirement->network_id = 42U;
    (void)memcpy(requirement->recipient, world->payee->id, 32U);
    (void)memcpy(requirement->asset, world->asset.asset_id, 32U);
    requirement->amount = (lxp_u128){0U, (uint64_t)PAYMENT_AMOUNT};
    requirement->invoice_id[0] = (uint8_t)(index & 0xffU);
    requirement->invoice_id[1] = (uint8_t)((index >> 8U) & 0xffU);
    requirement->invoice_id[2] = 0x44U;
    requirement->purpose_hash[0] = 0x55U;
    requirement->expiry = 200U;
    requirement->acceptable_conditions = UINT32_MAX;
}

static void send_for(
    lxp_send *send, const kv_world *world,
    const lxp_payment_requirement *requirement, uint64_t sequence,
    uint32_t idempotency_index)
{
    (void)memset(send, 0, sizeof(*send));
    (void)memcpy(send->from, world->payer->id, 32U);
    (void)memcpy(send->to, requirement->recipient, 32U);
    (void)memcpy(send->asset, requirement->asset, 32U);
    send->amount = requirement->amount;
    send->sequence = sequence;
    send->idempotency_key[0] = (uint8_t)(idempotency_index & 0xffU);
    send->idempotency_key[1] = (uint8_t)((idempotency_index >> 8U) & 0xffU);
    send->idempotency_key[2] = 0x33U;
    send->expires_at = 180U;
    (void)memcpy(send->context_hash, requirement->purpose_hash, 32U);
    send->authorization.kind = LXP_AUTH_OWNER;
    (void)memcpy(send->authorization.controller, world->payer->id, 32U);
    (void)memcpy(send->authorization.signed_context_hash,
                 send->context_hash, 32U);
    send->authorization.network_id = 42U;
    send->authorization.protocol_version = LXP_PROTOCOL_VERSION;
}

static int kv_primitives(void)
{
    lxp_gateway_kv forward;
    lxp_gateway_kv reverse;
    uint8_t keys[4][8];
    uint8_t value[16];
    uint8_t forward_root[32];
    uint8_t reverse_root[32];
    uint8_t empty_root[32];
    uint8_t rolled_back_root[32];
    const uint8_t *read_value = NULL;
    size_t read_length = 0U;
    size_t mark;
    size_t i;
    if (lxp_gateway_kv_init(&forward) != LXP_OK ||
        lxp_gateway_kv_init(&reverse) != LXP_OK)
        return 1;
    for (i = 0U; i < 4U; ++i) {
        (void)memset(keys[i], 0, sizeof(keys[i]));
        keys[i][0] = (uint8_t)('a' + (int)i);
        keys[i][1] = (uint8_t)i;
    }
    (void)memset(value, 0x11, sizeof(value));
    if (lxp_gateway_kv_root(&forward, empty_root) != LXP_OK) return 1;
    for (i = 0U; i < 4U; ++i)
        if (lxp_gateway_kv_put(&forward, keys[i], sizeof(keys[i]), value,
                               sizeof(value), NULL) != LXP_OK)
            return 1;
    for (i = 4U; i > 0U; --i)
        if (lxp_gateway_kv_put(&reverse, keys[i - 1U], sizeof(keys[i - 1U]),
                               value, sizeof(value), NULL) != LXP_OK)
            return 1;
    if (lxp_gateway_kv_root(&forward, forward_root) != LXP_OK ||
        lxp_gateway_kv_root(&reverse, reverse_root) != LXP_OK ||
        memcmp(forward_root, reverse_root, 32U) != 0 ||
        memcmp(forward_root, empty_root, 32U) == 0 ||
        forward.count != 4U || reverse.count != 4U ||
        forward.stored_bytes != reverse.stored_bytes ||
        forward.stored_bytes !=
            (uint64_t)(4U * (sizeof(keys[0]) + sizeof(value))))
        return 1;
    for (i = 0U; i + 1U < forward.count; ++i)
        if (memcmp(forward.entries[i].key, forward.entries[i + 1U].key,
                   forward.entries[i].key_length) >= 0)
            return 1;
    if (lxp_gateway_kv_get(&forward, keys[2], sizeof(keys[2]), &read_value,
                           &read_length) != LXP_OK ||
        read_length != sizeof(value) ||
        memcmp(read_value, value, sizeof(value)) != 0 ||
        lxp_gateway_kv_get(&forward, keys[0], 1U, &read_value,
                           &read_length) != LXP_ERR_UNKNOWN_FIELD)
        return 1;
    mark = lxp_gateway_kv_mark(&forward);
    {
        uint8_t replacement[32];
        uint8_t created_key[8];
        (void)memset(replacement, 0x22, sizeof(replacement));
        (void)memset(created_key, 0, sizeof(created_key));
        created_key[0] = 'z';
        if (lxp_gateway_kv_put(&forward, keys[1], sizeof(keys[1]),
                               replacement, sizeof(replacement),
                               NULL) != LXP_OK ||
            lxp_gateway_kv_put(&forward, created_key, sizeof(created_key),
                               replacement, sizeof(replacement),
                               NULL) != LXP_OK ||
            forward.count != 5U ||
            lxp_gateway_kv_get(&forward, keys[1], sizeof(keys[1]),
                               &read_value, &read_length) != LXP_OK ||
            read_length != sizeof(replacement) ||
            lxp_gateway_kv_root(&forward, rolled_back_root) != LXP_OK ||
            memcmp(rolled_back_root, forward_root, 32U) == 0)
            return 1;
    }
    if (lxp_gateway_kv_rollback(&forward, mark) != LXP_OK ||
        forward.count != 4U ||
        forward.stored_bytes != reverse.stored_bytes ||
        lxp_gateway_kv_root(&forward, rolled_back_root) != LXP_OK ||
        memcmp(rolled_back_root, forward_root, 32U) != 0 ||
        lxp_gateway_kv_get(&forward, keys[1], sizeof(keys[1]), &read_value,
                           &read_length) != LXP_OK ||
        read_length != sizeof(value) ||
        memcmp(read_value, value, sizeof(value)) != 0)
        return 1;
    lxp_gateway_kv_commit(&forward, 0U);
    if (forward.undo_count != 0U) return 1;
    lxp_gateway_kv_release(&forward);
    lxp_gateway_kv_release(&reverse);
    return forward.count != 0U || forward.entries != NULL ||
           reverse.count != 0U || reverse.entries != NULL;
}

int main(void)
{
    static const uint8_t payer_private_key[32] = {1U};
    static const uint8_t service_private_key[32] = {2U};
    static const uint8_t sequencer_private_key[32] = {3U};
    uint8_t payer_public_key[32];
    uint8_t service_public_key[32];
    static uint8_t arena_bytes[LXP_MAX_ACTIVITY_BYTES + 4096U];
    static uint8_t gas_arena_bytes[LXP_MAX_ACTIVITY_BYTES + 4096U];
    lxp_arena arena;
    lxp_arena gas_arena;
    static kv_world world;
    static kv_world gas_world;
    lxp_payment_requirement requirement;
    lxp_payment_requirement fresh_requirement;
    lxp_send send;
    lxp_send replay_send;
    lxp_receipt receipt;
    lxp_receipt first_receipt;
    lxp_receipt replay_receipt;
    uint8_t state_root[32];
    uint8_t previous_root[32];
    size_t invoice_count = 0U;
    uint64_t stored_bytes = 0U;
    uint32_t index;

    if (kv_primitives() != 0) {
        (void)fprintf(stderr, "kv primitives failed\n");
        return 1;
    }
    if (public_key_for(payer_private_key, payer_public_key) != 0 ||
        public_key_for(service_private_key, service_public_key) != 0 ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_arena_init(
            &gas_arena, gas_arena_bytes, sizeof(gas_arena_bytes)) != LXP_OK ||
        world_init(&world, payer_public_key, service_public_key,
                   sequencer_private_key, &arena, UINT64_MAX) != 0 ||
        world_init(&gas_world, payer_public_key, service_public_key,
                   sequencer_private_key, &gas_arena, 4096U) != 0)
        return 1;
    if (lxp_gateway_state_root(
            &world.accounts, world.invoices, previous_root) != LXP_OK)
        return 1;
    for (index = 0U; index < (uint32_t)SETTLEMENT_COUNT; ++index) {
        requirement_for(&requirement, &world, index);
        if (sign_requirement(service_private_key, &requirement) != 0) return 1;
        send_for(&send, &world, &requirement, (uint64_t)index, index);
        if (sign_send(payer_private_key, &send, payer_public_key) != 0)
            return 1;
        (void)memset(&receipt, 0, sizeof(receipt));
        if (lxp_gateway_send_settle(
                &requirement, &send, &world.settlement, &receipt) != LXP_OK) {
            (void)fprintf(stderr, "settlement %u refused\n", index);
            return 1;
        }
        if (lxp_gateway_invoice_count(
                &world.accounts, world.invoices, &invoice_count) != LXP_OK ||
            invoice_count != (size_t)index + 1U ||
            lxp_gateway_state_root(
                &world.accounts, world.invoices, state_root) != LXP_OK ||
            memcmp(state_root, previous_root, 32U) == 0)
            return 1;
        (void)memcpy(previous_root, state_root, 32U);
        if (index == 0U) first_receipt = receipt;
    }
    if (world.payer->balance.lo !=
            (uint64_t)STARTING_BALANCE -
                (uint64_t)SETTLEMENT_COUNT * (uint64_t)PAYMENT_AMOUNT ||
        world.payee->balance.lo !=
            (uint64_t)SETTLEMENT_COUNT * (uint64_t)PAYMENT_AMOUNT ||
        world.invoices->kv.count != (size_t)SETTLEMENT_COUNT * 2U ||
        lxp_gateway_stored_bytes(
            &world.accounts, world.invoices, &stored_bytes) != LXP_OK ||
        stored_bytes == 0U ||
        world.meter.net_storage_bytes != stored_bytes)
        return 1;

    requirement_for(&requirement, &world, 0U);
    if (sign_requirement(service_private_key, &requirement) != 0) return 1;
    send_for(&send, &world, &requirement, 0U, 0U);
    if (sign_send(payer_private_key, &send, payer_public_key) != 0) return 1;
    (void)memset(&replay_receipt, 0, sizeof(replay_receipt));
    if (lxp_gateway_send_settle(
            &requirement, &send, &world.settlement,
            &replay_receipt) != LXP_ERR_IDEMPOTENT_REPLAY ||
        memcmp(&replay_receipt, &first_receipt,
               sizeof(first_receipt)) != 0)
        return 1;

    requirement_for(&fresh_requirement, &world, 9000U);
    if (sign_requirement(service_private_key, &fresh_requirement) != 0)
        return 1;
    if (lxp_gateway_send_settle(
            &fresh_requirement, &send, &world.settlement,
            &replay_receipt) != LXP_ERR_SEQUENCE_REUSED)
        return 1;

    requirement_for(&fresh_requirement, &world, 9001U);
    if (sign_requirement(service_private_key, &fresh_requirement) != 0)
        return 1;
    send_for(&replay_send, &world, &fresh_requirement,
             (uint64_t)SETTLEMENT_COUNT, 0U);
    if (sign_send(payer_private_key, &replay_send, payer_public_key) != 0)
        return 1;
    if (lxp_gateway_send_settle(
            &fresh_requirement, &replay_send, &world.settlement,
            &replay_receipt) != LXP_ERR_IDEMPOTENT_REPLAY ||
        world.payer->balance.lo !=
            (uint64_t)STARTING_BALANCE -
                (uint64_t)SETTLEMENT_COUNT * (uint64_t)PAYMENT_AMOUNT ||
        lxp_gateway_invoice_count(
            &world.accounts, world.invoices, &invoice_count) != LXP_OK ||
        invoice_count != (size_t)SETTLEMENT_COUNT ||
        lxp_gateway_state_root(
            &world.accounts, world.invoices, state_root) != LXP_OK ||
        memcmp(state_root, previous_root, 32U) != 0)
        return 1;

    {
        lx_account payer_before;
        lx_account payee_before;
        lxp_meter_ctx meter_before;
        uint8_t root_before[32];
        size_t count_before = 0U;
        size_t kv_before;
        lxp_result status = LXP_OK;
        uint32_t attempt;
        for (attempt = 0U; attempt < 64U && status == LXP_OK; ++attempt) {
            payer_before = *gas_world.payer;
            payee_before = *gas_world.payee;
            meter_before = gas_world.meter;
            kv_before = gas_world.invoices->kv.count;
            if (lxp_gateway_invoice_count(
                    &gas_world.accounts, gas_world.invoices,
                    &count_before) != LXP_OK ||
                lxp_gateway_state_root(
                    &gas_world.accounts, gas_world.invoices,
                    root_before) != LXP_OK)
                return 1;
            requirement_for(&requirement, &gas_world, attempt);
            if (sign_requirement(service_private_key, &requirement) != 0)
                return 1;
            send_for(&send, &gas_world, &requirement, (uint64_t)attempt,
                     attempt);
            if (sign_send(payer_private_key, &send, payer_public_key) != 0)
                return 1;
            (void)memset(&receipt, 0xa5, sizeof(receipt));
            status = lxp_gateway_send_settle(
                &requirement, &send, &gas_world.settlement, &receipt);
        }
        if (status != LXP_ERR_GAS_EXHAUSTED) {
            (void)fprintf(stderr, "storage ceiling never refused: %d\n",
                          (int)status);
            return 1;
        }
        if (memcmp(gas_world.payer, &payer_before,
                   sizeof(payer_before)) != 0 ||
            memcmp(gas_world.payee, &payee_before,
                   sizeof(payee_before)) != 0 ||
            memcmp(&gas_world.meter, &meter_before,
                   sizeof(meter_before)) != 0 ||
            gas_world.meter.exhausted ||
            gas_world.invoices->kv.count != kv_before ||
            lxp_gateway_invoice_count(
                &gas_world.accounts, gas_world.invoices,
                &invoice_count) != LXP_OK ||
            invoice_count != count_before ||
            lxp_gateway_state_root(
                &gas_world.accounts, gas_world.invoices,
                state_root) != LXP_OK ||
            memcmp(state_root, root_before, 32U) != 0)
            return 1;
        {
            lxp_receipt zero_receipt;
            (void)memset(&zero_receipt, 0, sizeof(zero_receipt));
            if (memcmp(&receipt, &zero_receipt, sizeof(receipt)) != 0)
                return 1;
        }
    }

    return lxp_gateway_invoice_registry_destroy(
               &world.accounts, &world.invoices) != LXP_OK ||
           lxp_gateway_invoice_registry_destroy(
               &gas_world.accounts, &gas_world.invoices) != LXP_OK;
}
