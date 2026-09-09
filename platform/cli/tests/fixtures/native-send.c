#include "layerx/lxp_ledger.h"
#include <stdio.h>
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
    memset(send.authorization.public_key, 6, 32);
    memset(send.authorization.signature, 7, 64);
    memcpy(send.authorization.signed_context_hash, send.context_hash, 32);
    send.authorization.network_id = 402;
    send.authorization.protocol_version = 3;
    if (lxp_send_encode(&send, bytes, sizeof(bytes), &length) != LXP_OK)
        return 1;
    if (fwrite(bytes, 1, length, stdout) != length) return 2;
    if (lxp_send_authorization_message(&send, bytes, sizeof(bytes), &length) != LXP_OK)
        return 3;
    return fwrite(bytes, 1, length, stdout) == length ? 0 : 4;
}
