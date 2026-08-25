#include "layerx/lxp_gateway.h"
#include "lxp_gateway_internal.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

#include <string.h>

typedef struct receive_transaction {
    lxp_ledger_journal balances;
    size_t grant_count;
    size_t grant_index;
    lxp_grant_state grant_before;
    bool existing_grant;
    size_t idempotency_count;
    size_t invoice_count;
    size_t arena_mark;
} receive_transaction;

#ifdef LXP_TESTING
static _Thread_local lxp_gateway_transaction_boundary test_failure_boundary;
void lxp_gateway_receive_test_fail_after(
    lxp_gateway_transaction_boundary boundary)
{
    test_failure_boundary = boundary;
}
#endif

#ifdef LXP_TESTING
static lxp_result transaction_boundary(
    lxp_gateway_transaction_boundary boundary)
{
    if (test_failure_boundary != boundary) return LXP_OK;
    test_failure_boundary = 0;
    return LXP_ERR_IO;
}
#endif

static lxp_result receive_transaction_abort(
    lxp_gateway_receive_context *context,
    receive_transaction *transaction, lxp_receipt *receipt,
    lxp_result failure)
{
    lxp_result rollback = lxp_journal_rollback(&transaction->balances);
    lxp_result arena_reset;
    lxp_grant_store *grants = context->receive_environment->grants;
    lxp_send_store *idempotency = context->receive_environment->idempotency;
    size_t grant_end = grants->count < LXP_GRANT_STORE_CAPACITY ?
        grants->count : LXP_GRANT_STORE_CAPACITY;
    size_t idempotency_end = idempotency->count < LXP_SEND_STORE_CAPACITY ?
        idempotency->count : LXP_SEND_STORE_CAPACITY;
    size_t invoice_end = context->invoices->count <
            LXP_GATEWAY_INVOICE_CAPACITY ? context->invoices->count :
            LXP_GATEWAY_INVOICE_CAPACITY;
    if (transaction->existing_grant)
        grants->grants[transaction->grant_index] = transaction->grant_before;
    if (grant_end > transaction->grant_count)
        (void)memset(&grants->grants[transaction->grant_count], 0,
                     (grant_end - transaction->grant_count) *
                         sizeof(grants->grants[0]));
    grants->count = transaction->grant_count;
    if (idempotency_end > transaction->idempotency_count)
        (void)memset(&idempotency->records[transaction->idempotency_count], 0,
                     (idempotency_end - transaction->idempotency_count) *
                         sizeof(idempotency->records[0]));
    idempotency->count = transaction->idempotency_count;
    if (invoice_end > transaction->invoice_count)
        (void)memset(&context->invoices->records[transaction->invoice_count],
                     0, (invoice_end - transaction->invoice_count) *
                         sizeof(context->invoices->records[0]));
    context->invoices->count = transaction->invoice_count;
    arena_reset = lxp_arena_reset(context->arena, transaction->arena_mark);
    (void)memset(receipt, 0, sizeof(*receipt));
    return rollback == LXP_OK && arena_reset == LXP_OK ? failure :
           LXP_FATAL_INVARIANT;
}

static lx_account *account_for(
    lx_account_registry *accounts, const uint8_t account_id[32])
{
    size_t i;
    for (i = 0U; accounts != NULL && i < accounts->count; ++i)
        if (lxp_ct_memcmp(accounts->accounts[i].id,
                          account_id, 32U) == 0)
            return &accounts->accounts[i];
    return NULL;
}

static lxp_grant_state *grant_for(
    lxp_grant_store *store, const uint8_t grant_id[32])
{
    size_t i;
    for (i = 0U; store != NULL && i < store->count; ++i)
        if (lxp_ct_memcmp(store->grants[i].grant.grant_id,
                          grant_id, 32U) == 0)
            return &store->grants[i];
    return NULL;
}

static bool grant_equal(
    const lxp_payer_grant *left, const lxp_payer_grant *right)
{
    return lxp_ct_memcmp(left->grant_id, right->grant_id, 32U) == 0 &&
           lxp_ct_memcmp(left->from, right->from, 32U) == 0 &&
           lxp_ct_memcmp(left->recipient, right->recipient, 32U) == 0 &&
           lxp_ct_memcmp(left->asset, right->asset, 32U) == 0 &&
           lxp_u128_cmp(left->per_draw_maximum,
                        right->per_draw_maximum) == 0 &&
           lxp_u128_cmp(left->allowance, right->allowance) == 0 &&
           left->recurring == right->recurring &&
           left->window_length == right->window_length &&
           left->expiration == right->expiration &&
           lxp_ct_memcmp(left->purpose_hash, right->purpose_hash, 32U) == 0 &&
           left->has_reference == right->has_reference &&
           lxp_ct_memcmp(
               left->reference_hash, right->reference_hash, 32U) == 0 &&
           left->revocation_sequence == right->revocation_sequence &&
           lxp_ct_memcmp(left->public_key, right->public_key, 32U) == 0 &&
           lxp_ct_memcmp(left->signature, right->signature, 64U) == 0;
}

static lxp_result gateway_grant_present_locked(
    const lxp_payer_grant *grant,
    lx_account_registry *accounts,
    lxp_grant_store *store)
{
    lx_account *payer;
    lxp_grant_state *existing;
    if (grant == NULL || accounts == NULL || store == NULL ||
        accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY ||
        store->count > LXP_GRANT_STORE_CAPACITY)
        return LXP_ERR_MALFORMED_GRANT;
    payer = account_for(accounts, grant->from);
    if (payer == NULL) return LXP_ERR_NO_PAYER_GRANT;
    existing = grant_for(store, grant->grant_id);
    if (existing != NULL)
        return grant_equal(&existing->grant, grant) ?
            LXP_OK : LXP_ERR_SEQUENCE_REUSED;
    return lxp_grant_store_put(store, grant, payer);
}

#ifdef LXP_TESTING
lxp_result lxp_gateway_grant_present_test_locked(
    const lxp_payer_grant *grant,
    lx_account_registry *accounts,
    lxp_grant_store *store)
{
    return gateway_grant_present_locked(grant, accounts, store);
}
#endif

lxp_result lxp_gateway_grant_bounds_check(
    const lxp_payment_requirement *requirement,
    const lxp_receive *receive,
    const lxp_grant_state *grant_state,
    const lxp_receive_environment *environment)
{
    lxp_u128 drawn;
    const lxp_payer_grant *grant;
    if (requirement == NULL || receive == NULL || grant_state == NULL ||
        environment == NULL) return LXP_ERR_MALFORMED_GRANT;
    grant = &grant_state->grant;
    if (grant_state->revoked &&
        environment->global_sequence >= grant_state->revoked_at_sequence)
        return LXP_ERR_GRANT_REVOKED;
    if (environment->batch_timestamp > grant->expiration ||
        environment->batch_timestamp > requirement->expiry)
        return LXP_ERR_GRANT_EXPIRED;
    if (lxp_ct_memcmp(receive->to, requirement->recipient, 32U) != 0 ||
        lxp_ct_memcmp(receive->to, grant->recipient, 32U) != 0 ||
        lxp_ct_memcmp(receive->from, grant->from, 32U) != 0 ||
        lxp_ct_memcmp(receive->asset, requirement->asset, 32U) != 0 ||
        lxp_ct_memcmp(receive->asset, grant->asset, 32U) != 0 ||
        lxp_u128_cmp(receive->amount, requirement->amount) != 0 ||
        lxp_u128_cmp(receive->amount, grant->per_draw_maximum) > 0 ||
        !grant->has_reference ||
        lxp_ct_memcmp(grant->reference_hash,
                      requirement->invoice_id, 32U) != 0)
        return LXP_ERR_GRANT_SCOPE_VIOLATION;
    if (lxp_ct_memcmp(grant->purpose_hash,
                      requirement->purpose_hash, 32U) != 0)
        return LXP_ERR_PURPOSE_MISMATCH;
    if (lxp_u128_add(grant_state->drawn_total,
                     receive->amount, &drawn) != LXP_OK ||
        (!grant->recurring &&
         lxp_u128_cmp(drawn, grant->allowance) > 0))
        return LXP_ERR_GRANT_EXHAUSTED;
    return LXP_OK;
}

static lxp_result build_receive_receipt(
    const lxp_receive *receive,
    const lxp_send_receipt_projection *projection,
    const uint8_t previous_state_root[32],
    const uint8_t resulting_state_root[32],
    lxp_gateway_receive_context *context,
    lxp_receipt *receipt)
{
    uint8_t encoded[1024];
    uint8_t authorization[512];
    uint8_t transaction_id[32];
    uint8_t authorization_hash[32];
    size_t encoded_length = 0U;
    size_t authorization_length = 0U;
    lxp_ledger_receipt_input input;
    lxp_result status = lxp_receive_encode(
        receive, encoded, sizeof(encoded), &encoded_length);
    if (status == LXP_OK)
        status = lxp_hash_activity_id(
            encoded, encoded_length, transaction_id);
    if (status == LXP_OK)
        status = lxp_receive_authorization_message(
            receive, authorization, sizeof(authorization),
            &authorization_length);
    if (status == LXP_OK)
        status = lxp_hash_domain(
            LXP_DOMAIN_SIGNATURE_PREIMAGE, authorization,
            authorization_length, authorization_hash);
    if (status != LXP_OK) return status;
    (void)memset(&input, 0, sizeof(input));
    (void)memcpy(input.transaction_id, transaction_id, 32U);
    input.operation = (uint8_t)LX_ASSET_RECEIVE;
    input.global_sequence = context->global_sequence;
    (void)memcpy(input.asset, receive->asset, 32U);
    input.amount = receive->amount;
    (void)memcpy(input.from, receive->from, 32U);
    input.from_balance_before = projection->from_before;
    input.from_balance_after = projection->from_after;
    input.from_sequence = receive->receiver_sequence;
    (void)memcpy(input.to, receive->to, 32U);
    input.to_balance_before = projection->to_before;
    input.to_balance_after = projection->to_after;
    (void)memcpy(input.transfer_set_root,
                 projection->transfer_set_root, 32U);
    (void)memcpy(input.authorization_hash, authorization_hash, 32U);
    (void)memcpy(input.context_hash, receive->context_hash, 32U);
    (void)memcpy(input.previous_state_root, previous_state_root, 32U);
    (void)memcpy(input.resulting_state_root, resulting_state_root, 32U);
    (void)memcpy(input.batch_id, context->batch_id, 32U);
    input.timestamp = context->receive_environment->batch_timestamp;
    input.leg_count = 1U;
    status = lxp_ledger_receipt_build(receipt, &input);
    if (status == LXP_OK)
        status = lxp_receipt_sign(
            receipt, context->sequencer_private_key, context->arena);
    return status;
}

static lxp_result gateway_receive_claim_locked(
    const lxp_payment_requirement *requirement,
    const lxp_receive *receive,
    lxp_gateway_receive_context *context,
    lxp_receipt *receipt)
{
    lxp_grant_state *grant_state;
    lxp_grant_state *existing_grant;
    lxp_send_receipt_projection projection;
    lxp_transfer_leg leg;
    receive_transaction transaction;
    uint8_t previous_state_root[32];
    uint8_t resulting_state_root[32];
    bool settled = false;
    lxp_result status;
    if (context->receive_environment->accounts->count >
            LX_ACCOUNT_REGISTRY_CAPACITY ||
        context->receive_environment->grants->count >
            LXP_GRANT_STORE_CAPACITY ||
        (context->receive_environment->idempotency != NULL &&
         context->receive_environment->idempotency->count >
            LXP_SEND_STORE_CAPACITY) ||
        context->invoices->count > LXP_GATEWAY_INVOICE_CAPACITY)
        return LXP_ERR_LENGTH_LIMIT;
    status = lxp_payment_requirement_verify(
        requirement, context->receive_environment->network_id,
        context->service_public_key);
    if (status != LXP_OK) return status;
    status = lxp_gateway_invoice_state_locked(
        context->invoices, requirement->invoice_id,
        receive->idempotency_key, receipt, &settled);
    if (status != LXP_OK) return status;
    if (settled) return LXP_ERR_IDEMPOTENT_REPLAY;
    if (context->invoices->count >= LXP_GATEWAY_INVOICE_CAPACITY ||
        context->receive_environment->idempotency == NULL ||
        context->receive_environment->idempotency->count >=
            LXP_SEND_STORE_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    if (lxp_ct_memcmp(receive->grant_id,
                      receive->payer_grant.grant_id, 32U) != 0)
        return LXP_ERR_GRANT_SCOPE_VIOLATION;
    (void)memset(&leg, 0, sizeof(leg));
    leg.from = account_for(context->receive_environment->accounts,
                           receive->from);
    leg.to = account_for(context->receive_environment->accounts, receive->to);
    if (leg.from == NULL || leg.to == NULL)
        return LXP_ERR_GRANT_SCOPE_VIOLATION;
    (void)memcpy(leg.asset_id, receive->asset, 32U);
    leg.amount = receive->amount;
    leg.reason = LXP_REASON_PAYMENT;
    (void)memset(&transaction, 0, sizeof(transaction));
    transaction.grant_count = context->receive_environment->grants->count;
    transaction.idempotency_count =
        context->receive_environment->idempotency->count;
    transaction.invoice_count = context->invoices->count;
    transaction.arena_mark = lxp_arena_mark(context->arena);
    existing_grant = grant_for(context->receive_environment->grants,
                               receive->grant_id);
    if (existing_grant != NULL) {
        transaction.existing_grant = true;
        transaction.grant_index = (size_t)(existing_grant -
            context->receive_environment->grants->grants);
        transaction.grant_before = *existing_grant;
    }
    status = lxp_journal_open(&leg, 1U, &transaction.balances);
    if (status != LXP_OK) return status;
    status = gateway_grant_present_locked(
        &receive->payer_grant,
        context->receive_environment->accounts,
        context->receive_environment->grants);
    if (status != LXP_OK)
        return receive_transaction_abort(
            context, &transaction, receipt, status);
#ifdef LXP_TESTING
    status = transaction_boundary(LXP_GATEWAY_AFTER_GRANT_WRITE);
    if (status != LXP_OK)
        return receive_transaction_abort(
            context, &transaction, receipt, status);
#endif
    grant_state = grant_for(
        context->receive_environment->grants, receive->grant_id);
    status = lxp_gateway_grant_bounds_check(
        requirement, receive, grant_state,
        context->receive_environment);
    if (status != LXP_OK)
        return receive_transaction_abort(
            context, &transaction, receipt, status);
    status = lx_asset_state_root(
        context->assets, context->receive_environment->accounts,
        previous_state_root);
    if (status != LXP_OK)
        return receive_transaction_abort(
            context, &transaction, receipt, status);
    status = lxp_receive_execute(
        receive, context->receive_environment, &projection);
    if (status != LXP_OK)
        return receive_transaction_abort(
            context, &transaction, receipt, status);
#ifdef LXP_TESTING
    status = transaction_boundary(LXP_GATEWAY_AFTER_BALANCE_WRITE);
    if (status != LXP_OK)
        return receive_transaction_abort(
            context, &transaction, receipt, status);
    status = transaction_boundary(LXP_GATEWAY_AFTER_IDEMPOTENCY_WRITE);
    if (status != LXP_OK)
        return receive_transaction_abort(
            context, &transaction, receipt, status);
#endif
    status = lx_asset_state_root(
        context->assets, context->receive_environment->accounts,
        resulting_state_root);
    if (status != LXP_OK)
        return receive_transaction_abort(
            context, &transaction, receipt, status);
#ifdef LXP_TESTING
    status = transaction_boundary(LXP_GATEWAY_AFTER_STATE_ROOT);
    if (status != LXP_OK)
        return receive_transaction_abort(
            context, &transaction, receipt, status);
#endif
    status = build_receive_receipt(
        receive, &projection, previous_state_root, resulting_state_root,
        context, receipt);
    if (status != LXP_OK)
        return receive_transaction_abort(
            context, &transaction, receipt, status);
#ifdef LXP_TESTING
    status = transaction_boundary(LXP_GATEWAY_AFTER_RECEIPT_SIGN);
    if (status != LXP_OK)
        return receive_transaction_abort(
            context, &transaction, receipt, status);
#endif
    (void)memcpy(
        context->invoices->records[context->invoices->count].invoice_id,
        requirement->invoice_id, 32U);
    (void)memcpy(
        context->invoices->records[context->invoices->count].idempotency_key,
        receive->idempotency_key, 32U);
    context->invoices->records[context->invoices->count].receipt = *receipt;
    ++context->invoices->count;
#ifdef LXP_TESTING
    status = transaction_boundary(LXP_GATEWAY_AFTER_INVOICE_WRITE);
    if (status != LXP_OK)
        return receive_transaction_abort(
            context, &transaction, receipt, status);
#endif
    status = lxp_journal_commit(&transaction.balances);
    return status == LXP_OK ? LXP_OK : receive_transaction_abort(
        context, &transaction, receipt, LXP_FATAL_INVARIANT);
}

lxp_result lxp_gateway_receive_claim(
    const lxp_payment_requirement *requirement,
    const lxp_receive *receive,
    lxp_gateway_receive_context *context,
    lxp_receipt *receipt)
{
    lxp_result status;
    if (requirement == NULL || receive == NULL || context == NULL ||
        context->assets == NULL || context->receive_environment == NULL ||
        context->receive_environment->accounts == NULL ||
        context->receive_environment->grants == NULL ||
        context->invoices == NULL || context->service_public_key == NULL ||
        context->sequencer_private_key == NULL || context->arena == NULL ||
        receipt == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_gateway_registry_enter(
        context->invoices, context->receive_environment->accounts);
    if (status != LXP_OK) return status;
    status = gateway_receive_claim_locked(
        requirement, receive, context, receipt);
    return lxp_gateway_registry_leave(context->invoices) == LXP_OK ? status :
        LXP_FATAL_INVARIANT;
}
