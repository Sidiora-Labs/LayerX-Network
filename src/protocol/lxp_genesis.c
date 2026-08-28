#include "layerx/lxp_genesis.h"

#include "layerx/programs.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_protocol.h"

#include <string.h>

enum { LXP_GENESIS_STRUCTURE_TAG = 0x4701 };

static int keyed_compare(
    uint16_t left_module, const uint8_t left_key[32],
    uint16_t right_module, const uint8_t right_key[32])
{
    if (left_module != right_module)
        return left_module < right_module ? -1 : 1;
    return memcmp(left_key, right_key, 32U);
}

static lxp_result validate(const lxp_genesis_manifest *manifest)
{
    size_t i;
    if (manifest == NULL ||
        !lxp_protocol_version_supported(manifest->protocol_version) ||
        manifest->network_id == 0U || manifest->genesis_timestamp_ms == 0U ||
        manifest->parameter_count == 0U ||
        manifest->parameter_count > LXP_GENESIS_MAX_PARAMETERS ||
        manifest->guarantor_count == 0U ||
        manifest->guarantor_count > LXP_GENESIS_MAX_GUARANTORS ||
        manifest->account_count == 0U ||
        manifest->account_count > LXP_GENESIS_MAX_ACCOUNTS ||
        manifest->module_value_count > LXP_GENESIS_MAX_MODULE_VALUES)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < manifest->parameter_count; ++i) {
        if (manifest->parameters[i].module_id == 0U ||
            lxp_ct_is_zero(manifest->parameters[i].key, 32U) ||
            (i != 0U && keyed_compare(
                manifest->parameters[i - 1U].module_id,
                manifest->parameters[i - 1U].key,
                manifest->parameters[i].module_id,
                manifest->parameters[i].key) >= 0))
            return LXP_ERR_UNSORTED_SEQUENCE;
    }
    for (i = 0U; i < manifest->guarantor_count; ++i) {
        if (lxp_ct_is_zero(manifest->guarantors[i].guarantor_id, 32U) ||
            lxp_ct_is_zero(manifest->guarantors[i].public_key, 33U) ||
            lxp_u128_is_zero(manifest->guarantors[i].bond) ||
            (i != 0U && memcmp(
                manifest->guarantors[i - 1U].guarantor_id,
                manifest->guarantors[i].guarantor_id, 32U) >= 0))
            return LXP_ERR_UNSORTED_SEQUENCE;
    }
    for (i = 0U; i < manifest->account_count; ++i) {
        const lxp_genesis_account *account = &manifest->accounts[i];
        int order = i == 0U ? -1 : memcmp(
            manifest->accounts[i - 1U].asset_id, account->asset_id, 32U);
        if (i != 0U && order == 0)
            order = memcmp(manifest->accounts[i - 1U].account_id,
                           account->account_id, 32U);
        if (lxp_ct_is_zero(account->account_id, 32U) ||
            lxp_ct_is_zero(account->asset_id, 32U) ||
            (account->locked && account->subaccount_kind != 0U &&
             lxp_ct_is_zero(account->parent_account_id, 32U)) ||
            (i != 0U && order >= 0))
            return LXP_ERR_UNSORTED_SEQUENCE;
    }
    for (i = 0U; i < manifest->module_value_count; ++i) {
        if (manifest->module_values[i].module_id == 0U ||
            manifest->module_values[i].value_length == 0U ||
            manifest->module_values[i].value_length >
                LXP_GENESIS_MODULE_VALUE_BYTES ||
            (i != 0U && keyed_compare(
                manifest->module_values[i - 1U].module_id,
                manifest->module_values[i - 1U].key,
                manifest->module_values[i].module_id,
                manifest->module_values[i].key) >= 0))
            return LXP_ERR_UNSORTED_SEQUENCE;
    }
    return LXP_OK;
}

static lxp_result encode_content(
    const lxp_genesis_manifest *manifest, lxp_codec_writer *writer)
{
    size_t i;
    lxp_result status = lxp_codec_write_u16(
        writer, manifest->protocol_version);
    if (status == LXP_OK)
        status = lxp_codec_write_u32(writer, manifest->network_id);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(
            writer, manifest->genesis_timestamp_ms);
    if (status == LXP_OK)
        status = lxp_codec_write_u32(
            writer, (uint32_t)manifest->parameter_count);
    for (i = 0U; status == LXP_OK && i < manifest->parameter_count; ++i) {
        status = lxp_codec_write_u16(
            writer, manifest->parameters[i].module_id);
        if (status == LXP_OK) status = lxp_codec_write_bytes(
            writer, manifest->parameters[i].key, 32U, 32U);
        if (status == LXP_OK) status = lxp_codec_write_bytes(
            writer, manifest->parameters[i].value, 32U, 32U);
    }
    if (status == LXP_OK) status = lxp_codec_write_u32(
        writer, (uint32_t)manifest->guarantor_count);
    for (i = 0U; status == LXP_OK && i < manifest->guarantor_count; ++i) {
        status = lxp_codec_write_bytes(
            writer, manifest->guarantors[i].guarantor_id, 32U, 32U);
        if (status == LXP_OK) status = lxp_codec_write_bytes(
            writer, manifest->guarantors[i].public_key, 33U, 33U);
        if (status == LXP_OK) status = lxp_codec_write_u128(
            writer, manifest->guarantors[i].bond);
    }
    if (status == LXP_OK) status = lxp_codec_write_u32(
        writer, (uint32_t)manifest->account_count);
    for (i = 0U; status == LXP_OK && i < manifest->account_count; ++i) {
        const lxp_genesis_account *account = &manifest->accounts[i];
        status = lxp_codec_write_bytes(
            writer, account->account_id, 32U, 32U);
        if (status == LXP_OK) status = lxp_codec_write_bytes(
            writer, account->asset_id, 32U, 32U);
        if (status == LXP_OK)
            status = lxp_codec_write_u128(writer, account->balance);
        if (status == LXP_OK)
            status = lxp_codec_write_u8(writer, account->locked ? 1U : 0U);
        if (status == LXP_OK)
            status = lxp_codec_write_u16(writer, account->subaccount_kind);
        if (status == LXP_OK) status = lxp_codec_write_bytes(
            writer, account->parent_account_id, 32U, 32U);
    }
    if (status == LXP_OK) status = lxp_codec_write_u32(
        writer, (uint32_t)manifest->module_value_count);
    for (i = 0U; status == LXP_OK && i < manifest->module_value_count; ++i) {
        status = lxp_codec_write_u16(
            writer, manifest->module_values[i].module_id);
        if (status == LXP_OK) status = lxp_codec_write_bytes(
            writer, manifest->module_values[i].key, 32U, 32U);
        if (status == LXP_OK) status = lxp_codec_write_bytes(
            writer, manifest->module_values[i].value,
            manifest->module_values[i].value_length,
            LXP_GENESIS_MODULE_VALUE_BYTES);
    }
    return status;
}

lxp_result lxp_genesis_encode(
    const lxp_genesis_manifest *manifest, bool include_signature,
    lxp_arena *arena, lxp_byte_span *encoded)
{
    lxp_codec_writer writer;
    lxp_result status = validate(manifest);
    if (status != LXP_OK || arena == NULL || encoded == NULL)
        return status == LXP_OK ? LXP_ERR_NON_CANONICAL : status;
    status = lxp_codec_writer_init(
        &writer, arena, LXP_GENESIS_MAX_ENCODED_BYTES);
    if (status == LXP_OK)
        status = lxp_codec_write_struct_header_version(
            &writer, LXP_GENESIS_STRUCTURE_TAG,
            manifest->protocol_version);
    if (status == LXP_OK) status = encode_content(manifest, &writer);
    if (status == LXP_OK) status = lxp_codec_write_bytes(
        &writer, manifest->genesis_state_root, 32U, 32U);
    if (status == LXP_OK) status = lxp_codec_write_bytes(
        &writer, manifest->paxeer_genesis_checkpoint_id, 32U, 32U);
    if (status == LXP_OK) status = lxp_codec_write_bytes(
        &writer, manifest->signer_public_key, 32U, 32U);
    if (status == LXP_OK && include_signature)
        status = lxp_codec_write_bytes(
            &writer, manifest->signature, 64U, 64U);
    if (status != LXP_OK) return status;
    *encoded = (lxp_byte_span){writer.bytes, writer.length};
    return LXP_OK;
}

static lxp_result read_fixed(
    lxp_codec_reader *reader, uint8_t *output, size_t length)
{
    lxp_byte_span span;
    lxp_result status = lxp_codec_read_bytes(
        reader, &span, (uint32_t)length);
    if (status != LXP_OK || span.length != length)
        return status == LXP_OK ? LXP_ERR_NON_CANONICAL : status;
    (void)memcpy(output, span.bytes, length);
    return LXP_OK;
}

lxp_result lxp_genesis_parse(
    const uint8_t *bytes, size_t length, lxp_genesis_input_kind input_kind,
    lxp_genesis_manifest *manifest)
{
    lxp_codec_reader reader;
    uint32_t count = 0U;
    uint8_t locked;
    uint16_t envelope_version = 0U;
    lxp_byte_span value;
    size_t i;
    lxp_result status;
    if (input_kind != LXP_GENESIS_INPUT_MANIFEST)
        return LXP_ERR_NON_CANONICAL;
    if (bytes == NULL || length == 0U || manifest == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(manifest, 0, sizeof(*manifest));
    status = lxp_codec_reader_init(&reader, bytes, length);
    if (status == LXP_OK) status = lxp_codec_read_struct_header_version(
        &reader, LXP_GENESIS_STRUCTURE_TAG, &envelope_version);
    if (status == LXP_OK)
        status = lxp_codec_read_u16(&reader, &manifest->protocol_version);
    if (status == LXP_OK && manifest->protocol_version != envelope_version)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK)
        status = lxp_codec_read_u32(&reader, &manifest->network_id);
    if (status == LXP_OK) status = lxp_codec_read_u64(
        &reader, &manifest->genesis_timestamp_ms);
    if (status == LXP_OK) status = lxp_codec_read_u32(&reader, &count);
    if (status == LXP_OK && count > LXP_GENESIS_MAX_PARAMETERS)
        status = LXP_ERR_LENGTH_LIMIT;
    manifest->parameter_count = count;
    for (i = 0U; status == LXP_OK && i < count; ++i) {
        status = lxp_codec_read_u16(
            &reader, &manifest->parameters[i].module_id);
        if (status == LXP_OK) status = read_fixed(
            &reader, manifest->parameters[i].key, 32U);
        if (status == LXP_OK) status = read_fixed(
            &reader, manifest->parameters[i].value, 32U);
    }
    if (status == LXP_OK) status = lxp_codec_read_u32(&reader, &count);
    if (status == LXP_OK && count > LXP_GENESIS_MAX_GUARANTORS)
        status = LXP_ERR_LENGTH_LIMIT;
    manifest->guarantor_count = count;
    for (i = 0U; status == LXP_OK && i < count; ++i) {
        status = read_fixed(&reader,
            manifest->guarantors[i].guarantor_id, 32U);
        if (status == LXP_OK) status = read_fixed(&reader,
            manifest->guarantors[i].public_key, 33U);
        if (status == LXP_OK) status = lxp_codec_read_u128(
            &reader, &manifest->guarantors[i].bond);
    }
    if (status == LXP_OK) status = lxp_codec_read_u32(&reader, &count);
    if (status == LXP_OK && count > LXP_GENESIS_MAX_ACCOUNTS)
        status = LXP_ERR_LENGTH_LIMIT;
    manifest->account_count = count;
    for (i = 0U; status == LXP_OK && i < count; ++i) {
        lxp_genesis_account *account = &manifest->accounts[i];
        status = read_fixed(&reader, account->account_id, 32U);
        if (status == LXP_OK)
            status = read_fixed(&reader, account->asset_id, 32U);
        if (status == LXP_OK)
            status = lxp_codec_read_u128(&reader, &account->balance);
        if (status == LXP_OK)
            status = lxp_codec_read_u8(&reader, &locked);
        if (status == LXP_OK && locked > 1U)
            status = LXP_ERR_NON_CANONICAL;
        account->locked = locked == 1U;
        if (status == LXP_OK) status = lxp_codec_read_u16(
            &reader, &account->subaccount_kind);
        if (status == LXP_OK) status = read_fixed(
            &reader, account->parent_account_id, 32U);
    }
    if (status == LXP_OK) status = lxp_codec_read_u32(&reader, &count);
    if (status == LXP_OK && count > LXP_GENESIS_MAX_MODULE_VALUES)
        status = LXP_ERR_LENGTH_LIMIT;
    manifest->module_value_count = count;
    for (i = 0U; status == LXP_OK && i < count; ++i) {
        lxp_genesis_module_value *module = &manifest->module_values[i];
        status = lxp_codec_read_u16(&reader, &module->module_id);
        if (status == LXP_OK)
            status = read_fixed(&reader, module->key, 32U);
        if (status == LXP_OK) status = lxp_codec_read_bytes(
            &reader, &value, LXP_GENESIS_MODULE_VALUE_BYTES);
        if (status == LXP_OK && value.length == 0U)
            status = LXP_ERR_NON_CANONICAL;
        if (status == LXP_OK) {
            module->value_length = value.length;
            (void)memcpy(module->value, value.bytes, value.length);
        }
    }
    if (status == LXP_OK) status = read_fixed(
        &reader, manifest->genesis_state_root, 32U);
    if (status == LXP_OK) status = read_fixed(
        &reader, manifest->paxeer_genesis_checkpoint_id, 32U);
    if (status == LXP_OK) status = read_fixed(
        &reader, manifest->signer_public_key, 32U);
    if (status == LXP_OK)
        status = read_fixed(&reader, manifest->signature, 64U);
    if (status == LXP_OK) status = lxp_codec_finish(&reader);
    if (status == LXP_OK) status = validate(manifest);
    return status;
}

lxp_result lxp_genesis_state_root(
    const lxp_genesis_manifest *manifest, lxp_arena *arena,
    uint8_t state_root[32])
{
    lxp_codec_writer writer;
    size_t mark;
    lxp_result status = validate(manifest);
    if (status != LXP_OK || arena == NULL || state_root == NULL)
        return status == LXP_OK ? LXP_ERR_NON_CANONICAL : status;
    mark = lxp_arena_mark(arena);
    status = lxp_codec_writer_init(
        &writer, arena, LXP_GENESIS_MAX_ENCODED_BYTES);
    if (status == LXP_OK) status = encode_content(manifest, &writer);
    if (status == LXP_OK) status = lxp_hash_domain(
        LXP_DOMAIN_STATE_ROOT_CHAIN,
        writer.bytes, writer.length, state_root);
    (void)lxp_arena_reset(arena, mark);
    return status;
}

static lxp_result checkpoint_id_for(
    uint32_t network_id, const uint8_t state_root[32], uint8_t output[32])
{
    uint8_t preimage[36];
    preimage[0] = (uint8_t)(network_id >> 24U);
    preimage[1] = (uint8_t)(network_id >> 16U);
    preimage[2] = (uint8_t)(network_id >> 8U);
    preimage[3] = (uint8_t)network_id;
    (void)memcpy(preimage + 4U, state_root, 32U);
    return lxp_hash_domain(
        LXP_DOMAIN_CHECKPOINT_CERTIFICATE,
        preimage, sizeof(preimage), output);
}

lxp_result lxp_genesis_verify_signature(
    const lxp_genesis_manifest *manifest, lxp_arena *arena)
{
    uint8_t state_root[32];
    uint8_t checkpoint_id[32];
    lxp_byte_span preimage;
    size_t mark;
    lxp_result status;
    if (manifest == NULL || arena == NULL ||
        lxp_ct_is_zero(manifest->signer_public_key, 32U) ||
        lxp_ct_is_zero(manifest->signature, 64U))
        return LXP_ERR_BAD_SIGNATURE;
    status = lxp_genesis_state_root(manifest, arena, state_root);
    if (status == LXP_OK && lxp_ct_memcmp(
            state_root, manifest->genesis_state_root, 32U) != 0)
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK)
        status = checkpoint_id_for(
            manifest->network_id, state_root, checkpoint_id);
    if (status == LXP_OK && lxp_ct_memcmp(
            checkpoint_id,
            manifest->paxeer_genesis_checkpoint_id, 32U) != 0)
        status = LXP_ERR_ROOT_MISMATCH;
    mark = lxp_arena_mark(arena);
    if (status == LXP_OK)
        status = lxp_genesis_encode(manifest, false, arena, &preimage);
    if (status == LXP_OK)
        status = lxp_ed25519_verify_raw(
            manifest->signer_public_key, manifest->signature,
            preimage.bytes, preimage.length);
    (void)lxp_arena_reset(arena, mark);
    return status;
}

lxp_result lxp_genesis_accept(
    const lxp_genesis_manifest *manifest,
    const lxp_genesis_registration *registration,
    bool storage_empty, lxp_arena *arena, bool *activities_enabled)
{
    lxp_result status;
    if (activities_enabled == NULL) return LXP_ERR_NON_CANONICAL;
    *activities_enabled = false;
    if (!storage_empty || manifest == NULL || registration == NULL ||
        !registration->finalised || registration->registration_index != 0U ||
        registration->network_id != manifest->network_id ||
        lxp_ct_memcmp(registration->state_root,
                      manifest->genesis_state_root, 32U) != 0 ||
        lxp_ct_memcmp(registration->checkpoint_id,
                      manifest->paxeer_genesis_checkpoint_id, 32U) != 0)
        return LXP_ERR_ROOT_MISMATCH;
    status = lxp_genesis_verify_signature(manifest, arena);
    if (status == LXP_OK)
        status = lxp_programs_metering_genesis_validate(manifest);
    if (status == LXP_OK) *activities_enabled = true;
    return status;
}
