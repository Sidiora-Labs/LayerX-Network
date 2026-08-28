#include "layerx/program.h"

#include <stdio.h>
#include <string.h>

static int hex_digit(int byte)
{
    if (byte >= '0' && byte <= '9') return byte - '0';
    if (byte >= 'a' && byte <= 'f') return byte - 'a' + 10;
    return -1;
}

static size_t fixture_bytes(uint8_t *out, size_t capacity)
{
    static const char marker[] = "encoded_hex = \"";
    FILE *stream = fopen("programs/sdk/vectors/capability-boundary.kvx", "rb");
    char line[1024];
    char *hex;
    size_t length = 0U;
    if (stream == NULL) return 0U;
    while (fgets(line, sizeof(line), stream) != NULL) {
        hex = strstr(line, marker);
        if (hex == NULL) continue;
        hex += sizeof(marker) - 1U;
        while (hex[0] != '\0' && hex[0] != '"') {
            int high, low;
            if (hex[1] == '\0' || length >= capacity) {
                (void)fclose(stream);
                return 0U;
            }
            high = hex_digit((unsigned char)hex[0]);
            low = hex_digit((unsigned char)hex[1]);
            if (high < 0 || low < 0) {
                (void)fclose(stream);
                return 0U;
            }
            out[length++] = (uint8_t)((high << 4) | low);
            hex += 2;
        }
        break;
    }
    (void)fclose(stream);
    return length;
}

int main(void)
{
    lxp_program_capability storage[3];
    lxp_program_capability_set set;
    lxp_program_capability grant;
    lxp_program_id program;
    lxp_program_asset asset;
    lxp_program_account account;
    uint8_t expected[256];
    uint8_t actual[256];
    size_t expected_length;
    size_t actual_length = 0U;
    (void)memset(program.bytes, 0x11, sizeof(program.bytes));
    (void)memset(asset.bytes, 0x22, sizeof(asset.bytes));
    (void)memset(account.bytes, 0x33, sizeof(account.bytes));
    expected_length = fixture_bytes(expected, sizeof(expected));
    if (expected_length == 0U || LXP_PROGRAM_MAX_CAPABILITY_BYTES != 65535 ||
        LXP_PROGRAM_MAX_CAPABILITIES != 238 ||
        LXP_PROGRAM_MAX_CANONICAL_CAPABILITY_SET_BYTES != 65452 ||
        LXP_PROGRAM_MAX_EVENTS_PER_ACTIVITY != 64 ||
        lxp_program_capability_set_init(&set, storage, 3U) != LXP_PROGRAM_OK ||
        lxp_program_capability_set_push(
            &set, lxp_program_capability_emit_event()) != LXP_PROGRAM_OK ||
        lxp_program_capability_call(program, &grant) != LXP_PROGRAM_OK ||
        lxp_program_capability_set_push(&set, grant) != LXP_PROGRAM_OK ||
        lxp_program_capability_transfer_402(
            asset, account, (lxp_program_amount){0U, 7U}, &grant) != LXP_PROGRAM_OK ||
        lxp_program_capability_set_push(&set, grant) != LXP_PROGRAM_OK ||
        lxp_program_capability_set_encode(
            &set, actual, sizeof(actual), &actual_length) != LXP_PROGRAM_OK ||
        actual_length != expected_length ||
        memcmp(actual, expected, actual_length) != 0)
        return 1;
    return 0;
}
