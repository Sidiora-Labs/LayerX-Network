#include "layerx/lxp_paxeer.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

#define LXP_PAXEER_MEMBERSHIP_TAG "LXP/Paxeer/membership-mirror/v1"
#define LXP_PAXEER_BOND_BINDING_TAG "LXP/Paxeer/bond-binding/v1"

static void put64(uint8_t out[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        out[i] = (uint8_t)(value >> (56U - 8U * i));
}

static void put32(uint8_t out[4], uint32_t value)
{
    size_t i;
    for (i = 0U; i < 4U; ++i)
        out[i] = (uint8_t)(value >> (24U - 8U * i));
}

static lxp_result absorb(lxp_hash_context *context, const void *data,
                         size_t length, lxp_result status)
{
    if (status != LXP_OK) return status;
    return lxp_hash_update(context, data, length);
}

static lxp_result absorb64(lxp_hash_context *context, uint64_t value,
                           lxp_result status)
{
    uint8_t encoded[8];
    put64(encoded, value);
    return absorb(context, encoded, sizeof(encoded), status);
}

static lxp_result absorb32(lxp_hash_context *context, uint32_t value,
                           lxp_result status)
{
    uint8_t encoded[4];
    put32(encoded, value);
    return absorb(context, encoded, sizeof(encoded), status);
}

static lxp_result absorb128(lxp_hash_context *context, lxp_u128 value,
                            lxp_result status)
{
    uint8_t encoded[16];
    if (status != LXP_OK) return status;
    status = lxp_u128_to_be(value, encoded);
    return absorb(context, encoded, sizeof(encoded), status);
}

static lxp_result membership_commitment(
    uint64_t paxeer_chain_id, const uint8_t contract[20],
    uint64_t membership_version, uint64_t observed_epoch,
    uint64_t observed_block_number, lxp_u128 minimum_bond,
    const lxp_guarantor_set *members, uint8_t commitment[32])
{
    lxp_hash_context context;
    size_t i;
    size_t j;
    lxp_result status;
    if (members->count > LXP_MAX_GUARANTOR_ATTESTATIONS)
        return LXP_ERR_LENGTH_LIMIT;
    lxp_hash_init(&context);
    status = absorb(&context, LXP_PAXEER_MEMBERSHIP_TAG,
                    sizeof(LXP_PAXEER_MEMBERSHIP_TAG), LXP_OK);
    status = absorb64(&context, paxeer_chain_id, status);
    status = absorb(&context, contract, 20U, status);
    status = absorb64(&context, membership_version, status);
    status = absorb64(&context, observed_epoch, status);
    status = absorb64(&context, observed_block_number, status);
    status = absorb128(&context, minimum_bond, status);
    status = absorb64(&context, members->last_governance_sequence, status);
    status = absorb32(&context, (uint32_t)members->count, status);
    for (i = 0U; i < members->count; ++i) {
        const lxp_guarantor_bond_state *record = &members->records[i];
        uint8_t flags = (uint8_t)((record->active ? 1U : 0U) |
                                  (record->jailed ? 2U : 0U) |
                                  (record->unresolved_slashing ? 4U : 0U));
        if (record->signer_authorization_count >
            LXP_MAX_GUARANTOR_SIGNER_AUTHORIZATIONS) {
            lxp_secure_zero(&context, sizeof(context));
            return LXP_ERR_LENGTH_LIMIT;
        }
        status = absorb(&context, record->guarantor_id, 32U, status);
        status = absorb(&context, record->public_key, 33U, status);
        status = absorb128(&context, record->bond_amount, status);
        status = absorb64(&context, record->joined_epoch, status);
        status = absorb64(&context, record->removed_epoch, status);
        status = absorb64(&context, record->ejected_at_version, status);
        status = absorb(&context, &flags, 1U, status);
        status = absorb32(&context,
                          (uint32_t)record->signer_authorization_count,
                          status);
        for (j = 0U; j < record->signer_authorization_count; ++j) {
            const lxp_guarantor_signer_authorization *authorization =
                &record->signer_authorizations[j];
            status = absorb(&context, authorization->public_key, 33U, status);
            status = absorb64(&context, authorization->active_from_epoch,
                              status);
            status = absorb64(&context, authorization->active_until_epoch,
                              status);
            status = absorb64(&context, authorization->set_version, status);
        }
    }
    if (status != LXP_OK) {
        lxp_secure_zero(&context, sizeof(context));
        return status;
    }
    return lxp_hash_final(&context, commitment);
}

static lxp_guarantor_bond_state *find_bond(
    lxp_paxeer_bond_state *state, const uint8_t guarantor_id[32])
{
    size_t i;
    for (i = 0U; i < state->guarantors.count; ++i)
        if (memcmp(state->guarantors.records[i].guarantor_id,
                   guarantor_id, 32U) == 0)
            return &state->guarantors.records[i];
    return NULL;
}

lxp_result lxp_paxeer_bond_init(lxp_paxeer_bond_state *state,
                                 uint16_t protocol_version,
                                 uint32_t network_id,
                                 uint64_t paxeer_chain_id,
                                 const uint8_t paxeer_contract[20],
                                 lxp_u128 custodied_value,
                                 uint32_t minimum_bond_bps)
{
    lxp_result status;
    if (state == NULL || paxeer_contract == NULL ||
        !lxp_protocol_version_supported(protocol_version) || network_id == 0U ||
        paxeer_chain_id == 0U || lxp_ct_is_zero(paxeer_contract, 20U) ||
        minimum_bond_bps == 0U ||
        minimum_bond_bps > LXP_BASIS_POINTS_ONE)
        return LXP_ERR_PARAMETER_BOUNDS;
    (void)memset(state, 0, sizeof(*state));
    status = lxp_guarantor_set_init(&state->guarantors);
    if (status == LXP_OK)
        status = lxp_u128_mul_bps_ceil(custodied_value, minimum_bond_bps,
                                       &state->minimum_bond);
    if (status != LXP_OK) return status;
    state->protocol_version = protocol_version;
    state->network_id = network_id;
    state->paxeer_chain_id = paxeer_chain_id;
    (void)memcpy(state->paxeer_settlement_contract, paxeer_contract, 20U);
    state->custodied_value = custodied_value;
    state->minimum_bond_bps = minimum_bond_bps;
    return LXP_OK;
}

lxp_result lxp_paxeer_bond_deposit(
    lxp_paxeer_bond_state *state,
    const lxp_paxeer_bond_deposit_evidence *evidence)
{
    lxp_paxeer_membership_sync_availability availability;
    lxp_paxeer_bond_deposit_record *record;
    const lxp_guarantor_bond_state *bond;
    lxp_result status;
    size_t i;
    if (state == NULL || evidence == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_u128_is_zero(evidence->amount) ||
        lxp_u128_is_zero(evidence->total_bond) ||
        lxp_u128_cmp(evidence->amount, evidence->total_bond) > 0 ||
        lxp_ct_is_zero(evidence->transaction_id, 32U) ||
        lxp_ct_is_zero(evidence->guarantor_id, 32U) ||
        evidence->observed_block_number == 0U ||
        evidence->observed_at_ms == 0U || evidence->membership_version == 0U)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_guarantor_set_validate(&state->guarantors) != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    if (evidence->paxeer_chain_id != state->paxeer_chain_id ||
        memcmp(evidence->guarantor_bond_contract,
               state->paxeer_settlement_contract, 20U) != 0)
        return LXP_ERR_AUTH_SCOPE;
    status = lxp_paxeer_membership_sync_status(state, &availability);
    if (status != LXP_OK)
        return status;
    if (availability != LXP_PAXEER_MEMBERSHIP_SYNC_BOUND)
        return LXP_ERR_CONTEXT_MISMATCH;
    if (evidence->membership_version > state->membership.membership_version ||
        evidence->observed_block_number >
            state->membership.observed_block_number)
        return LXP_ERR_SEQUENCE_MISMATCH;
    for (i = 0U; i < state->deposit_count; ++i)
        if (memcmp(state->deposits[i].transaction_id,
                   evidence->transaction_id, 32U) == 0)
            return LXP_ERR_IDEMPOTENT_REPLAY;
    if (state->deposit_count >= LXP_MAX_GUARANTOR_ATTESTATIONS)
        return LXP_ERR_LENGTH_LIMIT;
    bond = find_bond(state, evidence->guarantor_id);
    if (bond == NULL || bond->removed_epoch != 0U ||
        bond->ejected_at_version != 0U)
        return LXP_ERR_AUTH_SCOPE;
    if (evidence->membership_version ==
            state->membership.membership_version &&
        lxp_u128_cmp(bond->bond_amount, evidence->total_bond) != 0)
        return LXP_ERR_CONSERVATION;
    if (lxp_u128_cmp(bond->bond_amount, evidence->amount) < 0)
        return LXP_ERR_CONSERVATION;
    record = &state->deposits[state->deposit_count];
    (void)memset(record, 0, sizeof(*record));
    (void)memcpy(record->guarantor_id, evidence->guarantor_id, 32U);
    (void)memcpy(record->transaction_id, evidence->transaction_id, 32U);
    record->observed_block_number = evidence->observed_block_number;
    record->observed_at_ms = evidence->observed_at_ms;
    record->membership_version = evidence->membership_version;
    record->amount = evidence->amount;
    record->total_bond = evidence->total_bond;
    state->deposit_count += 1U;
    return LXP_OK;
}

lxp_result lxp_paxeer_bond_deposit_proof(
    const lxp_paxeer_bond_state *state, const uint8_t transaction_id[32],
    lxp_paxeer_bond_deposit_record *record)
{
    size_t i;
    if (state == NULL || transaction_id == NULL || record == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (state->deposit_count > LXP_MAX_GUARANTOR_ATTESTATIONS)
        return LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; i < state->deposit_count; ++i)
        if (memcmp(state->deposits[i].transaction_id, transaction_id,
                   32U) == 0) {
            *record = state->deposits[i];
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_FIELD;
}

lxp_result lxp_paxeer_membership_commitment(
    const lxp_paxeer_membership_observation *observation,
    uint8_t commitment[32])
{
    if (observation == NULL || commitment == NULL)
        return LXP_ERR_NON_CANONICAL;
    return membership_commitment(observation->paxeer_chain_id,
                                 observation->guarantor_bond_contract,
                                 observation->membership_version,
                                 observation->observed_epoch,
                                 observation->observed_block_number,
                                 observation->minimum_bond,
                                 &observation->members, commitment);
}

lxp_result lxp_paxeer_membership_sync(
    lxp_paxeer_bond_state *state,
    const lxp_paxeer_membership_observation *observation,
    lxp_paxeer_membership_sync_availability *availability)
{
    uint8_t commitment[32];
    lxp_result status;
    if (state == NULL || observation == NULL || availability == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (!lxp_protocol_version_supported(state->protocol_version) ||
        state->network_id == 0U || state->paxeer_chain_id == 0U ||
        lxp_ct_is_zero(state->paxeer_settlement_contract, 20U))
        return LXP_ERR_NON_CANONICAL;
    if (observation->paxeer_chain_id != state->paxeer_chain_id ||
        memcmp(observation->guarantor_bond_contract,
               state->paxeer_settlement_contract, 20U) != 0)
        return LXP_ERR_AUTH_SCOPE;
    if (observation->membership_version == 0U ||
        observation->observed_epoch == 0U ||
        observation->observed_block_number == 0U ||
        observation->members.count == 0U ||
        observation->members.version != observation->membership_version)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_guarantor_set_validate(&observation->members);
    if (status != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_u128_cmp(observation->minimum_bond, state->minimum_bond) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    if (observation->membership_version <
            state->membership.membership_version ||
        observation->membership_version < state->guarantors.version ||
        observation->membership_version < state->mirror_version ||
        observation->observed_block_number <
            state->membership.observed_block_number ||
        observation->members.last_governance_sequence <
            state->membership.last_governance_sequence)
        return LXP_ERR_SEQUENCE_MISMATCH;
    status = lxp_paxeer_membership_commitment(observation, commitment);
    if (status != LXP_OK)
        return status;
    state->guarantors = observation->members;
    state->mirror_version = observation->members.version;
    state->membership.paxeer_chain_id = observation->paxeer_chain_id;
    (void)memcpy(state->membership.guarantor_bond_contract,
                 observation->guarantor_bond_contract, 20U);
    state->membership.membership_version = observation->membership_version;
    state->membership.observed_epoch = observation->observed_epoch;
    state->membership.observed_block_number =
        observation->observed_block_number;
    state->membership.last_governance_sequence =
        observation->members.last_governance_sequence;
    (void)memcpy(state->membership.commitment, commitment,
                 sizeof(state->membership.commitment));
    return lxp_paxeer_membership_sync_status(state, availability);
}

lxp_result lxp_paxeer_membership_sync_status(
    const lxp_paxeer_bond_state *state,
    lxp_paxeer_membership_sync_availability *availability)
{
    uint8_t commitment[32];
    lxp_result status;
    if (state == NULL || availability == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (state->membership.membership_version == 0U &&
        state->membership.observed_block_number == 0U &&
        lxp_ct_is_zero(state->membership.commitment,
                       sizeof(state->membership.commitment))) {
        *availability = LXP_PAXEER_MEMBERSHIP_SYNC_UNAVAILABLE;
        return LXP_OK;
    }
    if (state->membership.membership_version == 0U ||
        state->membership.observed_block_number == 0U ||
        lxp_ct_is_zero(state->membership.commitment,
                       sizeof(state->membership.commitment)) ||
        state->membership.paxeer_chain_id != state->paxeer_chain_id ||
        memcmp(state->membership.guarantor_bond_contract,
               state->paxeer_settlement_contract, 20U) != 0) {
        *availability = LXP_PAXEER_MEMBERSHIP_SYNC_DIVERGED;
        return LXP_OK;
    }
    if (state->guarantors.version != state->membership.membership_version ||
        state->mirror_version != state->membership.membership_version ||
        state->guarantors.last_governance_sequence !=
            state->membership.last_governance_sequence) {
        *availability = LXP_PAXEER_MEMBERSHIP_SYNC_STALE;
        return LXP_OK;
    }
    status = membership_commitment(state->paxeer_chain_id,
                                   state->paxeer_settlement_contract,
                                   state->membership.membership_version,
                                   state->membership.observed_epoch,
                                   state->membership.observed_block_number,
                                   state->minimum_bond, &state->guarantors,
                                   commitment);
    if (status != LXP_OK)
        return status;
    *availability =
        lxp_ct_memcmp(commitment, state->membership.commitment,
                      sizeof(state->membership.commitment)) == 0
            ? LXP_PAXEER_MEMBERSHIP_SYNC_BOUND
            : LXP_PAXEER_MEMBERSHIP_SYNC_DIVERGED;
    return LXP_OK;
}

static void put16(uint8_t out[2], uint16_t value)
{
    out[0] = (uint8_t)(value >> 8U);
    out[1] = (uint8_t)value;
}

static uint64_t get64(const uint8_t bytes[8])
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i)
        value = (value << 8U) | bytes[i];
    return value;
}

static uint32_t get32(const uint8_t bytes[4])
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | (uint32_t)bytes[3];
}

static uint16_t get16(const uint8_t bytes[2])
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | (uint16_t)bytes[1]);
}

static lxp_result binding_checksum(const uint8_t *bytes, size_t length,
                                   uint8_t checksum[32])
{
    lxp_hash_context context;
    lxp_result status;
    lxp_hash_init(&context);
    status = absorb(&context, LXP_PAXEER_BOND_BINDING_TAG,
                    sizeof(LXP_PAXEER_BOND_BINDING_TAG), LXP_OK);
    status = absorb(&context, bytes, length, status);
    if (status != LXP_OK) {
        lxp_secure_zero(&context, sizeof(context));
        return status;
    }
    return lxp_hash_final(&context, checksum);
}

static lxp_result deposit_encode(const lxp_paxeer_bond_deposit_record *record,
                                 uint8_t *out)
{
    lxp_result status;
    (void)memcpy(out, record->guarantor_id, 32U);
    (void)memcpy(out + 32U, record->transaction_id, 32U);
    put64(out + 64U, record->observed_block_number);
    put64(out + 72U, record->observed_at_ms);
    put64(out + 80U, record->membership_version);
    status = lxp_u128_to_be(record->amount, out + 88U);
    if (status != LXP_OK)
        return status;
    return lxp_u128_to_be(record->total_bond, out + 104U);
}

static lxp_result deposit_decode(const uint8_t *bytes,
                                 lxp_paxeer_bond_deposit_record *record)
{
    lxp_result status;
    (void)memset(record, 0, sizeof(*record));
    (void)memcpy(record->guarantor_id, bytes, 32U);
    (void)memcpy(record->transaction_id, bytes + 32U, 32U);
    record->observed_block_number = get64(bytes + 64U);
    record->observed_at_ms = get64(bytes + 72U);
    record->membership_version = get64(bytes + 80U);
    status = lxp_u128_from_be(bytes + 88U, &record->amount);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 104U, &record->total_bond);
    if (status != LXP_OK)
        return status;
    if (lxp_ct_is_zero(record->guarantor_id, 32U) ||
        lxp_ct_is_zero(record->transaction_id, 32U) ||
        record->observed_block_number == 0U || record->observed_at_ms == 0U ||
        record->membership_version == 0U ||
        lxp_u128_is_zero(record->amount) ||
        lxp_u128_is_zero(record->total_bond) ||
        lxp_u128_cmp(record->total_bond, record->amount) < 0)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lxp_paxeer_bond_binding_encode(
    const lxp_paxeer_bond_state *state, uint8_t *bytes, size_t capacity,
    size_t *length)
{
    size_t used;
    size_t i;
    lxp_result status;
    if (state == NULL || bytes == NULL || length == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (!lxp_protocol_version_supported(state->protocol_version) ||
        state->network_id == 0U || state->paxeer_chain_id == 0U ||
        state->minimum_bond_bps == 0U ||
        state->minimum_bond_bps > LXP_BASIS_POINTS_ONE ||
        lxp_ct_is_zero(state->paxeer_settlement_contract, 20U) ||
        state->membership.membership_version == 0U ||
        state->membership.observed_block_number == 0U ||
        state->membership.observed_epoch == 0U ||
        lxp_ct_is_zero(state->membership.commitment,
                       sizeof(state->membership.commitment)))
        return LXP_ERR_NON_CANONICAL;
    if (state->deposit_count > LXP_MAX_GUARANTOR_ATTESTATIONS)
        return LXP_ERR_LENGTH_LIMIT;
    used = (size_t)LXP_PAXEER_BOND_BINDING_PREFIX_SIZE +
           (size_t)LXP_PAXEER_BOND_DEPOSIT_ENCODED_SIZE * state->deposit_count +
           32U;
    if (capacity < used)
        return LXP_ERR_LENGTH_LIMIT;
    (void)memset(bytes, 0, used);
    put16(bytes, state->protocol_version);
    put32(bytes + 2U, state->network_id);
    put64(bytes + 6U, state->paxeer_chain_id);
    (void)memcpy(bytes + 14U, state->paxeer_settlement_contract, 20U);
    put64(bytes + 34U, state->membership.membership_version);
    put64(bytes + 42U, state->membership.observed_epoch);
    put64(bytes + 50U, state->membership.observed_block_number);
    put64(bytes + 58U, state->membership.last_governance_sequence);
    (void)memcpy(bytes + 66U, state->membership.commitment, 32U);
    status = lxp_u128_to_be(state->custodied_value, bytes + 98U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(state->minimum_bond, bytes + 114U);
    if (status != LXP_OK)
        return status;
    put32(bytes + 130U, state->minimum_bond_bps);
    put64(bytes + 134U, state->mirror_version);
    put32(bytes + 142U, (uint32_t)state->deposit_count);
    for (i = 0U; i < state->deposit_count; ++i) {
        status = deposit_encode(
            &state->deposits[i],
            bytes + LXP_PAXEER_BOND_BINDING_PREFIX_SIZE +
                (size_t)LXP_PAXEER_BOND_DEPOSIT_ENCODED_SIZE * i);
        if (status != LXP_OK)
            return status;
    }
    status = binding_checksum(bytes, used - 32U, bytes + used - 32U);
    if (status != LXP_OK)
        return status;
    *length = used;
    return LXP_OK;
}

lxp_result lxp_paxeer_bond_binding_decode(
    const uint8_t *bytes, size_t length, lxp_paxeer_bond_binding *binding)
{
    uint8_t checksum[32];
    uint32_t count;
    size_t expected;
    size_t i;
    size_t j;
    lxp_result status;
    if (bytes == NULL || binding == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (length < (size_t)LXP_PAXEER_BOND_BINDING_PREFIX_SIZE + 32U ||
        length > (size_t)LXP_PAXEER_BOND_BINDING_MAX_SIZE)
        return LXP_ERR_LENGTH_LIMIT;
    count = get32(bytes + 142U);
    if (count > LXP_MAX_GUARANTOR_ATTESTATIONS)
        return LXP_ERR_LENGTH_LIMIT;
    expected = (size_t)LXP_PAXEER_BOND_BINDING_PREFIX_SIZE +
               (size_t)LXP_PAXEER_BOND_DEPOSIT_ENCODED_SIZE * count + 32U;
    if (length != expected)
        return LXP_ERR_LENGTH_LIMIT;
    status = binding_checksum(bytes, length - 32U, checksum);
    if (status != LXP_OK)
        return status;
    if (lxp_ct_memcmp(checksum, bytes + length - 32U, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(binding, 0, sizeof(*binding));
    binding->protocol_version = get16(bytes);
    binding->network_id = get32(bytes + 2U);
    binding->membership.paxeer_chain_id = get64(bytes + 6U);
    (void)memcpy(binding->membership.guarantor_bond_contract, bytes + 14U, 20U);
    binding->membership.membership_version = get64(bytes + 34U);
    binding->membership.observed_epoch = get64(bytes + 42U);
    binding->membership.observed_block_number = get64(bytes + 50U);
    binding->membership.last_governance_sequence = get64(bytes + 58U);
    (void)memcpy(binding->membership.commitment, bytes + 66U, 32U);
    status = lxp_u128_from_be(bytes + 98U, &binding->custodied_value);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 114U, &binding->minimum_bond);
    if (status != LXP_OK)
        return status;
    binding->minimum_bond_bps = get32(bytes + 130U);
    binding->mirror_version = get64(bytes + 134U);
    binding->deposit_count = count;
    if (!lxp_protocol_version_supported(binding->protocol_version) ||
        binding->network_id == 0U ||
        binding->membership.paxeer_chain_id == 0U ||
        binding->minimum_bond_bps == 0U ||
        binding->minimum_bond_bps > LXP_BASIS_POINTS_ONE ||
        lxp_ct_is_zero(binding->membership.guarantor_bond_contract, 20U) ||
        binding->membership.membership_version == 0U ||
        binding->membership.observed_epoch == 0U ||
        binding->membership.observed_block_number == 0U ||
        binding->membership.last_governance_sequence >
            binding->membership.membership_version ||
        binding->mirror_version < binding->membership.membership_version ||
        lxp_ct_is_zero(binding->membership.commitment,
                       sizeof(binding->membership.commitment)))
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < binding->deposit_count; ++i) {
        status = deposit_decode(
            bytes + LXP_PAXEER_BOND_BINDING_PREFIX_SIZE +
                (size_t)LXP_PAXEER_BOND_DEPOSIT_ENCODED_SIZE * i,
            &binding->deposits[i]);
        if (status != LXP_OK)
            return status;
        if (binding->deposits[i].membership_version >
            binding->mirror_version)
            return LXP_ERR_SEQUENCE_MISMATCH;
        for (j = 0U; j < i; ++j)
            if (memcmp(binding->deposits[j].transaction_id,
                       binding->deposits[i].transaction_id, 32U) == 0)
                return LXP_ERR_IDEMPOTENT_REPLAY;
    }
    return LXP_OK;
}

lxp_result lxp_paxeer_bond_binding_adopt(
    lxp_paxeer_bond_state *state, const lxp_paxeer_bond_binding *previous)
{
    lxp_paxeer_membership_sync_availability availability;
    size_t i;
    size_t j;
    lxp_result status;
    if (state == NULL || previous == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (previous->deposit_count > LXP_MAX_GUARANTOR_ATTESTATIONS ||
        state->deposit_count > LXP_MAX_GUARANTOR_ATTESTATIONS)
        return LXP_ERR_LENGTH_LIMIT;
    status = lxp_paxeer_membership_sync_status(state, &availability);
    if (status != LXP_OK)
        return status;
    if (availability != LXP_PAXEER_MEMBERSHIP_SYNC_BOUND)
        return LXP_ERR_CONTEXT_MISMATCH;
    if (previous->protocol_version != state->protocol_version ||
        previous->network_id != state->network_id ||
        previous->membership.paxeer_chain_id != state->paxeer_chain_id ||
        memcmp(previous->membership.guarantor_bond_contract,
               state->paxeer_settlement_contract, 20U) != 0)
        return LXP_ERR_AUTH_SCOPE;
    if (previous->minimum_bond_bps != state->minimum_bond_bps ||
        lxp_u128_cmp(previous->custodied_value, state->custodied_value) != 0 ||
        lxp_u128_cmp(previous->minimum_bond, state->minimum_bond) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    if (previous->membership.membership_version >
            state->membership.membership_version ||
        previous->membership.observed_block_number >
            state->membership.observed_block_number ||
        previous->membership.last_governance_sequence >
            state->membership.last_governance_sequence ||
        previous->mirror_version > state->membership.membership_version)
        return LXP_ERR_SEQUENCE_MISMATCH;
    if (previous->membership.membership_version ==
            state->membership.membership_version &&
        (previous->membership.observed_epoch !=
             state->membership.observed_epoch ||
         lxp_ct_memcmp(previous->membership.commitment,
                       state->membership.commitment,
                       sizeof(state->membership.commitment)) != 0))
        return LXP_ERR_CONTEXT_MISMATCH;
    for (i = 0U; i < previous->deposit_count; ++i) {
        const lxp_paxeer_bond_deposit_record *record = &previous->deposits[i];
        bool known = false;
        if (record->membership_version >
                state->membership.membership_version ||
            record->observed_block_number >
                state->membership.observed_block_number)
            return LXP_ERR_SEQUENCE_MISMATCH;
        if (find_bond(state, record->guarantor_id) == NULL)
            return LXP_ERR_UNKNOWN_FIELD;
        for (j = 0U; j < state->deposit_count; ++j)
            if (memcmp(state->deposits[j].transaction_id,
                       record->transaction_id, 32U) == 0) {
                if (memcmp(&state->deposits[j], record,
                           sizeof(*record)) != 0)
                    return LXP_ERR_CONTEXT_MISMATCH;
                known = true;
                break;
            }
        if (known)
            continue;
        if (state->deposit_count >= LXP_MAX_GUARANTOR_ATTESTATIONS)
            return LXP_ERR_LENGTH_LIMIT;
        state->deposits[state->deposit_count] = *record;
        state->deposit_count += 1U;
    }
    return LXP_OK;
}

lxp_result lxp_paxeer_bond_state_read(
    const lxp_paxeer_bond_state *state, const uint8_t guarantor_id[32],
    lxp_guarantor_bond_state *bond, bool *threshold_eligible)
{
    size_t i;
    if (state == NULL || guarantor_id == NULL || bond == NULL ||
        threshold_eligible == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_guarantor_set_validate(&state->guarantors) != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < state->guarantors.count; ++i)
        if (memcmp(state->guarantors.records[i].guarantor_id,
                   guarantor_id, 32U) == 0) {
            *bond = state->guarantors.records[i];
            *threshold_eligible = bond->active && !bond->jailed &&
                !bond->unresolved_slashing && bond->removed_epoch == 0U &&
                bond->ejected_at_version == 0U &&
                lxp_u128_cmp(bond->bond_amount, state->minimum_bond) >= 0;
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_FIELD;
}

lxp_result lxp_paxeer_slash_submit(
    lxp_paxeer_bond_state *state, const uint8_t *evidence_bytes,
    size_t evidence_length, const lxp_equivocation_evidence *evidence,
    lxp_arena *arena)
{
    lxp_byte_span canonical;
    size_t mark;
    lxp_result status;
    if (state == NULL || evidence_bytes == NULL || evidence_length == 0U ||
        evidence == NULL || arena == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (evidence->kind == LXP_EQUIVOCATION_GUARANTOR &&
        (evidence->guarantor_first.protocol_version !=
             state->protocol_version ||
         evidence->guarantor_first.network_id != state->network_id ||
         evidence->guarantor_first.paxeer_chain_id !=
             state->paxeer_chain_id ||
         memcmp(evidence->guarantor_first.paxeer_settlement_contract,
                state->paxeer_settlement_contract, 20U) != 0))
        return LXP_ERR_AUTH_SCOPE;
    mark = lxp_arena_mark(arena);
    status = lxp_equivocation_encode(evidence, arena, &canonical);
    if (status == LXP_OK &&
        (canonical.length != evidence_length ||
         lxp_ct_memcmp(canonical.bytes, evidence_bytes,
                       evidence_length) != 0))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK)
        status = lxp_slashing_submit(evidence, &state->guarantors, arena);
    (void)lxp_arena_reset(arena, mark);
    if (status == LXP_OK) state->mirror_version = state->guarantors.version;
    return status;
}
