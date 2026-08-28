#ifndef LAYERX_PROGRAMS_H
#define LAYERX_PROGRAMS_H

#include "layerx/lxp_module.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_transfer.h"
#include "layerx/lxp_replica.h"

#include <pthread.h>
#include <stdint.h>

typedef struct lxp_kernel lxp_kernel;
typedef struct lxp_log lxp_log;
typedef struct lxp_history lxp_history;
typedef struct lxp_genesis_manifest lxp_genesis_manifest;

/* Node-owned durable account-state feed. `append` must commit the notice and
 * its canonical receipt reference before returning. The node binds the feed
 * to the same kernel instance that commits activities; a refusal halts
 * publication and restart recovery resumes from canonical history. */
typedef lxp_result (*lx_programs_state_feed_append_fn)(
    void *context, uint64_t global_sequence, uint32_t ordinal,
    const uint8_t program_id[32], uint32_t activity_type,
    uint16_t event_type, const lxp_receipt *receipt);
typedef lxp_result (*lx_programs_state_feed_begin_fn)(
    void *context, const lxp_activity *activity, const lxp_receipt *receipt);
typedef lxp_result (*lx_programs_state_feed_advance_fn)(
    void *context, const lxp_activity *activity, const lxp_receipt *receipt);
typedef lxp_result (*lx_programs_state_feed_lock_fn)(void *context);
typedef lxp_result (*lx_programs_state_feed_unlock_fn)(void *context);

typedef struct lx_programs_state_feed {
    lx_programs_state_feed_begin_fn begin;
    lx_programs_state_feed_append_fn append;
    lx_programs_state_feed_advance_fn advance;
    lx_programs_state_feed_lock_fn lock;
    lx_programs_state_feed_unlock_fn unlock;
    void *context;
} lx_programs_state_feed;

enum {
    LX_PROGRAMS_STATE_FEED_MAX_NOTICES = 4096,
    LX_PROGRAMS_STATE_FEED_CACHE_NOTICES = 256
};
typedef struct lx_programs_state_notice {
    uint64_t global_sequence;
    uint32_t ordinal;
    uint8_t program_id[32];
    uint32_t activity_type;
    uint16_t event_type;
    uint8_t receipt_digest[32];
} lx_programs_state_notice;

typedef struct lx_programs_state_feed_store {
    lxp_log *log;
    lxp_log *canonical_log;
    lxp_history *history;
    lxp_arena *scratch;
    pthread_mutex_t *coordination_mutex;
    lx_programs_state_notice notices[LX_PROGRAMS_STATE_FEED_CACHE_NOTICES];
    size_t notice_count;
    size_t notice_next;
    uint64_t notice_record_count;
    uint64_t open_notice_sequence;
    uint8_t open_notice_receipt_digest[32];
    uint32_t next_notice_ordinal;
    bool notice_group_open;
    uint64_t scanned_through_sequence;
    uint8_t head_receipt_digest[32];
    uint8_t head_state_root[32];
    uint64_t head_timestamp;
    uint64_t baseline_next_sequence;
    uint8_t baseline_state_root[32];
    bool baseline_present;
    lx_programs_state_feed feed;
} lx_programs_state_feed_store;

lxp_result lxp_programs_state_feed_store_open(
    lx_programs_state_feed_store *store, lxp_log *log,
    lxp_log *canonical_log, lxp_history *history, lxp_arena *scratch,
    pthread_mutex_t *coordination_mutex,
    uint64_t baseline_next_sequence, const uint8_t baseline_state_root[32]);
lxp_result lxp_programs_state_feed_store_recover(
    lx_programs_state_feed_store *store, lxp_kernel *kernel);
lxp_result lxp_programs_state_feed_store_page(
    const lx_programs_state_feed_store *store, uint64_t after_sequence,
    size_t maximum, lx_programs_state_notice *notices, size_t *notice_count,
    uint64_t *complete_through, uint64_t *scanned_through);

enum {
    LX_PROGRAMS_DEPLOY = 0x00090001,
    LX_PROGRAMS_UPGRADE = 0x00090002,
    LX_PROGRAMS_CALL = 0x00090003,
    LX_PROGRAMS_REGISTRY = 0x00090004,
    LX_PROGRAMS_TRANSFER = 0x00090005,
    LX_PROGRAMS_ACCOUNT = 0x00090006,
    LX_PROGRAMS_WIND_DOWN = 0x00090007,
    LX_PROGRAMS_ABI_VERSION = 1,
    LX_PROGRAMS_ACCOUNT_ABI_VERSION = 2,
    LX_PROGRAMS_EVENT_DEPLOYED = 1,
    LX_PROGRAMS_EVENT_UPGRADED = 2,
    LX_PROGRAMS_EVENT_CALLED = 3,
    LX_PROGRAMS_EVENT_REGISTRY_READ = 4,
    LX_PROGRAMS_EVENT_TRANSFERRED = 5,
    LX_PROGRAMS_EVENT_GUEST_ENVELOPE = 6,
    LX_PROGRAMS_EVENT_CALL_OUTCOME = 7,
    LX_PROGRAMS_EVENT_ACCOUNT_REGISTERED = 8,
    LX_PROGRAMS_EVENT_EXIT_ROUTE = 9,
    LX_PROGRAMS_EVENT_DEPRECATED = 10,
    LX_PROGRAMS_EVENT_TOMBSTONED = 11,
    LX_PROGRAMS_EVENT_VALUE_EXITED = 12
};

enum {
    LX_PROGRAMS_ACCOUNT_ID_BYTES = 32,
    LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES = 128
};

typedef struct lx_programs_account_binding {
    uint8_t record_version;
    uint8_t program_id[32];
    uint8_t account_id[32];
    uint8_t asset_id[32];
    uint16_t seed_length;
    uint8_t seed[LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES];
    uint64_t registered_sequence;
    uint8_t registration_event_digest[32];
} lx_programs_account_binding;

typedef lxp_result (*lx_programs_account_visit_fn)(
    const lx_programs_account_binding *binding, void *user);

typedef struct lx_programs_value_account_view {
    lx_programs_account_binding binding;
    lx_account account;
    lxp_u128 balance;
    bool frozen;
    uint64_t observed_sequence;
    uint64_t observed_at;
    uint8_t receipt_digest[32];
    uint8_t account_root[32];
    uint8_t universal_root[32];
    uint8_t programs_root[32];
    uint8_t state_root[32];
    lxp_state_proof account_proof;
    lxp_state_proof account_tree_proof;
    lxp_state_proof universal_root_proof;
    lxp_state_proof binding_proof;
    lxp_state_proof programs_root_proof;
} lx_programs_value_account_view;

typedef lxp_result (*lx_programs_value_account_visit_fn)(
    const lx_programs_value_account_view *account, void *user);

typedef struct lx_programs_account_state_head {
    uint64_t observed_sequence;
    uint64_t observed_at;
    uint8_t receipt_digest[32];
    uint8_t account_root[32];
    uint8_t universal_root[32];
    uint8_t programs_root[32];
    uint8_t state_root[32];
    lxp_state_proof account_tree_proof;
    lxp_state_proof universal_root_proof;
    lxp_state_proof programs_root_proof;
} lx_programs_account_state_head;

typedef enum lx_programs_lifecycle_status {
    LX_PROGRAMS_LIFECYCLE_ACTIVE = 1,
    LX_PROGRAMS_LIFECYCLE_DEPRECATED = 2,
    LX_PROGRAMS_LIFECYCLE_TOMBSTONED = 3
} lx_programs_lifecycle_status;

typedef struct lx_programs_wind_down_view {
    uint8_t program_id[32];
    lx_programs_lifecycle_status status;
    uint8_t exit_program[32];
    uint64_t deadline;
    uint64_t effective_sequence;
    uint16_t value_account_count;
    uint16_t live_value_account_count;
} lx_programs_wind_down_view;

typedef struct lx_programs_exit_route_view {
    uint8_t program_id[32];
    uint8_t account_id[32];
    uint8_t asset_id[32];
    uint8_t destination[32];
    uint16_t seed_length;
    uint8_t seed[LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES];
} lx_programs_exit_route_view;

typedef lxp_result (*lx_programs_exit_route_visit_fn)(
    const lx_programs_exit_route_view *route, void *user);

typedef struct lx_programs_wind_down_history_view {
    uint8_t program_id[32];
    lx_programs_lifecycle_status prior;
    lx_programs_lifecycle_status current;
    uint8_t authority[32];
    uint8_t exit_program[32];
    uint64_t deadline;
    uint64_t effective_sequence;
    uint16_t value_account_count;
    uint16_t live_value_account_count;
    uint8_t account_root[32];
} lx_programs_wind_down_history_view;

typedef lxp_result (*lx_programs_wind_down_history_visit_fn)(
    const lx_programs_wind_down_history_view *history, void *user);

typedef struct lx_programs_fee_schedule {
    uint32_t version;
    uint64_t cpu;
    uint64_t memory_byte;
    uint64_t storage_read_byte;
    uint64_t storage_write_byte;
    uint64_t output_value;
    uint64_t output_byte;
    uint64_t occupancy_byte_batch;
} lx_programs_fee_schedule;

enum {
    LX_PROGRAMS_METER_BASE = 0,
    LX_PROGRAMS_METER_ENTITY = 1,
    LX_PROGRAMS_METER_LOAD = 2,
    LX_PROGRAMS_METER_STORE = 3,
    LX_PROGRAMS_METER_CALL = 4,
    LX_PROGRAMS_METER_BRANCH_KEPT_PER_FUEL = 5,
    LX_PROGRAMS_METER_FUNC_LOCALS_PER_FUEL = 6,
    LX_PROGRAMS_METER_MEMORY_BYTES_PER_FUEL = 7,
    LX_PROGRAMS_METER_TABLE_ELEMENTS_PER_FUEL = 8,
    LX_PROGRAMS_METERING_COEFFICIENTS = 9,
    LX_PROGRAMS_METERING_RECORD_BYTES = 122,
    LX_PROGRAMS_METERING_AUTHORITY_GENESIS = 1,
    LX_PROGRAMS_METERING_AUTHORITY_GOVERNANCE = 2
};

typedef struct lx_programs_metering_schedule {
    uint32_t version;
    uint64_t coefficients[LX_PROGRAMS_METERING_COEFFICIENTS];
    uint64_t activation_batch;
    uint8_t authority_kind;
    uint8_t authority_digest[32];
} lx_programs_metering_schedule;

typedef lxp_result (*lx_programs_metering_schedule_fn)(
    void *context, uint32_t recorded_version, uint64_t batch_number,
    lx_programs_metering_schedule *schedule);

lxp_result lxp_programs_metering_schedule_current(
    const lxp_kernel *kernel, uint64_t batch_number,
    lx_programs_metering_schedule *schedule);
lxp_result lxp_programs_metering_schedule_at(
    const lxp_kernel *kernel, uint32_t recorded_version,
    uint64_t receipt_batch_number,
    lx_programs_metering_schedule *schedule);
lxp_result lxp_programs_metering_resolve_runtime(
    void *context, uint32_t recorded_version, uint64_t batch_number,
    lx_programs_metering_schedule *schedule);
lxp_result lxp_programs_metering_genesis_append(
    lxp_genesis_manifest *manifest,
    const lx_programs_metering_schedule *schedule);
lxp_result lxp_programs_metering_genesis_validate(
    const lxp_genesis_manifest *manifest);
lxp_result lxp_programs_metering_genesis_project(
    const lxp_genesis_manifest *manifest, lxp_arena *arena,
    lxp_kernel *kernel);

/* Canonical CALL payload limits. The activity names a UTF-8 export, carries
 * opaque ABI calldata, and has no ambient or process-global staging state. */
enum {
    LX_PROGRAMS_MAX_ENTRYPOINT_BYTES = 128,
    LX_PROGRAMS_MAX_CAPABILITY_BYTES = 4096,
    LX_PROGRAMS_MAX_CALLDATA_BYTES = 1048576,
    LX_PROGRAMS_MAX_RESPONSE_BYTES = 1048576,
    LX_PROGRAMS_CALL_BUDGET_FIELDS = 7
};

enum {
    LX_PROGRAMS_ACTIVITY_BYTES_WASM = 1,
    LX_PROGRAMS_ACTIVITY_BYTES_MIGRATION_HOOK = 2,
    LX_PROGRAMS_ACTIVITY_BYTES_ENTRYPOINT = 3,
    LX_PROGRAMS_ACTIVITY_BYTES_CALLDATA = 4,
    LX_PROGRAMS_ACTIVITY_BYTES_CAPABILITIES = 5
};

enum {
    LX_PROGRAMS_TERMINAL_BYTES_GRAPH = 0,
    LX_PROGRAMS_TERMINAL_BYTES_PAYLOAD = 1,
    LX_PROGRAMS_TERMINAL_BYTES_EVENTS = 2
};

const lxp_module_iface *programs_module_registration(void);
const lxp_module_iface *programs_module_registration_v2(void);
const lxp_module_iface *lx_programs_module_iface(void);

lxp_result lxp_programs_lifecycle_decode(lxp_module_ctx *ctx,
                                         uint16_t ordinal,
                                         const uint8_t *payload,
                                         size_t payload_length,
                                         void **decoded);
lxp_result lxp_programs_lifecycle_validate(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded);
lxp_result lxp_programs_lifecycle_execute(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects);
void lxp_programs_lifecycle_release(lxp_module_ctx *ctx, void *decoded);

/*
 * Scalar-only admission boundary for an arena-owned CALL activity. `token` is
 * the integer representation of the decoded activity that remains owned by
 * the active lxp_module_ctx; it is never a Rust-owned handle or lookup key.
 * Byte payloads remain in the C activity until the callback bridge supplied by
 * the activity owner consumes them.
 */
lxp_result layerx_programs_call_begin(
    uint64_t token, uint64_t occupancy_token,
    uint64_t p0, uint64_t p1, uint64_t p2, uint64_t p3,
    uint64_t r0, uint64_t r1, uint64_t r2, uint64_t r3,
    uint64_t h0, uint64_t h1, uint64_t h2, uint64_t h3,
    uint64_t b0, uint64_t b1, uint64_t b2, uint64_t b3,
    uint64_t signed_fee_hi, uint64_t signed_fee_lo,
    uint64_t available_fee_hi, uint64_t available_fee_lo,
    uint32_t fee_schedule_version, uint32_t metering_schedule_version,
    uint32_t parameter_version,
    uint64_t meter_base, uint64_t meter_entity, uint64_t meter_load,
    uint64_t meter_store, uint64_t meter_call,
    uint64_t meter_branch_kept_per_fuel,
    uint64_t meter_func_locals_per_fuel,
    uint64_t meter_memory_bytes_per_fuel,
    uint64_t meter_table_elements_per_fuel,
    uint64_t fee_cpu, uint64_t fee_memory_byte,
    uint64_t fee_storage_read_byte, uint64_t fee_storage_write_byte,
    uint64_t fee_output_value, uint64_t fee_output_byte,
    uint64_t fee_occupancy_byte_batch, uint64_t batch_number,
    uint64_t activity_sequence,
    uint16_t protocol_version,
    uint16_t abi_version, uint16_t entrypoint_length,
    uint32_t wasm_length, uint32_t calldata_length, uint16_t capabilities_length,
    uint32_t response_capacity,
    uint64_t cpu_fuel, uint64_t memory_bytes,
    uint64_t storage_read_bytes, uint64_t storage_write_bytes,
    uint64_t output_values, uint64_t output_bytes,
    uint64_t table_elements);

/* Returns one CALL-activity-owned byte (0 through 255) or a negative lxp_result.
 * The token only has meaning while its dispatch arena remains active. */
lxp_result layerx_programs_call_activity_byte(uint64_t token, uint16_t section,
                                              uint32_t offset);

/* Scalar storage projection. selector 0 is program/principal and selector 1
 * is program-shared; section 0 is key and section 1 is value. */
lxp_result layerx_programs_call_storage_cell_count(uint64_t token,
                                                    uint16_t selector);
lxp_result layerx_programs_call_storage_cell_length(
    uint64_t token, uint16_t selector, uint32_t index, uint16_t section);
lxp_result layerx_programs_call_storage_cell_byte(
    uint64_t token, uint16_t selector, uint32_t index, uint16_t section,
    uint32_t offset);
lxp_result layerx_programs_call_storage_final_begin(
    uint64_t token, uint16_t selector, uint32_t count);
lxp_result layerx_programs_call_storage_final_cell(
    uint64_t token, uint16_t selector, uint32_t index,
    uint16_t key_length, uint32_t value_length);
lxp_result layerx_programs_call_storage_final_byte(
    uint64_t token, uint16_t selector, uint32_t index, uint16_t section,
    uint32_t offset, uint8_t byte);
lxp_result layerx_programs_call_storage_final_apply(uint64_t token,
                                                     uint16_t selector);

/* Bounded deployed-program catalog for the exact CALL journal view.  Section
 * 0 is the program id and section 1 the validated registry code hash. */
lxp_result layerx_programs_call_catalog_count(uint64_t token);
lxp_result layerx_programs_call_catalog_wasm_length(uint64_t token,
                                                     uint32_t index);
lxp_result layerx_programs_call_catalog_abi_version(uint64_t token,
                                                    uint32_t index);
lxp_result layerx_programs_call_catalog_identity_byte(
    uint64_t token, uint32_t index, uint16_t section, uint32_t offset);
lxp_result layerx_programs_call_catalog_wasm_byte(uint64_t token,
                                                   uint32_t index,
                                                   uint32_t offset);
lxp_result layerx_programs_call_receipt_view_begin(
    uint64_t token, uint64_t d0, uint64_t d1, uint64_t d2, uint64_t d3);
lxp_result layerx_programs_call_receipt_view_byte(
    uint64_t token, uint16_t section, uint32_t offset);
lxp_result layerx_programs_call_balance_view_begin(
    uint64_t token,
    uint64_t a0, uint64_t a1, uint64_t a2, uint64_t a3,
    uint64_t s0, uint64_t s1, uint64_t s2, uint64_t s3,
    uint64_t d0, uint64_t d1, uint64_t d2, uint64_t d3);
lxp_result layerx_programs_call_balance_view_byte(
    uint64_t token, uint16_t section, uint32_t offset);

/* Per-catalog-program scalar storage projection.  The root-only storage
 * accessors above remain available for the root fixture; composed execution
 * imports and finalizes every deployed catalog member. */
lxp_result layerx_programs_call_catalog_storage_cell_count(
    uint64_t token, uint32_t program_index, uint16_t selector);
lxp_result layerx_programs_call_catalog_storage_cell_length(
    uint64_t token, uint32_t program_index, uint16_t selector,
    uint32_t index, uint16_t section);
lxp_result layerx_programs_call_catalog_storage_cell_byte(
    uint64_t token, uint32_t program_index, uint16_t selector,
    uint32_t index, uint16_t section, uint32_t offset);
lxp_result layerx_programs_call_catalog_storage_final_begin(
    uint64_t token, uint32_t program_index, uint16_t selector, uint32_t count);
lxp_result layerx_programs_call_catalog_storage_final_cell(
    uint64_t token, uint32_t program_index, uint16_t selector, uint32_t index,
    uint16_t key_length, uint32_t value_length);
lxp_result layerx_programs_call_catalog_storage_final_byte(
    uint64_t token, uint32_t program_index, uint16_t selector, uint32_t index,
    uint16_t section, uint32_t offset, uint8_t byte);
lxp_result layerx_programs_call_catalog_storage_final_apply(
    uint64_t token, uint32_t program_index, uint16_t selector);
/* Rust invokes this only after `PreparedAuthorizedActivity::strict_settle`;
 * no namespace replacement is accepted before that affine boundary. */
lxp_result layerx_programs_call_storage_final_authorize(uint64_t token);

/* Receipt-native terminal evidence.  Rust streams canonical graph, terminal,
 * and event-envelope evidence as scalar bytes; C hashes and binds it to the
 * one standard Programs receipt. */
lxp_result layerx_programs_call_terminal_begin(
    uint64_t token, uint8_t terminal_kind, lxp_result result_code,
    uint16_t runtime_version,
    uint16_t abi_version, uint32_t fee_schedule_version,
    uint32_t metering_schedule_version,
    uint64_t cpu_fuel, uint64_t memory_bytes, uint64_t storage_read_bytes,
    uint64_t storage_write_bytes, uint32_t output_values, uint64_t output_bytes,
    uint64_t fee_hi, uint64_t fee_lo,
    uint64_t transfer0, uint64_t transfer1, uint64_t transfer2, uint64_t transfer3,
    uint32_t graph_length, uint32_t terminal_length, uint32_t events_length);
lxp_result layerx_programs_call_terminal_byte(uint64_t token, uint16_t section,
                                               uint32_t offset, uint8_t byte);
lxp_result layerx_programs_call_terminal_publish(uint64_t token);

/* One guest event at a time, copied through scalar fields into the active C
 * journal.  `frame_path` is its canonical eight-byte CallFrameId path. */
lxp_result layerx_programs_call_event_begin(
    uint64_t token, uint32_t event_index,
    uint64_t p0, uint64_t p1, uint64_t p2, uint64_t p3,
    uint64_t r0, uint64_t r1, uint64_t r2, uint64_t r3,
    uint64_t frame_path, uint8_t frame_depth,
    uint16_t topic_length, uint32_t data_length);
lxp_result layerx_programs_call_event_byte(uint64_t token, uint16_t section,
                                            uint32_t offset, uint8_t byte);
lxp_result layerx_programs_call_event_emit(uint64_t token);

lxp_result layerx_programs_call_transfer_begin(uint64_t token,
                                               uint16_t leg_count);
lxp_result layerx_programs_call_transfer_leg(
    uint64_t token, uint16_t index, uint8_t source_kind,
    uint64_t f0, uint64_t f1, uint64_t f2, uint64_t f3,
    uint64_t o0, uint64_t o1, uint64_t o2, uint64_t o3,
    uint64_t p0, uint64_t p1, uint64_t p2, uint64_t p3,
    uint64_t frame_path, uint8_t frame_depth, uint16_t seed_length,
    uint64_t t0, uint64_t t1, uint64_t t2, uint64_t t3,
    uint64_t a0, uint64_t a1, uint64_t a2, uint64_t a3,
    uint64_t amount_hi, uint64_t amount_lo);
lxp_result layerx_programs_call_transfer_seed_byte(
    uint64_t token, uint16_t index, uint16_t offset, uint8_t byte);
lxp_result layerx_programs_call_transfer_apply(uint64_t token);
lxp_result layerx_programs_call_transfer_root_byte(uint64_t token,
                                                    uint32_t offset);

/* Lifecycle uses a distinct decoder token and therefore a distinct byte
 * callback; it must never reinterpret a CALL activity. */
lxp_result layerx_programs_migration_activity_byte(uint64_t token,
                                                   uint16_t section,
                                                   uint32_t offset);

/* Synchronous lifecycle bridge matching the same arena-token rule. */
lxp_result layerx_programs_migration_execute_activity(uint64_t token,
                                                       uint32_t wasm_length,
                                                       uint16_t hook_length,
                                                       uint16_t abi_version,
                                                       uint32_t metering_schedule_version,
                                                       uint64_t meter_base,
                                                       uint64_t meter_entity,
                                                       uint64_t meter_load,
                                                       uint64_t meter_store,
                                                       uint64_t meter_call,
                                                       uint64_t meter_branch_kept_per_fuel,
                                                       uint64_t meter_func_locals_per_fuel,
                                                       uint64_t meter_memory_bytes_per_fuel,
                                                       uint64_t meter_table_elements_per_fuel,
                                                       uint64_t h0, uint64_t h1,
                                                       uint64_t h2, uint64_t h3);

/* Node-local compiled artifacts are explicitly retired after durable protocol
 * transitions; these calls never mutate receipt or execution state. */
lxp_result layerx_programs_module_cache_invalidate_upgrade(
    uint64_t h0, uint64_t h1, uint64_t h2, uint64_t h3);
lxp_result layerx_programs_module_cache_invalidate_runtime(
    uint16_t retired_runtime_version);
lxp_result layerx_programs_module_cache_invalidate_abi(
    uint16_t retired_abi_version);

typedef struct lxp_programs_call_activity lxp_programs_call_activity;

lxp_result lxp_programs_call_decode(lxp_module_ctx *ctx,
                                    const uint8_t *payload,
                                    size_t payload_length,
                                    void **decoded);
lxp_result lxp_programs_call_validate(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded);
lxp_result lxp_programs_call_execute(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects);

lxp_result lxp_programs_account_derive(
    const uint8_t program_id[32], const uint8_t *seed, size_t seed_length,
    uint8_t account_id[LX_PROGRAMS_ACCOUNT_ID_BYTES]);
lxp_result lxp_programs_account_register(
    lxp_module_ctx *ctx, const uint8_t program_id[32], const uint8_t *seed,
    size_t seed_length, const uint8_t asset_id[32], lx_account **account,
    bool *created);
lxp_result lxp_programs_account_lookup(
    lxp_module_ctx *ctx, const uint8_t program_id[32], const uint8_t *seed,
    size_t seed_length, lx_programs_account_binding *binding,
    lx_account **account);
lxp_result lxp_programs_account_lookup_id(
    lxp_module_ctx *ctx, const uint8_t account_id[32],
    lx_programs_account_binding *binding, lx_account **account);
lxp_result lxp_programs_account_iter(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    lx_programs_account_visit_fn visit, void *user);
lxp_result lxp_programs_value_account_read(
    lxp_module_ctx *ctx, const uint8_t account_id[32],
    const uint8_t receipt_digest[32],
    lx_programs_value_account_view *view);
lxp_result lxp_programs_value_account_iter(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    const uint8_t receipt_digest[32],
    lx_programs_value_account_visit_fn visit, void *user);
lxp_result lxp_programs_account_state_head_read(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    const uint8_t receipt_digest[32], lx_programs_account_state_head *head);
lxp_result lxp_programs_state_record_encode(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    const uint8_t receipt_digest[32], lxp_arena *arena,
    lxp_byte_span *encoded);
lxp_result lxp_programs_account_owner_bind(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    const uint8_t owner[32]);
lxp_result lxp_programs_program_abi(
    lxp_module_ctx *ctx, const uint8_t program_id[32], uint16_t *abi_version);
lxp_result lxp_programs_program_active(
    lxp_module_ctx *ctx, const uint8_t program_id[32]);
lxp_result lxp_programs_wind_down_read(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    lx_programs_wind_down_view *view);
lxp_result lxp_programs_exit_route_read(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    const uint8_t account_id[32], lx_programs_exit_route_view *route);
lxp_result lxp_programs_exit_route_iter(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    lx_programs_exit_route_visit_fn visit, void *user);
lxp_result lxp_programs_wind_down_history_iter(
    lxp_module_ctx *ctx, const uint8_t program_id[32],
    lx_programs_wind_down_history_visit_fn visit, void *user);
lxp_result lxp_programs_wind_down_decode(
    lxp_module_ctx *ctx, const uint8_t *payload, size_t payload_length,
    void **decoded);
lxp_result lxp_programs_wind_down_validate(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded);
lxp_result lxp_programs_wind_down_execute(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects);
lxp_result lxp_programs_account_decode(lxp_module_ctx *ctx,
                                       const uint8_t *payload,
                                       size_t payload_length,
                                       void **decoded);
lxp_result lxp_programs_account_validate(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded);
lxp_result lxp_programs_account_execute(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects);

typedef lxp_result (*lx_programs_occupancy_parameters_fn)(
    void *context, uint32_t parameter_version,
    lx_programs_fee_schedule *schedule, uint8_t occupancy_asset_id[32]);

typedef struct lx_programs_transfer_runtime {
    lx_account_registry *accounts;
    const lxp_transfer_asset_state *assets;
    size_t asset_count;
    lx_programs_fee_schedule fee_schedule;
    uint8_t occupancy_asset_id[32];
    lx_programs_occupancy_parameters_fn resolve_occupancy_parameters;
    void *occupancy_parameter_context;
    const lx_programs_state_feed *state_feed;
    lx_programs_metering_schedule_fn resolve_metering_schedule;
    void *metering_schedule_context;
} lx_programs_transfer_runtime;

enum { LXP_PROGRAMS_OCCUPANCY_MAX_PAYERS = LXP_MAX_TRANSFER_SET_LEGS };
typedef struct lxp_programs_occupancy_payer_receipt {
    uint8_t principal[32];
    lxp_u128 due;
    lxp_u128 paid;
    lxp_u128 arrears;
    bool frozen;
} lxp_programs_occupancy_payer_receipt;

typedef struct lxp_programs_occupancy_receipt {
    uint64_t batch_number;
    uint64_t global_sequence;
    uint32_t parameter_version;
    uint32_t schedule_version;
    uint64_t schedule_prices[7];
    uint8_t occupancy_asset_id[32];
    lxp_u128 byte_batches;
    lxp_u128 fee_units;
    lxp_u128 paid_fee_units;
    lxp_u128 arrears_fee_units;
    lxp_programs_occupancy_payer_receipt
        payers[LXP_PROGRAMS_OCCUPANCY_MAX_PAYERS];
    uint16_t payer_count;
    uint8_t schedule_commitment[32];
    lxp_byte_span settlement_evidence;
    uint8_t settlement_evidence_digest[32];
    uint8_t ledger_root[32];
    uint8_t transfer_set_root[32];
    uint8_t previous_state_root[32];
    uint8_t resulting_state_root[32];
} lxp_programs_occupancy_receipt;

/* The batch coordinator invokes this exactly once after the batch's activity
 * transitions and before sealing roots, including for an empty batch. The
 * encoded receipt is included in the canonical batch receipt set. */
lxp_result lxp_programs_finalize_occupancy_batch(
    lxp_kernel *kernel, uint64_t batch_number, uint64_t batch_timestamp_ms,
    uint64_t global_sequence, uint32_t parameter_version, lxp_arena *arena,
    lxp_programs_occupancy_receipt *receipt, lxp_byte_span *encoded);
lxp_result lxp_programs_occupancy_receipt_encode(
    const lxp_programs_occupancy_receipt *receipt, lxp_arena *arena,
    lxp_byte_span *encoded);
lxp_result lxp_programs_occupancy_receipt_decode(
    const uint8_t *bytes, size_t length,
    lxp_programs_occupancy_receipt *receipt);
lxp_result lxp_programs_replay_finalize(
    void *context, const lxp_batch_header *header, uint32_t parameter_version,
    uint64_t system_sequence, const uint8_t previous_state_root[32],
    lxp_arena *arena, lxp_replay_activity_output *output);
lxp_result lxp_programs_replay_engine_bind(lxp_replay_engine *engine,
                                           lxp_kernel *kernel);

lxp_result lxp_programs_bind_fee_transaction(lxp_kernel *kernel);
lxp_result lxp_programs_bind_state_feed(
    lxp_kernel *kernel, const lx_programs_state_feed *feed);
lxp_result lxp_programs_state_feed_observe(
    const lx_programs_state_feed *feed, const lxp_activity *activity,
    const lxp_receipt *receipt);

lxp_result lxp_programs_transfer_decode(lxp_module_ctx *ctx,
                                        const uint8_t *payload,
                                        size_t payload_length,
                                        void **decoded);
lxp_result lxp_programs_transfer_validate(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded);
lxp_result lxp_programs_transfer_execute(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects);

lxp_result layerx_programs_authorize_402lxp_leg(
    uint64_t p0, uint64_t p1, uint64_t p2, uint64_t p3,
    uint64_t r0, uint64_t r1, uint64_t r2, uint64_t r3,
    uint64_t h0, uint64_t h1, uint64_t h2, uint64_t h3,
    uint64_t a0, uint64_t a1, uint64_t a2, uint64_t a3,
    uint64_t t0, uint64_t t1, uint64_t t2, uint64_t t3,
    uint64_t amount_hi, uint64_t amount_lo);

lxp_result layerx_programs_settle_wind_down_402lxp_leg(
    uint64_t token,
    uint64_t p0, uint64_t p1, uint64_t p2, uint64_t p3,
    uint64_t r0, uint64_t r1, uint64_t r2, uint64_t r3,
    uint64_t h0, uint64_t h1, uint64_t h2, uint64_t h3,
    const uint8_t *seed, size_t seed_length,
    uint64_t s0, uint64_t s1, uint64_t s2, uint64_t s3,
    uint64_t a0, uint64_t a1, uint64_t a2, uint64_t a3,
    uint64_t t0, uint64_t t1, uint64_t t2, uint64_t t3,
    uint64_t amount_hi, uint64_t amount_lo);
lxp_result layerx_programs_wind_down_transfer_begin(
    uint64_t token, uint64_t program_spend_token, uint8_t source_kind,
    uint64_t f0, uint64_t f1, uint64_t f2, uint64_t f3,
    uint64_t o0, uint64_t o1, uint64_t o2, uint64_t o3,
    uint64_t p0, uint64_t p1, uint64_t p2, uint64_t p3,
    uint64_t frame_path, uint8_t frame_depth, uint16_t seed_length,
    uint64_t t0, uint64_t t1, uint64_t t2, uint64_t t3,
    uint64_t a0, uint64_t a1, uint64_t a2, uint64_t a3,
    uint64_t amount_hi, uint64_t amount_lo);
lxp_result layerx_programs_wind_down_transfer_seed_byte(
    uint64_t token, uint16_t offset, uint8_t byte);
lxp_result layerx_programs_wind_down_transfer_apply(uint64_t token);
lxp_result layerx_programs_wind_down_transfer_root_byte(uint64_t token,
                                                         uint32_t offset);
lxp_result layerx_programs_consume_program_spend_authorization(
    uint64_t token, uint16_t origin_module_id,
    const uint8_t from[32], const uint8_t to[32],
    const uint8_t asset_id[32], uint64_t amount_hi, uint64_t amount_lo,
    uint16_t reason, uint8_t supply_mode,
    const uint8_t transfer_set_root[32]);

#endif
