#ifndef LAYERX_CMD_LXP_VERIFY_CLI_H
#define LAYERX_CMD_LXP_VERIFY_CLI_H

#include "layerx/lxp_arena.h"
#include "layerx/lxp_batch.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lxp_guarantor.h"
#include "layerx/lxp_merkle.h"
#include "layerx/lxp_receipt.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LXP_VERIFY_TRUST_ROOT_MAX_BYTES = LXP_GENESIS_MAX_ENCODED_BYTES,
    LXP_VERIFY_VALUE_MAX_BYTES = LXP_MAX_ACTIVITY_BYTES,
    LXP_VERIFY_PROOF_MAX_BYTES = 8192,
    LXP_VERIFY_WIRE_VERSION = 1,
    LXP_VERIFY_BUNDLE_ACTIVITY = 1,
    LXP_VERIFY_BUNDLE_RECEIPT = 3
};

/*
 * The trust root is the genesis manifest a relying party pins.  Genesis is
 * signed by the key that also authorizes every batch header and every receipt
 * (platform/hosted/node/bootstrap.sh hands the sequencer key to
 * layerx-genesis-build as the genesis signer), so the manifest carries the
 * whole authority key set: the signing authority for headers and receipts,
 * and the guarantor keys that carry checkpoint attestations.
 */
typedef struct lxp_verify_trust_root {
    uint16_t protocol_version;
    uint32_t network_id;
    uint64_t genesis_timestamp_ms;
    uint8_t authority_public_key[32];
    uint8_t manifest_commitment[32];
    uint8_t genesis_state_root[32];
    lxp_guarantor_key_record guarantors[LXP_GENESIS_MAX_GUARANTORS];
    size_t guarantor_count;
} lxp_verify_trust_root;

typedef struct lxp_verify_receipt_report {
    uint8_t activity_id[32];
    uint8_t receipt_digest[32];
    uint8_t asset[32];
    uint8_t previous_state_root[32];
    uint8_t resulting_state_root[32];
    uint8_t batch_id[32];
    lxp_u128 amount;
    lxp_u128 fee_charged;
    uint64_t global_sequence;
    uint64_t timestamp;
    lxp_result result_code;
    uint16_t protocol_version;
    uint16_t module_id;
    uint8_t operation;
} lxp_verify_receipt_report;

typedef struct lxp_verify_bundle_report {
    uint8_t kind;
    uint8_t activity_id[32];
    uint8_t header_hash[32];
    uint8_t leaf_hash[32];
    uint8_t sequencer_id[32];
    uint8_t previous_state_root[32];
    uint8_t resulting_state_root[32];
    uint32_t leaf_index;
    uint32_t leaf_count;
    uint32_t network_id;
    uint16_t protocol_version;
    uint64_t batch_number;
    uint64_t first_sequence;
    uint64_t last_sequence;
    uint64_t timestamp_ms;
    lxp_verify_receipt_report receipt;
    bool receipt_present;
} lxp_verify_bundle_report;

lxp_result lxp_verify_trust_root_load(
    const uint8_t *manifest_bytes, size_t manifest_length, lxp_arena *arena,
    lxp_verify_trust_root *trust_root, const char **stage);
lxp_result lxp_verify_receipt_offline_pinned(
    const lxp_verify_trust_root *trust_root, const uint8_t *receipt_bytes,
    size_t receipt_length, lxp_arena *arena,
    lxp_verify_receipt_report *report, const char **stage);
lxp_result lxp_verify_bundle_offline(
    const lxp_verify_trust_root *trust_root, const uint8_t *value_bytes,
    size_t value_length, const uint8_t *proof_bytes, size_t proof_length,
    lxp_arena *arena, lxp_verify_bundle_report *report, const char **stage);
int lxp_verify_cli_main(int argc, char **argv);

#endif
