#define _POSIX_C_SOURCE 200809L
#include "layerx/lxp_bridge_credit.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "files.h"

#include <stdio.h>
#include <string.h>

#define CHECK(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #condition); return 1; } } while (0)

typedef struct layout {
    size_t commit;
    size_t validators_hash;
    size_t validator_count;
    size_t validators;
    size_t signatures;
    size_t signatures_end;
    size_t first_signature;
    size_t key;
    size_t value;
    size_t value_length;
    size_t store_name;
    size_t store_name_length;
} layout;

typedef struct payload {
    uint8_t *bytes;
    size_t length;
} payload;

static size_t number(const uint8_t *bytes, size_t width)
{
    size_t value = 0U;
    for (size_t index = 0U; index < width; ++index) value = (value << 8U) | bytes[index];
    return value;
}

static int locate(const uint8_t *bundle, size_t length, layout *out)
{
    size_t offset = 41U;
    size_t count;
    (void)memset(out, 0, sizeof(*out));
    CHECK(length > offset && memcmp(bundle, "LXLB1", 5U) == 0);
    offset += 1U + bundle[offset];
    offset += 4U;
    offset += 1U + bundle[offset];
    for (size_t index = 0U; index < 8U; ++index) {
        if (index == 2U) out->validators_hash = offset + 1U;
        offset += 1U + bundle[offset];
    }
    offset += 1U + bundle[offset];
    out->commit = offset;
    offset += 40U;
    count = number(bundle + offset, 2U);
    out->validator_count = count;
    out->validators = offset + 2U;
    offset += 2U + count * 40U;
    out->signatures = offset;
    for (size_t index = 0U; index < count; ++index) {
        uint8_t flag = bundle[offset];
        if (flag == 2U && out->first_signature == 0U) out->first_signature = offset + 13U;
        offset += flag == 1U ? 1U : 77U;
    }
    out->signatures_end = offset;
    offset += 2U + number(bundle + offset, 2U) * 40U;
    out->key = offset + 2U;
    offset += 2U + number(bundle + offset, 2U);
    out->value_length = number(bundle + offset, 2U);
    out->value = offset + 2U;
    offset += 2U + out->value_length;
    offset += 1U + bundle[offset];
    count = bundle[offset++];
    for (size_t index = 0U; index < count; ++index) {
        offset += 1U + bundle[offset];
        offset += 1U + bundle[offset];
    }
    out->store_name_length = bundle[offset];
    out->store_name = offset + 1U;
    CHECK(out->first_signature != 0U && out->store_name + out->store_name_length < length);
    return 0;
}

static int amount_digit(const uint8_t *value, size_t length, size_t *position)
{
    size_t offset = 0U;
    while (offset < length) {
        uint8_t tag = value[offset++];
        if ((tag & 7U) == 0U) {
            while (offset < length && (value[offset] & 0x80U) != 0U) ++offset;
            ++offset;
        } else {
            size_t size = value[offset++];
            CHECK((tag & 7U) == 2U && size < 0x80U && size <= length - offset);
            if ((tag >> 3U) == 7U) {
                *position = offset + size - 1U;
                return 0;
            }
            offset += size;
        }
    }
    return 1;
}

static int load(const char *path, payload *out)
{
    return read_file(path, LXP_MAX_PAYLOAD_BYTES, false, &out->bytes, &out->length);
}

static int copy(const payload *source, payload *out)
{
    out->bytes = malloc(source->length + 1U);
    CHECK(out->bytes != NULL);
    (void)memcpy(out->bytes, source->bytes, source->length);
    out->length = source->length;
    return 0;
}

static uint64_t now_ms;

static lxp_result verify(const lxp_bridge_profile *profile, const payload *input, bool rebind,
                         const lxp_bridge_light_trust *trusted, lxp_bridge_light_trust *advanced)
{
    lxp_bridge_credit credit;
    uint8_t nullifier[32];
    uint32_t network = (uint32_t)number(profile->bytes + 201U, 4U);
    if (lxp_bridge_credit_parse(input->bytes, input->length, &credit) != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    if (rebind &&
        (lxp_hash_sha256(profile->bytes, sizeof(profile->bytes), credit.bytes + 5U) != LXP_OK ||
         lxp_hash_sha256(credit.proof, credit.proof_length, credit.bytes + 327U) != LXP_OK))
        return LXP_ERR_IO;
    return lxp_bridge_credit_verify(profile, &credit, network, 3U, trusted, now_ms, nullifier, advanced);
}

static int read_profile(const char *path, lxp_bridge_profile *profile)
{
    payload file;
    CHECK(load(path, &file) == 0 && file.length == sizeof(profile->bytes));
    (void)memcpy(profile->bytes, file.bytes, file.length);
    free(file.bytes);
    return 0;
}

int main(int argc, char **argv)
{
    lxp_bridge_profile profile;
    lxp_bridge_profile adjacent_profile;
    lxp_bridge_profile changed_profile;
    lxp_bridge_profile retired_profile;
    lxp_bridge_credit credit;
    lxp_bridge_light_trust seeded;
    lxp_bridge_light_trust first;
    lxp_bridge_light_trust second;
    lxp_bridge_light_trust unused;
    payload original;
    payload adjacent;
    payload newer;
    payload retired;
    payload changed;
    layout at;
    uint8_t nullifier[32];
    uint8_t expected[55] = "LX:DEPOSIT:NULLIFIER:v1";
    uint8_t digest[32];
    uint8_t trust_bytes[LXP_BRIDGE_LIGHT_TRUST_BYTES];
    const uint8_t *bundle;
    uint8_t *did = NULL;
    size_t did_length = 0U;
    size_t bundle_length;
    size_t digit;
    uint32_t network;
    if (argc != 11) {
        (void)fprintf(stderr, "usage: test-light-credit profile credit adjacent-profile adjacent-credit later-credit retired-profile retired-credit did skip-profile skip-credit\n");
        return 2;
    }
    CHECK(read_profile(argv[1], &profile) == 0 && read_profile(argv[3], &adjacent_profile) == 0);
    CHECK(load(argv[2], &original) == 0 && load(argv[4], &adjacent) == 0 && load(argv[5], &newer) == 0);
    CHECK(load(argv[6], &changed) == 0 && changed.length == 207U && load(argv[7], &retired) == 0);
    (void)memset(&retired_profile, 0, sizeof(retired_profile));
    (void)memcpy(retired_profile.bytes, changed.bytes, changed.length);
    free(changed.bytes);
    now_ms = number(newer.bytes + LXP_BRIDGE_CREDIT_BYTES + 29U, 8U) * 1000U;
    CHECK(read_file(argv[8], 256U, false, &did, &did_length) == 0);
    while (did_length > 0U && (did[did_length - 1U] == '\n' || did[did_length - 1U] == '\r')) --did_length;
    network = (uint32_t)number(profile.bytes + 201U, 4U);
    CHECK(lxp_bridge_profile_validate(&profile) == LXP_OK);
    CHECK(lxp_bridge_profile_validate(&adjacent_profile) == LXP_OK);
    CHECK(lxp_bridge_credit_parse(original.bytes, original.length, &credit) == LXP_OK);
    bundle = credit.proof;
    bundle_length = credit.proof_length;
    CHECK(locate(bundle, bundle_length, &at) == 0);

    lxp_bridge_profile_trust(&profile, &seeded);
    CHECK(number(credit.bytes + 287U, 8U) > seeded.height + 1U);
    CHECK(lxp_bridge_credit_verify(&profile, &credit, network, 3U, NULL, now_ms, nullifier, &first) == LXP_OK);
    (void)memcpy(expected + 23U, credit.bytes + 43U, 32U);
    CHECK(lxp_hash_sha256(expected, sizeof(expected), digest) == LXP_OK &&
          memcmp(digest, nullifier, 32U) == 0);
    CHECK(first.height == number(credit.bytes + 287U, 8U) &&
          memcmp(first.header_hash, credit.bytes + 223U, 32U) == 0 &&
          !lxp_ct_is_zero(first.next_validators_hash, 32U));
    CHECK(lxp_bridge_light_trust_encode(&first, trust_bytes) == LXP_OK &&
          lxp_bridge_light_trust_decode(trust_bytes, sizeof(trust_bytes), &unused) == LXP_OK &&
          unused.height == first.height && unused.time_seconds == first.time_seconds &&
          unused.time_nanos == first.time_nanos &&
          memcmp(unused.header_hash, first.header_hash, 32U) == 0 &&
          memcmp(unused.next_validators_hash, first.next_validators_hash, 32U) == 0);
    CHECK(verify(&profile, &original, false, &seeded, &unused) == LXP_OK);
    CHECK(verify(&profile, &original, true, NULL, NULL) == LXP_OK);

    lxp_bridge_profile_trust(&adjacent_profile, &seeded);
    CHECK(seeded.height + 1U == first.height);
    CHECK(verify(&adjacent_profile, &adjacent, false, NULL, &unused) == LXP_OK &&
          unused.height == first.height);
    changed_profile = adjacent_profile;
    changed_profile.bytes[65] ^= 1U;
    CHECK(verify(&changed_profile, &adjacent, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);

    CHECK(verify(&profile, &newer, false, &first, &second) == LXP_OK && second.height > first.height);
    CHECK(verify(&profile, &original, false, &first, &unused) == LXP_OK && unused.height == first.height);
    CHECK(verify(&profile, &original, false, &second, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    unused = first;
    unused.header_hash[0] ^= 1U;
    CHECK(verify(&profile, &original, false, &unused, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    unused = first;
    unused.time_seconds = second.time_seconds;
    unused.time_nanos = second.time_nanos;
    CHECK(verify(&profile, &newer, false, &unused, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);

    for (size_t index = 0U; index < LXP_BRIDGE_CREDIT_BYTES; ++index) {
        CHECK(copy(&original, &changed) == 0);
        changed.bytes[index] ^= 1U;
        if (index >= 139U && index < 171U) {
            CHECK(lxp_bridge_credit_owner_bound(did, did_length, original.bytes + 139U));
            CHECK(!lxp_bridge_credit_owner_bound(did, did_length, changed.bytes + 139U));
        } else {
            CHECK(verify(&profile, &changed, false, NULL, NULL) != LXP_OK);
        }
        free(changed.bytes);
    }
    for (size_t index = 0U; index < sizeof(profile.bytes); ++index) {
        changed_profile = profile;
        changed_profile.bytes[index] ^= 1U;
        CHECK(verify(&changed_profile, &original, false, NULL, NULL) != LXP_OK);
        CHECK(verify(&changed_profile, &original, true, NULL, NULL) != LXP_OK ||
              (index >= 161U && index < 169U) || index >= 207U);
    }
    for (size_t index = 0U; index < bundle_length; ++index) {
        CHECK(copy(&original, &changed) == 0);
        changed.bytes[LXP_BRIDGE_CREDIT_BYTES + index] ^= 1U;
        CHECK(verify(&profile, &changed, true, NULL, NULL) != LXP_OK);
        free(changed.bytes);
    }

    CHECK(copy(&original, &changed) == 0);
    changed.bytes[LXP_BRIDGE_CREDIT_BYTES + at.first_signature + 7U] ^= 0x10U;
    CHECK(verify(&profile, &changed, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    free(changed.bytes);

    changed.length = LXP_BRIDGE_CREDIT_BYTES + at.signatures + at.validator_count +
                     (bundle_length - at.signatures_end);
    changed.bytes = malloc(changed.length);
    CHECK(changed.bytes != NULL);
    (void)memcpy(changed.bytes, original.bytes, LXP_BRIDGE_CREDIT_BYTES + at.signatures);
    (void)memset(changed.bytes + LXP_BRIDGE_CREDIT_BYTES + at.signatures, 1, at.validator_count);
    (void)memcpy(changed.bytes + LXP_BRIDGE_CREDIT_BYTES + at.signatures + at.validator_count,
                 bundle + at.signatures_end, bundle_length - at.signatures_end);
    CHECK(verify(&profile, &changed, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    free(changed.bytes);

    changed_profile = profile;
    changed_profile.bytes[169U + strlen((const char *)profile.bytes + 169U) - 1U] ^= 3U;
    CHECK(lxp_bridge_profile_validate(&changed_profile) == LXP_OK);
    CHECK(verify(&changed_profile, &original, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);

    changed_profile = profile;
    changed_profile.bytes[96] ^= 1U;
    CHECK(lxp_bridge_profile_validate(&changed_profile) == LXP_OK);
    CHECK(verify(&changed_profile, &original, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);

    CHECK(copy(&original, &changed) == 0);
    changed.bytes[LXP_BRIDGE_CREDIT_BYTES + at.validators_hash] ^= 1U;
    CHECK(verify(&profile, &changed, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    free(changed.bytes);

    CHECK(amount_digit(bundle + at.value, at.value_length, &digit) == 0);
    CHECK(copy(&original, &changed) == 0);
    changed.bytes[LXP_BRIDGE_CREDIT_BYTES + at.value + digit] ^= 1U;
    CHECK(verify(&profile, &changed, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    changed.bytes[206] ^= 1U;
    CHECK(verify(&profile, &changed, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    free(changed.bytes);

    CHECK(at.store_name_length == 13U && memcmp(bundle + at.store_name, "layerxcustody", 13U) == 0);
    CHECK(copy(&original, &changed) == 0);
    (void)memcpy(changed.bytes + LXP_BRIDGE_CREDIT_BYTES + at.store_name, "layerxanchor!", 13U);
    CHECK(verify(&profile, &changed, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    free(changed.bytes);
    CHECK(copy(&original, &changed) == 0);
    changed.bytes[LXP_BRIDGE_CREDIT_BYTES + at.key] = 0x21U;
    CHECK(verify(&profile, &changed, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    free(changed.bytes);

    CHECK(copy(&original, &changed) == 0);
    changed.bytes[changed.length++] = 0U;
    CHECK(verify(&profile, &changed, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    changed.length -= 2U;
    CHECK(verify(&profile, &changed, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    free(changed.bytes);

    now_ms = (number(profile.bytes + 207U, 8U) + number(profile.bytes + 215U, 8U)) * 1000U;
    CHECK(verify(&profile, &original, false, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    now_ms -= 1U;
    CHECK(verify(&profile, &original, false, NULL, NULL) == LXP_OK);
    now_ms = (number(bundle + 29U, 8U) - LXP_BRIDGE_LIGHT_MAX_CLOCK_DRIFT_SECONDS) * 1000U - 1U;
    CHECK(verify(&profile, &original, false, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    now_ms += 1U;
    CHECK(verify(&profile, &original, false, NULL, NULL) == LXP_OK);
    now_ms = number(newer.bytes + LXP_BRIDGE_CREDIT_BYTES + 29U, 8U) * 1000U;
    for (uint32_t total = 0U; total <= 102U; total += 102U) {
        CHECK(copy(&original, &changed) == 0);
        changed.bytes[LXP_BRIDGE_CREDIT_BYTES + at.commit + 4U] = 0U;
        changed.bytes[LXP_BRIDGE_CREDIT_BYTES + at.commit + 5U] = 0U;
        changed.bytes[LXP_BRIDGE_CREDIT_BYTES + at.commit + 6U] = 0U;
        changed.bytes[LXP_BRIDGE_CREDIT_BYTES + at.commit + 7U] = (uint8_t)total;
        CHECK(verify(&profile, &changed, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
        free(changed.bytes);
    }
    CHECK(copy(&original, &changed) == 0);
    (void)memset(changed.bytes + LXP_BRIDGE_CREDIT_BYTES + at.first_signature - 12U, 0, 8U);
    CHECK(verify(&profile, &changed, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
    free(changed.bytes);

    {
        lxp_bridge_profile skip_profile;
        payload skip;
        layout skip_at;
        size_t section;
        CHECK(read_profile(argv[9], &skip_profile) == 0 && load(argv[10], &skip) == 0);
        CHECK(locate(skip.bytes + LXP_BRIDGE_CREDIT_BYTES, skip.length - LXP_BRIDGE_CREDIT_BYTES,
                     &skip_at) == 0);
        section = LXP_BRIDGE_CREDIT_BYTES + skip_at.signatures_end;
        now_ms = number(skip.bytes + LXP_BRIDGE_CREDIT_BYTES + 29U, 8U) * 1000U;
        CHECK(number(skip.bytes + section, 2U) != 0U);
        CHECK(number(skip.bytes + 287U, 8U) > number(skip_profile.bytes + 161U, 8U) + 1U);
        CHECK(memcmp(skip.bytes + 295U, skip_profile.bytes + 65U, 32U) != 0);
        CHECK(verify(&skip_profile, &skip, false, NULL, &unused) == LXP_OK &&
              unused.height == number(skip.bytes + 287U, 8U));
        CHECK(copy(&skip, &changed) == 0);
        changed.bytes[section + 2U + 39U] ^= 1U;
        CHECK(verify(&skip_profile, &changed, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
        free(changed.bytes);
        CHECK(copy(&skip, &changed) == 0);
        changed.bytes[section + 2U] ^= 1U;
        CHECK(verify(&skip_profile, &changed, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
        free(changed.bytes);
        changed.length = skip.length - 40U;
        changed.bytes = malloc(changed.length);
        CHECK(changed.bytes != NULL);
        (void)memcpy(changed.bytes, skip.bytes, section);
        changed.bytes[section] = 0U;
        changed.bytes[section + 1U] = 0U;
        (void)memcpy(changed.bytes + section + 2U, skip.bytes + section + 42U, skip.length - section - 42U);
        CHECK(number(skip.bytes + section, 2U) == 1U);
        CHECK(verify(&skip_profile, &changed, true, NULL, NULL) == LXP_ERR_DEPOSIT_PROOF_NOT_FINAL);
        free(changed.bytes);
        free(skip.bytes);
    }

    CHECK(memcmp(retired_profile.bytes, "LXBC2", 5U) == 0 && retired.length == 427U &&
          memcmp(retired.bytes, "LXDC2", 5U) == 0);
    CHECK(lxp_bridge_profile_validate(&retired_profile) == LXP_ERR_NON_CANONICAL);
    CHECK(verify(&retired_profile, &retired, false, NULL, NULL) != LXP_OK);
    CHECK(verify(&profile, &retired, false, NULL, NULL) != LXP_OK);
    CHECK(verify(&profile, &retired, true, NULL, NULL) != LXP_OK);
    CHECK(verify(&profile, &original, false, NULL, NULL) == LXP_OK);

    free(original.bytes);
    free(adjacent.bytes);
    free(newer.bytes);
    free(retired.bytes);
    (void)puts("Paxeer light-client deposit credit: header, quorum, trust and ICS-23 bindings hold on a real paxd vector");
    return 0;
}
