#include "layerx/lxp_kernel.h"
#include "layerx/lxp_transfer.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

#include <limits.h>
#include <stdlib.h>
#include <string.h>

typedef struct kv_view {
    const uint8_t *key;
    size_t key_length;
    const uint8_t *value;
    size_t value_length;
} kv_view;

static bool key_equal(const uint8_t *left, size_t left_length,
                      const uint8_t *right, size_t right_length)
{
    return left_length == right_length &&
           memcmp(left, right, left_length) == 0;
}

static int key_compare(const uint8_t *left, size_t left_length,
                       const uint8_t *right, size_t right_length)
{
    size_t common = left_length < right_length ? left_length : right_length;
    int comparison = memcmp(left, right, common);
    if (comparison != 0) return comparison;
    return left_length < right_length ? -1 : left_length != right_length;
}

static size_t committed_find(const lxp_module_ctx *ctx, const uint8_t *key,
                             size_t key_length)
{
    size_t i;
    for (i = 0U; i < ctx->kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &ctx->kernel->module_kv[i];
        if (entry->module_id == ctx->module_id &&
            key_equal(entry->key, entry->key_length, key, key_length))
            return i;
    }
    return ctx->kernel->module_kv_count;
}

static size_t staged_find(const lxp_module_ctx *ctx, const uint8_t *key,
                          size_t key_length)
{
    size_t i;
    for (i = 0U; i < ctx->staged_count; ++i)
        if (key_equal(ctx->staged[i].key, ctx->staged[i].key_length,
                      key, key_length)) return i;
    return ctx->staged_count;
}

static size_t committed_blob_find(const lxp_module_ctx *ctx,
                                  const uint8_t key[32])
{
    size_t i;
    for (i = 0U; i < ctx->kernel->blob_count; ++i)
        if (ctx->kernel->blobs[i].module_id == ctx->module_id &&
            memcmp(ctx->kernel->blobs[i].key, key, 32U) == 0) return i;
    return ctx->kernel->blob_count;
}

static size_t staged_blob_find(const lxp_module_ctx *ctx,
                               const uint8_t key[32])
{
    size_t i;
    for (i = 0U; i < ctx->staged_blob_count; ++i)
        if (memcmp(ctx->staged_blobs[i].key, key, 32U) == 0) return i;
    return ctx->staged_blob_count;
}

static lxp_result key_check(const uint8_t *key, size_t key_length)
{
    if (key == NULL || key_length == 0U) return LXP_ERR_NON_CANONICAL;
    if (key_length > LXP_MODULE_MAX_KEY_BYTES) return LXP_ERR_LENGTH_LIMIT;
    return LXP_OK;
}

lxp_result lxp_module_ctx_init(lxp_module_ctx *ctx, lxp_kernel *kernel,
                               uint16_t module_id,
                               uint64_t batch_timestamp_ms, uint64_t epoch,
                               uint64_t global_sequence, uint64_t gas_limit,
                               lxp_arena *arena, bool mutable)
{
    const lxp_module_registration *registration;
    if (ctx == NULL || kernel == NULL || arena == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_kernel_module_by_id(kernel, module_id, epoch, &registration) !=
        LXP_OK) return LXP_ERR_MODULE_DISABLED;
    (void)registration;
    (void)memset(ctx, 0, sizeof(*ctx));
    ctx->kernel = kernel;
    ctx->module_id = module_id;
    ctx->clock.sealed_timestamp_ms = batch_timestamp_ms;
    ctx->clock.bound = 1U;
    ctx->epoch = epoch;
    ctx->global_sequence = global_sequence;
    ctx->gas_limit = gas_limit;
    ctx->arena = arena;
    ctx->mutable = mutable;
    return LXP_OK;
}

lxp_result lxp_module_ctx_set_mutable(lxp_module_ctx *ctx, bool mutable)
{
    if (ctx == NULL) return LXP_ERR_NON_CANONICAL;
    if (!mutable && ctx->staged_count != 0U) return LXP_FATAL_INVARIANT;
    ctx->mutable = mutable;
    return LXP_OK;
}

lxp_result lxp_module_ctx_bind_effects(lxp_module_ctx *ctx,
                                       lxp_effect_buffer *effects)
{
    if (ctx == NULL || effects == NULL) return LXP_ERR_NON_CANONICAL;
    ctx->effects = effects;
    ctx->next_effect_ordinal = 0U;
    return LXP_OK;
}

lxp_result lxp_ctx_verified_receipt_facts(
    const lxp_module_ctx *ctx, const uint8_t receipt_digest[32],
    lxp_verified_receipt_facts *facts)
{
    if (ctx == NULL || ctx->verified_receipts == NULL)
        return LXP_ERR_UNKNOWN_FIELD;
    return lxp_verified_receipt_index_lookup(ctx->verified_receipts,
                                              receipt_digest, facts);
}

lxp_result lxp_ctx_kv_get(lxp_module_ctx *ctx, const uint8_t *key,
                          size_t key_length, const uint8_t **value,
                          size_t *value_length)
{
    size_t location;
    lxp_result status = key_check(key, key_length);
    if (status != LXP_OK || ctx == NULL || value == NULL ||
        value_length == NULL) return status != LXP_OK ? status :
                                      LXP_ERR_NON_CANONICAL;
    location = staged_find(ctx, key, key_length);
    if (location != ctx->staged_count) {
        if (ctx->staged[location].deleted) return LXP_ERR_UNKNOWN_FIELD;
        *value = ctx->staged[location].value;
        *value_length = ctx->staged[location].value_length;
        return LXP_OK;
    }
    location = committed_find(ctx, key, key_length);
    if (location == ctx->kernel->module_kv_count)
        return LXP_ERR_UNKNOWN_FIELD;
    *value = ctx->kernel->module_kv[location].value;
    *value_length = ctx->kernel->module_kv[location].value_length;
    return LXP_OK;
}

static lxp_result stage_change(lxp_module_ctx *ctx, const uint8_t *key,
                               size_t key_length, const uint8_t *value,
                               size_t value_length, bool deleted)
{
    size_t location;
    lxp_result status = key_check(key, key_length);
    if (status != LXP_OK || ctx == NULL) return status != LXP_OK ? status :
                                               LXP_ERR_NON_CANONICAL;
    if (!ctx->mutable) return LXP_FATAL_INVARIANT;
    if (!deleted && (value == NULL || value_length >
                     LXP_MODULE_MAX_VALUE_BYTES))
        return value == NULL ? LXP_ERR_NON_CANONICAL : LXP_ERR_LENGTH_LIMIT;
    location = staged_find(ctx, key, key_length);
    if (location == ctx->staged_count) {
        if (ctx->staged_count == LXP_MODULE_MAX_STAGED_WRITES)
            return LXP_ERR_ARENA_EXHAUSTED;
        ++ctx->staged_count;
        (void)memset(&ctx->staged[location], 0, sizeof(ctx->staged[location]));
        ctx->staged[location].key_length = (uint16_t)key_length;
        (void)memcpy(ctx->staged[location].key, key, key_length);
    }
    ctx->staged[location].deleted = deleted;
    ctx->staged[location].value_length = (uint32_t)value_length;
    if (!deleted && value_length != 0U)
        (void)memcpy(ctx->staged[location].value, value, value_length);
    return LXP_OK;
}

lxp_result lxp_ctx_kv_put(lxp_module_ctx *ctx, const uint8_t *key,
                          size_t key_length, const uint8_t *value,
                          size_t value_length)
{
    return stage_change(ctx, key, key_length, value, value_length, false);
}

lxp_result lxp_ctx_kv_del(lxp_module_ctx *ctx, const uint8_t *key,
                          size_t key_length)
{
    return stage_change(ctx, key, key_length, NULL, 0U, true);
}

lxp_result lxp_module_ctx_commit(lxp_module_ctx *ctx)
{
    size_t i;
    lxp_result status;
    if (ctx == NULL || !ctx->mutable)
        return LXP_FATAL_INVARIANT;
    if (!ctx->commit_prepared) {
        status = lxp_module_ctx_prepare_commit(ctx);
        if (status != LXP_OK) return status;
    }
    for (i = 0U; i < ctx->staged_count; ++i) {
        lxp_module_kv_change *change = &ctx->staged[i];
        size_t location = committed_find(ctx, change->key,
                                         change->key_length);
        if (change->deleted) {
            if (location != ctx->kernel->module_kv_count) {
                size_t tail = ctx->kernel->module_kv_count - location - 1U;
                if (tail != 0U)
                    (void)memmove(&ctx->kernel->module_kv[location],
                                  &ctx->kernel->module_kv[location + 1U],
                                  tail * sizeof(ctx->kernel->module_kv[0]));
                --ctx->kernel->module_kv_count;
            }
            continue;
        }
        if (location == ctx->kernel->module_kv_count)
            ++ctx->kernel->module_kv_count;
        ctx->kernel->module_kv[location].module_id = ctx->module_id;
        ctx->kernel->module_kv[location].key_length = change->key_length;
        ctx->kernel->module_kv[location].value_length = change->value_length;
        (void)memcpy(ctx->kernel->module_kv[location].key, change->key,
                     change->key_length);
        (void)memcpy(ctx->kernel->module_kv[location].value, change->value,
                     change->value_length);
    }
    for (i = 0U; i < ctx->staged_blob_count; ++i) {
        lxp_module_blob *staged = &ctx->staged_blobs[i];
        size_t location = committed_blob_find(ctx, staged->key);
        if (location == ctx->kernel->blob_count) {
            location = ctx->kernel->blob_count++;
            ctx->kernel->blobs[location] = *staged;
            ctx->kernel->blob_total_bytes += staged->length;
            staged->bytes = NULL;
        }
    }
    ctx->staged_blob_count = 0U;
    ctx->staged_count = 0U;
    ctx->transfer_snapshot_count = 0U;
    ctx->transfer_applied = false;
    ctx->commit_prepared = false;
    if (ctx->activity_state_release != NULL)
        ctx->activity_state_release(ctx->activity_state);
    ctx->activity_state = NULL;
    ctx->activity_state_release = NULL;
    return LXP_OK;
}

lxp_result lxp_module_ctx_prepare_commit(lxp_module_ctx *ctx)
{
    size_t additions = 0U;
    size_t blob_additions = 0U;
    size_t blob_bytes = 0U;
    size_t i;
    if (ctx == NULL || !ctx->mutable || ctx->commit_prepared)
        return LXP_FATAL_INVARIANT;
    for (i = 0U; i < ctx->staged_count; ++i)
        if (!ctx->staged[i].deleted &&
            committed_find(ctx, ctx->staged[i].key,
                           ctx->staged[i].key_length) ==
                ctx->kernel->module_kv_count) ++additions;
    if (additions > LXP_KERNEL_MAX_MODULE_KV - ctx->kernel->module_kv_count)
        return LXP_ERR_ARENA_EXHAUSTED;
    for (i = 0U; i < ctx->staged_blob_count; ++i)
        if (committed_blob_find(ctx, ctx->staged_blobs[i].key) ==
            ctx->kernel->blob_count) {
            ++blob_additions;
            if (SIZE_MAX - blob_bytes < ctx->staged_blobs[i].length)
                return LXP_ERR_OVERFLOW;
            blob_bytes += ctx->staged_blobs[i].length;
        }
    if (blob_additions > LXP_KERNEL_MAX_BLOBS - ctx->kernel->blob_count ||
        blob_bytes > LXP_KERNEL_MAX_BLOB_TOTAL_BYTES -
                         ctx->kernel->blob_total_bytes)
        return LXP_ERR_ARENA_EXHAUSTED;
    ctx->commit_prepared = true;
    return LXP_OK;
}

lxp_result lxp_module_ctx_preview_root(const lxp_module_ctx *ctx,
                                       uint8_t root[32])
{
    lxp_kernel *preview;
    size_t i;
    lxp_result status;
    if (ctx == NULL || root == NULL || !ctx->commit_prepared)
        return LXP_FATAL_INVARIANT;
    preview = (lxp_kernel *)malloc(sizeof(*preview));
    if (preview == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    *preview = *ctx->kernel;
    for (i = 0U; i < ctx->staged_count; ++i) {
        const lxp_module_kv_change *change = &ctx->staged[i];
        size_t location;
        for (location = 0U; location < preview->module_kv_count; ++location) {
            lxp_module_kv_entry *entry = &preview->module_kv[location];
            if (entry->module_id == ctx->module_id &&
                key_equal(entry->key, entry->key_length, change->key,
                          change->key_length)) break;
        }
        if (change->deleted) {
            if (location != preview->module_kv_count) {
                size_t tail = preview->module_kv_count - location - 1U;
                if (tail != 0U)
                    (void)memmove(&preview->module_kv[location],
                                  &preview->module_kv[location + 1U],
                                  tail * sizeof(preview->module_kv[0]));
                --preview->module_kv_count;
            }
            continue;
        }
        if (location == preview->module_kv_count)
            ++preview->module_kv_count;
        preview->module_kv[location].module_id = ctx->module_id;
        preview->module_kv[location].key_length = change->key_length;
        preview->module_kv[location].value_length = change->value_length;
        (void)memcpy(preview->module_kv[location].key, change->key,
                     change->key_length);
        (void)memcpy(preview->module_kv[location].value, change->value,
                     change->value_length);
    }
    for (i = 0U; i < ctx->staged_blob_count; ++i) {
        if (committed_blob_find(ctx, ctx->staged_blobs[i].key) !=
            ctx->kernel->blob_count) continue;
        preview->blobs[preview->blob_count++] = ctx->staged_blobs[i];
    }
    status = lxp_state_subtree_root(preview, ctx->module_id, root);
    free(preview);
    return status;
}

static void restore_transfer_snapshots(lxp_module_ctx *ctx)
{
    size_t i;
    for (i = 0U; i < ctx->transfer_snapshot_count; ++i) {
        lxp_module_account_snapshot *snapshot = &ctx->transfer_snapshots[i];
        snapshot->account->balance = snapshot->balance;
        (void)memcpy(snapshot->account->asset_id, snapshot->asset_id, 32U);
        snapshot->account->has_asset = snapshot->has_asset;
        snapshot->account->next_sequence = snapshot->next_sequence;
    }
    ctx->transfer_snapshot_count = 0U;
    ctx->transfer_applied = false;
}

void lxp_module_ctx_rollback(lxp_module_ctx *ctx)
{
    size_t i;
    if (ctx == NULL) return;
    restore_transfer_snapshots(ctx);
    ctx->commit_prepared = false;
    ctx->staged_count = 0U;
    for (i = 0U; i < ctx->staged_blob_count; ++i)
        free(ctx->staged_blobs[i].bytes);
    ctx->staged_blob_count = 0U;
    if (ctx->activity_state_release != NULL)
        ctx->activity_state_release(ctx->activity_state);
    ctx->activity_state = NULL;
    ctx->activity_state_release = NULL;
}

lxp_result lxp_ctx_blob_get(lxp_module_ctx *ctx, const uint8_t key[32],
                            const uint8_t **bytes, size_t *length)
{
    size_t location;
    if (ctx == NULL || key == NULL || bytes == NULL || length == NULL)
        return LXP_ERR_NON_CANONICAL;
    location = staged_blob_find(ctx, key);
    if (location != ctx->staged_blob_count) {
        *bytes = ctx->staged_blobs[location].bytes;
        *length = ctx->staged_blobs[location].length;
        return LXP_OK;
    }
    location = committed_blob_find(ctx, key);
    if (location == ctx->kernel->blob_count) return LXP_ERR_UNKNOWN_FIELD;
    *bytes = ctx->kernel->blobs[location].bytes;
    *length = ctx->kernel->blobs[location].length;
    return LXP_OK;
}

lxp_result lxp_ctx_blob_put(lxp_module_ctx *ctx, const uint8_t key[32],
                            const uint8_t *bytes, size_t length)
{
    uint8_t digest[32];
    uint8_t *copy;
    size_t location;
    lxp_result status;
    if (ctx == NULL || key == NULL || bytes == NULL || length == 0U)
        return LXP_ERR_NON_CANONICAL;
    if (!ctx->mutable) return LXP_FATAL_INVARIANT;
    if (length > LXP_KERNEL_MAX_BLOB_BYTES) return LXP_ERR_LENGTH_LIMIT;
    status = lxp_hash_sha256(bytes, length, digest);
    if (status != LXP_OK) return status;
    if (memcmp(digest, key, 32U) != 0) return LXP_ERR_CONTEXT_MISMATCH;
    location = staged_blob_find(ctx, key);
    if (location != ctx->staged_blob_count)
        return ctx->staged_blobs[location].length == length &&
                       memcmp(ctx->staged_blobs[location].bytes, bytes,
                              length) == 0 ? LXP_OK : LXP_FATAL_INVARIANT;
    location = committed_blob_find(ctx, key);
    if (location != ctx->kernel->blob_count)
        return ctx->kernel->blobs[location].length == length &&
                       memcmp(ctx->kernel->blobs[location].bytes, bytes,
                              length) == 0 ? LXP_OK : LXP_FATAL_INVARIANT;
    if (ctx->staged_blob_count == LXP_KERNEL_MAX_STAGED_BLOBS)
        return LXP_ERR_ARENA_EXHAUSTED;
    copy = (uint8_t *)malloc(length);
    if (copy == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    (void)memcpy(copy, bytes, length);
    location = ctx->staged_blob_count++;
    ctx->staged_blobs[location].module_id = ctx->module_id;
    (void)memcpy(ctx->staged_blobs[location].key, key, 32U);
    ctx->staged_blobs[location].length = length;
    ctx->staged_blobs[location].bytes = copy;
    return LXP_OK;
}

lxp_result lxp_ctx_bind_activity_state(lxp_module_ctx *ctx, void *state,
                                       lxp_activity_state_release_fn release)
{
    if (ctx == NULL || state == NULL || release == NULL ||
        ctx->activity_state != NULL) return LXP_ERR_NON_CANONICAL;
    ctx->activity_state = state;
    ctx->activity_state_release = release;
    return LXP_OK;
}

void *lxp_ctx_activity_state(const lxp_module_ctx *ctx)
{
    return ctx == NULL ? NULL : ctx->activity_state;
}

void *lxp_ctx_take_activity_state(lxp_module_ctx *ctx)
{
    void *state;
    if (ctx == NULL) return NULL;
    state = ctx->activity_state;
    ctx->activity_state = NULL;
    ctx->activity_state_release = NULL;
    return state;
}

const uint8_t *lxp_ctx_activity_id(const lxp_module_ctx *ctx)
{
    return ctx == NULL || lxp_ct_is_zero(ctx->activity_id, 32U) ? NULL :
           ctx->activity_id;
}

const lxp_call_admission_facts *lxp_ctx_call_admission(
    const lxp_module_ctx *ctx)
{
    return ctx != NULL && ctx->call_admission.present ?
           &ctx->call_admission : NULL;
}

lxp_result lxp_ctx_bind_program_outcome(
    lxp_module_ctx *ctx, const lxp_program_outcome *outcome)
{
    if (ctx == NULL || outcome == NULL || !outcome->present ||
        ctx->module_id != LXP_MODULE_PROGRAMS ||
        ctx->program_outcome.present)
        return LXP_ERR_NON_CANONICAL;
    ctx->program_outcome = *outcome;
    return LXP_OK;
}

const lxp_program_outcome *lxp_ctx_program_outcome(
    const lxp_module_ctx *ctx)
{
    return ctx != NULL && ctx->program_outcome.present ?
           &ctx->program_outcome : NULL;
}

static bool has_prefix(const kv_view *view, const uint8_t *prefix,
                       size_t prefix_length)
{
    return prefix_length <= view->key_length &&
           (prefix_length == 0U ||
            memcmp(view->key, prefix, prefix_length) == 0);
}

lxp_result lxp_ctx_kv_iter(lxp_module_ctx *ctx, const uint8_t *prefix,
                           size_t prefix_length, lxp_kv_visit_fn visit,
                           void *user)
{
    kv_view views[LXP_KERNEL_MAX_MODULE_KV + LXP_MODULE_MAX_STAGED_WRITES];
    size_t count = 0U;
    size_t i;
    if (ctx == NULL || visit == NULL ||
        (prefix == NULL && prefix_length != 0U) ||
        prefix_length > LXP_MODULE_MAX_KEY_BYTES)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < ctx->kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &ctx->kernel->module_kv[i];
        size_t staged;
        if (entry->module_id != ctx->module_id) continue;
        staged = staged_find(ctx, entry->key, entry->key_length);
        if (staged != ctx->staged_count) {
            const lxp_module_kv_change *change = &ctx->staged[staged];
            if (change->deleted) continue;
            views[count++] = (kv_view){ change->key, change->key_length,
                                        change->value,
                                        change->value_length };
        } else {
            views[count++] = (kv_view){ entry->key, entry->key_length,
                                        entry->value,
                                        entry->value_length };
        }
    }
    for (i = 0U; i < ctx->staged_count; ++i) {
        const lxp_module_kv_change *change = &ctx->staged[i];
        if (!change->deleted && committed_find(ctx, change->key,
                                               change->key_length) ==
            ctx->kernel->module_kv_count)
            views[count++] = (kv_view){ change->key, change->key_length,
                                        change->value,
                                        change->value_length };
    }
    for (i = 1U; i < count; ++i) {
        kv_view value = views[i];
        size_t position = i;
        while (position != 0U &&
               key_compare(value.key, value.key_length,
                           views[position - 1U].key,
                           views[position - 1U].key_length) < 0) {
            views[position] = views[position - 1U];
            --position;
        }
        views[position] = value;
    }
    for (i = 0U; i < count; ++i) {
        lxp_result status;
        if (!has_prefix(&views[i], prefix, prefix_length)) continue;
        status = visit(views[i].key, views[i].key_length, views[i].value,
                       views[i].value_length, user);
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
}

lxp_result lxp_ctx_emit_transfer_set(lxp_module_ctx *ctx,
                                     const lxp_transfer_set *set,
                                     lxp_receipt *receipt)
{
    lxp_transfer_set emitted;
    size_t i;
    if (ctx == NULL || set == NULL || receipt == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (!ctx->mutable || ctx->kernel->apply_transfer_set == NULL)
        return LXP_ERR_BALANCE_BYPASS;
    if (ctx->transfer_applied) return LXP_ERR_BALANCE_BYPASS;
    for (i = 0U; i < set->leg_count; ++i) {
        lx_account *accounts[2] = { set->legs[i].from, set->legs[i].to };
        size_t side;
        for (side = 0U; side < 2U; ++side) {
            size_t prior;
            if (accounts[side] == NULL) return LXP_ERR_NON_CANONICAL;
            for (prior = 0U; prior < ctx->transfer_snapshot_count; ++prior)
                if (ctx->transfer_snapshots[prior].account == accounts[side])
                    break;
            if (prior != ctx->transfer_snapshot_count) continue;
            if (prior == LXP_MAX_TRANSFER_SET_LEGS * 2U + 1U)
                return LXP_ERR_ARENA_EXHAUSTED;
            ctx->transfer_snapshots[prior].account = accounts[side];
            ctx->transfer_snapshots[prior].balance = accounts[side]->balance;
            (void)memcpy(ctx->transfer_snapshots[prior].asset_id,
                         accounts[side]->asset_id, 32U);
            ctx->transfer_snapshots[prior].has_asset = accounts[side]->has_asset;
            ctx->transfer_snapshots[prior].next_sequence =
                accounts[side]->next_sequence;
            ++ctx->transfer_snapshot_count;
        }
    }
    if (set->context.sequence_account != NULL) {
        size_t prior;
        lx_account *account = set->context.sequence_account;
        for (prior = 0U; prior < ctx->transfer_snapshot_count; ++prior)
            if (ctx->transfer_snapshots[prior].account == account) break;
        if (prior == ctx->transfer_snapshot_count) {
            if (prior == LXP_MAX_TRANSFER_SET_LEGS * 2U + 1U)
                return LXP_ERR_ARENA_EXHAUSTED;
            ctx->transfer_snapshots[prior].account = account;
            ctx->transfer_snapshots[prior].balance = account->balance;
            (void)memcpy(ctx->transfer_snapshots[prior].asset_id,
                         account->asset_id, 32U);
            ctx->transfer_snapshots[prior].has_asset = account->has_asset;
            ctx->transfer_snapshots[prior].next_sequence =
                account->next_sequence;
            ++ctx->transfer_snapshot_count;
        }
    }
    emitted = *set;
    emitted.context.origin_module_id = ctx->module_id;
    {
        lxp_result status = ctx->kernel->apply_transfer_set(ctx->kernel,
                                                            &emitted, receipt);
        if (status != LXP_OK) {
            restore_transfer_snapshots(ctx);
            return status;
        }
    }
    ctx->transfer_applied = true;
    return LXP_OK;
}

lxp_result lxp_ctx_emit_event(lxp_module_ctx *ctx, uint16_t event_type,
                              const uint8_t *body, size_t body_length)
{
    lxp_effect effect;
    if (ctx == NULL || ctx->effects == NULL ||
        (body == NULL && body_length != 0U) || body_length > 256U)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&effect, 0, sizeof(effect));
    effect.module_id = ctx->module_id;
    effect.ordinal = ctx->next_effect_ordinal;
    effect.event_type = event_type;
    effect.kind = LXP_EFFECT_EVENT;
    effect.body_length = (uint16_t)body_length;
    if (body_length != 0U) (void)memcpy(effect.body, body, body_length);
    if (ctx->next_effect_ordinal == UINT16_MAX) return LXP_ERR_OVERFLOW;
    ++ctx->next_effect_ordinal;
    return lxp_effect_buffer_add(ctx->effects, &effect);
}

uint64_t lxp_ctx_batch_timestamp_ms(const lxp_module_ctx *ctx)
{
    uint64_t timestamp_ms = 0U;
    return ctx != NULL && lxp_exec_clock_read(&ctx->clock, &timestamp_ms) ==
           LXP_OK ? timestamp_ms : 0U;
}

uint64_t lxp_ctx_epoch(const lxp_module_ctx *ctx)
{
    return ctx == NULL ? 0U : ctx->epoch;
}

uint64_t lxp_ctx_global_sequence(const lxp_module_ctx *ctx)
{
    return ctx == NULL ? 0U : ctx->global_sequence;
}

lxp_result lxp_ctx_read_param(const lxp_module_ctx *ctx, uint32_t parameter_id,
                              uint64_t *value)
{
    if (ctx == NULL || value == NULL) return LXP_ERR_NON_CANONICAL;
    if (ctx->kernel->read_parameter == NULL) return LXP_ERR_UNKNOWN_FIELD;
    return ctx->kernel->read_parameter(ctx->kernel->parameter_set,
                                       parameter_id, value);
}

lxp_result lxp_ctx_charge_gas(lxp_module_ctx *ctx, uint64_t units)
{
    if (ctx == NULL) return LXP_ERR_NON_CANONICAL;
    if (UINT64_MAX - ctx->gas_used < units ||
        ctx->gas_used + units > ctx->gas_limit) return LXP_ERR_GAS_EXHAUSTED;
    ctx->gas_used += units;
    return LXP_OK;
}

lxp_result lxp_ctx_arena_alloc(lxp_module_ctx *ctx, size_t size,
                               size_t alignment, void **allocation)
{
    if (ctx == NULL) return LXP_ERR_NON_CANONICAL;
    return lxp_arena_alloc(ctx->arena, size, alignment, allocation);
}

void *lxp_ctx_module_runtime(const lxp_module_ctx *ctx)
{
    if (ctx == NULL || ctx->kernel == NULL || ctx->module_id == 0U ||
        ctx->module_id > LXP_MODULE_RESERVED_COUNT)
        return NULL;
    return ctx->kernel->module_runtime[ctx->module_id];
}
