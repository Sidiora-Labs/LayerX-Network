#define _POSIX_C_SOURCE 200809L
#include "layerx/lxp_bridge_credit.h"
#include "layerx/lxp_bridge_light.h"
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_module_ctx.h"
#include "layerx/lx_asset.h"
#include "layerx/programs.h"
#include "files.h"

#include <stdio.h>
#include <string.h>

#define CHECK(expression) do { if (!(expression)) { \
    (void)fprintf(stderr, "credit admission check failed at line %d: %s\n", __LINE__, #expression); \
    return 1; } } while (0)

static int begin(lxp_module_ctx *ctx, lxp_kernel *kernel, lxp_arena *arena,
                 lxp_effect_buffer *effects, uint64_t batch_ms)
{
    CHECK(lxp_state_journal_open(kernel->state, 1U, kernel->journal) == LXP_OK);
    CHECK(lxp_module_ctx_init(ctx, kernel, LXP_MODULE_BRIDGE, batch_ms, 1U,
                              1U, 100000U, arena, true) == LXP_OK);
    ctx->protocol_version = 3U;
    CHECK(lxp_effect_buffer_init(effects) == LXP_OK);
    CHECK(lxp_module_ctx_bind_effects(ctx, effects) == LXP_OK);
    return 0;
}

int main(int argc, char **argv)
{
    uint8_t *manifest_bytes = NULL;
    uint8_t *activity_bytes = NULL;
    size_t manifest_length = 0U;
    size_t activity_length = 0U;
    uint8_t *arena_bytes = malloc(4U * LXP_MAX_ACTIVITY_BYTES);
    lxp_genesis_manifest *manifest = malloc(sizeof(*manifest));
    lxp_state_store *state = malloc(sizeof(*state));
    lxp_state_journal *journal = calloc(1U, sizeof(*journal));
    lxp_kernel *kernel = malloc(sizeof(*kernel));
    lx_account_registry *accounts = malloc(sizeof(*accounts));
    lxp_daemon_protocol_owner *owner = calloc(1U, sizeof(*owner));
    lxp_module_ctx *ctx = malloc(sizeof(*ctx));
    lxp_effect_buffer *effects = malloc(sizeof(*effects));
    lxp_arena arena;
    lxp_activity activity;
    lxp_activity other;
    lxp_authority_resolved authority = {0};
    lxp_bridge_profile profile;
    lxp_bridge_credit credit;
    lxp_transfer_asset_state asset = {0};
    lx_asset_record asset_record = {0};
    lx_asset_runtime runtime = {0};
    uint8_t name[LX_ACCOUNT_NAME_MAX];
    size_t name_length;
    uint8_t root[32];
    uint8_t initial_root[32];
    uint64_t header_ms = 0U;
    uint64_t sealed_ms;
    uint64_t drift_ms = LXP_BRIDGE_LIGHT_MAX_CLOCK_DRIFT_SECONDS * UINT64_C(1000);
    bool present;
    if (argc != 3) {
        (void)fprintf(stderr, "usage: test-credit-admission genesis.manifest signed-credit.activity\n");
        return 2;
    }
    CHECK(arena_bytes && manifest && state && journal && kernel && accounts && owner && ctx && effects);
    CHECK(read_file(argv[1], LXP_GENESIS_MAX_ENCODED_BYTES, false, &manifest_bytes, &manifest_length) == 0);
    CHECK(read_file(argv[2], LXP_MAX_ACTIVITY_BYTES, false, &activity_bytes, &activity_length) == 0);
    CHECK(lxp_arena_init(&arena, arena_bytes, 4U * LXP_MAX_ACTIVITY_BYTES) == LXP_OK);
    CHECK(lxp_genesis_parse(manifest_bytes, manifest_length, LXP_GENESIS_INPUT_MANIFEST, manifest) == LXP_OK);
    CHECK(lxp_genesis_verify_signature(manifest, &arena) == LXP_OK);
    CHECK(lxp_bridge_genesis_profile(manifest, &profile, &present) == LXP_OK && present);
    CHECK(lxp_activity_decode(activity_bytes, activity_length, &activity) == LXP_OK);
    CHECK(lxp_activity_verify_signature(&activity) == LXP_OK);
    CHECK(activity.activity_type == LXP_BRIDGE_CREDIT);
    CHECK(lxp_bridge_credit_parse(activity.payload.bytes, activity.payload.length, &credit) == LXP_OK);
    for (size_t index = 0U; index < 8U; ++index) header_ms = (header_ms << 8U) | credit.proof[29U + index];
    header_ms *= 1000U;
    CHECK(lx_account_registry_init(accounts) == LXP_OK);
    CHECK(lxp_state_store_init(state, 1U) == LXP_OK);
    CHECK(lxp_state_store_bind_accounts(state, accounts) == LXP_OK);
    CHECK(lxp_kernel_create(kernel, state, journal, manifest, 1U) == LXP_OK);
    CHECK(lxp_kernel_register_module(kernel, programs_module_registration_v4()) == LXP_OK);
    CHECK(lxp_kernel_register_module(kernel, lx_asset_module_iface()) == LXP_OK);
    CHECK(lxp_kernel_register_module(kernel, lxp_governance_module_iface()) == LXP_OK);
    CHECK(lxp_kernel_register_module(kernel, lxp_bridge_module_iface()) == LXP_OK);
    CHECK(lxp_kernel_set_capabilities(kernel, NULL, lxp_kernel_canonical_ledger_apply) == LXP_OK);
    CHECK(lxp_genesis_materialize(manifest, &arena, kernel) == LXP_OK);
    (void)memcpy(asset.asset_id, profile.bytes + 97U, 32U);
    asset.registered = true;
    (void)memcpy(asset_record.asset_id, asset.asset_id, 32U);
    runtime.accounts = accounts;
    runtime.assets = &asset_record;
    runtime.asset_count = 1U;
    runtime.transfer_assets = &asset;
    runtime.transfer_asset_count = 1U;
    runtime.network_id = manifest->network_id;
    runtime.protocol_version = 3U;
    CHECK(lxp_kernel_bind_module_runtime(kernel, LXP_MODULE_ASSET, &runtime) == LXP_OK);
    CHECK(lxp_state_root(kernel, initial_root) == LXP_OK);
    CHECK(memcmp(initial_root, manifest->genesis_state_root, 32U) == 0);

    owner->kernel = kernel;
    owner->scratch = &arena;
    owner->network_id = manifest->network_id;
    owner->protocol_version = 3U;
    owner->latest_sealed_timestamp = manifest->genesis_timestamp_ms;
    sealed_ms = owner->latest_sealed_timestamp;
    CHECK(state->next_sequence == 1U);
    CHECK(sealed_ms != 0U && header_ms > sealed_ms + drift_ms + UINT64_C(1000));

    CHECK(lxp_daemon_credit_admission(owner, &activity, header_ms) == LXP_OK);
    CHECK(owner->latest_sealed_timestamp == sealed_ms);
    CHECK(lxp_daemon_credit_admission(owner, &activity, header_ms - drift_ms) == LXP_OK);
    CHECK(lxp_daemon_credit_admission(owner, &activity, header_ms - drift_ms - UINT64_C(1000)) ==
          LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    CHECK(lxp_daemon_credit_admission(owner, &activity, sealed_ms) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    CHECK(lxp_daemon_credit_admission(owner, &activity, 0U) == LXP_ERR_MALFORMED_ENVELOPE);
    other = activity;
    other.activity_type = LX_ASSET_WITHDRAW;
    CHECK(lxp_daemon_credit_admission(owner, &other, header_ms) == LXP_ERR_MALFORMED_ENVELOPE);
    other = activity;
    other.idempotency_key[0] ^= 1U;
    CHECK(lxp_daemon_credit_admission(owner, &other, header_ms) == LXP_ERR_CONTEXT_MISMATCH);

    CHECK(activity.actor_did.length <= sizeof(name) - 11U);
    (void)memcpy(name, "agent:", 6U);
    (void)memcpy(name + 6U, activity.actor_did.bytes, activity.actor_did.length);
    name_length = 6U + activity.actor_did.length;
    (void)memcpy(name + name_length, ":main", 5U);
    name_length += 5U;
    CHECK(lx_account_id_from_string(name, name_length, authority.principal) == LXP_OK);
    authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(authority.verified_key, activity.authority.bytes, 32U);
    CHECK(begin(ctx, kernel, &arena, effects, sealed_ms) == 0);
    CHECK(lxp_activity_id(activity_bytes, activity_length, ctx->activity_id) == LXP_OK);
    CHECK(lxp_ctx_bridge_credit(ctx, &activity, &authority, &credit) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    lxp_module_ctx_rollback(ctx);
    CHECK(lxp_state_journal_rollback(journal) == LXP_OK);
    CHECK(lxp_state_root(kernel, root) == LXP_OK && memcmp(root, initial_root, 32U) == 0);
    CHECK(begin(ctx, kernel, &arena, effects, header_ms) == 0);
    CHECK(lxp_activity_id(activity_bytes, activity_length, ctx->activity_id) == LXP_OK);
    CHECK(lxp_ctx_bridge_credit(ctx, &activity, &authority, &credit) == LXP_OK);
    CHECK(lxp_module_ctx_prepare_commit(ctx) == LXP_OK);
    CHECK(lxp_state_journal_commit(journal) == LXP_OK);
    CHECK(lxp_module_ctx_commit(ctx) == LXP_OK);
    CHECK(lxp_state_root(kernel, root) == LXP_OK && memcmp(root, initial_root, 32U) != 0);

    CHECK(lxp_state_store_destroy(state) == LXP_OK);
    free(activity_bytes);
    free(manifest_bytes);
    free(effects);
    free(ctx);
    free(owner);
    lx_account_registry_release(accounts);
    free(accounts);
    free(kernel);
    free(journal);
    free(state);
    free(manifest);
    free(arena_bytes);
    (void)puts("first credit on a fresh chain: admitted at the batch time, executed at the batch time");
    return 0;
}
