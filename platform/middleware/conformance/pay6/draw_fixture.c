#include "layerx/lxp_activity.h"
#include "layerx/lxp_transfer.h"
#include <openssl/evp.h>
#include <stdio.h>
#include <string.h>

int main(int argc, char **argv)
{
    uint8_t payload[733], storage[LXP_MAX_ACTIVITY_BYTES], public_key[32], signature[64], digest[32];
    const uint8_t seed[32] = { 2U };
    const uint8_t actor[] = "did:lxp:pay6-receiver";
    lxp_receive receive;
    lxp_activity activity = {0};
    lxp_arena arena;
    lxp_byte_span encoded;
    size_t key_length = 32U, signature_length = 64U, i;
    FILE *input;
    EVP_PKEY *key;
    EVP_MD_CTX *context;
    if (argc != 2 || (input = fopen(argv[1], "r")) == NULL) return 1;
    for (i = 0U; i < sizeof(payload); ++i) {
        unsigned int value;
        if (fscanf(input, "%2x", &value) != 1) return 1;
        payload[i] = (uint8_t)value;
    }
    if (fclose(input) != 0 || lxp_receive_decode(payload, sizeof(payload), &receive) != LXP_OK) return 1;
    key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, seed, sizeof(seed));
    context = EVP_MD_CTX_new();
    if (key == NULL || context == NULL || EVP_PKEY_get_raw_public_key(key, public_key, &key_length) != 1) return 1;
    activity.protocol_version = LXP_PROTOCOL_VERSION;
    activity.network_id = 7U;
    activity.activity_type = UINT32_C(0x00010006);
    activity.actor_did = (lxp_byte_span){ actor, sizeof(actor) - 1U };
    activity.authority = (lxp_byte_span){ public_key, sizeof(public_key) };
    activity.account_sequence = 12U;
    activity.timestamp_bound = (lxp_timestamp_bound){ 1U, UINT64_MAX };
    memcpy(activity.idempotency_key, receive.idempotency_key, 32U);
    activity.fee_limit = (lxp_u128){ 0U, 1000U };
    activity.payload = (lxp_byte_span){ payload, sizeof(payload) };
    activity.signature = (lxp_byte_span){ signature, sizeof(signature) };
    if (lxp_hash_payload(payload, sizeof(payload), activity.payload_hash) != LXP_OK ||
        lxp_activity_signing_preimage(&activity, digest) != LXP_OK ||
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) != 1 ||
        EVP_DigestSign(context, signature, &signature_length, digest, sizeof(digest)) != 1 || signature_length != 64U ||
        lxp_activity_verify_signature(&activity) != LXP_OK ||
        lxp_arena_init(&arena, storage, sizeof(storage)) != LXP_OK ||
        lxp_activity_encode(&activity, &arena, &encoded) != LXP_OK) return 1;
    for (i = 0U; i < encoded.length; ++i) printf("%02x", encoded.bytes[i]);
    putchar('\n');
    if (lxp_activity_id(encoded.bytes, encoded.length, digest) != LXP_OK) return 1;
    for (i = 0U; i < sizeof(digest); ++i) printf("%02x", digest[i]);
    putchar('\n');
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return 0;
}
