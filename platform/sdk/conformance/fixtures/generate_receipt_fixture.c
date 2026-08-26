#include "layerx/lxp_gateway.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_module.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <string.h>

typedef struct fixture_world {
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
} fixture_world;

enum {
    FIXTURE_NETWORK_ID = 42,
    FIXTURE_GLOBAL_SEQUENCE = 1
};

static const uint64_t fixture_timestamp_ms = UINT64_C(1726000000000);
static const uint64_t fixture_requirement_expiry = UINT64_C(1726000600000);
static const uint64_t fixture_send_expires_at = UINT64_C(1726000300000);
static const uint64_t fixture_not_before = UINT64_C(1725999000000);
static const uint64_t fixture_payer_start = UINT64_C(1000000);
static const uint64_t fixture_amount = UINT64_C(25000);

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
    fixture_world *world,
    const uint8_t payer_public_key[32],
    const uint8_t service_public_key[32],
    const uint8_t sequencer_private_key[32],
    lxp_arena *arena)
{
    const char *payer_name = "agent:did:key:fixture-payer:main";
    const char *payee_name = "agent:did:key:fixture-service:main";
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
            (lxp_u128){0U, fixture_payer_start}, 0U) != LXP_OK ||
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
        &world->sends, fixture_timestamp_ms, FIXTURE_NETWORK_ID,
        LXP_PROTOCOL_VERSION
    };
    world->settlement.assets = &world->assets;
    world->settlement.send_environment = &world->environment;
    world->settlement.invoices = world->invoices;
    world->settlement.service_public_key = service_public_key;
    world->settlement.sequencer_private_key = sequencer_private_key;
    world->settlement.global_sequence = FIXTURE_GLOBAL_SEQUENCE;
    world->settlement.arena = arena;
    return lxp_hash_domain(LXP_DOMAIN_BATCH_HEADER,
                           (const uint8_t *)"layerx-sdk-conformance-batch-1",
                           30U, world->settlement.batch_id) != LXP_OK;
}

static void write_hex(FILE *output, const uint8_t *bytes, size_t length)
{
    size_t i;
    for (i = 0U; i < length; ++i)
        (void)fprintf(output, "%02x", bytes[i]);
}

static void write_hex_field(FILE *output, const char *indent,
                            const char *name, const uint8_t *bytes,
                            size_t length, const char *suffix)
{
    (void)fprintf(output, "%s\"%s\": \"", indent, name);
    write_hex(output, bytes, length);
    (void)fprintf(output, "\"%s\n", suffix);
}

static int u128_low(const lxp_u128 *value, uint64_t *low)
{
    if (value->hi != 0U) return 1;
    *low = value->lo;
    return 0;
}

int main(int argc, char **argv)
{
    static const uint8_t payer_private_key[32] = {
        0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
        0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
        0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
        0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11
    };
    static const uint8_t service_private_key[32] = {
        0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
        0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
        0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
        0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22
    };
    static const uint8_t sequencer_private_key[32] = {
        0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
        0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
        0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
        0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33
    };
    uint8_t payer_public_key[32];
    uint8_t service_public_key[32];
    uint8_t sequencer_public_key[32];
    static uint8_t arena_bytes[8U * LXP_MAX_ACTIVITY_BYTES];
    lxp_arena arena;
    static fixture_world world;
    lxp_payment_requirement requirement;
    lxp_send send;
    lxp_receipt receipt;
    lxp_receipt roundtrip;
    lxp_byte_span canonical;
    uint8_t receipt_digest[32];
    uint64_t from_before;
    uint64_t from_after;
    uint64_t to_before;
    uint64_t to_after;
    uint64_t amount_low;
    uint64_t fee_low;
    const char *output_path = argc > 1 ? argv[1] :
        "platform/sdk/conformance/fixtures/receipt-positive-v1.json";
    FILE *output;
    lxp_result status;

    if (public_key_for(payer_private_key, payer_public_key) != 0 ||
        public_key_for(service_private_key, service_public_key) != 0 ||
        public_key_for(sequencer_private_key, sequencer_public_key) != 0) {
        (void)fprintf(stderr, "fixture: key derivation failed\n");
        return 1;
    }
    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        world_init(&world, payer_public_key, service_public_key,
                   sequencer_private_key, &arena) != 0) {
        (void)fprintf(stderr, "fixture: world initialisation failed\n");
        return 1;
    }
    (void)memset(&requirement, 0, sizeof(requirement));
    requirement.network_id = FIXTURE_NETWORK_ID;
    (void)memcpy(requirement.recipient, world.payee->id, 32U);
    (void)memcpy(requirement.asset, world.asset.asset_id, 32U);
    requirement.amount = (lxp_u128){0U, fixture_amount};
    if (lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH,
                        (const uint8_t *)"layerx-sdk-conformance-invoice-1",
                        32U, requirement.invoice_id) != LXP_OK ||
        lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH,
                        (const uint8_t *)"layerx-sdk-conformance-purpose-1",
                        32U, requirement.purpose_hash) != LXP_OK) {
        (void)fprintf(stderr, "fixture: requirement hashing failed\n");
        return 1;
    }
    requirement.expiry = fixture_requirement_expiry;
    requirement.acceptable_conditions =
        (UINT32_C(1) << LXP_CONDITION_NOT_BEFORE) |
        (UINT32_C(1) << LXP_CONDITION_NOT_AFTER);
    if (sign_requirement(service_private_key, &requirement) != 0) {
        (void)fprintf(stderr, "fixture: requirement signing failed\n");
        return 1;
    }
    (void)memset(&send, 0, sizeof(send));
    (void)memcpy(send.from, world.payer->id, 32U);
    (void)memcpy(send.to, requirement.recipient, 32U);
    (void)memcpy(send.asset, requirement.asset, 32U);
    send.amount = requirement.amount;
    if (lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH,
                        (const uint8_t *)"layerx-sdk-conformance-idem-key1",
                        32U, send.idempotency_key) != LXP_OK) {
        (void)fprintf(stderr, "fixture: idempotency hashing failed\n");
        return 1;
    }
    send.expires_at = fixture_send_expires_at;
    (void)memcpy(send.context_hash, requirement.purpose_hash, 32U);
    send.condition_count = 1U;
    send.conditions[0] =
        (lxp_send_condition){LXP_CONDITION_NOT_BEFORE, fixture_not_before};
    send.authorization.kind = LXP_AUTH_OWNER;
    (void)memcpy(send.authorization.controller, send.from, 32U);
    (void)memcpy(send.authorization.signed_context_hash,
                 send.context_hash, 32U);
    send.authorization.network_id = FIXTURE_NETWORK_ID;
    send.authorization.protocol_version = LXP_PROTOCOL_VERSION;
    if (sign_send(payer_private_key, &send, payer_public_key) != 0) {
        (void)fprintf(stderr, "fixture: send signing failed\n");
        return 1;
    }
    status = lxp_gateway_send_settle(
        &requirement, &send, &world.settlement, &receipt);
    if (status != LXP_OK) {
        (void)fprintf(stderr, "fixture: settlement failed: %s\n",
                      lxp_result_name(status));
        return 1;
    }
    if (world.payer->balance.hi != 0U ||
        world.payer->balance.lo != fixture_payer_start - fixture_amount ||
        world.payee->balance.hi != 0U ||
        world.payee->balance.lo != fixture_amount) {
        (void)fprintf(stderr, "fixture: settled balances are wrong\n");
        return 1;
    }
    receipt.protocol_version = LXP_PROTOCOL_VERSION_LEGACY;
    receipt.module_id = LXP_MODULE_ASSET;
    receipt.module_version = 1U;
    receipt.parameter_version = 1U;
    status = lxp_receipt_sign(&receipt, sequencer_private_key, &arena);
    if (status != LXP_OK) {
        (void)fprintf(stderr, "fixture: receipt signing failed: %s\n",
                      lxp_result_name(status));
        return 1;
    }
    status = lxp_receipt_digest(&receipt, &arena, receipt_digest);
    if (status != LXP_OK) {
        (void)fprintf(stderr, "fixture: receipt digest failed: %s\n",
                      lxp_result_name(status));
        return 1;
    }
    status = lxp_receipt_encode(&receipt, true, &arena, &canonical);
    if (status != LXP_OK) {
        (void)fprintf(stderr, "fixture: receipt encoding failed: %s\n",
                      lxp_result_name(status));
        return 1;
    }
    status = lxp_receipt_decode(canonical.bytes, canonical.length, true,
                                &roundtrip);
    if (status != LXP_OK) {
        (void)fprintf(stderr, "fixture: canonical decode failed: %s\n",
                      lxp_result_name(status));
        return 1;
    }
    status = lxp_receipt_verify(&roundtrip, sequencer_public_key, &arena);
    if (status != LXP_OK) {
        (void)fprintf(stderr, "fixture: sequencer verification failed: %s\n",
                      lxp_result_name(status));
        return 1;
    }
    if (u128_low(&receipt.from_balance_before, &from_before) != 0 ||
        u128_low(&receipt.from_balance_after, &from_after) != 0 ||
        u128_low(&receipt.to_balance_before, &to_before) != 0 ||
        u128_low(&receipt.to_balance_after, &to_after) != 0 ||
        u128_low(&receipt.amount, &amount_low) != 0 ||
        u128_low(&receipt.fee_charged, &fee_low) != 0) {
        (void)fprintf(stderr, "fixture: receipt amounts exceed 64 bits\n");
        return 1;
    }
    output = fopen(output_path, "w");
    if (output == NULL) {
        (void)fprintf(stderr, "fixture: cannot open %s\n", output_path);
        return 1;
    }
    (void)fprintf(output, "{\n");
    (void)fprintf(output, "  \"name\": \"receipt-positive-v1\",\n");
    (void)fprintf(output,
        "  \"provenance\": {\n"
        "    \"generator\": "
        "\"platform/sdk/conformance/fixtures/generate_receipt_fixture.c\",\n"
        "    \"command\": \"make platform-receipt-fixture\",\n"
        "    \"description\": \"Canonical protocol receipt produced by the "
        "real LayerX C core: lxp_gateway_send_settle executed a signed "
        "transfer against real ledger accounts, then lxp_receipt_sign and "
        "lxp_receipt_encode produced these exact bytes. Regenerate with the "
        "command above; do not edit by hand.\"\n"
        "  },\n");
    write_hex_field(output, "  ", "canonical_receipt_hex",
                    canonical.bytes, canonical.length, ",");
    (void)fprintf(output, "  \"authorized_batch\": {\n");
    write_hex_field(output, "    ", "batch_id_hex", receipt.batch_id, 32U,
                    ",");
    write_hex_field(output, "    ", "asset_hex", receipt.asset, 32U, ",");
    write_hex_field(output, "    ", "previous_state_root_hex",
                    receipt.previous_state_root, 32U, ",");
    write_hex_field(output, "    ", "resulting_state_root_hex",
                    receipt.resulting_state_root, 32U, ",");
    write_hex_field(output, "    ", "sequencer_public_key_hex",
                    sequencer_public_key, 32U, "");
    (void)fprintf(output, "  },\n");
    (void)fprintf(output, "  \"expected\": {\n");
    (void)fprintf(output, "    \"level\": \"sequencer-signed\",\n");
    (void)fprintf(output, "    \"result_code\": 0,\n");
    (void)fprintf(output, "    \"protocol_version\": %u,\n",
                  (unsigned)receipt.protocol_version);
    (void)fprintf(output, "    \"operation\": %u,\n",
                  (unsigned)receipt.operation);
    (void)fprintf(output, "    \"module_id\": %u,\n",
                  (unsigned)receipt.module_id);
    (void)fprintf(output, "    \"global_sequence\": %llu,\n",
                  (unsigned long long)receipt.global_sequence);
    (void)fprintf(output, "    \"timestamp_ms\": %llu,\n",
                  (unsigned long long)receipt.timestamp);
    (void)fprintf(output, "    \"amount\": \"%llu\",\n",
                  (unsigned long long)amount_low);
    (void)fprintf(output, "    \"fee_charged\": \"%llu\",\n",
                  (unsigned long long)fee_low);
    (void)fprintf(output, "    \"from_balance_before\": \"%llu\",\n",
                  (unsigned long long)from_before);
    (void)fprintf(output, "    \"from_balance_after\": \"%llu\",\n",
                  (unsigned long long)from_after);
    (void)fprintf(output, "    \"to_balance_before\": \"%llu\",\n",
                  (unsigned long long)to_before);
    (void)fprintf(output, "    \"to_balance_after\": \"%llu\",\n",
                  (unsigned long long)to_after);
    write_hex_field(output, "    ", "activity_id_hex", receipt.activity_id,
                    32U, ",");
    write_hex_field(output, "    ", "from_hex", receipt.from, 32U, ",");
    write_hex_field(output, "    ", "to_hex", receipt.to, 32U, ",");
    write_hex_field(output, "    ", "receipt_digest_hex", receipt_digest,
                    32U, "");
    (void)fprintf(output, "  }\n");
    (void)fprintf(output, "}\n");
    if (fclose(output) != 0) {
        (void)fprintf(stderr, "fixture: closing %s failed\n", output_path);
        return 1;
    }
    (void)fprintf(stdout, "fixture: wrote %s (%zu receipt bytes)\n",
                  output_path, canonical.length);
    return 0;
}
