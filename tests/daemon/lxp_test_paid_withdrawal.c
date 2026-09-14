#define main program_admission_client_main
int program_admission_client_main(int argc, char **argv);
#include "lxp_test_program_admission.c"
#undef main
#include "layerx/lxp_authority.h"
#include "layerx/lxp_codec.h"

enum { PAID_RECORDS = 6, PAID_RECEIPT_BYTES = 8192 };

typedef struct paid_account {
    uint64_t balance;
    uint64_t sequence;
} paid_account;

typedef struct paid_record {
    uint8_t activity[ACTIVITY_CAPACITY];
    size_t activity_length;
    uint8_t receipt[PAID_RECEIPT_BYTES];
    size_t receipt_length;
    uint8_t activity_id[32];
} paid_record;

typedef struct paid_state {
    paid_record records[PAID_RECORDS];
    paid_account accounts[3];
    uint8_t state_root[32];
    uint64_t identity_sequence;
} paid_state;

static const uint8_t paid_asset[32] = {
    0xb5,0xa3,0x2b,0x12,0x02,0x9f,0x8d,0xdf,0xb9,0x05,0xf9,0x0f,0x28,0x0f,0x66,0x4b,
    0x46,0x39,0x0d,0xe0,0xfc,0x62,0x77,0x0f,0xc1,0x97,0xdd,0x87,0xb1,0x8c,0xd8,0x98
};

static int paid_identity(signer *owner)
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

static int paid_encode(const signer *key, uint64_t sequence, uint32_t type,
    uint64_t limit, const uint8_t *payload, size_t length, paid_record *record)
{
    uint8_t storage[2U * LXP_MAX_ACTIVITY_BYTES], digest[32], signature[64];
    lxp_activity activity;
    lxp_arena arena;
    lxp_byte_span encoded;
    REQUIRE(build_activity(key, sequence, type, 0U, payload, length,
        record->activity, sizeof(record->activity), &record->activity_length) == 0);
    REQUIRE(lxp_activity_decode(record->activity, record->activity_length, &activity) == LXP_OK);
    activity.fee_limit = (lxp_u128){0U, limit};
    REQUIRE(lxp_activity_signing_preimage(&activity, digest) == LXP_OK);
    REQUIRE(sign_raw(key, digest, sizeof(digest), signature) == 0);
    activity.signature = (lxp_byte_span){signature, sizeof(signature)};
    REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
    REQUIRE(lxp_activity_encode(&activity, &arena, &encoded) == LXP_OK);
    REQUIRE(encoded.length <= sizeof(record->activity));
    (void)memcpy(record->activity, encoded.bytes, encoded.length);
    record->activity_length = encoded.length;
    return 0;
}

static int paid_receipt(int descriptor, paid_record *record, lxp_result result,
    uint64_t fee, bool replay)
{
    uint8_t query[34] = {1U}, storage[2U * LXP_MAX_ACTIVITY_BYTES];
    signer sequencer;
    REQUIRE(signer_init(&sequencer, 0x22U) == 0);
    (void)memcpy(query + 1U, record->activity_id, 32U);
    query[33U] = 1U;
    for (unsigned attempt = 0U; attempt < 200U; ++attempt) {
        wire_envelope response;
        REQUIRE(send_request(descriptor, LNI_MINOR, 5U, 901U, query, sizeof(query)) == 0);
        REQUIRE(receive_envelope(descriptor, &response) == 0);
        REQUIRE(response.tag == 6U && response.correlation_id == 901U && response.proof_length == 0U);
        if (response.payload_length != 0U) {
            lxp_receipt receipt;
            lxp_activity activity;
            lxp_arena arena;
            REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
            REQUIRE(lxp_activity_decode(record->activity, record->activity_length, &activity) == LXP_OK);
            REQUIRE(lxp_receipt_decode(response.payload, response.payload_length, true, &receipt) == LXP_OK);
            REQUIRE(lxp_receipt_verify(&receipt, sequencer.public_key, &arena) == LXP_OK);
            if (receipt.result_code != result)
                fprintf(stderr, "paid withdrawal receipt result=%d expected=%d\n", receipt.result_code, result);
            REQUIRE(receipt.protocol_version == 3U && activity.protocol_version == 3U && activity.network_id == NETWORK_ID);
            REQUIRE(lxp_activity_verify_signature(&activity) == LXP_OK);
            REQUIRE(memcmp(receipt.activity_id, record->activity_id, 32U) == 0);
            REQUIRE(receipt.module_id == lxp_activity_module_id(activity.activity_type));
            REQUIRE(receipt.result_code == result && receipt.fee_charged.hi == 0U && receipt.fee_charged.lo == fee);
            REQUIRE(receipt.parameter_version == 1U);
            REQUIRE(response.payload_length <= sizeof(record->receipt));
            if (replay) {
                REQUIRE(response.payload_length == record->receipt_length);
                REQUIRE(memcmp(response.payload, record->receipt, record->receipt_length) == 0);
            } else {
                (void)memcpy(record->receipt, response.payload, response.payload_length);
                record->receipt_length = response.payload_length;
            }
            release_envelope(&response);
            return 0;
        }
        release_envelope(&response);
        const struct timespec pause = {0, 50000000L};
        REQUIRE(nanosleep(&pause, NULL) == 0);
    }
    REQUIRE(false);
}

static int paid_submit(int descriptor, paid_record *record, lxp_result result,
    uint64_t fee, bool replay)
{
    REQUIRE(lxp_activity_id(record->activity, record->activity_length, record->activity_id) == LXP_OK);
    REQUIRE(send_request(descriptor, LNI_MINOR, SUBMIT_REQUEST, 900U,
        record->activity, record->activity_length) == 0);
    REQUIRE(expect_ack(descriptor, 900U, record->activity, record->activity_length, record->activity_id) == 0);
    return paid_receipt(descriptor, record, result, fee, replay);
}

static int paid_account_read(int descriptor, const char *name, paid_account *account)
{
    uint8_t query[37] = {0U, 1U, 2U};
    wire_envelope response;
    size_t name_length = strlen(name), cursor;
    REQUIRE(lx_account_id_from_string((const uint8_t *)name, name_length, query + 3U) == LXP_OK);
    query[35U] = 1U; query[36U] = 3U;
    REQUIRE(send_request(descriptor, LNI_MINOR, 7U, 902U, query, sizeof(query)) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    REQUIRE(response.tag == 8U && response.correlation_id == 902U && response.proof_length != 0U);
    REQUIRE(response.payload_length == name_length + 103U);
    REQUIRE(load_u16(response.payload) == name_length && memcmp(response.payload + 2U, name, name_length) == 0);
    cursor = 3U + name_length;
    REQUIRE(load_u64(response.payload + cursor) == 0U);
    account->balance = load_u64(response.payload + cursor + 8U);
    REQUIRE(memcmp(response.payload + cursor + 16U, paid_asset, 32U) == 0);
    REQUIRE(response.payload[cursor + 48U] == 1U);
    account->sequence = load_u64(response.payload + cursor + 49U);
    release_envelope(&response);
    return 0;
}

static int paid_snapshot(int descriptor, paid_state *state)
{
    char owner[128];
    uint8_t query[79];
    wire_envelope response;
    int length = snprintf(owner, sizeof(owner), "agent:%s:main", REGISTERED_DID);
    REQUIRE(length > 0 && (size_t)length < sizeof(owner));
    REQUIRE(paid_account_read(descriptor, owner, &state->accounts[0]) == 0);
    REQUIRE(paid_account_read(descriptor, "system:fees", &state->accounts[1]) == 0);
    REQUIRE(paid_account_read(descriptor, "system:paxeer-withdrawals", &state->accounts[2]) == 0);
    store_u16(query, 1U); store_u16(query + 2U, 75U);
    (void)memcpy(query + 4U, REGISTERED_DID, 75U);
    REQUIRE(send_request(descriptor, LNI_MINOR, 26U, 903U, query, sizeof(query)) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    REQUIRE(response.tag == 27U && response.correlation_id == 903U && response.payload_length >= 139U);
    state->identity_sequence = load_u64(response.payload + 83U);
    (void)memcpy(state->state_root, response.payload + 107U, 32U);
    release_envelope(&response);
    return 0;
}

static int paid_quote(int descriptor)
{
    uint8_t query[30] = {0U, 1U};
    wire_envelope response;
    lxp_fee_params schedule;
    store_u32(query + 2U, LX_ASSET_WITHDRAW);
    REQUIRE(send_request(descriptor, LNI_MINOR, FEE_ESTIMATE_REQUEST, 904U, query, sizeof(query)) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    REQUIRE(response.tag == FEE_ESTIMATE_RESPONSE && response.correlation_id == 904U);
    REQUIRE(response.payload_length == 64U + LXP_FEE_PARAMS_V3_BYTES && response.proof_length == 0U);
    REQUIRE(load_u32(response.payload + 42U) == 1U && load_u64(response.payload + 46U) == 0U);
    REQUIRE(load_u64(response.payload + 54U) == 17U);
    REQUIRE(lxp_fee_params_decode(response.payload + 64U, LXP_FEE_PARAMS_V3_BYTES, &schedule) == LXP_OK);
    REQUIRE(schedule.version == 3U && schedule.asset_price_count == 11U);
    REQUIRE(schedule.asset_prices[10].hi == 0U && schedule.asset_prices[10].lo == 17U);
    release_envelope(&response);
    return 0;
}

static int paid_withdraw(const signer *key, uint64_t sequence, uint64_t limit, paid_record *record)
{
    uint8_t payload[108] = {0};
    (void)memcpy(payload, paid_asset, 32U);
    payload[47] = 1U;
    (void)memset(payload + 48U, 0x31, 20U);
    (void)memset(payload + 68U, 0x42, 32U);
    store_u64(payload + 100U, limit);
    return paid_encode(key, sequence, LX_ASSET_WITHDRAW, limit, payload, sizeof(payload), record);
}

static int paid_grant(const signer *owner, const signer *delegate, uint64_t generation, paid_record *record)
{
    uint8_t storage[4096];
    lxp_authority_grant grant = {0};
    lxp_arena arena;
    lxp_byte_span body;
    lxp_codec_writer writer;
    struct timespec now;
    REQUIRE(clock_gettime(CLOCK_REALTIME, &now) == 0 && now.tv_sec > 0);
    REQUIRE(lxp_did_id_derive(REGISTERED_DID, 75U, grant.grantor) == LXP_OK);
    (void)memcpy(grant.grantee, grant.grantor, 32U);
    (void)memcpy(grant.key, delegate->public_key, 32U);
    grant.kind = LXP_AUTHORITY_DELEGATED_CAPABILITY;
    grant.scope.module_mask = UINT64_C(1) << LXP_MODULE_ASSET;
    grant.scope.activity_ordinal_min = 9U; grant.scope.activity_ordinal_max = 9U;
    (void)memcpy(grant.scope.asset_id, paid_asset, 32U);
    grant.scope.maximum_per_activity = (lxp_u128){0U, 1U};
    grant.scope.maximum_total = (lxp_u128){0U, 1U};
    grant.scope.purpose_hash[0] = 0x67U;
    grant.not_before = (uint64_t)now.tv_sec * 1000U - 60000U;
    grant.not_after = grant.not_before + 3600000U;
    grant.grantor_revocation_sequence = generation;
    grant.fee_budget.present = true;
    (void)memcpy(grant.fee_budget.asset_id, paid_asset, 32U);
    grant.fee_budget.maximum_per_activity = (lxp_u128){0U, 17U};
    grant.fee_budget.maximum_total = (lxp_u128){0U, 17U};
    REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
    REQUIRE(lxp_grant_encode(&grant, &arena, &body) == LXP_OK);
    REQUIRE(lxp_codec_writer_init(&writer, &arena, 2048U) == LXP_OK);
    REQUIRE(lxp_codec_write_u16(&writer, 0x7108U) == LXP_OK);
    REQUIRE(lxp_codec_write_u16(&writer, 0x0101U) == LXP_OK);
    REQUIRE(lxp_codec_write_bytes(&writer, body.bytes, body.length, 1024U) == LXP_OK);
    return paid_encode(owner, 4U, 0x00070008U, 0U, writer.bytes, writer.length, record);
}

static int paid_replay(int *descriptor, paid_state *state)
{
    static const lxp_result results[PAID_RECORDS] = {
        LXP_OK, LXP_ERR_FEE_LIMIT, LXP_OK, LXP_OK, LXP_ERR_NON_CANONICAL, LXP_OK
    };
    static const uint64_t fees[PAID_RECORDS] = {0U, 16U, 17U, 0U, 17U, 0U};
    paid_state after = {0};
    REQUIRE(paid_quote(*descriptor) == 0);
    REQUIRE(paid_snapshot(*descriptor, &after) == 0);
    REQUIRE(memcmp(after.accounts, state->accounts, sizeof(after.accounts)) == 0);
    REQUIRE(after.identity_sequence == state->identity_sequence);
    REQUIRE(memcmp(after.state_root, state->state_root, 32U) == 0);
    for (size_t i = 0U; i < PAID_RECORDS; ++i)
        REQUIRE(paid_submit(*descriptor, &state->records[i], results[i], fees[i], true) == 0);
    REQUIRE(maintenance_head(descriptor, 12U, 6U) == 0);
    REQUIRE(paid_snapshot(*descriptor, &after) == 0);
    REQUIRE(memcmp(after.accounts, state->accounts, sizeof(after.accounts)) == 0);
    REQUIRE(after.identity_sequence == state->identity_sequence);
    REQUIRE(memcmp(after.state_root, state->state_root, 32U) == 0);
    return 0;
}

int main(int argc, char **argv)
{
    static paid_state state;
    paid_state before = {0}, limited = {0}, transferred = {0};
    struct sockaddr_un address = {0};
    signer owner, delegate;
    FILE *file;
    int descriptor;
    REQUIRE(argc == 4 && strlen(argv[1]) < sizeof(address.sun_path));
    REQUIRE(paid_identity(&owner) == 0 && signer_init(&delegate, 0x67U) == 0);
    address.sun_family = AF_UNIX;
    (void)memcpy(address.sun_path, argv[1], strlen(argv[1]) + 1U);
    descriptor = socket(AF_UNIX, SOCK_STREAM, 0);
    REQUIRE(descriptor >= 0 && connect(descriptor, (struct sockaddr *)&address, sizeof(address)) == 0);
    REQUIRE(handshake(descriptor) == 0);
    if (strcmp(argv[2], "--paid-withdrawal-recovered") == 0) {
        file = fopen(argv[3], "rb");
        REQUIRE(file != NULL && fread(&state, sizeof(state), 1U, file) == 1U);
        REQUIRE(fgetc(file) == EOF && !ferror(file) && fclose(file) == 0);
        REQUIRE(paid_replay(&descriptor, &state) == 0);
    } else {
        REQUIRE(strcmp(argv[2], "--paid-withdrawal") == 0);
        const char *credit = getenv("LAYERX_TEST_WITHDRAW_CREDIT");
        REQUIRE(credit != NULL && (file = fopen(credit, "rb")) != NULL);
        state.records[0].activity_length = fread(state.records[0].activity, 1U, ACTIVITY_CAPACITY, file);
        REQUIRE(state.records[0].activity_length > 0U && fgetc(file) == EOF && !ferror(file) && fclose(file) == 0);
        REQUIRE(paid_submit(descriptor, &state.records[0], LXP_OK, 0U, false) == 0);
        REQUIRE(maintenance_head(&descriptor, 2U, 1U) == 0);
        REQUIRE(paid_quote(descriptor) == 0 && paid_snapshot(descriptor, &before) == 0);
        REQUIRE(before.accounts[0].balance == 1000000U && before.accounts[1].balance == 0U && before.accounts[2].balance == 0U);
        REQUIRE(paid_withdraw(&owner, 1U, 16U, &state.records[1]) == 0);
        REQUIRE(paid_submit(descriptor, &state.records[1], LXP_ERR_FEE_LIMIT, 16U, false) == 0);
        REQUIRE(maintenance_head(&descriptor, 4U, 2U) == 0 && paid_snapshot(descriptor, &limited) == 0);
        REQUIRE(limited.accounts[0].balance + 16U == before.accounts[0].balance);
        REQUIRE(limited.accounts[1].balance == 16U && limited.accounts[2].balance == 0U);
        REQUIRE(limited.accounts[0].sequence == before.accounts[0].sequence);
        REQUIRE(limited.identity_sequence == 2U);
        REQUIRE(paid_withdraw(&owner, 2U, 17U, &state.records[2]) == 0);
        REQUIRE(paid_submit(descriptor, &state.records[2], LXP_OK, 17U, false) == 0);
        REQUIRE(maintenance_head(&descriptor, 6U, 3U) == 0 && paid_snapshot(descriptor, &transferred) == 0);
        REQUIRE(transferred.accounts[0].balance + 18U == limited.accounts[0].balance);
        REQUIRE(transferred.accounts[1].balance == 33U && transferred.accounts[2].balance == 1U);
        REQUIRE(transferred.accounts[0].sequence == limited.accounts[0].sequence + 1U && transferred.identity_sequence == 3U);
        {
            uint8_t rotation[68] = {0x71U, 1U, 0U, 2U};
            lxp_receipt receipt;
            REQUIRE(lxp_did_id_derive(REGISTERED_DID, 75U, rotation + 4U) == LXP_OK);
            (void)memcpy(rotation + 36U, owner.public_key, 32U);
            REQUIRE(paid_encode(&owner, 3U, 0x00070001U, 0U,
                rotation, sizeof(rotation), &state.records[5]) == 0);
            REQUIRE(paid_submit(descriptor, &state.records[5], LXP_OK, 0U, false) == 0);
            REQUIRE(maintenance_head(&descriptor, 8U, 4U) == 0);
            REQUIRE(lxp_receipt_decode(state.records[5].receipt,
                state.records[5].receipt_length, true, &receipt) == LXP_OK);
            REQUIRE(receipt.global_sequence == 7U);
            REQUIRE(paid_grant(&owner, &delegate, receipt.global_sequence, &state.records[3]) == 0);
        }
        REQUIRE(paid_submit(descriptor, &state.records[3], LXP_OK, 0U, false) == 0);
        REQUIRE(maintenance_head(&descriptor, 10U, 5U) == 0);
        REQUIRE(paid_withdraw(&delegate, 5U, 17U, &state.records[4]) == 0);
        REQUIRE(paid_submit(descriptor, &state.records[4], LXP_ERR_NON_CANONICAL, 17U, false) == 0);
        REQUIRE(maintenance_head(&descriptor, 12U, 6U) == 0 && paid_snapshot(descriptor, &state) == 0);
        REQUIRE(state.accounts[0].balance + 17U == transferred.accounts[0].balance);
        REQUIRE(state.accounts[1].balance == 50U && state.accounts[2].balance == 1U);
        REQUIRE(state.accounts[0].sequence == transferred.accounts[0].sequence && state.identity_sequence == 6U);
        REQUIRE(paid_replay(&descriptor, &state) == 0);
        file = fopen(argv[3], "wbx");
        REQUIRE(file != NULL && fwrite(&state, sizeof(state), 1U, file) == 1U);
        REQUIRE(fflush(file) == 0 && fsync(fileno(file)) == 0 && fclose(file) == 0);
    }
    REQUIRE(close(descriptor) == 0);
    puts("paid owner withdrawal, fee-limit and delegated refusal, exact fees and duplicate balances verified");
    return 0;
}
