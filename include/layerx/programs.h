#ifndef LAYERX_PROGRAMS_H
#define LAYERX_PROGRAMS_H

#include "layerx/lxp_module.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_transfer.h"

#include <stdint.h>

typedef struct lxp_kernel lxp_kernel;

enum {
    LX_PROGRAMS_DEPLOY = 0x00090001,
    LX_PROGRAMS_UPGRADE = 0x00090002,
    LX_PROGRAMS_CALL = 0x00090003,
    LX_PROGRAMS_REGISTRY = 0x00090004,
    LX_PROGRAMS_TRANSFER = 0x00090005,
    LX_PROGRAMS_ABI_VERSION = 1,
    LX_PROGRAMS_EVENT_DEPLOYED = 1,
    LX_PROGRAMS_EVENT_UPGRADED = 2,
    LX_PROGRAMS_EVENT_CALLED = 3,
    LX_PROGRAMS_EVENT_REGISTRY_READ = 4,
    LX_PROGRAMS_EVENT_TRANSFERRED = 5,
    LX_PROGRAMS_EVENT_GUEST_ENVELOPE = 6,
    LX_PROGRAMS_EVENT_CALL_OUTCOME = 7
};

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
    uint64_t token,
    uint64_t p0, uint64_t p1, uint64_t p2, uint64_t p3,
    uint64_t r0, uint64_t r1, uint64_t r2, uint64_t r3,
    uint64_t h0, uint64_t h1, uint64_t h2, uint64_t h3,
    uint64_t b0, uint64_t b1, uint64_t b2, uint64_t b3,
    uint64_t signed_fee_hi, uint64_t signed_fee_lo,
    uint64_t available_fee_hi, uint64_t available_fee_lo,
    uint16_t fee_schedule_version, uint32_t parameter_version,
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
lxp_result layerx_programs_call_catalog_identity_byte(
    uint64_t token, uint32_t index, uint16_t section, uint32_t offset);
lxp_result layerx_programs_call_catalog_wasm_byte(uint64_t token,
                                                   uint32_t index,
                                                   uint32_t offset);
lxp_result layerx_programs_call_receipt_view_begin(
    uint64_t token, uint64_t d0, uint64_t d1, uint64_t d2, uint64_t d3);
lxp_result layerx_programs_call_receipt_view_byte(
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
    uint64_t token, uint16_t index,
    uint64_t f0, uint64_t f1, uint64_t f2, uint64_t f3,
    uint64_t t0, uint64_t t1, uint64_t t2, uint64_t t3,
    uint64_t a0, uint64_t a1, uint64_t a2, uint64_t a3,
    uint64_t amount_hi, uint64_t amount_lo);
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
                                                       uint16_t hook_length);

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

typedef struct lx_programs_transfer_runtime {
    lx_account_registry *accounts;
    const lxp_transfer_asset_state *assets;
    size_t asset_count;
} lx_programs_transfer_runtime;

lxp_result lxp_programs_bind_fee_transaction(lxp_kernel *kernel);

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

#endif
