#include "layerx/lxp_gateway.h"
#include "lxp_gateway_internal.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

#include <stdlib.h>
#include <string.h>
#ifdef LXP_TESTING
#include <sched.h>
#endif

typedef struct send_transaction {
    lxp_ledger_journal balances;
    size_t send_count;
    size_t invoice_count;
    size_t arena_mark;
} send_transaction;

enum {
    LXP_GATEWAY_REGISTRY_ZERO = 0,
    LXP_GATEWAY_REGISTRY_READY = 1,
    LXP_GATEWAY_REGISTRY_DESTROYING = 2,
    LXP_GATEWAY_REGISTRY_DESTROYED = 3
};

#ifdef LXP_TESTING
static _Thread_local lxp_gateway_transaction_boundary test_failure_boundary;
static atomic_bool test_pause_before_activation;
static atomic_bool test_activation_paused;
void lxp_gateway_send_test_fail_after(
    lxp_gateway_transaction_boundary boundary)
{
    test_failure_boundary = boundary;
}

void lxp_gateway_registry_test_pause_before_activation(void)
{
    atomic_store(&test_activation_paused, false);
    atomic_store(&test_pause_before_activation, true);
}

bool lxp_gateway_registry_test_activation_paused(void)
{
    return atomic_load(&test_activation_paused);
}

void lxp_gateway_registry_test_release_activation(void)
{
    atomic_store(&test_pause_before_activation, false);
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

static lxp_result send_transaction_abort(
    lxp_gateway_settlement_context *context, send_transaction *transaction,
    lxp_receipt *receipt, lxp_result failure)
{
    lxp_result rollback = lxp_journal_rollback(&transaction->balances);
    lxp_result arena_reset;
    size_t send_end = context->send_environment->store->count <
            LXP_SEND_STORE_CAPACITY ? context->send_environment->store->count :
            LXP_SEND_STORE_CAPACITY;
    size_t invoice_end = context->invoices->count <
            LXP_GATEWAY_INVOICE_CAPACITY ? context->invoices->count :
            LXP_GATEWAY_INVOICE_CAPACITY;
    if (send_end > transaction->send_count)
        (void)memset(&context->send_environment->store->records[
                         transaction->send_count], 0,
                     (send_end - transaction->send_count) *
                         sizeof(context->send_environment->store->records[0]));
    context->send_environment->store->count = transaction->send_count;
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

lxp_result lxp_gateway_invoice_state_locked(
    const lxp_gateway_invoice_registry *registry,
    const uint8_t invoice_id[32],
    const uint8_t idempotency_key[32],
    lxp_receipt *receipt,
    bool *settled)
{
    size_t i;
    if (registry == NULL || invoice_id == NULL || idempotency_key == NULL ||
        receipt == NULL || settled == NULL ||
        registry->count > LXP_GATEWAY_INVOICE_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    *settled = false;
    for (i = 0U; i < registry->count; ++i) {
        if (lxp_ct_memcmp(registry->records[i].invoice_id,
                          invoice_id, 32U) == 0 &&
            lxp_ct_memcmp(registry->records[i].idempotency_key,
                          idempotency_key, 32U) == 0) {
            *receipt = registry->records[i].receipt;
            *settled = true;
            return LXP_OK;
        }
    }
    return LXP_OK;
}

lxp_gateway_invoice_registry *lxp_gateway_invoice_registry_create(
    lx_account_registry *owner_accounts,
    lxp_result *status)
{
    lxp_gateway_invoice_registry *created;
    lxp_gateway_invoice_registry *unowned = NULL;
    if (status == NULL) return NULL;
    *status = LXP_ERR_NON_CANONICAL;
    if (owner_accounts == NULL) return NULL;
    created = (lxp_gateway_invoice_registry *)calloc(1U, sizeof(*created));
    if (created == NULL) {
        *status = LXP_ERR_ARENA_EXHAUSTED;
        return NULL;
    }
    atomic_init(&created->active_users, 0U);
    atomic_init(&created->lifecycle, LXP_GATEWAY_REGISTRY_ZERO);
    atomic_init(&created->owner_accounts, owner_accounts);
    if (pthread_mutex_init(&created->coordination_mutex, NULL) != 0) {
        free(created);
        *status = LXP_ERR_IO;
        return NULL;
    }
    if (!atomic_compare_exchange_strong(
            &owner_accounts->gateway_owner, &unowned, created)) {
        (void)pthread_mutex_destroy(&created->coordination_mutex);
        free(created);
        *status = LXP_ERR_SEQUENCE_REUSED;
        return NULL;
    }
    atomic_store(&created->lifecycle, LXP_GATEWAY_REGISTRY_READY);
    *status = LXP_OK;
    return created;
}

lxp_result lxp_gateway_invoice_registry_destroy(
    lxp_gateway_invoice_registry **registry)
{
    lxp_gateway_invoice_registry *owned;
    unsigned expected = LXP_GATEWAY_REGISTRY_READY;
    if (registry == NULL || *registry == NULL) return LXP_ERR_NON_CANONICAL;
    owned = *registry;
    if (!atomic_compare_exchange_strong(
            &owned->lifecycle, &expected,
            LXP_GATEWAY_REGISTRY_DESTROYING))
        return LXP_ERR_NON_CANONICAL;
    if (atomic_load(&owned->active_users) != 0U) {
        atomic_store(&owned->lifecycle, LXP_GATEWAY_REGISTRY_READY);
        return LXP_ERR_IO;
    }
    {
        lx_account_registry *owner_accounts =
            atomic_load(&owned->owner_accounts);
        if (owner_accounts == NULL ||
            atomic_load(&owner_accounts->gateway_owner) != owned) {
            atomic_store(&owned->lifecycle, LXP_GATEWAY_REGISTRY_READY);
            return LXP_FATAL_INVARIANT;
        }
    }
    if (pthread_mutex_destroy(&owned->coordination_mutex) != 0) {
        atomic_store(&owned->lifecycle, LXP_GATEWAY_REGISTRY_READY);
        return LXP_ERR_IO;
    }
    (void)memset(owned->records, 0, sizeof(owned->records));
    owned->count = 0U;
    atomic_store(&owned->owner_accounts, NULL);
    atomic_store(&owned->lifecycle, LXP_GATEWAY_REGISTRY_DESTROYED);
    *registry = NULL;
    return LXP_OK;
}

lxp_result lxp_gateway_registry_enter(
    lxp_gateway_invoice_registry *registry,
    lx_account_registry *accounts)
{
    lx_account_registry *owner_accounts;
    if (registry == NULL ||
        atomic_load(&registry->lifecycle) != LXP_GATEWAY_REGISTRY_READY)
        return LXP_ERR_NON_CANONICAL;
#ifdef LXP_TESTING
    if (atomic_load(&test_pause_before_activation)) {
        atomic_store(&test_activation_paused, true);
        while (atomic_load(&test_pause_before_activation)) (void)sched_yield();
        atomic_store(&test_activation_paused, false);
    }
#endif
    (void)atomic_fetch_add(&registry->active_users, 1U);
    if (atomic_load(&registry->lifecycle) != LXP_GATEWAY_REGISTRY_READY) {
        (void)atomic_fetch_sub(&registry->active_users, 1U);
        return LXP_ERR_IO;
    }
    owner_accounts = atomic_load(&registry->owner_accounts);
    if (owner_accounts == NULL ||
        (accounts != NULL && owner_accounts != accounts) ||
        atomic_load(&owner_accounts->gateway_owner) != registry) {
        (void)atomic_fetch_sub(&registry->active_users, 1U);
        return LXP_ERR_NON_CANONICAL;
    }
    if (pthread_mutex_lock(&registry->coordination_mutex) != 0) {
        (void)atomic_fetch_sub(&registry->active_users, 1U);
        return LXP_ERR_IO;
    }
    return LXP_OK;
}

lxp_result lxp_gateway_registry_leave(
    lxp_gateway_invoice_registry *registry)
{
    if (registry == NULL) return LXP_FATAL_INVARIANT;
    if (pthread_mutex_unlock(&registry->coordination_mutex) != 0)
        return LXP_FATAL_INVARIANT;
    if (atomic_fetch_sub(&registry->active_users, 1U) == 0U)
        return LXP_FATAL_INVARIANT;
    return LXP_OK;
}

lxp_result lxp_gateway_invoice_state(
    lxp_gateway_invoice_registry *registry,
    const uint8_t invoice_id[32],
    const uint8_t idempotency_key[32],
    lxp_receipt *receipt,
    bool *settled)
{
    lxp_result status;
    if (registry == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_gateway_registry_enter(registry, NULL);
    if (status != LXP_OK) return status;
    status = lxp_gateway_invoice_state_locked(
        registry, invoice_id, idempotency_key, receipt, settled);
    return lxp_gateway_registry_leave(registry) == LXP_OK ? status :
        LXP_FATAL_INVARIANT;
}

static lxp_result requirement_matches_send(
    const lxp_payment_requirement *requirement,
    const lxp_send *send,
    const lxp_send_environment *environment)
{
    size_t i;
    if (requirement == NULL || send == NULL || environment == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (send->expires_at == 0U || send->expires_at > requirement->expiry ||
        environment->batch_timestamp > requirement->expiry ||
        environment->batch_timestamp > send->expires_at)
        return LXP_ERR_EXPIRED;
    if (lxp_ct_memcmp(send->to, requirement->recipient, 32U) != 0 ||
        lxp_ct_memcmp(send->asset, requirement->asset, 32U) != 0 ||
        lxp_u128_cmp(send->amount, requirement->amount) != 0)
        return LXP_ERR_GRANT_SCOPE_VIOLATION;
    if (lxp_ct_memcmp(send->context_hash,
                      requirement->purpose_hash, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    for (i = 0U; i < send->condition_count; ++i) {
        uint32_t condition_bit;
        if (send->conditions[i].kind == 0U ||
            send->conditions[i].kind >= 32U)
            return LXP_ERR_CONDITION_UNMET;
        condition_bit = UINT32_C(1) << send->conditions[i].kind;
        if ((requirement->acceptable_conditions & condition_bit) == 0U)
            return LXP_ERR_CONDITION_UNMET;
    }
    return LXP_OK;
}

static lxp_result build_signed_receipt(
    const lxp_send *send,
    const lxp_send_receipt_projection *projection,
    const uint8_t previous_state_root[32],
    const uint8_t resulting_state_root[32],
    lxp_gateway_settlement_context *context,
    lxp_receipt *receipt)
{
    uint8_t encoded[512];
    uint8_t authorization[512];
    uint8_t transaction_id[32];
    uint8_t authorization_hash[32];
    size_t encoded_length = 0U;
    size_t authorization_length = 0U;
    lxp_ledger_receipt_input input;
    lxp_result status = lxp_send_encode(
        send, encoded, sizeof(encoded), &encoded_length);
    if (status == LXP_OK)
        status = lxp_hash_activity_id(
            encoded, encoded_length, transaction_id);
    if (status == LXP_OK)
        status = lxp_send_authorization_message(
            send, authorization, sizeof(authorization),
            &authorization_length);
    if (status == LXP_OK)
        status = lxp_hash_domain(
            LXP_DOMAIN_SIGNATURE_PREIMAGE, authorization,
            authorization_length, authorization_hash);
    if (status != LXP_OK) return status;
    (void)memset(&input, 0, sizeof(input));
    (void)memcpy(input.transaction_id, transaction_id, 32U);
    input.operation = (uint8_t)LX_ASSET_SEND;
    input.global_sequence = context->global_sequence;
    (void)memcpy(input.asset, send->asset, 32U);
    input.amount = send->amount;
    (void)memcpy(input.from, send->from, 32U);
    input.from_balance_before = projection->from_before;
    input.from_balance_after = projection->from_after;
    input.from_sequence = send->sequence;
    (void)memcpy(input.to, send->to, 32U);
    input.to_balance_before = projection->to_before;
    input.to_balance_after = projection->to_after;
    (void)memcpy(input.transfer_set_root,
                 projection->transfer_set_root, 32U);
    (void)memcpy(input.authorization_hash, authorization_hash, 32U);
    (void)memcpy(input.context_hash, send->context_hash, 32U);
    (void)memcpy(input.previous_state_root, previous_state_root, 32U);
    (void)memcpy(input.resulting_state_root, resulting_state_root, 32U);
    (void)memcpy(input.batch_id, context->batch_id, 32U);
    input.timestamp = context->send_environment->batch_timestamp;
    input.leg_count = 1U;
    status = lxp_ledger_receipt_build(receipt, &input);
    if (status == LXP_OK)
        status = lxp_receipt_sign(
            receipt, context->sequencer_private_key, context->arena);
    return status;
}

static lxp_result gateway_send_settle_locked(
    const lxp_payment_requirement *requirement,
    const lxp_send *send,
    lxp_gateway_settlement_context *context,
    lxp_receipt *receipt)
{
    lxp_send_receipt_projection projection;
    lxp_transfer_leg leg;
    send_transaction transaction;
    uint8_t previous_state_root[32];
    uint8_t resulting_state_root[32];
    bool settled = false;
    lxp_result status;
    if (context->send_environment->accounts->count >
            LX_ACCOUNT_REGISTRY_CAPACITY ||
        (context->send_environment->store != NULL &&
         context->send_environment->store->count > LXP_SEND_STORE_CAPACITY) ||
        context->invoices->count > LXP_GATEWAY_INVOICE_CAPACITY)
        return LXP_ERR_LENGTH_LIMIT;
    status = lxp_payment_requirement_verify(
        requirement, context->send_environment->network_id,
        context->service_public_key);
    if (status != LXP_OK) return status;
    status = lxp_gateway_invoice_state_locked(
        context->invoices, requirement->invoice_id,
        send->idempotency_key, receipt, &settled);
    if (status != LXP_OK) return status;
    if (settled) return LXP_ERR_IDEMPOTENT_REPLAY;
    if (context->invoices->count >= LXP_GATEWAY_INVOICE_CAPACITY ||
        context->send_environment->store == NULL ||
        context->send_environment->store->count >= LXP_SEND_STORE_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    status = requirement_matches_send(
        requirement, send, context->send_environment);
    if (status != LXP_OK) return status;
    status = lx_asset_state_root(
        context->assets, context->send_environment->accounts,
        previous_state_root);
    if (status != LXP_OK) return status;
    status = lxp_send_build_transfer_set(
        send, context->send_environment->accounts, &leg);
    if (status != LXP_OK) return status;
    (void)memset(&transaction, 0, sizeof(transaction));
    transaction.send_count = context->send_environment->store->count;
    transaction.invoice_count = context->invoices->count;
    transaction.arena_mark = lxp_arena_mark(context->arena);
    status = lxp_journal_open(&leg, 1U, &transaction.balances);
    if (status != LXP_OK) return status;
    status = lxp_send_execute(
        send, context->send_environment, &projection);
    if (status != LXP_OK)
        return send_transaction_abort(context, &transaction, receipt, status);
#ifdef LXP_TESTING
    status = transaction_boundary(LXP_GATEWAY_AFTER_BALANCE_WRITE);
    if (status != LXP_OK)
        return send_transaction_abort(context, &transaction, receipt, status);
    status = transaction_boundary(LXP_GATEWAY_AFTER_IDEMPOTENCY_WRITE);
    if (status != LXP_OK)
        return send_transaction_abort(context, &transaction, receipt, status);
#endif
    status = lx_asset_state_root(
        context->assets, context->send_environment->accounts,
        resulting_state_root);
    if (status != LXP_OK)
        return send_transaction_abort(context, &transaction, receipt, status);
#ifdef LXP_TESTING
    status = transaction_boundary(LXP_GATEWAY_AFTER_STATE_ROOT);
    if (status != LXP_OK)
        return send_transaction_abort(context, &transaction, receipt, status);
#endif
    status = build_signed_receipt(
        send, &projection, previous_state_root, resulting_state_root,
        context, receipt);
    if (status != LXP_OK)
        return send_transaction_abort(context, &transaction, receipt, status);
#ifdef LXP_TESTING
    status = transaction_boundary(LXP_GATEWAY_AFTER_RECEIPT_SIGN);
    if (status != LXP_OK)
        return send_transaction_abort(context, &transaction, receipt, status);
#endif
    (void)memcpy(
        context->invoices->records[context->invoices->count].invoice_id,
        requirement->invoice_id, 32U);
    (void)memcpy(
        context->invoices->records[context->invoices->count].idempotency_key,
        send->idempotency_key, 32U);
    context->invoices->records[context->invoices->count].receipt = *receipt;
    ++context->invoices->count;
#ifdef LXP_TESTING
    status = transaction_boundary(LXP_GATEWAY_AFTER_INVOICE_WRITE);
    if (status != LXP_OK)
        return send_transaction_abort(context, &transaction, receipt, status);
#endif
    status = lxp_journal_commit(&transaction.balances);
    return status == LXP_OK ? LXP_OK : send_transaction_abort(
        context, &transaction, receipt, LXP_FATAL_INVARIANT);
}

lxp_result lxp_gateway_send_settle(
    const lxp_payment_requirement *requirement,
    const lxp_send *send,
    lxp_gateway_settlement_context *context,
    lxp_receipt *receipt)
{
    lxp_result status;
    if (requirement == NULL || send == NULL || context == NULL ||
        context->assets == NULL || context->send_environment == NULL ||
        context->send_environment->accounts == NULL ||
        context->invoices == NULL || context->service_public_key == NULL ||
        context->sequencer_private_key == NULL || context->arena == NULL ||
        receipt == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_gateway_registry_enter(
        context->invoices, context->send_environment->accounts);
    if (status != LXP_OK) return status;
    status = gateway_send_settle_locked(requirement, send, context, receipt);
    return lxp_gateway_registry_leave(context->invoices) == LXP_OK ? status :
        LXP_FATAL_INVARIANT;
}

lxp_result lxp_gateway_receipt_return(
    const lxp_receipt *receipt,
    lxp_arena *arena,
    lxp_byte_span *canonical_receipt)
{
    if (receipt == NULL || arena == NULL || canonical_receipt == NULL ||
        lxp_ct_is_zero(receipt->sequencer_signature, 64U))
        return LXP_ERR_BAD_SIGNATURE;
    return lxp_receipt_encode(receipt, true, arena, canonical_receipt);
}
