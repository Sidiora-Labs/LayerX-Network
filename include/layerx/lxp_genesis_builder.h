#ifndef LAYERX_LXP_GENESIS_BUILDER_H
#define LAYERX_LXP_GENESIS_BUILDER_H

#include "layerx/lxp_genesis.h"
#include "layerx/lxp_snapshot.h"
#include "layerx/programs.h"
#include "layerx/lxp_bridge_credit.h"

enum {
    LXP_GENESIS_REGISTRATION_REQUEST_BYTES = 73,
    LXP_GENESIS_DEPLOYMENT_DESCRIPTOR_BYTES = 105
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
lxp_result lxp_genesis_build_snapshot_migration(
    const lxp_genesis_manifest *genesis,
    const lxp_snapshot_manifest_record *source_manifest,
    const uint8_t *source_snapshot, size_t source_snapshot_length,
    const uint8_t signer_private_key[32], lxp_arena *arena,
    lxp_snapshot_manifest_record *target_manifest,
    lxp_byte_span *target_snapshot);
int lxp_genesis_builder_cli_main(int argc, char **argv);

#endif
