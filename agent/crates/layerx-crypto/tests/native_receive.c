#include "layerx/lxp_ledger.h"
#include "layerx/lxp_crypto.h"
#include <stdio.h>
#include <string.h>

int main(int argc, char **argv) {
    unsigned char bytes[733], encoded[1024], message[512];
    size_t length = 0, message_length = 0;
    lxp_receive receive;
    lx_account account;
    if (argc != 2) return 1;
    FILE *file = fopen(argv[1], "r");
    if (file == NULL) return 2;
    for (size_t i = 0; i < sizeof(bytes); i++) {
        unsigned int byte;
        if (fscanf(file, "%2x", &byte) != 1) return 3;
        bytes[i] = (unsigned char)byte;
    }
    fclose(file);
    if (lxp_receive_decode(bytes, sizeof(bytes), &receive) != LXP_OK) return 4;
    if (lxp_receive_encode(&receive, encoded, sizeof(encoded), &length) != LXP_OK ||
        length != sizeof(bytes) || memcmp(bytes, encoded, length)) return 5;
    memset(&account, 0, sizeof(account));
    memcpy(account.id, receive.from, 32);
    memcpy(account.authority_key, receive.payer_grant.public_key, 32);
    account.has_authority_key = true;
    if (lxp_verify_payer_grant(&receive.payer_grant, &account) != LXP_OK) return 6;
    if (lxp_receive_authorization_message(&receive, message, sizeof(message), &message_length) != LXP_OK) return 7;
    if (lxp_ed25519_verify(receive.receiver_authorization.public_key,
        receive.receiver_authorization.signature, LXP_DOMAIN_SIGNATURE_PREIMAGE,
        message, message_length) != LXP_OK) return 8;
    receive.payer_grant.signature[0] ^= 1;
    if (lxp_verify_payer_grant(&receive.payer_grant, &account) == LXP_OK) return 9;
    puts("native receive roundtrip and grant/receiver signatures verified");
    return 0;
}
