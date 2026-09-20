#include "layerx/lxp_bridge_credit.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

const uint8_t lxp_bridge_profile_key[32] = "custody-credit-profile/v1";
const uint8_t lxp_bridge_light_trust_key[32] = "paxeer-light-trust/v1";

static uint64_t read_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    for (size_t index = 0U; index < 8U; ++index)
        value = (value << 8U) | bytes[index];
    return value;
}

static const uint8_t module_domain[] = "LX:CUSTODY:MODULE:v1";
static const uint8_t custody_store[] = "layerxcustody";

static size_t chain_id_length(const lxp_bridge_profile *profile)
{
    const uint8_t *text = profile->bytes + 169U;
    size_t length = 0U;
    while (length < LXP_BRIDGE_LIGHT_MAX_CHAIN_ID && text[length] != 0U) {
        if (text[length] < 0x21U || text[length] > 0x7eU) return 0U;
        ++length;
    }
    return lxp_ct_is_zero(text + length, LXP_BRIDGE_LIGHT_MAX_CHAIN_ID - length) ? length : 0U;
}

lxp_result lxp_bridge_profile_validate(const lxp_bridge_profile *profile)
{
    uint8_t reserve[32];
    uint8_t module[32];
    uint8_t identity[sizeof(module_domain) - 1U + sizeof(custody_store) - 1U + 20U];
    static const uint8_t name[] = "system:paxeer-reserve";
    if (profile == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(identity, module_domain, sizeof(module_domain) - 1U);
    (void)memcpy(identity + sizeof(module_domain) - 1U, custody_store, sizeof(custody_store) - 1U);
    (void)memcpy(identity + sizeof(identity) - 20U, profile->bytes + 13U, 20U);
    if (lxp_hash_sha256(identity, sizeof(identity), module) != LXP_OK ||
        lxp_ct_memcmp(module, profile->bytes + 33U, 32U) != 0 ||
        memcmp(profile->bytes, "LXBC3", 5U) != 0 ||
        read_u64(profile->bytes + 5U) != 125U ||
        lxp_ct_is_zero(profile->bytes + 13U, 20U) ||
        lxp_ct_is_zero(profile->bytes + 33U, 32U) ||
        lxp_ct_is_zero(profile->bytes + 65U, 32U) ||
        lxp_ct_is_zero(profile->bytes + 97U, 32U) ||
        read_u64(profile->bytes + 161U) == 0U ||
        read_u64(profile->bytes + 161U) >= (uint64_t)INT64_MAX ||
        chain_id_length(profile) == 0U ||
        lxp_ct_is_zero(profile->bytes + 201U, 4U) ||
        profile->bytes[205] != 0U || profile->bytes[206] != 3U ||
        read_u64(profile->bytes + 207U) == 0U ||
        read_u64(profile->bytes + 207U) > UINT32_MAX ||
        read_u64(profile->bytes + 215U) == 0U ||
        read_u64(profile->bytes + 215U) > UINT64_C(253402300799) ||
        lx_account_id_from_string(name, sizeof(name) - 1U, reserve) != LXP_OK ||
        lxp_ct_memcmp(reserve, profile->bytes + 129U, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

void lxp_bridge_profile_trust(const lxp_bridge_profile *profile,
                              lxp_bridge_light_trust *trust)
{
    (void)memset(trust, 0, sizeof(*trust));
    trust->height = read_u64(profile->bytes + 161U);
    (void)memcpy(trust->next_validators_hash, profile->bytes + 65U, 32U);
    trust->time_seconds = (int64_t)read_u64(profile->bytes + 215U);
}

lxp_result lxp_bridge_light_trust_load(lxp_module_ctx *ctx,
                                       const lxp_bridge_profile *profile,
                                       lxp_bridge_light_trust *trust)
{
    const uint8_t *stored;
    size_t length;
    lxp_result status;
    if (ctx == NULL || trust == NULL || lxp_bridge_profile_validate(profile) != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_kv_get(ctx, lxp_bridge_light_trust_key, 32U, &stored, &length);
    if (status == LXP_ERR_UNKNOWN_FIELD) {
        lxp_bridge_profile_trust(profile, trust);
        return LXP_OK;
    }
    if (status != LXP_OK) return status;
    status = lxp_bridge_light_trust_decode(stored, length, trust);
    if (status == LXP_OK && trust->height <= read_u64(profile->bytes + 161U))
        status = LXP_ERR_NON_CANONICAL;
    return status;
}

lxp_result lxp_bridge_credit_parse(const uint8_t *payload, size_t length,
                                   lxp_bridge_credit *credit)
{
    if (payload == NULL || credit == NULL || length < LXP_BRIDGE_CREDIT_MIN_PAYLOAD_BYTES ||
        length > LXP_MAX_PAYLOAD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(credit->bytes, payload, sizeof(credit->bytes));
    credit->proof = payload + sizeof(credit->bytes);
    credit->proof_length = length - sizeof(credit->bytes);
    return LXP_OK;
}

bool lxp_bridge_credit_matches(const lxp_bridge_credit *credit,
                               const uint8_t *payload, size_t length)
{
    return credit != NULL && payload != NULL && credit->proof != NULL &&
        length >= LXP_BRIDGE_CREDIT_MIN_PAYLOAD_BYTES &&
        length - sizeof(credit->bytes) == credit->proof_length &&
        memcmp(payload, credit->bytes, sizeof(credit->bytes)) == 0 &&
        memcmp(payload + sizeof(credit->bytes), credit->proof, credit->proof_length) == 0;
}

bool lxp_bridge_credit_owner_bound(const uint8_t *did, size_t did_length,
                                   const uint8_t owner_key[32])
{
    static const char digits[] = "0123456789abcdef";
    uint8_t expected[75] = "did:layerx:";
    if (did == NULL || owner_key == NULL || did_length != sizeof(expected)) return false;
    for (size_t index = 0U; index < 32U; ++index) {
        expected[11U + index * 2U] = (uint8_t)digits[owner_key[index] >> 4U];
        expected[12U + index * 2U] = (uint8_t)digits[owner_key[index] & 15U];
    }
    return lxp_ct_memcmp(did, expected, sizeof(expected)) == 0;
}

lxp_result lxp_bridge_genesis_profile(const lxp_genesis_manifest *manifest,
                                     lxp_bridge_profile *profile, bool *present)
{
    if (manifest == NULL || profile == NULL || present == NULL ||
        manifest->module_value_count > LXP_GENESIS_MAX_MODULE_VALUES)
        return LXP_ERR_NON_CANONICAL;
    *present = false;
    (void)memset(profile, 0, sizeof(*profile));
    for (size_t index = 0U; index < manifest->module_value_count; ++index) {
        const lxp_genesis_module_value *entry = &manifest->module_values[index];
        if (entry->module_id != LXP_MODULE_BRIDGE ||
            memcmp(entry->key, lxp_bridge_profile_key, 32U) != 0) continue;
        if (*present || manifest->protocol_version != 3U ||
            entry->value_length != sizeof(profile->bytes))
            return LXP_ERR_NON_CANONICAL;
        (void)memcpy(profile->bytes, entry->value, sizeof(profile->bytes));
        if (lxp_bridge_profile_validate(profile) != LXP_OK)
            return LXP_ERR_NON_CANONICAL;
        if ((((uint32_t)profile->bytes[201] << 24U) |
             ((uint32_t)profile->bytes[202] << 16U) |
             ((uint32_t)profile->bytes[203] << 8U) | profile->bytes[204]) != manifest->network_id)
            return LXP_ERR_NON_CANONICAL;
        *present = true;
    }
    return LXP_OK;
}

lxp_result lxp_bridge_genesis_append(lxp_genesis_manifest *manifest,
                                    const lxp_bridge_profile *profile)
{
    size_t position = 0U;
    if (manifest == NULL || manifest->protocol_version != 3U ||
        lxp_bridge_profile_validate(profile) != LXP_OK ||
        manifest->module_value_count >= LXP_GENESIS_MAX_MODULE_VALUES)
        return LXP_ERR_NON_CANONICAL;
    for (size_t index = 0U; index < manifest->module_value_count; ++index)
        if (manifest->module_values[index].module_id == LXP_MODULE_BRIDGE)
            return LXP_ERR_SEQUENCE_REUSED;
    while (position < manifest->module_value_count &&
           manifest->module_values[position].module_id < LXP_MODULE_BRIDGE)
        ++position;
    (void)memmove(&manifest->module_values[position + 1U],
                  &manifest->module_values[position],
                  (manifest->module_value_count - position) *
                      sizeof(manifest->module_values[0]));
    (void)memset(&manifest->module_values[position], 0,
                 sizeof(manifest->module_values[0]));
    manifest->module_values[position].module_id = LXP_MODULE_BRIDGE;
    (void)memcpy(manifest->module_values[position].key,
                 lxp_bridge_profile_key, 32U);
    (void)memcpy(manifest->module_values[position].value,
                 profile->bytes, sizeof(profile->bytes));
    manifest->module_values[position].value_length = sizeof(profile->bytes);
    ++manifest->module_value_count;
    return LXP_OK;
}

lxp_result lxp_bridge_credit_verify(const lxp_bridge_profile *profile,
                                    const lxp_bridge_credit *credit,
                                    uint32_t network_id,
                                    uint16_t protocol_version,
                                    const lxp_bridge_light_trust *trusted,
                                    uint64_t now_ms,
                                    uint8_t nullifier[32],
                                    lxp_bridge_light_trust *advanced)
{
    static const uint8_t deposit_domain[] = "LXP/Paxeer/custody-deposit/v1";
    static const uint8_t nullifier_domain[] = "LX:DEPOSIT:NULLIFIER:v1";
    uint8_t deposit[320] = {0};
    uint8_t digest[32];
    uint8_t nullifier_input[sizeof(nullifier_domain) - 1U + 32U];
    lxp_bridge_light_trust seeded;
    lxp_bridge_light_result proven;
    lxp_bridge_light_deposit record;
    const uint8_t *bytes;
    uint32_t network;
    uint64_t state_height;
    lxp_result status;
    if (credit == NULL || nullifier == NULL || protocol_version != 3U ||
        network_id == 0U || lxp_bridge_profile_validate(profile) != LXP_OK ||
        credit->proof == NULL || credit->proof_length == 0U ||
        credit->proof_length > LXP_MAX_PAYLOAD_BYTES - sizeof(credit->bytes))
        return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    bytes = credit->bytes;
    network = ((uint32_t)bytes[37] << 24U) | ((uint32_t)bytes[38] << 16U) |
              ((uint32_t)bytes[39] << 8U) | bytes[40];
    state_height = read_u64(bytes + 215U);
    status = lxp_hash_sha256(profile->bytes, sizeof(profile->bytes), digest);
    if (status != LXP_OK) return status;
    if (memcmp(bytes, "LXDC3", 5U) != 0 || network != network_id ||
        memcmp(bytes + 37U, profile->bytes + 201U, 6U) != 0 ||
        bytes[41] != 0U || bytes[42] != 3U ||
        lxp_ct_memcmp(bytes + 5U, digest, 32U) != 0 ||
        lxp_ct_memcmp(bytes + 75U, profile->bytes + 97U, 32U) != 0 ||
        lxp_ct_is_zero(bytes + 107U, 32U) ||
        !lxp_ed25519_pubkey_is_canonical(bytes + 139U) ||
        lxp_ct_is_zero(bytes + 171U, 20U) ||
        lxp_ct_is_zero(bytes + 191U, 16U) || read_u64(bytes + 207U) == 0U ||
        state_height == 0U || state_height >= (uint64_t)INT64_MAX - 1U ||
        read_u64(bytes + 287U) != state_height + 1U ||
        bytes[359] != 0U || bytes[360] != 0U || bytes[361] != 0U ||
        bytes[362] != LXP_BRIDGE_LIGHT_PROOF_KIND)
        return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    status = lxp_hash_sha256(credit->proof, credit->proof_length, digest);
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(digest, bytes + 327U, 32U) != 0)
        return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    deposit[30] = 1U;
    (void)memcpy(deposit + 56U, profile->bytes + 5U, 8U);
    (void)memcpy(deposit + 76U, profile->bytes + 13U, 20U);
    (void)memcpy(deposit + 108U, bytes + 171U, 20U);
    (void)memcpy(deposit + 128U, bytes + 75U, 32U);
    (void)memcpy(deposit + 160U, bytes + 107U, 32U);
    (void)memcpy(deposit + 208U, bytes + 191U, 16U);
    (void)memcpy(deposit + 248U, bytes + 207U, 8U);
    deposit[287] = (uint8_t)(sizeof(deposit_domain) - 1U);
    (void)memcpy(deposit + 288U, deposit_domain, sizeof(deposit_domain) - 1U);
    status = lxp_hash_sha256(deposit, sizeof(deposit), digest);
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(digest, bytes + 43U, 32U) != 0)
        return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    if (trusted == NULL) {
        lxp_bridge_profile_trust(profile, &seeded);
        trusted = &seeded;
    } else if (trusted->height < read_u64(profile->bytes + 161U)) {
        return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    }
    status = lxp_bridge_light_verify(profile->bytes + 169U, chain_id_length(profile),
                                     custody_store, sizeof(custody_store) - 1U, trusted,
                                     read_u64(profile->bytes + 207U), now_ms / 1000U,
                                     credit->proof, credit->proof_length, &proven);
    if (status != LXP_OK) return status;
    if (proven.height != state_height + 1U ||
        lxp_ct_memcmp(proven.header_hash, bytes + 223U, 32U) != 0 ||
        lxp_ct_memcmp(proven.app_hash, bytes + 255U, 32U) != 0 ||
        lxp_ct_memcmp(proven.validators_hash, bytes + 295U, 32U) != 0 ||
        proven.key_length != 33U || proven.key[0] != 0x20U ||
        lxp_ct_memcmp(proven.key + 1U, bytes + 43U, 32U) != 0 ||
        lxp_bridge_light_deposit_decode(proven.value, proven.value_length, &record) != LXP_OK ||
        lxp_ct_memcmp(record.deposit_id, bytes + 43U, 32U) != 0 ||
        lxp_ct_memcmp(record.asset_id, bytes + 75U, 32U) != 0 ||
        lxp_ct_memcmp(record.beneficiary, bytes + 107U, 32U) != 0 ||
        lxp_ct_memcmp(record.depositor, bytes + 171U, 20U) != 0 ||
        lxp_ct_memcmp(record.amount, bytes + 191U, 16U) != 0 ||
        record.nonce != read_u64(bytes + 207U) || record.height > state_height)
        return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    if (advanced != NULL) *advanced = proven.advanced;
    (void)memcpy(nullifier_input, nullifier_domain, sizeof(nullifier_domain) - 1U);
    (void)memcpy(nullifier_input + sizeof(nullifier_domain) - 1U, bytes + 43U, 32U);
    return lxp_hash_sha256(nullifier_input, sizeof(nullifier_input), nullifier);
}

static lxp_result genesis(lxp_module_ctx *ctx, const uint8_t *bytes, size_t length)
{
    return ctx == NULL || (bytes == NULL && length != 0U) ?
        LXP_ERR_NON_CANONICAL : lxp_ctx_charge_gas(ctx, length);
}

static lxp_result decode(lxp_module_ctx *ctx, uint16_t ordinal,
                          const uint8_t *bytes, size_t length, void **decoded)
{
    void *memory = NULL;
    lxp_result status;
    if (ctx == NULL || ordinal != 1U || bytes == NULL || decoded == NULL ||
        length < LXP_BRIDGE_CREDIT_MIN_PAYLOAD_BYTES || length > LXP_MAX_PAYLOAD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_arena_alloc(ctx, sizeof(lxp_bridge_credit),
                                 _Alignof(lxp_bridge_credit), &memory);
    if (status != LXP_OK) return status;
    status = lxp_bridge_credit_parse(bytes, length, memory);
    if (status != LXP_OK) return status;
    *decoded = memory;
    return LXP_OK;
}

static lxp_result validate_credit(lxp_module_ctx *ctx, const lxp_activity *activity,
                                  const lxp_authority_resolved *authority,
                                  const void *decoded)
{
    if (ctx == NULL || activity == NULL || authority == NULL || decoded == NULL ||
        activity->activity_type != LXP_BRIDGE_CREDIT ||
        activity->protocol_version != 3U || authority->kind != LXP_AUTHORITY_OWNER)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    return lxp_ctx_charge_gas(ctx, activity->payload.length);
}

static lxp_result execute(lxp_module_ctx *ctx, const lxp_activity *activity,
                          const lxp_authority_resolved *authority,
                          const void *decoded, lxp_effect_buffer *effects)
{
    (void)effects;
    return lxp_ctx_bridge_credit(ctx, activity, authority, decoded);
}

static lxp_result epoch(lxp_module_ctx *ctx, uint64_t number, uint64_t timestamp)
{
    (void)number;
    (void)timestamp;
    return ctx == NULL ? LXP_ERR_NON_CANONICAL : LXP_OK;
}

static lxp_result root(lxp_module_ctx *ctx, uint8_t digest[32])
{
    return ctx == NULL ? LXP_ERR_NON_CANONICAL :
        lxp_state_subtree_root(ctx->kernel, LXP_MODULE_BRIDGE, digest);
}

const lxp_module_iface *lxp_bridge_module_iface(void)
{
    static const uint32_t types[] = {LXP_BRIDGE_CREDIT};
    static const lxp_module_iface iface = {
        LXP_MODULE_BRIDGE, 1U, "bridge", types, 1U, genesis, decode,
        validate_credit, execute, epoch, epoch, root, NULL
    };
    return &iface;
}
