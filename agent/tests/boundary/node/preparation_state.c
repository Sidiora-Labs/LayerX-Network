#include "layerx/lxp_daemon.h"
#include "layerx/lxp_crypto.h"

#include <stdint.h>
#include <string.h>

enum { RESPONSE_BYTES = 4096 };

static uint16_t read_u16(const uint8_t bytes[2])
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | bytes[1]);
}

static uint32_t read_u32(const uint8_t bytes[4])
{
    return ((uint32_t)bytes[0] << 24U) |
        ((uint32_t)bytes[1] << 16U) |
        ((uint32_t)bytes[2] << 8U) | bytes[3];
}

static uint64_t read_u64(const uint8_t bytes[8])
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index)
        value = (value << 8U) | bytes[index];
    return value;
}

static void write_u16(uint8_t bytes[2], uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

int main(void)
{
    static const uint8_t did[] = "did:layerx:production-boundary";
    static uint64_t parameters = 1U;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_identity_store identities;
    lxp_identity *identity = NULL;
    lxp_daemon_receipt_authority_store authority;
    lxp_daemon_protocol_owner owner;
    uint8_t request[4U + sizeof(did) - 1U];
    uint8_t response[RESPONSE_BYTES];
    size_t response_length = 0U;
    size_t actor_length = sizeof(did) - 1U;
    size_t cursor;
    lxp_result status;
    (void)memset(&state, 0, sizeof(state));
    (void)memset(&journal, 0, sizeof(journal));
    (void)memset(&kernel, 0, sizeof(kernel));
    (void)memset(&identities, 0, sizeof(identities));
    (void)memset(&authority, 0, sizeof(authority));
    (void)memset(&owner, 0, sizeof(owner));
    if (lxp_state_store_init(&state, 1U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 3U) !=
            LXP_OK ||
        lxp_kernel_register_module(
            &kernel, programs_module_registration_v3()) != LXP_OK ||
        lxp_state_root(&kernel, kernel.current_state_root) != LXP_OK ||
        lxp_ct_is_zero(kernel.current_state_root, 32U) ||
        lxp_identity_register(&identities, did, actor_length,
                              (const uint8_t[32]){7U}, &identity) != LXP_OK ||
        identity == NULL || pthread_mutex_init(&owner.mutex, NULL) != 0)
        return 1;
    identity->next_sequence = 5U;
    owner.kernel = &kernel;
    owner.identities = &identities;
    owner.receipt_authority = &authority;
    owner.network_id = 77U;
    owner.latest_sealed_timestamp = UINT64_C(1700000001000);
    owner.feed_store.baseline_next_sequence = state.next_sequence;
    (void)memcpy(owner.feed_store.baseline_state_root,
                 kernel.current_state_root, 32U);
    owner.attached = true;
    write_u16(request, 1U);
    write_u16(request + 2U, (uint16_t)actor_length);
    (void)memcpy(request + 4U, did, actor_length);
    status = lxp_daemon_lni_preparation_state(
        &owner, request, sizeof(request), response, sizeof(response),
        &response_length);
    cursor = 0U;
    if (status != LXP_OK || response_length < 76U + actor_length ||
        read_u16(response + cursor) != 1U)
        return 1;
    cursor += 2U;
    if (read_u16(response + cursor) != actor_length) return 1;
    cursor += 2U;
    if (memcmp(response + cursor, did, actor_length) != 0) return 1;
    cursor += actor_length;
    if (read_u32(response + cursor) != 77U) return 1;
    cursor += 4U;
    if (read_u64(response + cursor) != identity->next_sequence) return 1;
    cursor += 8U;
    if (read_u64(response + cursor) != owner.latest_sealed_timestamp)
        return 1;
    cursor += 8U;
    if (read_u64(response + cursor) != 0U) return 1;
    cursor += 8U;
    if (lxp_ct_memcmp(response + cursor, kernel.current_state_root, 32U) != 0)
        return 1;
    cursor += 32U;
    if (read_u64(response + cursor) != kernel.epoch) return 1;
    cursor += 8U;
    if (read_u16(response + cursor) != 1U) return 1;
    cursor += 2U;
    if (read_u16(response + cursor) != LXP_MODULE_PROGRAMS) return 1;
    cursor += 2U;
    if (read_u16(response + cursor) == 0U) return 1;
    identity->status = LXP_IDENTITY_FROZEN;
    if (lxp_daemon_lni_preparation_state(
            &owner, request, sizeof(request), response, sizeof(response),
            &response_length) != LXP_ERR_IDENTITY_FROZEN ||
        response_length != 0U)
        return 1;
    identity->status = LXP_IDENTITY_RECOVERING;
    if (lxp_daemon_lni_preparation_state(
            &owner, request, sizeof(request), response, sizeof(response),
            &response_length) != LXP_ERR_UNKNOWN_DID)
        return 1;
    identity->status = LXP_IDENTITY_ACTIVE;
    request[sizeof(request) - 1U] ^= 1U;
    if (lxp_daemon_lni_preparation_state(
            &owner, request, sizeof(request), response, sizeof(response),
            &response_length) != LXP_ERR_UNKNOWN_DID)
        return 1;
    request[sizeof(request) - 1U] ^= 1U;
    if (lxp_daemon_lni_preparation_state(
            &owner, request, sizeof(request), response, 64U,
            &response_length) != LXP_ERR_LENGTH_LIMIT ||
        response_length != 0U)
        return 1;
    {
        lxp_daemon_receipt_authority_store *saved_authority =
            owner.receipt_authority;
        owner.receipt_authority = NULL;
        if (lxp_daemon_lni_preparation_state(
                &owner, request, sizeof(request), response,
                sizeof(response), &response_length) !=
            LXP_ERR_MODULE_DISABLED)
            return 1;
        owner.receipt_authority = saved_authority;
    }
    {
        uint32_t saved_activity = kernel.modules[0].activity_types[0];
        kernel.modules[0].activity_types[0] = UINT32_C(0x00080001);
        if (lxp_daemon_lni_preparation_state(
                &owner, request, sizeof(request), response, sizeof(response),
                &response_length) !=
            LXP_ERR_UNKNOWN_ACTIVITY)
            return 1;
        kernel.modules[0].activity_types[0] = saved_activity;
    }
    owner.feed_store.baseline_state_root[0] ^= 1U;
    if (lxp_daemon_lni_preparation_state(
            &owner, request, sizeof(request), response, sizeof(response),
            &response_length) != LXP_ERR_PROJECTION_STALE)
        return 1;
    owner.feed_store.baseline_state_root[0] ^= 1U;
    request[1] = 2U;
    if (lxp_daemon_lni_preparation_state(
            &owner, request, sizeof(request), response, sizeof(response),
            &response_length) != LXP_ERR_MALFORMED_ENVELOPE)
        return 1;
    if (pthread_mutex_destroy(&owner.mutex) != 0 ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}
