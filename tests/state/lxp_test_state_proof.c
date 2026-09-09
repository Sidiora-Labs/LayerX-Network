#include "layerx/lxp_state_proof.h"
#include "layerx/lx_asset.h"
#include "layerx/programs.h"

#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static void hex(const uint8_t *bytes, size_t length)
{
    for (size_t i = 0U; i < length; ++i) printf("%02x", bytes[i]);
}

static void check(const lxp_kernel *kernel, uint16_t module, lxp_byte_span key, bool output,
                  bool *first)
{
    lxp_state_witness *proof = malloc(sizeof(*proof));
    lxp_state_witness *decoded = malloc(sizeof(*decoded));
    uint8_t *wire = malloc(LXP_STATE_WITNESS_MAX_BYTES);
    uint8_t root[32], altered[32];
    size_t length = 0U;
    assert(proof != NULL && decoded != NULL && wire != NULL);
    assert(lxp_state_root(kernel, root) == LXP_OK);
    assert(lxp_state_proof_build(kernel, module, key, proof) == LXP_OK);
    assert(lxp_state_proof_verify(proof, root) == LXP_OK);
    assert(lxp_state_proof_encode(proof, wire, LXP_STATE_WITNESS_MAX_BYTES, &length) == LXP_OK);
    assert(length == 26U + proof->key_length + proof->value_length +
           32U * ((size_t)proof->layer_a.depth + proof->layer_b.depth));
    assert(lxp_state_proof_decode(wire, length, decoded) == LXP_OK);
    assert(lxp_state_proof_verify(decoded, root) == LXP_OK);
    assert(lxp_state_proof_encode(proof, wire, length - 1U, &length) == LXP_ERR_LENGTH_LIMIT);
    for (size_t n = 0U; n < length; ++n)
        assert(lxp_state_proof_decode(wire, n, decoded) != LXP_OK);
    wire[length] = 0U;
    assert(lxp_state_proof_decode(wire, length + 1U, decoded) != LXP_OK);
    wire[1] = 1U;
    assert(lxp_state_proof_decode(wire, length, decoded) == LXP_ERR_VERSION_UNSUPPORTED);
    wire[1] = 2U;
    memcpy(altered, root, sizeof(altered));
    altered[0] ^= 1U;
    assert(lxp_state_proof_verify(proof, altered) == LXP_ERR_ROOT_MISMATCH);
    proof->key[0] ^= 1U;
    assert(lxp_state_proof_verify(proof, root) != LXP_OK);
    proof->key[0] ^= 1U;
    proof->value[0] ^= 1U;
    assert(lxp_state_proof_verify(proof, root) != LXP_OK);
    proof->value[0] ^= 1U;
    proof->layer_a.leaf_index = proof->layer_a.leaf_count;
    assert(lxp_state_proof_verify(proof, root) != LXP_OK);
    assert(lxp_state_proof_decode(wire, length, proof) == LXP_OK);
    ++proof->layer_a.depth;
    assert(lxp_state_proof_verify(proof, root) != LXP_OK);
    assert(lxp_state_proof_decode(wire, length, proof) == LXP_OK);
    proof->layer_b.siblings[0][0] ^= 1U;
    assert(lxp_state_proof_verify(proof, root) != LXP_OK);
    assert(lxp_state_proof_decode(wire, length, proof) == LXP_OK);
    proof->module_id = 10U;
    assert(lxp_state_proof_verify(proof, root) != LXP_OK);
    if (output) {
        printf("%s{\"root\":\"0x", *first ? "" : ",\n");
        hex(root, 32U);
        printf("\",\"proof\":\"0x");
        hex(wire, length);
        printf("\"}");
        *first = false;
    }
    free(proof);
    free(decoded);
    free(wire);
}

int main(int argc, char **argv)
{
    lxp_kernel *kernel = calloc(1U, sizeof(*kernel));
    lxp_state_store *state = calloc(1U, sizeof(*state));
    lxp_state_journal *journal = calloc(1U, sizeof(*journal));
    lxp_module_ctx *ctx = calloc(1U, sizeof(*ctx));
    lxp_state_witness *proof = malloc(sizeof(*proof));
    lx_account_registry *accounts = calloc(1U, sizeof(*accounts));
    uint8_t memory[8192];
    lxp_arena arena;
    uint64_t parameters = 1U;
    bool output = argc == 2 && strcmp(argv[1], "--vectors") == 0;
    bool first = true;
    assert(kernel && state && journal && ctx && proof && accounts);
    assert(lxp_state_store_init(state, 1U) == LXP_OK);
    assert(lx_account_registry_init(accounts) == LXP_OK);
    state->accounts = accounts;
    state->account_root_required = true;
    assert(lxp_arena_init(&arena, memory, sizeof(memory)) == LXP_OK);
    assert(lxp_kernel_create(kernel, state, journal, &parameters, 0U) == LXP_OK);
    assert(lxp_kernel_register_module(kernel, lx_asset_module_iface()) == LXP_OK);
    if (output) printf("{\"vectors\":[\n");
    check(kernel, 0U, (lxp_byte_span){(const uint8_t *)"sequence", 8U}, output, &first);
    check(kernel, 0U, (lxp_byte_span){(const uint8_t *)"account-tree", 12U}, output, &first);
    assert(lxp_kernel_register_module(kernel, programs_module_registration_v4()) == LXP_OK);
    assert(lxp_module_ctx_init(ctx, kernel, 9U, 1U, 0U, 0U, 100000U, &arena, true) == LXP_OK);
    for (uint8_t i = 3U; i != 0U; --i) {
        uint8_t value[2] = {i, (uint8_t)(i + 10U)};
        assert(lxp_ctx_kv_put(ctx, &i, 1U, value, sizeof(value)) == LXP_OK);
    }
    assert(lxp_module_ctx_commit(ctx) == LXP_OK);
    for (uint8_t i = 1U; i <= 3U; ++i)
        check(kernel, 9U, (lxp_byte_span){&i, 1U}, output, &first);
    check(kernel, 0U, (lxp_byte_span){(const uint8_t *)"sequence", 8U}, output, &first);
    {
        uint8_t key[7] = {3U, 0U, 9U, 0U, 0U, 0U, 4U};
        check(kernel, 0U, (lxp_byte_span){key, sizeof(key)}, output, &first);
    }
    assert(lxp_state_proof_build(kernel, 9U, (lxp_byte_span){(const uint8_t *)"missing", 7U}, proof)
           == LXP_ERR_UNKNOWN_FIELD);
    assert(lxp_state_proof_build(kernel, 10U, (lxp_byte_span){(const uint8_t *)"x", 1U}, proof)
           != LXP_OK);
    if (output) printf("\n]}\n");
    assert(lxp_state_store_destroy(state) == LXP_OK);
    free(accounts);
    free(proof);
    free(ctx);
    free(journal);
    free(state);
    free(kernel);
    return 0;
}
