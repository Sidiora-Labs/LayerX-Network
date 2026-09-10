#include "lx_escrow_internal.h"

#include "layerx/lxp_activity.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static const uint32_t activity_types[] = {
    LX_ESCROW_OPEN, LX_ESCROW_CAPTURE, LX_ESCROW_PARTIAL_CAPTURE,
    LX_ESCROW_RELEASE, LX_ESCROW_TIMEOUT, LX_ESCROW_DISPUTE_OPEN,
    LX_ESCROW_DISPUTE_RESOLVE
};

typedef struct escrow_open_payload {
    lx_escrow_record record;
    lxp_u128 amount;
} escrow_open_payload;

typedef struct escrow_capture_payload {
    uint8_t escrow_id[32];
    lxp_u128 amount;
    uint8_t idempotency_key[32];
} escrow_capture_payload;

typedef struct escrow_release_payload {
    uint8_t escrow_id[32];
    uint8_t idempotency_key[32];
} escrow_release_payload;

typedef struct escrow_dispute_payload {
    uint8_t escrow_id[32];
    uint32_t beneficiary_basis_points;
    uint8_t idempotency_key[32];
} escrow_dispute_payload;

typedef struct escrow_decoded {
    uint16_t ordinal;
    union {
        escrow_open_payload open;
        escrow_capture_payload capture;
        escrow_release_payload release;
        escrow_dispute_payload dispute;
    } value;
} escrow_decoded;

static lxp_result decode_open(const uint8_t *payload, size_t payload_length,
                              escrow_open_payload *out)
{
    lxp_result status;
    if (payload_length != (size_t)LX_ESCROW_OPEN_PAYLOAD_BYTES)
        return LXP_ERR_LENGTH_LIMIT;
    (void)memset(out, 0, sizeof(*out));
    (void)memcpy(out->record.escrow_id, payload, 32U);
    (void)memcpy(out->record.owner, payload + 32U, 32U);
    (void)memcpy(out->record.escrow_account, payload + 64U, 32U);
    (void)memcpy(out->record.beneficiary, payload + 96U, 32U);
    (void)memcpy(out->record.arbiter, payload + 128U, 32U);
    (void)memcpy(out->record.asset_id, payload + 160U, 32U);
    status = lxp_u128_from_be(payload + 192U, &out->amount);
    if (status != LXP_OK) return status;
    out->record.locked_amount = out->amount;
    out->record.state = LX_ESCROW_STATE_OPEN;
    out->record.expiry = lx_escrow_read_u64(payload + 208U);
    out->record.dispute_window = lx_escrow_read_u64(payload + 216U);
    (void)memcpy(out->record.terms_hash, payload + 224U, 32U);
    (void)memcpy(out->record.agreement_reference, payload + 256U, 32U);
    return lxp_ct_is_zero(out->record.escrow_id, 32U) ?
               LXP_ERR_NON_CANONICAL : LXP_OK;
}

static lxp_result decode_capture(const uint8_t *payload,
                                 size_t payload_length,
                                 escrow_capture_payload *out)
{
    lxp_result status;
    if (payload_length != (size_t)LX_ESCROW_CAPTURE_PAYLOAD_BYTES)
        return LXP_ERR_LENGTH_LIMIT;
    (void)memset(out, 0, sizeof(*out));
    (void)memcpy(out->escrow_id, payload, 32U);
    status = lxp_u128_from_be(payload + 32U, &out->amount);
    if (status != LXP_OK) return status;
    (void)memcpy(out->idempotency_key, payload + 48U, 32U);
    return lxp_ct_is_zero(out->escrow_id, 32U) ||
           lxp_ct_is_zero(out->idempotency_key, 32U) ?
               LXP_ERR_NON_CANONICAL : LXP_OK;
}

static lxp_result decode_release(const uint8_t *payload,
                                 size_t payload_length,
                                 escrow_release_payload *out)
{
    if (payload_length != (size_t)LX_ESCROW_RELEASE_PAYLOAD_BYTES)
        return LXP_ERR_LENGTH_LIMIT;
    (void)memset(out, 0, sizeof(*out));
    (void)memcpy(out->escrow_id, payload, 32U);
    (void)memcpy(out->idempotency_key, payload + 32U, 32U);
    return lxp_ct_is_zero(out->escrow_id, 32U) ||
           lxp_ct_is_zero(out->idempotency_key, 32U) ?
               LXP_ERR_NON_CANONICAL : LXP_OK;
}

static lxp_result decode_dispute(uint16_t ordinal, const uint8_t *payload,
                                 size_t payload_length,
                                 escrow_dispute_payload *out)
{
    size_t expected = ordinal == 6U ?
        (size_t)LX_ESCROW_DISPUTE_OPEN_PAYLOAD_BYTES :
        (size_t)LX_ESCROW_DISPUTE_RESOLVE_PAYLOAD_BYTES;
    if (payload_length != expected) return LXP_ERR_LENGTH_LIMIT;
    (void)memset(out, 0, sizeof(*out));
    (void)memcpy(out->escrow_id, payload, 32U);
    if (ordinal == 7U) {
        out->beneficiary_basis_points =
            (uint32_t)((uint32_t)payload[32] << 24U) |
            (uint32_t)((uint32_t)payload[33] << 16U) |
            (uint32_t)((uint32_t)payload[34] << 8U) |
            (uint32_t)payload[35];
        (void)memcpy(out->idempotency_key, payload + 36U, 32U);
        if (out->beneficiary_basis_points > LXP_BASIS_POINTS_ONE ||
            lxp_ct_is_zero(out->idempotency_key, 32U))
            return LXP_ERR_NON_CANONICAL;
    }
    return lxp_ct_is_zero(out->escrow_id, 32U) ? LXP_ERR_NON_CANONICAL :
                                                 LXP_OK;
}

static lxp_result module_genesis(lxp_module_ctx *ctx, const uint8_t *manifest,
                                 size_t manifest_length)
{
    if (ctx == NULL || (manifest == NULL && manifest_length != 0U))
        return LXP_ERR_NON_CANONICAL;
    return lxp_ctx_charge_gas(ctx, manifest_length);
}

static lxp_result module_decode(lxp_module_ctx *ctx, uint16_t ordinal,
                                const uint8_t *payload, size_t payload_length,
                                void **decoded)
{
    escrow_decoded *value;
    void *memory;
    lxp_result status;
    if (ctx == NULL || decoded == NULL || ordinal == 0U || ordinal > 7U ||
        payload == NULL || payload_length == 0U)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value), _Alignof(escrow_decoded),
                                 &memory);
    if (status != LXP_OK) return status;
    value = (escrow_decoded *)memory;
    (void)memset(value, 0, sizeof(*value));
    value->ordinal = ordinal;
    switch (ordinal) {
    case 1U:
        status = decode_open(payload, payload_length, &value->value.open);
        break;
    case 2U:
    case 3U:
        status = decode_capture(payload, payload_length,
                                &value->value.capture);
        break;
    case 4U:
    case 5U:
        status = decode_release(payload, payload_length,
                                &value->value.release);
        break;
    default:
        status = decode_dispute(ordinal, payload, payload_length,
                                &value->value.dispute);
        break;
    }
    if (status != LXP_OK) return status;
    *decoded = value;
    return LXP_OK;
}

static lxp_result module_validate(lxp_module_ctx *ctx,
                                  const lxp_activity *activity,
                                  const lxp_authority_resolved *authority,
                                  const void *decoded)
{
    const escrow_decoded *value = (const escrow_decoded *)decoded;
    if (ctx == NULL || activity == NULL || authority == NULL ||
        value == NULL || value->ordinal == 0U || value->ordinal > 7U)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (lxp_activity_module_id(activity->activity_type) != LXP_MODULE_ESCROW ||
        lxp_activity_type_ordinal(activity->activity_type) != value->ordinal)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (value->ordinal == 1U &&
        lxp_u128_is_zero(value->value.open.amount))
        return LXP_ERR_ZERO_AMOUNT;
    if (value->ordinal == 3U &&
        lxp_u128_is_zero(value->value.capture.amount))
        return LXP_ERR_ZERO_AMOUNT;
    if (value->ordinal == 7U &&
        value->value.dispute.beneficiary_basis_points > LXP_BASIS_POINTS_ONE)
        return LXP_ERR_PARAMETER_BOUNDS;
    return lxp_ctx_charge_gas(ctx, activity->payload.length + 1U);
}

static lxp_result resolve_asset(lx_escrow_runtime *runtime,
                                const uint8_t asset_id[32],
                                const lx_asset_record **asset)
{
    lx_asset_record *record;
    lxp_result status = lx_asset_lookup(runtime->assets, asset_id, &record);
    if (status != LXP_OK) return status;
    *asset = record;
    return LXP_OK;
}

static lxp_result execute_open(lxp_module_ctx *ctx,
                               lx_escrow_runtime *runtime,
                               const lxp_authority_resolved *authority,
                               const escrow_open_payload *payload,
                               lxp_receipt *receipt)
{
    lx_escrow_open_request request;
    lxp_result status;
    (void)memset(&request, 0, sizeof(request));
    status = lx_escrow_resolve_account(runtime, payload->record.owner,
                                       &request.owner);
    if (status == LXP_OK)
        status = lx_escrow_resolve_account(runtime,
                                           payload->record.escrow_account,
                                           &request.escrow_account);
    if (status == LXP_OK)
        status = resolve_asset(runtime, payload->record.asset_id,
                               &request.asset);
    if (status != LXP_OK) return status;
    if (memcmp(authority->principal, payload->record.owner, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    request.amount = payload->amount;
    request.record = payload->record;
    return lx_escrow_open_execute(ctx, &request, receipt);
}

static lxp_result execute_capture_activity(
    lxp_module_ctx *ctx, lx_escrow_runtime *runtime,
    const lxp_authority_resolved *authority, uint16_t ordinal,
    const escrow_capture_payload *payload, lxp_receipt *receipt)
{
    lx_escrow_capture_request request;
    lx_escrow_record record;
    lxp_result status = lx_escrow_lookup(ctx, payload->escrow_id, &record);
    if (status != LXP_OK) return status;
    (void)memset(&request, 0, sizeof(request));
    status = lx_escrow_resolve_account(runtime, record.escrow_account,
                                       &request.escrow_account);
    if (status == LXP_OK)
        status = lx_escrow_resolve_account(runtime, record.beneficiary,
                                           &request.beneficiary_account);
    if (status == LXP_OK)
        status = lx_escrow_resolve_account(runtime, record.owner,
                                           &request.owner_account);
    if (status == LXP_OK)
        status = resolve_asset(runtime, record.asset_id, &request.asset);
    if (status != LXP_OK) return status;
    request.escrow_id = payload->escrow_id;
    request.amount = payload->amount;
    request.authority = authority;
    (void)memcpy(request.idempotency_key, payload->idempotency_key, 32U);
    return ordinal == 2U ?
        lx_escrow_capture_execute(ctx, &request, receipt) :
        lx_escrow_partial_capture_execute(ctx, &request, receipt);
}

static lxp_result execute_release_activity(
    lxp_module_ctx *ctx, lx_escrow_runtime *runtime,
    const lxp_authority_resolved *authority, uint16_t ordinal,
    const escrow_release_payload *payload, lxp_receipt *receipt)
{
    lx_escrow_release_request request;
    lx_escrow_record record;
    lxp_result status = lx_escrow_lookup(ctx, payload->escrow_id, &record);
    if (status != LXP_OK) return status;
    (void)memset(&request, 0, sizeof(request));
    status = lx_escrow_resolve_account(runtime, record.escrow_account,
                                       &request.escrow_account);
    if (status == LXP_OK)
        status = lx_escrow_resolve_account(runtime, record.owner,
                                           &request.owner_account);
    if (status == LXP_OK)
        status = resolve_asset(runtime, record.asset_id, &request.asset);
    if (status != LXP_OK) return status;
    request.escrow_id = payload->escrow_id;
    request.authority = authority;
    (void)memcpy(request.idempotency_key, payload->idempotency_key, 32U);
    return ordinal == 4U ?
        lx_escrow_release_execute(ctx, &request, receipt) :
        lx_escrow_timeout_execute(ctx, &request, receipt);
}

static lxp_result execute_dispute_activity(
    lxp_module_ctx *ctx, lx_escrow_runtime *runtime,
    const lxp_authority_resolved *authority, uint16_t ordinal,
    const escrow_dispute_payload *payload, lxp_receipt *receipt)
{
    lx_escrow_dispute_request request;
    lx_escrow_record record;
    lxp_result status = lx_escrow_lookup(ctx, payload->escrow_id, &record);
    if (status != LXP_OK) return status;
    (void)memset(&request, 0, sizeof(request));
    status = lx_escrow_resolve_account(runtime, record.escrow_account,
                                       &request.escrow_account);
    if (status == LXP_OK)
        status = lx_escrow_resolve_account(runtime, record.beneficiary,
                                           &request.beneficiary_account);
    if (status == LXP_OK)
        status = lx_escrow_resolve_account(runtime, record.owner,
                                           &request.owner_account);
    if (status == LXP_OK)
        status = resolve_asset(runtime, record.asset_id, &request.asset);
    if (status != LXP_OK) return status;
    request.escrow_id = payload->escrow_id;
    request.authority = authority;
    request.beneficiary_basis_points = payload->beneficiary_basis_points;
    (void)memcpy(request.idempotency_key, payload->idempotency_key, 32U);
    return ordinal == 6U ? lx_escrow_dispute_open_execute(ctx, &request) :
        lx_escrow_dispute_resolve_execute(ctx, &request, receipt);
}

static lxp_result emit_state_event(lxp_module_ctx *ctx,
                                   const uint8_t escrow_id[32],
                                   uint16_t ordinal)
{
    lx_escrow_record record;
    uint8_t body[LX_ESCROW_EVENT_BYTES];
    lxp_result status = lx_escrow_lookup(ctx, escrow_id, &record);
    if (status != LXP_OK) return status;
    status = lx_escrow_event_body(&record, ordinal, body);
    if (status != LXP_OK) return status;
    return lxp_ctx_emit_event(ctx, ordinal, body, sizeof(body));
}

static lxp_result module_execute(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const void *decoded,
                                 lxp_effect_buffer *effects)
{
    const escrow_decoded *value = (const escrow_decoded *)decoded;
    lx_escrow_runtime *runtime;
    lxp_receipt receipt;
    const uint8_t *escrow_id;
    lxp_result status;
    (void)effects;
    if (ctx == NULL || activity == NULL || authority == NULL ||
        value == NULL || value->ordinal == 0U || value->ordinal > 7U)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    runtime = lx_escrow_require_runtime(ctx);
    if (runtime == NULL) return LXP_ERR_MODULE_DISABLED;
    (void)memset(&receipt, 0, sizeof(receipt));
    switch (value->ordinal) {
    case 1U:
        escrow_id = value->value.open.record.escrow_id;
        status = execute_open(ctx, runtime, authority, &value->value.open,
                              &receipt);
        break;
    case 2U:
    case 3U:
        escrow_id = value->value.capture.escrow_id;
        status = execute_capture_activity(ctx, runtime, authority,
                                          value->ordinal,
                                          &value->value.capture, &receipt);
        break;
    case 4U:
    case 5U:
        escrow_id = value->value.release.escrow_id;
        status = execute_release_activity(ctx, runtime, authority,
                                          value->ordinal,
                                          &value->value.release, &receipt);
        break;
    default:
        escrow_id = value->value.dispute.escrow_id;
        status = execute_dispute_activity(ctx, runtime, authority,
                                          value->ordinal,
                                          &value->value.dispute, &receipt);
        break;
    }
    if (status != LXP_OK) return status;
    return emit_state_event(ctx, escrow_id, value->ordinal);
}

static lxp_result module_epoch_end(lxp_module_ctx *ctx, uint64_t epoch,
                                   uint64_t timestamp)
{
    (void)epoch;
    (void)timestamp;
    return ctx == NULL ? LXP_ERR_NON_CANONICAL : lxp_ctx_charge_gas(ctx, 1U);
}

static lxp_result module_state_root(lxp_module_ctx *ctx, uint8_t root[32])
{
    if (ctx == NULL || root == NULL) return LXP_ERR_NON_CANONICAL;
    return lxp_state_subtree_root(ctx->kernel, LXP_MODULE_ESCROW, root);
}

const lxp_module_iface *lx_escrow_module_iface(void)
{
    static const lxp_module_iface iface = {
        LXP_MODULE_ESCROW, 1U, "escrow", activity_types,
        sizeof(activity_types) / sizeof(activity_types[0]),
        module_genesis, module_decode, module_validate, module_execute,
        lx_escrow_epoch_begin, module_epoch_end, module_state_root, NULL
    };
    return &iface;
}
