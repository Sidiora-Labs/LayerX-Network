#define _POSIX_C_SOURCE 200809L
#include "layerx/lxp_bridge_credit.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_ledger.h"
#include "files.h"

#include <errno.h>
#include <openssl/evp.h>
#include <stdio.h>
#include <string.h>

static int number(const char *text, uint64_t *value)
{
    char *end;
    unsigned long long parsed;
    if (*text < '0' || *text > '9') return 1;
    errno = 0;
    parsed = strtoull(text, &end, 10);
    if (errno != 0 || *end != '\0' || parsed > UINT64_MAX) return 1;
    *value = (uint64_t)parsed;
    return 0;
}

int main(int argc, char **argv)
{
    uint8_t *profile_bytes = NULL;
    uint8_t *credit_bytes = NULL;
    uint8_t *seed = NULL;
    uint8_t *arena_bytes = NULL;
    size_t profile_length = 0U;
    size_t credit_length = 0U;
    size_t seed_length = 0U;
    uint8_t public_key[32];
    uint8_t signature[64];
    uint8_t preimage[32];
    uint8_t name[LX_ACCOUNT_NAME_MAX];
    uint8_t beneficiary[32];
    size_t name_length;
    lxp_bridge_profile profile;
    lxp_bridge_credit credit;
    lxp_activity activity = {0};
    lxp_arena arena;
    lxp_byte_span encoded;
    EVP_PKEY *key = NULL;
    EVP_MD_CTX *context = NULL;
    size_t public_length = 32U;
    size_t signature_length = 64U;
    int result = 1;
    int output = -1;
    if (argc != 8) {
        (void)fprintf(stderr, "usage: sign-credit profile credit actor-did actor-key sequence timestamp-ms output\n");
        return 2;
    }
    if (read_file(argv[1], sizeof(profile.bytes), false, &profile_bytes, &profile_length) ||
        profile_length != sizeof(profile.bytes) ||
        read_file(argv[2], sizeof(credit.bytes), false, &credit_bytes, &credit_length) ||
        credit_length != sizeof(credit.bytes) ||
        read_file(argv[4], 32U, true, &seed, &seed_length) || seed_length != 32U)
        goto done;
    (void)memcpy(profile.bytes, profile_bytes, sizeof(profile.bytes));
    (void)memcpy(credit.bytes, credit_bytes, sizeof(credit.bytes));
    key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, seed, 32U);
    context = EVP_MD_CTX_new();
    if (key == NULL || context == NULL ||
        EVP_PKEY_get_raw_public_key(key, public_key, &public_length) != 1 || public_length != 32U ||
        memcmp(public_key, credit.bytes + 139U, 32U) != 0)
        goto done;
    activity.protocol_version = 3U;
    activity.network_id = ((uint32_t)credit.bytes[37] << 24U) |
        ((uint32_t)credit.bytes[38] << 16U) | ((uint32_t)credit.bytes[39] << 8U) | credit.bytes[40];
    activity.activity_type = LXP_BRIDGE_CREDIT;
    activity.actor_did = (lxp_byte_span){(const uint8_t *)argv[3], strlen(argv[3])};
    activity.authority = (lxp_byte_span){public_key, sizeof(public_key)};
    activity.payload = (lxp_byte_span){credit.bytes, sizeof(credit.bytes)};
    activity.signature = (lxp_byte_span){signature, sizeof(signature)};
    if (activity.actor_did.length == 0U || activity.actor_did.length > sizeof(name) - 11U)
        goto done;
    (void)memcpy(name, "agent:", 6U);
    (void)memcpy(name + 6U, activity.actor_did.bytes, activity.actor_did.length);
    name_length = 6U + activity.actor_did.length;
    (void)memcpy(name + name_length, ":main", 5U);
    name_length += 5U;
    if (lx_account_id_from_string(name, name_length, beneficiary) != LXP_OK ||
        memcmp(beneficiary, credit.bytes + 107U, 32U) != 0)
        goto done;
    if (number(argv[5], &activity.account_sequence) ||
        number(argv[6], &activity.timestamp_bound.not_before) ||
        activity.timestamp_bound.not_before > UINT64_MAX - 300000U)
        goto done;
    activity.timestamp_bound.not_after = activity.timestamp_bound.not_before + 300000U;
    if (lxp_bridge_credit_verify(&profile, &credit, activity.network_id, 3U, activity.idempotency_key) != LXP_OK ||
        lxp_hash_payload(credit.bytes, sizeof(credit.bytes), activity.payload_hash) != LXP_OK ||
        lxp_activity_signing_preimage(&activity, preimage) != LXP_OK ||
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) != 1 ||
        EVP_DigestSign(context, signature, &signature_length, preimage, 32U) != 1 || signature_length != 64U ||
        lxp_activity_verify_signature(&activity) != LXP_OK)
        goto done;
    arena_bytes = malloc(LXP_MAX_ACTIVITY_BYTES);
    if (arena_bytes == NULL || lxp_arena_init(&arena, arena_bytes, LXP_MAX_ACTIVITY_BYTES) != LXP_OK ||
        lxp_activity_encode(&activity, &arena, &encoded) != LXP_OK)
        goto done;
    output = open(argv[7], O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, 0600);
    if (output < 0) goto done;
    {
        size_t offset = 0U;
        while (offset < encoded.length) {
            ssize_t count = write(output, encoded.bytes + offset, encoded.length - offset);
            if (count <= 0) goto done;
            offset += (size_t)count;
        }
    }
    if (fsync(output) != 0) goto done;
    result = 0;
done:
    if (output >= 0 && close(output) != 0) result = 1;
    if (output >= 0 && result != 0) (void)unlink(argv[7]);
    if (seed != NULL) lxp_secure_zero(seed, seed_length);
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    free(seed);
    free(profile_bytes);
    free(credit_bytes);
    free(arena_bytes);
    return result;
}
