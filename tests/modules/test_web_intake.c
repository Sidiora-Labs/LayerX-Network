#include "layerx/lx_web.h"
#include "layerx/lx_service.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_ledger.h"

#include <stdio.h>
#include <string.h>

#define WEB_CHECK(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "web intake check failed at line %d\n", \
                  __LINE__); \
    return 1; } } while (0)

/* Shared attestation vectors: three registered attestors in ascending signer
 * order, one unregistered signer, and the origin-2 digest they signed for
 * program request 0x0102030405060708. The origin-1 preimage pins the EVM
 * precompile layout from the same specification. */
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

static const uint8_t web_outsider[] = {
    0x9dU, 0x19U, 0x61U, 0xe8U, 0x33U, 0x1aU, 0x46U, 0x55U,
    0x17U, 0x40U, 0xfcU, 0xf9U, 0x41U, 0x4cU, 0x7aU, 0x43U,
    0xe4U, 0x31U, 0x03U, 0x02U
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

static const uint8_t web_response_hash[] = {
    0xe7U, 0x5dU, 0x95U, 0x6cU, 0xdfU, 0x14U, 0xf4U, 0xa1U,
    0xe9U, 0x4dU, 0x2cU, 0xa2U, 0x08U, 0x57U, 0x26U, 0x32U,
    0xa4U, 0xb4U, 0xa0U, 0xccU, 0x53U, 0x23U, 0xa2U, 0xdeU,
    0x37U, 0xe8U, 0x6fU, 0x08U, 0xc9U, 0xf0U, 0xacU, 0xfdU
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

static const uint8_t web_first_sig_outsider[] = {
    0x7cU, 0xb9U, 0xb6U, 0x51U, 0xc0U, 0xa4U, 0x2dU, 0xc0U,
    0xb3U, 0x8aU, 0xc3U, 0x13U, 0x17U, 0x76U, 0x2fU, 0x99U,
    0xa1U, 0x59U, 0x4eU, 0xc9U, 0x98U, 0xbdU, 0x9cU, 0xafU,
    0xb9U, 0x2fU, 0xedU, 0x5dU, 0x20U, 0x45U, 0x82U, 0x84U,
    0x73U, 0xb7U, 0x34U, 0x72U, 0xe6U, 0x8aU, 0x7cU, 0x51U,
    0x83U, 0xe9U, 0x98U, 0xc4U, 0x79U, 0xd3U, 0x60U, 0xc4U,
    0x9aU, 0x8bU, 0xabU, 0x39U, 0xe8U, 0x40U, 0xfcU, 0x22U,
    0x6aU, 0xeaU, 0x09U, 0x15U, 0x3eU, 0xb7U, 0x58U, 0x59U,
    0x1bU
};

static const uint8_t web_first_sig_0_high_s[] = {
    0x5aU, 0xa6U, 0x7dU, 0xfdU, 0xbcU, 0xd1U, 0x3dU, 0x33U,
    0x48U, 0xb8U, 0x36U, 0x46U, 0xf6U, 0xc9U, 0x40U, 0xa4U,
    0xabU, 0x46U, 0xbdU, 0x53U, 0xbdU, 0x1eU, 0x56U, 0x99U,
    0x49U, 0xa7U, 0x7eU, 0xffU, 0x57U, 0xe9U, 0x46U, 0xe4U,
    0xb4U, 0x63U, 0x3aU, 0x3aU, 0x89U, 0x48U, 0x4eU, 0xa5U,
    0x81U, 0x7fU, 0x05U, 0x8fU, 0xe2U, 0x6bU, 0xa5U, 0x08U,
    0x50U, 0x64U, 0xc8U, 0x10U, 0x3dU, 0xa6U, 0x48U, 0xb6U,
    0x98U, 0xc6U, 0x06U, 0x98U, 0x2fU, 0x72U, 0x6aU, 0x6fU,
    0x1cU
};

static const uint8_t web_origin_evm_preimage[] = {
    0x50U, 0x41U, 0x58U, 0x45U, 0x45U, 0x52U, 0x58U, 0x5fU,
    0x57U, 0x45U, 0x42U, 0x5fU, 0x56U, 0x31U, 0x01U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x12U, 0x34U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x0aU, 0xbcU, 0xdeU, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x07U, 0x02U,
    0x63U, 0x7cU, 0x79U, 0x0bU, 0xf5U, 0xfeU, 0x77U, 0x88U,
    0x8fU, 0x96U, 0x96U, 0xcdU, 0x67U, 0xeaU, 0x9cU, 0x58U,
    0x2fU, 0x9bU, 0xe5U, 0x7cU, 0x0bU, 0x16U, 0xc9U, 0xeeU,
    0x11U, 0xd1U, 0xcfU, 0x62U, 0xeeU, 0x25U, 0xc2U, 0xf8U,
    0x2dU, 0x82U, 0x3eU, 0x82U, 0x31U, 0x31U, 0x01U, 0x70U,
    0x7aU, 0x70U, 0x81U, 0xbeU, 0x2eU, 0xfbU, 0x28U, 0xedU,
    0xceU, 0x96U, 0x6eU, 0x3eU, 0x6dU, 0x1eU, 0xaeU, 0xc9U,
    0xf7U, 0xc1U, 0xe4U, 0x80U, 0x88U, 0xf9U, 0x07U, 0x81U,
    0xe7U, 0x5dU, 0x95U, 0x6cU, 0xdfU, 0x14U, 0xf4U, 0xa1U,
    0xe9U, 0x4dU, 0x2cU, 0xa2U, 0x08U, 0x57U, 0x26U, 0x32U,
    0xa4U, 0xb4U, 0xa0U, 0xccU, 0x53U, 0x23U, 0xa2U, 0xdeU,
    0x37U, 0xe8U, 0x6fU, 0x08U, 0xc9U, 0xf0U, 0xacU, 0xfdU,
    0x00U, 0x00U, 0x10U, 0x00U
};

static const uint8_t web_origin_evm_digest[] = {
    0xbfU, 0xb4U, 0x9dU, 0xa4U, 0x9bU, 0xe0U, 0x1aU, 0x79U,
    0x38U, 0xb7U, 0x5cU, 0x23U, 0x6dU, 0xffU, 0xe2U, 0x01U,
    0xc5U, 0x10U, 0x2dU, 0xa9U, 0x62U, 0x4bU, 0x6aU, 0xafU,
    0x21U, 0x15U, 0x77U, 0x54U, 0xffU, 0x7cU, 0xa1U, 0xfcU
};

enum {
    WEB_NETWORK = 9,
    WEB_SEQUENCE = 44,
    WEB_ENCODED_CAPACITY = LX_WEB_OBSERVATION_MAX_BYTES + 1
};

static const uint8_t web_payload[] = "https://paxeer.app/";
static const uint8_t web_text[] = "Paxeer X Network";
static const uint64_t web_first_request = UINT64_C(0x0102030405060708);

static lx_web_store store;
static lx_web_observation observation;
static lx_web_committed committed;
static uint8_t encoded[WEB_ENCODED_CAPACITY];

static void program_id_fill(uint8_t program_id[32])
{
    size_t i;
    for (i = 0U; i < 32U; ++i) program_id[i] = (uint8_t)(0xa0U + i);
}

static void observation_fill(lx_web_observation *value,
                             const uint8_t *const *signatures, size_t count)
{
    size_t i;
    (void)memset(value, 0, sizeof(*value));
    value->origin = LX_WEB_ORIGIN_PROGRAM;
    value->network_id = WEB_NETWORK;
    program_id_fill(value->program_id);
    value->request_id = web_first_request;
    value->kind = LX_WEB_KIND_FETCH;
    (void)memcpy(value->payload_hash, web_payload_hash, 32U);
    (void)memcpy(value->content_digest, web_content_digest, 32U);
    value->response_length = (uint32_t)(sizeof(web_text) - 1U);
    value->full_length = value->response_length;
    (void)memcpy(value->response, web_text, sizeof(web_text) - 1U);
    value->signature_count = count;
    for (i = 0U; i < count; ++i)
        (void)memcpy(value->signatures[i], signatures[i],
                     LX_WEB_SIGNATURE_BYTES);
}

static void attestors_fill(lx_web_attestor_set *set, uint32_t threshold)
{
    size_t i;
    (void)memset(set, 0, sizeof(*set));
    (void)memcpy(set->attestors[0].signer, web_signer_0, 20U);
    (void)memcpy(set->attestors[1].signer, web_signer_1, 20U);
    (void)memcpy(set->attestors[2].signer, web_signer_2, 20U);
    for (i = 0U; i < 3U; ++i)
        (void)memset(set->attestors[i].payout_account, (int)(0x10U + i), 32U);
    set->count = 3U;
    set->threshold = threshold;
}

static void store_reset(void)
{
    (void)memset(&store, 0, sizeof(store));
    store.network_id = WEB_NETWORK;
}

static lxp_result submit(lxp_module_ctx *ctx, const lx_web_attestor_set *set,
                         const lx_web_observation *value)
{
    lx_web_intake_request request;
    size_t length;
    lxp_result status = lx_web_observation_encode(value, encoded,
                                                  sizeof(encoded), &length);
    if (status != LXP_OK) return status;
    request.store = &store;
    request.attestors = set;
    request.payload = encoded;
    request.payload_length = length;
    return lx_web_intake(ctx, &request, &committed);
}

static int untouched(void)
{
    return store.committed_count == 0U && store.pending_count == 1U &&
           !store.pending[0].fulfilled;
}

static int preimage_vectors(void)
{
    uint8_t network_id[32] = { 0U };
    uint8_t requester[32] = { 0U };
    uint8_t payload_hash[32];
    uint8_t preimage[LX_WEB_PREIMAGE_BYTES];
    uint8_t digest[32];
    static const uint8_t evm_payload[] = "paxeer x network";
    network_id[30] = 0x12U;
    network_id[31] = 0x34U;
    requester[29] = 0x0aU;
    requester[30] = 0xbcU;
    requester[31] = 0xdeU;
    WEB_CHECK(sizeof(web_origin_evm_preimage) == LX_WEB_PREIMAGE_BYTES);
    WEB_CHECK(lxp_keccak256(evm_payload, sizeof(evm_payload) - 1U,
                            payload_hash) == LXP_OK);
    WEB_CHECK(lx_web_preimage_encode(LX_WEB_ORIGIN_EVM, network_id, requester,
                                     7U, LX_WEB_KIND_SEARCH, payload_hash,
                                     web_content_digest, web_text,
                                     sizeof(web_text) - 1U, 4096U,
                                     preimage) == LXP_OK);
    WEB_CHECK(memcmp(preimage, web_origin_evm_preimage,
                     LX_WEB_PREIMAGE_BYTES) == 0);
    WEB_CHECK(lxp_keccak256(preimage, sizeof(preimage), digest) == LXP_OK);
    WEB_CHECK(memcmp(digest, web_origin_evm_digest, 32U) == 0);
    WEB_CHECK(lx_web_preimage_encode(3U, network_id, requester, 7U,
                                     LX_WEB_KIND_SEARCH, payload_hash,
                                     web_content_digest, web_text,
                                     sizeof(web_text) - 1U, 4096U,
                                     preimage) == LXP_ERR_NON_CANONICAL);
    WEB_CHECK(lx_web_preimage_encode(LX_WEB_ORIGIN_EVM, network_id, requester,
                                     7U, LX_WEB_KIND_SEARCH, payload_hash,
                                     web_content_digest, web_text,
                                     sizeof(web_text) - 1U, 3U,
                                     preimage) == LXP_ERR_NON_CANONICAL);
    WEB_CHECK(lxp_keccak256(web_payload, sizeof(web_payload) - 1U,
                            payload_hash) == LXP_OK);
    WEB_CHECK(memcmp(payload_hash, web_payload_hash, 32U) == 0);
    WEB_CHECK(lxp_keccak256(web_text, sizeof(web_text) - 1U,
                            digest) == LXP_OK);
    WEB_CHECK(memcmp(digest, web_response_hash, 32U) == 0);
    observation_fill(&observation, NULL, 0U);
    WEB_CHECK(lx_web_observation_digest(&observation, digest) == LXP_OK);
    WEB_CHECK(memcmp(digest, web_first_digest, 32U) == 0);
    return 0;
}

/* Values copied from modules/xweb/types/testdata/preimage-vectors.json so
 * the kernel preimage stays byte-identical to the xweb module. */
static const uint8_t xweb_vector_0_requester[] = {
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x0aU, 0x11U, 0xceU
};
static const uint8_t xweb_vector_0_payload_hash[] = {
    0x62U, 0xdaU, 0x65U, 0xe5U, 0x13U, 0xa2U, 0xdcU, 0x07U,
    0xe8U, 0xc5U, 0x6bU, 0xb6U, 0xe1U, 0x48U, 0xa9U, 0x6dU,
    0x6dU, 0x25U, 0x9eU, 0x49U, 0x6eU, 0xc2U, 0x15U, 0xd5U,
    0x8cU, 0x89U, 0x52U, 0x2fU, 0xa9U, 0x64U, 0x9cU, 0xe1U
};
static const uint8_t xweb_vector_0_content_digest[] = {
    0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U,
    0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U,
    0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U,
    0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U
};
static const uint8_t xweb_vector_0_preimage[] = {
    0x50U, 0x41U, 0x58U, 0x45U, 0x45U, 0x52U, 0x58U, 0x5fU,
    0x57U, 0x45U, 0x42U, 0x5fU, 0x56U, 0x31U, 0x01U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x0aU, 0xe3U, 0xf2U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x0aU, 0x11U, 0xceU, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x07U, 0x01U,
    0x62U, 0xdaU, 0x65U, 0xe5U, 0x13U, 0xa2U, 0xdcU, 0x07U,
    0xe8U, 0xc5U, 0x6bU, 0xb6U, 0xe1U, 0x48U, 0xa9U, 0x6dU,
    0x6dU, 0x25U, 0x9eU, 0x49U, 0x6eU, 0xc2U, 0x15U, 0xd5U,
    0x8cU, 0x89U, 0x52U, 0x2fU, 0xa9U, 0x64U, 0x9cU, 0xe1U,
    0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U,
    0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U,
    0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U,
    0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U, 0x22U,
    0xe7U, 0x5dU, 0x95U, 0x6cU, 0xdfU, 0x14U, 0xf4U, 0xa1U,
    0xe9U, 0x4dU, 0x2cU, 0xa2U, 0x08U, 0x57U, 0x26U, 0x32U,
    0xa4U, 0xb4U, 0xa0U, 0xccU, 0x53U, 0x23U, 0xa2U, 0xdeU,
    0x37U, 0xe8U, 0x6fU, 0x08U, 0xc9U, 0xf0U, 0xacU, 0xfdU,
    0x00U, 0x00U, 0x00U, 0x10U
};
static const uint8_t xweb_vector_0_digest[] = {
    0x21U, 0xe2U, 0xa7U, 0x0fU, 0x24U, 0x3fU, 0x13U, 0xf7U,
    0xe2U, 0xb7U, 0x2dU, 0x14U, 0xccU, 0x84U, 0x2cU, 0x5fU,
    0x6bU, 0x17U, 0x79U, 0xb5U, 0x32U, 0x14U, 0x4bU, 0x01U,
    0xc1U, 0xd6U, 0x1dU, 0x47U, 0xb8U, 0x86U, 0xa0U, 0xbeU
};
static const uint8_t xweb_vector_0_signer[] = {
    0x76U, 0x7eU, 0xb8U, 0x7fU, 0xebU, 0xa9U, 0xe1U, 0x85U,
    0x1bU, 0xd2U, 0xf5U, 0xd6U, 0xfcU, 0x93U, 0x1dU, 0x24U,
    0x76U, 0xb1U, 0xdbU, 0x90U
};
static const uint8_t xweb_vector_0_signature[] = {
    0x1eU, 0x95U, 0xbbU, 0x99U, 0x40U, 0x2eU, 0x2aU, 0xe4U,
    0xb0U, 0x6eU, 0x98U, 0xebU, 0x3cU, 0xe2U, 0x9aU, 0x00U,
    0x97U, 0x3cU, 0xe7U, 0x59U, 0xafU, 0xeaU, 0xc2U, 0x7cU,
    0x3dU, 0x02U, 0x53U, 0xebU, 0xbaU, 0x32U, 0x11U, 0xa7U,
    0x55U, 0x5aU, 0x89U, 0x54U, 0xe3U, 0xd0U, 0xd4U, 0x6bU,
    0xceU, 0x57U, 0x95U, 0x2eU, 0x76U, 0x48U, 0xf0U, 0x8aU,
    0xcbU, 0xb3U, 0xf8U, 0x4fU, 0x2aU, 0x17U, 0x41U, 0x05U,
    0x69U, 0x74U, 0xa3U, 0xe9U, 0x6cU, 0x79U, 0x61U, 0x61U,
    0x1bU
};
static const uint8_t xweb_vector_0_response[] = "Paxeer X Network";
static const uint8_t xweb_vector_1_requester[] = {
    0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U,
    0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U,
    0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U,
    0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U
};
static const uint8_t xweb_vector_1_payload_hash[] = {
    0x63U, 0x7cU, 0x79U, 0x0bU, 0xf5U, 0xfeU, 0x77U, 0x88U,
    0x8fU, 0x96U, 0x96U, 0xcdU, 0x67U, 0xeaU, 0x9cU, 0x58U,
    0x2fU, 0x9bU, 0xe5U, 0x7cU, 0x0bU, 0x16U, 0xc9U, 0xeeU,
    0x11U, 0xd1U, 0xcfU, 0x62U, 0xeeU, 0x25U, 0xc2U, 0xf8U
};
static const uint8_t xweb_vector_1_content_digest[] = {
    0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U,
    0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U,
    0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U,
    0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U
};
static const uint8_t xweb_vector_1_preimage[] = {
    0x50U, 0x41U, 0x58U, 0x45U, 0x45U, 0x52U, 0x58U, 0x5fU,
    0x57U, 0x45U, 0x42U, 0x5fU, 0x56U, 0x31U, 0x02U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x01U, 0x55U,
    0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U,
    0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U,
    0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U,
    0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x55U, 0x00U,
    0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x00U, 0x2aU, 0x02U,
    0x63U, 0x7cU, 0x79U, 0x0bU, 0xf5U, 0xfeU, 0x77U, 0x88U,
    0x8fU, 0x96U, 0x96U, 0xcdU, 0x67U, 0xeaU, 0x9cU, 0x58U,
    0x2fU, 0x9bU, 0xe5U, 0x7cU, 0x0bU, 0x16U, 0xc9U, 0xeeU,
    0x11U, 0xd1U, 0xcfU, 0x62U, 0xeeU, 0x25U, 0xc2U, 0xf8U,
    0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U,
    0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U,
    0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U,
    0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U, 0x33U,
    0x09U, 0xb4U, 0xdcU, 0x1fU, 0x0fU, 0xb6U, 0x23U, 0xc5U,
    0xffU, 0xb2U, 0x40U, 0xfeU, 0xf8U, 0x81U, 0x76U, 0xc4U,
    0x70U, 0x6aU, 0xadU, 0x87U, 0xd7U, 0xa6U, 0x3eU, 0x62U,
    0x27U, 0x77U, 0xc0U, 0x29U, 0xd5U, 0x52U, 0xfbU, 0x3aU,
    0x00U, 0x00U, 0x13U, 0x88U
};
static const uint8_t xweb_vector_1_digest[] = {
    0xc6U, 0x34U, 0x94U, 0x2bU, 0xc3U, 0x2eU, 0x91U, 0xffU,
    0x57U, 0x7eU, 0x48U, 0x52U, 0x7cU, 0x40U, 0xc9U, 0x51U,
    0xd7U, 0xf0U, 0x0bU, 0x08U, 0x08U, 0x76U, 0x4bU, 0x08U,
    0x05U, 0x40U, 0xb5U, 0x72U, 0x92U, 0x34U, 0x33U, 0x79U
};
static const uint8_t xweb_vector_1_signer[] = {
    0x76U, 0x7eU, 0xb8U, 0x7fU, 0xebU, 0xa9U, 0xe1U, 0x85U,
    0x1bU, 0xd2U, 0xf5U, 0xd6U, 0xfcU, 0x93U, 0x1dU, 0x24U,
    0x76U, 0xb1U, 0xdbU, 0x90U
};
static const uint8_t xweb_vector_1_signature[] = {
    0x78U, 0x93U, 0x00U, 0x97U, 0x09U, 0xdcU, 0x16U, 0x8fU,
    0x13U, 0x78U, 0xccU, 0x72U, 0x96U, 0xbcU, 0xa5U, 0x94U,
    0x59U, 0x4bU, 0x52U, 0x2eU, 0x93U, 0x7aU, 0x17U, 0xb1U,
    0x94U, 0x8eU, 0xfbU, 0x39U, 0x97U, 0x26U, 0xe7U, 0x8cU,
    0x45U, 0x32U, 0xa5U, 0xceU, 0x17U, 0x15U, 0x61U, 0x7cU,
    0x5cU, 0xe3U, 0x02U, 0x72U, 0x25U, 0x88U, 0x93U, 0x5bU,
    0x99U, 0x6fU, 0x8bU, 0x8dU, 0x11U, 0x50U, 0x93U, 0xc7U,
    0x47U, 0x59U, 0x32U, 0xa0U, 0x81U, 0x71U, 0x7aU, 0xa9U,
    0x1bU
};
static const uint8_t xweb_vector_1_response[] =
    "[{\"url\":\"https://paxeer.app/\",\"title\":\"Paxeer\","
    "\"snippet\":\"Paxeer X Network\"}]";

static int xweb_vector_check(uint8_t origin, uint32_t network,
                             uint64_t request_id, uint8_t kind,
                             const uint8_t requester[32],
                             const uint8_t payload_hash[32],
                             const uint8_t content_digest[32],
                             const uint8_t *response, size_t response_length,
                             uint32_t full_length,
                             const uint8_t *expected_preimage,
                             const uint8_t expected_digest[32],
                             const uint8_t *expected_signer,
                             const uint8_t signature[LX_WEB_SIGNATURE_BYTES])
{
    uint8_t network_id[32] = { 0U };
    uint8_t preimage[LX_WEB_PREIMAGE_BYTES];
    uint8_t digest[32];
    uint8_t signer[LX_WEB_SIGNER_BYTES];
    network_id[28] = (uint8_t)(network >> 24);
    network_id[29] = (uint8_t)(network >> 16);
    network_id[30] = (uint8_t)(network >> 8);
    network_id[31] = (uint8_t)network;
    WEB_CHECK(lx_web_preimage_encode(origin, network_id, requester, request_id,
                                     kind, payload_hash, content_digest,
                                     response, response_length, full_length,
                                     preimage) == LXP_OK);
    WEB_CHECK(memcmp(preimage, expected_preimage, LX_WEB_PREIMAGE_BYTES) == 0);
    WEB_CHECK(lxp_keccak256(preimage, sizeof(preimage), digest) == LXP_OK);
    WEB_CHECK(memcmp(digest, expected_digest, 32U) == 0);
    WEB_CHECK(signature[64] == 27U || signature[64] == 28U);
    WEB_CHECK(lxp_secp256k1_recover_address(
                  signature, (uint8_t)(signature[64] - 27U), digest,
                  signer) == LXP_OK);
    WEB_CHECK(memcmp(signer, expected_signer, LX_WEB_SIGNER_BYTES) == 0);
    return 0;
}

static int xweb_vectors(void)
{
    WEB_CHECK(sizeof(xweb_vector_0_preimage) == LX_WEB_PREIMAGE_BYTES);
    WEB_CHECK(xweb_vector_check(
                  1U, 713714U, 7U, 1U, xweb_vector_0_requester,
                  xweb_vector_0_payload_hash, xweb_vector_0_content_digest,
                  xweb_vector_0_response,
                  sizeof(xweb_vector_0_response) - 1U, 16U,
                  xweb_vector_0_preimage, xweb_vector_0_digest,
                  xweb_vector_0_signer, xweb_vector_0_signature) == 0);
    WEB_CHECK(sizeof(xweb_vector_1_preimage) == LX_WEB_PREIMAGE_BYTES);
    WEB_CHECK(xweb_vector_check(
                  2U, 1U, 42U, 2U, xweb_vector_1_requester,
                  xweb_vector_1_payload_hash, xweb_vector_1_content_digest,
                  xweb_vector_1_response,
                  sizeof(xweb_vector_1_response) - 1U, 5000U,
                  xweb_vector_1_preimage, xweb_vector_1_digest,
                  xweb_vector_1_signer, xweb_vector_1_signature) == 0);
    return 0;
}

static int request_records(void)
{
    uint8_t record[LX_WEB_REQUEST_RECORD_HEADER_BYTES + sizeof(web_payload)];
    uint8_t program_id[32];
    uint64_t request_id = 0U;
    uint8_t kind = 0U;
    lxp_byte_span payload;
    size_t length = LX_WEB_REQUEST_RECORD_HEADER_BYTES +
                    sizeof(web_payload) - 1U;
    size_t i;
    WEB_CHECK(sizeof(LX_WEB_REQUEST_TOPIC) - 1U == LX_WEB_REQUEST_TOPIC_BYTES);
    for (i = 0U; i < 8U; ++i)
        record[i] = (uint8_t)(web_first_request >> ((7U - i) * 8U));
    record[8] = LX_WEB_KIND_FETCH;
    record[9] = 0U;
    record[10] = 0U;
    record[11] = 0U;
    record[12] = (uint8_t)(sizeof(web_payload) - 1U);
    (void)memcpy(record + LX_WEB_REQUEST_RECORD_HEADER_BYTES, web_payload,
                 sizeof(web_payload) - 1U);
    WEB_CHECK(lx_web_request_record_decode(record, length, &request_id,
                                           &kind, &payload) == LXP_OK);
    WEB_CHECK(request_id == web_first_request && kind == LX_WEB_KIND_FETCH);
    WEB_CHECK(payload.length == sizeof(web_payload) - 1U &&
              memcmp(payload.bytes, web_payload, payload.length) == 0);
    WEB_CHECK(lx_web_request_record_decode(record, length - 1U, &request_id,
                                           &kind, &payload) ==
              LXP_ERR_NON_CANONICAL);
    record[8] = 3U;
    WEB_CHECK(lx_web_request_record_decode(record, length, &request_id,
                                           &kind, &payload) ==
              LXP_ERR_NON_CANONICAL);
    store_reset();
    program_id_fill(program_id);
    WEB_CHECK(lx_web_pending_add(&store, program_id, web_first_request,
                                 LX_WEB_KIND_FETCH, payload.bytes,
                                 payload.length, 40U) == LXP_OK);
    WEB_CHECK(store.pending_count == 1U &&
              memcmp(store.pending[0].payload_hash, web_payload_hash,
                     32U) == 0 &&
              store.pending[0].recorded_sequence == 40U &&
              !store.pending[0].fulfilled);
    WEB_CHECK(lx_web_pending_add(&store, program_id, web_first_request,
                                 LX_WEB_KIND_FETCH, payload.bytes,
                                 payload.length, 41U) ==
              LXP_ERR_DUPLICATE_ENTRY);
    WEB_CHECK(lx_web_pending_add(&store, program_id, web_first_request + 1U,
                                 3U, payload.bytes, payload.length, 41U) ==
              LXP_ERR_NON_CANONICAL);
    WEB_CHECK(store.pending_count == 1U);
    return 0;
}

static int observation_refusals(lxp_module_ctx *ctx)
{
    const uint8_t *signatures[3];
    lx_web_attestor_set set;
    lx_web_attestor_set half;
    uint8_t high_v[LX_WEB_SIGNATURE_BYTES];
    size_t length;

    attestors_fill(&set, 2U);
    attestors_fill(&half, 1U);
    {
        const lx_web_attestor *attestor = NULL;
        WEB_CHECK(lx_web_attestor_lookup(&set, web_outsider, &attestor) ==
                  LXP_ERR_UNKNOWN_FIELD);
        WEB_CHECK(lx_web_attestor_lookup(&set, web_signer_1, &attestor) ==
                      LXP_OK &&
                  attestor == &set.attestors[1]);
    }
    signatures[0] = web_first_sig_0;
    signatures[1] = web_first_sig_1;
    signatures[2] = web_first_sig_2;

    observation_fill(&observation, signatures, 3U);
    observation.request_id = web_first_request + 1U;
    WEB_CHECK(submit(ctx, &set, &observation) == LXP_ERR_UNKNOWN_FIELD);
    WEB_CHECK(untouched());

    observation_fill(&observation, signatures, 3U);
    observation.network_id = WEB_NETWORK + 1U;
    WEB_CHECK(submit(ctx, &set, &observation) == LXP_ERR_WRONG_NETWORK);
    WEB_CHECK(untouched());

    observation_fill(&observation, signatures, 3U);
    observation.kind = LX_WEB_KIND_SEARCH;
    WEB_CHECK(submit(ctx, &set, &observation) == LXP_ERR_CONTEXT_MISMATCH);
    observation_fill(&observation, signatures, 3U);
    observation.payload_hash[5] ^= 1U;
    WEB_CHECK(submit(ctx, &set, &observation) == LXP_ERR_CONTEXT_MISMATCH);
    WEB_CHECK(untouched());

    observation_fill(&observation, signatures, 3U);
    WEB_CHECK(submit(ctx, &half, &observation) ==
              LXP_ERR_ATTESTATION_THRESHOLD);
    WEB_CHECK(untouched());

    observation_fill(&observation, signatures, 1U);
    WEB_CHECK(submit(ctx, &set, &observation) ==
              LXP_ERR_ATTESTATION_THRESHOLD);
    WEB_CHECK(untouched());

    signatures[0] = web_first_sig_1;
    signatures[1] = web_first_sig_0;
    observation_fill(&observation, signatures, 2U);
    WEB_CHECK(submit(ctx, &set, &observation) == LXP_ERR_UNSORTED_SEQUENCE);
    signatures[0] = web_first_sig_0;
    signatures[1] = web_first_sig_0;
    observation_fill(&observation, signatures, 2U);
    WEB_CHECK(submit(ctx, &set, &observation) ==
              LXP_ERR_AUTH_DUPLICATE_SIGNER);
    WEB_CHECK(untouched());

    signatures[0] = web_first_sig_0_high_s;
    signatures[1] = web_first_sig_1;
    observation_fill(&observation, signatures, 2U);
    WEB_CHECK(submit(ctx, &set, &observation) == LXP_ERR_BAD_SIGNATURE);
    (void)memcpy(high_v, web_first_sig_0, sizeof(high_v));
    high_v[64] = 29U;
    signatures[0] = high_v;
    observation_fill(&observation, signatures, 2U);
    WEB_CHECK(submit(ctx, &set, &observation) == LXP_ERR_BAD_SIGNATURE);
    high_v[64] = 0U;
    observation_fill(&observation, signatures, 2U);
    WEB_CHECK(submit(ctx, &set, &observation) == LXP_ERR_BAD_SIGNATURE);
    (void)memcpy(high_v, web_first_sig_0, sizeof(high_v));
    high_v[64] = (uint8_t)(55U - high_v[64]);
    observation_fill(&observation, signatures, 2U);
    WEB_CHECK(submit(ctx, &set, &observation) != LXP_OK);
    WEB_CHECK(untouched());

    signatures[0] = web_first_sig_0;
    signatures[1] = web_first_sig_outsider;
    observation_fill(&observation, signatures, 2U);
    WEB_CHECK(submit(ctx, &set, &observation) == LXP_ERR_INVALID_ATTESTATION);
    signatures[0] = web_first_sig_0;
    signatures[1] = web_first_sig_1;
    signatures[2] = web_first_sig_outsider;
    observation_fill(&observation, signatures, 3U);
    WEB_CHECK(submit(ctx, &set, &observation) != LXP_OK);
    WEB_CHECK(untouched());

    signatures[2] = web_first_sig_2;
    observation_fill(&observation, signatures, 3U);
    observation.content_digest[0] ^= 1U;
    WEB_CHECK(submit(ctx, &set, &observation) != LXP_OK);
    observation_fill(&observation, signatures, 3U);
    observation.response[0] ^= 1U;
    WEB_CHECK(submit(ctx, &set, &observation) != LXP_OK);
    observation_fill(&observation, signatures, 3U);
    observation.full_length += 1U;
    WEB_CHECK(submit(ctx, &set, &observation) != LXP_OK);
    WEB_CHECK(untouched());

    observation_fill(&observation, signatures, 3U);
    WEB_CHECK(lx_web_observation_encode(&observation, encoded,
                                        sizeof(encoded), &length) == LXP_OK);
    encoded[length] = 0U;
    {
        lx_web_intake_request request;
        request.store = &store;
        request.attestors = &set;
        request.payload = encoded;
        request.payload_length = length + 1U;
        WEB_CHECK(lx_web_intake(ctx, &request, &committed) ==
                  LXP_ERR_NON_CANONICAL);
        encoded[3] = 1U;
        request.payload_length = length;
        WEB_CHECK(lx_web_intake(ctx, &request, &committed) ==
                  LXP_ERR_NON_CANONICAL);
        encoded[3] = 0U;
        encoded[0] = LX_WEB_ORIGIN_EVM;
        WEB_CHECK(lx_web_intake(ctx, &request, &committed) ==
                  LXP_ERR_NON_CANONICAL);
    }
    WEB_CHECK(untouched());

    observation_fill(&observation, signatures, 3U);
    observation.response_length = LX_WEB_MAX_RESPONSE_BYTES + 1U;
    observation.full_length = observation.response_length;
    WEB_CHECK(lx_web_observation_encode(&observation, encoded,
                                        sizeof(encoded), &length) ==
              LXP_ERR_NON_CANONICAL);
    observation_fill(&observation, signatures, 3U);
    observation.full_length = observation.response_length - 1U;
    WEB_CHECK(lx_web_observation_encode(&observation, encoded,
                                        sizeof(encoded), &length) ==
              LXP_ERR_NON_CANONICAL);
    return 0;
}

static int observation_accepts(lxp_module_ctx *ctx)
{
    const uint8_t *signatures[3];
    const lx_web_committed *found;
    lx_web_attestor_set set;
    uint8_t program_id[32];

    attestors_fill(&set, 2U);
    program_id_fill(program_id);
    signatures[0] = web_first_sig_0;
    signatures[1] = web_first_sig_1;
    signatures[2] = web_first_sig_2;
    observation_fill(&observation, signatures, 3U);
    WEB_CHECK(lx_web_committed_lookup(&store, program_id, web_first_request,
                                      &found) == LXP_ERR_UNKNOWN_FIELD);
    WEB_CHECK(submit(ctx, &set, &observation) == LXP_OK);
    WEB_CHECK(store.committed_count == 1U && store.pending[0].fulfilled);
    WEB_CHECK(committed.global_sequence == WEB_SEQUENCE);
    WEB_CHECK(committed.signer_count == 3U);
    WEB_CHECK(memcmp(committed.signers[0], web_signer_0, 20U) == 0 &&
              memcmp(committed.signers[1], web_signer_1, 20U) == 0 &&
              memcmp(committed.signers[2], web_signer_2, 20U) == 0);
    WEB_CHECK(memcmp(committed.attestation_digest, web_first_digest,
                     32U) == 0);
    WEB_CHECK(lx_web_committed_lookup(&store, program_id, web_first_request,
                                      &found) == LXP_OK);
    WEB_CHECK(found == &store.committed[0] &&
              found->observation.response_length == sizeof(web_text) - 1U &&
              memcmp(found->observation.response, web_text,
                     sizeof(web_text) - 1U) == 0 &&
              memcmp(found->observation.content_digest, web_content_digest,
                     32U) == 0);
    program_id[0] ^= 1U;
    WEB_CHECK(lx_web_committed_lookup(&store, program_id, web_first_request,
                                      &found) == LXP_ERR_UNKNOWN_FIELD);

    WEB_CHECK(submit(ctx, &set, &observation) == LXP_ERR_SEQUENCE_REUSED);
    WEB_CHECK(store.committed_count == 1U);

    program_id_fill(program_id);
    store_reset();
    WEB_CHECK(lx_web_pending_add(&store, program_id, web_first_request,
                                 LX_WEB_KIND_FETCH, web_payload,
                                 sizeof(web_payload) - 1U, 40U) == LXP_OK);
    signatures[0] = web_first_sig_0;
    signatures[1] = web_first_sig_2;
    observation_fill(&observation, signatures, 2U);
    WEB_CHECK(submit(ctx, &set, &observation) == LXP_OK);
    WEB_CHECK(store.committed_count == 1U && committed.signer_count == 2U &&
              memcmp(committed.signers[1], web_signer_2, 20U) == 0);
    return 0;
}

static int attestor_sets(lxp_module_ctx *ctx, lxp_kernel *kernel)
{
    static const uint8_t governance_key[32] = { 0x42U, 7U };
    uint8_t payload[LX_WEB_ATTESTOR_SET_MAX_BYTES];
    lx_web_attestor_set proposed;
    lx_web_attestor_set active;
    lx_web_attestor_set decoded;
    lxp_authority_resolved authority;
    lxp_activity activity;
    size_t length;

    attestors_fill(&proposed, 1U);
    WEB_CHECK(lx_web_attestor_set_validate(&proposed) ==
              LXP_ERR_PARAMETER_BOUNDS);
    proposed.threshold = 4U;
    WEB_CHECK(lx_web_attestor_set_validate(&proposed) ==
              LXP_ERR_PARAMETER_BOUNDS);
    proposed.threshold = 0U;
    WEB_CHECK(lx_web_attestor_set_validate(&proposed) ==
              LXP_ERR_PARAMETER_BOUNDS);
    proposed.count = 2U;
    proposed.threshold = 1U;
    WEB_CHECK(lx_web_attestor_set_validate(&proposed) ==
              LXP_ERR_PARAMETER_BOUNDS);
    proposed.threshold = 2U;
    WEB_CHECK(lx_web_attestor_set_validate(&proposed) == LXP_OK);
    attestors_fill(&proposed, 2U);
    (void)memcpy(proposed.attestors[1].signer, web_signer_0, 20U);
    WEB_CHECK(lx_web_attestor_set_validate(&proposed) ==
              LXP_ERR_UNSORTED_SEQUENCE);
    attestors_fill(&proposed, 3U);
    WEB_CHECK(lx_web_attestor_set_validate(&proposed) == LXP_OK);
    WEB_CHECK(lx_web_attestor_set_encode(&proposed, payload, sizeof(payload),
                                         &length) == LXP_OK);
    WEB_CHECK(length == 2U + 3U * LX_WEB_ATTESTOR_ENTRY_BYTES &&
              payload[0] == 3U && payload[1] == 3U);
    WEB_CHECK(lx_web_attestor_set_decode(payload, length, &decoded) ==
              LXP_OK);
    WEB_CHECK(decoded.count == 3U && decoded.threshold == 3U &&
              memcmp(decoded.attestors[2].signer, web_signer_2, 20U) == 0);
    WEB_CHECK(lx_web_attestor_set_decode(payload, length - 1U, &decoded) ==
              LXP_ERR_NON_CANONICAL);

    (void)memset(&activity, 0, sizeof(activity));
    activity.activity_type = LX_WEB_ATTESTOR_SET_ACTIVITY;
    activity.payload.bytes = payload;
    activity.payload.length = length;
    WEB_CHECK(lxp_hash_payload(payload, length, activity.payload_hash) ==
              LXP_OK);
    (void)memset(&authority, 0, sizeof(authority));
    authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(authority.verified_key, governance_key, 32U);
    attestors_fill(&active, 2U);

    kernel->handover.enabled = false;
    (void)memcpy(kernel->handover.governance_public_key, governance_key, 32U);
    WEB_CHECK(lx_web_attestor_set_execute(ctx, &activity, &authority,
                                          &active) == LXP_ERR_AUTH_SCOPE);
    kernel->handover.enabled = true;
    authority.verified_key[0] ^= 1U;
    WEB_CHECK(lx_web_attestor_set_execute(ctx, &activity, &authority,
                                          &active) == LXP_ERR_AUTH_SCOPE);
    authority.verified_key[0] ^= 1U;
    authority.kind = LXP_AUTHORITY_SESSION_KEY;
    WEB_CHECK(lx_web_attestor_set_execute(ctx, &activity, &authority,
                                          &active) == LXP_ERR_AUTH_SCOPE);
    authority.kind = LXP_AUTHORITY_OWNER;
    activity.activity_type = LX_WEB_OBSERVATION_ACTIVITY;
    WEB_CHECK(lx_web_attestor_set_execute(ctx, &activity, &authority,
                                          &active) == LXP_ERR_NON_CANONICAL);
    activity.activity_type = LX_WEB_ATTESTOR_SET_ACTIVITY;
    WEB_CHECK(active.threshold == 2U && active.updated_sequence == 0U);

    payload[1] = 1U;
    WEB_CHECK(lx_web_attestor_set_execute(ctx, &activity, &authority,
                                          &active) ==
              LXP_ERR_PAYLOAD_HASH_MISMATCH);
    WEB_CHECK(lxp_hash_payload(payload, length, activity.payload_hash) ==
              LXP_OK);
    WEB_CHECK(lx_web_attestor_set_execute(ctx, &activity, &authority,
                                          &active) ==
              LXP_ERR_PARAMETER_BOUNDS);
    WEB_CHECK(active.threshold == 2U && active.updated_sequence == 0U);

    payload[1] = 3U;
    WEB_CHECK(lxp_hash_payload(payload, length, activity.payload_hash) ==
              LXP_OK);
    WEB_CHECK(lx_web_attestor_set_execute(ctx, &activity, &authority,
                                          &active) == LXP_OK);
    WEB_CHECK(active.count == 3U && active.threshold == 3U &&
              active.updated_sequence == WEB_SEQUENCE &&
              memcmp(active.attestors[0].signer, web_signer_0, 20U) == 0);
    return 0;
}

int main(void)
{
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    static uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;

    if (preimage_vectors() != 0 || xweb_vectors() != 0 ||
        request_records() != 0)
        return 1;
    WEB_CHECK(lxp_state_store_init(&state, 0U) == LXP_OK);
    WEB_CHECK(lxp_kernel_create(&kernel, &state, &journal, &parameters,
                                0U) == LXP_OK);
    WEB_CHECK(lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) ==
              LXP_OK);
    WEB_CHECK(lxp_kernel_register_module(&kernel,
                                         lx_service_module_iface()) ==
              LXP_OK);
    WEB_CHECK(lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_SERVICE, 500U, 0U,
                                  WEB_SEQUENCE, 1000U, &arena, true) ==
              LXP_OK);
    if (observation_refusals(&ctx) != 0 || observation_accepts(&ctx) != 0 ||
        attestor_sets(&ctx, &kernel) != 0)
        return 1;
    WEB_CHECK(lxp_state_store_destroy(&state) == LXP_OK);
    return 0;
}
