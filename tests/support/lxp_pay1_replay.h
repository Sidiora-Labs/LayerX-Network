#ifndef LXP_PAY1_REPLAY_H
#define LXP_PAY1_REPLAY_H

#include "lxp_real_replay.h"

#define PAY_REQUIRE(x) do { if (!(x)) { fprintf(stderr, "PAY1 replay line %d\n", __LINE__); return 1; } } while (0)

static int pay1_replay_activities(lxp_real_replay_fixture *f, lxp_arena *arena,
                                lxp_byte_span activities[8])
{
    static const uint16_t ordinals[8] = {1U, 4U, 10U, 11U, 4U, 7U, 6U, 8U};
    uint8_t id[32], salt[32] = {9U}, target[32], recipient[32], digest[32];
    uint8_t payload[1024], message[512];
    char name[128];
    static const char hex[] = "0123456789abcdef";
    lxp_hash_context hash;
    lxp_receive receive;
    size_t length, message_length;
    lxp_hash_init(&hash);
    PAY_REQUIRE(lxp_hash_update(&hash, (const uint8_t *)"LX:ASSET:v1", 11U) == LXP_OK);
    PAY_REQUIRE(lxp_hash_update(&hash, f->authority.actor, 32U) == LXP_OK);
    PAY_REQUIRE(lxp_hash_update(&hash, salt, 32U) == LXP_OK);
    PAY_REQUIRE(lxp_hash_final(&hash, id) == LXP_OK);
    for (unsigned which = 0U; which < 2U; ++which) {
        const uint8_t *asset = which == 0U ? id : f->asset.asset_id;
        int prefix = snprintf(name, sizeof(name), "agent:did:key:alice:asset:");
        PAY_REQUIRE(prefix > 0 && (size_t)prefix + 64U < sizeof(name));
        for (size_t i = 0U; i < 32U; ++i) {
            name[(size_t)prefix + i * 2U] = hex[asset[i] >> 4U];
            name[(size_t)prefix + i * 2U + 1U] = hex[asset[i] & 15U];
        }
        PAY_REQUIRE(lx_account_id_from_string((const uint8_t *)name,
            (size_t)prefix + 64U, which == 0U ? target : recipient) == LXP_OK);
    }
    memset(&receive, 0, sizeof(receive));
    memcpy(receive.from, f->accounts.accounts[0].id, 32U);
    memcpy(receive.to, recipient, 32U);
    memcpy(receive.asset, f->asset.asset_id, 32U);
    memcpy(receive.payer_grant.from, receive.from, 32U);
    memcpy(receive.payer_grant.recipient, receive.to, 32U);
    memcpy(receive.payer_grant.asset, receive.asset, 32U);
    receive.payer_grant.per_draw_maximum.lo = 2U;
    receive.payer_grant.allowance.lo = 3U;
    receive.payer_grant.expiration = 100U;
    receive.payer_grant.purpose_hash[0] = 9U;
    memcpy(receive.payer_grant.public_key, f->public_key, 32U);
    PAY_REQUIRE(lxp_grant_authorization_message(&receive.payer_grant, message,
        sizeof(message), &message_length) == LXP_OK);
    PAY_REQUIRE(lxp_hash_authority(message, message_length, receive.payer_grant.grant_id) == LXP_OK);
    PAY_REQUIRE(lxp_hash_domain(LXP_DOMAIN_AUTHORITY_HASH, message, message_length, digest) == LXP_OK);
    PAY_REQUIRE(lxp_real_replay_sign(digest, receive.payer_grant.signature, f->public_key) == 0);
    for (size_t step = 0U; step < 8U; ++step) {
        memset(payload, 0, sizeof(payload));
        payload[1] = 1U;
        memcpy(payload + 2U, id, 32U);
        length = 34U;
        if (step == 0U) {
            memcpy(payload + 34U, salt, 32U);
            length = 66U;
            payload[length++] = 1U; payload[length++] = 'T';
            payload[length++] = 1U; payload[length++] = 'T';
            payload[length++] = 0U;
            PAY_REQUIRE(lxp_u128_to_be((lxp_u128){0U, 100U}, payload + length) == LXP_OK);
            length += 16U;
            payload[length++] = 1U; payload[length++] = 0U;
        } else if (step == 2U || step == 3U) {
            memcpy(payload + 34U, target, 32U);
            PAY_REQUIRE(lxp_u128_to_be((lxp_u128){0U, step == 2U ? 70U : 20U}, payload + 66U) == LXP_OK);
            length = 82U;
        } else if (step == 4U) memcpy(payload + 2U, f->asset.asset_id, 32U);
        else if (step == 5U) {
            PAY_REQUIRE(lxp_payer_grant_encode(&receive.payer_grant, payload, sizeof(payload), &length) == LXP_OK);
        } else if (step == 6U) {
            memcpy(receive.grant_id, receive.payer_grant.grant_id, 32U);
            receive.amount.lo = 2U;
            receive.idempotency_key[0] = (uint8_t)(step + 40U);
            PAY_REQUIRE(lxp_hash_context_value(receive.payer_grant.purpose_hash, 32U, receive.context_hash) == LXP_OK);
            receive.receiver_authorization.kind = LXP_AUTH_OWNER;
            receive.receiver_authorization.network_id = 7U;
            receive.receiver_authorization.protocol_version = 3U;
            memcpy(receive.receiver_authorization.controller, receive.to, 32U);
            memcpy(receive.receiver_authorization.public_key, f->public_key, 32U);
            memcpy(receive.receiver_authorization.signed_context_hash, receive.context_hash, 32U);
            PAY_REQUIRE(lxp_receive_authorization_message(&receive, message, sizeof(message), &message_length) == LXP_OK);
            PAY_REQUIRE(lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, message, message_length, digest) == LXP_OK);
            PAY_REQUIRE(lxp_real_replay_sign(digest, receive.receiver_authorization.signature, f->public_key) == 0);
            PAY_REQUIRE(lxp_receive_encode(&receive, payload, sizeof(payload), &length) == LXP_OK);
        } else if (step == 7U) {
            memcpy(payload + 2U, receive.payer_grant.grant_id, 32U);
            payload[41] = 7U;
            length = 42U;
        }
        f->activity.activity_type = ((uint32_t)LXP_MODULE_ASSET << 16U) | ordinals[step];
        f->activity.account_sequence = step;
        f->activity.idempotency_key[0] = (uint8_t)(step + 40U);
        f->activity.payload = (lxp_byte_span){payload, length};
        PAY_REQUIRE(lxp_hash_payload(payload, length, f->activity.payload_hash) == LXP_OK);
        PAY_REQUIRE(lxp_activity_signing_preimage(&f->activity, digest) == LXP_OK);
        PAY_REQUIRE(lxp_real_replay_sign(digest, f->signature, f->public_key) == 0);
        size_t mark = lxp_arena_mark(arena);
        lxp_byte_span encoded;
        PAY_REQUIRE(lxp_activity_encode(&f->activity, arena, &encoded) == LXP_OK);
        uint8_t *saved = malloc(encoded.length);
        PAY_REQUIRE(saved != NULL);
        memcpy(saved, encoded.bytes, encoded.length);
        activities[step] = (lxp_byte_span){saved, encoded.length};
        PAY_REQUIRE(lxp_arena_reset(arena, mark) == LXP_OK);
    }
    return 0;
}

static int pay1_guarantor_replay(void)
{
    lxp_real_replay_fixture *builder = calloc(1U, sizeof(*builder));
    lxp_real_replay_fixture *verifier = calloc(1U, sizeof(*verifier));
    uint8_t *memory = malloc(16U * 1024U * 1024U);
    lxp_arena arena;
    lxp_byte_span activities[8];
    lxp_batch_body body;
    lxp_replay_batch_result replay;
    lxp_batch_roots roots;
    PAY_REQUIRE(builder != NULL && verifier != NULL && memory != NULL);
    PAY_REQUIRE(lxp_arena_init(&arena, memory, 16U * 1024U * 1024U) == LXP_OK);
    PAY_REQUIRE(lxp_real_replay_init(builder) == 0 && lxp_real_replay_init(verifier) == 0);
    PAY_REQUIRE(pay1_replay_activities(builder, &builder->arena, activities) == 0);
    for (size_t step = 0U; step < 8U; ++step) {
        lxp_receipt receipt;
        PAY_REQUIRE(lxp_arena_reset(&arena, 0U) == LXP_OK);
        PAY_REQUIRE(lxp_real_replay_build(builder, step + 1U, &activities[step], 1U,
            NULL, 0U, &arena, &body) == 0);
        verifier->execution.batch_number = step + 1U;
        PAY_REQUIRE(lxp_replay_batch(&verifier->engine, &body,
            verifier->kernel.current_state_root, &arena, &replay) == LXP_OK);
        PAY_REQUIRE(replay.activity_count == 1U && replay.receipt_count == 2U);
        PAY_REQUIRE(lxp_guarantor_recompute_roots(&body, &replay, &arena, &roots) == LXP_OK);
        PAY_REQUIRE(memcmp(builder->kernel.current_state_root, verifier->kernel.current_state_root, 32U) == 0);
        PAY_REQUIRE(replay.outputs[0].result_code == LXP_OK);
        PAY_REQUIRE(lxp_receipt_decode(replay.encoded_receipts[0].bytes,
            replay.encoded_receipts[0].length, true, &receipt) == LXP_OK);
        PAY_REQUIRE(lxp_receipt_verify(&receipt, verifier->public_key, &arena) == LXP_OK);
        replay.resulting_state_root[0] ^= 1U;
        PAY_REQUIRE(lxp_guarantor_recompute_roots(&body, &replay, &arena, &roots) != LXP_OK);
    }
    PAY_REQUIRE(builder->accounts.count == verifier->accounts.count && builder->accounts.count == 5U);
    PAY_REQUIRE(builder->accounts.accounts[2].balance.lo == 50U);
    PAY_REQUIRE(builder->accounts.accounts[3].balance.lo == 50U);
    PAY_REQUIRE(builder->accounts.accounts[4].balance.lo == 2U);
    PAY_REQUIRE(lxp_state_store_destroy(&builder->state) == LXP_OK);
    PAY_REQUIRE(lxp_state_store_destroy(&verifier->state) == LXP_OK);
    for (size_t i = 0U; i < 8U; ++i) free((void *)activities[i].bytes);
    free(memory); free(verifier); free(builder);
    return 0;
}
#undef PAY_REQUIRE
#endif
