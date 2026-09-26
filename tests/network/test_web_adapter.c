#include "layerx/lx_web.h"
#include "layerx/lxp_crypto.h"

#include <stdio.h>
#include <string.h>

#define ADAPTER_CHECK(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "web adapter check failed at line %d\n", \
                  __LINE__); \
    return 1; } } while (0)

/* The origin-2 observation for program request 0x0102030405060708 signed by
 * the three registered attestors of the shared attestation vectors. */
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

static const uint8_t web_first_sig_0[] = {
    0x31U, 0x94U, 0x7aU, 0xb5U, 0x31U, 0x87U, 0x68U, 0x6fU,
    0xc5U, 0xc3U, 0xc2U, 0x1fU, 0x0fU, 0x78U, 0xdcU, 0xddU,
    0xc2U, 0xa7U, 0xbaU, 0x4aU, 0xe8U, 0xfdU, 0x7cU, 0x47U,
    0x91U, 0x2fU, 0x86U, 0x36U, 0x14U, 0xbeU, 0x8cU, 0xafU,
    0x4bU, 0xfcU, 0x95U, 0x41U, 0x33U, 0x9aU, 0x82U, 0x3eU,
    0x77U, 0x2aU, 0xa6U, 0x57U, 0xd0U, 0x00U, 0x17U, 0xa5U,
    0xedU, 0x0cU, 0x36U, 0xccU, 0x06U, 0x8bU, 0x58U, 0xe1U,
    0x02U, 0x00U, 0x23U, 0xcbU, 0x2cU, 0xabU, 0x7fU, 0xabU,
    0x1cU
};

static const uint8_t web_first_sig_1[] = {
    0x20U, 0x7eU, 0x4aU, 0xf3U, 0xfbU, 0x80U, 0x72U, 0x60U,
    0xe2U, 0xc4U, 0x30U, 0x7bU, 0x23U, 0x31U, 0x3cU, 0x31U,
    0xaaU, 0xe3U, 0x7bU, 0x91U, 0xaeU, 0x22U, 0x34U, 0xf1U,
    0x6dU, 0x85U, 0x23U, 0xf4U, 0xb5U, 0x75U, 0xb3U, 0xf2U,
    0x4cU, 0xdfU, 0x15U, 0x34U, 0x25U, 0xe5U, 0xefU, 0x70U,
    0x8dU, 0x4dU, 0x7bU, 0x3eU, 0x8dU, 0x4fU, 0x27U, 0x0bU,
    0x0cU, 0x5aU, 0x9dU, 0x7fU, 0x43U, 0xdaU, 0x00U, 0xa3U,
    0x74U, 0x9aU, 0x47U, 0x7aU, 0x62U, 0xccU, 0x14U, 0x5dU,
    0x1bU
};

static const uint8_t web_first_sig_2[] = {
    0x2bU, 0xd8U, 0x75U, 0x56U, 0x72U, 0xa6U, 0x6bU, 0xc2U,
    0x0bU, 0x1cU, 0x68U, 0xacU, 0x3aU, 0xd4U, 0xeaU, 0x8fU,
    0x5eU, 0xeeU, 0x7cU, 0xa9U, 0xfcU, 0x41U, 0x95U, 0x2dU,
    0xe9U, 0x58U, 0x52U, 0x7dU, 0xceU, 0x9bU, 0xccU, 0xecU,
    0x51U, 0x50U, 0xb8U, 0x8cU, 0x5aU, 0xfcU, 0xc0U, 0x10U,
    0x81U, 0x1aU, 0x94U, 0xe0U, 0x83U, 0xfdU, 0x8bU, 0x0bU,
    0x38U, 0xcfU, 0xc9U, 0xa6U, 0xdbU, 0xa6U, 0x4fU, 0x0aU,
    0x35U, 0x3aU, 0xa3U, 0x75U, 0x1bU, 0x07U, 0xa7U, 0xcfU,
    0x1bU
};

enum {
    ADAPTER_NETWORK = 9,
    ADAPTER_SEQUENCE = 7,
    ADAPTER_FIXTURE_CAPACITY = 16384
};

static const char fixture_path[] =
    "tests/fixtures/web/observation-activity.hex";
static const uint8_t adapter_text[] = "Paxeer X Network";
static const uint8_t adapter_actor[] = "did:key:web-attestor-one";
static const uint8_t adapter_seed[32] = { 21U };
static const lxp_timestamp_bound adapter_bound = { 1000U, 61000U };

static lx_web_observation observation;
static lx_web_observation decoded_observation;
static uint8_t arena_bytes[LXP_MAX_ACTIVITY_BYTES];
static uint8_t payload[LX_WEB_OBSERVATION_MAX_BYTES];
static uint8_t fixture[ADAPTER_FIXTURE_CAPACITY];
static uint8_t submissions[2][ADAPTER_FIXTURE_CAPACITY];
static size_t submission_lengths[2];

typedef struct adapter_feed {
    size_t remaining;
    size_t submitted;
} adapter_feed;

static void observation_fill(lx_web_observation *value)
{
    size_t i;
    (void)memset(value, 0, sizeof(*value));
    value->origin = LX_WEB_ORIGIN_PROGRAM;
    value->network_id = ADAPTER_NETWORK;
    for (i = 0U; i < 32U; ++i) value->program_id[i] = (uint8_t)(0xa0U + i);
    value->request_id = UINT64_C(0x0102030405060708);
    value->kind = LX_WEB_KIND_FETCH;
    (void)memcpy(value->payload_hash, web_payload_hash, 32U);
    (void)memcpy(value->content_digest, web_content_digest, 32U);
    value->response_length = (uint32_t)(sizeof(adapter_text) - 1U);
    value->full_length = value->response_length;
    (void)memcpy(value->response, adapter_text, sizeof(adapter_text) - 1U);
    value->signature_count = 3U;
    (void)memcpy(value->signatures[0], web_first_sig_0, 65U);
    (void)memcpy(value->signatures[1], web_first_sig_1, 65U);
    (void)memcpy(value->signatures[2], web_first_sig_2, 65U);
}

static int hex_value(int character)
{
    if (character >= '0' && character <= '9') return character - '0';
    if (character >= 'a' && character <= 'f') return character - 'a' + 10;
    return -1;
}

static int fixture_load(size_t *length)
{
    FILE *file = fopen(fixture_path, "rb");
    int character;
    int high = -1;
    size_t count = 0U;
    if (file == NULL) return 1;
    while ((character = fgetc(file)) != EOF) {
        int value;
        if (character == '\n') continue;
        value = hex_value(character);
        if (value < 0 || count == sizeof(fixture)) {
            (void)fclose(file);
            return 1;
        }
        if (high < 0) {
            high = value;
        } else {
            fixture[count++] = (uint8_t)((high << 4) | value);
            high = -1;
        }
    }
    if (fclose(file) != 0 || high >= 0 || count == 0U) return 1;
    *length = count;
    return 0;
}

static lxp_result feed_poll(void *context, lx_web_observation *value,
                            bool *available)
{
    adapter_feed *feed = (adapter_feed *)context;
    if (feed->remaining == 0U) {
        *available = false;
        return LXP_OK;
    }
    observation_fill(value);
    value->request_id += feed->submitted;
    --feed->remaining;
    *available = true;
    return LXP_OK;
}

static lxp_result feed_submit(void *context, const uint8_t *activity,
                              size_t activity_length)
{
    adapter_feed *feed = (adapter_feed *)context;
    if (feed->submitted == 2U || activity_length > sizeof(submissions[0]))
        return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(submissions[feed->submitted], activity, activity_length);
    submission_lengths[feed->submitted] = activity_length;
    ++feed->submitted;
    return LXP_OK;
}

static int encodes_payload(void)
{
    size_t length;
    ADAPTER_CHECK(lx_web_observation_encode(&observation, payload,
                                            sizeof(payload), &length) ==
                  LXP_OK);
    ADAPTER_CHECK(length == LX_WEB_OBSERVATION_HEADER_BYTES +
                                sizeof(adapter_text) - 1U + 1U + 3U * 65U);
    ADAPTER_CHECK(payload[0] == LX_WEB_ORIGIN_PROGRAM &&
                  lxp_ct_is_zero(payload + 1U, 28U) &&
                  payload[32] == ADAPTER_NETWORK);
    ADAPTER_CHECK(payload[33] == 0xa0U && payload[64] == 0xbfU);
    ADAPTER_CHECK(payload[65] == 1U && payload[72] == 8U &&
                  payload[73] == LX_WEB_KIND_FETCH);
    ADAPTER_CHECK(memcmp(payload + 74U, web_payload_hash, 32U) == 0 &&
                  memcmp(payload + 106U, web_content_digest, 32U) == 0);
    ADAPTER_CHECK(payload[141] == 16U && payload[145] == 16U);
    ADAPTER_CHECK(memcmp(payload + 146U, adapter_text, 16U) == 0 &&
                  payload[162] == 3U &&
                  memcmp(payload + 163U, web_first_sig_0, 65U) == 0 &&
                  memcmp(payload + 293U, web_first_sig_2, 65U) == 0);
    ADAPTER_CHECK(lx_web_observation_decode(payload, length,
                                            &decoded_observation) == LXP_OK);
    ADAPTER_CHECK(decoded_observation.request_id == observation.request_id &&
                  decoded_observation.signature_count == 3U &&
                  memcmp(decoded_observation.response, adapter_text,
                         16U) == 0);
    ADAPTER_CHECK(lx_web_observation_encode(&observation, payload,
                                            length - 1U, &length) ==
                  LXP_ERR_LENGTH_LIMIT);
    observation.signature_count = 0U;
    ADAPTER_CHECK(lx_web_observation_encode(&observation, payload,
                                            sizeof(payload), &length) ==
                  LXP_ERR_NON_CANONICAL);
    observation.signature_count = LX_WEB_MAX_ATTESTORS + 1U;
    ADAPTER_CHECK(lx_web_observation_encode(&observation, payload,
                                            sizeof(payload), &length) ==
                  LXP_ERR_NON_CANONICAL);
    observation_fill(&observation);
    observation.kind = 3U;
    ADAPTER_CHECK(lx_web_observation_encode(&observation, payload,
                                            sizeof(payload), &length) ==
                  LXP_ERR_NON_CANONICAL);
    observation_fill(&observation);
    observation.response_length = LX_WEB_MAX_RESPONSE_BYTES + 1U;
    observation.full_length = observation.response_length;
    ADAPTER_CHECK(lx_web_observation_encode(&observation, payload,
                                            sizeof(payload), &length) ==
                  LXP_ERR_NON_CANONICAL);
    observation_fill(&observation);
    observation.full_length = 15U;
    ADAPTER_CHECK(lx_web_observation_encode(&observation, payload,
                                            sizeof(payload), &length) ==
                  LXP_ERR_NON_CANONICAL);
    observation_fill(&observation);
    return 0;
}

static int encodes_activity(size_t fixture_length)
{
    lxp_activity decoded;
    lxp_arena arena;
    lxp_byte_span encoded;
    size_t payload_length;
    ADAPTER_CHECK(lx_web_observation_encode(&observation, payload,
                                            sizeof(payload),
                                            &payload_length) == LXP_OK);
    ADAPTER_CHECK(lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) ==
                  LXP_OK);
    ADAPTER_CHECK(lx_web_activity_encode(&observation, adapter_seed,
                                         ADAPTER_NETWORK, adapter_actor,
                                         sizeof(adapter_actor) - 1U,
                                         ADAPTER_SEQUENCE,
                                         (lxp_u128){ 0U, 100U },
                                         adapter_bound, &arena,
                                         &encoded) == LXP_OK);
    ADAPTER_CHECK(encoded.length == fixture_length &&
                  memcmp(encoded.bytes, fixture, fixture_length) == 0);
    ADAPTER_CHECK(lxp_activity_decode(encoded.bytes, encoded.length,
                                      &decoded) == LXP_OK);
    ADAPTER_CHECK(decoded.protocol_version == LXP_PROTOCOL_VERSION &&
                  decoded.network_id == ADAPTER_NETWORK &&
                  decoded.activity_type == LX_WEB_OBSERVATION_ACTIVITY &&
                  decoded.account_sequence == ADAPTER_SEQUENCE &&
                  decoded.timestamp_bound.not_before == 1000U &&
                  decoded.timestamp_bound.not_after == 61000U &&
                  decoded.fee_limit.lo == 100U &&
                  decoded.actor_did.length == sizeof(adapter_actor) - 1U);
    ADAPTER_CHECK(decoded.payload.length == payload_length &&
                  memcmp(decoded.payload.bytes, payload,
                         payload_length) == 0);
    ADAPTER_CHECK(lxp_activity_check_envelope(&decoded, ADAPTER_NETWORK) ==
                  LXP_OK);
    ADAPTER_CHECK(lxp_activity_verify_payload_hash(&decoded) == LXP_OK);
    ADAPTER_CHECK(lxp_activity_verify_signature(&decoded) == LXP_OK);
    ADAPTER_CHECK(lx_web_observation_decode(decoded.payload.bytes,
                                            decoded.payload.length,
                                            &decoded_observation) == LXP_OK);
    ADAPTER_CHECK(memcmp(decoded_observation.signatures[1], web_first_sig_1,
                         65U) == 0);

    ADAPTER_CHECK(lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) ==
                  LXP_OK);
    ADAPTER_CHECK(lx_web_activity_encode(&observation, adapter_seed,
                                         ADAPTER_NETWORK + 1U, adapter_actor,
                                         sizeof(adapter_actor) - 1U,
                                         ADAPTER_SEQUENCE,
                                         (lxp_u128){ 0U, 100U },
                                         adapter_bound, &arena,
                                         &encoded) == LXP_ERR_NON_CANONICAL);
    ADAPTER_CHECK(lx_web_activity_encode(&observation, adapter_seed,
                                         ADAPTER_NETWORK, adapter_actor,
                                         sizeof(adapter_actor) - 1U,
                                         ADAPTER_SEQUENCE,
                                         (lxp_u128){ 0U, 0U },
                                         adapter_bound, &arena,
                                         &encoded) == LXP_ERR_NON_CANONICAL);
    ADAPTER_CHECK(lx_web_activity_encode(
                      &observation, adapter_seed, ADAPTER_NETWORK,
                      adapter_actor, sizeof(adapter_actor) - 1U,
                      ADAPTER_SEQUENCE, (lxp_u128){ 0U, 100U },
                      (lxp_timestamp_bound){ 61000U, 1000U }, &arena,
                      &encoded) == LXP_ERR_NON_CANONICAL);
    return 0;
}

static int adapter_runs(size_t fixture_length)
{
    lx_web_adapter_config config;
    adapter_feed feed = { 2U, 0U };
    size_t submitted = 0U;
    (void)memset(&config, 0, sizeof(config));
    config.poll_observations = feed_poll;
    config.poll_context = &feed;
    config.submit_activity = feed_submit;
    config.submit_context = &feed;
    (void)memcpy(config.submitter_private_key, adapter_seed, 32U);
    config.network_id = ADAPTER_NETWORK;
    config.actor_did = adapter_actor;
    config.actor_did_length = sizeof(adapter_actor) - 1U;
    config.next_account_sequence = ADAPTER_SEQUENCE;
    config.fee_limit = (lxp_u128){ 0U, 100U };
    config.timestamp_bound = adapter_bound;
    config.maximum_observations = 4U;
    ADAPTER_CHECK(lx_web_adapter_run(&config, &submitted) == LXP_OK);
    ADAPTER_CHECK(submitted == 2U && feed.submitted == 2U &&
                  config.next_account_sequence == ADAPTER_SEQUENCE + 2U);
    ADAPTER_CHECK(submission_lengths[0] == fixture_length &&
                  memcmp(submissions[0], fixture, fixture_length) == 0);
    ADAPTER_CHECK(submission_lengths[1] == fixture_length &&
                  memcmp(submissions[1], fixture, fixture_length) != 0);
    config.maximum_observations = 0U;
    ADAPTER_CHECK(lx_web_adapter_run(&config, &submitted) ==
                  LXP_ERR_NON_CANONICAL);
    return 0;
}

int main(void)
{
    size_t fixture_length = 0U;
    ADAPTER_CHECK(fixture_load(&fixture_length) == 0);
    observation_fill(&observation);
    if (encodes_payload() != 0 || encodes_activity(fixture_length) != 0 ||
        adapter_runs(fixture_length) != 0)
        return 1;
    return 0;
}
