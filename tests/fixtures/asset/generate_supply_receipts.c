#include "layerx/lxp_receipt.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint8_t arena_bytes[4U * LXP_MAX_ACTIVITY_BYTES];
static uint8_t input_bytes[LXP_MAX_ACTIVITY_BYTES];

int main(int argc, char **argv)
{
    lxp_receipt original;
    lxp_receipt receipt;
    lxp_receipt decoded;
    lxp_arena arena;
    lxp_byte_span encoded;
    EVP_PKEY *key;
    FILE *file;
    size_t length;
    size_t public_length = 32U;
    uint8_t public_key[32];
    uint8_t seed[32];
    uint8_t operation;
    char path[1024];
    int written;
    if (argc != 3 || lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK)
        return 1;
    (void)memset(seed, 0x33, sizeof(seed));
    key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, seed, sizeof(seed));
    if (key == NULL || EVP_PKEY_get_raw_public_key(key, public_key, &public_length) != 1 ||
        public_length != sizeof(public_key)) return 1;
    EVP_PKEY_free(key);
    file = fopen(argv[1], "rb");
    if (file == NULL) return 1;
    length = fread(input_bytes, 1U, sizeof(input_bytes), file);
    if (ferror(file) || !feof(file) || fclose(file) != 0 ||
        lxp_receipt_decode(input_bytes, length, true, &original) != LXP_OK ||
        lxp_receipt_verify(&original, public_key, &arena) != LXP_OK ||
        original.supply_binding_version != 1U || original.module_id != 1U ||
        original.operation != 5U || original.effects.count != 0U)
        return 1;
    for (operation = 2U; operation <= 3U; ++operation) {
        receipt = original;
        receipt.operation = operation;
        receipt.amount = (lxp_u128){0U, 0U};
        (void)memset(receipt.from, 0, sizeof(receipt.from));
        (void)memset(receipt.to, 0, sizeof(receipt.to));
        receipt.from_sequence = 0U;
        receipt.from_balance_before = (lxp_u128){0U, 0U};
        receipt.from_balance_after = (lxp_u128){0U, 0U};
        receipt.to_balance_before = (lxp_u128){0U, 0U};
        receipt.to_balance_after = (lxp_u128){0U, 0U};
        (void)memset(receipt.transfer_set_root, 0, sizeof(receipt.transfer_set_root));
        if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
            lxp_receipt_sign(&receipt, seed, &arena) != LXP_OK ||
            lxp_receipt_encode(&receipt, true, &arena, &encoded) != LXP_OK ||
            lxp_receipt_decode(encoded.bytes, encoded.length, true, &decoded) != LXP_OK ||
            lxp_receipt_verify(&decoded, public_key, &arena) != LXP_OK)
            return 1;
        written = snprintf(path, sizeof(path), "%s/supply-%s.receipt", argv[2],
                            operation == 2U ? "pause" : "unpause");
        if (written < 0 || (size_t)written >= sizeof(path)) return 1;
        file = fopen(path, "wb");
        if (file == NULL || fwrite(encoded.bytes, 1U, encoded.length, file) != encoded.length ||
            fclose(file) != 0) return 1;
    }
    return 0;
}
