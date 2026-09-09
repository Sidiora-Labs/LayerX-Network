#ifndef LAYERX_LXP_GENESIS_BUILDER_H
#define LAYERX_LXP_GENESIS_BUILDER_H

#include "layerx/lxp_genesis.h"
#include "layerx/lxp_snapshot.h"
#include "layerx/programs.h"
#include "layerx/lxp_bridge_credit.h"

enum {
    LXP_GENESIS_REGISTRATION_REQUEST_BYTES = 73,
    LXP_GENESIS_DEPLOYMENT_DESCRIPTOR_BYTES = 105,
    LXP_ISSUANCE_MIGRATION_BYTES = 301
};

lxp_result lxp_genesis_build_fresh_empty(
    const lxp_genesis_manifest *draft, const uint8_t asset_id[32],
    const lx_programs_metering_schedule *metering,
    const lx_programs_fee_genesis_parameters *fees,
    const uint8_t signer_private_key[32], lxp_arena *arena,
    lxp_genesis_manifest *signed_manifest,
    lxp_snapshot_manifest_record *snapshot_manifest,
    lxp_byte_span *encoded_manifest, lxp_byte_span *snapshot);

lxp_result lxp_genesis_registration_request_encode(
    const lxp_genesis_manifest *manifest,
    uint8_t encoded[LXP_GENESIS_REGISTRATION_REQUEST_BYTES]);

lxp_result lxp_genesis_build_fresh_custody(
    const lxp_genesis_manifest *draft, const uint8_t asset_id[32],
    const lx_programs_metering_schedule *metering,
    const lx_programs_fee_genesis_parameters *fees,
    const lxp_bridge_profile *profile,
    const uint8_t signer_private_key[32], lxp_arena *arena,
    lxp_genesis_manifest *signed_manifest,
    lxp_snapshot_manifest_record *snapshot_manifest,
    lxp_byte_span *encoded_manifest, lxp_byte_span *snapshot);
lxp_result lxp_genesis_deployment_descriptor_encode(
    const lxp_genesis_manifest *manifest, lxp_arena *arena,
    uint8_t encoded[LXP_GENESIS_DEPLOYMENT_DESCRIPTOR_BYTES]);
lxp_result lxp_genesis_build_artifacts(
    const char *request_path, const char *signer_key_path,
    const char *output_directory);
lxp_result lxp_genesis_sign_preimage(
    const uint8_t private_key[32], const uint8_t *bytes, size_t length,
    uint8_t signature[64]);
lxp_result lxp_genesis_issuance_migration_authorize(
    const uint8_t private_key[32], uint64_t sequence,
    const uint8_t old_digest[32], const uint8_t old_canonical[32],
    const uint8_t old_receipt[32], const uint8_t new_digest[32],
    const uint8_t new_canonical[32], const uint8_t new_receipt[32],
    uint8_t encoded[LXP_ISSUANCE_MIGRATION_BYTES]);
lxp_result lxp_genesis_issuance_migration_verify(
    const uint8_t encoded[LXP_ISSUANCE_MIGRATION_BYTES], uint64_t sequence,
    const uint8_t old_digest[32], const uint8_t old_canonical[32],
    const uint8_t old_receipt[32], const uint8_t new_digest[32],
    const uint8_t new_canonical[32], const uint8_t new_receipt[32]);
int lxp_genesis_builder_cli_main(int argc, char **argv);

#endif
