#include "layerx/lxp_module_ctx.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_protocol.h"
#include <openssl/evp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(expression) do { if (!(expression)) { (void)fprintf(stderr, "rotation assertion line %d: %s\n", __LINE__, #expression); return 1; } } while (0)

static const uint8_t owner_did[] = "did:layerx:rotation-owner";
typedef struct rotation_fixture {
    lxp_state_store store;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_fee_params parameters;
    lxp_identity_store identities;
    lx_account_registry accounts;
    lxp_module_ctx ctx;
    lxp_effect_buffer effects;
    lxp_arena arena;
    uint8_t arena_bytes[2U * LXP_MAX_ACTIVITY_BYTES];
    uint8_t previous[223];
    uint8_t old_public[32];
    uint8_t new_public[32];
    uint8_t other_public[32];
    EVP_PKEY *old_key;
    EVP_PKEY *new_key;
    EVP_PKEY *other_key;
} rotation_fixture;

static void number(uint8_t *out, uint64_t value)
{ for (size_t i = 0U; i < 8U; ++i) out[7U - i] = (uint8_t)(value >> (8U * i)); }

static int key(uint8_t seed_byte, EVP_PKEY **key_out, uint8_t public_key[32])
{
    uint8_t seed[32]; size_t length = 32U;
    (void)memset(seed, seed_byte, sizeof(seed));
    *key_out = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, seed, sizeof(seed));
    CHECK(*key_out != NULL && EVP_PKEY_get_raw_public_key(*key_out, public_key, &length) == 1 && length == 32U);
    return 0;
}

static int account(rotation_fixture *fixture, const char *name, const uint8_t public_key[32])
{
    lx_account value = {0}; size_t slot;
    value.name_length = (uint16_t)strlen(name);
    (void)memcpy(value.name, name, value.name_length);
    CHECK(lx_account_kind_of(value.name, value.name_length, &value.kind) == LXP_OK);
    CHECK(lx_account_id_from_string(value.name, value.name_length, value.id) == LXP_OK);
    value.has_asset = true; value.asset_id[0] = 5U;
    value.balance.lo = 100U; value.created_at_sequence = 1U;
    value.has_authority_key = true; (void)memcpy(value.authority_key, public_key, 32U);
    CHECK(lx_account_registry_slot_insert(&fixture->accounts, &value, &slot) == LXP_OK);
    return 0;
}

static int fixture_init(rotation_fixture *fixture)
{
    lxp_identity *identity;
    CHECK(key(1U, &fixture->old_key, fixture->old_public) == 0 &&
        key(2U, &fixture->new_key, fixture->new_public) == 0 &&
        key(3U, &fixture->other_key, fixture->other_public) == 0);
    CHECK(lxp_state_store_init(&fixture->store, 2U) == LXP_OK &&
        lx_account_registry_init(&fixture->accounts) == LXP_OK);
    CHECK(account(fixture, "agent:did:layerx:rotation-owner:main", fixture->old_public) == 0);
    CHECK(account(fixture, "agent:did:layerx:rotation-owner:asset:0500000000000000000000000000000000000000000000000000000000000000", fixture->old_public) == 0);
    CHECK(account(fixture, "agent:did:layerx:unrelated:main", fixture->other_public) == 0);
    CHECK(lxp_state_store_bind_accounts(&fixture->store, &fixture->accounts) == LXP_OK &&
        lxp_state_store_require_account_root(&fixture->store) == LXP_OK);
    fixture->parameters.version = 1U;
    fixture->parameters.multiplier_basis_points = 10000U;
    CHECK(lxp_kernel_create(&fixture->kernel, &fixture->store, &fixture->journal, &fixture->parameters, 1U) == LXP_OK &&
        lxp_kernel_register_module(&fixture->kernel, lxp_governance_module_iface()) == LXP_OK);
    CHECK(lxp_identity_register(&fixture->identities, owner_did, sizeof(owner_did) - 1U,
        fixture->old_public, &identity) == LXP_OK);
    (void)memcpy(fixture->previous, "LXGI1", 5U);
    (void)memcpy(fixture->previous + 5U, identity->did_id, 32U);
    (void)memcpy(fixture->previous + 37U, fixture->old_public, 32U);
    (void)memcpy(fixture->previous + 111U, fixture->new_public, 32U);
    number(fixture->previous + 69U, 1U); number(fixture->previous + 143U, 900U);
    number(fixture->previous + 151U, 2000U); number(fixture->previous + 159U, 2U);
    number(fixture->previous + 167U, 1U); number(fixture->previous + 215U, 1U);
    lxp_module_kv_entry *entry = &fixture->kernel.module_kv[fixture->kernel.module_kv_count++];
    entry->module_id = LXP_MODULE_GOVERNANCE; entry->key_length = 32U; entry->value_length = 223U;
    (void)memcpy(entry->key, identity->did_id, 32U); (void)memcpy(entry->value, fixture->previous, 223U);
    CHECK(lxp_governance_identity_refresh(&fixture->kernel, identity) == LXP_OK);
    return 0;
}

static int begin(rotation_fixture *fixture)
{
    CHECK(lxp_arena_init(&fixture->arena, fixture->arena_bytes, sizeof(fixture->arena_bytes)) == LXP_OK);
    CHECK(lxp_module_ctx_init(&fixture->ctx, &fixture->kernel, LXP_MODULE_GOVERNANCE,
        1000U, 1U, 2U, 100000U, &fixture->arena, true) == LXP_OK);
    fixture->ctx.identities = &fixture->identities;
    CHECK(lxp_effect_buffer_init(&fixture->effects) == LXP_OK &&
        lxp_module_ctx_bind_effects(&fixture->ctx, &fixture->effects) == LXP_OK &&
        lxp_state_journal_open(&fixture->store, 2U, &fixture->journal) == LXP_OK);
    return 0;
}

static int encode(EVP_PKEY *key_value, const uint8_t public_key[32], const uint8_t *payload,
    size_t payload_length, uint8_t *output, size_t *length)
{
    lxp_activity activity = {0}; lxp_arena arena; lxp_byte_span encoded;
    uint8_t storage[LXP_MAX_ACTIVITY_BYTES], digest[32], signature[64]; size_t signature_length = sizeof(signature);
    activity.protocol_version = 3U; activity.network_id = 77U; activity.activity_type = 0x00070002U;
    activity.actor_did = (lxp_byte_span){owner_did, sizeof(owner_did) - 1U};
    activity.authority = (lxp_byte_span){public_key, 32U}; activity.idempotency_key[0] = 4U;
    activity.timestamp_bound.not_before = 900U; activity.timestamp_bound.not_after = 2000U;
    activity.payload = (lxp_byte_span){payload, payload_length};
    CHECK(lxp_hash_payload(payload, payload_length, activity.payload_hash) == LXP_OK);
    CHECK(lxp_activity_signing_preimage(&activity, digest) == LXP_OK);
    EVP_MD_CTX *signing = EVP_MD_CTX_new();
    CHECK(signing != NULL && EVP_DigestSignInit(signing, NULL, NULL, NULL, key_value) == 1 &&
        EVP_DigestSign(signing, signature, &signature_length, digest, sizeof(digest)) == 1 && signature_length == sizeof(signature));
    EVP_MD_CTX_free(signing);
    activity.signature = (lxp_byte_span){signature, sizeof(signature)};
    CHECK(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK &&
        lxp_activity_encode(&activity, &arena, &encoded) == LXP_OK);
    (void)memcpy(output, encoded.bytes, encoded.length); *length = encoded.length;
    return 0;
}

static int stage(rotation_fixture *fixture)
{
    uint8_t consent[140] = {0x71U, 2U, 2U, 5U}, wrapper[1024] = {0x71U, 2U, 1U, 1U};
    uint8_t inner[1024], outer[2048]; size_t inner_length, outer_length;
    lxp_activity activity; lxp_authority_resolved authority = {0}; void *decoded = NULL;
    (void)memcpy(consent + 4U, fixture->previous + 5U, 32U);
    (void)memcpy(consent + 36U, fixture->old_public, 32U);
    CHECK(lxp_hash_context_value(fixture->previous, 223U, consent + 68U) == LXP_OK);
    consent[100U] = 4U; number(consent + 132U, 2000U);
    CHECK(encode(fixture->new_key, fixture->new_public, consent, sizeof(consent), inner, &inner_length) == 0);
    CHECK(inner_length < sizeof(wrapper) - 8U);
    for (size_t i = 0U; i < 4U; ++i) wrapper[7U - i] = (uint8_t)(inner_length >> (8U * i));
    (void)memcpy(wrapper + 8U, inner, inner_length);
    CHECK(encode(fixture->old_key, fixture->old_public, wrapper, inner_length + 8U, outer, &outer_length) == 0);
    CHECK(lxp_activity_decode(outer, outer_length, &activity) == LXP_OK &&
        lxp_activity_id(outer, outer_length, fixture->ctx.activity_id) == LXP_OK);
    authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(authority.actor, fixture->previous + 5U, 32U);
    (void)memcpy(authority.verified_key, fixture->old_public, 32U);
    const lxp_module_iface *module = lxp_governance_module_iface();
    CHECK(module->decode(&fixture->ctx, 2U, wrapper, inner_length + 8U, &decoded) == LXP_OK &&
        module->validate(&fixture->ctx, &activity, &authority, decoded) == LXP_OK &&
        module->execute(&fixture->ctx, &activity, &authority, decoded, &fixture->effects) == LXP_OK);
    return 0;
}

int main(void)
{
    rotation_fixture *fixture = calloc(1U, sizeof(*fixture));
    uint8_t before[32], preview[32], after[32];
    CHECK(fixture != NULL && fixture_init(fixture) == 0 && lxp_state_root(&fixture->kernel, before) == LXP_OK);
    CHECK(begin(fixture) == 0 && stage(fixture) == 0);
    CHECK(memcmp(fixture->accounts.accounts[0].authority_key, fixture->old_public, 32U) == 0 &&
        memcmp(fixture->accounts.accounts[1].authority_key, fixture->old_public, 32U) == 0);
    CHECK(lxp_module_ctx_prepare_commit(&fixture->ctx) == LXP_OK &&
        lxp_module_ctx_preview_state_root(&fixture->ctx, &fixture->journal, preview) == LXP_OK &&
        memcmp(before, preview, 32U) != 0);
    lxp_module_ctx_rollback(&fixture->ctx);
    CHECK(lxp_state_journal_rollback(&fixture->journal) == LXP_OK &&
        lxp_state_root(&fixture->kernel, after) == LXP_OK && memcmp(before, after, 32U) == 0);
    CHECK(begin(fixture) == 0 && stage(fixture) == 0);
    lxp_module_ctx *ctx = &fixture->ctx;
    CHECK(ctx->staged_count == 2U);
    ctx->staged[0].value[69U] ^= 1U;
    CHECK(lxp_module_ctx_prepare_commit(ctx) == LXP_ERR_AUTH_SCOPE);
    ctx->staged[0].value[69U] ^= 1U;
    ctx->staged[0].value[109U] ^= 1U;
    CHECK(lxp_module_ctx_prepare_commit(ctx) == LXP_ERR_AUTH_SCOPE);
    ctx->staged[0].value[109U] ^= 1U;
    ctx->staged[1].value[80U] ^= 1U;
    CHECK(lxp_module_ctx_prepare_commit(ctx) == LXP_ERR_AUTH_SCOPE);
    ctx->staged[1].value[80U] ^= 1U;
    (void)memcpy(fixture->accounts.accounts[1].authority_key, fixture->other_public, 32U);
    CHECK(lxp_module_ctx_prepare_commit(ctx) == LXP_ERR_AUTH_SCOPE);
    (void)memcpy(fixture->accounts.accounts[1].authority_key, fixture->old_public, 32U);
    CHECK(lxp_module_ctx_prepare_commit(ctx) == LXP_OK &&
        lxp_module_ctx_preview_state_root(ctx, &fixture->journal, preview) == LXP_OK &&
        lxp_state_journal_commit(&fixture->journal) == LXP_OK && lxp_module_ctx_commit(ctx) == LXP_OK &&
        lxp_state_root(&fixture->kernel, after) == LXP_OK && memcmp(preview, after, 32U) == 0);
    CHECK(memcmp(fixture->accounts.accounts[0].authority_key, fixture->new_public, 32U) == 0 &&
        memcmp(fixture->accounts.accounts[1].authority_key, fixture->new_public, 32U) == 0 &&
        memcmp(fixture->accounts.accounts[2].authority_key, fixture->other_public, 32U) == 0);
    const lxp_identity *identity = &fixture->identities.identities[0];
    CHECK(memcmp(identity->primary_key, fixture->new_public, 32U) == 0 && !identity->has_pending_key &&
        identity->has_superseded_key && identity->revocation_sequence == 2U && identity->rotation_effective_sequence == 2U);
    CHECK(lxp_identity_key_valid(identity, fixture->old_public, 1000U, 1U) &&
        !lxp_identity_key_valid(identity, fixture->old_public, 1000U, 2U) &&
        lxp_identity_key_valid(identity, fixture->new_public, 1000U, 2U));
    EVP_PKEY_free(fixture->old_key); EVP_PKEY_free(fixture->new_key); EVP_PKEY_free(fixture->other_key);
    CHECK(lxp_state_store_destroy(&fixture->store) == LXP_OK);
    lx_account_registry_release(&fixture->accounts); free(fixture);
    puts("two-key native rotation previews and commits exact account roots, preserves unrelated owners and rolls back staged changes");
    return 0;
}
