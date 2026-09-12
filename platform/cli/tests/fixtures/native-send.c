#include "layerx/lxp_ledger.h"
#include <stdio.h>
#include <openssl/evp.h>
#include <string.h>

int main(void)
{
    lxp_send send = {0};
    unsigned char bytes[512];
    size_t length = 0;
    memset(send.from, 1, 32);
    memset(send.to, 2, 32);
    memset(send.asset, 3, 32);
    send.amount.hi = UINT64_C(0x8000000000000000);
    send.amount.lo = 257;
    send.sequence = UINT64_C(0x0102030405060708);
    memset(send.idempotency_key, 4, 32);
    send.expires_at = 2000;
    memset(send.context_hash, 5, 32);
    send.condition_count = 2;
    send.conditions[0].kind = 1;
    send.conditions[0].timestamp = 1000;
    send.conditions[1].kind = 2;
    send.conditions[1].timestamp = 2000;
    send.authorization.kind = 1;
    memcpy(send.authorization.controller, send.from, 32);

    memcpy(send.authorization.signed_context_hash, send.context_hash, 32);
    send.authorization.network_id = 402;
    send.authorization.protocol_version = 3;
    unsigned char seed[32], digest[32];
    unsigned int digest_length = 0;
    size_t public_length = 32, signature_length = 64;
    memset(seed, 6, sizeof(seed));
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, seed, sizeof(seed));
    EVP_MD_CTX *hash = EVP_MD_CTX_new(), *sign = EVP_MD_CTX_new();
    static const unsigned char domain[] = "LXP/v1/signature-preimage";
    if (!key || !hash || !sign || EVP_PKEY_get_raw_public_key(key, send.authorization.public_key, &public_length) != 1 || public_length != 32) return 5;
    if (lxp_send_authorization_message(&send, bytes, sizeof(bytes), &length) != LXP_OK) return 6;
    if (EVP_DigestInit_ex(hash, EVP_sha256(), NULL) != 1 ||
        EVP_DigestUpdate(hash, domain, sizeof(domain)) != 1 ||
        EVP_DigestUpdate(hash, bytes, length) != 1 ||
        EVP_DigestFinal_ex(hash, digest, &digest_length) != 1 || digest_length != 32 ||
        EVP_DigestSignInit(sign, NULL, NULL, NULL, key) != 1 ||
        EVP_DigestSign(sign, send.authorization.signature, &signature_length, digest, sizeof(digest)) != 1 || signature_length != 64) return 7;
    EVP_MD_CTX_free(hash); EVP_MD_CTX_free(sign); EVP_PKEY_free(key);
    if (lxp_send_encode(&send, bytes, sizeof(bytes), &length) != LXP_OK)
        return 1;
    if (fwrite(bytes, 1, length, stdout) != length) return 2;
    if (lxp_send_authorization_message(&send, bytes, sizeof(bytes), &length) != LXP_OK)
        return 3;
    return fwrite(bytes, 1, length, stdout) == length ? 0 : 4;
}
