#ifndef LAYERX_PLATFORM_EMULATOR_CORE_H
#define LAYERX_PLATFORM_EMULATOR_CORE_H

#include <stddef.h>
#include <stdint.h>

typedef struct platform_emulator platform_emulator;

typedef struct platform_emulator_receipt {
    uint8_t activity_id[32];
    uint8_t batch_id[32];
    uint8_t state_root[32];
    uint8_t previous_state_root[32];
    uint8_t asset[32];
    uint8_t sequencer_public_key[32];
    uint64_t global_sequence;
    int32_t result_code;
    uint64_t metered_cost_hi;
    uint64_t metered_cost_lo;
    const uint8_t *bytes;
    size_t length;
    const uint8_t *terminal_payload;
    size_t terminal_payload_length;
    const uint8_t *call_graph;
    size_t call_graph_length;
    platform_emulator *isolated_owner;
} platform_emulator_receipt;

typedef struct platform_emulator_state {
    uint8_t state_root[32];
    uint64_t next_sequence;
    uint64_t batch_number;
    uint64_t timestamp_ms;
    size_t cell_count;
    size_t account_count;
} platform_emulator_state;

typedef struct platform_emulator_program {
    uint8_t program_id[32];
    uint8_t code_hash[32];
    uint8_t deployment_receipt_digest[32];
    uint32_t version;
    uint16_t abi_version;
    uint8_t lifecycle;
    const uint8_t *interface_bytes;
    size_t interface_length;
    uint8_t has_interface;
    uint8_t state_root[32];
    uint64_t observed_sequence;
} platform_emulator_program;

platform_emulator *platform_emulator_create(uint32_t network_id,
                                             uint64_t timestamp_ms,
                                             const uint8_t sequencer_seed[32]);
void platform_emulator_destroy(platform_emulator *emulator);
const char *platform_emulator_error_name(int32_t result);
int32_t platform_emulator_set_time(platform_emulator *emulator,
                                   uint64_t timestamp_ms);
int32_t platform_emulator_advance_time(platform_emulator *emulator,
                                       uint64_t delta_ms);
int32_t platform_emulator_inject_failure(platform_emulator *emulator,
                                         uint32_t kind, uint64_t count);
int32_t platform_emulator_prefund(platform_emulator *emulator,
                                  const uint8_t *did, size_t did_length,
                                  const uint8_t public_key[32],
                                  uint64_t amount_hi, uint64_t amount_lo);
int32_t platform_emulator_execute(platform_emulator *emulator,
                                  const uint8_t *activity, size_t length,
                                  platform_emulator_receipt *receipt);
int32_t platform_emulator_simulate(platform_emulator *emulator,
                                   const uint8_t *activity,
                                   size_t length,
                                   platform_emulator_receipt *receipt);
int32_t platform_emulator_inspect(const platform_emulator *emulator,
                                  platform_emulator_state *state);
int32_t platform_emulator_program_read(platform_emulator *emulator,
                                       const uint8_t program_id[32],
                                       platform_emulator_program *program);
size_t platform_emulator_program_count(const platform_emulator *emulator);
int32_t platform_emulator_program_at(const platform_emulator *emulator,
                                     size_t index, uint8_t program_id[32]);
int32_t platform_emulator_cell(const platform_emulator *emulator, size_t index,
                               uint8_t key[32], uint64_t *value_hi,
                               uint64_t *value_lo);
int32_t platform_emulator_account(const platform_emulator *emulator,
                                  size_t index, uint8_t id[32],
                                  const uint8_t **name, size_t *name_length,
                                  uint64_t *balance_hi, uint64_t *balance_lo);
int32_t platform_emulator_snapshot_export(platform_emulator *emulator,
                                          const uint8_t **bytes,
                                          size_t *length);
int32_t platform_emulator_snapshot_import(platform_emulator *emulator,
                                          const uint8_t *bytes,
                                          size_t length);
void platform_emulator_receipt_release(platform_emulator_receipt *receipt);

#endif
