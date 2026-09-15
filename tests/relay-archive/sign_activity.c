#define main admission_fixture_main
#include "../daemon/lxp_test_program_admission.c"
#undef main

int main(int argc, char **argv)
{
    signer key;
    uint8_t deploy[112] = {1U}, encoded[ACTIVITY_CAPACITY];
    size_t length = 0U;
    char *end = NULL;
    static const char digits[] = "0123456789abcdef";
    unsigned long long sequence;
    REQUIRE(argc == 2 || argc == 3 || argc == 4);
    REQUIRE(argc != 4 || strcmp(argv[3], "expired") == 0);
    errno = 0;
    sequence = strtoull(argv[1], &end, 10);
    REQUIRE(errno == 0 && end != argv[1] && *end == '\0' && sequence < 255U);
    REQUIRE(signer_init(&key, 0x11U) == 0);
    memcpy(REGISTERED_DID, "did:layerx:", 11U);
    for (size_t i = 0U; i < 32U; ++i) {
        REGISTERED_DID[11U + i * 2U] = (uint8_t)digits[key.public_key[i] >> 4U];
        REGISTERED_DID[12U + i * 2U] = (uint8_t)digits[key.public_key[i] & 15U];
    }
    deploy[0] = (uint8_t)(sequence + 1U);
    if (argc >= 3) {
        unsigned long long variant;
        errno = 0;
        variant = strtoull(argv[2], &end, 10);
        REQUIRE(errno == 0 && end != argv[2] && *end == '\0' && variant > 0U && variant < 255U);
        deploy[1] = (uint8_t)variant;
    }
    store_u16(deploy + 32U, 1U);
    store_u32(deploy + 100U, 8U);
    memcpy(deploy + 104U, "\0asm\1\0\0\0", 8U);
    REQUIRE(lxp_hash_sha256(deploy + 104U, 8U, deploy + 68U) == LXP_OK);
    REQUIRE(build_activity(&key, (uint64_t)sequence, LX_PROGRAMS_DEPLOY, argc == 4 ? 1U : 0U,
                           deploy, sizeof(deploy), encoded, sizeof(encoded), &length) == 0);
    REQUIRE(fwrite(encoded, 1U, length, stdout) == length);
    return 0;
}
