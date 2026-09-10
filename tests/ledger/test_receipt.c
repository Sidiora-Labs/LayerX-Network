#define _POSIX_C_SOURCE 200809L

#include "layerx/lx_asset.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_storage.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static uint8_t arena_bytes[LXP_MAX_ACTIVITY_BYTES + 4096U];
static uint8_t log_body[LXP_MAX_ACTIVITY_BYTES];
static uint8_t unsigned_bytes[4096];
static uint8_t stripped_bytes[4096];

static int fields_match(const lxp_receipt *decoded, const lxp_receipt *origin)
{
    return decoded->protocol_version == origin->protocol_version &&
        decoded->global_sequence == origin->global_sequence &&
        decoded->result_code == origin->result_code &&
        decoded->effects.count == origin->effects.count &&
        decoded->module_id == origin->module_id &&
        decoded->module_version == origin->module_version &&
        decoded->parameter_version == origin->parameter_version &&
        decoded->operation == origin->operation &&
        decoded->from_sequence == origin->from_sequence &&
        decoded->supply_binding_version == origin->supply_binding_version &&
        decoded->timestamp == origin->timestamp &&
        !decoded->program_outcome.present &&
        lxp_u128_cmp(decoded->fee_charged, origin->fee_charged) == 0 &&
        lxp_u128_cmp(decoded->amount, origin->amount) == 0 &&
        lxp_u128_cmp(decoded->from_balance_before,
                     origin->from_balance_before) == 0 &&
        lxp_u128_cmp(decoded->from_balance_after,
                     origin->from_balance_after) == 0 &&
        lxp_u128_cmp(decoded->to_balance_before,
                     origin->to_balance_before) == 0 &&
        lxp_u128_cmp(decoded->to_balance_after,
                     origin->to_balance_after) == 0 &&
        lxp_u128_cmp(decoded->total_units_before,
                     origin->total_units_before) == 0 &&
        lxp_u128_cmp(decoded->total_units_after,
                     origin->total_units_after) == 0 &&
        memcmp(decoded->activity_id, origin->activity_id, 32U) == 0 &&
        memcmp(decoded->previous_state_root,
               origin->previous_state_root, 32U) == 0 &&
        memcmp(decoded->resulting_state_root,
               origin->resulting_state_root, 32U) == 0 &&
        memcmp(decoded->activity_root, origin->activity_root, 32U) == 0 &&
        memcmp(decoded->batch_id, origin->batch_id, 32U) == 0 &&
        memcmp(decoded->asset, origin->asset, 32U) == 0 &&
        memcmp(decoded->from, origin->from, 32U) == 0 &&
        memcmp(decoded->to, origin->to, 32U) == 0 &&
        memcmp(decoded->transfer_set_root,
               origin->transfer_set_root, 32U) == 0 &&
        memcmp(decoded->authorization_hash,
               origin->authorization_hash, 32U) == 0 &&
        memcmp(decoded->context_hash, origin->context_hash, 32U) == 0;
}

int main(void)
{
    static const uint8_t seed[32] = { 3U };
    uint8_t public_key[32];
    size_t public_length = 32U;
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                  seed, 32U);
    const lxp_module_iface *asset_module;
    lxp_ledger_receipt_input input;
    lxp_receipt receipt;
    lxp_receipt cached;
    lxp_receipt decoded;
    lxp_receipt stripped;
    lxp_arena arena;
    lxp_byte_span encoded;
    size_t mark;
    size_t unsigned_length;
    size_t stripped_length;
    char directory[] = "/tmp/lxp-ledger-receipt-XXXXXX";
    char path[128];
    lxp_log log;
    lxp_log_record_header header;
    uint64_t durable_length;

    if (key == NULL ||
        EVP_PKEY_get_raw_public_key(key, public_key, &public_length) != 1 ||
        public_length != 32U ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        mkdtemp(directory) == NULL ||
        lxp_log_segment_create(&log, directory, 0U,
                               LXP_MAX_ACTIVITY_BYTES + 4096U) != LXP_OK)
        return 1;
    EVP_PKEY_free(key);
    if (snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0)
        return 1;
    (void)memset(&input, 0, sizeof(input));
    input.transaction_id[0] = 1U;
    input.operation = 1U;
    input.global_sequence = 9U;
    input.asset[0] = 2U;
    input.amount = (lxp_u128){ 0U, 25U };
    input.from[0] = 3U;
    input.from_balance_before = (lxp_u128){ 0U, 100U };
    input.from_balance_after = (lxp_u128){ 0U, 75U };
    input.from_sequence = 7U;
    input.to[0] = 4U;
    input.to_balance_before = (lxp_u128){ 0U, 10U };
    input.to_balance_after = (lxp_u128){ 0U, 35U };
    input.transfer_set_root[0] = 5U;
    input.authorization_hash[0] = 6U;
    input.context_hash[0] = 7U;
    input.previous_state_root[0] = 8U;
    input.resulting_state_root[0] = 9U;
    input.batch_id[0] = 10U;
    input.timestamp = 1234U;
    input.leg_count = 1U;
    if (lxp_balance_writer_guard(false) != LXP_ERR_BALANCE_BYPASS ||
        lxp_balance_writer_guard(true) != LXP_OK ||
        lxp_ledger_receipt_issue(&receipt, &input, seed, &arena, &log) != LXP_OK ||
        lxp_receipt_verify(&receipt, public_key, &arena) != LXP_OK)
        return 1;
    asset_module = lx_asset_module_iface();
    if (asset_module == NULL ||
        receipt.module_id != (uint16_t)LXP_LEDGER_RECEIPT_MODULE_ID ||
        receipt.module_version !=
            (uint32_t)LXP_LEDGER_RECEIPT_MODULE_VERSION ||
        asset_module->module_id != receipt.module_id ||
        asset_module->abi_version != receipt.module_version) return 1;
    durable_length = log.write_offset;
    mark = lxp_arena_mark(&arena);
    if (lxp_receipt_encode(&receipt, true, &arena, &encoded) != LXP_OK ||
        lxp_log_read(&log, 0U, &header, log_body, sizeof(log_body)) != LXP_OK ||
        header.record_kind != LXP_LOG_RECEIPT || header.global_sequence != 9U ||
        header.body_length != encoded.length ||
        memcmp(log_body, encoded.bytes, encoded.length) != 0) return 1;
    (void)lxp_arena_reset(&arena, mark);
    if (lxp_receipt_decode(log_body, (size_t)header.body_length, true,
                           &decoded) != LXP_OK ||
        !fields_match(&decoded, &receipt) ||
        memcmp(decoded.sequencer_signature,
               receipt.sequencer_signature, 64U) != 0 ||
        lxp_receipt_verify(&decoded, public_key, &arena) != LXP_OK) return 1;
    mark = lxp_arena_mark(&arena);
    if (lxp_receipt_encode(&decoded, true, &arena, &encoded) != LXP_OK ||
        encoded.length != (size_t)header.body_length ||
        memcmp(encoded.bytes, log_body, encoded.length) != 0) return 1;
    (void)lxp_arena_reset(&arena, mark);
    mark = lxp_arena_mark(&arena);
    if (lxp_receipt_encode(&receipt, false, &arena, &encoded) != LXP_OK ||
        encoded.length > sizeof(unsigned_bytes)) return 1;
    unsigned_length = encoded.length;
    (void)memcpy(unsigned_bytes, encoded.bytes, unsigned_length);
    (void)lxp_arena_reset(&arena, mark);
    if (lxp_receipt_decode(unsigned_bytes, unsigned_length, true, &decoded) !=
            LXP_ERR_BAD_SIGNATURE ||
        lxp_receipt_decode(unsigned_bytes, unsigned_length, false,
                           &decoded) != LXP_OK ||
        !fields_match(&decoded, &receipt)) return 1;
    stripped = receipt;
    stripped.module_id = 0U;
    stripped.module_version = 0U;
    mark = lxp_arena_mark(&arena);
    if (lxp_receipt_sign(&stripped, seed, &arena) != LXP_OK ||
        lxp_receipt_encode(&stripped, true, &arena, &encoded) != LXP_OK ||
        encoded.length > sizeof(stripped_bytes)) return 1;
    stripped_length = encoded.length;
    (void)memcpy(stripped_bytes, encoded.bytes, stripped_length);
    (void)lxp_arena_reset(&arena, mark);
    if (lxp_receipt_decode(stripped_bytes, stripped_length, true, &decoded) !=
        LXP_ERR_NON_CANONICAL) return 1;
    mark = lxp_arena_mark(&arena);
    if (lxp_receipt_encode(&receipt, true, &arena, &encoded) != LXP_OK)
        return 1;
    cached = receipt;
    if (memcmp(&cached, &receipt, sizeof(receipt)) != 0) return 1;
    (void)lxp_arena_reset(&arena, mark);
    cached.sequencer_signature[0] ^= 1U;
    if (lxp_receipt_verify(&cached, public_key, &arena) != LXP_ERR_BAD_SIGNATURE)
        return 1;
    input.from_balance_after.lo = 74U;
    if (lxp_ledger_receipt_issue(&cached, &input, seed, &arena, &log) !=
            LXP_FATAL_INVARIANT || log.write_offset != durable_length)
        return 1;
    if (lxp_log_close(&log) != LXP_OK || unlink(path) != 0 ||
        rmdir(directory) != 0) return 1;
    return 0;
}
