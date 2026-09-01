#include "layerx/lxp_genesis.h"

#include "layerx/programs.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_state.h"
#include "layerx/lxp_snapshot.h"

#include <stdlib.h>
#include <string.h>

enum { LXP_GENESIS_STRUCTURE_TAG = 0x4701 };

static const uint8_t parameter_version_key[32] = {
    'p','a','r','a','m','e','t','e','r','-','v','e','r','s','i','o','n'
};

static const uint8_t genesis_manifest_key[] = "genesis/manifest/v1";

static const char *fresh_system_name(uint16_t kind)
{
    switch ((lx_account_kind)kind) {
    case LX_ACCOUNT_SYSTEM_INSURANCE: return "system:insurance";
    case LX_ACCOUNT_SYSTEM_FEES: return "system:fees";
    case LX_ACCOUNT_SYSTEM_PAXEER_RESERVE: return "system:paxeer-reserve";
    case LX_ACCOUNT_SYSTEM_PAXEER_WITHDRAWALS:
        return "system:paxeer-withdrawals";
    default: return NULL;
    }
}

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
    bool fees = false;
    bool reserve = false;
    bool withdrawals = false;
    if (manifest == NULL ||
        !lxp_protocol_version_supported(manifest->protocol_version) ||
        manifest->network_id == 0U || manifest->genesis_timestamp_ms == 0U ||
        manifest->parameter_count == 0U ||
        manifest->parameter_count > LXP_GENESIS_MAX_PARAMETERS ||
        manifest->guarantor_count == 0U ||
        manifest->guarantor_count > LXP_GENESIS_MAX_GUARANTORS ||
        manifest->account_count < LXP_GENESIS_FRESH_SYSTEM_ACCOUNT_COUNT ||
        manifest->account_count > LXP_GENESIS_MAX_ACCOUNTS ||
        manifest->module_value_count > LXP_GENESIS_MAX_MODULE_VALUES)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < manifest->parameter_count; ++i) {
        if (manifest->parameters[i].module_id == 0U ||
            manifest->parameters[i].module_id > LXP_MODULE_RESERVED_COUNT ||
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
            !lxp_u128_is_zero(manifest->guarantors[i].bond) ||
            (i != 0U && memcmp(
                manifest->guarantors[i - 1U].guarantor_id,
                manifest->guarantors[i].guarantor_id, 32U) >= 0))
            return LXP_ERR_UNSORTED_SEQUENCE;
    }
    for (i = 0U; i < manifest->account_count; ++i) {
        const lxp_genesis_account *account = &manifest->accounts[i];
        const char *name = fresh_system_name(account->subaccount_kind);
        uint8_t derived[32];
        int order = i == 0U ? -1 : memcmp(
            manifest->accounts[i - 1U].asset_id, account->asset_id, 32U);
        if (i != 0U && order == 0)
            order = memcmp(manifest->accounts[i - 1U].account_id,
                           account->account_id, 32U);
        if (name == NULL ||
            lx_account_id_from_string((const uint8_t *)name, strlen(name),
                                      derived) != LXP_OK ||
            lxp_ct_memcmp(derived, account->account_id, 32U) != 0 ||
            lxp_ct_is_zero(account->asset_id, 32U) ||
            !lxp_u128_is_zero(account->balance) || account->locked ||
            !lxp_ct_is_zero(account->parent_account_id, 32U) ||
            (i != 0U && lxp_ct_memcmp(manifest->accounts[0].asset_id,
                                      account->asset_id, 32U) != 0) ||
            (i != 0U && order >= 0))
            return LXP_ERR_UNSORTED_SEQUENCE;
        if (account->subaccount_kind == LX_ACCOUNT_SYSTEM_FEES) {
            if (fees) return LXP_ERR_SEQUENCE_REUSED;
            fees = true;
        } else if (account->subaccount_kind ==
                   LX_ACCOUNT_SYSTEM_PAXEER_RESERVE) {
            if (reserve) return LXP_ERR_SEQUENCE_REUSED;
            reserve = true;
        } else if (account->subaccount_kind ==
                   LX_ACCOUNT_SYSTEM_PAXEER_WITHDRAWALS) {
            if (withdrawals) return LXP_ERR_SEQUENCE_REUSED;
            withdrawals = true;
        }
    }
    if (!fees || !reserve || !withdrawals) return LXP_ERR_UNKNOWN_FIELD;
    for (i = 0U; i < manifest->module_value_count; ++i) {
        if (manifest->module_values[i].module_id == 0U ||
            manifest->module_values[i].module_id > LXP_MODULE_RESERVED_COUNT ||
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
        &writer, manifest->genesis_receipt_state_root, 32U, 32U);
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
        &reader, manifest->genesis_receipt_state_root, 32U);
    if (status == LXP_OK) status = read_fixed(
        &reader, manifest->signer_public_key, 32U);
    if (status == LXP_OK)
        status = read_fixed(&reader, manifest->signature, 64U);
    if (status == LXP_OK) status = lxp_codec_finish(&reader);
    if (status == LXP_OK) status = validate(manifest);
    return status;
}

static int kernel_entry_order(uint16_t module_id, const uint8_t *key,
                              size_t key_length,
                              const lxp_module_kv_entry *right)
{
    size_t common;
    int order;
    if (module_id < right->module_id) return -1;
    if (module_id > right->module_id) return 1;
    common = key_length < right->key_length ? key_length : right->key_length;
    order = memcmp(key, right->key, common);
    if (order != 0) return order;
    return key_length < right->key_length ? -1 :
           key_length > right->key_length ? 1 : 0;
}

static lxp_result kernel_insert(lxp_kernel *kernel, uint16_t module_id,
                                const uint8_t *key, size_t key_length,
                                const uint8_t *value, size_t value_length)
{
    size_t location = 0U;
    if (kernel == NULL || module_id == 0U ||
        module_id > LXP_MODULE_RESERVED_COUNT || key == NULL ||
        key_length == 0U || key_length > LXP_MODULE_MAX_KEY_BYTES ||
        value == NULL || value_length == 0U ||
        value_length > LXP_MODULE_MAX_VALUE_BYTES ||
        kernel->module_kv_count == LXP_KERNEL_MAX_MODULE_KV)
        return LXP_ERR_LENGTH_LIMIT;
    while (location < kernel->module_kv_count &&
           kernel_entry_order(module_id, key, key_length,
                              &kernel->module_kv[location]) > 0)
        ++location;
    if (location < kernel->module_kv_count &&
        kernel_entry_order(module_id, key, key_length,
                           &kernel->module_kv[location]) == 0)
        return LXP_ERR_SEQUENCE_REUSED;
    (void)memmove(&kernel->module_kv[location + 1U],
                  &kernel->module_kv[location],
                  (kernel->module_kv_count - location) *
                      sizeof(kernel->module_kv[0]));
    (void)memset(&kernel->module_kv[location], 0,
                 sizeof(kernel->module_kv[location]));
    kernel->module_kv[location].module_id = module_id;
    kernel->module_kv[location].key_length = (uint16_t)key_length;
    kernel->module_kv[location].value_length = (uint32_t)value_length;
    (void)memcpy(kernel->module_kv[location].key, key, key_length);
    (void)memcpy(kernel->module_kv[location].value, value, value_length);
    ++kernel->module_kv_count;
    return LXP_OK;
}

lxp_result lxp_genesis_manifest_commitment(
    const lxp_genesis_manifest *manifest, lxp_arena *arena,
    uint8_t digest[32])
{
    lxp_codec_writer writer;
    lxp_hash_context context;
    const uint8_t *tag;
    size_t tag_length = 0U;
    size_t mark;
    lxp_result status;
    if (manifest == NULL || arena == NULL || digest == NULL)
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(arena);
    status = lxp_codec_writer_init(&writer, arena,
                                   LXP_GENESIS_MAX_ENCODED_BYTES);
    if (status == LXP_OK) status = encode_content(manifest, &writer);
    tag = status == LXP_OK ?
        lxp_domain_tag(LXP_DOMAIN_GENESIS_MANIFEST, &tag_length) : NULL;
    if (status == LXP_OK && tag == NULL) status = LXP_ERR_INVALID_TAG;
    if (status == LXP_OK) {
        lxp_hash_init(&context);
        status = lxp_hash_update(&context, tag, tag_length);
        if (status == LXP_OK)
            status = lxp_hash_update(&context, writer.bytes, writer.length);
        if (status == LXP_OK)
            status = lxp_hash_update(&context, manifest->signer_public_key, 32U);
        if (status == LXP_OK) status = lxp_hash_final(&context, digest);
    }
    (void)lxp_arena_reset(arena, mark);
    return status;
}

static lxp_result materialize_account(const lxp_genesis_account *source,
                                      lx_account *target)
{
    const char *name = source == NULL ? NULL :
        fresh_system_name(source->subaccount_kind);
    size_t length = name == NULL ? 0U : strlen(name);
    if (source == NULL || target == NULL || name == NULL ||
        length > LX_ACCOUNT_NAME_MAX)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(target, 0, sizeof(*target));
    (void)memcpy(target->id, source->account_id, 32U);
    (void)memcpy(target->name, name, length);
    target->name_length = (uint16_t)length;
    target->kind = (lx_account_kind)source->subaccount_kind;
    (void)memcpy(target->asset_id, source->asset_id, 32U);
    target->has_asset = true;
    return lx_account_validate_canonical(target);
}

static lxp_result module_value_materialize(
    const lxp_genesis_module_value *value, lxp_kernel *kernel)
{
    size_t index;
    for (index = 0U; index < kernel->module_kv_count; ++index) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[index];
        if (entry->module_id != value->module_id || entry->key_length > 32U ||
            memcmp(entry->key, value->key, entry->key_length) != 0 ||
            !lxp_ct_is_zero(value->key + entry->key_length,
                            32U - entry->key_length))
            continue;
        return entry->value_length == value->value_length &&
               memcmp(entry->value, value->value, value->value_length) == 0 ?
                   LXP_OK : LXP_FATAL_REPLAY_DIVERGENCE;
    }
    return kernel_insert(kernel, value->module_id, value->key, 32U,
                         value->value, value->value_length);
}

lxp_result lxp_genesis_parameter_version(
    const lxp_genesis_manifest *manifest, uint32_t *parameter_version)
{
    size_t index;
    bool found = false;
    uint32_t version = 0U;
    if (manifest == NULL || parameter_version == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < manifest->parameter_count; ++index) {
        const lxp_genesis_parameter *parameter = &manifest->parameters[index];
        if (parameter->module_id != LXP_MODULE_GOVERNANCE ||
            memcmp(parameter->key, parameter_version_key, 32U) != 0)
            continue;
        if (found || !lxp_ct_is_zero(parameter->value, 28U))
            return LXP_ERR_NON_CANONICAL;
        version = ((uint32_t)parameter->value[28] << 24U) |
                  ((uint32_t)parameter->value[29] << 16U) |
                  ((uint32_t)parameter->value[30] << 8U) |
                  parameter->value[31];
        found = true;
    }
    if (!found || version == 0U || version > UINT16_MAX)
        return LXP_ERR_VERSION_UNSUPPORTED;
    *parameter_version = version;
    return LXP_OK;
}

lxp_result lxp_genesis_materialize(const lxp_genesis_manifest *manifest,
                                   lxp_arena *arena, lxp_kernel *kernel)
{
    lx_account_registry *accounts;
    uint8_t commitment[32];
    uint32_t parameter_version;
    size_t index;
    lxp_result status = validate(manifest);
    if (status != LXP_OK || arena == NULL || kernel == NULL ||
        kernel->state == NULL || kernel->journal == NULL ||
        kernel->module_count != 1U ||
        kernel->modules[0].module_id != LXP_MODULE_PROGRAMS ||
        kernel->modules[0].abi_version != programs_module_registration_v4()->abi_version ||
        kernel->state->count != 0U || kernel->state->idempotency_count != 0U ||
        kernel->state->next_sequence != 1U ||
        kernel->module_kv_count != 0U || kernel->blob_count != 0U ||
        kernel->state->accounts == NULL ||
        kernel->state->accounts->count != 0U)
        return status == LXP_OK ? LXP_ERR_NON_CANONICAL : status;
    status = lxp_genesis_parameter_version(manifest, &parameter_version);
    (void)parameter_version;
    if (status == LXP_OK)
        status = lxp_state_store_require_account_root(kernel->state);
    accounts = kernel->state->accounts;
    for (index = 0U; status == LXP_OK &&
         index < manifest->account_count; ++index) {
        status = materialize_account(&manifest->accounts[index],
                                     &accounts->accounts[index]);
        if (status == LXP_OK) ++accounts->count;
    }
    for (index = 0U; status == LXP_OK &&
         index < manifest->parameter_count; ++index)
        status = kernel_insert(kernel, manifest->parameters[index].module_id,
                               manifest->parameters[index].key, 32U,
                               manifest->parameters[index].value, 32U);
    if (status == LXP_OK)
        status = lxp_programs_metering_genesis_materialize(manifest, kernel);
    if (status == LXP_OK)
        status = lxp_programs_fee_genesis_materialize(manifest, kernel);
    for (index = 0U; status == LXP_OK &&
         index < manifest->module_value_count; ++index)
        status = module_value_materialize(&manifest->module_values[index],
                                          kernel);
    if (status == LXP_OK)
        status = lxp_genesis_manifest_commitment(manifest, arena, commitment);
    if (status == LXP_OK)
        status = kernel_insert(kernel, LXP_MODULE_GOVERNANCE,
                               genesis_manifest_key,
                               sizeof(genesis_manifest_key) - 1U,
                               commitment, sizeof(commitment));
    return status;
}

lxp_result lxp_genesis_state_root(
    const lxp_genesis_manifest *manifest, lxp_arena *arena,
    uint8_t state_root[32])
{
    lxp_state_store *state;
    lxp_state_journal *journal;
    lxp_kernel *kernel;
    lx_account_registry *accounts;
    bool state_open = false;
    lxp_result status;
    if (manifest == NULL || arena == NULL || state_root == NULL)
        return LXP_ERR_NON_CANONICAL;
    state = (lxp_state_store *)malloc(sizeof(*state));
    journal = (lxp_state_journal *)calloc(1U, sizeof(*journal));
    kernel = (lxp_kernel *)malloc(sizeof(*kernel));
    accounts = (lx_account_registry *)malloc(sizeof(*accounts));
    if (state == NULL || journal == NULL || kernel == NULL || accounts == NULL) {
        free(accounts); free(kernel); free(journal); free(state);
        return LXP_ERR_IO;
    }
    status = lx_account_registry_init(accounts);
    if (status == LXP_OK) {
        status = lxp_state_store_init(state, 1U);
        state_open = status == LXP_OK;
    }
    if (status == LXP_OK) status = lxp_state_store_bind_accounts(state, accounts);
    if (status == LXP_OK)
        status = lxp_kernel_create(kernel, state, journal, manifest, 1U);
    if (status == LXP_OK)
        status = lxp_kernel_register_module(kernel,
                                            programs_module_registration_v4());
    if (status == LXP_OK) status = lxp_genesis_materialize(manifest, arena, kernel);
    if (status == LXP_OK) status = lxp_state_root(kernel, state_root);
    if (state_open) {
        lxp_result close_status = lxp_state_store_destroy(state);
        if (status == LXP_OK && close_status != LXP_OK) status = close_status;
    }
    free(accounts); free(kernel); free(journal); free(state);
    return status;
}

lxp_result lxp_genesis_fresh_empty_accounts(
    lxp_genesis_manifest *manifest, const uint8_t asset_id[32])
{
    static const uint16_t kinds[LXP_GENESIS_FRESH_SYSTEM_ACCOUNT_COUNT] = {
        LX_ACCOUNT_SYSTEM_FEES, LX_ACCOUNT_SYSTEM_PAXEER_RESERVE,
        LX_ACCOUNT_SYSTEM_PAXEER_WITHDRAWALS
    };
    size_t index;
    if (manifest == NULL || asset_id == NULL ||
        lxp_ct_is_zero(asset_id, 32U) || manifest->account_count != 0U)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < LXP_GENESIS_FRESH_SYSTEM_ACCOUNT_COUNT; ++index) {
        lxp_genesis_account *account = &manifest->accounts[index];
        const char *name = fresh_system_name(kinds[index]);
        size_t position = index;
        (void)memset(account, 0, sizeof(*account));
        account->subaccount_kind = kinds[index];
        (void)memcpy(account->asset_id, asset_id, 32U);
        if (lx_account_id_from_string((const uint8_t *)name, strlen(name),
                                      account->account_id) != LXP_OK)
            return LXP_FATAL_INVARIANT;
        while (position != 0U && memcmp(
                   manifest->accounts[position - 1U].account_id,
                   account->account_id, 32U) > 0) {
            lxp_genesis_account prior = manifest->accounts[position - 1U];
            manifest->accounts[position - 1U] = *account;
            *account = prior;
            --position;
            account = &manifest->accounts[position];
        }
    }
    manifest->account_count = LXP_GENESIS_FRESH_SYSTEM_ACCOUNT_COUNT;
    return LXP_OK;
}

lxp_result lxp_genesis_registration_encode(
    const lxp_genesis_bootstrap_registration *registration,
    uint8_t encoded[LXP_GENESIS_REGISTRATION_BYTES])
{
    size_t index;
    if (registration == NULL || encoded == NULL ||
        registration->network_id == 0U || registration->registration_index != 0U ||
        !registration->finalised || lxp_ct_is_zero(registration->settlement_anchor, 32U) ||
        lxp_ct_is_zero(registration->state_root, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(encoded, "LXGR", 4U); encoded[4] = 1U;
    encoded[5] = (uint8_t)(registration->network_id >> 24U);
    encoded[6] = (uint8_t)(registration->network_id >> 16U);
    encoded[7] = (uint8_t)(registration->network_id >> 8U);
    encoded[8] = (uint8_t)registration->network_id;
    for (index = 0U; index < 8U; ++index)
        encoded[9U + index] = (uint8_t)(registration->registration_index >>
                                        (56U - 8U * index));
    (void)memcpy(encoded + 17U, registration->settlement_anchor, 32U);
    (void)memcpy(encoded + 49U, registration->state_root, 32U);
    encoded[81] = 1U;
    return LXP_OK;
}

lxp_result lxp_genesis_registration_parse(
    const uint8_t *encoded, size_t encoded_length,
    lxp_genesis_bootstrap_registration *registration)
{
    size_t index;
    if (encoded == NULL || encoded_length != LXP_GENESIS_REGISTRATION_BYTES ||
        registration == NULL || memcmp(encoded, "LXGR", 4U) != 0 ||
        encoded[4] != 1U || encoded[81] != 1U)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(registration, 0, sizeof(*registration));
    registration->network_id = ((uint32_t)encoded[5] << 24U) |
        ((uint32_t)encoded[6] << 16U) | ((uint32_t)encoded[7] << 8U) |
        encoded[8];
    for (index = 0U; index < 8U; ++index)
        registration->registration_index =
            (registration->registration_index << 8U) | encoded[9U + index];
    (void)memcpy(registration->settlement_anchor, encoded + 17U, 32U);
    (void)memcpy(registration->state_root, encoded + 49U, 32U);
    registration->finalised = true;
    return registration->network_id != 0U &&
           registration->registration_index == 0U &&
           !lxp_ct_is_zero(registration->settlement_anchor, 32U) &&
           !lxp_ct_is_zero(registration->state_root, 32U) ?
               LXP_OK : LXP_ERR_NON_CANONICAL;
}

lxp_result lxp_genesis_receipt_state_root(
    uint32_t network_id, const uint8_t canonical_state_root[32],
    uint8_t receipt_state_root[32])
{
    uint8_t preimage[36];
    if (network_id == 0U || canonical_state_root == NULL ||
        receipt_state_root == NULL ||
        lxp_ct_is_zero(canonical_state_root, 32U))
        return LXP_ERR_NON_CANONICAL;
    preimage[0] = (uint8_t)(network_id >> 24U);
    preimage[1] = (uint8_t)(network_id >> 16U);
    preimage[2] = (uint8_t)(network_id >> 8U);
    preimage[3] = (uint8_t)network_id;
    (void)memcpy(preimage + 4U, canonical_state_root, 32U);
    return lxp_hash_domain(LXP_DOMAIN_GENESIS_RECEIPT_ROOT,
                           preimage, sizeof(preimage), receipt_state_root);
}

lxp_result lxp_genesis_verify_signature(
    const lxp_genesis_manifest *manifest, lxp_arena *arena)
{
    uint8_t state_root[32];
    uint8_t receipt_state_root[32];
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
        status = lxp_genesis_receipt_state_root(
            manifest->network_id, state_root, receipt_state_root);
    if (status == LXP_OK && lxp_ct_memcmp(
            receipt_state_root,
            manifest->genesis_receipt_state_root, 32U) != 0)
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
    const lxp_genesis_bootstrap_registration *registration,
    bool storage_empty, lxp_arena *arena, bool *activities_enabled)
{
    lxp_result status;
    if (activities_enabled == NULL) return LXP_ERR_NON_CANONICAL;
    *activities_enabled = false;
    if (!storage_empty || manifest == NULL || registration == NULL ||
        !registration->finalised || registration->registration_index != 0U ||
        registration->network_id != manifest->network_id ||
        lxp_ct_memcmp(registration->state_root,
                      manifest->genesis_receipt_state_root, 32U) != 0 ||
        lxp_ct_memcmp(registration->settlement_anchor,
                      manifest->genesis_receipt_state_root, 32U) != 0)
        return LXP_ERR_ROOT_MISMATCH;
    status = lxp_genesis_verify_signature(manifest, arena);
    if (status == LXP_OK)
        status = lxp_programs_metering_genesis_validate(manifest);
    if (status == LXP_OK)
        status = lxp_programs_fee_genesis_validate(manifest);
    if (status == LXP_OK) *activities_enabled = true;
    return status;
}

lxp_result lxp_genesis_bootstrap_verify(
    const lxp_genesis_manifest *manifest,
    const lxp_genesis_bootstrap_registration *registration,
    uint32_t configured_network_id, bool storage_empty,
    const lxp_snapshot_manifest_record *snapshot,
    const lxp_kernel *kernel, lxp_arena *arena,
    bool *activities_enabled)
{
    uint8_t projected_root[32];
    uint8_t live_root[32];
    uint8_t expected_receipt_root[32];
    lxp_result status;
    if (manifest == NULL || registration == NULL || snapshot == NULL ||
        kernel == NULL || arena == NULL || activities_enabled == NULL)
        return LXP_ERR_NON_CANONICAL;
    *activities_enabled = false;
    if (manifest->protocol_version != LXP_PROTOCOL_VERSION ||
        !lxp_network_id_matches(configured_network_id,
                                manifest->network_id) ||
        snapshot->global_sequence != 0U || !storage_empty ||
        lxp_ct_memcmp(manifest->genesis_state_root,
                      snapshot->canonical_state_root, 32U) != 0)
        return LXP_ERR_ROOT_MISMATCH;
    status = lxp_genesis_accept(manifest, registration, storage_empty,
                                arena, activities_enabled);
    if (status == LXP_OK)
        status = lxp_genesis_state_root(manifest, arena, projected_root);
    if (status == LXP_OK)
        status = lxp_genesis_receipt_state_root(
            manifest->network_id, projected_root, expected_receipt_root);
    if (status == LXP_OK) status = lxp_state_root(kernel, live_root);
    if (status == LXP_OK &&
        (lxp_ct_memcmp(projected_root,
                       snapshot->canonical_state_root, 32U) != 0 ||
         lxp_ct_memcmp(live_root,
                       snapshot->canonical_state_root, 32U) != 0 ||
         lxp_ct_memcmp(expected_receipt_root,
                       snapshot->receipt_state_root, 32U) != 0 ||
         lxp_ct_memcmp(kernel->current_state_root,
                       snapshot->receipt_state_root, 32U) != 0))
        status = LXP_ERR_ROOT_MISMATCH;
    if (status != LXP_OK) *activities_enabled = false;
    return status;
}
