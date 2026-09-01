#ifndef LAYERX_LXP_GENESIS_H
#define LAYERX_LXP_GENESIS_H

#include "layerx/lxp_codec.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LXP_GENESIS_MAX_PARAMETERS = 64,
    LXP_GENESIS_MAX_GUARANTORS = 32,
    LXP_GENESIS_MAX_ACCOUNTS = 256,
    LXP_GENESIS_MAX_MODULE_VALUES = 128,
    LXP_GENESIS_MODULE_VALUE_BYTES = 256,
    LXP_GENESIS_MAX_ENCODED_BYTES = 262144,
    LXP_IMPORT_SECTION_COUNT = 11,
    LXP_IMPORT_MAX_ITEMS = 256,
    LXP_IMPORT_MAX_ASSET_TOTALS = 32,
    LXP_GENESIS_REGISTRATION_BYTES = 82,
    LXP_GENESIS_FRESH_SYSTEM_ACCOUNT_COUNT = 3
};

typedef enum lxp_import_section_kind {
    LXP_IMPORT_USDX_BALANCES = 1,
    LXP_IMPORT_VAULT_RESERVES = 2,
    LXP_IMPORT_OPEN_HOLDS = 3,
    LXP_IMPORT_QUEUED_WITHDRAWALS = 4,
    LXP_IMPORT_LIQUIDITY_POOLS = 5,
    LXP_IMPORT_INSURANCE_POOLS = 6,
    LXP_IMPORT_PERPS_POSITIONS = 7,
    LXP_IMPORT_PENDING_ORDERS = 8,
    LXP_IMPORT_FUNDING_STATE = 9,
    LXP_IMPORT_DID_EVM_BINDINGS = 10,
    LXP_IMPORT_HISTORICAL_COMMITMENTS = 11
} lxp_import_section_kind;

typedef struct lxp_import_item {
    uint8_t item_id[32];
    uint8_t account_id[32];
    uint8_t parent_account_id[32];
    uint8_t asset_id[32];
    lxp_u128 amount;
    uint8_t payload_hash[32];
    bool can_authorize_balance;
    bool immutable;
    bool re_executable;
} lxp_import_item;

typedef struct lxp_import_asset_total {
    uint8_t asset_id[32];
    lxp_u128 amount;
} lxp_import_asset_total;

typedef struct lxp_import_section {
    lxp_import_section_kind kind;
    lxp_import_item items[LXP_IMPORT_MAX_ITEMS];
    size_t item_count;
    lxp_import_asset_total asset_totals[LXP_IMPORT_MAX_ASSET_TOTALS];
    size_t asset_total_count;
} lxp_import_section;
#define lxp_import_section lxp_import_section

typedef struct lxp_import_totals_report {
    size_t section_count;
    size_t item_counts[LXP_IMPORT_SECTION_COUNT];
    lxp_import_asset_total
        asset_totals[LXP_IMPORT_SECTION_COUNT][LXP_IMPORT_MAX_ASSET_TOTALS];
    size_t asset_total_counts[LXP_IMPORT_SECTION_COUNT];
} lxp_import_totals_report;

typedef struct lxp_custody_attested_asset {
    uint8_t asset_id[32];
    lxp_u128 amount;
} lxp_custody_attested_asset;

typedef struct lxp_custody_attestation {
    uint32_t network_id;
    uint8_t checkpoint_id[32];
    uint8_t custody_state_root[32];
    lxp_custody_attested_asset assets[LXP_IMPORT_MAX_ASSET_TOTALS];
    size_t asset_count;
    uint8_t paxeer_public_key[32];
    uint8_t signature[64];
} lxp_custody_attestation;

typedef struct lxp_genesis_reconcile_report {
    bool matched;
    uint8_t mismatch_asset_id[32];
    lxp_u128 attested_amount;
    lxp_u128 computed_amount;
    lxp_u128 difference;
    bool computed_exceeds_attested;
} lxp_genesis_reconcile_report;

typedef enum lxp_genesis_input_kind {
    LXP_GENESIS_INPUT_MANIFEST = 1,
    LXP_GENESIS_INPUT_DATABASE = 2,
    LXP_GENESIS_INPUT_INDEX = 3,
    LXP_GENESIS_INPUT_LOG = 4
} lxp_genesis_input_kind;

typedef struct lxp_genesis_parameter {
    uint16_t module_id;
    uint8_t key[32];
    uint8_t value[32];
} lxp_genesis_parameter;

typedef struct lxp_genesis_guarantor {
    uint8_t guarantor_id[32];
    uint8_t public_key[33];
    lxp_u128 bond;
} lxp_genesis_guarantor;

typedef struct lxp_genesis_account {
    uint8_t account_id[32];
    uint8_t asset_id[32];
    lxp_u128 balance;
    bool locked;
    uint16_t subaccount_kind;
    uint8_t parent_account_id[32];
} lxp_genesis_account;

typedef struct lxp_genesis_module_value {
    uint16_t module_id;
    uint8_t key[32];
    uint8_t value[LXP_GENESIS_MODULE_VALUE_BYTES];
    size_t value_length;
} lxp_genesis_module_value;

typedef struct lxp_genesis_manifest {
    uint16_t protocol_version;
    uint32_t network_id;
    uint64_t genesis_timestamp_ms;
    lxp_genesis_parameter parameters[LXP_GENESIS_MAX_PARAMETERS];
    size_t parameter_count;
    lxp_genesis_guarantor guarantors[LXP_GENESIS_MAX_GUARANTORS];
    size_t guarantor_count;
    lxp_genesis_account accounts[LXP_GENESIS_MAX_ACCOUNTS];
    size_t account_count;
    lxp_genesis_module_value module_values[LXP_GENESIS_MAX_MODULE_VALUES];
    size_t module_value_count;
    uint8_t genesis_state_root[32];
    uint8_t genesis_receipt_state_root[32];
    uint8_t signer_public_key[32];
    uint8_t signature[64];
} lxp_genesis_manifest;
#define lxp_genesis_manifest lxp_genesis_manifest

typedef struct lxp_genesis_registration {
    uint32_t network_id;
    uint64_t registration_index;
    uint8_t checkpoint_id[32];
    uint8_t state_root[32];
    bool finalised;
} lxp_genesis_registration;

typedef struct lxp_genesis_bootstrap_registration {
    uint32_t network_id;
    uint64_t registration_index;
    uint8_t settlement_anchor[32];
    uint8_t state_root[32];
    bool finalised;
} lxp_genesis_bootstrap_registration;

struct lxp_kernel;
struct lxp_snapshot_manifest_record;

lxp_result lxp_genesis_encode(
    const lxp_genesis_manifest *manifest, bool include_signature,
    lxp_arena *arena, lxp_byte_span *encoded);
lxp_result lxp_genesis_parse(
    const uint8_t *bytes, size_t length, lxp_genesis_input_kind input_kind,
    lxp_genesis_manifest *manifest);
lxp_result lxp_genesis_state_root(
    const lxp_genesis_manifest *manifest, lxp_arena *arena,
    uint8_t state_root[32]);
lxp_result lxp_genesis_manifest_commitment(
    const lxp_genesis_manifest *manifest, lxp_arena *arena,
    uint8_t digest[32]);
lxp_result lxp_genesis_materialize(
    const lxp_genesis_manifest *manifest, lxp_arena *arena,
    struct lxp_kernel *kernel);
lxp_result lxp_genesis_fresh_empty_accounts(
    lxp_genesis_manifest *manifest, const uint8_t asset_id[32]);
lxp_result lxp_genesis_parameter_version(
    const lxp_genesis_manifest *manifest, uint32_t *parameter_version);
lxp_result lxp_genesis_receipt_state_root(
    uint32_t network_id, const uint8_t canonical_state_root[32],
    uint8_t receipt_state_root[32]);
lxp_result lxp_genesis_registration_encode(
    const lxp_genesis_bootstrap_registration *registration,
    uint8_t encoded[LXP_GENESIS_REGISTRATION_BYTES]);
lxp_result lxp_genesis_registration_parse(
    const uint8_t *encoded, size_t encoded_length,
    lxp_genesis_bootstrap_registration *registration);
lxp_result lxp_genesis_verify_signature(
    const lxp_genesis_manifest *manifest, lxp_arena *arena);
lxp_result lxp_genesis_accept(
    const lxp_genesis_manifest *manifest,
    const lxp_genesis_bootstrap_registration *registration,
    bool storage_empty, lxp_arena *arena, bool *activities_enabled);
lxp_result lxp_genesis_bootstrap_verify(
    const lxp_genesis_manifest *manifest,
    const lxp_genesis_bootstrap_registration *registration,
    uint32_t configured_network_id, bool storage_empty,
    const struct lxp_snapshot_manifest_record *snapshot,
    const struct lxp_kernel *kernel, lxp_arena *arena,
    bool *activities_enabled);
lxp_result lxp_genesis_main(
    const uint8_t *manifest_bytes, size_t manifest_length,
    const lxp_genesis_bootstrap_registration *registration,
    bool storage_empty, lxp_arena *arena, bool *activities_enabled);
lxp_result lxp_import_balances(
    const lxp_import_section *section, lxp_genesis_manifest *manifest);
lxp_result lxp_import_positions(
    const lxp_import_section *section, lxp_genesis_manifest *manifest);
lxp_result lxp_import_bindings(
    const lxp_import_section *section, lxp_genesis_manifest *manifest);
lxp_result lxp_import_history(
    const lxp_import_section *section, lxp_genesis_manifest *manifest);
lxp_result lxp_import_totals(
    const lxp_import_section *sections, size_t section_count,
    lxp_import_totals_report *report);
lxp_result lxp_custody_attestation_verify(
    const lxp_custody_attestation *attestation,
    const lxp_genesis_registration *finalised_state,
    const uint8_t expected_paxeer_public_key[32]);
lxp_result lxp_genesis_reject(
    lxp_genesis_manifest *accepted_manifest,
    lxp_genesis_reconcile_report *report,
    const uint8_t asset_id[32], lxp_u128 attested,
    lxp_u128 computed);
lxp_result lxp_genesis_reconcile(
    const lxp_genesis_manifest *candidate,
    const lxp_custody_attestation *attestation,
    const lxp_genesis_registration *finalised_state,
    const uint8_t expected_paxeer_public_key[32],
    lxp_genesis_manifest *accepted_manifest,
    lxp_genesis_reconcile_report *report);

#endif
