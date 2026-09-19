#include "lxp_daemon_lni_head_attestation.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define CHECK(x) do { if (!(x)) { fprintf(stderr, "LNI head attestation line %d\n", __LINE__); return 1; } } while (0)

static void print_hex_field(const char *name, const uint8_t *bytes, size_t length, bool final)
{
    printf("\"%s\":\"0x", name);
    for (size_t i = 0U; i < length; ++i) printf("%02x", bytes[i]);
    printf("\"%s", final ? "" : ",");
}

static void print_u64_field(const char *name, uint64_t value, bool final)
{
    printf("\"%s\":\"%llu\"%s", name, (unsigned long long)value, final ? "" : ",");
}

static int derive_public_key(const uint8_t private_key[32], uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, private_key, 32U);
    size_t length = 32U;
    int derived = key != NULL &&
        EVP_PKEY_get_raw_public_key(key, public_key, &length) == 1 && length == 32U;
    EVP_PKEY_free(key);
    return derived ? 0 : 1;
}

static lxp_daemon_head_attestation fixture_attestation(void)
{
    lxp_daemon_head_attestation attestation;
    memset(&attestation, 0, sizeof(attestation));
    memset(attestation.program_id, 0x11, 32U);
    attestation.version = 3U;
    memset(attestation.code_hash, 0x22, 32U);
    attestation.abi_version = 2U;
    attestation.observed_sequence = 4242U;
    attestation.observed_at = 1758200000000ULL;
    attestation.valid_through = 1758200000000ULL + 60000ULL;
    memset(attestation.state_root, 0x33, 32U);
    memset(attestation.head_receipt_digest, 0x44, 32U);
    return attestation;
}

static int independent_digest(const lxp_daemon_head_attestation *attestation, uint8_t digest[32])
{
    static const char domain[] = "LayerX/program-discovery-proof/v1";
    uint8_t preimage[161];
    size_t cursor = 0U;
    size_t index;
    CHECK(sizeof(domain) == 34U);
    memcpy(preimage, domain, sizeof(domain)); cursor += sizeof(domain);
    memcpy(preimage + cursor, attestation->program_id, 32U); cursor += 32U;
    preimage[cursor++] = 1U;
    for (index = 0U; index < 4U; ++index)
        preimage[cursor++] = (uint8_t)(attestation->version >> ((3U - index) * 8U));
    memcpy(preimage + cursor, attestation->code_hash, 32U); cursor += 32U;
    preimage[cursor++] = (uint8_t)(attestation->abi_version >> 8U);
    preimage[cursor++] = (uint8_t)attestation->abi_version;
    for (index = 0U; index < 8U; ++index)
        preimage[cursor++] = (uint8_t)(attestation->observed_sequence >> ((7U - index) * 8U));
    for (index = 0U; index < 8U; ++index)
        preimage[cursor++] = (uint8_t)(attestation->observed_at >> ((7U - index) * 8U));
    for (index = 0U; index < 8U; ++index)
        preimage[cursor++] = (uint8_t)(attestation->valid_through >> ((7U - index) * 8U));
    memcpy(preimage + cursor, attestation->state_root, 32U); cursor += 32U;
    CHECK(cursor == sizeof(preimage));
    CHECK(lxp_hash_sha256(preimage, cursor, digest) == LXP_OK);
    return 0;
}

int main(int argc, char **argv)
{
    uint8_t private_key[32];
    uint8_t public_key[32];
    uint8_t digest[32];
    uint8_t expected_digest[32];
    uint8_t signature[64];
    uint8_t payload[LXP_DAEMON_HEAD_ATTESTATION_PAYLOAD_BYTES];
    uint8_t proof[LXP_DAEMON_HEAD_ATTESTATION_PROOF_BYTES];
    lxp_daemon_head_attestation attestation = fixture_attestation();
    lxp_daemon_head_attestation changed;
    uint8_t changed_digest[32];
    bool output = argc == 2 && strcmp(argv[1], "--vector") == 0;
    CHECK(argc == 1 || output);
    memset(private_key, 0x31, sizeof(private_key));
    CHECK(derive_public_key(private_key, public_key) == 0);
    CHECK(lxp_daemon_head_attestation_digest(&attestation, digest) == LXP_OK);
    CHECK(independent_digest(&attestation, expected_digest) == 0);
    CHECK(memcmp(digest, expected_digest, 32U) == 0);
    CHECK(lxp_daemon_head_attestation_sign(private_key, public_key, &attestation, signature) == LXP_OK);
    CHECK(lxp_ed25519_verify_raw(public_key, signature, digest, 32U) == LXP_OK);
    CHECK(lxp_daemon_head_attestation_encode(&attestation, payload) == LXP_OK);
    CHECK(payload[0] == 0U && payload[1] == 1U);
    CHECK(memcmp(payload + 2U, attestation.program_id, 32U) == 0);
    CHECK(payload[34] == 0U && payload[35] == 0U && payload[36] == 0U && payload[37] == 3U);
    CHECK(memcmp(payload + 38U, attestation.code_hash, 32U) == 0);
    CHECK(payload[70] == 0U && payload[71] == 2U);
    CHECK(memcmp(payload + 96U, attestation.state_root, 32U) == 0);
    CHECK(memcmp(payload + 128U, attestation.head_receipt_digest, 32U) == 0);
    memcpy(proof, public_key, 32U);
    memcpy(proof + 32U, signature, 64U);

    changed = attestation;
    changed.state_root[0] ^= 1U;
    CHECK(lxp_daemon_head_attestation_digest(&changed, changed_digest) == LXP_OK);
    CHECK(memcmp(changed_digest, digest, 32U) != 0);
    CHECK(lxp_ed25519_verify_raw(public_key, signature, changed_digest, 32U) != LXP_OK);
    changed = attestation;
    changed.valid_through += 1U;
    CHECK(lxp_daemon_head_attestation_digest(&changed, changed_digest) == LXP_OK);
    CHECK(lxp_ed25519_verify_raw(public_key, signature, changed_digest, 32U) != LXP_OK);
    changed = attestation;
    changed.head_receipt_digest[0] ^= 1U;
    CHECK(lxp_daemon_head_attestation_digest(&changed, changed_digest) == LXP_OK);
    CHECK(memcmp(changed_digest, digest, 32U) == 0);
    changed = attestation;
    changed.abi_version = 1U;
    CHECK(lxp_daemon_head_attestation_digest(&changed, changed_digest) == LXP_OK);
    CHECK(lxp_ed25519_verify_raw(public_key, signature, changed_digest, 32U) != LXP_OK);
    signature[0] ^= 1U;
    CHECK(lxp_ed25519_verify_raw(public_key, signature, digest, 32U) != LXP_OK);
    signature[0] ^= 1U;
    private_key[0] ^= 1U;
    CHECK(lxp_daemon_head_attestation_sign(private_key, public_key, &attestation, signature) == LXP_ERR_BAD_SIGNATURE);
    private_key[0] ^= 1U;
    CHECK(lxp_daemon_head_attestation_sign(private_key, public_key, &attestation, signature) == LXP_OK);

    if (output) {
        printf("{");
        print_hex_field("program_id", attestation.program_id, 32U, false);
        print_u64_field("version", attestation.version, false);
        print_hex_field("code_hash", attestation.code_hash, 32U, false);
        print_u64_field("abi_version", attestation.abi_version, false);
        print_u64_field("observed_sequence", attestation.observed_sequence, false);
        print_u64_field("observed_at", attestation.observed_at, false);
        print_u64_field("valid_through", attestation.valid_through, false);
        print_hex_field("state_root", attestation.state_root, 32U, false);
        print_hex_field("head_receipt_digest", attestation.head_receipt_digest, 32U, false);
        print_hex_field("digest", digest, 32U, false);
        print_hex_field("sequencer_public_key", public_key, 32U, false);
        print_hex_field("signature", signature, 64U, false);
        print_hex_field("payload", payload, sizeof(payload), false);
        print_hex_field("proof", proof, sizeof(proof), true);
        printf("}\n");
    }
    lxp_secure_zero(private_key, sizeof(private_key));
    return 0;
}
