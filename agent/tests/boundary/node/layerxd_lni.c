#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_activity.h"
#include "layerx/lxp_batch.h"
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_receipt.h"

#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <openssl/evp.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

enum {
    MAX_FRAME = 1048576,
    ARENA_BYTES = 8 * 1024 * 1024,
    ACTIVITY_BYTES = 4096,
    RECEIPT_BYTES = 4096,
    CHECKPOINT_BYTES = 8192
};

typedef enum node_mode {
    NODE_NORMAL,
    NODE_BEHIND,
    NODE_DEGRADED
} node_mode;

typedef struct node_fixture {
    uint8_t activity[ACTIVITY_BYTES];
    size_t activity_length;
    uint8_t activity_id[32];
    uint8_t activity_root[32];
    uint8_t receipt[RECEIPT_BYTES];
    size_t receipt_length;
    uint8_t receipt_root[32];
    uint8_t header[LXP_BATCH_HEADER_ENCODED_SIZE];
    size_t header_length;
    uint8_t header_hash[32];
    uint8_t checkpoint[CHECKPOINT_BYTES];
    size_t checkpoint_length;
    uint8_t checkpoint_id[32];
    uint8_t state_leaf[3];
    uint8_t state_root[32];
    uint8_t event[5];
    uint8_t event_root[32];
    uint8_t sequencer_key[32];
} node_fixture;

typedef struct daemon_apply_state {
    uint64_t expected_sequence;
} daemon_apply_state;

typedef struct preparation_fixture {
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_identity_store identities;
    lxp_daemon_receipt_authority_store authority;
    lxp_daemon_protocol_owner owner;
    bool initialized;
} preparation_fixture;

static volatile sig_atomic_t running = 1;

static void stop_running(int signal_number)
{
    (void)signal_number;
    running = 0;
}

static void store_u16(uint8_t *output, uint16_t value)
{
    output[0] = (uint8_t)(value >> 8U);
    output[1] = (uint8_t)value;
}

static void store_u32(uint8_t *output, uint32_t value)
{
    output[0] = (uint8_t)(value >> 24U);
    output[1] = (uint8_t)(value >> 16U);
    output[2] = (uint8_t)(value >> 8U);
    output[3] = (uint8_t)value;
}

static void store_u64(uint8_t *output, uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        output[index] = (uint8_t)(value >> ((7U - index) * 8U));
}

static uint16_t load_u16(const uint8_t *input)
{
    return (uint16_t)(((uint16_t)input[0] << 8U) | input[1]);
}

static uint32_t load_u32(const uint8_t *input)
{
    return ((uint32_t)input[0] << 24U) |
        ((uint32_t)input[1] << 16U) |
        ((uint32_t)input[2] << 8U) | input[3];
}

static uint64_t load_u64(const uint8_t *input)
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index)
        value = (value << 8U) | input[index];
    return value;
}

static int write_exact(int descriptor, const uint8_t *bytes, size_t length)
{
    size_t offset = 0U;
    while (offset < length) {
        ssize_t written = write(descriptor, bytes + offset, length - offset);
        if (written > 0) offset += (size_t)written;
        else if (written < 0 && errno == EINTR) continue;
        else return 1;
    }
    return 0;
}

static int read_exact(int descriptor, uint8_t *bytes, size_t length)
{
    size_t offset = 0U;
    while (offset < length) {
        ssize_t received = read(descriptor, bytes + offset, length - offset);
        if (received > 0) offset += (size_t)received;
        else if (received < 0 && errno == EINTR) continue;
        else return 1;
    }
    return 0;
}

static int send_envelope(
    int descriptor, uint16_t message_tag, uint64_t correlation_id,
    const uint8_t *payload, size_t payload_length,
    const uint8_t *proof, size_t proof_length)
{
    uint8_t *frame;
    size_t body_length = 22U + payload_length + proof_length;
    size_t cursor = 4U;
    if (body_length > MAX_FRAME || payload_length > UINT32_MAX ||
        proof_length > UINT32_MAX) return 1;
    frame = malloc(4U + body_length);
    if (frame == NULL) return 1;
    store_u32(frame, (uint32_t)body_length);
    store_u16(frame + cursor, 1U); cursor += 2U;
    store_u16(frame + cursor, 1U); cursor += 2U;
    store_u16(frame + cursor, message_tag); cursor += 2U;
    store_u64(frame + cursor, correlation_id); cursor += 8U;
    store_u32(frame + cursor, (uint32_t)payload_length); cursor += 4U;
    if (payload_length != 0U) {
        (void)memcpy(frame + cursor, payload, payload_length);
        cursor += payload_length;
    }
    store_u32(frame + cursor, (uint32_t)proof_length); cursor += 4U;
    if (proof_length != 0U) {
        (void)memcpy(frame + cursor, proof, proof_length);
        cursor += proof_length;
    }
    if (cursor != 4U + body_length ||
        write_exact(descriptor, frame, cursor) != 0) {
        free(frame);
        return 1;
    }
    free(frame);
    return 0;
}

static int public_key_for(
    const uint8_t private_key[32], uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(
        EVP_PKEY_ED25519, NULL, private_key, 32U);
    size_t length = 32U;
    int ok = key != NULL && EVP_PKEY_get_raw_public_key(
        key, public_key, &length) == 1 && length == 32U;
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static int sign_raw(
    const uint8_t private_key[32], const uint8_t *message,
    size_t message_length, uint8_t signature[64])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(
        EVP_PKEY_ED25519, NULL, private_key, 32U);
    EVP_MD_CTX *context = EVP_MD_CTX_new();
    size_t signature_length = 64U;
    int ok = key != NULL && context != NULL &&
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(context, signature, &signature_length,
                       message, message_length) == 1 &&
        signature_length == 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static lxp_result genesis_checkpoint_id(
    uint32_t network_id, const uint8_t state_root[32], uint8_t output[32])
{
    uint8_t preimage[36];
    store_u32(preimage, network_id);
    (void)memcpy(preimage + 4U, state_root, 32U);
    return lxp_hash_domain(
        LXP_DOMAIN_CHECKPOINT_CERTIFICATE,
        preimage, sizeof(preimage), output);
}

static int write_genesis(const char *path)
{
    static const uint8_t private_key[32] = {7U};
    static uint8_t arena_storage[ARENA_BYTES];
    static lxp_genesis_manifest manifest;
    lxp_arena arena;
    lxp_byte_span preimage;
    lxp_byte_span encoded;
    lx_programs_metering_schedule metering;
    lx_programs_fee_genesis_parameters fee_genesis;
    FILE *file;
    if (lxp_arena_init(&arena, arena_storage, sizeof(arena_storage)) != LXP_OK)
        return 1;
    (void)memset(&manifest, 0, sizeof(manifest));
    manifest.protocol_version = LXP_PROTOCOL_VERSION;
    manifest.network_id = 77U;
    manifest.genesis_timestamp_ms = UINT64_C(1700000000000);
    manifest.parameter_count = 1U;
    manifest.parameters[0].module_id = 1U;
    manifest.parameters[0].key[0] = 1U;
    manifest.parameters[0].value[0] = 1U;
    manifest.guarantor_count = 1U;
    manifest.guarantors[0].guarantor_id[0] = 1U;
    manifest.guarantors[0].public_key[0] = 2U;
    manifest.guarantors[0].bond = (lxp_u128){0U, 100U};
    manifest.account_count = 1U;
    manifest.accounts[0].account_id[0] = 1U;
    manifest.accounts[0].asset_id[0] = 1U;
    manifest.accounts[0].balance = (lxp_u128){0U, 1000U};
    (void)memset(&metering, 0, sizeof(metering));
    metering.version = 1U;
    metering.coefficients[0] = 1U;
    metering.coefficients[1] = 1U;
    metering.coefficients[2] = 1U;
    metering.coefficients[3] = 1U;
    metering.coefficients[4] = 1U;
    metering.coefficients[5] = 8U;
    metering.coefficients[6] = 8U;
    metering.coefficients[7] = 64U;
    metering.coefficients[8] = 8U;
    metering.activation_batch = 1U;
    metering.authority_kind = LX_PROGRAMS_METERING_AUTHORITY_GENESIS;
    (void)memset(&fee_genesis, 0, sizeof(fee_genesis));
    fee_genesis.schedule = (lx_programs_fee_schedule){
        1U, 1U, 1U, 2U, 4U, 1U, 1U, 100U
    };
    fee_genesis.occupancy_asset_id[0] = 1U;
    fee_genesis.target_occupancy_byte_batches = 100U;
    fee_genesis.response_denominator = 1U;
    fee_genesis.maximum_change_numerator = 1U;
    fee_genesis.maximum_change_denominator = 10U;
    fee_genesis.minimum_fee_units_per_occupancy_byte_batch = 1U;
    fee_genesis.maximum_fee_units_per_occupancy_byte_batch = 1000U;
    if (public_key_for(private_key, manifest.signer_public_key) != 0 ||
        lxp_hash_payload(
            manifest.signer_public_key, 32U,
            metering.authority_digest) != LXP_OK ||
        lxp_programs_metering_genesis_append(
            &manifest, &metering) != LXP_OK ||
        lxp_programs_fee_genesis_append(
            &manifest, &fee_genesis) != LXP_OK ||
        lxp_genesis_state_root(
            &manifest, &arena, manifest.genesis_state_root) != LXP_OK ||
        genesis_checkpoint_id(
            manifest.network_id, manifest.genesis_state_root,
            manifest.paxeer_genesis_checkpoint_id) != LXP_OK ||
        lxp_genesis_encode(
            &manifest, false, &arena, &preimage) != LXP_OK ||
        sign_raw(private_key, preimage.bytes, preimage.length,
                 manifest.signature) != 0 ||
        lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_genesis_encode(&manifest, true, &arena, &encoded) != LXP_OK)
        return 1;
    file = fopen(path, "wb");
    if (file == NULL) return 1;
    if (fwrite(encoded.bytes, 1U, encoded.length, file) != encoded.length ||
        fclose(file) != 0) return 1;
    return 0;
}

static int load_genesis(
    const char *path, lxp_genesis_manifest *manifest, lxp_arena *arena)
{
    uint8_t *bytes;
    long size;
    FILE *file = fopen(path, "rb");
    lxp_genesis_registration registration;
    bool enabled = false;
    if (file == NULL) {
        (void)fputs("boundary genesis open failed\n", stderr);
        return 1;
    }
    if (fseek(file, 0L, SEEK_END) != 0 ||
        (size = ftell(file)) <= 0 || fseek(file, 0L, SEEK_SET) != 0) {
        if (file != NULL) (void)fclose(file);
        (void)fputs("boundary genesis sizing failed\n", stderr);
        return 1;
    }
    bytes = malloc((size_t)size);
    if (bytes == NULL) {
        (void)fclose(file);
        (void)fputs("boundary genesis allocation failed\n", stderr);
        return 1;
    }
    if (fread(bytes, 1U, (size_t)size, file) != (size_t)size ||
        fclose(file) != 0) {
        free(bytes);
        (void)fputs("boundary genesis read failed\n", stderr);
        return 1;
    }
    if (lxp_genesis_parse(
            bytes, (size_t)size, LXP_GENESIS_INPUT_MANIFEST,
            manifest) != LXP_OK) {
        free(bytes);
        (void)fputs("boundary genesis parse failed\n", stderr);
        return 1;
    }
    if (lxp_genesis_verify_signature(manifest, arena) != LXP_OK) {
        free(bytes);
        (void)fputs("boundary genesis signature failed\n", stderr);
        return 1;
    }
    if (lxp_arena_reset(arena, 0U) != LXP_OK) {
        free(bytes);
        (void)fputs("boundary genesis verification arena reset failed\n", stderr);
        return 1;
    }
    free(bytes);
    (void)memset(&registration, 0, sizeof(registration));
    registration.network_id = manifest->network_id;
    (void)memcpy(registration.checkpoint_id,
                 manifest->paxeer_genesis_checkpoint_id, 32U);
    (void)memcpy(registration.state_root,
                 manifest->genesis_state_root, 32U);
    registration.finalised = true;
    if (lxp_genesis_accept(
            manifest, &registration, true, arena, &enabled) != LXP_OK ||
        !enabled) {
        (void)fputs("boundary genesis acceptance failed\n", stderr);
        return 1;
    }
    return 0;
}

static lxp_result apply_activity(
    void *context, uint64_t global_sequence,
    const uint8_t *bytes, size_t length)
{
    daemon_apply_state *state = (daemon_apply_state *)context;
    lxp_activity activity;
    if (global_sequence != state->expected_sequence ||
        lxp_activity_decode(bytes, length, &activity) != LXP_OK ||
        lxp_activity_check_envelope(&activity, 77U) != LXP_OK ||
        lxp_activity_verify_payload_hash(&activity) != LXP_OK)
        return LXP_ERR_MALFORMED_ENVELOPE;
    ++state->expected_sequence;
    return LXP_OK;
}

static int fixture_init(node_fixture *fixture, lxp_arena *arena)
{
    uint8_t actor[] = {'d','i','d',':','l','x',':','a'};
    uint8_t authority[] = {1U, 2U};
    uint8_t payload[] = {9U, 8U, 7U};
    uint8_t signature[64] = {6U};
    uint8_t state_root[32] = {3U};
    uint8_t batch_id[32] = {4U};
    uint8_t validity_proof[] = {'P','R','O','O','F'};
    uint8_t guarantor_id[32] = {1U};
    uint8_t guarantor_signature[64] = {2U};
    lxp_activity activity;
    lxp_receipt receipt;
    lxp_effect_buffer effects;
    lxp_batch_header header;
    lxp_byte_span encoded;
    uint8_t checkpoint_preimage[LXP_BATCH_HEADER_ENCODED_SIZE + 9U];
    size_t cursor;
    (void)memset(fixture, 0, sizeof(*fixture));
    fixture->state_leaf[0] = 0xa1U;
    fixture->state_leaf[1] = 0x20U;
    fixture->state_leaf[2] = 0x19U;
    (void)memcpy(fixture->event, "EVENT", sizeof(fixture->event));
    fixture->sequencer_key[0] = 9U;
    (void)memset(&activity, 0, sizeof(activity));
    activity.protocol_version = 1U;
    activity.network_id = 77U;
    activity.activity_type = UINT32_C(0x00010001);
    activity.actor_did = (lxp_byte_span){actor, sizeof(actor)};
    activity.authority = (lxp_byte_span){authority, sizeof(authority)};
    activity.account_sequence = 2U;
    activity.timestamp_bound.not_before = 100U;
    activity.timestamp_bound.not_after = 200U;
    activity.idempotency_key[0] = 0xa5U;
    activity.payload = (lxp_byte_span){payload, sizeof(payload)};
    activity.signature = (lxp_byte_span){signature, sizeof(signature)};
    if (lxp_hash_payload(payload, sizeof(payload), activity.payload_hash) != LXP_OK ||
        lxp_activity_encode(&activity, arena, &encoded) != LXP_OK ||
        encoded.length > sizeof(fixture->activity)) return 1;
    fixture->activity_length = encoded.length;
    (void)memcpy(fixture->activity, encoded.bytes, encoded.length);
    if (lxp_activity_id(
            fixture->activity, fixture->activity_length,
            fixture->activity_id) != LXP_OK ||
        lxp_hash_domain(
            LXP_DOMAIN_MERKLE_LEAF, fixture->activity,
            fixture->activity_length, fixture->activity_root) != LXP_OK ||
        lxp_hash_domain(
            LXP_DOMAIN_MERKLE_LEAF, fixture->state_leaf,
            sizeof(fixture->state_leaf), fixture->state_root) != LXP_OK ||
        lxp_hash_domain(
            LXP_DOMAIN_MERKLE_LEAF, fixture->event,
            sizeof(fixture->event), fixture->event_root) != LXP_OK ||
        lxp_arena_reset(arena, 0U) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK) return 1;
    (void)memset(&receipt, 0, sizeof(receipt));
    receipt.protocol_version = activity.protocol_version;
    if (lxp_receipt_build(
            &receipt, fixture->activity_id, 10U, state_root,
            state_root, state_root, LXP_OK, &effects,
            (lxp_u128){0U, 0U}, batch_id, 1U, 1U, 1U) != LXP_OK)
        return 1;
    receipt.sequencer_signature[0] = 7U;
    if (lxp_receipt_encode(&receipt, true, arena, &encoded) != LXP_OK ||
        encoded.length > sizeof(fixture->receipt)) return 1;
    fixture->receipt_length = encoded.length;
    (void)memcpy(fixture->receipt, encoded.bytes, encoded.length);
    if (lxp_hash_domain(
            LXP_DOMAIN_MERKLE_LEAF, fixture->receipt,
            fixture->receipt_length, fixture->receipt_root) != LXP_OK ||
        lxp_arena_reset(arena, 0U) != LXP_OK) return 1;
    (void)memset(&header, 0, sizeof(header));
    header.protocol_version = 1U;
    header.network_id = 77U;
    header.batch_number = 22U;
    header.first_sequence = 10U;
    header.last_sequence = 10U;
    (void)memcpy(header.activity_merkle_root, fixture->activity_root, 32U);
    (void)memcpy(header.receipt_merkle_root, fixture->receipt_root, 32U);
    (void)memcpy(header.event_merkle_root, fixture->event_root, 32U);
    (void)memcpy(header.data_availability_root, fixture->activity_root, 32U);
    (void)memcpy(header.resulting_state_root, fixture->state_root, 32U);
    header.timestamp_ms = UINT64_C(1700000001000);
    header.sequencer_id[0] = 9U;
    if (lxp_batch_header_encode(&header, arena, &encoded) != LXP_OK ||
        encoded.length != sizeof(fixture->header)) return 1;
    fixture->header_length = encoded.length;
    (void)memcpy(fixture->header, encoded.bytes, encoded.length);
    if (lxp_batch_header_hash(&header, arena, fixture->header_hash) != LXP_OK)
        return 1;
    cursor = 0U;
    (void)memcpy(fixture->checkpoint + cursor,
                 fixture->header, fixture->header_length);
    cursor += fixture->header_length;
    store_u32(fixture->checkpoint + cursor, sizeof(validity_proof)); cursor += 4U;
    (void)memcpy(fixture->checkpoint + cursor,
                 validity_proof, sizeof(validity_proof));
    cursor += sizeof(validity_proof);
    store_u32(fixture->checkpoint + cursor, 1U); cursor += 4U;
    store_u32(fixture->checkpoint + cursor, sizeof(guarantor_id)); cursor += 4U;
    (void)memcpy(fixture->checkpoint + cursor,
                 guarantor_id, sizeof(guarantor_id)); cursor += sizeof(guarantor_id);
    store_u32(fixture->checkpoint + cursor, sizeof(guarantor_signature)); cursor += 4U;
    (void)memcpy(fixture->checkpoint + cursor,
                 guarantor_signature, sizeof(guarantor_signature));
    cursor += sizeof(guarantor_signature);
    store_u32(fixture->checkpoint + cursor, 1U); cursor += 4U;
    store_u32(fixture->checkpoint + cursor, 0U); cursor += 4U;
    fixture->checkpoint_length = cursor;
    (void)memcpy(checkpoint_preimage,
                 fixture->header, fixture->header_length);
    store_u32(checkpoint_preimage + fixture->header_length,
              sizeof(validity_proof));
    (void)memcpy(checkpoint_preimage + fixture->header_length + 4U,
                 validity_proof, sizeof(validity_proof));
    return lxp_hash_domain(
        LXP_DOMAIN_CHECKPOINT_CERTIFICATE,
        checkpoint_preimage,
        fixture->header_length + 4U + sizeof(validity_proof),
        fixture->checkpoint_id) == LXP_OK ? 0 : 1;
}

static size_t node_info_payload(
    uint8_t *output, const node_fixture *fixture, node_mode mode)
{
    static const char *capabilities[] = {
        "account_read", "availability_fetch", "batch_header", "checkpoint",
        "event_subscribe", "history_range", "node_info",
        "preparation_state", "proof_bundle", "receipt_lookup", "submit"
    };
    size_t count = mode == NODE_DEGRADED ? 1U :
        sizeof(capabilities) / sizeof(capabilities[0]);
    size_t cursor = 0U;
    size_t index;
    store_u16(output + cursor, 1U); cursor += 2U;
    store_u16(output + cursor, 1U); cursor += 2U;
    store_u16(output + cursor, 1U); cursor += 2U;
    store_u32(output + cursor, 77U); cursor += 4U;
    output[cursor++] = 1U;
    store_u64(output + cursor, mode == NODE_BEHIND ? 5U : 10U); cursor += 8U;
    store_u64(output + cursor, 22U); cursor += 8U;
    (void)memcpy(output + cursor, fixture->checkpoint_id, 32U); cursor += 32U;
    (void)memcpy(output + cursor, fixture->sequencer_key, 32U); cursor += 32U;
    store_u16(output + cursor, (uint16_t)count); cursor += 2U;
    for (index = 0U; index < count; ++index) {
        const char *name = mode == NODE_DEGRADED ? "node_info" : capabilities[index];
        size_t length = strlen(name);
        store_u16(output + cursor, (uint16_t)length); cursor += 2U;
        (void)memcpy(output + cursor, name, length); cursor += length;
    }
    return cursor;
}

static int send_error(int descriptor, uint64_t correlation_id, uint8_t code)
{
    return send_envelope(
        descriptor, 25U, correlation_id, &code, 1U, NULL, 0U);
}

static int handle_request(
    int descriptor, const uint8_t *request, size_t length,
    node_fixture *fixture, preparation_fixture *preparation,
    lxp_daemon *daemon, node_mode mode)
{
    uint16_t major;
    uint16_t tag;
    uint64_t correlation_id;
    uint32_t payload_length;
    const uint8_t *payload;
    uint8_t info[1024];
    uint8_t end[33];
    if (length < 22U) return send_error(descriptor, 0U, 1U);
    major = load_u16(request);
    tag = load_u16(request + 4U);
    correlation_id = load_u64(request + 6U);
    payload_length = load_u32(request + 14U);
    if ((size_t)payload_length + 22U > length)
        return send_error(descriptor, correlation_id, 1U);
    payload = request + 18U;
    if (major != 1U) return send_error(descriptor, correlation_id, 2U);
    if (tag == 1U) {
        size_t info_length = node_info_payload(info, fixture, mode);
        return send_envelope(
            descriptor, 2U, correlation_id,
            info, info_length, NULL, 0U);
    }
    if (mode == NODE_DEGRADED)
        return send_error(descriptor, correlation_id, 3U);
    switch (tag) {
    case 3U:
        if (payload_length == 0U ||
            lxp_daemon_submit(daemon, payload, payload_length) != LXP_OK)
            return send_error(descriptor, correlation_id, 4U);
        return send_envelope(
            descriptor, 4U, correlation_id,
            payload, payload_length,
            fixture->activity_id, sizeof(fixture->activity_id));
    case 5U:
        return send_envelope(
            descriptor, 6U, correlation_id,
            fixture->receipt, fixture->receipt_length,
            fixture->receipt_root, sizeof(fixture->receipt_root));
    case 7U:
        return send_envelope(
            descriptor, 8U, correlation_id,
            fixture->state_leaf, sizeof(fixture->state_leaf),
            fixture->state_root, sizeof(fixture->state_root));
    case 9U:
        if (send_envelope(
                descriptor, 10U, correlation_id,
                fixture->activity, fixture->activity_length,
                fixture->activity_root, sizeof(fixture->activity_root)) != 0)
            return 1;
        store_u64(end, 11U);
        return send_envelope(
            descriptor, 11U, correlation_id, end, 8U, NULL, 0U);
    case 12U:
        return send_envelope(
            descriptor, 13U, correlation_id,
            fixture->header, fixture->header_length,
            fixture->header_hash, sizeof(fixture->header_hash));
    case 14U:
        return send_envelope(
            descriptor, 15U, correlation_id,
            fixture->checkpoint, fixture->checkpoint_length,
            fixture->checkpoint_id, sizeof(fixture->checkpoint_id));
    case 16U:
        return send_envelope(
            descriptor, 17U, correlation_id,
            fixture->state_leaf, sizeof(fixture->state_leaf),
            fixture->state_root, sizeof(fixture->state_root));
    case 18U:
        if (send_envelope(
                descriptor, 19U, correlation_id,
                fixture->activity, fixture->activity_length,
                fixture->activity_root, sizeof(fixture->activity_root)) != 0)
            return 1;
        end[0] = 0x1fU;
        (void)memcpy(end + 1U, fixture->activity_root, 32U);
        return send_envelope(
            descriptor, 20U, correlation_id,
            end, sizeof(end),
            fixture->activity_root, sizeof(fixture->activity_root));
    case 21U:
        if (send_envelope(
                descriptor, 22U, correlation_id,
                fixture->event, sizeof(fixture->event),
                fixture->event_root, sizeof(fixture->event_root)) != 0)
            return 1;
        (void)memset(end, 0, 16U);
        if (send_envelope(
                descriptor, 23U, correlation_id,
                end, 16U, NULL, 0U) != 0) return 1;
        store_u64(end, mode == NODE_BEHIND ? 5U : 10U);
        return send_envelope(
            descriptor, 24U, correlation_id, end, 8U, NULL, 0U);
    case 26U: {
        uint8_t snapshot[4096];
        size_t snapshot_length = 0U;
        lxp_result status = lxp_daemon_lni_preparation_state(
            &preparation->owner, payload, payload_length,
            snapshot, sizeof(snapshot), &snapshot_length);
        if (status != LXP_OK)
            return send_error(descriptor, correlation_id, 4U);
        return send_envelope(
            descriptor, 27U, correlation_id,
            snapshot, snapshot_length, NULL, 0U);
    }
    default:
        return send_error(descriptor, correlation_id, 1U);
    }
}

static int serve_connection(
    int descriptor, node_fixture *fixture,
    preparation_fixture *preparation, lxp_daemon *daemon, node_mode mode)
{
    uint8_t prefix[4];
    while (running) {
        uint8_t *frame;
        uint32_t length;
        if (read_exact(descriptor, prefix, sizeof(prefix)) != 0) return 0;
        length = load_u32(prefix);
        if (length == 0U || length > MAX_FRAME) return 1;
        frame = malloc(length);
        if (frame == NULL || read_exact(descriptor, frame, length) != 0) {
            free(frame);
            return 1;
        }
        if (handle_request(
                descriptor, frame, length,
                fixture, preparation, daemon, mode) != 0) {
            free(frame);
            return 1;
        }
        free(frame);
    }
    return 0;
}

static int serve_node(
    const char *socket_path, const char *genesis_path, node_mode mode)
{
    static uint8_t arena_storage[ARENA_BYTES];
    static lxp_genesis_manifest genesis;
    static node_fixture fixture;
    static preparation_fixture preparation;
    static lxp_daemon daemon;
    lxp_daemon_configuration configuration;
    daemon_apply_state apply_state = {10U};
    lxp_arena arena;
    struct sockaddr_un address;
    int listener;
    int result = 1;
    if (strlen(socket_path) >= sizeof(address.sun_path)) {
        (void)fputs("boundary node socket path is too long\n", stderr);
        return 1;
    }
    if (lxp_arena_init(&arena, arena_storage, sizeof(arena_storage)) != LXP_OK) {
        (void)fputs("boundary node arena initialization failed\n", stderr);
        return 1;
    }
    if (load_genesis(genesis_path, &genesis, &arena) != 0) {
        (void)fputs("boundary node genesis load failed\n", stderr);
        return 1;
    }
    if (lxp_arena_reset(&arena, 0U) != LXP_OK) {
        (void)fputs("boundary node arena reset failed\n", stderr);
        return 1;
    }
    if (fixture_init(&fixture, &arena) != 0) {
        (void)fputs("boundary node protocol fixture failed\n", stderr);
        return 1;
    }
    (void)memset(&preparation, 0, sizeof(preparation));
    {
        static const uint8_t actor[] =
            "did:layerx:production-boundary";
        static uint64_t parameters = 1U;
        lxp_identity *identity = NULL;
        if (lxp_state_store_init(&preparation.state, 11U) != LXP_OK) {
            (void)fputs("boundary preparation state initialization failed\n", stderr);
            return 1;
        }
        if (lxp_kernel_create(
                &preparation.kernel, &preparation.state,
                &preparation.journal, &parameters, 3U) != LXP_OK) {
            (void)fputs("boundary preparation kernel creation failed\n", stderr);
            return 1;
        }
        if (lxp_kernel_register_module(
                &preparation.kernel,
                programs_module_registration_v3()) != LXP_OK) {
            (void)fputs("boundary preparation module registration failed\n", stderr);
            return 1;
        }
        if (lxp_state_root(
                &preparation.kernel,
                preparation.kernel.current_state_root) != LXP_OK) {
            (void)fputs("boundary preparation state root failed\n", stderr);
            return 1;
        }
        if (lxp_identity_register(
                &preparation.identities, actor, sizeof(actor) - 1U,
                (const uint8_t[32]){7U}, &identity) != LXP_OK ||
            identity == NULL) {
            (void)fputs("boundary preparation identity registration failed\n", stderr);
            return 1;
        }
        if (pthread_mutex_init(&preparation.owner.mutex, NULL) != 0) {
            (void)fputs("boundary preparation owner mutex failed\n", stderr);
            return 1;
        }
        identity->next_sequence = 5U;
        preparation.owner.kernel = &preparation.kernel;
        preparation.owner.identities = &preparation.identities;
        preparation.owner.receipt_authority = &preparation.authority;
        preparation.owner.network_id = 77U;
        preparation.authority.record_count = 1U;
        preparation.authority.last_global_sequence = 10U;
        preparation.authority.last_sealed_timestamp =
            UINT64_C(1700000001000);
        preparation.owner.latest_sealed_timestamp =
            preparation.authority.last_sealed_timestamp;
        preparation.owner.feed_store.scanned_through_sequence = 10U;
        preparation.owner.feed_store.head_timestamp =
            preparation.owner.latest_sealed_timestamp;
        (void)memcpy(preparation.owner.feed_store.head_state_root,
                     preparation.kernel.current_state_root, 32U);
        preparation.owner.attached = true;
        preparation.initialized = true;
    }
    (void)memset(&configuration, 0, sizeof(configuration));
    configuration.role = LXP_DAEMON_SEQUENCER;
    configuration.network_id = genesis.network_id;
    configuration.start_sequence = 10U;
    configuration.serial_execution = true;
    if (lxp_daemon_start(
            &daemon, &configuration,
            apply_activity, &apply_state) != LXP_OK) {
        (void)fputs("boundary daemon start failed\n", stderr);
        return 1;
    }
    listener = socket(AF_UNIX, SOCK_STREAM, 0);
    if (listener < 0) goto shutdown;
    (void)memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    (void)memcpy(address.sun_path, socket_path, strlen(socket_path) + 1U);
    (void)unlink(socket_path);
    if (bind(listener, (struct sockaddr *)&address, sizeof(address)) != 0 ||
        listen(listener, 16) != 0) goto close_listener;
    (void)signal(SIGTERM, stop_running);
    (void)signal(SIGINT, stop_running);
    result = 0;
    while (running) {
        int connection = accept(listener, NULL, NULL);
        if (connection >= 0) {
            if (serve_connection(
                    connection, &fixture, &preparation,
                    &daemon, mode) != 0) result = 1;
            (void)close(connection);
        } else if (errno != EINTR) {
            result = 1;
            break;
        }
    }
close_listener:
    (void)close(listener);
    (void)unlink(socket_path);
shutdown:
    if (lxp_daemon_shutdown(&daemon) != LXP_OK) result = 1;
    if (preparation.initialized) {
        if (pthread_mutex_destroy(&preparation.owner.mutex) != 0 ||
            lxp_state_store_destroy(&preparation.state) != LXP_OK)
            result = 1;
        preparation.initialized = false;
    }
    return result;
}

int main(int argc, char **argv)
{
    node_mode mode;
    if (argc == 3 && strcmp(argv[1], "--write-genesis") == 0)
        return write_genesis(argv[2]);
    if (argc != 5 || strcmp(argv[1], "--serve") != 0) return 2;
    if (strcmp(argv[4], "normal") == 0) mode = NODE_NORMAL;
    else if (strcmp(argv[4], "behind") == 0) mode = NODE_BEHIND;
    else if (strcmp(argv[4], "degraded") == 0) mode = NODE_DEGRADED;
    else return 2;
    return serve_node(argv[2], argv[3], mode);
}
