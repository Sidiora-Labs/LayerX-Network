#include "layerx/lxp_kernel.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_receipt.h"

#include <openssl/evp.h>
#include <stdint.h>
#include <string.h>

int main(void)
{
    static const uint8_t private_key[32] = {
        1U,2U,3U,4U,5U,6U,7U,8U,9U,10U,11U,12U,13U,14U,15U,16U,
        17U,18U,19U,20U,21U,22U,23U,24U,25U,26U,27U,28U,29U,30U,31U,32U
    };
    static uint8_t arena_bytes[LXP_MAX_ACTIVITY_BYTES + 4096U];
    lxp_arena arena;
    lxp_effect_buffer effects;
    lxp_effect state_effect = { .module_id = 1U, .ordinal = 1U,
        .kind = LXP_EFFECT_STATE, .body_length = 1U, .body = { 9U } };
    lxp_effect event_effect = { .module_id = 1U, .ordinal = 2U,
        .event_type = 7U, .kind = LXP_EFFECT_EVENT, .body_length = 2U,
        .body = { 7U, 8U } };
    lxp_effect bad_money = { .module_id = 1U, .ordinal = 3U,
        .kind = LXP_EFFECT_STATE, .monetary = true };
    lxp_receipt receipt;
    uint8_t activity_id[32] = { 1U };
    uint8_t previous[32] = { 2U };
    uint8_t resulting[32] = { 3U };
    uint8_t activity_root[32] = { 4U };
    uint8_t batch_id[32] = { 5U };
    uint8_t public_key[32];
    size_t public_key_length = sizeof(public_key);
    uint8_t event_root[32];
    lxp_byte_span encoded;
    EVP_PKEY *key;
    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_effect_buffer_add(&effects, &state_effect) != LXP_OK ||
        lxp_effect_buffer_add(&effects, &event_effect) != LXP_OK ||
        lxp_effect_buffer_add(&effects, &bad_money) != LXP_FATAL_INVARIANT ||
        lxp_effect_event_root(&effects, &arena, event_root) != LXP_OK ||
        lxp_ct_is_zero(event_root, 32U)) return 1;
    if (lxp_receipt_build(&receipt, activity_id, 8U, previous, resulting,
                          activity_root, LXP_OK, &effects,
                          (lxp_u128){ 0U, 3U }, batch_id, 1U, 2U, 4U) !=
        LXP_OK || lxp_receipt_sign(&receipt, private_key, &arena) != LXP_OK)
        return 1;
    key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, private_key,
                                       sizeof(private_key));
    if (key == NULL || EVP_PKEY_get_raw_public_key(key, public_key,
                                                   &public_key_length) != 1 ||
        public_key_length != 32U) {
        EVP_PKEY_free(key);
        return 1;
    }
    EVP_PKEY_free(key);
    if (lxp_receipt_verify(&receipt, public_key, &arena) != LXP_OK ||
        lxp_receipt_encode(&receipt, true, &arena, &encoded) != LXP_OK ||
        encoded.length == 0U) return 1;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK) return 1;
    receipt.result_code = LXP_ERR_AGREEMENT_STATE;
    if (lxp_receipt_verify(&receipt, public_key, &arena) !=
        LXP_ERR_BAD_SIGNATURE) return 1;
    {
        static const uint8_t unchanged_supply[] = { 2U, 3U, 4U, 5U, 6U, 7U, 8U };
        lxp_receipt supply;
        size_t index;
        (void)memset(&supply, 0, sizeof(supply));
        supply.module_id = LXP_MODULE_ASSET;
        supply.result_code = LXP_OK;
        supply.supply_binding_version = 1U;
        supply.asset[0] = 6U;
        supply.total_units_before = (lxp_u128){ 0U, 41U };
        for (index = 0U; index < sizeof(unchanged_supply) /
             sizeof(unchanged_supply[0]); ++index) {
            supply.operation = unchanged_supply[index];
            supply.total_units_after = supply.total_units_before;
            if (lxp_receipt_validate_supply(&supply) != LXP_OK) return 1;
            supply.total_units_after = (lxp_u128){ 0U, 42U };
            if (lxp_receipt_validate_supply(&supply) !=
                LXP_FATAL_SUPPLY_MISMATCH) return 1;
        }
        supply.operation = 9U;
        supply.total_units_after = supply.total_units_before;
        if (lxp_receipt_validate_supply(&supply) != LXP_ERR_NON_CANONICAL)
            return 1;
        supply.operation = 2U;
        supply.module_id = LXP_MODULE_PROGRAMS;
        if (lxp_receipt_validate_supply(&supply) != LXP_ERR_NON_CANONICAL)
            return 1;
    }
    return 0;
}
