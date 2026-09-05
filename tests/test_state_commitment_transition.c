#include "layerx/lxp_kernel.h"
#include "layerx/lx_asset.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define REQUIRE(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "state commitment check failed at line %d\n", __LINE__); \
    return 1; } } while (0)

typedef struct fixture {
    lxp_kernel kernel;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_identity_store identities;
    lx_account_registry accounts;
    lx_asset_runtime runtime;
    lx_asset_record asset;
    lxp_transfer_asset_state transfer_asset;
    lxp_arena arena;
    uint8_t arena_bytes[4U * 1024U * 1024U];
    uint64_t parameters;
    uint8_t public_key[32];
    uint8_t signature[64];
    uint8_t payload[512];
    lxp_activity activity;
    lxp_kernel_execution execution;
    lxp_authority_resolved authority;
    lxp_fee_params fees;
    lxp_receipt receipt;
} fixture;

static const uint8_t seed[32] = {1U};
static const uint8_t did[] = "did:key:alice";

static int sign_digest(const uint8_t digest[32], uint8_t signature[64],
                        uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, seed, 32U);
    EVP_MD_CTX *ctx = EVP_MD_CTX_new();
    size_t key_length = 32U;
    size_t signature_length = 64U;
    int ok = key != NULL && ctx != NULL &&
        EVP_PKEY_get_raw_public_key(key, public_key, &key_length) == 1 &&
        key_length == 32U && EVP_DigestSignInit(ctx, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(ctx, signature, &signature_length, digest, 32U) == 1 &&
        signature_length == 64U;
    EVP_MD_CTX_free(ctx);
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static int prepare(fixture *f, uint16_t version, bool success)
{
    static const uint8_t from_name[] = "agent:did:key:alice:main";
    static const uint8_t to_name[] = "agent:did:key:bob:main";
    uint8_t digest[32] = {0};
    uint8_t material[144];
    uint8_t message[512];
    size_t message_length;
    size_t payload_length;
    lx_account *from;
    lx_account *to;
    lxp_identity *identity;
    lxp_send send;
    (void)memset(&send, 0, sizeof(send));
    REQUIRE(sign_digest(digest, f->signature, f->public_key) == 0);
    REQUIRE(lxp_arena_init(&f->arena, f->arena_bytes, sizeof(f->arena_bytes)) == LXP_OK);
    REQUIRE(lxp_state_store_init(&f->state, 1U) == LXP_OK);
    f->parameters = 1U;
    REQUIRE(lxp_kernel_create(&f->kernel, &f->state, &f->journal,
                              &f->parameters, 0U) == LXP_OK);
    REQUIRE(lxp_kernel_set_capabilities(&f->kernel, NULL,
                                        lxp_kernel_canonical_ledger_apply) == LXP_OK);
    REQUIRE(lxp_kernel_register_module(&f->kernel, lx_asset_module_iface()) == LXP_OK);
    REQUIRE(lxp_identity_register(&f->identities, did, sizeof(did) - 1U,
                                  f->public_key, &identity) == LXP_OK);
    REQUIRE(lx_account_id_from_string(from_name, sizeof(from_name) - 1U, send.from) == LXP_OK);
    REQUIRE(lx_account_id_from_string(to_name, sizeof(to_name) - 1U, send.to) == LXP_OK);
    send.asset[0] = 3U;
    send.amount.lo = 1U;
    send.expires_at = 100U;
    send.idempotency_key[0] = 7U;
    send.authorization.kind = LXP_AUTH_OWNER;
    send.authorization.network_id = 7U;
    send.authorization.protocol_version = version;
    (void)memcpy(send.authorization.controller, send.from, 32U);
    (void)memcpy(material, send.from, 32U);
    (void)memcpy(material + 32U, send.to, 32U);
    (void)memcpy(material + 64U, send.asset, 32U);
    REQUIRE(lxp_u128_to_be(send.amount, material + 96U) == LXP_OK);
    (void)memcpy(material + 112U, send.idempotency_key, 32U);
    REQUIRE(lxp_hash_context_value(material, sizeof(material), send.context_hash) == LXP_OK);
    (void)memcpy(send.authorization.signed_context_hash, send.context_hash, 32U);
    (void)memcpy(send.authorization.public_key, f->public_key, 32U);
    REQUIRE(lxp_send_authorization_message(&send, message, sizeof(message), &message_length) == LXP_OK);
    REQUIRE(lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, message, message_length, digest) == LXP_OK);
    REQUIRE(sign_digest(digest, send.authorization.signature, f->public_key) == 0);
    REQUIRE(lxp_send_encode(&send, f->payload, sizeof(f->payload), &payload_length) == LXP_OK);
    if (success) {
        REQUIRE(lx_account_registry_init(&f->accounts) == LXP_OK);
        REQUIRE(lx_account_open(&f->accounts, from_name, sizeof(from_name) - 1U,
                                send.from, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &from) == LXP_OK);
        REQUIRE(lx_account_open(&f->accounts, to_name, sizeof(to_name) - 1U,
                                send.to, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &to) == LXP_OK);
        REQUIRE(lxp_ledger_bootstrap_balance(from, send.asset, (lxp_u128){0U, 10U}, 0U) == LXP_OK);
        REQUIRE(lxp_ledger_bootstrap_balance(to, send.asset, (lxp_u128){0U, 0U}, 0U) == LXP_OK);
        from->has_authority_key = true;
        (void)memcpy(from->authority_key, f->public_key, 32U);
        (void)memcpy(f->asset.asset_id, send.asset, 32U);
        (void)memcpy(f->transfer_asset.asset_id, send.asset, 32U);
        f->transfer_asset.registered = true;
        f->runtime = (lx_asset_runtime){&f->accounts, &f->asset, 1U,
            &f->transfer_asset, 1U, 7U, version};
        REQUIRE(lxp_kernel_bind_module_runtime(&f->kernel, LXP_MODULE_ASSET, &f->runtime) == LXP_OK);
    }
    f->activity.protocol_version = version;
    f->activity.network_id = 7U;
    f->activity.activity_type = LX_ASSET_SEND;
    f->activity.actor_did = (lxp_byte_span){did, sizeof(did) - 1U};
    f->activity.authority = (lxp_byte_span){f->public_key, 32U};
    f->activity.signature = (lxp_byte_span){f->signature, 64U};
    f->activity.timestamp_bound = (lxp_timestamp_bound){1U, 100U};
    f->activity.payload = (lxp_byte_span){f->payload, payload_length};
    (void)memcpy(f->activity.idempotency_key, send.idempotency_key, 32U);
    REQUIRE(lxp_hash_payload(f->payload, payload_length, f->activity.payload_hash) == LXP_OK);
    REQUIRE(lxp_activity_signing_preimage(&f->activity, digest) == LXP_OK);
    REQUIRE(sign_digest(digest, f->signature, f->public_key) == 0);
    REQUIRE(lxp_activity_verify_signature(&f->activity) == LXP_OK);
    f->authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(f->authority.principal, send.from, 32U);
    (void)memcpy(f->authority.verified_key, f->public_key, 32U);
    (void)memcpy(f->authority.actor, identity->did_id, 32U);
    f->fees.version = 1U;
    f->fees.base_fee.lo = success ? 0U : 1U;
    f->fees.multiplier_basis_points = 10000U;
    f->execution.network_id = 7U;
    f->execution.batch_number = 1U;
    f->execution.batch_timestamp_ms = 10U;
    f->execution.maximum_timestamp_window = 100U;
    f->execution.global_sequence = 1U;
    f->execution.recorded_module_version = 1U;
    f->execution.parameter_version = 1U;
    f->execution.signature_valid = true;
    f->execution.identities = &f->identities;
    f->execution.authority = &f->authority;
    f->execution.fee_parameters = &f->fees;
    f->execution.gas_limit = 10000U;
    f->execution.arena = &f->arena;
    f->execution.sequencer_private_key = seed;
    f->execution.batch_id[0] = 5U;
    REQUIRE(lxp_state_root(&f->kernel, f->kernel.current_state_root) == LXP_OK);
    return 0;
}

static int legacy_root(fixture *f)
{
    uint8_t material[156];
    uint8_t module_root[32];
    uint8_t expected[32];
    size_t i;
    size_t offset = 0U;
    (void)memcpy(material + offset, f->receipt.previous_state_root, 32U); offset += 32U;
    (void)memcpy(material + offset, f->receipt.activity_id, 32U); offset += 32U;
    for (i = 0U; i < 8U; ++i)
        material[offset++] = (uint8_t)(f->receipt.global_sequence >> (56U - 8U * i));
    for (i = 0U; i < 4U; ++i)
        material[offset++] = (uint8_t)((uint32_t)f->receipt.result_code >> (24U - 8U * i));
    REQUIRE(lxp_u128_to_be(f->receipt.fee_charged, material + offset) == LXP_OK); offset += 16U;
    for (i = 0U; i < 4U; ++i)
        material[offset++] = (uint8_t)(f->receipt.module_version >> (24U - 8U * i));
    REQUIRE(lxp_state_subtree_root(&f->kernel, f->receipt.module_id, module_root) == LXP_OK);
    (void)memcpy(material + offset, module_root, 32U); offset += 32U;
    REQUIRE(lxp_hash_domain(LXP_DOMAIN_RECEIPT, material, offset, expected) == LXP_OK);
    REQUIRE(memcmp(expected, f->receipt.resulting_state_root, 32U) == 0);
    return 0;
}

static int receipt_state_tamper(fixture *f, const uint8_t original[32])
{
    lxp_idempotency_key_state *entry = &f->state.idempotency[0];
    uint8_t changed[32];
    uint8_t projection[LXP_STATE_MAX_RECEIPT_BYTES];
    uint32_t original_length = entry->receipt_length;
    size_t offset;
    REQUIRE(original_length > 1U);
    REQUIRE(lxp_kernel_idempotency_state_value(entry->receipt, original_length,
                projection, original_length - 1U) == LXP_ERR_NON_CANONICAL);
    entry->receipt_length = original_length - 1U;
    REQUIRE(lxp_state_root(&f->kernel, changed) == LXP_ERR_NON_CANONICAL);
    entry->receipt_length = original_length;
    for (offset = 0U; offset + 32U <= entry->receipt_length; ++offset)
        if (memcmp(entry->receipt + offset, f->receipt.activity_id, 32U) == 0) break;
    REQUIRE(offset + 32U <= entry->receipt_length);
    entry->receipt[offset] ^= 1U;
    REQUIRE(lxp_state_root(&f->kernel, changed) == LXP_OK);
    REQUIRE(memcmp(original, changed, 32U) != 0);
    entry->receipt[offset] ^= 1U;
    for (offset = 0U; offset + 32U <= entry->receipt_length; ++offset)
        if (memcmp(entry->receipt + offset, f->receipt.resulting_state_root, 32U) == 0) break;
    REQUIRE(offset + 32U <= entry->receipt_length);
    entry->receipt[offset] ^= 1U;
    REQUIRE(lxp_state_root(&f->kernel, changed) == LXP_OK);
    REQUIRE(memcmp(original, changed, 32U) == 0);
    entry->receipt[offset] ^= 1U;
    REQUIRE(lxp_receipt_verify(&f->receipt, f->public_key, &f->arena) == LXP_OK);
    return 0;
}

static int transition(uint16_t version, bool success)
{
    fixture *f = (fixture *)calloc(1U, sizeof(*f));
    uint8_t committed[32];
    uint8_t changed[32];
    uint8_t saved;
    lxp_byte_span wire;
    lxp_receipt *decoded;
    REQUIRE(f != NULL);
    REQUIRE(prepare(f, version, success) == 0);
    REQUIRE(lxp_kernel_execute_activity(&f->kernel, &f->activity,
                                         &f->execution, &f->receipt) == LXP_OK);
    if (f->receipt.result_code != (success ? LXP_OK : LXP_ERR_FEE_LIMIT))
        (void)fprintf(stderr, "protocol %u success %u receipt result %d\n",
                      (unsigned)version, (unsigned)success,
                      (int)f->receipt.result_code);
    REQUIRE(f->receipt.result_code == (success ? LXP_OK : LXP_ERR_FEE_LIMIT));
    if (version == LXP_PROTOCOL_VERSION_STATE_COMMITMENT && !success) {
        lxp_send attempted;
        REQUIRE(lxp_send_decode(f->payload, f->activity.payload.length, &attempted) == LXP_OK);
        REQUIRE(f->receipt.operation == lxp_activity_type_ordinal(LX_ASSET_SEND));
        REQUIRE(memcmp(f->receipt.asset, attempted.asset, 32U) == 0);
        REQUIRE(memcmp(f->receipt.from, attempted.from, 32U) == 0);
        REQUIRE(memcmp(f->receipt.to, attempted.to, 32U) == 0);
        REQUIRE(memcmp(f->receipt.context_hash, attempted.context_hash, 32U) == 0);
        REQUIRE(lxp_u128_cmp(f->receipt.amount, attempted.amount) == 0);
        REQUIRE(lxp_u128_is_zero(f->receipt.from_balance_before));
        REQUIRE(lxp_u128_is_zero(f->receipt.from_balance_after));
        REQUIRE(lxp_u128_is_zero(f->receipt.to_balance_before));
        REQUIRE(lxp_u128_is_zero(f->receipt.to_balance_after));
        REQUIRE(f->receipt.effects.count == 0U);
        REQUIRE(lxp_ct_is_zero(f->receipt.transfer_set_root, 32U));
    }
    REQUIRE(lxp_receipt_verify(&f->receipt, f->public_key, &f->arena) == LXP_OK);
    REQUIRE(lxp_receipt_encode(&f->receipt, true, &f->arena, &wire) == LXP_OK);
    decoded = (lxp_receipt *)malloc(sizeof(*decoded));
    REQUIRE(decoded != NULL);
    REQUIRE(lxp_receipt_decode(wire.bytes, wire.length, true, decoded) == LXP_OK);
    REQUIRE(decoded->protocol_version == version);
    REQUIRE(lxp_receipt_verify(decoded, f->public_key, &f->arena) == LXP_OK);
    if (version == LXP_PROTOCOL_VERSION_STATE_COMMITMENT && !success) {
        decoded->operation ^= 1U;
        REQUIRE(lxp_receipt_verify(decoded, f->public_key, &f->arena) != LXP_OK);
        decoded->operation ^= 1U;
        decoded->asset[0] ^= 1U;
        REQUIRE(lxp_receipt_verify(decoded, f->public_key, &f->arena) != LXP_OK);
        decoded->asset[0] ^= 1U;
        decoded->from[0] ^= 1U;
        REQUIRE(lxp_receipt_verify(decoded, f->public_key, &f->arena) != LXP_OK);
        decoded->from[0] ^= 1U;
    }
    decoded->resulting_state_root[0] ^= 1U;
    REQUIRE(lxp_receipt_verify(decoded, f->public_key, &f->arena) != LXP_OK);
    free(decoded);
    REQUIRE(lxp_state_root(&f->kernel, committed) == LXP_OK);
    if (version == LXP_PROTOCOL_VERSION_STATE_COMMITMENT) {
        REQUIRE(memcmp(committed, f->receipt.resulting_state_root, 32U) == 0);
        REQUIRE(f->state.idempotency_count == 1U);
        REQUIRE(receipt_state_tamper(f, committed) == 0);
        saved = f->state.idempotency[0].key_hash[0];
        f->state.idempotency[0].key_hash[0] ^= 1U;
        REQUIRE(lxp_state_root(&f->kernel, changed) == LXP_OK);
        REQUIRE(memcmp(committed, changed, 32U) != 0);
        f->state.idempotency[0].key_hash[0] = saved;
        REQUIRE(lxp_state_root(&f->kernel, changed) == LXP_OK);
        REQUIRE(memcmp(committed, changed, 32U) == 0);
        if (success) {
            REQUIRE(f->accounts.accounts[0].balance.lo + f->accounts.accounts[1].balance.lo == 10U);
            REQUIRE(f->receipt.amount.lo == 1U);
        }
    } else {
        REQUIRE(legacy_root(f) == 0);
        REQUIRE(memcmp(committed, f->receipt.resulting_state_root, 32U) != 0);
    }
    REQUIRE(lxp_state_store_destroy(&f->state) == LXP_OK);
    free(f);
    return 0;
}

static int refusal_with_accounts(bool malformed)
{
    fixture *f = (fixture *)calloc(1U, sizeof(*f));
    uint8_t digest[32];
    uint8_t original_root[32];
    lxp_send decoded;
    lxp_result expected;
    REQUIRE(f != NULL);
    REQUIRE(prepare(f, LXP_PROTOCOL_VERSION_STATE_COMMITMENT, true) == 0);
    f->fees.base_fee.lo = 1U;
    REQUIRE(lxp_state_root(&f->kernel, original_root) == LXP_OK);
    if (malformed) {
        --f->activity.payload.length;
        REQUIRE(lxp_hash_payload(f->payload, f->activity.payload.length,
                                  f->activity.payload_hash) == LXP_OK);
        REQUIRE(lxp_activity_signing_preimage(&f->activity, digest) == LXP_OK);
        REQUIRE(sign_digest(digest, f->signature, f->public_key) == 0);
        REQUIRE(lxp_activity_verify_signature(&f->activity) == LXP_OK);
        expected = lxp_send_decode(f->payload, f->activity.payload.length, &decoded);
        REQUIRE(expected != LXP_OK);
        REQUIRE(lxp_kernel_execute_activity(&f->kernel, &f->activity,
                                              &f->execution, &f->receipt) == expected);
        REQUIRE(f->state.idempotency_count == 0U);
        REQUIRE(f->state.next_sequence == 1U);
        REQUIRE(lxp_state_root(&f->kernel, digest) == LXP_OK);
        REQUIRE(memcmp(original_root, digest, 32U) == 0);
    } else {
        REQUIRE(lxp_kernel_execute_activity(&f->kernel, &f->activity,
                                              &f->execution, &f->receipt) == LXP_OK);
        REQUIRE(f->receipt.result_code == LXP_ERR_FEE_LIMIT);
        REQUIRE(f->receipt.operation == lxp_activity_type_ordinal(LX_ASSET_SEND));
        REQUIRE(f->receipt.asset[0] == 3U);
        REQUIRE(f->receipt.amount.lo == 1U);
        REQUIRE(f->receipt.from_balance_before.lo == 10U);
        REQUIRE(f->receipt.from_balance_after.lo == 10U);
        REQUIRE(lxp_u128_is_zero(f->receipt.to_balance_before));
        REQUIRE(lxp_u128_is_zero(f->receipt.to_balance_after));
        REQUIRE(f->receipt.effects.count == 0U);
        REQUIRE(lxp_ct_is_zero(f->receipt.transfer_set_root, 32U));
        REQUIRE(lxp_receipt_verify(&f->receipt, f->public_key, &f->arena) == LXP_OK);
        REQUIRE(lxp_state_root(&f->kernel, digest) == LXP_OK);
        REQUIRE(memcmp(f->receipt.resulting_state_root, digest, 32U) == 0);
    }
    REQUIRE(f->accounts.accounts[0].balance.lo == 10U);
    REQUIRE(f->accounts.accounts[0].next_sequence == 0U);
    REQUIRE(lxp_u128_is_zero(f->accounts.accounts[1].balance));
    REQUIRE(!f->journal.open);
    REQUIRE(lxp_state_store_destroy(&f->state) == LXP_OK);
    free(f);
    return 0;
}

static int preview_commit(void)
{
    fixture *f = (fixture *)calloc(1U, sizeof(*f));
    lxp_module_ctx *ctx = (lxp_module_ctx *)malloc(sizeof(*ctx));
    uint8_t before[32], preview[32], committed[32];
    const uint8_t key[] = "state-commitment-check";
    const uint8_t value[] = {9U};
    REQUIRE(f != NULL && ctx != NULL);
    REQUIRE(prepare(f, LXP_PROTOCOL_VERSION_STATE_COMMITMENT, true) == 0);
    REQUIRE(lxp_state_root(&f->kernel, before) == LXP_OK);
    REQUIRE(lxp_state_journal_open(&f->state, 1U, &f->journal) == LXP_OK);
    REQUIRE(lxp_module_ctx_init(ctx, &f->kernel, LXP_MODULE_ASSET, 10U, 0U, 1U,
                                100U, &f->arena, true) == LXP_OK);
    ctx->protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    REQUIRE(lxp_ctx_kv_put(ctx, key, sizeof(key) - 1U, value, sizeof(value)) == LXP_OK);
    REQUIRE(lxp_module_ctx_prepare_commit(ctx) == LXP_OK);
    REQUIRE(lxp_module_ctx_preview_state_root(ctx, &f->journal, preview) == LXP_OK);
    REQUIRE(memcmp(before, preview, 32U) != 0);
    REQUIRE(lxp_state_root(&f->kernel, committed) == LXP_OK);
    REQUIRE(memcmp(before, committed, 32U) == 0);
    REQUIRE(lxp_state_journal_commit(&f->journal) == LXP_OK);
    REQUIRE(lxp_module_ctx_commit(ctx) == LXP_OK);
    REQUIRE(lxp_state_root(&f->kernel, committed) == LXP_OK);
    REQUIRE(memcmp(preview, committed, 32U) == 0);
    REQUIRE(lxp_state_store_destroy(&f->state) == LXP_OK);
    free(ctx);
    free(f);
    return 0;
}

int main(void)
{
    REQUIRE(LXP_PROTOCOL_VERSION == LXP_PROTOCOL_VERSION_OCCUPANCY);
    REQUIRE(lxp_protocol_version_supported(LXP_PROTOCOL_VERSION_STATE_COMMITMENT));
    REQUIRE(!lxp_protocol_version_supported(4U));
    REQUIRE(transition(LXP_PROTOCOL_VERSION_LEGACY, false) == 0);
    REQUIRE(transition(LXP_PROTOCOL_VERSION_OCCUPANCY, false) == 0);
    REQUIRE(transition(LXP_PROTOCOL_VERSION_STATE_COMMITMENT, false) == 0);
    REQUIRE(transition(LXP_PROTOCOL_VERSION_STATE_COMMITMENT, true) == 0);
    REQUIRE(refusal_with_accounts(false) == 0);
    REQUIRE(refusal_with_accounts(true) == 0);
    REQUIRE(preview_commit() == 0);
    (void)puts("state commitment transition: legacy, version 3, preview, signatures and tampering passed");
    return 0;
}
