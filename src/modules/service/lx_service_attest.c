#include "layerx/lx_service.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

enum { LX_SERVICE_ATTESTATION_BYTES = 344, LX_SERVICE_EXECUTION_BYTES = 416 };

static void put_u64(uint8_t bytes[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        bytes[i] = (uint8_t)(value >> ((7U - i) * 8U));
}

static uint64_t get_u64(const uint8_t bytes[8])
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

lxp_result lx_service_attestation_bytes(
    const lx_service_execution *execution, uint8_t *bytes, size_t capacity,
    size_t *length)
{
    static const uint8_t tag[] = "LXP:SERVICE:ATTEST:v1";
    size_t offset = 0U;
    if (execution == NULL || bytes == NULL || length == NULL ||
        capacity < LX_SERVICE_ATTESTATION_BYTES + sizeof(tag) - 1U)
        return LXP_ERR_LENGTH_LIMIT;
#define COPY_FIELD(field) do { \
    (void)memcpy(bytes + offset, execution->field, \
                 sizeof(execution->field)); \
    offset += sizeof(execution->field); \
} while (0)
    (void)memcpy(bytes + offset, tag, sizeof(tag) - 1U);
    offset += sizeof(tag) - 1U;
    COPY_FIELD(attestation_id);
    COPY_FIELD(activity_id);
    COPY_FIELD(agreement_id);
    COPY_FIELD(commitment_id);
    COPY_FIELD(tool_id);
    COPY_FIELD(input_commitment_hash);
    COPY_FIELD(output_commitment_hash);
    put_u64(bytes + offset, execution->execution_start); offset += 8U;
    put_u64(bytes + offset, execution->execution_end); offset += 8U;
    put_u64(bytes + offset, execution->resource_units); offset += 8U;
    COPY_FIELD(attestor_identity);
    COPY_FIELD(availability_reference);
    COPY_FIELD(public_key);
#undef COPY_FIELD
    *length = offset;
    return LXP_OK;
}

lxp_result lx_service_execution_encode(const lx_service_execution *execution,
                                       uint8_t *bytes, size_t capacity,
                                       size_t *length)
{
    size_t message_length;
    size_t tag_length = sizeof("LXP:SERVICE:ATTEST:v1") - 1U;
    lxp_result status;
    if (execution == NULL || bytes == NULL || length == NULL ||
        capacity < LX_SERVICE_EXECUTION_BYTES)
        return LXP_ERR_LENGTH_LIMIT;
    status = lx_service_attestation_bytes(execution, bytes,
                                          capacity, &message_length);
    if (status != LXP_OK) return status;
    (void)memmove(bytes, bytes + tag_length, LX_SERVICE_ATTESTATION_BYTES);
    (void)memcpy(bytes + LX_SERVICE_ATTESTATION_BYTES,
                 execution->signature, 64U);
    put_u64(bytes + LX_SERVICE_ATTESTATION_BYTES + 64U,
            execution->global_sequence);
    *length = LX_SERVICE_EXECUTION_BYTES;
    return LXP_OK;
}

lxp_result lx_service_execution_decode(const uint8_t *bytes, size_t length,
                                       lx_service_execution *execution)
{
    size_t offset = 0U;
    if (bytes == NULL || execution == NULL ||
        length != LX_SERVICE_EXECUTION_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(execution, 0, sizeof(*execution));
#define READ_FIELD(field) do { \
    (void)memcpy(execution->field, bytes + offset, \
                 sizeof(execution->field)); \
    offset += sizeof(execution->field); \
} while (0)
    READ_FIELD(attestation_id);
    READ_FIELD(activity_id);
    READ_FIELD(agreement_id);
    READ_FIELD(commitment_id);
    READ_FIELD(tool_id);
    READ_FIELD(input_commitment_hash);
    READ_FIELD(output_commitment_hash);
    execution->execution_start = get_u64(bytes + offset); offset += 8U;
    execution->execution_end = get_u64(bytes + offset); offset += 8U;
    execution->resource_units = get_u64(bytes + offset); offset += 8U;
    READ_FIELD(attestor_identity);
    READ_FIELD(availability_reference);
    READ_FIELD(public_key);
    READ_FIELD(signature);
#undef READ_FIELD
    execution->global_sequence = get_u64(bytes + offset);
    return LXP_OK;
}

static const lx_service_commitment *commitment_find(
    const lx_service_store *store, const uint8_t commitment_id[32])
{
    size_t i;
    for (i = 0U; i < store->commitment_count; ++i)
        if (memcmp(store->commitments[i].commitment_id,
                   commitment_id, 32U) == 0)
            return &store->commitments[i];
    return NULL;
}

lxp_result lx_service_attestor_verify(
    const lx_service_store *store, const lx_service_execution *execution,
    const lx_service_attestor_grant *grant, uint64_t batch_timestamp)
{
    const lx_service_commitment *commitment;
    uint8_t bytes[384];
    size_t length;
    lxp_result status;
    if (lx_service_store_validate(store) != LXP_OK || execution == NULL ||
        grant == NULL ||
        lxp_ct_is_zero(execution->attestation_id, 32U) ||
        lxp_ct_is_zero(execution->activity_id, 32U) ||
        lxp_ct_is_zero(execution->agreement_id, 32U) ||
        lxp_ct_is_zero(execution->commitment_id, 32U) ||
        lxp_ct_is_zero(execution->tool_id, 32U) ||
        lxp_ct_is_zero(execution->input_commitment_hash, 32U) ||
        lxp_ct_is_zero(execution->output_commitment_hash, 32U) ||
        lxp_ct_is_zero(execution->availability_reference, 32U) ||
        execution->execution_end < execution->execution_start ||
        execution->resource_units == 0U || grant->revoked ||
        batch_timestamp < grant->not_before ||
        (grant->not_after != 0U && batch_timestamp > grant->not_after) ||
        grant->module_id != LXP_MODULE_SERVICE ||
        grant->activity_type != LX_SERVICE_TOOL_EXEC_ATTEST ||
        memcmp(execution->public_key, grant->public_key, 32U) != 0 ||
        memcmp(execution->attestor_identity, grant->principal, 32U) != 0)
        return LXP_ERR_INVALID_ATTESTATION;
    commitment = commitment_find(store, execution->commitment_id);
    if (commitment == NULL || commitment->abandoned ||
        memcmp(commitment->agreement_id, execution->agreement_id, 32U) != 0 ||
        memcmp(commitment->provider, grant->principal, 32U) != 0)
        return LXP_ERR_INVALID_ATTESTATION;
    status = lx_service_attestation_bytes(execution, bytes, sizeof(bytes),
                                          &length);
    if (status == LXP_OK)
        status = lxp_ed25519_verify(execution->public_key,
                                    execution->signature,
                                    LXP_DOMAIN_SIGNATURE_PREIMAGE,
                                    bytes, length);
    return status == LXP_OK ? LXP_OK : LXP_ERR_INVALID_ATTESTATION;
}

lxp_result lx_service_execution_put(lx_service_store *store,
                                    const lx_service_execution *execution)
{
    size_t i;
    if (lx_service_store_validate(store) != LXP_OK || execution == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < store->execution_count; ++i)
        if (memcmp(store->executions[i].attestation_id,
                   execution->attestation_id, 32U) == 0)
            return LXP_ERR_SEQUENCE_REUSED;
    if (store->execution_count == LX_SERVICE_STORE_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    store->executions[store->execution_count++] = *execution;
    return LXP_OK;
}

lxp_result lx_service_tool_exec_attest_execute(
    lxp_module_ctx *ctx, const lx_service_attest_request *request,
    lx_service_execution *result)
{
    lx_service_execution execution;
    uint8_t bytes[384];
    size_t length;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        result == NULL) return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    status = lx_service_attestor_verify(request->store, &request->execution,
                                        request->grant,
                                        lxp_ctx_batch_timestamp_ms(ctx));
    if (status != LXP_OK) return status;
    execution = request->execution;
    execution.global_sequence = lxp_ctx_global_sequence(ctx);
    status = lx_service_attestation_bytes(&execution, bytes, sizeof(bytes),
                                          &length);
    if (status != LXP_OK || length > sizeof(execution.canonical_payload))
        return status != LXP_OK ? status : LXP_ERR_LENGTH_LIMIT;
    execution.canonical_payload_length = (uint16_t)length;
    (void)memcpy(execution.canonical_payload, bytes, length);
    status = lx_service_execution_put(request->store, &execution);
    if (status != LXP_OK) return status;
    *result = execution;
    return LXP_OK;
}
