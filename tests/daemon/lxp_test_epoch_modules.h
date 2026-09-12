#ifndef LAYERX_TESTS_DAEMON_LXP_TEST_EPOCH_MODULES_H
#define LAYERX_TESTS_DAEMON_LXP_TEST_EPOCH_MODULES_H

#include "lxp_daemon_modules.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_state.h"

#include <stddef.h>
#include <stdint.h>
#include <string.h>

/* One node-shaped kernel for the epoch transition tests: the genesis module
 * plan is resolved from a manifest carrying the module-enable flags exactly as
 * layerxd and the guarantor resolve theirs, the modules it selects are
 * registered at epoch 1, the production ledger applier is installed, and the
 * ASSET runtime and the gated module runtimes are bound through the same
 * binder both processes call. */
enum {
    EPOCH_FIXTURE_ARENA_BYTES = 65536,
    EPOCH_FIXTURE_NETWORK_ID = 42,
    EPOCH_FIXTURE_GAS_LIMIT = 100000
};

typedef struct epoch_fixture {
    lx_asset_registry assets;
    lx_asset_record asset;
    lx_account_registry accounts;
    lxp_transfer_asset_state transfer_assets[1];
    lx_asset_runtime asset_runtime;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_daemon_module_runtimes runtimes;
    lxp_genesis_manifest manifest;
    lxp_genesis_module_plan plan;
    lxp_arena arena;
    uint8_t arena_bytes[EPOCH_FIXTURE_ARENA_BYTES];
    uint64_t parameters;
} epoch_fixture;

static inline void epoch_fixture_asset(lx_asset_record *asset)
{
    (void)memset(asset, 0, sizeof(*asset));
    asset->asset_id[0] = 1U;
    asset->symbol_length = 3U;
    (void)memcpy(asset->symbol, "USD", 4U);
    asset->name[0] = (uint8_t)'A';
    asset->name_length = 1U;
    asset->issuer_kind = 2U;
    asset->issuer_did32[0] = 1U;
    asset->custody_kind = LX_ASSET_CUSTODY_PAXEER;
    asset->custody_reference[0] = 1U;
    asset->custody_reference_length = 1U;
}

/* Sets the genesis module-enable flag of one gated module the way the genesis
 * builder writes it: a governance parameter keyed module-enable:<name> whose
 * last byte is 1. */
static inline lxp_result epoch_fixture_enable(epoch_fixture *fixture,
                                              uint16_t module_id)
{
    lxp_genesis_parameter *parameter;
    lxp_result status;
    if (fixture->manifest.parameter_count >=
        (size_t)LXP_GENESIS_MAX_PARAMETERS)
        return LXP_ERR_LENGTH_LIMIT;
    parameter =
        &fixture->manifest.parameters[fixture->manifest.parameter_count];
    (void)memset(parameter, 0, sizeof(*parameter));
    parameter->module_id = LXP_MODULE_GOVERNANCE;
    status = lxp_genesis_module_enable_key(module_id, parameter->key);
    if (status != LXP_OK) return status;
    parameter->value[31] = 1U;
    ++fixture->manifest.parameter_count;
    return LXP_OK;
}

static inline lxp_result epoch_fixture_open(epoch_fixture *fixture,
                                            const uint16_t *enabled,
                                            size_t enabled_count)
{
    size_t i;
    lxp_result status = LXP_OK;
    if (fixture == NULL || (enabled == NULL && enabled_count != 0U))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(fixture, 0, sizeof(*fixture));
    fixture->parameters = 1U;
    epoch_fixture_asset(&fixture->asset);
    fixture->manifest.protocol_version =
        LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    fixture->manifest.network_id = EPOCH_FIXTURE_NETWORK_ID;
    fixture->manifest.genesis_timestamp_ms = 1U;
    for (i = 0U; status == LXP_OK && i < enabled_count; ++i)
        status = epoch_fixture_enable(fixture, enabled[i]);
    if (status == LXP_OK)
        status = lxp_genesis_module_plan_resolve(&fixture->manifest,
                                                 &fixture->plan);
    if (status == LXP_OK)
        status = lx_asset_registry_init(&fixture->assets, 0U);
    if (status == LXP_OK)
        status = lx_asset_register(&fixture->assets, &fixture->asset, 0U,
                                   (lxp_u128){ 0U, 0U });
    if (status == LXP_OK)
        status = lx_asset_transfer_state(&fixture->asset,
                                         &fixture->transfer_assets[0]);
    if (status == LXP_OK)
        status = lx_account_registry_init(&fixture->accounts);
    if (status == LXP_OK) status = lxp_state_store_init(&fixture->state, 1U);
    if (status == LXP_OK)
        status = lxp_state_store_bind_accounts(&fixture->state,
                                               &fixture->accounts);
    if (status == LXP_OK)
        status = lxp_kernel_create(&fixture->kernel, &fixture->state,
                                   &fixture->journal, &fixture->parameters,
                                   1U);
    if (status == LXP_OK)
        status = lxp_genesis_module_plan_register(&fixture->plan,
                                                  &fixture->kernel);
    if (status == LXP_OK)
        status = lxp_kernel_set_capabilities(
            &fixture->kernel, NULL, lxp_kernel_canonical_ledger_apply);
    if (status == LXP_OK)
        status = lxp_arena_init(&fixture->arena, fixture->arena_bytes,
                                sizeof(fixture->arena_bytes));
    return status;
}

/* Opens one credit account of the fixture asset and bootstraps its balance. */
static inline lxp_result epoch_fixture_account(epoch_fixture *fixture,
                                               const char *name,
                                               uint64_t sequence,
                                               uint64_t balance,
                                               lx_account **account)
{
    lxp_result status = lx_asset_account_open(
        &fixture->assets, &fixture->accounts, fixture->asset.asset_id,
        (const uint8_t *)name, strlen(name), sequence, LX_ACCOUNT_OPEN_CREDIT,
        NULL, account);
    if (status != LXP_OK) return status;
    if (balance == 0U) return LXP_OK;
    return lxp_ledger_bootstrap_balance(*account, fixture->asset.asset_id,
                                        (lxp_u128){ 0U, balance }, 0U);
}

/* Binds the runtimes in the order the node binds them once its registries
 * are populated: the ASSET runtime first, then the gated module runtimes. */
static inline lxp_result epoch_fixture_bind(epoch_fixture *fixture)
{
    lxp_result status;
    fixture->asset_runtime = (lx_asset_runtime){
        &fixture->accounts, fixture->assets.assets, fixture->assets.count,
        fixture->transfer_assets, 1U, EPOCH_FIXTURE_NETWORK_ID,
        LXP_PROTOCOL_VERSION_STATE_COMMITMENT
    };
    status = lxp_kernel_bind_module_runtime(&fixture->kernel, LXP_MODULE_ASSET,
                                            &fixture->asset_runtime);
    if (status != LXP_OK) return status;
    return lxp_daemon_module_runtimes_bind(
        &fixture->kernel, &fixture->runtimes, &fixture->accounts,
        &fixture->assets, fixture->transfer_assets, 1U);
}

/* A module context at the kernel's current epoch and next sequence, used to
 * seed committed module state and to read it back. */
static inline lxp_result epoch_fixture_ctx(epoch_fixture *fixture,
                                           lxp_module_ctx *ctx,
                                           lxp_effect_buffer *effects,
                                           uint16_t module_id,
                                           uint64_t timestamp_ms)
{
    lxp_result status = lxp_arena_reset(&fixture->arena, 0U);
    if (status == LXP_OK) status = lxp_effect_buffer_init(effects);
    if (status != LXP_OK) return status;
    status = lxp_module_ctx_init(ctx, &fixture->kernel, module_id,
                                 timestamp_ms, fixture->kernel.epoch,
                                 fixture->state.next_sequence,
                                 (uint64_t)EPOCH_FIXTURE_GAS_LIMIT,
                                 &fixture->arena, true);
    if (status != LXP_OK) return status;
    return lxp_module_ctx_bind_effects(ctx, effects);
}

/* The transition must leave the kernel exactly where a replayer expects it:
 * the journal closed, the epoch and sequence where the caller asserts them,
 * and current_state_root equal to the root recomputed from the state. */
static inline int epoch_fixture_consistent(epoch_fixture *fixture,
                                           uint64_t epoch,
                                           uint64_t next_sequence)
{
    uint8_t root[32];
    if (fixture->kernel.epoch != epoch ||
        fixture->state.next_sequence != next_sequence ||
        fixture->journal.open ||
        lxp_state_root(&fixture->kernel, root) != LXP_OK ||
        memcmp(root, fixture->kernel.current_state_root, 32U) != 0)
        return 0;
    return 1;
}

static inline int epoch_fixture_close(epoch_fixture *fixture)
{
    int failed = lxp_state_store_destroy(&fixture->state) != LXP_OK;
    lx_account_registry_release(&fixture->accounts);
    return failed;
}

#endif
