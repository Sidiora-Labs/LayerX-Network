#define main existing_receive_harness
#include "../../../../tests/ledger/test_receive.c"
#undef main
#include "layerx/lxp_crypto.h"
#include <stdio.h>

static void print_hex(const uint8_t *bytes, size_t length)
{
    size_t index;
    for (index = 0U; index < length; ++index) printf("%02x", bytes[index]);
    putchar('\n');
}

int main(void)
{
    static const uint8_t payer_seed[32] = { 1U };
    static const uint8_t receiver_seed[32] = { 2U };
    lxp_receive receive = {0};
    lxp_receive decoded;
    lx_account payer = {0};
    uint8_t encoded[1024];
    uint8_t message[512];
    size_t length;
    size_t encoded_length;
    receive.from[0] = 1U;
    receive.to[0] = 2U;
    receive.asset[0] = 3U;
    receive.amount = (lxp_u128){ 1U, 25U };
    receive.receiver_sequence = UINT64_MAX;
    receive.idempotency_key[0] = 4U;
    receive.context_hash[0] = 5U;
    receive.receiver_authorization.kind = LXP_AUTH_OWNER;
    memcpy(receive.receiver_authorization.controller, receive.to, 32U);
    memcpy(receive.receiver_authorization.signed_context_hash, receive.context_hash, 32U);
    receive.receiver_authorization.network_id = 7U;
    receive.receiver_authorization.protocol_version = LXP_PROTOCOL_VERSION;
    memcpy(receive.payer_grant.from, receive.from, 32U);
    memcpy(receive.payer_grant.recipient, receive.to, 32U);
    memcpy(receive.payer_grant.asset, receive.asset, 32U);
    receive.payer_grant.per_draw_maximum = (lxp_u128){ 1U, 30U };
    receive.payer_grant.allowance = (lxp_u128){ 2U, 50U };
    receive.payer_grant.recurring = true;
    receive.payer_grant.window_length = 3600U;
    receive.payer_grant.expiration = UINT64_MAX;
    receive.payer_grant.purpose_hash[0] = 6U;
    receive.payer_grant.revocation_sequence = 9U;
    if (public_from_seed(payer_seed, receive.payer_grant.public_key) != 0 ||
        public_from_seed(receiver_seed, receive.receiver_authorization.public_key) != 0 ||
        sign_grant(&receive.payer_grant, payer_seed) != 0) return 1;
    memcpy(receive.grant_id, receive.payer_grant.grant_id, 32U);
    memcpy(payer.id, receive.from, 32U);
    memcpy(payer.authority_key, receive.payer_grant.public_key, 32U);
    payer.has_authority_key = true;
    if (lxp_verify_payer_grant(&receive.payer_grant, &payer) != LXP_OK ||
        sign_receive(&receive, receiver_seed) != 0 ||
        lxp_receive_encode(&receive, encoded, sizeof(encoded), &encoded_length) != LXP_OK ||
        lxp_receive_decode(encoded, encoded_length, &decoded) != LXP_OK ||
        memcmp(&receive, &decoded, sizeof(receive)) != 0) return 1;
    print_hex(encoded, encoded_length);
    if (lxp_grant_authorization_message(&receive.payer_grant, message, sizeof(message), &length) != LXP_OK) return 1;
    print_hex(message, length);
    if (lxp_receive_authorization_message(&receive, message, sizeof(message), &length) != LXP_OK ||
        lxp_ed25519_verify(receive.receiver_authorization.public_key, receive.receiver_authorization.signature,
                           LXP_DOMAIN_SIGNATURE_PREIMAGE, message, length) != LXP_OK) return 1;
    print_hex(message, length);
    return 0;
}
