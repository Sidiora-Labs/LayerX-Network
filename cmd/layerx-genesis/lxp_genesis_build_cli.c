#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_genesis_builder.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lx_asset.h"
#include "layerx/lxp_fee.h"
#include "layerx/lxp_protocol.h"

#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

enum {
    GENESIS_BUILD_REQUEST_MAX_BYTES = 16384,
    GENESIS_BUILD_ARENA_BYTES = 4 * 1024 * 1024
};

typedef struct build_reader {
    const uint8_t *bytes;
    size_t length;
    size_t offset;
} build_reader;

static lxp_result reader_take(build_reader *reader, size_t length,
                              const uint8_t **bytes)
{
    if (reader == NULL || bytes == NULL || reader->offset > reader->length ||
        length > reader->length - reader->offset)
        return LXP_ERR_TRUNCATED;
    *bytes = reader->bytes + reader->offset;
    reader->offset += length;
    return LXP_OK;
}

static lxp_result reader_copy(build_reader *reader, uint8_t *output,
                              size_t length)
{
    const uint8_t *bytes;
    lxp_result status = reader_take(reader, length, &bytes);
    if (status == LXP_OK) (void)memcpy(output, bytes, length);
    return status;
}

static lxp_result reader_u8(build_reader *reader, uint8_t *value)
{
    const uint8_t *bytes;
    lxp_result status = reader_take(reader, 1U, &bytes);
    if (status == LXP_OK) *value = bytes[0];
    return status;
}

static lxp_result reader_u16(build_reader *reader, uint16_t *value)
{
    const uint8_t *bytes;
    lxp_result status = reader_take(reader, 2U, &bytes);
    if (status == LXP_OK)
        *value = (uint16_t)(((uint16_t)bytes[0] << 8U) | bytes[1]);
    return status;
}

static lxp_result reader_u32(build_reader *reader, uint32_t *value)
{
    const uint8_t *bytes;
    lxp_result status = reader_take(reader, 4U, &bytes);
    if (status == LXP_OK)
        *value = ((uint32_t)bytes[0] << 24U) |
                 ((uint32_t)bytes[1] << 16U) |
                 ((uint32_t)bytes[2] << 8U) | bytes[3];
    return status;
}

static lxp_result reader_u64(build_reader *reader, uint64_t *value)
{
    const uint8_t *bytes;
    size_t index;
    lxp_result status = reader_take(reader, 8U, &bytes);
    if (status != LXP_OK) return status;
    *value = 0U;
    for (index = 0U; index < 8U; ++index)
        *value = (*value << 8U) | bytes[index];
    return LXP_OK;
}

static lxp_result reader_u128(build_reader *reader, lxp_u128 *value)
{
    const uint8_t *bytes;
    lxp_result status = reader_take(reader, 16U, &bytes);
    return status == LXP_OK ? lxp_u128_from_be(bytes, value) : status;
}

static lxp_result parse_request(
    const uint8_t *bytes, size_t length, lxp_genesis_manifest *draft,
    uint8_t asset_id[32], lx_programs_metering_schedule *metering,
    lx_programs_fee_genesis_parameters *fees)
{
    build_reader reader = {bytes, length, 0U};
    const uint8_t *magic;
    uint8_t version;
    uint16_t count = 0U;
    size_t index;
    lxp_result status;
    if (bytes == NULL || draft == NULL || asset_id == NULL ||
        metering == NULL || fees == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(draft, 0, sizeof(*draft));
    (void)memset(metering, 0, sizeof(*metering));
    (void)memset(fees, 0, sizeof(*fees));
    status = reader_take(&reader, 4U, &magic);
    if (status == LXP_OK && memcmp(magic, "LXGB", 4U) != 0)
        status = LXP_ERR_INVALID_TAG;
    if (status == LXP_OK) status = reader_u8(&reader, &version);
    if (status == LXP_OK && version != 1U && version != 2U)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK)
        status = reader_u16(&reader, &draft->protocol_version);
    if (status == LXP_OK && draft->protocol_version != LXP_PROTOCOL_VERSION &&
        draft->protocol_version != LXP_PROTOCOL_VERSION_STATE_COMMITMENT)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK) status = reader_u32(&reader, &draft->network_id);
    if (status == LXP_OK)
        status = reader_u64(&reader, &draft->genesis_timestamp_ms);
    if (status == LXP_OK) status = reader_u16(&reader, &count);
    if (status == LXP_OK &&
        (count == 0U || count > LXP_GENESIS_MAX_PARAMETERS))
        status = LXP_ERR_LENGTH_LIMIT;
    draft->parameter_count = count;
    for (index = 0U; status == LXP_OK && index < count; ++index) {
        status = reader_u16(&reader, &draft->parameters[index].module_id);
        if (status == LXP_OK)
            status = reader_copy(&reader, draft->parameters[index].key, 32U);
        if (status == LXP_OK)
            status = reader_copy(&reader, draft->parameters[index].value, 32U);
    }
    if (status == LXP_OK) status = reader_u16(&reader, &count);
    if (status == LXP_OK &&
        (count == 0U || count > LXP_GENESIS_MAX_GUARANTORS))
        status = LXP_ERR_LENGTH_LIMIT;
    draft->guarantor_count = count;
    for (index = 0U; status == LXP_OK && index < count; ++index) {
        status = reader_copy(&reader,
                             draft->guarantors[index].guarantor_id, 32U);
        if (status == LXP_OK)
            status = reader_copy(&reader,
                                 draft->guarantors[index].public_key, 33U);
        if (status == LXP_OK)
            status = reader_u128(&reader, &draft->guarantors[index].bond);
    }
    if (status == LXP_OK) status = reader_copy(&reader, asset_id, 32U);
    if (status == LXP_OK) status = reader_u32(&reader, &metering->version);
    for (index = 0U; status == LXP_OK &&
         index < LX_PROGRAMS_METERING_COEFFICIENTS; ++index)
        status = reader_u64(&reader, &metering->coefficients[index]);
    if (status == LXP_OK)
        status = reader_u64(&reader, &metering->activation_batch);
    if (status == LXP_OK)
        status = reader_u8(&reader, &metering->authority_kind);
    if (status == LXP_OK)
        status = reader_u32(&reader, &fees->schedule.version);
    if (status == LXP_OK) status = reader_u64(&reader, &fees->schedule.cpu);
    if (status == LXP_OK)
        status = reader_u64(&reader, &fees->schedule.memory_byte);
    if (status == LXP_OK)
        status = reader_u64(&reader, &fees->schedule.storage_read_byte);
    if (status == LXP_OK)
        status = reader_u64(&reader, &fees->schedule.storage_write_byte);
    if (status == LXP_OK)
        status = reader_u64(&reader, &fees->schedule.output_value);
    if (status == LXP_OK)
        status = reader_u64(&reader, &fees->schedule.output_byte);
    if (status == LXP_OK)
        status = reader_u64(&reader, &fees->schedule.occupancy_byte_batch);
    if (status == LXP_OK)
        status = reader_u64(&reader,
                            &fees->target_occupancy_byte_batches);
    if (status == LXP_OK)
        status = reader_u64(&reader, &fees->response_denominator);
    if (status == LXP_OK)
        status = reader_u64(&reader, &fees->maximum_change_numerator);
    if (status == LXP_OK)
        status = reader_u64(&reader, &fees->maximum_change_denominator);
    if (status == LXP_OK)
        status = reader_u64(
            &reader, &fees->minimum_fee_units_per_occupancy_byte_batch);
    if (status == LXP_OK)
        status = reader_u64(
            &reader, &fees->maximum_fee_units_per_occupancy_byte_batch);
    if (status == LXP_OK && version == 2U) {
        uint16_t records;
        status = reader_u16(&reader, &records);
        if (status == LXP_OK && (records == 0U || records > LX_ASSET_REGISTRY_CAPACITY))
            status = LXP_ERR_LENGTH_LIMIT;
        for (size_t i = 0U; status == LXP_OK && i < records; ++i) {
            uint16_t record_length;
            lx_asset_record record;
            lxp_genesis_module_value *value = &draft->module_values[draft->module_value_count];
            status = reader_u16(&reader, &record_length);
            if (status == LXP_OK && record_length > sizeof(value->value)) status = LXP_ERR_LENGTH_LIMIT;
            if (status == LXP_OK) status = reader_copy(&reader, value->value, record_length);
            if (status == LXP_OK) status = lx_asset_record_decode(value->value, record_length, &record);
            if (status == LXP_OK && (!lxp_u128_is_zero(record.total_units) || record.issuer_kind == 1U))
                status = LXP_ERR_NON_CANONICAL;
            if (status == LXP_OK) {
                value->module_id = LXP_MODULE_ASSET;
                value->value_length = record_length;
                (void)memcpy(value->key, record.asset_id, 32U);
                ++draft->module_value_count;
            }
        }
        if (status == LXP_OK) {
            uint16_t schedule_length;
            lxp_fee_params schedule;
            lxp_genesis_module_value *value = &draft->module_values[draft->module_value_count];
            status = reader_u16(&reader, &schedule_length);
            if (status == LXP_OK && schedule_length > sizeof(value->value)) status = LXP_ERR_LENGTH_LIMIT;
            if (status == LXP_OK) status = reader_copy(&reader, value->value, schedule_length);
            if (status == LXP_OK) status = lxp_fee_params_decode(value->value, schedule_length, &schedule);
            if (status == LXP_OK && schedule.version != 2U && schedule.version != 3U)
                status = LXP_ERR_VERSION_UNSUPPORTED;
            if (status == LXP_OK) {
                value->module_id = LXP_MODULE_GOVERNANCE;
                value->value_length = schedule_length;
                (void)memcpy(value->key, "fee.schedule", 12U);
                ++draft->module_value_count;
            }
        }
    }
    if (status == LXP_OK && reader.offset != reader.length)
        status = LXP_ERR_TRAILING_BYTES;
    if (status == LXP_OK)
        (void)memcpy(fees->occupancy_asset_id, asset_id, 32U);
    return status;
}

static lxp_result read_regular_file(const char *path, size_t maximum,
                                    bool private_file, uint8_t **bytes,
                                    size_t *length)
{
    struct stat information;
    uint8_t *memory;
    size_t offset = 0U;
    int descriptor;
    lxp_result status = LXP_OK;
    if (path == NULL || bytes == NULL || length == NULL || maximum == 0U)
        return LXP_ERR_NON_CANONICAL;
    descriptor = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (descriptor < 0 || fstat(descriptor, &information) != 0 ||
        !S_ISREG(information.st_mode) || information.st_nlink != 1 ||
        information.st_size <= 0 || (uint64_t)information.st_size > maximum ||
        (private_file && (information.st_mode & (S_IRWXG | S_IRWXO)) != 0)) {
        if (descriptor >= 0) (void)close(descriptor);
        return LXP_ERR_IO;
    }
    memory = (uint8_t *)malloc((size_t)information.st_size);
    if (memory == NULL) {
        (void)close(descriptor);
        return LXP_ERR_IO;
    }
    while (offset < (size_t)information.st_size) {
        ssize_t count = read(descriptor, memory + offset,
                             (size_t)information.st_size - offset);
        if (count <= 0) {
            status = LXP_ERR_IO;
            break;
        }
        offset += (size_t)count;
    }
    if (close(descriptor) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    if (status != LXP_OK) {
        lxp_secure_zero(memory, (size_t)information.st_size);
        free(memory);
        return status;
    }
    *bytes = memory;
    *length = (size_t)information.st_size;
    return LXP_OK;
}

static lxp_result write_exclusive(const char *path, const uint8_t *bytes,
                                  size_t length)
{
    size_t offset = 0U;
    int descriptor;
    lxp_result status = LXP_OK;
    descriptor = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC |
                            O_NOFOLLOW, 0600);
    if (descriptor < 0) return LXP_ERR_IO;
    while (offset < length) {
        ssize_t count = write(descriptor, bytes + offset, length - offset);
        if (count <= 0) {
            status = LXP_ERR_IO;
            break;
        }
        offset += (size_t)count;
    }
    if (status == LXP_OK && fsync(descriptor) != 0) status = LXP_ERR_IO;
    if (close(descriptor) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    return status;
}

static lxp_result join_path(char *output, size_t capacity,
                            const char *directory, const char *name)
{
    int length;
    if (output == NULL || capacity == 0U || directory == NULL ||
        name == NULL)
        return LXP_ERR_NON_CANONICAL;
    length = snprintf(output, capacity, "%s/%s", directory, name);
    return length >= 0 && (size_t)length < capacity ?
        LXP_OK : LXP_ERR_LENGTH_LIMIT;
}

static void put_u32(uint8_t output[4], uint32_t value)
{
    output[0] = (uint8_t)(value >> 24U);
    output[1] = (uint8_t)(value >> 16U);
    output[2] = (uint8_t)(value >> 8U);
    output[3] = (uint8_t)value;
}

lxp_result lxp_genesis_registration_request_encode(
    const lxp_genesis_manifest *manifest,
    uint8_t encoded[LXP_GENESIS_REGISTRATION_REQUEST_BYTES])
{
    if (manifest == NULL || encoded == NULL || manifest->network_id == 0U ||
        lxp_ct_is_zero(manifest->genesis_state_root, 32U) ||
        lxp_ct_is_zero(manifest->genesis_receipt_state_root, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(encoded, "LXRR", 4U);
    encoded[4] = 1U;
    put_u32(encoded + 5U, manifest->network_id);
    (void)memcpy(encoded + 9U, manifest->genesis_state_root, 32U);
    (void)memcpy(encoded + 41U,
                 manifest->genesis_receipt_state_root, 32U);
    return LXP_OK;
}

lxp_result lxp_genesis_deployment_descriptor_encode(
    const lxp_genesis_manifest *manifest, lxp_arena *arena,
    uint8_t encoded[LXP_GENESIS_DEPLOYMENT_DESCRIPTOR_BYTES])
{
    lxp_result status;
    if (manifest == NULL || arena == NULL || encoded == NULL ||
        manifest->network_id == 0U ||
        lxp_ct_is_zero(manifest->genesis_state_root, 32U) ||
        lxp_ct_is_zero(manifest->genesis_receipt_state_root, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(encoded, "LXGD", 4U);
    encoded[4] = 1U;
    put_u32(encoded + 5U, manifest->network_id);
    status = lxp_genesis_manifest_commitment(manifest, arena, encoded + 9U);
    if (status != LXP_OK) return status;
    (void)memcpy(encoded + 41U, manifest->genesis_state_root, 32U);
    (void)memcpy(encoded + 73U,
                 manifest->genesis_receipt_state_root, 32U);
    return LXP_OK;
}

static lxp_result build_artifacts(
    const char *request_path, const char *signer_key_path,
    const char *output_directory, const char *profile_path)
{
    static const char manifest_name[] = "genesis.manifest";
    static const char snapshot_name[] = "00000000000000000000.lxs";
    static const char snapshot_temporary_name[] =
        "00000000000000000000.lxs.tmp";
    static const char request_name[] = "paxeer-registration-request.lxrr";
    static const char descriptor_name[] =
        "paxeer-deployment-descriptor.lxgd";
    uint8_t registration_request[LXP_GENESIS_REGISTRATION_REQUEST_BYTES];
    uint8_t deployment_descriptor[LXP_GENESIS_DEPLOYMENT_DESCRIPTOR_BYTES];
    uint8_t asset_id[32];
    uint8_t *request_bytes = NULL;
    uint8_t *profile_bytes = NULL;
    size_t profile_length = 0U;
    lxp_bridge_profile profile;
    uint8_t *signer_key = NULL;
    uint8_t *arena_bytes = NULL;
    size_t request_length = 0U;
    size_t signer_key_length = 0U;
    lxp_genesis_manifest *draft = NULL;
    lxp_genesis_manifest *manifest = NULL;
    lx_programs_metering_schedule metering;
    lx_programs_fee_genesis_parameters fees;
    lxp_snapshot_manifest_record snapshot_manifest;
    lxp_byte_span encoded_manifest;
    lxp_byte_span snapshot;
    lxp_arena arena;
    char manifest_path[4096];
    char snapshot_path[4096];
    char snapshot_temporary_path[4096];
    char registration_request_path[4096];
    char deployment_descriptor_path[4096];
    int directory_descriptor = -1;
    bool directory_created = false;
    lxp_result status;
    if (request_path == NULL || signer_key_path == NULL ||
        output_directory == NULL || output_directory[0] == '\0')
        return LXP_ERR_NON_CANONICAL;
    status = read_regular_file(request_path, GENESIS_BUILD_REQUEST_MAX_BYTES,
                               false, &request_bytes, &request_length);
    if (status == LXP_OK && profile_path != NULL)
        status = read_regular_file(profile_path, LXP_BRIDGE_PROFILE_BYTES, false,
                                    &profile_bytes, &profile_length);
    if (status == LXP_OK && profile_path != NULL) {
        if (profile_length != sizeof(profile.bytes)) status = LXP_ERR_NON_CANONICAL;
        else {
            (void)memcpy(profile.bytes, profile_bytes, sizeof(profile.bytes));
            status = lxp_bridge_profile_validate(&profile);
        }
    }
    if (status == LXP_OK)
        status = read_regular_file(signer_key_path, 32U, true,
                                   &signer_key, &signer_key_length);
    if (status == LXP_OK && signer_key_length != 32U)
        status = LXP_ERR_NON_CANONICAL;
    draft = (lxp_genesis_manifest *)calloc(1U, sizeof(*draft));
    manifest = (lxp_genesis_manifest *)calloc(1U, sizeof(*manifest));
    arena_bytes = (uint8_t *)malloc(GENESIS_BUILD_ARENA_BYTES);
    if (status == LXP_OK &&
        (draft == NULL || manifest == NULL || arena_bytes == NULL))
        status = LXP_ERR_IO;
    if (status == LXP_OK)
        status = parse_request(request_bytes, request_length, draft,
                               asset_id, &metering, &fees);
    if (status == LXP_OK)
        status = lxp_arena_init(&arena, arena_bytes,
                                GENESIS_BUILD_ARENA_BYTES);
    if (status == LXP_OK && profile_path == NULL)
        status = lxp_genesis_build_fresh_empty(
            draft, asset_id, &metering, &fees, signer_key, &arena, manifest,
            &snapshot_manifest, &encoded_manifest, &snapshot);
    if (status == LXP_OK && profile_path != NULL)
        status = lxp_genesis_build_fresh_custody(
            draft, asset_id, &metering, &fees, &profile, signer_key, &arena, manifest,
            &snapshot_manifest, &encoded_manifest, &snapshot);
    if (status == LXP_OK)
        status = lxp_genesis_registration_request_encode(
            manifest, registration_request);
    if (status == LXP_OK)
        status = lxp_genesis_deployment_descriptor_encode(
            manifest, &arena, deployment_descriptor);
    if (status == LXP_OK)
        status = join_path(manifest_path, sizeof(manifest_path),
                           output_directory, manifest_name);
    if (status == LXP_OK)
        status = join_path(snapshot_path, sizeof(snapshot_path),
                           output_directory, snapshot_name);
    if (status == LXP_OK)
        status = join_path(snapshot_temporary_path,
                           sizeof(snapshot_temporary_path), output_directory,
                           snapshot_temporary_name);
    if (status == LXP_OK)
        status = join_path(registration_request_path,
                           sizeof(registration_request_path),
                           output_directory, request_name);
    if (status == LXP_OK)
        status = join_path(deployment_descriptor_path,
                           sizeof(deployment_descriptor_path),
                           output_directory, descriptor_name);
    if (status == LXP_OK && mkdir(output_directory, 0700) != 0)
        status = LXP_ERR_IO;
    else if (status == LXP_OK)
        directory_created = true;
    if (status == LXP_OK)
        status = write_exclusive(manifest_path, encoded_manifest.bytes,
                                 encoded_manifest.length);
    if (status == LXP_OK)
        status = lxp_snapshot_store_write(output_directory,
                                          &snapshot_manifest,
                                          snapshot.bytes, snapshot.length);
    if (status == LXP_OK)
        status = write_exclusive(registration_request_path,
                                 registration_request,
                                 sizeof(registration_request));
    if (status == LXP_OK)
        status = write_exclusive(deployment_descriptor_path,
                                 deployment_descriptor,
                                 sizeof(deployment_descriptor));
    if (status == LXP_OK) {
        directory_descriptor = open(output_directory,
                                    O_RDONLY | O_DIRECTORY | O_CLOEXEC |
                                        O_NOFOLLOW);
        if (directory_descriptor < 0 || fsync(directory_descriptor) != 0)
            status = LXP_ERR_IO;
    }
    if (directory_descriptor >= 0 && close(directory_descriptor) != 0 &&
        status == LXP_OK)
        status = LXP_ERR_IO;
    if (status != LXP_OK && directory_created) {
        (void)unlink(deployment_descriptor_path);
        (void)unlink(registration_request_path);
        (void)unlink(snapshot_temporary_path);
        (void)unlink(snapshot_path);
        (void)unlink(manifest_path);
        (void)rmdir(output_directory);
    }
    if (signer_key != NULL) {
        lxp_secure_zero(signer_key, signer_key_length);
        free(signer_key);
    }
    if (request_bytes != NULL) {
        lxp_secure_zero(request_bytes, request_length);
        free(request_bytes);
    }
    if (draft != NULL) lxp_secure_zero(draft, sizeof(*draft));
    if (manifest != NULL) lxp_secure_zero(manifest, sizeof(*manifest));
    if (arena_bytes != NULL)
        lxp_secure_zero(arena_bytes, GENESIS_BUILD_ARENA_BYTES);
    lxp_secure_zero(&metering, sizeof(metering));
    lxp_secure_zero(&fees, sizeof(fees));
    lxp_secure_zero(registration_request, sizeof(registration_request));
    lxp_secure_zero(deployment_descriptor, sizeof(deployment_descriptor));
    free(draft);
    free(manifest);
    free(arena_bytes);
    free(profile_bytes);
    return status;
}

lxp_result lxp_genesis_build_artifacts(
    const char *request_path, const char *signer_key_path,
    const char *output_directory)
{
    return build_artifacts(request_path, signer_key_path, output_directory, NULL);
}

static lxp_result migrate_asset_v2(const char *input_path, const char *salt_path,
    const char *salt_source, const char *directory)
{
    uint8_t *input = NULL;
    uint8_t *salt = NULL;
    uint8_t output[384];
    size_t input_length = 0U, salt_length = 0U, output_length = 0U;
    char output_path[4096], source_path[4096];
    bool created = false;
    lxp_result status;
    if (salt_source == NULL || strlen(salt_source) == 0U || strlen(salt_source) > 4096U)
        return LXP_ERR_NON_CANONICAL;
    status = read_regular_file(input_path, sizeof(output), false, &input, &input_length);
    if (status == LXP_OK) status = read_regular_file(salt_path, 32U, false, &salt, &salt_length);
    if (status == LXP_OK && salt_length != 32U) status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) status = lx_asset_record_migrate_v2(input, input_length, salt,
        output, sizeof(output), &output_length);
    if (status == LXP_OK) status = join_path(output_path, sizeof(output_path), directory, "asset-v3.bin");
    if (status == LXP_OK) status = join_path(source_path, sizeof(source_path), directory, "salt-source.txt");
    if (status == LXP_OK) {
        if (mkdir(directory, 0700) != 0) status = LXP_ERR_IO;
        else created = true;
    }
    if (status == LXP_OK) status = write_exclusive(source_path,
        (const uint8_t *)salt_source, strlen(salt_source));
    if (status == LXP_OK) status = write_exclusive(output_path, output, output_length);
    if (status == LXP_OK) {
        int descriptor = open(directory, O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
        if (descriptor < 0) status = LXP_ERR_IO;
        else {
            if (fsync(descriptor) != 0) status = LXP_ERR_IO;
            if (close(descriptor) != 0) status = LXP_ERR_IO;
        }
    }
    if (status != LXP_OK && created) {
        (void)unlink(output_path);
        (void)unlink(source_path);
        (void)rmdir(directory);
    }
    free(input);
    free(salt);
    return status;
}

static lxp_result migrate_snapshot_issuance(
    const char *snapshot_path, const char *manifest_path,
    const char *signer_key_path, const char *directory)
{
    uint8_t *manifest_bytes = NULL;
    uint8_t *signer_key = NULL;
    uint8_t *source_arena_bytes = NULL;
    uint8_t *target_arena_bytes = NULL;
    size_t manifest_length = 0U;
    size_t signer_key_length = 0U;
    lxp_genesis_manifest *genesis = NULL;
    lxp_snapshot_manifest_record source_manifest;
    lxp_snapshot_manifest_record target_manifest;
    lxp_byte_span source_snapshot;
    lxp_byte_span target_snapshot;
    lxp_arena source_arena;
    lxp_arena target_arena;
    char output_path[4096];
    char temporary_path[4096];
    bool created = false;
    lxp_result status;
    if (snapshot_path == NULL || manifest_path == NULL ||
        signer_key_path == NULL || directory == NULL ||
        directory[0] == '\0')
        return LXP_ERR_NON_CANONICAL;
    status = read_regular_file(
        manifest_path, LXP_GENESIS_MAX_ENCODED_BYTES, false,
        &manifest_bytes, &manifest_length);
    if (status == LXP_OK)
        status = read_regular_file(
            signer_key_path, 32U, true, &signer_key, &signer_key_length);
    if (status == LXP_OK && signer_key_length != 32U)
        status = LXP_ERR_NON_CANONICAL;
    genesis = (lxp_genesis_manifest *)malloc(sizeof(*genesis));
    source_arena_bytes = (uint8_t *)malloc(GENESIS_BUILD_ARENA_BYTES);
    target_arena_bytes = (uint8_t *)malloc(GENESIS_BUILD_ARENA_BYTES);
    if (status == LXP_OK &&
        (genesis == NULL || source_arena_bytes == NULL ||
         target_arena_bytes == NULL))
        status = LXP_ERR_IO;
    if (status == LXP_OK)
        status = lxp_genesis_parse(
            manifest_bytes, manifest_length, LXP_GENESIS_INPUT_MANIFEST,
            genesis);
    if (status == LXP_OK)
        status = lxp_arena_init(
            &source_arena, source_arena_bytes, GENESIS_BUILD_ARENA_BYTES);
    if (status == LXP_OK)
        status = lxp_arena_init(
            &target_arena, target_arena_bytes, GENESIS_BUILD_ARENA_BYTES);
    if (status == LXP_OK)
        status = lxp_snapshot_store_read(
            snapshot_path, &source_arena, &source_manifest,
            &source_snapshot);
    if (status == LXP_OK)
        status = lxp_genesis_build_snapshot_migration(
            genesis, &source_manifest, source_snapshot.bytes,
            source_snapshot.length, signer_key, &target_arena,
            &target_manifest, &target_snapshot);
    if (status == LXP_OK) {
        int length = snprintf(
            output_path, sizeof(output_path), "%s/%020llu.lxs", directory,
            (unsigned long long)target_manifest.global_sequence);
        if (length < 0 || (size_t)length >= sizeof(output_path))
            status = LXP_ERR_LENGTH_LIMIT;
    }
    if (status == LXP_OK) {
        int length = snprintf(
            temporary_path, sizeof(temporary_path), "%s.tmp", output_path);
        if (length < 0 || (size_t)length >= sizeof(temporary_path))
            status = LXP_ERR_LENGTH_LIMIT;
    }
    if (status == LXP_OK) {
        if (mkdir(directory, 0700) != 0) status = LXP_ERR_IO;
        else created = true;
    }
    if (status == LXP_OK)
        status = lxp_snapshot_store_write(
            directory, &target_manifest, target_snapshot.bytes,
            target_snapshot.length);
    if (status != LXP_OK && created) {
        (void)unlink(temporary_path);
        (void)unlink(output_path);
        (void)rmdir(directory);
    }
    if (signer_key != NULL) {
        lxp_secure_zero(signer_key, signer_key_length);
        free(signer_key);
    }
    if (manifest_bytes != NULL) {
        lxp_secure_zero(manifest_bytes, manifest_length);
        free(manifest_bytes);
    }
    if (genesis != NULL) lxp_secure_zero(genesis, sizeof(*genesis));
    if (source_arena_bytes != NULL)
        lxp_secure_zero(source_arena_bytes, GENESIS_BUILD_ARENA_BYTES);
    if (target_arena_bytes != NULL)
        lxp_secure_zero(target_arena_bytes, GENESIS_BUILD_ARENA_BYTES);
    free(genesis);
    free(source_arena_bytes);
    free(target_arena_bytes);
    return status;
}

int lxp_genesis_builder_cli_main(int argc, char **argv)
{
    if (argv != NULL && argc == 6 && strcmp(argv[1], "--migrate-asset-v2") == 0)
        return migrate_asset_v2(argv[2], argv[3], argv[4], argv[5]) == LXP_OK ? 0 : 1;
    if (argv != NULL && argc == 6 &&
        strcmp(argv[1], "--migrate-snapshot-issuance") == 0)
        return migrate_snapshot_issuance(
            argv[2], argv[3], argv[4], argv[5]) == LXP_OK ? 0 : 1;
    if (argv == NULL || (argc != 4 && argc != 6) ||
        (argc == 6 && strcmp(argv[4], "--custody-profile") != 0))
        return 2;
    return build_artifacts(argv[1], argv[2], argv[3], argc == 6 ? argv[5] : NULL) == LXP_OK ?
        0 : 1;
}
