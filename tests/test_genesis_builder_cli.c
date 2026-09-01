#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_genesis_builder.h"

#include "layerx/lxp_crypto.h"

#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef struct request_writer {
    uint8_t bytes[512];
    size_t length;
} request_writer;

static int append(request_writer *writer, const void *bytes, size_t length)
{
    if (writer->length > sizeof(writer->bytes) ||
        length > sizeof(writer->bytes) - writer->length)
        return 1;
    (void)memcpy(writer->bytes + writer->length, bytes, length);
    writer->length += length;
    return 0;
}

static int append_u8(request_writer *writer, uint8_t value)
{
    return append(writer, &value, 1U);
}

static int append_u16(request_writer *writer, uint16_t value)
{
    uint8_t bytes[2] = {(uint8_t)(value >> 8U), (uint8_t)value};
    return append(writer, bytes, sizeof(bytes));
}

static int append_u32(request_writer *writer, uint32_t value)
{
    uint8_t bytes[4] = {
        (uint8_t)(value >> 24U), (uint8_t)(value >> 16U),
        (uint8_t)(value >> 8U), (uint8_t)value
    };
    return append(writer, bytes, sizeof(bytes));
}

static int append_u64(request_writer *writer, uint64_t value)
{
    uint8_t bytes[8];
    size_t index;
    for (index = 0U; index < sizeof(bytes); ++index)
        bytes[index] = (uint8_t)(value >> (56U - index * 8U));
    return append(writer, bytes, sizeof(bytes));
}

static int build_request(request_writer *writer, const uint8_t asset_id[32])
{
    static const uint8_t parameter_key[32] = {
        'p','a','r','a','m','e','t','e','r','-','v','e','r','s','i','o','n'
    };
    uint8_t parameter_value[32] = {0U};
    uint8_t guarantor_id[32] = {1U};
    uint8_t guarantor_key[33] = {2U};
    uint8_t zero_bond[16] = {0U};
    static const uint64_t metering[9] = {1U,1U,1U,1U,1U,8U,8U,64U,8U};
    static const uint64_t fee_prices[7] = {1U,1U,2U,4U,1U,1U,100U};
    static const uint64_t fee_demand[6] = {100U,1U,1U,10U,1U,1000U};
    size_t index;
    parameter_value[31] = 1U;
    (void)memset(writer, 0, sizeof(*writer));
    if (append(writer, "LXGB", 4U) != 0 || append_u8(writer, 1U) != 0 ||
        append_u16(writer, LXP_PROTOCOL_VERSION) != 0 ||
        append_u32(writer, 42U) != 0 ||
        append_u64(writer, UINT64_C(1700000000000)) != 0 ||
        append_u16(writer, 1U) != 0 ||
        append_u16(writer, LXP_MODULE_GOVERNANCE) != 0 ||
        append(writer, parameter_key, sizeof(parameter_key)) != 0 ||
        append(writer, parameter_value, sizeof(parameter_value)) != 0 ||
        append_u16(writer, 1U) != 0 ||
        append(writer, guarantor_id, sizeof(guarantor_id)) != 0 ||
        append(writer, guarantor_key, sizeof(guarantor_key)) != 0 ||
        append(writer, zero_bond, sizeof(zero_bond)) != 0 ||
        append(writer, asset_id, 32U) != 0 || append_u32(writer, 1U) != 0)
        return 1;
    for (index = 0U; index < 9U; ++index)
        if (append_u64(writer, metering[index]) != 0) return 1;
    if (append_u64(writer, 1U) != 0 ||
        append_u8(writer, LX_PROGRAMS_METERING_AUTHORITY_GENESIS) != 0 ||
        append_u32(writer, 1U) != 0)
        return 1;
    for (index = 0U; index < 7U; ++index)
        if (append_u64(writer, fee_prices[index]) != 0) return 1;
    for (index = 0U; index < 6U; ++index)
        if (append_u64(writer, fee_demand[index]) != 0) return 1;
    return 0;
}

static int write_file(const char *path, const uint8_t *bytes, size_t length,
                      mode_t mode)
{
    int descriptor = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC,
                          mode);
    int failed = descriptor < 0;
    if (!failed && write(descriptor, bytes, length) != (ssize_t)length)
        failed = 1;
    if (!failed && fsync(descriptor) != 0) failed = 1;
    if (descriptor >= 0 && close(descriptor) != 0) failed = 1;
    return failed;
}

static int read_file(const char *path, uint8_t *bytes, size_t capacity,
                     size_t *length)
{
    int descriptor = open(path, O_RDONLY | O_CLOEXEC);
    ssize_t count;
    if (descriptor < 0) return 1;
    count = read(descriptor, bytes, capacity);
    if (count < 0 || close(descriptor) != 0) return 1;
    *length = (size_t)count;
    return 0;
}

int main(void)
{
    static uint8_t arena_bytes[4 * 1024 * 1024];
    static lxp_genesis_manifest manifest;
    request_writer request;
    uint8_t private_key[32] = {7U};
    uint8_t asset_id[32] = {9U};
    uint8_t manifest_bytes[LXP_GENESIS_MAX_ENCODED_BYTES];
    uint8_t registration_request[LXP_GENESIS_REGISTRATION_REQUEST_BYTES];
    uint8_t deployment_descriptor[LXP_GENESIS_DEPLOYMENT_DESCRIPTOR_BYTES];
    uint8_t manifest_commitment[32];
    size_t manifest_length;
    size_t request_length;
    lxp_snapshot_manifest_record snapshot_manifest;
    lxp_genesis_bootstrap_registration forged_registration;
    lxp_byte_span snapshot;
    lxp_arena arena;
    char base[] = "/tmp/lxp-genesis-builder-XXXXXX";
    char request_path[160];
    char key_path[160];
    char key_link_path[160];
    char manifest_path[192];
    char snapshot_path[192];
    char registration_request_path[192];
    char deployment_descriptor_path[192];
    char output_path[160];
    char rejected_output_path[160];
    if (mkdtemp(base) == NULL || build_request(&request, asset_id) != 0 ||
        snprintf(request_path, sizeof(request_path), "%s/request.lxgb", base) < 0 ||
        snprintf(key_path, sizeof(key_path), "%s/signer.key", base) < 0 ||
        snprintf(key_link_path, sizeof(key_link_path), "%s/signer-link", base) < 0 ||
        snprintf(output_path, sizeof(output_path), "%s/artifacts", base) < 0 ||
        snprintf(rejected_output_path, sizeof(rejected_output_path),
                 "%s/rejected", base) < 0 ||
        write_file(request_path, request.bytes, request.length, 0600) != 0 ||
        write_file(key_path, private_key, sizeof(private_key), 0600) != 0 ||
        symlink(key_path, key_link_path) != 0 ||
        lxp_genesis_build_artifacts(request_path, key_link_path,
                                    rejected_output_path) == LXP_OK ||
        access(rejected_output_path, F_OK) == 0 ||
        lxp_genesis_build_artifacts(request_path, key_path,
                                    output_path) != LXP_OK ||
        lxp_genesis_build_artifacts(request_path, key_path,
                                    output_path) == LXP_OK)
        return 1;
    if (snprintf(manifest_path, sizeof(manifest_path), "%s/genesis.manifest",
                 output_path) < 0 ||
        snprintf(snapshot_path, sizeof(snapshot_path),
                 "%s/00000000000000000000.lxs", output_path) < 0 ||
        snprintf(registration_request_path,
                 sizeof(registration_request_path),
                 "%s/paxeer-registration-request.lxrr", output_path) < 0 ||
        snprintf(deployment_descriptor_path,
                 sizeof(deployment_descriptor_path),
                 "%s/paxeer-deployment-descriptor.lxgd", output_path) < 0 ||
        read_file(manifest_path, manifest_bytes, sizeof(manifest_bytes),
                  &manifest_length) != 0 ||
        lxp_genesis_parse(manifest_bytes, manifest_length,
                          LXP_GENESIS_INPUT_MANIFEST, &manifest) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_genesis_verify_signature(&manifest, &arena) != LXP_OK ||
        lxp_genesis_manifest_commitment(
            &manifest, &arena, manifest_commitment) != LXP_OK ||
        lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_snapshot_store_read(snapshot_path, &arena, &snapshot_manifest,
                                &snapshot) != LXP_OK ||
        snapshot_manifest.global_sequence != 0U ||
        memcmp(snapshot_manifest.canonical_state_root,
               manifest.genesis_state_root, 32U) != 0 ||
        memcmp(snapshot_manifest.receipt_state_root,
               manifest.genesis_receipt_state_root, 32U) != 0 ||
        read_file(registration_request_path, registration_request,
                  sizeof(registration_request), &request_length) != 0 ||
        request_length != sizeof(registration_request) ||
        memcmp(registration_request, "LXRR", 4U) != 0 ||
        read_file(deployment_descriptor_path, deployment_descriptor,
                  sizeof(deployment_descriptor), &request_length) != 0 ||
        request_length != sizeof(deployment_descriptor) ||
        memcmp(deployment_descriptor, "LXGD\001", 5U) != 0 ||
        memcmp(deployment_descriptor + 5U, registration_request + 5U,
               4U) != 0 ||
        memcmp(deployment_descriptor + 9U, manifest_commitment, 32U) != 0 ||
        memcmp(deployment_descriptor + 41U, registration_request + 9U,
               32U) != 0 ||
        memcmp(deployment_descriptor + 73U, registration_request + 41U,
               32U) != 0 ||
        lxp_genesis_registration_parse(
            registration_request, request_length, &forged_registration) ==
                LXP_OK)
        return 1;
    if (unlink(deployment_descriptor_path) != 0 ||
        unlink(registration_request_path) != 0 || unlink(snapshot_path) != 0 ||
        unlink(manifest_path) != 0 || rmdir(output_path) != 0 ||
        unlink(key_link_path) != 0 || unlink(key_path) != 0 ||
        unlink(request_path) != 0 || rmdir(base) != 0)
        return 1;
    lxp_secure_zero(private_key, sizeof(private_key));
    return 0;
}
