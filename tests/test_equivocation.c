#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_guarantor.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/evp.h>
#include <openssl/obj_mac.h>
#include <stdint.h>
#include <string.h>

static int secp_key(uint8_t value, uint8_t private_key[32],
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

static int ed_public(const uint8_t private_key[32], uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                  private_key, 32U);
    size_t length = 32U;
    int ok = key != NULL && EVP_PKEY_get_raw_public_key(
        key, public_key, &length) == 1 && length == 32U;
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

int main(void)
{
    uint8_t arena_one_storage[131072];
    uint8_t arena_two_storage[131072];
    uint8_t sequencer_private[32] = {9U};
    uint8_t sequencer_public[32];
    lxp_arena arena_one;
    lxp_arena arena_two;
    lxp_checkpoint_certificate first_checkpoint;
    lxp_checkpoint_certificate second_checkpoint;
    lxp_checkpoint_certificate foreign_checkpoint;
    lxp_guarantor_ctx guarantor;
    lxp_guarantor_ctx foreign_guarantor;
    uint8_t rotated_private_key[32];
    uint8_t rotated_public_key[33];
    lxp_guarantor_attestation first;
    lxp_guarantor_attestation second;
    lxp_guarantor_attestation foreign;
    lxp_equivocation_evidence evidence;
    lxp_equivocation_evidence second_node;
    lxp_byte_span first_encoding;
    lxp_byte_span second_encoding;
    lxp_guarantor_set set;
    lxp_guarantor_set unauthorized_set;
    lxp_guarantor_bond_state bond;
    lxp_sequencer_authorization authorization;
    lxp_sealed_header_record first_header;
    lxp_sealed_header_record second_header;
    lxp_sealed_header_record foreign_header;
    lxp_equivocation_evidence sequencer_evidence;
    size_t invalid_mark;
    if (lxp_arena_init(&arena_one, arena_one_storage,
                       sizeof(arena_one_storage)) != LXP_OK ||
        lxp_arena_init(&arena_two, arena_two_storage,
                       sizeof(arena_two_storage)) != LXP_OK)
        return 1;
    (void)memset(&first_checkpoint, 0, sizeof(first_checkpoint));
    first_checkpoint.header.protocol_version = 1U;
    first_checkpoint.header.network_id = 9U;
    first_checkpoint.header.epoch = 4U;
    first_checkpoint.header.batch_number = 12U;
    first_checkpoint.header.resulting_state_root[0] = 1U;
    second_checkpoint = first_checkpoint;
    second_checkpoint.header.resulting_state_root[0] = 2U;
    (void)memset(&guarantor, 0, sizeof(guarantor));
    guarantor.guarantor_id[0] = 7U;
    guarantor.ready_to_sign = true;
    guarantor.possesses_availability = true;
    guarantor.bond_view.bonded = true;
    guarantor.protocol_version = 1U;
    guarantor.network_id = 9U;
    guarantor.paxeer_chain_id = 31337U;
    guarantor.paxeer_settlement_contract[0] = 0xa1U;
    if (secp_key(7U, guarantor.paxeer_private_key,
                 guarantor.paxeer_public_key) != 0 ||
        lxp_guarantor_attest(&guarantor, &first_checkpoint, true, true, 100U,
                             &arena_one, &first) != LXP_OK ||
        lxp_guarantor_attest(&guarantor, &second_checkpoint, true, true, 101U,
                             &arena_one, &second) != LXP_OK ||
        lxp_equivocation_detect(LXP_EQUIVOCATION_GUARANTOR, &first, &second,
                                guarantor.paxeer_public_key, 33U,
                                &evidence) != LXP_OK ||
        lxp_equivocation_verify(&evidence, &arena_one) != LXP_OK ||
        lxp_equivocation_encode(&evidence, &arena_one,
                                &first_encoding) != LXP_OK)
        return 1;
    invalid_mark = lxp_arena_mark(&arena_one);
    second_node = evidence;
    second_node.kind = (lxp_equivocation_kind)UINT8_MAX;
    second_node.offender_public_key_length = SIZE_MAX;
    if (lxp_equivocation_encode(&second_node, &arena_one,
                                &second_encoding) != LXP_ERR_NON_CANONICAL ||
        lxp_arena_mark(&arena_one) != invalid_mark)
        return 1;
    second_node = evidence;
    second_node.offender_public_key_length = 32U;
    if (lxp_equivocation_encode(&second_node, &arena_one,
                                &second_encoding) != LXP_ERR_NON_CANONICAL ||
        lxp_arena_mark(&arena_one) != invalid_mark)
        return 1;
    foreign_checkpoint = second_checkpoint;
    foreign_checkpoint.header.network_id = 10U;
    if (lxp_guarantor_attest(&guarantor, &foreign_checkpoint, true, true,
                             102U, &arena_one, &foreign) !=
            LXP_ERR_ATTESTATION_THRESHOLD)
        return 1;
    foreign_guarantor = guarantor;
    foreign_guarantor.network_id = 10U;
    if (lxp_guarantor_attest(&foreign_guarantor, &foreign_checkpoint,
                             true, true, 102U, &arena_one, &foreign) != LXP_OK ||
        lxp_equivocation_detect(LXP_EQUIVOCATION_GUARANTOR, &first, &foreign,
                                guarantor.paxeer_public_key, 33U,
                                &second_node) != LXP_ERR_NON_CANONICAL)
        return 1;
    foreign_checkpoint = second_checkpoint;
    foreign_checkpoint.header.batch_number = 13U;
    if (lxp_guarantor_attest(&guarantor, &foreign_checkpoint, true, true,
                             103U, &arena_one, &foreign) != LXP_OK ||
        lxp_equivocation_detect(LXP_EQUIVOCATION_GUARANTOR, &first, &foreign,
                                guarantor.paxeer_public_key, 33U,
                                &second_node) != LXP_ERR_NON_CANONICAL)
        return 1;
    foreign_checkpoint = second_checkpoint;
    foreign_checkpoint.header.epoch = 5U;
    if (lxp_guarantor_attest(&guarantor, &foreign_checkpoint, true, true,
                             104U, &arena_one, &foreign) != LXP_OK ||
        lxp_equivocation_detect(LXP_EQUIVOCATION_GUARANTOR, &first, &foreign,
                                guarantor.paxeer_public_key, 33U,
                                &second_node) != LXP_ERR_NON_CANONICAL)
        return 1;
    foreign_guarantor = guarantor;
    foreign_guarantor.paxeer_chain_id = 31338U;
    if (lxp_guarantor_attest(&foreign_guarantor, &second_checkpoint,
                             true, true, 105U, &arena_one, &foreign) != LXP_OK ||
        lxp_equivocation_detect(LXP_EQUIVOCATION_GUARANTOR, &first, &foreign,
                                guarantor.paxeer_public_key, 33U,
                                &second_node) != LXP_ERR_NON_CANONICAL)
        return 1;
    foreign_guarantor = guarantor;
    foreign_guarantor.paxeer_settlement_contract[0] = 0xb2U;
    if (lxp_guarantor_attest(&foreign_guarantor, &second_checkpoint,
                             true, true, 106U, &arena_one, &foreign) != LXP_OK ||
        lxp_equivocation_detect(LXP_EQUIVOCATION_GUARANTOR, &first, &foreign,
                                guarantor.paxeer_public_key, 33U,
                                &second_node) != LXP_ERR_NON_CANONICAL)
        return 1;
    second_node = evidence;
    if (lxp_equivocation_verify(&second_node, &arena_two) != LXP_OK ||
        lxp_equivocation_encode(&second_node, &arena_two,
                                &second_encoding) != LXP_OK ||
        first_encoding.length != second_encoding.length ||
        memcmp(first_encoding.bytes, second_encoding.bytes,
               first_encoding.length) != 0 ||
        lxp_equivocation_detect(LXP_EQUIVOCATION_GUARANTOR, &first, &first,
                                guarantor.paxeer_public_key, 33U,
                                &second_node) != LXP_ERR_NON_CANONICAL)
        return 1;
    if (lxp_guarantor_set_init(&set) != LXP_OK) return 1;
    (void)memset(&bond, 0, sizeof(bond));
    (void)memcpy(bond.guarantor_id, guarantor.guarantor_id, 32U);
    (void)memcpy(bond.public_key, guarantor.paxeer_public_key, 33U);
    bond.bond_amount = (lxp_u128){0U, 100U};
    bond.joined_epoch = 1U;
    bond.active = true;
    if (lxp_guarantor_set_apply(&set, 1U, true, &bond) != LXP_OK)
        return 1;
    unauthorized_set = set;
    unauthorized_set.records[0].joined_epoch = 5U;
    unauthorized_set.records[0].signer_authorizations[0].active_from_epoch = 5U;
    if (lxp_slashing_submit(&evidence, &unauthorized_set,
                            &arena_one) != LXP_ERR_AUTH_SCOPE ||
        secp_key(8U, rotated_private_key, rotated_public_key) != 0 ||
        lxp_guarantor_set_rotate_signer(
            &set, 2U, true, guarantor.guarantor_id,
            rotated_public_key, 5U) != LXP_OK ||
        set.records[0].signer_authorization_count != 2U ||
        memcmp(set.records[0].public_key, rotated_public_key, 33U) != 0 ||
        lxp_slashing_submit(&evidence, &set, &arena_one) != LXP_OK ||
        set.records[0].active || !set.records[0].jailed ||
        !lxp_u128_is_zero(set.records[0].bond_amount) ||
        set.records[0].removed_epoch != 0U ||
        set.records[0].ejected_at_version != 3U || set.version != 3U ||
        lxp_slashing_submit(&evidence, &set, &arena_one) !=
            LXP_ERR_AUTH_SCOPE || set.version != 3U)
        return 1;

    (void)memset(&authorization, 0, sizeof(authorization));
    if (ed_public(sequencer_private, sequencer_public) != 0) return 1;
    (void)memcpy(authorization.public_key, sequencer_public, 32U);
    (void)memcpy(authorization.sequencer_id, sequencer_public, 32U);
    authorization.first_batch_number = 15U;
    authorization.last_batch_number = 15U;
    authorization.authorized = 1U;
    (void)memset(&first_header, 0, sizeof(first_header));
    first_header.header.protocol_version = 1U;
    first_header.header.network_id = 9U;
    first_header.header.epoch = 3U;
    first_header.header.batch_number = 15U;
    (void)memcpy(first_header.header.sequencer_id, sequencer_public, 32U);
    second_header.header = first_header.header;
    second_header.header.resulting_state_root[0] = 8U;
    foreign_header.header = second_header.header;
    foreign_header.header.network_id = 10U;
    if (lxp_batch_header_hash(&first_header.header, &arena_one,
                              first_header.header_hash) != LXP_OK ||
        lxp_batch_header_hash(&second_header.header, &arena_one,
                              second_header.header_hash) != LXP_OK ||
        lxp_batch_sign(&first_header.header, sequencer_private, &authorization,
                       first_header.signature, &arena_one) != LXP_OK ||
        lxp_batch_sign(&second_header.header, sequencer_private, &authorization,
                       second_header.signature, &arena_one) != LXP_OK ||
        lxp_batch_header_hash(&foreign_header.header, &arena_one,
                              foreign_header.header_hash) != LXP_OK ||
        lxp_batch_sign(&foreign_header.header, sequencer_private,
                       &authorization, foreign_header.signature,
                       &arena_one) != LXP_OK ||
        lxp_equivocation_detect(LXP_EQUIVOCATION_SEQUENCER, &first_header,
                                &foreign_header, sequencer_public, 32U,
                                &sequencer_evidence) != LXP_ERR_NON_CANONICAL ||
        lxp_equivocation_detect(LXP_EQUIVOCATION_SEQUENCER, &first_header,
                                &second_header, sequencer_public, 32U,
                                &sequencer_evidence) != LXP_OK ||
        lxp_equivocation_verify(&sequencer_evidence, &arena_one) != LXP_OK)
        return 1;
    invalid_mark = lxp_arena_mark(&arena_one);
    second_node = sequencer_evidence;
    second_node.offender_public_key_length = 33U;
    if (lxp_equivocation_encode(&second_node, &arena_one,
                                &second_encoding) != LXP_ERR_NON_CANONICAL ||
        lxp_arena_mark(&arena_one) != invalid_mark)
        return 1;
    sequencer_evidence.sequencer_second.signature[0] ^= 1U;
    return lxp_equivocation_verify(&sequencer_evidence, &arena_one) ==
           LXP_ERR_BAD_SIGNATURE ? 0 : 1;
}
