#include "layerx/lx_web.h"
#include "layerx/lx_service.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_merkle.h"

#include <stdio.h>
#include <string.h>

#define ROOT_CHECK(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "web root check failed at line %d\n", \
                  __LINE__); \
    return 1; } } while (0)

/* Shared attestation vectors for program requests 0x0102030405060708 and
 * 0x0102030405060709, each signed by the three registered attestors. */
static const uint8_t web_signer_0[] = {
    0x95U, 0x7eU, 0x1eU, 0x3bU, 0x0fU, 0xceU, 0x62U, 0x06U,
    0x18U, 0x53U, 0x54U, 0x4cU, 0x13U, 0xe0U, 0x2cU, 0xceU,
    0xbaU, 0x82U, 0xbaU, 0x8bU
};

static const uint8_t web_signer_1[] = {
    0xb4U, 0x17U, 0xffU, 0xcfU, 0x7bU, 0xc5U, 0x99U, 0x6eU,
    0x2aU, 0x06U, 0x77U, 0xbaU, 0x25U, 0x03U, 0x3dU, 0xa7U,
    0xe9U, 0x0cU, 0x4fU, 0xe2U
};

static const uint8_t web_signer_2[] = {
    0xc2U, 0xbcU, 0xe3U, 0x16U, 0xfaU, 0xb5U, 0x2bU, 0x82U,
    0x63U, 0xd3U, 0xc6U, 0x79U, 0xf3U, 0x6aU, 0x6bU, 0xb4U,
    0x53U, 0x24U, 0x27U, 0x84U
};

static const uint8_t web_payload_hash[] = {
    0x62U, 0xdaU, 0x65U, 0xe5U, 0x13U, 0xa2U, 0xdcU, 0x07U,
    0xe8U, 0xc5U, 0x6bU, 0xb6U, 0xe1U, 0x48U, 0xa9U, 0x6dU,
    0x6dU, 0x25U, 0x9eU, 0x49U, 0x6eU, 0xc2U, 0x15U, 0xd5U,
    0x8cU, 0x89U, 0x52U, 0x2fU, 0xa9U, 0x64U, 0x9cU, 0xe1U
};

static const uint8_t web_content_digest[] = {
    0x2dU, 0x82U, 0x3eU, 0x82U, 0x31U, 0x31U, 0x01U, 0x70U,
    0x7aU, 0x70U, 0x81U, 0xbeU, 0x2eU, 0xfbU, 0x28U, 0xedU,
    0xceU, 0x96U, 0x6eU, 0x3eU, 0x6dU, 0x1eU, 0xaeU, 0xc9U,
    0xf7U, 0xc1U, 0xe4U, 0x80U, 0x88U, 0xf9U, 0x07U, 0x81U
};

static const uint8_t web_first_digest[] = {
    0x9bU, 0xe7U, 0xb0U, 0xf4U, 0xb8U, 0x6dU, 0xb6U, 0x79U,
    0x4fU, 0x11U, 0xbaU, 0x48U, 0x98U, 0x82U, 0x7dU, 0x69U,
    0xfcU, 0xdaU, 0x52U, 0xfaU, 0xa6U, 0x81U, 0xb6U, 0xd6U,
    0x22U, 0x18U, 0x1fU, 0x33U, 0x54U, 0x7aU, 0x70U, 0xc1U
};

static const uint8_t web_first_sig_0[] = {
    0x5aU, 0xa6U, 0x7dU, 0xfdU, 0xbcU, 0xd1U, 0x3dU, 0x33U,
    0x48U, 0xb8U, 0x36U, 0x46U, 0xf6U, 0xc9U, 0x40U, 0xa4U,
    0xabU, 0x46U, 0xbdU, 0x53U, 0xbdU, 0x1eU, 0x56U, 0x99U,
    0x49U, 0xa7U, 0x7eU, 0xffU, 0x57U, 0xe9U, 0x46U, 0xe4U,
    0x4bU, 0x9cU, 0xc5U, 0xc5U, 0x76U, 0xb7U, 0xb1U, 0x5aU,
    0x7eU, 0x80U, 0xfaU, 0x70U, 0x1dU, 0x94U, 0x5aU, 0xf6U,
    0x6aU, 0x4aU, 0x14U, 0xd6U, 0x71U, 0xa2U, 0x57U, 0x85U,
    0x27U, 0x0cU, 0x57U, 0xf4U, 0xa0U, 0xc3U, 0xd6U, 0xd2U,
    0x1bU
};

static const uint8_t web_first_sig_1[] = {
    0xcaU, 0x5eU, 0x20U, 0xf8U, 0x60U, 0x7eU, 0x3dU, 0xd9U,
    0xcfU, 0x1cU, 0xf3U, 0x4eU, 0x2eU, 0xb1U, 0xbfU, 0x9aU,
    0x60U, 0x8eU, 0x39U, 0x9eU, 0xa5U, 0xdaU, 0x70U, 0x44U,
    0xe8U, 0xa0U, 0xebU, 0x75U, 0xc5U, 0xb8U, 0x7fU, 0xbaU,
    0x5aU, 0xc7U, 0x1aU, 0x44U, 0x90U, 0x71U, 0xbaU, 0x4aU,
    0xfdU, 0xe2U, 0x96U, 0x14U, 0xadU, 0xfbU, 0x21U, 0xb1U,
    0x35U, 0x36U, 0x7fU, 0x0bU, 0xbbU, 0x84U, 0x7fU, 0x9bU,
    0x26U, 0x89U, 0xecU, 0xceU, 0xaeU, 0x9cU, 0x39U, 0xefU,
    0x1bU
};

static const uint8_t web_first_sig_2[] = {
    0x4aU, 0x8fU, 0x5dU, 0x5cU, 0x2dU, 0x77U, 0x2aU, 0xd6U,
    0xfeU, 0xe3U, 0xa9U, 0x46U, 0xb5U, 0x94U, 0xb9U, 0x16U,
    0x05U, 0xc9U, 0x52U, 0xe8U, 0xcdU, 0x52U, 0x29U, 0x61U,
    0x36U, 0x8cU, 0x4eU, 0x3eU, 0x66U, 0x16U, 0xa9U, 0xd1U,
    0x3cU, 0x96U, 0x11U, 0x87U, 0xd6U, 0xe8U, 0x7aU, 0x4bU,
    0x3cU, 0x72U, 0x7dU, 0x39U, 0x70U, 0x87U, 0x1bU, 0xc8U,
    0x3aU, 0x7bU, 0x80U, 0x07U, 0x52U, 0xa2U, 0x39U, 0x4bU,
    0xb7U, 0x02U, 0xc5U, 0x6eU, 0xb5U, 0xa7U, 0x6dU, 0xb0U,
    0x1bU
};

static const uint8_t web_second_digest[] = {
    0x5aU, 0x59U, 0x18U, 0x86U, 0xbbU, 0xcbU, 0x4fU, 0x26U,
    0x7fU, 0x41U, 0xacU, 0x23U, 0xcfU, 0xa3U, 0x19U, 0x36U,
    0x2fU, 0x6dU, 0xefU, 0x04U, 0x7fU, 0xc5U, 0x2cU, 0x6cU,
    0x0dU, 0xe8U, 0x2aU, 0xdaU, 0x56U, 0xb7U, 0xd6U, 0xadU
};

static const uint8_t web_second_sig_0[] = {
    0x38U, 0x27U, 0x5cU, 0x06U, 0xe8U, 0x21U, 0xbbU, 0xa6U,
    0x8dU, 0xc3U, 0x4dU, 0x51U, 0xc5U, 0xafU, 0x11U, 0x57U,
    0x3dU, 0x19U, 0x41U, 0x38U, 0x30U, 0x4aU, 0x71U, 0xb0U,
    0x11U, 0xa4U, 0x3dU, 0x3fU, 0x1cU, 0x48U, 0x52U, 0x47U,
    0x32U, 0xedU, 0xd6U, 0xcfU, 0x9cU, 0x5fU, 0x41U, 0x28U,
    0xf2U, 0x1eU, 0xf0U, 0x9fU, 0xd0U, 0x7bU, 0x74U, 0x84U,
    0x09U, 0x9dU, 0x56U, 0x22U, 0x19U, 0xcdU, 0xaaU, 0x75U,
    0x78U, 0xddU, 0x00U, 0xb6U, 0xfcU, 0xfbU, 0xe0U, 0xc2U,
    0x1bU
};

static const uint8_t web_second_sig_1[] = {
    0xb7U, 0xcaU, 0xfcU, 0x6aU, 0x5cU, 0xe0U, 0xc6U, 0x19U,
    0xf9U, 0x33U, 0x2fU, 0xccU, 0x18U, 0x5eU, 0xa2U, 0x5cU,
    0xc2U, 0x67U, 0x0bU, 0x9eU, 0x96U, 0xdcU, 0x0dU, 0xecU,
    0x53U, 0xe7U, 0xd7U, 0xb6U, 0x26U, 0x7fU, 0x68U, 0x03U,
    0x00U, 0xd4U, 0xd7U, 0x46U, 0xbbU, 0x05U, 0x3eU, 0x6dU,
    0x6cU, 0xc3U, 0x9eU, 0xf8U, 0xabU, 0x88U, 0xbcU, 0x96U,
    0xbbU, 0xfeU, 0x5dU, 0xc5U, 0x49U, 0x82U, 0x0aU, 0x13U,
    0x61U, 0xbfU, 0x84U, 0x30U, 0x41U, 0x9aU, 0x3cU, 0x51U,
    0x1cU
};

static const uint8_t web_second_sig_2[] = {
    0x33U, 0x0cU, 0xc2U, 0xa6U, 0xb6U, 0x01U, 0xfcU, 0xa2U,
    0xebU, 0x47U, 0xeeU, 0x90U, 0x61U, 0x15U, 0x28U, 0x9aU,
    0xafU, 0xaaU, 0x83U, 0x27U, 0x23U, 0xb9U, 0xe9U, 0xb2U,
    0xc8U, 0x2eU, 0x98U, 0x88U, 0xbaU, 0x2dU, 0x74U, 0x1dU,
    0x14U, 0xcdU, 0xb8U, 0xfcU, 0x70U, 0x59U, 0x55U, 0x2fU,
    0x06U, 0x0aU, 0x38U, 0x07U, 0x1dU, 0x0dU, 0x35U, 0x43U,
    0xb7U, 0xb3U, 0xdbU, 0xdcU, 0xdaU, 0x88U, 0x0eU, 0xfdU,
    0xf4U, 0x90U, 0x76U, 0x30U, 0x3aU, 0xd1U, 0xe3U, 0x3fU,
    0x1cU
};

enum { ROOT_NETWORK = 9 };

static const uint8_t root_payload[] = "https://paxeer.app/";
static const uint8_t root_text[] = "Paxeer X Network";
static const uint64_t root_first_request = UINT64_C(0x0102030405060708);

typedef struct root_case {
    uint64_t request_id;
    const uint8_t *signatures[3];
} root_case;

static lx_web_store first_store;
static lx_web_store replay_store;
static lx_web_store reordered_store;
static lx_web_availability_bundle bundle;
static lx_web_observation observation;
static lx_web_committed committed;
static uint8_t encoded[LX_WEB_OBSERVATION_MAX_BYTES];
static uint8_t arena_bytes[16384];
static lxp_state_store state;
static lxp_state_journal journal;
static lxp_kernel kernel;

static void program_id_fill(uint8_t program_id[32])
{
    size_t i;
    for (i = 0U; i < 32U; ++i) program_id[i] = (uint8_t)(0xa0U + i);
}

static void attestors_fill(lx_web_attestor_set *set)
{
    size_t i;
    (void)memset(set, 0, sizeof(*set));
    (void)memcpy(set->attestors[0].signer, web_signer_0, 20U);
    (void)memcpy(set->attestors[1].signer, web_signer_1, 20U);
    (void)memcpy(set->attestors[2].signer, web_signer_2, 20U);
    for (i = 0U; i < 3U; ++i)
        (void)memset(set->attestors[i].payout_account, (int)(0x10U + i), 32U);
    set->count = 3U;
    set->threshold = 2U;
}

static lxp_result store_prepare(lx_web_store *store)
{
    uint8_t program_id[32];
    lxp_result status;
    (void)memset(store, 0, sizeof(*store));
    store->network_id = ROOT_NETWORK;
    program_id_fill(program_id);
    status = lx_web_pending_add(store, program_id, root_first_request,
                                LX_WEB_KIND_FETCH, root_payload,
                                sizeof(root_payload) - 1U, 10U);
    if (status != LXP_OK) return status;
    return lx_web_pending_add(store, program_id, root_first_request + 1U,
                              LX_WEB_KIND_FETCH, root_payload,
                              sizeof(root_payload) - 1U, 11U);
}

static lxp_result observe(lx_web_store *store, const root_case *value,
                          uint64_t global_sequence)
{
    lx_web_attestor_set set;
    lx_web_intake_request request;
    lxp_module_ctx ctx;
    lxp_arena arena;
    size_t length;
    size_t i;
    lxp_result status;
    attestors_fill(&set);
    (void)memset(&observation, 0, sizeof(observation));
    observation.origin = LX_WEB_ORIGIN_PROGRAM;
    observation.network_id = ROOT_NETWORK;
    program_id_fill(observation.program_id);
    observation.request_id = value->request_id;
    observation.kind = LX_WEB_KIND_FETCH;
    (void)memcpy(observation.payload_hash, web_payload_hash, 32U);
    (void)memcpy(observation.content_digest, web_content_digest, 32U);
    observation.response_length = (uint32_t)(sizeof(root_text) - 1U);
    observation.full_length = observation.response_length;
    (void)memcpy(observation.response, root_text, sizeof(root_text) - 1U);
    observation.signature_count = 3U;
    for (i = 0U; i < 3U; ++i)
        (void)memcpy(observation.signatures[i], value->signatures[i],
                     LX_WEB_SIGNATURE_BYTES);
    status = lx_web_observation_encode(&observation, encoded,
                                       sizeof(encoded), &length);
    if (status == LXP_OK)
        status = lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes));
    if (status == LXP_OK)
        status = lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_SERVICE, 500U,
                                     0U, global_sequence, 1000U, &arena,
                                     true);
    if (status != LXP_OK) return status;
    request.store = store;
    request.attestors = &set;
    request.payload = encoded;
    request.payload_length = length;
    return lx_web_intake(&ctx, &request, &committed);
}

static lxp_result root_of(const lx_web_store *store, uint8_t root[32])
{
    lxp_arena arena;
    lxp_result status = lxp_arena_init(&arena, arena_bytes,
                                       sizeof(arena_bytes));
    if (status != LXP_OK) return status;
    return lx_web_root(store, &arena, root);
}

static lxp_result bundle_root(uint8_t root[32])
{
    lxp_arena arena;
    lxp_result status = lxp_arena_init(&arena, arena_bytes,
                                       sizeof(arena_bytes));
    if (status != LXP_OK) return status;
    return lx_web_root_from_availability(&bundle, &arena, root);
}

int main(void)
{
    const root_case first = {
        root_first_request,
        { web_first_sig_0, web_first_sig_1, web_first_sig_2 }
    };
    const root_case second = {
        root_first_request + 1U,
        { web_second_sig_0, web_second_sig_1, web_second_sig_2 }
    };
    uint64_t parameters = 1U;
    uint8_t empty_root[32];
    uint8_t expected_empty[32];
    uint8_t root[32];
    uint8_t replayed[32];
    uint8_t recomputed[32];
    uint8_t signer_digest[32];
    uint8_t signers[3 * LX_WEB_SIGNER_BYTES];
    uint8_t leaf[LX_WEB_LEAF_BYTES];
    size_t length;
    lxp_arena arena;

    ROOT_CHECK(lxp_state_store_init(&state, 0U) == LXP_OK);
    ROOT_CHECK(lxp_kernel_create(&kernel, &state, &journal, &parameters,
                                 0U) == LXP_OK);
    ROOT_CHECK(lxp_kernel_register_module(&kernel,
                                          lx_service_module_iface()) ==
               LXP_OK);

    ROOT_CHECK(store_prepare(&first_store) == LXP_OK);
    ROOT_CHECK(root_of(&first_store, empty_root) == LXP_OK);
    ROOT_CHECK(lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) ==
               LXP_OK);
    ROOT_CHECK(lxp_merkle_build(NULL, 0U, &arena, expected_empty) == LXP_OK);
    ROOT_CHECK(memcmp(empty_root, expected_empty, 32U) == 0);

    ROOT_CHECK(observe(&first_store, &first, 101U) == LXP_OK);
    ROOT_CHECK(lx_web_leaf_encode(&committed, leaf, sizeof(leaf),
                                  &length) == LXP_OK);
    ROOT_CHECK(length == LX_WEB_LEAF_BYTES);
    ROOT_CHECK(memcmp(leaf, web_first_digest, 32U) == 0);
    ROOT_CHECK(leaf[32] == 0xa0U && leaf[63] == 0xbfU);
    ROOT_CHECK(leaf[64] == 1U && leaf[71] == 8U && leaf[72] == 1U);
    ROOT_CHECK(memcmp(leaf + 73U, web_payload_hash, 32U) == 0);
    ROOT_CHECK(memcmp(leaf + 105U, web_content_digest, 32U) == 0);
    ROOT_CHECK(leaf[172] == (uint8_t)(sizeof(root_text) - 1U));
    (void)memcpy(signers, web_signer_0, 20U);
    (void)memcpy(signers + 20U, web_signer_1, 20U);
    (void)memcpy(signers + 40U, web_signer_2, 20U);
    ROOT_CHECK(lxp_keccak256(signers, sizeof(signers), signer_digest) ==
               LXP_OK);
    ROOT_CHECK(memcmp(leaf + 173U, signer_digest, 32U) == 0);
    ROOT_CHECK(leaf[212] == 101U && leaf[205] == 0U);
    ROOT_CHECK(lx_web_leaf_encode(&committed, leaf, sizeof(leaf) - 1U,
                                  &length) == LXP_ERR_NON_CANONICAL);

    ROOT_CHECK(observe(&first_store, &second, 102U) == LXP_OK);
    ROOT_CHECK(root_of(&first_store, root) == LXP_OK);
    ROOT_CHECK(memcmp(root, empty_root, 32U) != 0);
    ROOT_CHECK(lx_web_availability_bundle_build(&first_store, &bundle) ==
               LXP_OK);
    ROOT_CHECK(bundle.count == 2U);
    ROOT_CHECK(memcmp(bundle.leaves[0], web_second_digest, 32U) == 0 &&
               memcmp(bundle.leaves[1], web_first_digest, 32U) == 0);
    ROOT_CHECK(bundle_root(recomputed) == LXP_OK);
    ROOT_CHECK(memcmp(root, recomputed, 32U) == 0);

    ROOT_CHECK(store_prepare(&replay_store) == LXP_OK);
    ROOT_CHECK(observe(&replay_store, &first, 101U) == LXP_OK);
    ROOT_CHECK(observe(&replay_store, &second, 102U) == LXP_OK);
    ROOT_CHECK(root_of(&replay_store, replayed) == LXP_OK);
    ROOT_CHECK(memcmp(root, replayed, 32U) == 0);
    ROOT_CHECK(observe(&replay_store, &first, 103U) ==
               LXP_ERR_SEQUENCE_REUSED);
    ROOT_CHECK(root_of(&replay_store, replayed) == LXP_OK);
    ROOT_CHECK(memcmp(root, replayed, 32U) == 0);

    ROOT_CHECK(store_prepare(&reordered_store) == LXP_OK);
    ROOT_CHECK(observe(&reordered_store, &second, 102U) == LXP_OK);
    ROOT_CHECK(observe(&reordered_store, &first, 101U) == LXP_OK);
    ROOT_CHECK(root_of(&reordered_store, replayed) == LXP_OK);
    ROOT_CHECK(memcmp(root, replayed, 32U) == 0);

    ROOT_CHECK(store_prepare(&reordered_store) == LXP_OK);
    ROOT_CHECK(observe(&reordered_store, &first, 101U) == LXP_OK);
    ROOT_CHECK(observe(&reordered_store, &second, 104U) == LXP_OK);
    ROOT_CHECK(root_of(&reordered_store, replayed) == LXP_OK);
    ROOT_CHECK(memcmp(root, replayed, 32U) != 0);

    bundle.leaves[1][100] ^= 1U;
    ROOT_CHECK(bundle_root(recomputed) == LXP_OK);
    ROOT_CHECK(memcmp(root, recomputed, 32U) != 0);
    bundle.leaves[1][100] ^= 1U;
    (void)memcpy(leaf, bundle.leaves[0], sizeof(leaf));
    (void)memcpy(bundle.leaves[0], bundle.leaves[1], sizeof(leaf));
    (void)memcpy(bundle.leaves[1], leaf, sizeof(leaf));
    ROOT_CHECK(bundle_root(recomputed) == LXP_ERR_UNSORTED_SEQUENCE);

    ROOT_CHECK(lxp_state_store_destroy(&state) == LXP_OK);
    return 0;
}
