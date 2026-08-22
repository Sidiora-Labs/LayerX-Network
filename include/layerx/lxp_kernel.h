#ifndef LAYERX_LXP_KERNEL_H
#define LAYERX_LXP_KERNEL_H

#include "layerx/lxp_module.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_state.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_fee.h"
#include "layerx/lxp_batch.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LXP_KERNEL_MAX_MODULE_REGISTRATIONS = 32,
    LXP_KERNEL_MAX_MODULE_KV = 512,
    LXP_MODULE_MAX_KEY_BYTES = 128,
    LXP_MODULE_MAX_VALUE_BYTES = 1024,
    LXP_MODULE_MAX_STAGED_WRITES = 64,
    LXP_KERNEL_MAX_BLOBS = 512,
    LXP_KERNEL_MAX_STAGED_BLOBS = 4,
    LXP_KERNEL_MAX_BLOB_BYTES = 1048576,
    LXP_KERNEL_MAX_BLOB_TOTAL_BYTES = 67108864
};

typedef struct lxp_module_blob {
    uint16_t module_id;
    uint8_t key[32];
    size_t length;
    uint8_t *bytes;
} lxp_module_blob;

typedef struct lxp_module_kv_entry {
    uint16_t module_id;
    uint16_t key_length;
    uint32_t value_length;
    uint8_t key[LXP_MODULE_MAX_KEY_BYTES];
    uint8_t value[LXP_MODULE_MAX_VALUE_BYTES];
} lxp_module_kv_entry;

typedef struct lxp_module_kv_change {
    uint16_t key_length;
    uint32_t value_length;
    uint8_t key[LXP_MODULE_MAX_KEY_BYTES];
    uint8_t value[LXP_MODULE_MAX_VALUE_BYTES];
    bool deleted;
} lxp_module_kv_change;

typedef void (*lxp_activity_state_release_fn)(void *state);

typedef struct lxp_module_account_snapshot {
    lx_account *account;
    lxp_u128 balance;
    uint8_t asset_id[32];
    bool has_asset;
    uint64_t next_sequence;
} lxp_module_account_snapshot;

typedef struct lxp_call_admission_facts {
    uint8_t activity_binding[32];
    uint8_t payer[32];
    lxp_u128 available_fee_units;
    lxp_u128 signed_fee_limit;
    uint16_t fee_schedule_version;
    uint32_t parameter_version;
    bool present;
} lxp_call_admission_facts;

struct lxp_kernel;
typedef lxp_result (*lxp_kernel_parameter_reader)(const void *parameter_set,
                                                  uint32_t parameter_id,
                                                  uint64_t *value);
typedef lxp_result (*lxp_kernel_transfer_applier)(
    struct lxp_kernel *kernel, const lxp_transfer_set *set,
    lxp_receipt *receipt);
typedef lxp_result (*lxp_kernel_fee_prepare_fn)(
    struct lxp_kernel *kernel, const lxp_activity *activity,
    const lxp_authority_resolved *authority, lxp_u128 fee,
    void **transaction);
typedef void (*lxp_kernel_fee_finish_fn)(struct lxp_kernel *kernel,
                                         void *transaction);
/* prepare applies the fee atomically and returns its rollback token on LXP_OK;
 * on error it leaves fee state unchanged. commit and rollback are infallible,
 * consume the token exactly once, and must not retain it. */
typedef struct lxp_kernel_fee_transaction {
    lxp_kernel_fee_prepare_fn prepare;
    lxp_kernel_fee_finish_fn commit;
    lxp_kernel_fee_finish_fn rollback;
} lxp_kernel_fee_transaction;
typedef lxp_result (*lxp_kernel_supply_checker)(const struct lxp_kernel *kernel);

struct lxp_module_ctx {
    struct lxp_kernel *kernel;
    uint16_t module_id;
    lxp_exec_clock clock;
    uint64_t epoch;
    uint64_t global_sequence;
    uint64_t gas_limit;
    uint64_t gas_used;
    uint8_t activity_id[32];
    lxp_arena *arena;
    lxp_effect_buffer *effects;
    uint16_t next_effect_ordinal;
    bool mutable;
    lxp_module_kv_change staged[LXP_MODULE_MAX_STAGED_WRITES];
    size_t staged_count;
    lxp_module_account_snapshot transfer_snapshots[
        LXP_MAX_TRANSFER_SET_LEGS * 2U + 1U];
    size_t transfer_snapshot_count;
    bool transfer_applied;
    bool commit_prepared;
    void *activity_state;
    lxp_activity_state_release_fn activity_state_release;
    lxp_program_outcome program_outcome;
    lxp_call_admission_facts call_admission;
    const lxp_verified_receipt_index *verified_receipts;
    lxp_module_blob staged_blobs[LXP_KERNEL_MAX_STAGED_BLOBS];
    size_t staged_blob_count;
};

typedef struct lxp_module_registration {
    const lxp_module_iface *iface;
    uint16_t module_id;
    uint32_t abi_version;
    char name[LXP_MODULE_MAX_NAME + 1];
    uint32_t activity_types[LXP_MODULE_MAX_ACTIVITY_TYPES];
    size_t activity_type_count;
    uint64_t enabled_epoch;
    uint64_t disabled_epoch;
    bool enabled;
} lxp_module_registration;

typedef struct lxp_kernel {
    lxp_state_store *state;
    lxp_state_journal *journal;
    const void *parameter_set;
    lxp_module_registration modules[LXP_KERNEL_MAX_MODULE_REGISTRATIONS];
    size_t module_count;
    lxp_module_kv_entry module_kv[LXP_KERNEL_MAX_MODULE_KV];
    size_t module_kv_count;
    lxp_module_blob blobs[LXP_KERNEL_MAX_BLOBS];
    size_t blob_count;
    size_t blob_total_bytes;
    uint64_t epoch;
    lxp_kernel_parameter_reader read_parameter;
    lxp_kernel_transfer_applier apply_transfer_set;
    lxp_kernel_fee_transaction fee_transaction;
    lxp_kernel_supply_checker check_supply;
    void *module_runtime[LXP_MODULE_RESERVED_COUNT + 1U];
    uint8_t current_state_root[32];
} lxp_kernel;
#define lxp_kernel lxp_kernel

lxp_result lxp_kernel_create(lxp_kernel *kernel, lxp_state_store *state,
                             lxp_state_journal *journal,
                             const void *parameter_set, uint64_t epoch);
lxp_result lxp_kernel_set_epoch(lxp_kernel *kernel, uint64_t epoch);
lxp_result lxp_kernel_set_capabilities(
    lxp_kernel *kernel, lxp_kernel_parameter_reader read_parameter,
    lxp_kernel_transfer_applier apply_transfer_set);
lxp_result lxp_kernel_set_fee_transaction(
    lxp_kernel *kernel, const lxp_kernel_fee_transaction *transaction);
lxp_result lxp_kernel_set_supply_checker(lxp_kernel *kernel,
                                         lxp_kernel_supply_checker checker);
lxp_result lxp_kernel_bind_module_runtime(lxp_kernel *kernel,
                                          uint16_t module_id,
                                          void *runtime);
lxp_result lxp_kernel_register_module(lxp_kernel *kernel,
                                      const lxp_module_iface *iface);
lxp_result lxp_kernel_module_for_activity(
    const lxp_kernel *kernel, uint32_t activity_type, uint64_t epoch,
    const lxp_module_registration **registration);
lxp_result lxp_module_ctx_init(lxp_module_ctx *ctx, lxp_kernel *kernel,
                               uint16_t module_id,
                               uint64_t batch_timestamp_ms, uint64_t epoch,
                               uint64_t global_sequence, uint64_t gas_limit,
                               lxp_arena *arena, bool mutable);
lxp_result lxp_module_ctx_set_mutable(lxp_module_ctx *ctx, bool mutable);
lxp_result lxp_module_ctx_bind_effects(lxp_module_ctx *ctx,
                                       lxp_effect_buffer *effects);
lxp_result lxp_module_ctx_prepare_commit(lxp_module_ctx *ctx);
lxp_result lxp_module_ctx_preview_root(const lxp_module_ctx *ctx,
                                       uint8_t root[32]);
lxp_result lxp_module_ctx_commit(lxp_module_ctx *ctx);
void lxp_module_ctx_rollback(lxp_module_ctx *ctx);
lxp_result lxp_ctx_bind_activity_state(lxp_module_ctx *ctx, void *state,
                                       lxp_activity_state_release_fn release);
void *lxp_ctx_activity_state(const lxp_module_ctx *ctx);
void *lxp_ctx_take_activity_state(lxp_module_ctx *ctx);
const uint8_t *lxp_ctx_activity_id(const lxp_module_ctx *ctx);
const lxp_call_admission_facts *lxp_ctx_call_admission(
    const lxp_module_ctx *ctx);
lxp_result lxp_ctx_bind_program_outcome(
    lxp_module_ctx *ctx, const lxp_program_outcome *outcome);
const lxp_program_outcome *lxp_ctx_program_outcome(
    const lxp_module_ctx *ctx);
lxp_result lxp_ctx_blob_get(lxp_module_ctx *ctx, const uint8_t key[32],
                            const uint8_t **bytes, size_t *length);
lxp_result lxp_ctx_blob_put(lxp_module_ctx *ctx, const uint8_t key[32],
                            const uint8_t *bytes, size_t length);

typedef struct lxp_kernel_execution {
    uint32_t network_id;
    uint64_t batch_timestamp_ms;
    uint64_t maximum_timestamp_window;
    uint64_t epoch;
    uint64_t global_sequence;
    uint32_t recorded_module_version;
    uint32_t parameter_version;
    bool signature_valid;
    lxp_identity_store *identities;
    const lxp_authority_resolved *authority;
    const lxp_fee_params *fee_parameters;
    lxp_fee_meter fee_meter;
    lxp_u128 fee_balance;
    uint64_t gas_limit;
    lxp_arena *arena;
    uint8_t batch_id[32];
    uint8_t activity_root[32];
    const uint8_t *sequencer_private_key;
    const lxp_verified_receipt_index *verified_receipts;
    /* Output only. On a committed Programs CALL, the kernel writes the exact
     * envelope projection that the replay transition must publish as that
     * activity's canonical_events span. */
    lxp_byte_span *canonical_events_out;
} lxp_kernel_execution;
#define lxp_kernel_execution lxp_kernel_execution

lxp_result lxp_module_version_for_epoch(
    const lxp_kernel *kernel, uint16_t module_id, uint64_t epoch,
    uint32_t recorded_version,
    const lxp_module_registration **registration);
lxp_result lxp_kernel_dispatch(const lxp_module_registration *registration,
                               lxp_module_ctx *ctx,
                               const lxp_activity *activity,
                               const lxp_authority_resolved *authority,
                               lxp_effect_buffer *effects,
                               lxp_result *module_result);
lxp_result lxp_kernel_execute_activity(lxp_kernel *kernel,
                                       const lxp_activity *activity,
                                       const lxp_kernel_execution *execution,
                                       lxp_receipt *receipt);
uint8_t lxp_kernel_step_order(size_t index);

typedef struct lxp_replay_record {
    uint16_t module_id;
    uint16_t key_length;
    uint32_t value_length;
    uint8_t key[LXP_MODULE_MAX_KEY_BYTES];
    uint8_t value[LXP_MODULE_MAX_VALUE_BYTES];
} lxp_replay_record;
#define lxp_replay_record lxp_replay_record

lxp_result lxp_determinism_guard_check(void);
lxp_result lxp_determinism_guard_trip(const char *symbol);
void lxp_determinism_guard_reset(void);
lxp_result lxp_kernel_replay(lxp_kernel *kernel,
                             const lxp_replay_record *records,
                             const uint8_t (*expected_roots)[32],
                             size_t record_count, size_t worker_threads,
                             uint8_t terminal_root[32]);
lxp_result lxp_replay_compare_roots(const uint8_t expected[32],
                                    const uint8_t produced[32]);
lxp_result lxp_replay_golden_run(const lxp_replay_record *records,
                                 size_t record_count,
                                 const uint8_t (*roots)[32],
                                 size_t worker_threads,
                                 uint8_t digest[32]);
lxp_result lxp_kernel_module_by_id(
    const lxp_kernel *kernel, uint16_t module_id, uint64_t epoch,
    const lxp_module_registration **registration);

#endif
