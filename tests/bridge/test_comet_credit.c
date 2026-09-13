#define _POSIX_C_SOURCE 200809L
#include "layerx/lxp_bridge_credit.h"
#include "layerx/lxp_crypto.h"
#include "files.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <string.h>

#define CHECK(expression) do { if (!(expression)) { \
    (void)fprintf(stderr, "Comet credit check failed at line %d: %s\n", __LINE__, #expression); \
    return 1; } } while (0)

static int sign_credit(lxp_bridge_credit *credit, const uint8_t public[32], bool legacy)
{
    static const uint8_t domain[] = "LX:CUSTODY:CREDIT:v2";
    static const uint8_t old_domain[] = "LX:CUSTODY:CREDIT:v1";
    uint8_t seed[32];
    uint8_t derived[32];
    uint8_t message[sizeof(domain) - 1U + LXP_BRIDGE_CREDIT_SIGNED_BYTES];
    size_t public_length = sizeof(derived);
    size_t signature_length = 64U;
    (void)memset(seed, 0x55, sizeof(seed));
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, seed, sizeof(seed));
    EVP_MD_CTX *context = EVP_MD_CTX_new();
    (void)memcpy(message, legacy ? old_domain : domain, sizeof(domain) - 1U);
    (void)memcpy(message + sizeof(domain) - 1U, credit->bytes, LXP_BRIDGE_CREDIT_SIGNED_BYTES);
    int ok = key != NULL && context != NULL &&
        EVP_PKEY_get_raw_public_key(key, derived, &public_length) == 1 && public_length == 32U &&
        memcmp(derived, public, 32U) == 0 &&
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(context, credit->bytes + LXP_BRIDGE_CREDIT_SIGNED_BYTES,
                       &signature_length, message, sizeof(message)) == 1 && signature_length == 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

int main(int argc, char **argv)
{
    uint8_t *profile_bytes = NULL;
    uint8_t *credit_bytes = NULL;
    size_t profile_length = 0U;
    size_t credit_length = 0U;
    lxp_bridge_profile profile;
    lxp_bridge_profile changed_profile;
    lxp_bridge_credit credit;
    lxp_bridge_credit changed;
    uint8_t nullifier[32];
    uint32_t network;
    if (argc != 3) {
        (void)fprintf(stderr, "usage: test-comet-credit custody.profile custody.credit\n");
        return 2;
    }
    CHECK(read_file(argv[1], sizeof(profile.bytes), false, &profile_bytes, &profile_length) == 0);
    CHECK(read_file(argv[2], sizeof(credit.bytes), false, &credit_bytes, &credit_length) == 0);
    CHECK(profile_length == sizeof(profile.bytes) && credit_length == sizeof(credit.bytes));
    (void)memcpy(profile.bytes, profile_bytes, profile_length);
    (void)memcpy(credit.bytes, credit_bytes, credit_length);
    free(profile_bytes);
    free(credit_bytes);
    CHECK(memcmp(profile.bytes, "LXBC2", 5U) == 0 && memcmp(credit.bytes, "LXDC2", 5U) == 0);
    network = ((uint32_t)profile.bytes[201] << 24U) | ((uint32_t)profile.bytes[202] << 16U) |
              ((uint32_t)profile.bytes[203] << 8U) | profile.bytes[204];
    CHECK(lxp_bridge_profile_validate(&profile) == LXP_OK);
    CHECK(lxp_bridge_credit_verify(&profile, &credit, network, 3U, nullifier) == LXP_OK);
    CHECK(!lxp_ct_is_zero(nullifier, sizeof(nullifier)));
    for (size_t index = 0U; index < sizeof(credit.bytes); ++index) {
        changed = credit;
        changed.bytes[index] ^= 1U;
        CHECK(lxp_bridge_credit_verify(&profile, &changed, network, 3U, nullifier) != LXP_OK);
    }
    for (size_t index = 0U; index < sizeof(profile.bytes); ++index) {
        changed_profile = profile;
        changed_profile.bytes[index] ^= 1U;
        CHECK(lxp_bridge_credit_verify(&changed_profile, &credit, network, 3U, nullifier) != LXP_OK);
    }
    const size_t signed_offsets[] = {4U, 215U, 287U, 359U, 362U};
    for (size_t index = 0U; index < sizeof(signed_offsets) / sizeof(signed_offsets[0]); ++index) {
        changed = credit;
        changed.bytes[signed_offsets[index]] ^= 1U;
        CHECK(sign_credit(&changed, profile.bytes + 65U, false) == 0);
        CHECK(lxp_bridge_credit_verify(&profile, &changed, network, 3U, nullifier) != LXP_OK);
    }
    changed = credit;
    CHECK(sign_credit(&changed, profile.bytes + 65U, true) == 0);
    CHECK(lxp_bridge_credit_verify(&profile, &changed, network, 3U, nullifier) != LXP_OK);
    CHECK(lxp_bridge_credit_verify(&profile, &credit, network ^ 1U, 3U, nullifier) != LXP_OK);
    CHECK(lxp_bridge_credit_verify(&profile, &credit, network, 2U, nullifier) != LXP_OK);
    (void)puts("real Comet custody credit preserves typed evidence, signatures and refusal bindings");
    return 0;
}
