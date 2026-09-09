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

static int prepare_names(fixture *f, uint16_t version, bool success,
                          const uint8_t *from_name, const uint8_t *to_name)
{
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
    REQUIRE(lx_account_id_from_string(from_name, strlen((const char *)from_name), send.from) == LXP_OK);
    REQUIRE(lx_account_id_from_string(to_name, strlen((const char *)to_name), send.to) == LXP_OK);
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
        REQUIRE(lx_account_open(&f->accounts, from_name, strlen((const char *)from_name),
                                send.from, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &from) == LXP_OK);
        REQUIRE(lx_account_open(&f->accounts, to_name, strlen((const char *)to_name),
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

static int prepare(fixture *f, uint16_t version, bool success)
{
    return prepare_names(f, version, success,
        (const uint8_t *)"agent:did:key:alice:main",
        (const uint8_t *)"agent:did:key:bob:main");
}

static int asset_send(unsigned refusal)
{
    static const uint8_t from_name[] =
        "agent:did:key:alice:asset:0300000000000000000000000000000000000000000000000000000000000000";
    static const uint8_t to_name[] =
        "agent:did:key:bob:asset:0300000000000000000000000000000000000000000000000000000000000000";
    static const uint8_t foreign_name[] =
        "agent:did:key:mallory:asset:0300000000000000000000000000000000000000000000000000000000000000";
    fixture *f = (fixture *)calloc(1U, sizeof(*f));
    lxp_result expected = LXP_OK;
    lxp_u128 before_from;
    lxp_u128 before_to;
    uint8_t root[32];
    REQUIRE(f != NULL);
    REQUIRE(prepare_names(f, LXP_PROTOCOL_VERSION_STATE_COMMITMENT, true,
        refusal == 1U ? foreign_name : from_name, to_name) == 0);
    if (refusal == 1U) expected = LXP_ERR_UNAUTHORIZED_DEBIT;
    if (refusal == 2U) {
        f->transfer_asset.paused = true;
        expected = LXP_ERR_ASSET_PAUSED;
    }
    if (refusal == 3U) {
        f->asset.asset_id[0] = 4U;
        expected = LXP_ERR_UNAUTHORIZED_DEBIT;
    }
    if (refusal == 4U) {
        REQUIRE(lxp_ledger_bootstrap_balance(&f->accounts.accounts[0],
            f->asset.asset_id, (lxp_u128){0U, 0U}, 0U) == LXP_OK);
        expected = LXP_ERR_INSUFFICIENT_BALANCE;
    }
    if (refusal == 5U) {
        REQUIRE(lxp_ledger_bootstrap_balance(&f->accounts.accounts[1],
            f->asset.asset_id, (lxp_u128){UINT64_MAX, UINT64_MAX}, 0U) == LXP_OK);
        expected = LXP_ERR_OVERFLOW;
    }
    before_from = f->accounts.accounts[0].balance;
    before_to = f->accounts.accounts[1].balance;
    REQUIRE(lxp_state_root(&f->kernel, f->kernel.current_state_root) == LXP_OK);
    REQUIRE(lxp_kernel_execute_activity(&f->kernel, &f->activity,
        &f->execution, &f->receipt) == LXP_OK);
    if (f->receipt.result_code != expected)
        (void)fprintf(stderr, "asset send case %u: expected %d got %d\n",
            refusal, (int)expected, (int)f->receipt.result_code);
    REQUIRE(f->receipt.result_code == expected);
    REQUIRE(lxp_receipt_verify(&f->receipt, f->public_key, &f->arena) == LXP_OK);
    REQUIRE(lxp_state_root(&f->kernel, root) == LXP_OK);
    REQUIRE(memcmp(root, f->receipt.resulting_state_root, 32U) == 0);
    if (refusal == 0U) {
        REQUIRE(f->accounts.accounts[0].balance.lo == 9U);
        REQUIRE(f->accounts.accounts[1].balance.lo == 1U);
        REQUIRE(f->receipt.from_balance_before.lo == 10U);
        REQUIRE(f->receipt.from_balance_after.lo == 9U);
        REQUIRE(f->receipt.to_balance_before.lo == 0U);
        REQUIRE(f->receipt.to_balance_after.lo == 1U);
        REQUIRE(f->receipt.asset[0] == 3U);
    } else {
        REQUIRE(lxp_u128_cmp(before_from, f->accounts.accounts[0].balance) == 0);
        REQUIRE(lxp_u128_cmp(before_to, f->accounts.accounts[1].balance) == 0);
    }
    REQUIRE(lxp_state_store_destroy(&f->state) == LXP_OK);
    free(f);
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

static int pay1_submit_bytes(fixture *f, uint16_t ordinal, const uint8_t *payload, size_t length, lxp_result expected)
{
    uint8_t digest[32];
    uint8_t root[32];
    f->activity.activity_type = ((uint32_t)LXP_MODULE_ASSET << 16U) | ordinal;
    lxp_identity *actor_identity;
    REQUIRE(lxp_identity_resolve(&f->identities, f->activity.actor_did.bytes,
                                 f->activity.actor_did.length, &actor_identity) == LXP_OK);
    f->activity.account_sequence = actor_identity->next_sequence;
    f->activity.idempotency_key[0] = (uint8_t)(f->activity.account_sequence + 40U);
    f->activity.payload = (lxp_byte_span){ payload, length };
    f->execution.global_sequence = f->state.next_sequence;
    REQUIRE(lxp_hash_payload(payload, length, f->activity.payload_hash) == LXP_OK);
    REQUIRE(lxp_activity_signing_preimage(&f->activity, digest) == LXP_OK);
    REQUIRE(sign_digest(digest, f->signature, f->public_key) == 0);
    REQUIRE(lxp_state_root(&f->kernel, f->kernel.current_state_root) == LXP_OK);
    lxp_result result = lxp_kernel_execute_activity(&f->kernel, &f->activity, &f->execution, &f->receipt);
    if (result != LXP_OK || f->receipt.result_code != expected)
        (void)fprintf(stderr, "PAY1 ordinal %u kernel %d receipt %d expected %d\n",
                      ordinal, result, f->receipt.result_code, expected);
    REQUIRE(result == LXP_OK);
    REQUIRE(f->receipt.result_code == expected);
    REQUIRE(lxp_receipt_verify(&f->receipt, f->public_key, &f->arena) == LXP_OK);
    REQUIRE(lxp_state_root(&f->kernel, root) == LXP_OK);
    REQUIRE(memcmp(root, f->receipt.resulting_state_root, 32U) == 0);
    {
        lxp_receipt replay;
        uint8_t replay_root[32];
        uint64_t next_sequence = actor_identity->next_sequence;
        REQUIRE(lxp_kernel_execute_activity(&f->kernel, &f->activity,
            &f->execution, &replay) == LXP_ERR_IDEMPOTENT_REPLAY);
        REQUIRE(replay.result_code == f->receipt.result_code);
        REQUIRE(memcmp(replay.activity_id, f->receipt.activity_id, 32U) == 0);
        REQUIRE(actor_identity->next_sequence == next_sequence);
        REQUIRE(lxp_state_root(&f->kernel, replay_root) == LXP_OK);
        REQUIRE(memcmp(root, replay_root, 32U) == 0);
    }
    return 0;
}

static int pay1_submit(fixture *f, uint16_t ordinal, size_t length, lxp_result expected)
{
    return pay1_submit_bytes(f, ordinal, f->payload, length, expected);
}

static int pay1_receive_grant(void)
{
    fixture *f = (fixture *)calloc(1U, sizeof(*f));
    lxp_receive receive;
    uint8_t payload[1024];
    uint8_t message[512];
    uint8_t digest[32];
    size_t length;
    size_t message_length;
    REQUIRE(f != NULL);
    REQUIRE(prepare(f, 3U, true) == 0);
    f->payload[0] = 0U; f->payload[1] = 1U;
    (void)memcpy(f->payload + 2U, f->asset.asset_id, 32U);
    REQUIRE(pay1_submit(f, 4U, 34U, LXP_OK) == 0);
    (void)memset(&receive, 0, sizeof(receive));
    (void)memcpy(receive.from, f->accounts.accounts[0].id, 32U);
    (void)memcpy(receive.to, f->accounts.accounts[2].id, 32U);
    (void)memcpy(receive.asset, f->asset.asset_id, 32U);
    (void)memcpy(receive.payer_grant.from, receive.from, 32U);
    (void)memcpy(receive.payer_grant.recipient, receive.to, 32U);
    (void)memcpy(receive.payer_grant.asset, receive.asset, 32U);
    receive.payer_grant.per_draw_maximum.lo = 2U;
    receive.payer_grant.allowance.lo = 3U;
    receive.payer_grant.expiration = 100U;
    receive.payer_grant.purpose_hash[0] = 9U;
    (void)memcpy(receive.payer_grant.public_key, f->public_key, 32U);
    REQUIRE(lxp_grant_authorization_message(&receive.payer_grant, message, sizeof(message), &message_length) == LXP_OK);
    REQUIRE(lxp_hash_authority(message, message_length, receive.payer_grant.grant_id) == LXP_OK);
    REQUIRE(lxp_hash_domain(LXP_DOMAIN_AUTHORITY_HASH, message, message_length, digest) == LXP_OK);
    REQUIRE(sign_digest(digest, receive.payer_grant.signature, f->public_key) == 0);
    REQUIRE(lxp_payer_grant_encode(&receive.payer_grant, f->payload, sizeof(f->payload), &length) == LXP_OK);
    REQUIRE(pay1_submit(f, 7U, length, LXP_OK) == 0);
    REQUIRE(pay1_submit(f, 7U, length, LXP_ERR_SEQUENCE_REUSED) == 0);
    (void)memcpy(receive.grant_id, receive.payer_grant.grant_id, 32U);
    receive.amount.lo = 2U;
    REQUIRE(lxp_hash_context_value(receive.payer_grant.purpose_hash, 32U, receive.context_hash) == LXP_OK);
    receive.receiver_authorization.kind = LXP_AUTH_OWNER;
    receive.receiver_authorization.network_id = 7U;
    receive.receiver_authorization.protocol_version = 3U;
    (void)memcpy(receive.receiver_authorization.controller, receive.to, 32U);
    (void)memcpy(receive.receiver_authorization.public_key, f->public_key, 32U);
    (void)memcpy(receive.receiver_authorization.signed_context_hash, receive.context_hash, 32U);
    for (unsigned draw = 0U; draw < 2U; ++draw) {
        receive.receiver_sequence = f->accounts.accounts[2].next_sequence;
        receive.idempotency_key[0] = (uint8_t)(f->identities.identities[0].next_sequence + 40U);
        REQUIRE(lxp_receive_authorization_message(&receive, message, sizeof(message), &message_length) == LXP_OK);
        REQUIRE(lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, message, message_length, digest) == LXP_OK);
        REQUIRE(sign_digest(digest, receive.receiver_authorization.signature, f->public_key) == 0);
        REQUIRE(lxp_receive_encode(&receive, payload, sizeof(payload), &length) == LXP_OK);
        REQUIRE(pay1_submit_bytes(f, 6U, payload, length, draw == 0U ? LXP_OK : LXP_ERR_GRANT_EXHAUSTED) == 0);
        REQUIRE(f->accounts.accounts[0].balance.lo == 8U);
        REQUIRE(f->accounts.accounts[2].balance.lo == 2U);
        if (draw == 0U) REQUIRE(f->receipt.operation == 6U && f->receipt.amount.lo == 2U);
    }
    f->payload[0] = 0U; f->payload[1] = 1U;
    (void)memcpy(f->payload + 2U, receive.grant_id, 32U);
    for (size_t i = 0U; i < 8U; ++i)
        f->payload[34U + i] = (uint8_t)(f->identities.identities[0].next_sequence >> (56U - i * 8U));
    REQUIRE(pay1_submit(f, 8U, 42U, LXP_OK) == 0);
    receive.amount.lo = 1U;
    receive.idempotency_key[0] = (uint8_t)(f->identities.identities[0].next_sequence + 40U);
    REQUIRE(lxp_receive_authorization_message(&receive, message, sizeof(message), &message_length) == LXP_OK);
    REQUIRE(lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, message, message_length, digest) == LXP_OK);
    REQUIRE(sign_digest(digest, receive.receiver_authorization.signature, f->public_key) == 0);
    REQUIRE(lxp_receive_encode(&receive, payload, sizeof(payload), &length) == LXP_OK);
    REQUIRE(pay1_submit_bytes(f, 6U, payload, length, LXP_ERR_GRANT_REVOKED) == 0);
    REQUIRE(f->accounts.accounts[0].balance.lo == 8U && f->accounts.accounts[2].balance.lo == 2U);
    REQUIRE(lxp_state_store_destroy(&f->state) == LXP_OK);
    free(f);
    return 0;
}

static int pay1_prepared_account(unsigned mode)
{
    bool issuance_mode = mode != 0U;
    fixture *f = (fixture *)calloc(1U, sizeof(*f));
    lxp_module_ctx *ctx = (lxp_module_ctx *)calloc(1U, sizeof(*ctx));
    lxp_prepared_module_transition *prepared = NULL;
    lxp_effect_buffer effects;
    const lxp_module_registration *registration;
    lxp_result module_result;
    lxp_byte_span encoded;
    uint8_t digest[32], activity_id[32], token[32] = {1U};
    uint8_t before[32], preview[32], imported[32], committed[32];
    REQUIRE(f != NULL && ctx != NULL);
    REQUIRE(prepare(f, 3U, true) == 0);
    f->activity.activity_type = LX_ASSET_ACCOUNT_OPEN;
    f->payload[0] = 0U; f->payload[1] = 1U;
    (void)memcpy(f->payload + 2U, f->asset.asset_id, 32U);
    f->activity.payload = (lxp_byte_span){f->payload, 34U};
    if (issuance_mode) {
        lxp_hash_context hash;
        uint8_t salt[32] = {8U};
        uint8_t asset_id[32];
        size_t cursor = 2U;
        lxp_hash_init(&hash);
        REQUIRE(lxp_hash_update(&hash, (const uint8_t *)"LX:ASSET:v1", 11U) == LXP_OK);
        REQUIRE(lxp_hash_update(&hash, f->authority.actor, 32U) == LXP_OK);
        REQUIRE(lxp_hash_update(&hash, salt, 32U) == LXP_OK);
        REQUIRE(lxp_hash_final(&hash, asset_id) == LXP_OK);
        (void)memcpy(f->payload + cursor, asset_id, 32U); cursor += 32U;
        (void)memcpy(f->payload + cursor, salt, 32U); cursor += 32U;
        f->payload[cursor++] = 1U; f->payload[cursor++] = 'T';
        f->payload[cursor++] = 1U; f->payload[cursor++] = 'T';
        f->payload[cursor++] = 0U;
        REQUIRE(lxp_u128_to_be((lxp_u128){0U, mode == 2U ? 0U : 100U}, f->payload + cursor) == LXP_OK); cursor += 16U;
        f->payload[cursor++] = 1U; f->payload[cursor++] = 0U;
        f->activity.activity_type = LX_ASSET_REGISTER;
        f->activity.payload.length = cursor;
    }
    REQUIRE(lxp_hash_payload(f->payload, f->activity.payload.length, f->activity.payload_hash) == LXP_OK);
    REQUIRE(lxp_activity_signing_preimage(&f->activity, digest) == LXP_OK);
    REQUIRE(sign_digest(digest, f->signature, f->public_key) == 0);
    REQUIRE(lxp_activity_encode(&f->activity, &f->arena, &encoded) == LXP_OK);
    REQUIRE(lxp_activity_id(encoded.bytes, encoded.length, activity_id) == LXP_OK);
    REQUIRE(lxp_kernel_module_for_activity(&f->kernel, f->activity.activity_type, 0U, &registration) == LXP_OK);
    REQUIRE(lxp_state_root(&f->kernel, before) == LXP_OK);
    REQUIRE(lxp_state_journal_open(&f->state, 1U, &f->journal) == LXP_OK);
    REQUIRE(lxp_module_ctx_init(ctx, &f->kernel, LXP_MODULE_ASSET, 10U, 0U, 1U,
                                10000U, &f->arena, true) == LXP_OK);
    ctx->protocol_version = 3U;
    (void)memcpy(ctx->activity_id, activity_id, 32U);
    REQUIRE(lxp_effect_buffer_init(&effects) == LXP_OK);
    REQUIRE(lxp_module_ctx_bind_effects(ctx, &effects) == LXP_OK);
    REQUIRE(lxp_kernel_dispatch(registration, ctx, &f->activity, &f->authority,
                                &effects, &module_result) == LXP_OK);
    REQUIRE(module_result == LXP_OK);
    if (issuance_mode) {
        REQUIRE(!ctx->ledger_receipt_present && ctx->staged_count == 1U);
        REQUIRE(ctx->staged_accounts[0].account.balance.lo == (mode == 2U ? UINT64_MAX : 100U));
        REQUIRE(ctx->staged_accounts[0].account.balance.hi == (mode == 2U ? UINT64_MAX : 0U));
        REQUIRE(ctx->staged_accounts[0].account.kind == LX_ACCOUNT_MODULE_VALUE);
    } else REQUIRE(ctx->ledger_receipt_present && ctx->ledger_receipt.operation == 4U);
    REQUIRE(f->accounts.count == 2U && ctx->staged_account_count == 1U);
    if (!issuance_mode) {
        lxp_ledger_receipt_input valid = ctx->ledger_receipt;
        for (unsigned mutation = 0U; mutation < 10U; ++mutation) {
            lxp_ledger_receipt_input invalid = valid;
            ctx->ledger_receipt_present = false;
            if (mutation == 0U) invalid.operation = 6U;
            if (mutation == 1U) invalid.operation = 10U;
            if (mutation == 2U) invalid.operation = 11U;
            if (mutation == 3U) invalid.amount.lo = 1U;
            if (mutation == 4U) invalid.asset[0] ^= 1U;
            if (mutation == 5U) invalid.to[0] ^= 1U;
            if (mutation == 6U) invalid.timestamp++;
            if (mutation == 7U) invalid.from_balance_before.lo++;
            if (mutation == 8U) (void)memset(invalid.authorization_hash, 0, 32U);
            if (mutation == 9U) invalid.resulting_state_root[0] = 1U;
            REQUIRE(lxp_ctx_bind_ledger_receipt(ctx, &invalid) != LXP_OK);
            REQUIRE(!ctx->ledger_receipt_present && f->accounts.count == 2U);
        }
        REQUIRE(lxp_ctx_bind_ledger_receipt(ctx, &valid) == LXP_OK);
        lx_account *unexpected;
        REQUIRE(lx_account_registration_commit(&f->accounts, &ctx->staged_accounts[0],
                                               &unexpected) == LXP_FATAL_INVARIANT);
        REQUIRE(f->accounts.count == 2U);
    }
    REQUIRE(lxp_module_ctx_prepare_commit(ctx) == LXP_OK);
    REQUIRE(lxp_module_ctx_preview_state_root(ctx, &f->journal, preview) == LXP_OK);
    REQUIRE(lxp_module_ctx_export_prepared(ctx, &effects, token, &prepared) == LXP_OK);
    lxp_module_ctx_rollback(ctx);
    REQUIRE(lxp_state_journal_rollback(&f->journal) == LXP_OK);
    REQUIRE(lxp_state_root(&f->kernel, committed) == LXP_OK);
    REQUIRE(memcmp(before, committed, 32U) == 0 && f->accounts.count == 2U);
    REQUIRE(lxp_state_journal_open(&f->state, 1U, &f->journal) == LXP_OK);
    REQUIRE(lxp_module_ctx_init(ctx, &f->kernel, LXP_MODULE_ASSET, 10U, 0U, 1U,
                                10000U, &f->arena, true) == LXP_OK);
    ctx->protocol_version = 3U;
    (void)memcpy(ctx->activity_id, activity_id, 32U);
    REQUIRE(lxp_effect_buffer_init(&effects) == LXP_OK);
    REQUIRE(lxp_module_ctx_bind_effects(ctx, &effects) == LXP_OK);
    REQUIRE(lxp_kernel_bind_ledger_admission(ctx, &f->authority, LX_ASSET_MINT) == LXP_OK);
    REQUIRE(lxp_module_ctx_import_prepared(ctx, prepared, token, &effects) == LXP_ERR_CONTEXT_MISMATCH);
    ctx->ledger_admission.activity_type = f->activity.activity_type;
    token[0] ^= 1U;
    REQUIRE(lxp_module_ctx_import_prepared(ctx, prepared, token, &effects) == LXP_ERR_CONTEXT_MISMATCH);
    token[0] ^= 1U;
    REQUIRE(lxp_module_ctx_import_prepared(ctx, prepared, token, &effects) == LXP_OK);
    REQUIRE(ctx->commit_prepared);
    REQUIRE(lxp_module_ctx_preview_state_root(ctx, &f->journal, imported) == LXP_OK);
    REQUIRE(memcmp(preview, imported, 32U) == 0);
    REQUIRE(ctx->commit_prepared);
    REQUIRE(lxp_state_journal_commit(&f->journal) == LXP_OK);
    REQUIRE(lxp_module_ctx_commit(ctx) == LXP_OK);
    REQUIRE(lxp_state_root(&f->kernel, committed) == LXP_OK);
    REQUIRE(memcmp(preview, committed, 32U) == 0 && f->accounts.count == 3U);
    lxp_prepared_module_transition_destroy(prepared);
    REQUIRE(lxp_state_store_destroy(&f->state) == LXP_OK);
    free(ctx); free(f);
    return 0;
}

static int pay1_issuance(uint8_t final_root[32])
{
    fixture *f = (fixture *)calloc(1U, sizeof(*f));
    uint8_t id[32];
    uint8_t salt[32] = {9U};
    uint8_t account_id[32];
    uint8_t registration[128];
    size_t length;
    size_t registration_length;
    lxp_hash_context hash;
    lx_asset_record record;
    REQUIRE(f != NULL);
    REQUIRE(prepare(f, 3U, true) == 0);
    lxp_hash_init(&hash);
    REQUIRE(lxp_hash_update(&hash, (const uint8_t *)"LX:ASSET:v1", 11U) == LXP_OK);
    REQUIRE(lxp_hash_update(&hash, f->authority.actor, 32U) == LXP_OK);
    REQUIRE(lxp_hash_update(&hash, salt, 32U) == LXP_OK);
    REQUIRE(lxp_hash_final(&hash, id) == LXP_OK);
    length = 0U;
    f->payload[length++] = 0U; f->payload[length++] = 1U;
    (void)memcpy(f->payload + length, id, 32U); length += 32U;
    (void)memcpy(f->payload + length, salt, 32U); length += 32U;
    f->payload[length++] = 3U;
    (void)memcpy(f->payload + length, "TOK", 3U); length += 3U;
    f->payload[length++] = 5U;
    (void)memcpy(f->payload + length, "Token", 5U); length += 5U;
    f->payload[length++] = 6U;
    REQUIRE(lxp_u128_to_be((lxp_u128){0U, 100U}, f->payload + length) == LXP_OK); length += 16U;
    f->payload[length++] = 1U; f->payload[length++] = 0U;
    registration_length = length;
    (void)memcpy(registration, f->payload, length);
    REQUIRE(pay1_submit(f, 1U, length, LXP_OK) == 0);
    REQUIRE(f->accounts.count == 3U);
    REQUIRE(f->accounts.accounts[2].balance.lo == 100U);
    REQUIRE(f->kernel.module_kv_count == 1U);
    REQUIRE(lx_asset_record_decode(f->kernel.module_kv[0].value,
        f->kernel.module_kv[0].value_length, &record) == LXP_OK);
    REQUIRE(record.total_units.lo == 0U && record.supply_cap.lo == 100U);
    {
        uint8_t canonical[384];
        size_t canonical_length;
        lx_asset_record decoded;
        REQUIRE(lx_asset_record_encode(&record, canonical, sizeof(canonical), &canonical_length) == LXP_OK);
        for (size_t prefix = 0U; prefix < canonical_length; ++prefix)
            REQUIRE(lx_asset_record_decode(canonical, prefix, &decoded) != LXP_OK);
        canonical[canonical_length] = 0U;
        REQUIRE(lx_asset_record_decode(canonical, canonical_length + 1U, &decoded) != LXP_OK);
        canonical[1] = 1U;
        REQUIRE(lx_asset_record_decode(canonical, canonical_length, &decoded) != LXP_OK);
        canonical[1] = 2U;
        REQUIRE(lx_asset_record_decode(canonical, canonical_length, &decoded) == LXP_OK);
        REQUIRE(memcmp(decoded.name, "Token", 5U) == 0 && decoded.name_length == 5U);
    }
    REQUIRE(memcmp(record.issuer_did32, f->authority.actor, 32U) == 0);
    REQUIRE(pay1_submit(f, 1U, registration_length, LXP_ERR_ASSET_ALREADY_REGISTERED) == 0);
    f->payload[0] = 0U; f->payload[1] = 1U;
    (void)memcpy(f->payload + 2U, id, 32U);
    REQUIRE(pay1_submit(f, 4U, 34U, LXP_OK) == 0);
    REQUIRE(f->receipt.operation == 4U);
    REQUIRE(f->accounts.count == 4U);
    {
        uint8_t ids[4][32];
        size_t count = 99U;
        uint8_t other[32] = {0U};
        REQUIRE(lx_account_list_did(&f->accounts, f->authority.actor, ids, 4U, &count) == LXP_OK);
        REQUIRE(count == 2U && memcmp(ids[0], ids[1], 32U) < 0);
        REQUIRE(lx_account_list_did(&f->accounts, f->authority.actor, ids, 1U, &count) == LXP_ERR_LENGTH_LIMIT);
        REQUIRE(lx_account_list_did(&f->accounts, other, ids, 4U, &count) == LXP_OK && count == 0U);
    }
    REQUIRE(f->accounts.accounts[3].has_authority_key);
    (void)memcpy(account_id, f->accounts.accounts[3].id, 32U);
    REQUIRE(memcmp(f->receipt.to, account_id, 32U) == 0);
    REQUIRE(pay1_submit(f, 4U, 34U, LXP_ERR_CONTEXT_MISMATCH) == 0);
    (void)memcpy(f->payload + 34U, account_id, 32U);
    REQUIRE(lxp_u128_to_be((lxp_u128){0U, 70U}, f->payload + 66U) == LXP_OK);
    REQUIRE(pay1_submit(f, 10U, 82U, LXP_OK) == 0);
    REQUIRE(f->receipt.operation == 10U && f->receipt.from_balance_before.lo == 100U);
    REQUIRE(f->receipt.from_balance_after.lo == 30U && f->receipt.to_balance_after.lo == 70U);
    REQUIRE(f->accounts.accounts[2].balance.lo == 30U && f->accounts.accounts[3].balance.lo == 70U);
    REQUIRE(pay1_submit(f, 10U, 82U, LXP_ERR_INSUFFICIENT_BALANCE) == 0);
    REQUIRE(f->accounts.accounts[2].balance.lo == 30U && f->accounts.accounts[3].balance.lo == 70U);
    REQUIRE(lxp_u128_to_be((lxp_u128){0U, 20U}, f->payload + 66U) == LXP_OK);
    REQUIRE(pay1_submit(f, 11U, 82U, LXP_OK) == 0);
    REQUIRE(f->receipt.operation == 11U && f->receipt.from_balance_before.lo == 70U);
    REQUIRE(f->receipt.from_balance_after.lo == 50U && f->receipt.to_balance_after.lo == 50U);
    REQUIRE(lx_asset_record_decode(f->kernel.module_kv[0].value,
        f->kernel.module_kv[0].value_length, &record) == LXP_OK);
    REQUIRE(record.total_units.lo == 50U);
    REQUIRE(lxp_u128_to_be((lxp_u128){0U, 60U}, f->payload + 66U) == LXP_OK);
    REQUIRE(pay1_submit(f, 11U, 82U, LXP_ERR_UNDERFLOW) == 0);
    REQUIRE(f->accounts.accounts[2].balance.lo == 50U && f->accounts.accounts[3].balance.lo == 50U);
    for (unsigned refusal = 0U; refusal < 5U; ++refusal) {
        lxp_result expected = LXP_ERR_ASSET_MISMATCH;
        (void)memcpy(f->payload + 2U, id, 32U);
        (void)memcpy(f->payload + 34U, account_id, 32U);
        REQUIRE(lxp_u128_to_be((lxp_u128){0U, 1U}, f->payload + 66U) == LXP_OK);
        if (refusal == 0U) f->payload[2U] ^= 1U;
        if (refusal == 1U) { f->payload[34U] ^= 1U; expected = LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE; }
        if (refusal == 2U) (void)memcpy(f->payload + 34U, f->accounts.accounts[0].id, 32U);
        if (refusal == 3U) { f->accounts.accounts[3].frozen = true; expected = LXP_ERR_ACCOUNT_FROZEN; }
        if (refusal == 4U) { (void)memset(f->payload + 66U, 0, 16U); expected = LXP_ERR_INVALID_AMOUNT; }
        REQUIRE(pay1_submit(f, 10U, 82U, expected) == 0);
        REQUIRE(f->accounts.accounts[2].balance.lo == 50U && f->accounts.accounts[3].balance.lo == 50U);
        f->accounts.accounts[3].frozen = false;
    }
    {
        static const uint8_t other_did[] = "did:key:bob";
        lxp_identity *other;
        uint8_t issuer[32];
        (void)memcpy(issuer, f->authority.actor, 32U);
        REQUIRE(lxp_identity_register(&f->identities, other_did, sizeof(other_did) - 1U,
                                      f->public_key, &other) == LXP_OK);
        f->activity.actor_did = (lxp_byte_span){other_did, sizeof(other_did) - 1U};
        (void)memcpy(f->authority.actor, other->did_id, 32U);
        (void)memcpy(f->payload + 2U, id, 32U);
        (void)memcpy(f->payload + 34U, account_id, 32U);
        REQUIRE(lxp_u128_to_be((lxp_u128){0U, 1U}, f->payload + 66U) == LXP_OK);
        REQUIRE(pay1_submit(f, 10U, 82U, LXP_ERR_UNAUTHORIZED_DEBIT) == 0);
        REQUIRE(pay1_submit(f, 11U, 82U, LXP_ERR_UNAUTHORIZED_DEBIT) == 0);
        REQUIRE(f->accounts.accounts[2].balance.lo == 50U && f->accounts.accounts[3].balance.lo == 50U);
        f->activity.actor_did = (lxp_byte_span){did, sizeof(did) - 1U};
        (void)memcpy(f->authority.actor, issuer, 32U);
    }
    REQUIRE(lxp_state_root(&f->kernel, final_root) == LXP_OK);
    REQUIRE(lxp_state_store_destroy(&f->state) == LXP_OK);
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
    {
        uint8_t first[32], replay[32];
        REQUIRE(pay1_issuance(first) == 0);
        REQUIRE(pay1_issuance(replay) == 0);
        REQUIRE(memcmp(first, replay, 32U) == 0);
    }
    REQUIRE(pay1_receive_grant() == 0);
    REQUIRE(pay1_prepared_account(0U) == 0);
    REQUIRE(pay1_prepared_account(1U) == 0);
    REQUIRE(pay1_prepared_account(2U) == 0);
    for (unsigned i = 0U; i < 6U; ++i) REQUIRE(asset_send(i) == 0);
    (void)puts("state commitment transition: legacy, version 3, preview, signatures and tampering passed");
    return 0;
}
