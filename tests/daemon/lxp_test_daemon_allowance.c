#include "lxp_daemon_allowance.h"

#include "layerx/lx_asset.h"
#include "layerx/lx_stream.h"
#include "layerx/lxp_activity.h"
#include "layerx/lxp_authority.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_fee.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_state.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "daemon allowance check failed at line %d\n", __LINE__); \
    return 1; } } while (0)

enum { STREAM_ACCOUNTS = 9 };

/* The daemon's execution shape for one activity: the kernel the genesis plan
 * builds with the stream module and the canonical ledger applier, the actor's
 * identity, and grants resolved from governance records. Every activity is
 * executed exactly as cmd/layerxd/lxp_daemon_process.c executes it, with the
 * allowance lxp_daemon_live_allowance derives from the resolved grant. */
typedef struct fixture {
    lxp_kernel kernel;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_identity_store identities;
    lx_account_registry accounts;
    lx_asset_record asset;
    lxp_transfer_asset_state asset_state;
    lx_stream_runtime runtime;
    lxp_arena arena;
    uint8_t arena_bytes[4U * 1024U * 1024U];
    uint64_t parameters;
    lxp_fee_params fees;
    lxp_identity *identity;
    lx_account *payer;
    lx_account *provider;
    lx_account *streams[STREAM_ACCOUNTS];
    uint8_t owner_public[32];
    uint8_t delegate_public[32];
    uint8_t foreign_public[32];
    uint8_t sequencer_public[32];
    uint8_t signature[64];
    uint8_t payload[LX_STREAM_OPEN_PAYLOAD_MAX];
    lxp_activity activity;
    lxp_receipt receipt;
} fixture;

/* Who signs an activity: the identity's owner key, the delegate the metered
 * grant names, or a key whose grant is bound to another asset. */
typedef struct signer {
    const uint8_t *seed;
    const uint8_t *public_key;
} signer;

static const uint8_t owner_seed[32] = { 1U };
static const uint8_t delegate_seed[32] = { 2U };
static const uint8_t foreign_seed[32] = { 3U };
static const uint8_t sequencer_seed[32] = { 9U };
static const uint8_t did[] = "did:key:payer";
static const char payer_name[] = "agent:did:key:payer:main";
static const char provider_name[] = "agent:did:key:provider:main";
static const char *const stream_names[STREAM_ACCOUNTS] = {
    "agent:did:key:payer:stream:s1", "agent:did:key:payer:stream:s2",
    "agent:did:key:payer:stream:s3", "agent:did:key:payer:stream:s4",
    "agent:did:key:payer:stream:s5", "agent:did:key:payer:stream:s6",
    "agent:did:key:payer:stream:s7", "agent:did:key:payer:stream:s8",
    "agent:did:key:payer:stream:s9"
};

static int public_from_seed(const uint8_t seed[32], uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                 seed, 32U);
    size_t length = 32U;
    int ok = key != NULL &&
        EVP_PKEY_get_raw_public_key(key, public_key, &length) == 1 &&
        length == 32U;
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static int sign_digest(const uint8_t seed[32], const uint8_t digest[32],
                       uint8_t signature[64])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                 seed, 32U);
    EVP_MD_CTX *ctx = EVP_MD_CTX_new();
    size_t signature_length = 64U;
    int ok = key != NULL && ctx != NULL &&
        EVP_DigestSignInit(ctx, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(ctx, signature, &signature_length, digest, 32U) == 1 &&
        signature_length == 64U;
    EVP_MD_CTX_free(ctx);
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static int open_account(fixture *f, const char *name, lx_account **account)
{
    uint8_t id[32];
    size_t length = strlen(name);
    CHECK(lx_account_id_from_string((const uint8_t *)name, length, id) ==
          LXP_OK);
    CHECK(lx_account_open(&f->accounts, (const uint8_t *)name, length, id, 1U,
                          LX_ACCOUNT_OPEN_CREDIT, NULL, account) == LXP_OK);
    return 0;
}

static bool balance_is(const lx_account *account, uint64_t expected)
{
    return account->balance.hi == 0U && account->balance.lo == expected;
}

static int fixture_init(fixture *f)
{
    size_t i;
    CHECK(public_from_seed(owner_seed, f->owner_public) == 0);
    CHECK(public_from_seed(delegate_seed, f->delegate_public) == 0);
    CHECK(public_from_seed(foreign_seed, f->foreign_public) == 0);
    CHECK(public_from_seed(sequencer_seed, f->sequencer_public) == 0);
    CHECK(lxp_arena_init(&f->arena, f->arena_bytes, sizeof(f->arena_bytes)) ==
          LXP_OK);
    (void)memset(&f->asset, 0, sizeof(f->asset));
    f->asset.asset_id[0] = 6U;
    CHECK(lx_asset_transfer_state(&f->asset, &f->asset_state) == LXP_OK);
    f->runtime.assets = &f->asset_state;
    f->runtime.asset_count = 1U;
    CHECK(lx_account_registry_init(&f->accounts) == LXP_OK);
    CHECK(open_account(f, payer_name, &f->payer) == 0);
    CHECK(open_account(f, provider_name, &f->provider) == 0);
    for (i = 0U; i < STREAM_ACCOUNTS; ++i) {
        CHECK(open_account(f, stream_names[i], &f->streams[i]) == 0);
        CHECK(f->streams[i]->kind == LX_ACCOUNT_AGENT_STREAM);
    }
    CHECK(f->payer->kind == LX_ACCOUNT_AGENT_MAIN);
    CHECK(f->provider->kind == LX_ACCOUNT_AGENT_MAIN);
    CHECK(lxp_ledger_bootstrap_balance(f->payer, f->asset.asset_id,
                                       (lxp_u128){ 0U, 100U }, 0U) == LXP_OK);
    f->payer->has_authority_key = true;
    (void)memcpy(f->payer->authority_key, f->owner_public, 32U);
    CHECK(lxp_state_store_init(&f->state, 1U) == LXP_OK);
    CHECK(lxp_state_store_bind_accounts(&f->state, &f->accounts) == LXP_OK);
    f->parameters = 1U;
    CHECK(lxp_kernel_create(&f->kernel, &f->state, &f->journal,
                            &f->parameters, 0U) == LXP_OK);
    CHECK(lxp_kernel_set_capabilities(&f->kernel, NULL,
                                      lxp_kernel_canonical_ledger_apply) ==
          LXP_OK);
    CHECK(lxp_kernel_register_module(&f->kernel, lx_stream_module_iface()) ==
          LXP_OK);
    CHECK(lxp_kernel_bind_module_runtime(&f->kernel, LXP_MODULE_STREAM,
                                         &f->runtime) == LXP_OK);
    CHECK(lxp_identity_register(&f->identities, did, sizeof(did) - 1U,
                                f->owner_public, &f->identity) == LXP_OK);
    /* Metered grants bind to the grantor's revocation sequence, which the
     * governance identity record has advanced past zero before any grant is
     * issued. */
    f->identity->revocation_sequence = 1U;
    f->fees.version = 1U;
    f->fees.multiplier_basis_points = 10000U;
    return 0;
}

/* A delegated capability the identity granted over one asset to the given
 * key: at most 50 per stream opening and 70 in total. */
static void metered_grant(const fixture *f, lxp_authority_grant *grant,
                          const uint8_t key[32], uint8_t asset_marker)
{
    (void)memset(grant, 0, sizeof(*grant));
    (void)memcpy(grant->grantor, f->identity->did_id, 32U);
    (void)memcpy(grant->grantee, f->identity->did_id, 32U);
    grant->kind = LXP_AUTHORITY_DELEGATED_CAPABILITY;
    (void)memcpy(grant->key, key, 32U);
    grant->scope.module_mask = UINT64_C(1) << LXP_MODULE_STREAM;
    grant->scope.activity_ordinal_min = 1U;
    grant->scope.activity_ordinal_max = 1U;
    grant->scope.asset_id[0] = asset_marker;
    grant->scope.maximum_per_activity = (lxp_u128){ 0U, 50U };
    grant->scope.maximum_total = (lxp_u128){ 0U, 70U };
    grant->scope.purpose_hash[0] = 0x77U;
    grant->not_before = 1U;
    grant->not_after = 1000U;
    grant->grantor_revocation_sequence = f->identity->revocation_sequence;
}

/* Publishes a grant as the governance record the authority store resolves
 * activities against, as a committed grant activity leaves it. */
static int seed_grant(fixture *f, lxp_authority_grant *grant)
{
    lxp_byte_span encoded;
    lxp_module_kv_entry *entry;
    uint8_t grant_id[32];
    CHECK(lxp_grant_id_compute(grant, grant_id) == LXP_OK);
    (void)memcpy(grant->grant_id, grant_id, 32U);
    CHECK(lxp_grant_encode(grant, &f->arena, &encoded) == LXP_OK);
    CHECK(encoded.length <= (size_t)LXP_MODULE_MAX_VALUE_BYTES);
    CHECK(f->kernel.module_kv_count < (size_t)LXP_KERNEL_MAX_MODULE_KV);
    entry = &f->kernel.module_kv[f->kernel.module_kv_count++];
    (void)memset(entry, 0, sizeof(*entry));
    entry->module_id = LXP_MODULE_GOVERNANCE;
    entry->key_length = 33U;
    entry->key[0] = 5U;
    (void)memcpy(entry->key + 1U, grant->grant_id, 32U);
    entry->value_length = (uint32_t)encoded.length;
    (void)memcpy(entry->value, encoded.bytes, encoded.length);
    return 0;
}

/* A signed stream opening that funds the given stream account from the
 * actor's main account. The marker keys both the stream and the activity. */
static int build_activity(fixture *f, uint8_t marker,
                          const lx_account *stream_account, uint64_t funding,
                          const signer *who)
{
    lx_stream_open_payload payload;
    size_t payload_length = 0U;
    uint8_t digest[32];
    (void)memset(&payload, 0, sizeof(payload));
    payload.record.stream_id[0] = marker;
    (void)memcpy(payload.record.stream_account, stream_account->id, 32U);
    (void)memcpy(payload.record.recipient, f->provider->id, 32U);
    (void)memcpy(payload.record.asset_id, f->asset.asset_id, 32U);
    payload.record.mode = LX_STREAM_MODE_TIME;
    payload.record.rate = (lxp_u128){ 0U, 10U };
    payload.record.rate_unit = 1000U;
    payload.record.start_timestamp = 10U;
    payload.record.total_cap = (lxp_u128){ 0U, 500U };
    payload.initial_funding = (lxp_u128){ 0U, funding };
    CHECK(lx_stream_open_encode(&payload, f->payload, sizeof(f->payload),
                                &payload_length) == LXP_OK);
    (void)memset(&f->activity, 0, sizeof(f->activity));
    f->activity.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    f->activity.network_id = 7U;
    f->activity.activity_type = LX_STREAM_OPEN;
    f->activity.actor_did = (lxp_byte_span){ did, sizeof(did) - 1U };
    f->activity.authority = (lxp_byte_span){ who->public_key, 32U };
    f->activity.account_sequence = f->identity->next_sequence;
    f->activity.timestamp_bound = (lxp_timestamp_bound){ 1U, 100U };
    f->activity.idempotency_key[0] = marker;
    f->activity.payload = (lxp_byte_span){ f->payload, payload_length };
    f->activity.signature = (lxp_byte_span){ f->signature, 64U };
    CHECK(lxp_hash_payload(f->payload, payload_length,
                           f->activity.payload_hash) == LXP_OK);
    CHECK(lxp_activity_signing_preimage(&f->activity, digest) == LXP_OK);
    CHECK(sign_digest(who->seed, digest, f->signature) == 0);
    CHECK(lxp_activity_verify_signature(&f->activity) == LXP_OK);
    return 0;
}

/* Resolves the built activity's authority the way the daemon does before
 * execution, including binding the principal to the actor's main account. */
static int resolve(fixture *f, lxp_authority_grant *grant,
                   lxp_authority_resolved *resolved)
{
    (void)memset(grant, 0, sizeof(*grant));
    (void)memset(resolved, 0, sizeof(*resolved));
    CHECK(lxp_authority_resolve_activity(
              &f->kernel, f->identity, &f->activity,
              lxp_identity_key_valid(f->identity, f->activity.authority.bytes,
                                     10U, f->state.next_sequence),
              true, 10U, 100U, f->state.next_sequence, grant, resolved) ==
          LXP_OK);
    (void)memcpy(resolved->principal, f->payer->id, 32U);
    return 0;
}

/* The execution the daemon hands the kernel for the built activity: the
 * resolved authority and the live allowance derived from its grant. */
static int prepare_execution(fixture *f, lxp_authority_grant *grant,
                             const lxp_authority_resolved *resolved,
                             lxp_transfer_allowance *allowance,
                             lxp_kernel_execution *execution)
{
    lxp_daemon_live_allowance(grant, resolved, allowance);
    CHECK(allowance->scope == &grant->scope);
    CHECK(allowance->kind == grant->kind);
    CHECK(memcmp(allowance->grantor, f->payer->id, 32U) == 0);
    CHECK(memcmp(allowance->grant_id, grant->grant_id, 32U) == 0);
    (void)memset(execution, 0, sizeof(*execution));
    execution->network_id = 7U;
    execution->batch_number = 1U;
    execution->batch_timestamp_ms = 10U;
    execution->maximum_timestamp_window = 100U;
    execution->global_sequence = f->state.next_sequence;
    execution->recorded_module_version =
        lx_stream_module_iface()->abi_version;
    execution->parameter_version = 1U;
    execution->signature_valid = true;
    execution->identities = &f->identities;
    execution->authority = resolved;
    execution->allowance = allowance;
    execution->fee_parameters = &f->fees;
    execution->gas_limit = 10000U;
    execution->arena = &f->arena;
    execution->sequencer_private_key = sequencer_seed;
    execution->batch_id[0] = 5U;
    (void)memset(&f->receipt, 0, sizeof(f->receipt));
    CHECK(lxp_state_root(&f->kernel, f->kernel.current_state_root) == LXP_OK);
    return 0;
}

/* Executes the built activity through the kernel with the daemon's live
 * allowance and requires the receipt the daemon would publish. */
static int execute(fixture *f, lxp_authority_grant *grant,
                   const lxp_authority_resolved *resolved,
                   lxp_result expected)
{
    lxp_kernel_execution execution;
    lxp_transfer_allowance allowance;
    uint8_t root[32];
    uint64_t sequence_before = f->identity->next_sequence;
    CHECK(prepare_execution(f, grant, resolved, &allowance, &execution) == 0);
    CHECK(lxp_kernel_execute_activity(&f->kernel, &f->activity, &execution,
                                      &f->receipt) == LXP_OK);
    if (f->receipt.result_code != expected)
        (void)fprintf(stderr, "receipt %d expected %d\n",
                      f->receipt.result_code, expected);
    CHECK(f->receipt.result_code == expected);
    CHECK(lxp_receipt_verify(&f->receipt, f->sequencer_public, &f->arena) ==
          LXP_OK);
    CHECK(lxp_state_root(&f->kernel, root) == LXP_OK);
    CHECK(memcmp(root, f->receipt.resulting_state_root, 32U) == 0);
    CHECK(f->identity->next_sequence == sequence_before + 1U);
    return 0;
}

/* Executes the built activity when the kernel cannot commit the module's
 * effects after the module has already charged the allowance: no receipt is
 * produced, no sequence is consumed, and the kernel unwinds the activity. */
static int execute_refused_commit(fixture *f, lxp_authority_grant *grant,
                                  const lxp_authority_resolved *resolved)
{
    lxp_kernel_execution execution;
    lxp_transfer_allowance allowance;
    uint64_t sequence_before = f->identity->next_sequence;
    uint64_t global_before = f->state.next_sequence;
    CHECK(prepare_execution(f, grant, resolved, &allowance, &execution) == 0);
    CHECK(lxp_kernel_execute_activity(&f->kernel, &f->activity, &execution,
                                      &f->receipt) == LXP_ERR_ARENA_EXHAUSTED);
    CHECK(f->identity->next_sequence == sequence_before);
    CHECK(f->state.next_sequence == global_before);
    return 0;
}

/* Fills the module store to capacity with inert governance entries, so the
 * next commit has no room for the stream record an opening stages. */
static void fill_module_store(fixture *f)
{
    size_t i;
    for (i = f->kernel.module_kv_count; i < (size_t)LXP_KERNEL_MAX_MODULE_KV;
         ++i) {
        lxp_module_kv_entry *entry = &f->kernel.module_kv[i];
        size_t byte;
        (void)memset(entry, 0, sizeof(*entry));
        entry->module_id = LXP_MODULE_GOVERNANCE;
        entry->key_length = 9U;
        entry->key[0] = 0xF0U;
        for (byte = 0U; byte < 8U; ++byte)
            entry->key[1U + byte] = (uint8_t)(i >> (56U - 8U * byte));
        entry->value_length = 1U;
        entry->value[0] = 1U;
    }
    f->kernel.module_kv_count = (size_t)LXP_KERNEL_MAX_MODULE_KV;
}

/* Reads the charge record the kernel persists beside a grant, if any. */
static int charge_record(const fixture *f, const uint8_t grant_id[32],
                         bool *present, lxp_authority_scope *scope)
{
    uint8_t key[LXP_AUTHORITY_CHARGE_RECORD_KEY_BYTES];
    size_t i;
    lxp_authority_charge_record_key(grant_id, key);
    *present = false;
    (void)memset(scope, 0, sizeof(*scope));
    for (i = 0U; i < f->kernel.module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &f->kernel.module_kv[i];
        if (entry->module_id != LXP_MODULE_GOVERNANCE ||
            entry->key_length != sizeof(key) ||
            memcmp(entry->key, key, sizeof(key)) != 0)
            continue;
        CHECK(!*present);
        CHECK(lxp_authority_charge_record_decode(
                  entry->value, entry->value_length, grant_id, scope) ==
              LXP_OK);
        *present = true;
    }
    return 0;
}

/* Requires the persisted state of a grant: no charge record and a zero
 * committed counter, or a record carrying exactly the given spent total that
 * a later resolution overlays onto the grant. */
static int persisted_spent(const fixture *f, const uint8_t grant_id[32],
                           bool expected_present, uint64_t expected_spent)
{
    lxp_authority_scope scope;
    lxp_authority_grant loaded;
    bool present;
    CHECK(charge_record(f, grant_id, &present, &scope) == 0);
    CHECK(present == expected_present);
    if (present)
        CHECK(scope.spent_total.hi == 0U &&
              scope.spent_total.lo == expected_spent);
    CHECK(lxp_authority_grant_load(&f->kernel, grant_id, &loaded) == LXP_OK);
    CHECK(loaded.scope.spent_total.hi == 0U &&
          loaded.scope.spent_total.lo == expected_spent);
    return 0;
}

static int allowance_end_to_end(void)
{
    fixture *f = (fixture *)calloc(1U, sizeof(*f));
    lxp_authority_grant owner;
    lxp_authority_grant live;
    lxp_authority_grant stale;
    lxp_authority_grant foreign;
    lxp_authority_grant fresh;
    lxp_authority_resolved resolved;
    lxp_authority_resolved stale_resolved;
    signer owner_signer;
    signer delegate_signer;
    signer foreign_signer;
    uint8_t live_id[32];
    uint8_t foreign_id[32];
    size_t store_count;
    CHECK(f != NULL);
    CHECK(fixture_init(f) == 0);
    owner_signer = (signer){ owner_seed, f->owner_public };
    delegate_signer = (signer){ delegate_seed, f->delegate_public };
    foreign_signer = (signer){ foreign_seed, f->foreign_public };
    metered_grant(f, &live, f->delegate_public, 6U);
    CHECK(seed_grant(f, &live) == 0);
    (void)memcpy(live_id, live.grant_id, 32U);
    metered_grant(f, &foreign, f->foreign_public, 9U);
    CHECK(seed_grant(f, &foreign) == 0);
    (void)memcpy(foreign_id, foreign.grant_id, 32U);

    /* The owner key resolves to the synthesized owner grant. The allowance
     * the daemon derives from it is presented and binds the debit, but an
     * owner scope is not metered: nothing is charged or persisted. */
    CHECK(build_activity(f, 1U, f->streams[0], 10U, &owner_signer) == 0);
    CHECK(resolve(f, &owner, &resolved) == 0);
    CHECK(resolved.kind == LXP_AUTHORITY_OWNER);
    CHECK(execute(f, &owner, &resolved, LXP_OK) == 0);
    CHECK(balance_is(f->payer, 90U) && balance_is(f->streams[0], 10U));
    CHECK(f->payer->next_sequence == 1U);
    CHECK(lxp_u128_is_zero(owner.scope.spent_total));
    CHECK(persisted_spent(f, live_id, false, 0U) == 0);
    CHECK(persisted_spent(f, foreign_id, false, 0U) == 0);

    /* A funding above the per-activity cap is refused before any balance
     * moves. */
    CHECK(build_activity(f, 2U, f->streams[1], 60U, &delegate_signer) == 0);
    CHECK(resolve(f, &live, &resolved) == 0);
    CHECK(resolved.kind == LXP_AUTHORITY_DELEGATED_CAPABILITY);
    CHECK(memcmp(live.grant_id, live_id, 32U) == 0);
    CHECK(execute(f, &live, &resolved, LXP_ERR_GRANT_EXHAUSTED) == 0);
    CHECK(balance_is(f->payer, 90U) && balance_is(f->streams[1], 0U));
    CHECK(f->payer->next_sequence == 1U);
    CHECK(lxp_u128_is_zero(live.scope.spent_total));
    CHECK(persisted_spent(f, live_id, false, 0U) == 0);

    /* A grant over another asset never binds to this debit. */
    CHECK(build_activity(f, 3U, f->streams[2], 10U, &foreign_signer) == 0);
    CHECK(resolve(f, &foreign, &resolved) == 0);
    CHECK(memcmp(foreign.grant_id, foreign_id, 32U) == 0);
    CHECK(execute(f, &foreign, &resolved, LXP_ERR_ASSET_MISMATCH) == 0);
    CHECK(balance_is(f->payer, 90U) && balance_is(f->streams[2], 0U));
    CHECK(lxp_u128_is_zero(foreign.scope.spent_total));
    CHECK(persisted_spent(f, foreign_id, false, 0U) == 0);

    /* A copy of the grant resolved before anything is charged: what a second
     * activity under the same grant in one batch presents. */
    CHECK(build_activity(f, 4U, f->streams[3], 10U, &delegate_signer) == 0);
    CHECK(resolve(f, &stale, &stale_resolved) == 0);
    CHECK(lxp_u128_is_zero(stale.scope.spent_total));

    /* Within both caps: charged, applied, and persisted beside the grant. */
    CHECK(build_activity(f, 5U, f->streams[4], 40U, &delegate_signer) == 0);
    CHECK(resolve(f, &live, &resolved) == 0);
    CHECK(execute(f, &live, &resolved, LXP_OK) == 0);
    CHECK(balance_is(f->payer, 50U) && balance_is(f->streams[4], 40U));
    CHECK(f->payer->next_sequence == 2U);
    CHECK(live.scope.spent_total.hi == 0U && live.scope.spent_total.lo == 40U);
    CHECK(persisted_spent(f, live_id, true, 40U) == 0);

    /* The total cap is enforced against the persisted counter. */
    CHECK(build_activity(f, 6U, f->streams[5], 40U, &delegate_signer) == 0);
    CHECK(resolve(f, &live, &resolved) == 0);
    CHECK(live.scope.spent_total.hi == 0U && live.scope.spent_total.lo == 40U);
    CHECK(execute(f, &live, &resolved, LXP_ERR_GRANT_EXHAUSTED) == 0);
    CHECK(balance_is(f->payer, 50U) && balance_is(f->streams[5], 0U));
    CHECK(live.scope.spent_total.hi == 0U && live.scope.spent_total.lo == 40U);
    CHECK(persisted_spent(f, live_id, true, 40U) == 0);

    /* The module charges the scope and moves the balances, then the kernel
     * finds no room to commit the stream record. The unwind restores the
     * charged scope with the balances and leaves the persisted counter. */
    CHECK(build_activity(f, 7U, f->streams[8], 20U, &delegate_signer) == 0);
    CHECK(resolve(f, &live, &resolved) == 0);
    store_count = f->kernel.module_kv_count;
    fill_module_store(f);
    CHECK(execute_refused_commit(f, &live, &resolved) == 0);
    CHECK(f->kernel.module_kv_count == (size_t)LXP_KERNEL_MAX_MODULE_KV);
    f->kernel.module_kv_count = store_count;
    CHECK(balance_is(f->payer, 50U) && balance_is(f->streams[8], 0U));
    CHECK(f->payer->next_sequence == 2U);
    CHECK(live.scope.spent_total.hi == 0U && live.scope.spent_total.lo == 40U);
    CHECK(persisted_spent(f, live_id, true, 40U) == 0);

    /* The copy resolved before the charge no longer continues the committed
     * grant and is refused without touching either scope. */
    CHECK(build_activity(f, 4U, f->streams[3], 10U, &delegate_signer) == 0);
    CHECK(execute(f, &stale, &stale_resolved, LXP_ERR_CONTEXT_MISMATCH) == 0);
    CHECK(balance_is(f->payer, 50U) && balance_is(f->streams[3], 0U));
    CHECK(lxp_u128_is_zero(stale.scope.spent_total));
    CHECK(live.scope.spent_total.hi == 0U && live.scope.spent_total.lo == 40U);
    CHECK(persisted_spent(f, live_id, true, 40U) == 0);

    /* Exactly the remainder of the total. */
    CHECK(build_activity(f, 8U, f->streams[6], 30U, &delegate_signer) == 0);
    CHECK(resolve(f, &live, &resolved) == 0);
    CHECK(execute(f, &live, &resolved, LXP_OK) == 0);
    CHECK(balance_is(f->payer, 20U) && balance_is(f->streams[6], 30U));
    CHECK(f->payer->next_sequence == 3U);
    CHECK(live.scope.spent_total.hi == 0U && live.scope.spent_total.lo == 70U);
    CHECK(persisted_spent(f, live_id, true, 70U) == 0);

    /* Exhausted: one more unit is refused. */
    CHECK(build_activity(f, 9U, f->streams[7], 1U, &delegate_signer) == 0);
    CHECK(resolve(f, &live, &resolved) == 0);
    CHECK(execute(f, &live, &resolved, LXP_ERR_GRANT_EXHAUSTED) == 0);
    CHECK(balance_is(f->payer, 20U) && balance_is(f->streams[7], 0U));
    CHECK(f->payer->next_sequence == 3U);
    CHECK(persisted_spent(f, live_id, true, 70U) == 0);

    /* A fresh resolution carries the charged scope; the other grant is still
     * untouched. */
    CHECK(resolve(f, &fresh, &resolved) == 0);
    CHECK(memcmp(fresh.grant_id, live_id, 32U) == 0);
    CHECK(fresh.scope.spent_total.hi == 0U &&
          fresh.scope.spent_total.lo == 70U);
    CHECK(persisted_spent(f, foreign_id, false, 0U) == 0);

    CHECK(lxp_state_store_destroy(&f->state) == LXP_OK);
    free(f);
    return 0;
}

int main(void)
{
    if (allowance_end_to_end() != 0) return 1;
    return 0;
}
