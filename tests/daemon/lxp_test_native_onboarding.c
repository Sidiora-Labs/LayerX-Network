#define main program_admission_client_main
int program_admission_client_main(int argc, char **argv);
#include "lxp_test_program_admission.c"
#undef main
#include "layerx/lxp_ledger.h"
#include "layerx/lx_budget.h"

enum { ONBOARD_RECEIPTS = 64 };
typedef struct onboarding_run {
    uint64_t owner_sequence;
    uint64_t target_sequence;
    uint64_t generation;
    size_t count;
    uint8_t ids[ONBOARD_RECEIPTS][32];
    lxp_result results[ONBOARD_RECEIPTS];
    uint8_t root[32];
} onboarding_run;
static const char *onboarding_path;
static const uint8_t onboarding_asset[32] = {
    0xb5,0xa3,0x2b,0x12,0x02,0x9f,0x8d,0xdf,0xb9,0x05,0xf9,0x0f,0x28,0x0f,0x66,0x4b,
    0x46,0x39,0x0d,0xe0,0xfc,0x62,0x77,0x0f,0xc1,0x97,0xdd,0x87,0xb1,0x8c,0xd8,0x98
};
static int onboard_owner(signer *owner)
{
    static const char digits[] = "0123456789abcdef";
    REQUIRE(signer_init(owner, 0x11U) == 0);
    (void)memcpy(REGISTERED_DID, "did:layerx:", 11U);
    for (size_t i = 0U; i < 32U; ++i) {
        REGISTERED_DID[11U + i * 2U] = (uint8_t)digits[owner->public_key[i] >> 4U];
        REGISTERED_DID[12U + i * 2U] = (uint8_t)digits[owner->public_key[i] & 15U];
    }
    return 0;
}

static int onboard_head(int descriptor, uint64_t minimum, uint8_t root[32])
{
    struct sockaddr_un address;
    socklen_t address_length = sizeof(address);
    uint8_t preparation[79];
    store_u16(preparation, 1U); store_u16(preparation + 2U, 75U);
    (void)memcpy(preparation + 4U, REGISTERED_DID, 75U);
    REQUIRE(getpeername(descriptor, (struct sockaddr *)&address, &address_length) == 0);
    REQUIRE(close(descriptor) == 0);
    for (unsigned attempt = 0U; attempt < 200U; ++attempt) {
        wire_envelope response;
        int current = socket(AF_UNIX, SOCK_STREAM, 0);
        REQUIRE(current >= 0 && connect(current, (struct sockaddr *)&address, address_length) == 0);
        REQUIRE(handshake(current) == 0);
        REQUIRE(send_request(current, LNI_MINOR, 26U, 613U, preparation, sizeof(preparation)) == 0);
        REQUIRE(receive_envelope(current, &response) == 0);
        bool reached = false;
        if (response.tag == ERROR_RESPONSE) {
            REQUIRE(response.correlation_id == 613U && response.payload_length == 5U &&
                    response.proof_length == 0U && response.payload[0] == 4U);
            lxp_result refusal = (lxp_result)load_u32(response.payload + 1U);
            (void)fprintf(stderr, "onboarding preparation attempt=%u refusal=%d\n",
                          attempt, refusal);
            REQUIRE(refusal == LXP_ERR_MODULE_DISABLED || refusal == LXP_ERR_PROJECTION_STALE);
        } else {
            REQUIRE(response.tag == 27U && response.correlation_id == 613U &&
                    response.payload_length >= 139U);
            reached = load_u64(response.payload + 99U) >= minimum;
            if (reached) (void)memcpy(root, response.payload + 107U, 32U);
        }
        release_envelope(&response);
        if (reached) {
            if (current != descriptor) {
                REQUIRE(dup2(current, descriptor) == descriptor);
                REQUIRE(close(current) == 0);
            }
            return 0;
        }
        REQUIRE(close(current) == 0);
        const struct timespec pause = {0, 50000000L};
        REQUIRE(nanosleep(&pause, NULL) == 0);
    }
    REQUIRE(false);
}

static void onboard_did(const signer *key, uint8_t did[75])
{
    static const char digits[] = "0123456789abcdef";
    (void)memcpy(did, "did:layerx:", 11U);
    for (size_t i = 0U; i < 32U; ++i) {
        did[11U + 2U * i] = (uint8_t)digits[key->public_key[i] >> 4U];
        did[12U + 2U * i] = (uint8_t)digits[key->public_key[i] & 15U];
    }
}

static int onboard_file(size_t index, const char *suffix, const uint8_t *bytes, size_t length)
{
    char path[4096];
    int count = snprintf(path, sizeof(path), "%s.%02zu.%s", onboarding_path, index, suffix);
    REQUIRE(count > 0 && (size_t)count < sizeof(path));
    FILE *file = fopen(path, "wbx");
    REQUIRE(file != NULL && fwrite(bytes, 1U, length, file) == length && fflush(file) == 0 &&
        fsync(fileno(file)) == 0 && fclose(file) == 0);
    return 0;
}

static int onboard_receipt(int descriptor, const uint8_t id[32], lxp_result expected,
    size_t index, bool retain, lxp_receipt *receipt)
{
    uint8_t query[33] = {1U};
    (void)memcpy(query + 1U, id, 32U);
    for (unsigned attempt = 0U; attempt < 200U; ++attempt) {
        wire_envelope response;
        REQUIRE(send_request(descriptor, LNI_MINOR, 5U, 701U, query, sizeof(query)) == 0);
        REQUIRE(receive_envelope(descriptor, &response) == 0);
        REQUIRE(response.tag == 6U && response.correlation_id == 701U);
        if (response.payload_length != 0U) {
            uint8_t storage[2U * LXP_MAX_ACTIVITY_BYTES];
            lxp_arena arena;
            signer sequencer;
            REQUIRE(signer_init(&sequencer, 0x22U) == 0);
            REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
            REQUIRE(lxp_receipt_decode(response.payload, response.payload_length, true, receipt) == LXP_OK);
            REQUIRE(lxp_receipt_verify(receipt, sequencer.public_key, &arena) == LXP_OK);
            REQUIRE(memcmp(receipt->activity_id, id, 32U) == 0 && receipt->protocol_version == 3U && receipt->network_id == 77U);
            if (receipt->result_code != expected) (void)fprintf(stderr, "onboarding receipt=%d expected=%d index=%zu\n", receipt->result_code, expected, index);
            REQUIRE(receipt->result_code == expected);
            if (retain) REQUIRE(onboard_file(index, "receipt", response.payload, response.payload_length) == 0);
            release_envelope(&response);
            return 0;
        }
        release_envelope(&response);
        const struct timespec pause = {0, 50000000L};
        REQUIRE(nanosleep(&pause, NULL) == 0);
    }
    REQUIRE(false);
}

static int onboard_encode(const signer *key, uint64_t sequence, uint32_t type,
    uint64_t timestamp, const uint8_t action_key[32], lxp_u128 fee,
    const uint8_t *payload, size_t payload_length, uint8_t *bytes, size_t *length)
{
    uint8_t did[75], storage[LXP_MAX_ACTIVITY_BYTES], digest[32], signature[64];
    lxp_activity activity = {0};
    lxp_arena arena;
    lxp_byte_span encoded;
    onboard_did(key, did);
    activity.protocol_version = 3U;
    activity.network_id = NETWORK_ID;
    activity.activity_type = type;
    activity.actor_did = (lxp_byte_span){did, sizeof(did)};
    activity.authority = (lxp_byte_span){key->public_key, 32U};
    activity.account_sequence = sequence;
    activity.timestamp_bound.not_before = timestamp;
    activity.timestamp_bound.not_after = timestamp + 300000U;
    (void)memcpy(activity.idempotency_key, action_key, 32U);
    activity.fee_limit = fee;
    activity.payload = (lxp_byte_span){payload, payload_length};
    REQUIRE(lxp_hash_payload(payload, payload_length, activity.payload_hash) == LXP_OK);
    REQUIRE(lxp_activity_signing_preimage(&activity, digest) == LXP_OK && sign_raw(key, digest, 32U, signature) == 0);
    activity.signature = (lxp_byte_span){signature, sizeof(signature)};
    REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
    REQUIRE(lxp_activity_encode(&activity, &arena, &encoded) == LXP_OK && encoded.length <= ACTIVITY_CAPACITY);
    (void)memcpy(bytes, encoded.bytes, encoded.length);
    *length = encoded.length;
    return 0;
}

static int onboard_submit(int descriptor, onboarding_run *run, const uint8_t *bytes,
    size_t length, lxp_result expected, lxp_receipt *receipt)
{
    REQUIRE(run->count < ONBOARD_RECEIPTS);
    size_t index = run->count;
    REQUIRE(lxp_activity_id(bytes, length, run->ids[index]) == LXP_OK);
    REQUIRE(send_request(descriptor, LNI_MINOR, SUBMIT_REQUEST, 702U, bytes, length) == 0);
    REQUIRE(expect_ack(descriptor, 702U, bytes, length, run->ids[index]) == 0);
    REQUIRE(onboard_file(index, "activity", bytes, length) == 0);
    REQUIRE(onboard_receipt(descriptor, run->ids[index], expected, index, true, receipt) == 0);
    run->results[index] = expected;
    ++run->count;
    REQUIRE(onboard_head(descriptor, receipt->global_sequence + 1U, run->root) == 0);
    return 0;
}

static int onboard_wrong_network(const signer *key, uint8_t *bytes, size_t *length)
{
    lxp_activity activity;
    lxp_arena arena;
    lxp_byte_span encoded;
    uint8_t storage[LXP_MAX_ACTIVITY_BYTES], digest[32], signature[64];
    REQUIRE(lxp_activity_decode(bytes, *length, &activity) == LXP_OK);
    activity.network_id = NETWORK_ID + 1U;
    REQUIRE(lxp_activity_signing_preimage(&activity, digest) == LXP_OK && sign_raw(key, digest, 32U, signature) == 0);
    activity.signature = (lxp_byte_span){signature, sizeof(signature)};
    REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK && lxp_activity_encode(&activity, &arena, &encoded) == LXP_OK);
    REQUIRE(encoded.length <= ACTIVITY_CAPACITY);
    (void)memcpy(bytes, encoded.bytes, encoded.length); *length = encoded.length;
    return 0;
}

static int onboard_account(int descriptor, const uint8_t id[32], const uint8_t asset[32],
    lxp_u128 *balance, uint64_t *sequence, bool exists)
{
    uint8_t query[37] = {0U, 1U, 2U};
    (void)memcpy(query + 3U, id, 32U); query[35U] = 1U; query[36U] = 3U;
    wire_envelope response;
    REQUIRE(send_request(descriptor, LNI_MINOR, 7U, 703U, query, sizeof(query)) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    if (!exists) {
        REQUIRE(response.tag == ERROR_RESPONSE && response.payload_length == 5U &&
            (lxp_result)load_u32(response.payload + 1U) == LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE);
    } else {
        REQUIRE(response.tag == 8U && response.payload_length >= 102U && response.proof_length != 0U);
        const uint8_t *value = response.payload;
        size_t names = load_u16(value), offset = 3U + names;
        uint8_t actual[32];
        REQUIRE(names != 0U && names <= LX_ACCOUNT_NAME_MAX && response.payload_length == names + 102U);
        REQUIRE(lx_account_id_from_string(value + 2U, names, actual) == LXP_OK && memcmp(actual, id, 32U) == 0);
        REQUIRE(lxp_u128_from_be(value + offset, balance) == LXP_OK);
        REQUIRE(memcmp(value + offset + 16U, asset, 32U) == 0 && value[offset + 48U] == 1U);
        *sequence = load_u64(value + offset + 49U);
    }
    release_envelope(&response);
    return 0;
}

static int onboard_main_id(const signer *key, uint8_t id[32])
{
    uint8_t name[86];
    (void)memcpy(name, "agent:", 6U); onboard_did(key, name + 6U);
    (void)memcpy(name + 81U, ":main", 5U);
    REQUIRE(lx_account_id_from_string(name, sizeof(name), id) == LXP_OK);
    return 0;
}

static int onboard_execute(int descriptor, onboarding_run *run, const signer *key, uint64_t *sequence,
    uint32_t type, const uint8_t *payload, size_t payload_length, lxp_result expected, lxp_receipt *receipt)
{
    uint8_t encoded[ACTIVITY_CAPACITY], action[32] = {0};
    struct timespec now;
    size_t length;
    REQUIRE(clock_gettime(CLOCK_REALTIME, &now) == 0);
    action[0] = 0x90U; store_u64(action + 24U, run->count + 1U);
    REQUIRE(onboard_encode(key, *sequence, type, (uint64_t)now.tv_sec * 1000U, action,
        (lxp_u128){0U, 67108864U}, payload, payload_length, encoded, &length) == 0);
    REQUIRE(onboard_submit(descriptor, run, encoded, length, expected, receipt) == 0);
    ++*sequence;
    return 0;
}

static int onboard_budget(int descriptor, onboarding_run *run, const signer *owner, const signer *target)
{
    uint8_t create[LX_BUDGET_CREATE_V2_PAYLOAD_BYTES] = {0U, 2U};
    uint8_t fund[LX_BUDGET_FUND_V2_PAYLOAD_BYTES] = {0U, 2U};
    uint8_t close_budget[LX_BUDGET_CLOSE_PAYLOAD_BYTES] = {0U, 1U};
    uint8_t target_id[32], owner_id[32], budget_id[32], budget_account[32], name[153];
    static const char digits[] = "0123456789abcdef";
    lxp_u128 before, after, expected, budget_balance;
    uint64_t sequence, budget_sequence;
    lxp_receipt receipt;
    struct timespec now;
    REQUIRE(clock_gettime(CLOCK_REALTIME, &now) == 0);
    REQUIRE(onboard_main_id(target, target_id) == 0 && onboard_main_id(owner, owner_id) == 0);
    (void)memset(budget_id, 0x81U, 32U);
    (void)memcpy(name, "agent:", 6U); onboard_did(target, name + 6U);
    (void)memcpy(name + 81U, ":budget:", 8U);
    for (size_t i = 0U; i < 32U; ++i) {
        name[89U + i * 2U] = (uint8_t)digits[budget_id[i] >> 4U];
        name[90U + i * 2U] = (uint8_t)digits[budget_id[i] & 15U];
    }
    REQUIRE(lx_account_id_from_string(name, sizeof(name), budget_account) == LXP_OK);
    (void)memcpy(create + 2U, budget_id, 32U); (void)memcpy(create + 34U, budget_account, 32U);
    (void)memcpy(create + 66U, onboarding_asset, 32U); create[98U] = 0x91U;
    store_u64(create + 138U, 2000U); store_u64(create + 170U, 1000U);
    store_u64(create + 178U, 60000U); store_u64(create + 186U, (uint64_t)now.tv_sec * 1000U);
    store_u64(create + 194U, (uint64_t)now.tv_sec * 1000U + 300000U); create[210U] = 1U;
    REQUIRE(onboard_account(descriptor, target_id, onboarding_asset, &before, &sequence, true) == 0);
    (void)memcpy(create + 211U, owner_id, 32U); store_u64(create + 243U, sequence);
    REQUIRE(onboard_execute(descriptor, run, target, &run->target_sequence, LX_BUDGET_CREATE, create, sizeof(create), LXP_ERR_UNAUTHORIZED_DEBIT, &receipt) == 0);
    REQUIRE(onboard_account(descriptor, budget_account, onboarding_asset, &after, &budget_sequence, false) == 0);
    REQUIRE(lxp_u128_sub(before, receipt.fee_charged, &expected) == LXP_OK);
    REQUIRE(onboard_account(descriptor, target_id, onboarding_asset, &after, &sequence, true) == 0 && lxp_u128_cmp(after, expected) == 0);
    before = after;
    (void)memcpy(create + 211U, target_id, 32U); store_u64(create + 243U, sequence + 1U);
    REQUIRE(onboard_execute(descriptor, run, target, &run->target_sequence, LX_BUDGET_CREATE, create, sizeof(create), LXP_ERR_CONTEXT_MISMATCH, &receipt) == 0);
    REQUIRE(onboard_account(descriptor, budget_account, onboarding_asset, &after, &budget_sequence, false) == 0);
    REQUIRE(lxp_u128_sub(before, receipt.fee_charged, &expected) == LXP_OK);
    REQUIRE(onboard_account(descriptor, target_id, onboarding_asset, &after, &sequence, true) == 0 && lxp_u128_cmp(after, expected) == 0);
    before = after; store_u64(create + 243U, sequence);
    REQUIRE(onboard_execute(descriptor, run, target, &run->target_sequence, LX_BUDGET_CREATE, create, sizeof(create), LXP_OK, &receipt) == 0);
    REQUIRE(lxp_u128_sub(before, receipt.fee_charged, &expected) == LXP_OK && lxp_u128_sub(expected, (lxp_u128){0U, 1000U}, &expected) == LXP_OK);
    REQUIRE(onboard_account(descriptor, target_id, onboarding_asset, &after, &sequence, true) == 0 && lxp_u128_cmp(after, expected) == 0);
    REQUIRE(onboard_account(descriptor, budget_account, onboarding_asset, &budget_balance, &budget_sequence, true) == 0 && budget_balance.hi == 0U && budget_balance.lo == 1000U);
    (void)memcpy(fund + 2U, budget_id, 32U); store_u64(fund + 42U, 200U); store_u64(fund + 50U, sequence);
    before = after;
    REQUIRE(onboard_execute(descriptor, run, target, &run->target_sequence, LX_BUDGET_FUND, fund, sizeof(fund), LXP_OK, &receipt) == 0);
    REQUIRE(lxp_u128_sub(before, receipt.fee_charged, &expected) == LXP_OK && lxp_u128_sub(expected, (lxp_u128){0U, 200U}, &expected) == LXP_OK);
    REQUIRE(onboard_account(descriptor, target_id, onboarding_asset, &after, &sequence, true) == 0 && lxp_u128_cmp(after, expected) == 0);
    REQUIRE(onboard_account(descriptor, budget_account, onboarding_asset, &budget_balance, &budget_sequence, true) == 0 && budget_balance.hi == 0U && budget_balance.lo == 1200U);
    before = after;
    (void)memcpy(close_budget + 2U, budget_id, 32U); store_u64(close_budget + 34U, 1U);
    REQUIRE(onboard_execute(descriptor, run, target, &run->target_sequence, LX_BUDGET_CLOSE, close_budget, sizeof(close_budget), LXP_OK, &receipt) == 0);
    REQUIRE(lxp_u128_sub(before, receipt.fee_charged, &expected) == LXP_OK && lxp_u128_add(expected, (lxp_u128){0U, 1200U}, &expected) == LXP_OK);
    REQUIRE(onboard_account(descriptor, target_id, onboarding_asset, &after, &sequence, true) == 0 && lxp_u128_cmp(after, expected) == 0);
    REQUIRE(onboard_account(descriptor, budget_account, onboarding_asset, &budget_balance, &budget_sequence, true) == 0 && lxp_u128_is_zero(budget_balance));
    puts("native managed Budget binds exact source sequence, charges refused fees, funds once and returns the complete remainder");
    return 0;
}

static int onboard_initial(int descriptor, const signer *owner, const signer *target, onboarding_run *run)
{
    uint8_t encoded[ACTIVITY_CAPACITY], payload[1024], inner[ACTIVITY_CAPACITY];
    uint8_t owner_id[32], target_id[32], did[75], owner_did[32], target_did[32];
    lxp_receipt receipt;
    lxp_u128 balance;
    uint64_t account_sequence;
    size_t length, inner_length;
    const char *credit_path = getenv("LAYERX_TEST_WITHDRAW_CREDIT");
    REQUIRE(credit_path != NULL);
    FILE *credit = fopen(credit_path, "rb");
    REQUIRE(credit != NULL);
    length = fread(encoded, 1U, sizeof(encoded), credit);
    REQUIRE(length != 0U && length < sizeof(encoded) && !ferror(credit) && fclose(credit) == 0);
    REQUIRE(onboard_submit(descriptor, run, encoded, length, LXP_OK, &receipt) == 0);
    ++run->owner_sequence;
    onboard_did(owner, did);
    REQUIRE(lxp_did_id_derive(did, sizeof(did), owner_did) == LXP_OK);
    onboard_did(target, did);
    REQUIRE(lxp_did_id_derive(did, sizeof(did), target_did) == LXP_OK);
    REQUIRE(onboard_main_id(owner, owner_id) == 0 && onboard_main_id(target, target_id) == 0);
    (void)memset(payload, 0, sizeof(payload));
    payload[0] = 0x71U; payload[1] = 1U; payload[3] = 2U;
    (void)memcpy(payload + 4U, owner_did, 32U); (void)memcpy(payload + 36U, owner->public_key, 32U);
    REQUIRE(onboard_execute(descriptor, run, owner, &run->owner_sequence, 0x00070001U, payload, 68U, LXP_OK, &receipt) == 0);
    REQUIRE(onboard_account(descriptor, target_id, onboarding_asset, &balance, &account_sequence, false) == 0);
    for (uint8_t variant = 0U; variant < 8U; ++variant) {
        uint8_t consent[140] = {0x71U, 1U, 3U, 5U}, action[32] = {0xa1U};
        struct timespec now;
        REQUIRE(clock_gettime(CLOCK_REALTIME, &now) == 0);
        uint64_t timestamp = (uint64_t)now.tv_sec * 1000U;
        action[31U] = variant + 1U;
        (void)memcpy(consent + 4U, owner_did, 32U);
        (void)memcpy(consent + 36U, onboarding_asset, 32U);
        (void)memcpy(consent + 68U, target_id, 32U);
        (void)memcpy(consent + 100U, action, 32U); store_u64(consent + 132U, timestamp + 300000U);
        if (variant < 5U) consent[(size_t[]){4U, 36U, 68U, 100U, 132U}[variant]] ^= 1U;
        REQUIRE(onboard_encode(target, 0U, 0x00070001U, timestamp, action, (lxp_u128){0U, 0U}, consent, sizeof(consent), inner, &inner_length) == 0);
        if (variant == 5U) inner[inner_length - 1U] ^= 1U;
        if (variant == 6U) REQUIRE(onboard_wrong_network(target, inner, &inner_length) == 0);
        if (variant == 7U) {
            REQUIRE(send_request(descriptor, LNI_MINOR, SUBMIT_REQUEST, 706U, inner, inner_length) == 0);
            REQUIRE(expect_error(descriptor, 706U, 4U, LXP_ERR_UNKNOWN_DID) == 0);
        }
        payload[0] = 0x71U; payload[1] = 1U; payload[2] = 2U; payload[3] = 1U;
        REQUIRE(inner_length <= sizeof(payload) - 8U);
        store_u32(payload + 4U, (uint32_t)inner_length); (void)memcpy(payload + 8U, inner, inner_length);
        REQUIRE(onboard_encode(owner, run->owner_sequence, 0x00070001U, timestamp, action,
            (lxp_u128){0U, 67108864U}, payload, inner_length + 8U, encoded, &length) == 0);
        lxp_result expected = variant == 7U ? LXP_OK : variant == 5U ? LXP_ERR_BAD_SIGNATURE : LXP_ERR_AUTH_SCOPE;
        REQUIRE(onboard_submit(descriptor, run, encoded, length, expected, &receipt) == 0);
        ++run->owner_sequence;
        REQUIRE(!lxp_u128_is_zero(receipt.fee_charged));
        REQUIRE(onboard_account(descriptor, target_id, onboarding_asset, &balance, &account_sequence, variant == 7U) == 0);
        if (variant == 7U) {
            REQUIRE(lxp_u128_is_zero(balance) && account_sequence == 0U);
            run->generation = receipt.global_sequence;
            uint8_t replay_id[32];
            REQUIRE(send_request(descriptor, LNI_MINOR, SUBMIT_REQUEST, 705U, encoded, length) == 0);
            REQUIRE(expect_ack(descriptor, 705U, encoded, length, replay_id) == 0);
            REQUIRE(memcmp(replay_id, run->ids[run->count - 1U], 32U) == 0);
            REQUIRE(onboard_head(descriptor, 0U, replay_id) == 0 && memcmp(replay_id, run->root, 32U) == 0);
        }
    }
    REQUIRE(onboard_account(descriptor, owner_id, onboarding_asset, &balance, &account_sequence, true) == 0);
    lxp_send send = {0};
    uint8_t material[144], message[512], digest[32];
    (void)memcpy(send.from, owner_id, 32U); (void)memcpy(send.to, target_id, 32U);
    (void)memcpy(send.asset, onboarding_asset, 32U); send.amount.lo = 200000000U;
    send.sequence = account_sequence; send.idempotency_key[0] = 0x90U; store_u64(send.idempotency_key + 24U, run->count + 1U);
    struct timespec now;
    REQUIRE(clock_gettime(CLOCK_REALTIME, &now) == 0);
    send.expires_at = (uint64_t)now.tv_sec * 1000U + 300000U;
    (void)memcpy(material, send.from, 32U); (void)memcpy(material + 32U, send.to, 32U);
    (void)memcpy(material + 64U, send.asset, 32U); REQUIRE(lxp_u128_to_be(send.amount, material + 96U) == LXP_OK);
    (void)memcpy(material + 112U, send.idempotency_key, 32U);
    REQUIRE(lxp_hash_context_value(material, sizeof(material), send.context_hash) == LXP_OK);
    send.authorization.kind = LXP_AUTH_OWNER; send.authorization.network_id = NETWORK_ID; send.authorization.protocol_version = 3U;
    (void)memcpy(send.authorization.controller, owner_id, 32U);
    (void)memcpy(send.authorization.public_key, owner->public_key, 32U);
    (void)memcpy(send.authorization.signed_context_hash, send.context_hash, 32U);
    REQUIRE(lxp_send_authorization_message(&send, message, sizeof(message), &length) == LXP_OK);
    REQUIRE(lxp_hash_signature_preimage(message, length, digest) == LXP_OK);
    REQUIRE(sign_raw(owner, digest, 32U, send.authorization.signature) == 0);
    REQUIRE(lxp_send_encode(&send, payload, sizeof(payload), &length) == LXP_OK);
    REQUIRE(onboard_execute(descriptor, run, owner, &run->owner_sequence, 0x00010005U, payload, length, LXP_OK, &receipt) == 0);
    REQUIRE(onboard_account(descriptor, target_id, onboarding_asset, &balance, &account_sequence, true) == 0);
    REQUIRE(balance.hi == 0U && balance.lo == 200000000U && account_sequence == 0U);
    (void)memset(payload, 0, sizeof(payload));
    payload[0] = 0x71U; payload[1] = 1U; payload[2] = 3U; payload[3] = 5U;
    REQUIRE(onboard_execute(descriptor, run, target, &run->target_sequence, 0x00070001U, payload, 140U, LXP_ERR_NON_CANONICAL, &receipt) == 0);
    (void)memset(payload, 0, sizeof(payload));
    payload[0] = 0x71U; payload[1] = 3U; payload[3] = 3U;
    (void)memcpy(payload + 4U, target_did, 32U); payload[36U] = 0x72U; store_u16(payload + 68U, 2U);
    REQUIRE(onboard_execute(descriptor, run, target, &run->target_sequence, 0x00070003U, payload, 70U, LXP_OK, &receipt) == 0);
    REQUIRE(!lxp_u128_is_zero(receipt.fee_charged));
    REQUIRE(onboard_budget(descriptor, run, owner, target) == 0);
    puts("sponsored owner consent registers a zero-balance identity; invalid consent charges only the sponsor; real funding and target recovery execute");
    return 0;
}

int main(int argc, char **argv)
{
    struct sockaddr_un address = {0};
    onboarding_run run = {0};
    signer owner, target;
    REQUIRE(argc == 4 && strlen(argv[1]) < sizeof(address.sun_path));
    bool recovered = strcmp(argv[2], "--native-onboarding-recovered") == 0;
    REQUIRE(recovered || strcmp(argv[2], "--native-onboarding") == 0);
    onboarding_path = argv[3];
    REQUIRE(onboard_owner(&owner) == 0 && signer_init(&target, 0x63U) == 0);
    address.sun_family = AF_UNIX; (void)memcpy(address.sun_path, argv[1], strlen(argv[1]) + 1U);
    int descriptor = socket(AF_UNIX, SOCK_STREAM, 0);
    REQUIRE(descriptor >= 0 && connect(descriptor, (struct sockaddr *)&address, sizeof(address)) == 0 && handshake(descriptor) == 0);
    FILE *state;
    if (recovered) {
        state = fopen(argv[3], "rb");
        REQUIRE(state != NULL && fread(&run, sizeof(run), 1U, state) == 1U && fgetc(state) == EOF && !ferror(state) && fclose(state) == 0);
        uint8_t root[32]; lxp_receipt receipt;
        REQUIRE(onboard_head(descriptor, 0U, root) == 0 && memcmp(root, run.root, 32U) == 0);
        for (size_t i = 0U; i < run.count; ++i) REQUIRE(onboard_receipt(descriptor, run.ids[i], run.results[i], i, false, &receipt) == 0);
        uint8_t payload[70] = {0x71U, 3U, 0U, 3U}, did[75];
        onboard_did(&target, did); REQUIRE(lxp_did_id_derive(did, sizeof(did), payload + 4U) == LXP_OK);
        payload[36U] = 0x73U; store_u16(payload + 68U, 3U);
        REQUIRE(onboard_execute(descriptor, &run, &target, &run.target_sequence, 0x00070003U, payload, sizeof(payload), LXP_OK, &receipt) == 0);
        puts("daemon and authority replica restart preserve sponsored identity, sequence, original signed receipts and current owner execution");
    } else {
        REQUIRE(onboard_initial(descriptor, &owner, &target, &run) == 0);
        state = fopen(argv[3], "wbx");
        REQUIRE(state != NULL && fwrite(&run, sizeof(run), 1U, state) == 1U && fflush(state) == 0 && fsync(fileno(state)) == 0 && fclose(state) == 0);
    }
    REQUIRE(close(descriptor) == 0);
    return 0;
}
