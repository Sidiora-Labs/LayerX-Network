#ifndef LAYERX_LXP_RECEIPT_H
#define LAYERX_LXP_RECEIPT_H

#include "layerx/lxp_protocol.h"
#include "layerx/lxp_codec.h"
#include "layerx/lxp_result.h"
#include "layerx/lxp_u128.h"

#include <stddef.h>
#include <stdint.h>

typedef enum lxp_effect_kind {
    LXP_EFFECT_STATE = 1,
    LXP_EFFECT_TRANSFER = 2,
    LXP_EFFECT_EVENT = 3
} lxp_effect_kind;

typedef struct lxp_effect {
    uint16_t module_id;
    uint16_t ordinal;
    uint16_t event_type;
    lxp_effect_kind kind;
    bool monetary;
    uint8_t transfer_set_root[32];
    uint16_t body_length;
    uint8_t body[256];
} lxp_effect;

typedef struct lxp_effect_buffer {
    lxp_effect effects[LXP_MAX_EFFECTS];
    size_t count;
} lxp_effect_buffer;

typedef enum lxp_program_terminal_kind {
    LXP_PROGRAM_TERMINAL_NONE = 0,
    LXP_PROGRAM_TERMINAL_SUCCESS = 1,
    LXP_PROGRAM_TERMINAL_FAILURE = 2,
    LXP_PROGRAM_TERMINAL_RESOURCE = 3
} lxp_program_terminal_kind;

enum { LXP_PROGRAM_METERING_SCHEDULE_VERSION_V1 = 1 };

/* Receipt-native Programs outcome evidence.  These fields describe one
 * terminal runtime outcome; they are not a second receipt or a version alias. */
typedef struct lxp_program_outcome {
    bool present;
    uint8_t encoding_version;
    uint8_t terminal_kind;
    lxp_result result_code;
    uint16_t runtime_version;
    uint16_t abi_version;
    uint32_t fee_schedule_version;
    uint32_t metering_schedule_version;
    uint64_t cpu_fuel;
    uint64_t memory_bytes;
    uint64_t storage_read_bytes;
    uint64_t storage_write_bytes;
    uint32_t output_values;
    uint64_t output_bytes;
    lxp_u128 occupancy_byte_batches;
    lxp_u128 occupancy_fee_units;
    uint64_t fee_schedule_prices[7];
    uint8_t occupancy_asset_id[32];
    uint8_t occupancy_evidence_digest[32];
    uint8_t occupancy_transfer_root[32];
    lxp_u128 fee_units;
    uint8_t call_graph_root[32];
    lxp_byte_span call_graph_payload;
    uint8_t terminal_payload_root[32];
    lxp_byte_span terminal_payload;
    uint8_t transfer_root[32];
} lxp_program_outcome;

typedef struct lxp_receipt {
    uint16_t protocol_version;
    uint8_t activity_id[32];
    uint64_t global_sequence;
    uint8_t previous_state_root[32];
    uint8_t resulting_state_root[32];
    uint8_t activity_root[32];
    lxp_result result_code;
    lxp_effect_buffer effects;
    lxp_u128 fee_charged;
    uint8_t batch_id[32];
    uint16_t module_id;
    uint32_t module_version;
    uint32_t parameter_version;
    uint8_t operation;
    uint8_t asset[32];
    lxp_u128 amount;
    uint8_t from[32];
    lxp_u128 from_balance_before;
    lxp_u128 from_balance_after;
    uint64_t from_sequence;
    uint8_t to[32];
    lxp_u128 to_balance_before;
    lxp_u128 to_balance_after;
    uint8_t transfer_set_root[32];
    uint8_t authorization_hash[32];
    uint8_t context_hash[32];
    uint64_t timestamp;
    lxp_program_outcome program_outcome;
    uint8_t sequencer_signature[64];
} lxp_receipt;

enum { LXP_VERIFIED_RECEIPT_INDEX_MAX = 4096 };

typedef struct lxp_verified_receipt_facts {
    uint8_t receipt_digest[32];
    lxp_result result_code;
    uint64_t global_sequence;
    uint64_t timestamp;
    uint8_t asset[32];
    lxp_u128 amount;
    uint8_t resulting_state_root[32];
} lxp_verified_receipt_facts;

typedef lxp_result (*lxp_verified_receipt_fallback_fn)(
    void *context, const uint8_t receipt_digest[32],
    lxp_verified_receipt_facts *facts);

typedef struct lxp_verified_receipt_index {
    lxp_verified_receipt_facts entries[LXP_VERIFIED_RECEIPT_INDEX_MAX];
    size_t count;
    lxp_verified_receipt_fallback_fn fallback;
    void *fallback_context;
} lxp_verified_receipt_index;

typedef struct lxp_ledger_receipt_input {
    uint8_t transaction_id[32];
    uint8_t operation;
    uint64_t global_sequence;
    uint8_t asset[32];
    lxp_u128 amount;
    uint8_t from[32];
    lxp_u128 from_balance_before;
    lxp_u128 from_balance_after;
    uint64_t from_sequence;
    uint8_t to[32];
    lxp_u128 to_balance_before;
    lxp_u128 to_balance_after;
    uint8_t transfer_set_root[32];
    uint8_t authorization_hash[32];
    uint8_t context_hash[32];
    uint8_t previous_state_root[32];
    uint8_t resulting_state_root[32];
    uint8_t batch_id[32];
    uint64_t timestamp;
    size_t leg_count;
} lxp_ledger_receipt_input;

#define lxp_effect_buffer lxp_effect_buffer
#define lxp_program_outcome lxp_program_outcome
#define lxp_receipt lxp_receipt

lxp_result lxp_effect_buffer_init(lxp_effect_buffer *buffer);
lxp_result lxp_effect_buffer_add(lxp_effect_buffer *buffer,
                                 const lxp_effect *effect);
lxp_result lxp_effect_event_root(const lxp_effect_buffer *buffer,
                                 lxp_arena *arena, uint8_t root[32]);
lxp_result lxp_receipt_build(lxp_receipt *receipt,
                             const uint8_t activity_id[32],
                             uint64_t global_sequence,
                             const uint8_t previous_state_root[32],
                             const uint8_t resulting_state_root[32],
                             const uint8_t activity_root[32],
                             lxp_result result_code,
                             const lxp_effect_buffer *effects,
                             lxp_u128 fee_charged,
                             const uint8_t batch_id[32], uint16_t module_id,
                             uint32_t module_version,
                             uint32_t parameter_version);
lxp_result lxp_receipt_bind_program_outcome(
    lxp_receipt *receipt, const lxp_program_outcome *outcome);
lxp_result lxp_program_outcome_validate(const lxp_program_outcome *outcome);
lxp_result lxp_program_outcome_validate_for_protocol(
    const lxp_program_outcome *outcome, uint16_t protocol_version);
bool lxp_program_metering_schedule_available(uint32_t schedule_version);
lxp_result lxp_receipt_encode(const lxp_receipt *receipt,
                              bool include_signature, lxp_arena *arena,
                              lxp_byte_span *encoded);
lxp_result lxp_receipt_decode(const uint8_t *bytes, size_t length,
                              bool require_signature, lxp_receipt *receipt);
lxp_result lxp_receipt_sign(lxp_receipt *receipt,
                            const uint8_t private_key[32], lxp_arena *arena);
lxp_result lxp_receipt_verify(const lxp_receipt *receipt,
                              const uint8_t public_key[32], lxp_arena *arena);
lxp_result lxp_receipt_digest(const lxp_receipt *receipt, lxp_arena *arena,
                              uint8_t digest[32]);
lxp_result lxp_verified_receipt_index_init(lxp_verified_receipt_index *index);
lxp_result lxp_verified_receipt_index_bind_fallback(
    lxp_verified_receipt_index *index,
    lxp_verified_receipt_fallback_fn fallback, void *context);
lxp_result lxp_verified_receipt_index_add(
    lxp_verified_receipt_index *index, const lxp_receipt *receipt,
    const uint8_t sequencer_public_key[32], lxp_arena *arena);
lxp_result lxp_verified_receipt_index_lookup(
    const lxp_verified_receipt_index *index,
    const uint8_t receipt_digest[32], lxp_verified_receipt_facts *facts);
lxp_result lxp_ledger_receipt_build(lxp_receipt *receipt,
                                    const lxp_ledger_receipt_input *input);
struct lxp_log;
lxp_result lxp_ledger_receipt_issue(lxp_receipt *receipt,
                                    const lxp_ledger_receipt_input *input,
                                    const uint8_t private_key[32],
                                    lxp_arena *arena, struct lxp_log *log);
lxp_result lxp_balance_writer_guard(bool through_ledger_primitive);

#endif
