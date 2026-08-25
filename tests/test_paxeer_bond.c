#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_paxeer.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/obj_mac.h>
#include <stdint.h>
#include <string.h>

static int key_pair(uint8_t value, uint8_t private_key[32],
                    uint8_t public_key[33])
{
    EC_KEY *key = EC_KEY_new_by_curve_name(NID_secp256k1);
    BIGNUM *private_value = BN_new();
    const EC_GROUP *group = key == NULL ? NULL : EC_KEY_get0_group(key);
    EC_POINT *point = group == NULL ? NULL : EC_POINT_new(group);
    size_t length = 0U;
    (void)memset(private_key, 0, 32U);
    private_key[31] = value;
    if (key != NULL && private_value != NULL && point != NULL &&
        BN_bin2bn(private_key, 32, private_value) != NULL &&
        EC_POINT_mul(group, point, private_value, NULL, NULL, NULL) == 1 &&
        EC_KEY_set_private_key(key, private_value) == 1 &&
        EC_KEY_set_public_key(key, point) == 1)
        length = EC_POINT_point2oct(group, point, POINT_CONVERSION_COMPRESSED,
                                    public_key, 33U, NULL);
    EC_POINT_free(point);
    BN_free(private_value);
    EC_KEY_free(key);
    return length == 33U ? 0 : 1;
}

int main(void)
{
    uint8_t arena_storage[262144];
    uint8_t private_key[32];
    uint8_t public_key[33];
    uint8_t other_id[32] = {8U};
    uint8_t other_key[33] = {2U};
    uint8_t paxeer_contract[20] = {0xa1U};
    uint8_t corrupted[1024];
    lxp_arena arena;
    lxp_checkpoint_certificate first_checkpoint;
    lxp_checkpoint_certificate second_checkpoint;
    lxp_checkpoint_certificate foreign_first_checkpoint;
    lxp_checkpoint_certificate foreign_second_checkpoint;
    lxp_guarantor_ctx guarantor;
    lxp_guarantor_ctx foreign_guarantor;
    lxp_guarantor_attestation first;
    lxp_guarantor_attestation second;
    lxp_guarantor_attestation foreign_first;
    lxp_guarantor_attestation foreign_second;
    lxp_equivocation_evidence evidence;
    lxp_equivocation_evidence foreign_evidence;
    lxp_byte_span encoded;
    lxp_byte_span foreign_encoded;
    lxp_paxeer_bond_state bonds;
    lxp_guarantor_bond_state view;
    bool eligible = false;
    if (lxp_arena_init(&arena, arena_storage, sizeof(arena_storage)) != LXP_OK ||
        key_pair(7U, private_key, public_key) != 0 ||
        lxp_paxeer_bond_init(&bonds, 1U, 42U, 31337U, paxeer_contract,
                             (lxp_u128){0U, 10000U}, 100U) != LXP_OK)
        return 1;
    (void)memset(&guarantor, 0, sizeof(guarantor));
    guarantor.guarantor_id[0] = 7U;
    (void)memcpy(guarantor.paxeer_private_key, private_key, 32U);
    (void)memcpy(guarantor.paxeer_public_key, public_key, 33U);
    guarantor.ready_to_sign = true;
    guarantor.possesses_availability = true;
    guarantor.bond_view.bonded = true;
    guarantor.protocol_version = 1U;
    guarantor.network_id = 42U;
    guarantor.paxeer_chain_id = 31337U;
    guarantor.paxeer_settlement_contract[0] = 0xa1U;
    (void)memset(&first_checkpoint, 0, sizeof(first_checkpoint));
    first_checkpoint.header.protocol_version = 1U;
    first_checkpoint.header.network_id = 42U;
    first_checkpoint.header.epoch = 4U;
    first_checkpoint.header.batch_number = 12U;
    first_checkpoint.header.resulting_state_root[0] = 1U;
    second_checkpoint = first_checkpoint;
    second_checkpoint.header.resulting_state_root[0] = 2U;
    if (lxp_guarantor_attest(&guarantor, &first_checkpoint, true, true, 100U,
                             &arena, &first) != LXP_OK ||
        lxp_guarantor_attest(&guarantor, &second_checkpoint, true, true, 101U,
                             &arena, &second) != LXP_OK ||
        lxp_equivocation_detect(LXP_EQUIVOCATION_GUARANTOR, &first, &second,
                                public_key, 33U, &evidence) != LXP_OK ||
        lxp_equivocation_encode(&evidence, &arena, &encoded) != LXP_OK ||
        lxp_paxeer_bond_deposit(&bonds, guarantor.guarantor_id, public_key,
                                (lxp_u128){0U, 100U}, 1U) != LXP_OK ||
        lxp_paxeer_bond_deposit(&bonds, other_id, other_key,
                                (lxp_u128){0U, 99U}, 1U) != LXP_OK ||
        lxp_paxeer_bond_state_read(&bonds, other_id, &view, &eligible) !=
            LXP_OK || eligible)
        return 1;
    foreign_first_checkpoint = first_checkpoint;
    foreign_first_checkpoint.header.network_id = 43U;
    foreign_second_checkpoint = second_checkpoint;
    foreign_second_checkpoint.header.network_id = 43U;
    foreign_guarantor = guarantor;
    foreign_guarantor.network_id = 43U;
    if (lxp_guarantor_attest(&foreign_guarantor, &foreign_first_checkpoint,
                             true, true, 102U, &arena,
                             &foreign_first) != LXP_OK ||
        lxp_guarantor_attest(&foreign_guarantor, &foreign_second_checkpoint,
                             true, true, 103U, &arena,
                             &foreign_second) != LXP_OK ||
        lxp_equivocation_detect(LXP_EQUIVOCATION_GUARANTOR,
                                &foreign_first, &foreign_second,
                                public_key, 33U, &foreign_evidence) != LXP_OK ||
        lxp_equivocation_encode(&foreign_evidence, &arena,
                                &foreign_encoded) != LXP_OK ||
        lxp_paxeer_slash_submit(&bonds, foreign_encoded.bytes,
                                foreign_encoded.length, &foreign_evidence,
                                &arena) != LXP_ERR_AUTH_SCOPE)
        return 1;
    if (encoded.length > sizeof(corrupted)) return 1;
    (void)memcpy(corrupted, encoded.bytes, encoded.length);
    corrupted[0] ^= 1U;
    if (lxp_paxeer_slash_submit(&bonds, corrupted, encoded.length,
                                &evidence, &arena) !=
            LXP_ERR_NON_CANONICAL)
        return 1;
    if (lxp_paxeer_slash_submit(&bonds, encoded.bytes, encoded.length,
                                &evidence, &arena) != LXP_OK ||
        lxp_paxeer_bond_state_read(&bonds, guarantor.guarantor_id,
                                   &view, &eligible) != LXP_OK ||
        eligible || view.active || !view.jailed ||
        !lxp_u128_is_zero(view.bond_amount) || view.removed_epoch != 0U ||
        lxp_paxeer_jail_guarantor(&bonds, other_id, 5U) != LXP_OK ||
        lxp_paxeer_bond_state_read(&bonds, other_id, &view, &eligible) !=
            LXP_OK || eligible || !view.jailed)
        return 1;
    return 0;
}
