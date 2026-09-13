#include "layerx/lxp_batch.h"

#include <stdio.h>
#include <string.h>

static int read_decimal(const char *text, uint64_t maximum, uint64_t *out)
{
    uint64_t value = 0U;
    if (text == NULL || text[0] == '\0') return 1;
    while (*text != '\0') {
        uint64_t digit;
        if (*text < '0' || *text > '9') return 1;
        digit = (uint64_t)(*text - '0');
        if (value > maximum / 10U ||
            (value == maximum / 10U && digit > maximum % 10U)) return 1;
        value = value * 10U + digit;
        ++text;
    }
    *out = value;
    return 0;
}

static int read_digest(const char *text, uint8_t out[32])
{
    static const char digits[] = "0123456789abcdef";
    size_t index;
    if (strlen(text) != 64U) return 1;
    for (index = 0U; index < 32U; ++index) {
        const char *high = strchr(digits, text[index * 2U]);
        const char *low = strchr(digits, text[index * 2U + 1U]);
        if (high == NULL || low == NULL) return 1;
        out[index] = (uint8_t)((size_t)(high - digits) * 16U +
                                (size_t)(low - digits));
    }
    return 0;
}

int main(int argc, char **argv)
{
    lxp_batch_header header = {0};
    lxp_batch_header decoded;
    lxp_arena arena;
    lxp_byte_span encoded;
    lxp_byte_span reproduced;
    uint8_t storage[2U * LXP_BATCH_HEADER_ENCODED_SIZE];
    uint64_t version;
    uint64_t network;
    size_t index;
    if (argc != 8 ||
        read_decimal(argv[1], UINT16_MAX, &version) != 0 ||
        read_decimal(argv[2], UINT32_MAX, &network) != 0 ||
        read_decimal(argv[3], UINT64_MAX, &header.timestamp_ms) != 0 ||
        read_digest(argv[4], header.previous_state_root) != 0 ||
        read_digest(argv[5], header.resulting_state_root) != 0 ||
        read_digest(argv[6], header.receipt_merkle_root) != 0 ||
        read_digest(argv[7], header.sequencer_id) != 0) return 1;
    header.protocol_version = (uint16_t)version;
    header.network_id = (uint32_t)network;
    header.epoch = 1U;
    header.batch_number = 1U;
    header.first_sequence = 1U;
    header.last_sequence = 1U;
    (void)memset(header.data_availability_root, 9, 32U);
    if (lxp_arena_init(&arena, storage, sizeof(storage)) != LXP_OK ||
        lxp_batch_header_encode(&header, &arena, &encoded) != LXP_OK ||
        lxp_batch_header_decode(encoded.bytes, encoded.length, &decoded) != LXP_OK ||
        lxp_batch_header_encode(&decoded, &arena, &reproduced) != LXP_OK ||
        reproduced.length != encoded.length ||
        memcmp(encoded.bytes, reproduced.bytes, encoded.length) != 0) return 1;
    for (index = 0U; index < encoded.length; ++index)
        if (printf("%02x", encoded.bytes[index]) < 0) return 1;
    return putchar('\n') == EOF ? 1 : 0;
}
