#include "layerx/lxp_ledger.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include <openssl/evp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void require(int ok) { if (!ok) exit(1); }
static EVP_PKEY *key(unsigned char value, unsigned char public_key[32]) {
    unsigned char seed[32];
    size_t length = 32;
    memset(seed, value, sizeof(seed));
    EVP_PKEY *result = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, seed, sizeof(seed));
    require(result != NULL && EVP_PKEY_get_raw_public_key(result, public_key, &length) == 1 && length == 32);
    return result;
}
static void sign(EVP_PKEY *key_value, lxp_domain_tag_id domain, const unsigned char *message,
                 size_t length, unsigned char signature[64]) {
    unsigned char digest[32];
    size_t signature_length = 64;
    require(lxp_hash_domain(domain, message, length, digest) == LXP_OK);
    EVP_MD_CTX *context = EVP_MD_CTX_new();
    require(context != NULL && EVP_DigestSignInit(context, NULL, NULL, NULL, key_value) == 1);
    require(EVP_DigestSign(context, signature, &signature_length, digest, sizeof(digest)) == 1 && signature_length == 64);
    EVP_MD_CTX_free(context);
}
static void dump(const char *directory, const char *name, const unsigned char *bytes, size_t length) {
    char path[4096];
    int n = snprintf(path, sizeof(path), "%s/%s", directory, name);
    require(n > 0 && (size_t)n < sizeof(path));
    FILE *file = fopen(path, "w");
    require(file != NULL);
    for (size_t i = 0; i < length; ++i) require(fprintf(file, "%02x", bytes[i]) == 2);
    require(fputc('\n', file) != EOF && fclose(file) == 0);
}
int main(int argc, char **argv) {
    unsigned char bytes[1024], grant_bytes[346], message[512];
    size_t length = 0, grant_length = 0, message_length = 0;
    lxp_receive receive = {0};
    lxp_send send = {0};
    require(argc == 2);
    memset(receive.from, 1, 32); memset(receive.to, 2, 32); memset(receive.asset, 3, 32);
    receive.amount.lo = 123; receive.receiver_sequence = 7;
    memset(receive.idempotency_key, 4, 32);
    lxp_payer_grant *grant = &receive.payer_grant;
    memcpy(grant->from, receive.from, 32); memcpy(grant->recipient, receive.to, 32); memcpy(grant->asset, receive.asset, 32);
    grant->per_draw_maximum.lo = 200; grant->allowance.lo = 1000;
    grant->expiration = 100; grant->revocation_sequence = 9;
    memset(grant->purpose_hash, 5, 32);
    EVP_PKEY *payer = key(42, grant->public_key);
    require(lxp_grant_authorization_message(grant, message, sizeof(message), &message_length) == LXP_OK);
    require(lxp_hash_authority(message, message_length, grant->grant_id) == LXP_OK);
    sign(payer, LXP_DOMAIN_AUTHORITY_HASH, message, message_length, grant->signature);
    memcpy(receive.grant_id, grant->grant_id, 32);
    require(lxp_hash_context_value(grant->purpose_hash, 32, receive.context_hash) == LXP_OK);
    lxp_send_authorization *auth = &receive.receiver_authorization;
    auth->kind = 1; memcpy(auth->controller, receive.to, 32);
    memcpy(auth->signed_context_hash, receive.context_hash, 32);
    auth->network_id = 17; auth->protocol_version = 3;
    EVP_PKEY *receiver = key(43, auth->public_key);
    require(lxp_receive_authorization_message(&receive, message, sizeof(message), &message_length) == LXP_OK);
    sign(receiver, LXP_DOMAIN_SIGNATURE_PREIMAGE, message, message_length, auth->signature);
    require(lxp_ed25519_verify(auth->public_key, auth->signature, LXP_DOMAIN_SIGNATURE_PREIMAGE, message, message_length) == LXP_OK);
    lx_account account = {0}; memcpy(account.id, grant->from, 32); memcpy(account.authority_key, grant->public_key, 32); account.has_authority_key = true;
    require(lxp_verify_payer_grant(grant, &account) == LXP_OK);
    require(lxp_payer_grant_encode(grant, grant_bytes, sizeof(grant_bytes), &grant_length) == LXP_OK && grant_length == sizeof(grant_bytes));
    require(lxp_receive_encode(&receive, bytes, sizeof(bytes), &length) == LXP_OK && length == 733);
    require(memcmp(bytes + 387, grant_bytes, grant_length) == 0);
    dump(argv[1], "native-1-6.hex", bytes, length);
    dump(argv[1], "native-1-7.hex", grant_bytes, grant_length);
    memcpy(send.from, receive.from, 32); memcpy(send.to, receive.to, 32); memcpy(send.asset, receive.asset, 32);
    send.amount.lo = 123; send.sequence = 7; memcpy(send.idempotency_key, receive.idempotency_key, 32);
    send.expires_at = 100; memset(send.context_hash, 5, 32);
    send.condition_count = 1; send.conditions[0].kind = 1; send.conditions[0].timestamp = 10;
    send.authorization.kind = 1; memcpy(send.authorization.controller, send.from, 32);
    memcpy(send.authorization.public_key, grant->public_key, 32);
    memcpy(send.authorization.signed_context_hash, send.context_hash, 32);
    send.authorization.network_id = 17; send.authorization.protocol_version = 3;
    require(lxp_send_authorization_message(&send, message, sizeof(message), &message_length) == LXP_OK);
    dump(argv[1], "native-send-authorization.hex", message, message_length);
    sign(payer, LXP_DOMAIN_SIGNATURE_PREIMAGE, message, message_length, send.authorization.signature);
    require(lxp_ed25519_verify(send.authorization.public_key, send.authorization.signature, LXP_DOMAIN_SIGNATURE_PREIMAGE, message, message_length) == LXP_OK);
    require(lxp_send_encode(&send, bytes, sizeof(bytes), &length) == LXP_OK);
    dump(argv[1], "native-1-5.hex", bytes, length);
    EVP_PKEY_free(payer); EVP_PKEY_free(receiver);
    return 0;
}
