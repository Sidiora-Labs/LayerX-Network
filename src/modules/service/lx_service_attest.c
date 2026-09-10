#include "lx_service_dispatch.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

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
    lx_service_put_u64(bytes + offset, execution->execution_start);
    offset += 8U;
    lx_service_put_u64(bytes + offset, execution->execution_end);
    offset += 8U;
    lx_service_put_u64(bytes + offset, execution->resource_units);
    offset += 8U;
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
    lx_service_put_u64(bytes + LX_SERVICE_ATTESTATION_BYTES + 64U,
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
    execution->execution_start = lx_service_get_u64(bytes + offset);
    offset += 8U;
    execution->execution_end = lx_service_get_u64(bytes + offset);
    offset += 8U;
    execution->resource_units = lx_service_get_u64(bytes + offset);
    offset += 8U;
    READ_FIELD(attestor_identity);
    READ_FIELD(availability_reference);
    READ_FIELD(public_key);
    READ_FIELD(signature);
#undef READ_FIELD
    execution->global_sequence = lx_service_get_u64(bytes + offset);
    return LXP_OK;
}

lxp_result lx_service_attestor_verify(
    lxp_module_ctx *ctx, const lx_service_execution *execution,
    const lx_service_attestor_grant *grant, uint64_t batch_timestamp)
{
    lx_service_commitment commitment;
    uint8_t bytes[384];
    size_t length;
    lxp_result status;
    if (ctx == NULL || execution == NULL || grant == NULL ||
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
    status = lx_service_commitment_lookup(ctx, execution->commitment_id,
                                          &commitment);
    if (status != LXP_OK || commitment.abandoned ||
        memcmp(commitment.agreement_id, execution->agreement_id, 32U) != 0 ||
        memcmp(commitment.provider, grant->principal, 32U) != 0)
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

lxp_result lx_service_tool_exec_attest_execute(
    lxp_module_ctx *ctx, const lx_service_attest_request *request,
    lx_service_execution *result)
{
    lx_service_execution execution;
    uint8_t bytes[384];
    size_t length;
    lxp_result status;
    if (ctx == NULL || request == NULL || result == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    status = lx_service_attestor_verify(ctx, &request->execution,
                                        request->grant,
                                        lxp_ctx_batch_timestamp_ms(ctx));
    if (status != LXP_OK) return status;
    if (lx_service_execution_lookup(ctx, request->execution.attestation_id,
                                    &execution) == LXP_OK)
        return LXP_ERR_SEQUENCE_REUSED;
    execution = request->execution;
    execution.global_sequence = lxp_ctx_global_sequence(ctx);
    status = lx_service_attestation_bytes(&execution, bytes, sizeof(bytes),
                                          &length);
    if (status != LXP_OK || length > sizeof(execution.canonical_payload))
        return status != LXP_OK ? status : LXP_ERR_LENGTH_LIMIT;
    execution.canonical_payload_length = (uint16_t)length;
    (void)memcpy(execution.canonical_payload, bytes, length);
    status = lx_service_execution_put(ctx, &execution);
    if (status != LXP_OK) return status;
    *result = execution;
    return LXP_OK;
}
