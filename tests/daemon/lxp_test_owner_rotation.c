#define ONBOARDING_CLIENT_MAIN native_onboarding_client_main
int native_onboarding_client_main(int argc, char **argv);
#include "lxp_test_native_onboarding.c"
#undef ONBOARDING_CLIENT_MAIN

typedef struct rotation_run {
    onboarding_run history;
    uint8_t did[512];
    uint16_t did_length;
    uint8_t account[32];
    uint8_t original_key[32];
    uint8_t identity[223];
    uint64_t global_sequence;
} rotation_run;

static int rotation_read(const char *path, uint8_t *bytes, size_t capacity, size_t *length)
{
    FILE *input = fopen(path, "rb");
    REQUIRE(input != NULL);
    *length = fread(bytes, 1U, capacity, input);
    REQUIRE(*length != 0U && *length < capacity && !ferror(input) && fclose(input) == 0);
    return 0;
}

static void rotation_hex(FILE *output, const uint8_t *bytes, size_t length)
{
    for (size_t i = 0U; i < length; ++i) (void)fprintf(output, "%02x", bytes[i]);
}

static int rotation_account(int descriptor, const uint8_t account[32], const uint8_t key[32],
    lxp_u128 *balance, uint64_t *sequence)
{
    uint8_t query[37] = {0U, 1U, 2U};
    wire_envelope response;
    REQUIRE(onboard_account(descriptor, account, onboarding_asset, balance, sequence, true) == 0);
    (void)memcpy(query + 3U, account, 32U); query[35U] = 1U; query[36U] = 3U;
    REQUIRE(send_request(descriptor, LNI_MINOR, 7U, 730U, query, sizeof(query)) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    REQUIRE(response.tag == 8U && response.proof_length != 0U);
    size_t names = load_u16(response.payload);
    REQUIRE(response.payload_length == names + 103U);
    REQUIRE(response.payload[names + 102U] == 1U && memcmp(response.payload + names + 70U, key, 32U) == 0);
    release_envelope(&response);
    return 0;
}

static int rotation_state(const lxp_receipt *receipt, rotation_run *run, bool required)
{
    bool found = false;
    uint8_t did[32];
    REQUIRE(lxp_did_id_derive(run->did, run->did_length, did) == LXP_OK);
    for (size_t i = 0U; i < receipt->effects.count; ++i) {
        const lxp_effect *effect = &receipt->effects.effects[i];
        if (effect->kind != LXP_EFFECT_EVENT || effect->module_id != LXP_MODULE_GOVERNANCE || effect->event_type != 0x7110U ||
            effect->body_length != sizeof(run->identity) || memcmp(effect->body, "LXGI1", 5U) != 0)
            continue;
        REQUIRE(!found && memcmp(effect->body + 5U, did, 32U) == 0);
        (void)memcpy(run->identity, effect->body, sizeof(run->identity));
        found = true;
    }
    REQUIRE(found == required);
    run->global_sequence = receipt->global_sequence;
    return 0;
}

static int rotation_save(int descriptor, const char *prefix, rotation_run *run)
{
    char path[4096];
    uint8_t commitment[32];
    lxp_u128 balance;
    uint64_t account_sequence;
    uint8_t preparation[516];
    wire_envelope head;
    REQUIRE(rotation_account(descriptor, run->account, run->identity + 37U, &balance, &account_sequence) == 0);
    REQUIRE(run->did_length != 0U && run->did_length <= sizeof(run->did));
    store_u16(preparation, 1U); store_u16(preparation + 2U, run->did_length);
    (void)memcpy(preparation + 4U, run->did, run->did_length);
    REQUIRE(send_request(descriptor, LNI_MINOR, 26U, 731U, preparation, 4U + run->did_length) == 0);
    REQUIRE(receive_envelope(descriptor, &head) == 0 && head.tag == 27U &&
        head.correlation_id == 731U && head.payload_length >= 64U + run->did_length &&
        load_u16(head.payload) == 1U && load_u16(head.payload + 2U) == run->did_length &&
        memcmp(head.payload + 4U, run->did, run->did_length) == 0 &&
        load_u32(head.payload + 4U + run->did_length) == NETWORK_ID &&
        load_u64(head.payload + 8U + run->did_length) == run->history.target_sequence);
    uint64_t head_sequence = load_u64(head.payload + 24U + run->did_length);
    REQUIRE(head_sequence > run->global_sequence && memcmp(head.payload + 32U + run->did_length, run->history.root, 32U) == 0);
    release_envelope(&head);
    REQUIRE(lxp_hash_context_value(run->identity, sizeof(run->identity), commitment) == LXP_OK);
    REQUIRE(snprintf(path, sizeof(path), "%s.bin", prefix) > 0);
    FILE *output = fopen(path, "wb");
    REQUIRE(output != NULL && fwrite(run, sizeof(*run), 1U, output) == 1U && fflush(output) == 0 && fsync(fileno(output)) == 0 && fclose(output) == 0);
    REQUIRE(snprintf(path, sizeof(path), "%s.json", prefix) > 0);
    output = fopen(path, "w");
    REQUIRE(output != NULL);
    (void)fprintf(output, "{\"network_id\":77,\"protocol_version\":3,\"did_hex\":\"");
    rotation_hex(output, run->did, run->did_length);
    (void)fprintf(output, "\",\"account\":\""); rotation_hex(output, run->account, 32U);
    (void)fprintf(output, "\",\"original_key\":\""); rotation_hex(output, run->original_key, 32U);
    (void)fprintf(output, "\",\"identity\":\""); rotation_hex(output, run->identity, sizeof(run->identity));
    (void)fprintf(output, "\",\"announcement\":\""); rotation_hex(output, commitment, 32U);
    (void)fprintf(output, "\",\"root\":\""); rotation_hex(output, run->history.root, 32U);
    (void)fprintf(output, "\",\"activity_sequence\":%llu,\"account_sequence\":%llu,\"global_sequence\":%llu,\"head_sequence\":%llu,\"balance_hi\":%llu,\"balance_lo\":%llu,\"receipts\":%zu}\n",
        (unsigned long long)run->history.target_sequence, (unsigned long long)account_sequence,
        (unsigned long long)run->global_sequence, (unsigned long long)head_sequence, (unsigned long long)balance.hi,
        (unsigned long long)balance.lo, run->history.count);
    REQUIRE(fflush(output) == 0 && fsync(fileno(output)) == 0 && fclose(output) == 0);
    return 0;
}

static int rotation_fund(int descriptor, signer *sponsor, rotation_run *run)
{
    uint8_t source[32], material[144], message[512], digest[32], payload[1024];
    size_t length;
    lxp_u128 balance;
    uint64_t sequence;
    lxp_receipt receipt;
    lxp_send send = {0};
    REQUIRE(onboard_main_id(sponsor, source) == 0);
    REQUIRE(onboard_account(descriptor, source, onboarding_asset, &balance, &sequence, true) == 0);
    (void)memcpy(send.from, source, 32U); (void)memcpy(send.to, run->account, 32U);
    (void)memcpy(send.asset, onboarding_asset, 32U); send.amount.lo = 200000000U;
    send.sequence = sequence; send.idempotency_key[0] = 0x90U;
    store_u64(send.idempotency_key + 24U, run->history.count + 1U);
    struct timespec now;
    REQUIRE(clock_gettime(CLOCK_REALTIME, &now) == 0);
    send.expires_at = (uint64_t)now.tv_sec * 1000U + 300000U;
    (void)memcpy(material, source, 32U); (void)memcpy(material + 32U, run->account, 32U);
    (void)memcpy(material + 64U, onboarding_asset, 32U);
    REQUIRE(lxp_u128_to_be(send.amount, material + 96U) == LXP_OK);
    (void)memcpy(material + 112U, send.idempotency_key, 32U);
    REQUIRE(lxp_hash_context_value(material, sizeof(material), send.context_hash) == LXP_OK);
    send.authorization.kind = LXP_AUTH_OWNER; send.authorization.network_id = NETWORK_ID;
    send.authorization.protocol_version = 3U;
    (void)memcpy(send.authorization.controller, source, 32U);
    (void)memcpy(send.authorization.public_key, sponsor->public_key, 32U);
    (void)memcpy(send.authorization.signed_context_hash, send.context_hash, 32U);
    REQUIRE(lxp_send_authorization_message(&send, message, sizeof(message), &length) == LXP_OK);
    REQUIRE(lxp_hash_signature_preimage(message, length, digest) == LXP_OK);
    REQUIRE(sign_raw(sponsor, digest, 32U, send.authorization.signature) == 0);
    REQUIRE(lxp_send_encode(&send, payload, sizeof(payload), &length) == LXP_OK);
    REQUIRE(onboard_execute(descriptor, &run->history, sponsor, &run->history.owner_sequence,
        0x00010005U, payload, length, LXP_OK, &receipt) == 0);
    REQUIRE(rotation_account(descriptor, run->account, run->original_key, &balance, &sequence) == 0);
    REQUIRE(balance.hi == 0U && balance.lo == 200000000U && sequence == 0U);
    run->global_sequence = receipt.global_sequence;
    return 0;
}

static int rotation_prepare(int descriptor, signer *sponsor, const char *input, rotation_run *run)
{
    uint8_t bytes[ACTIVITY_CAPACITY], payload[1024], encoded[ACTIVITY_CAPACITY], sponsor_did[32];
    size_t length, encoded_length;
    lxp_receipt receipt;
    lxp_activity consent;
    REQUIRE(rotation_read(input, bytes, sizeof(bytes), &length) == 0);
    REQUIRE(lxp_activity_decode(bytes, length, &consent) == LXP_OK && consent.protocol_version == 3U &&
        consent.network_id == NETWORK_ID && consent.activity_type == 0x00070001U &&
        consent.payload.length == 140U && memcmp(consent.payload.bytes, "\x71\x01\x03\x05", 4U) == 0 &&
        consent.authority.length == 32U && consent.actor_did.length <= sizeof(run->did));
    REQUIRE(lxp_activity_verify_payload_hash(&consent) == LXP_OK && lxp_activity_verify_signature(&consent) == LXP_OK);
    run->did_length = (uint16_t)consent.actor_did.length;
    (void)memcpy(run->did, consent.actor_did.bytes, run->did_length);
    (void)memcpy(run->original_key, consent.authority.bytes, 32U);
    (void)memcpy(run->account, consent.payload.bytes + 68U, 32U);
    REQUIRE(lxp_did_id_derive(REGISTERED_DID, 75U, sponsor_did) == LXP_OK);
    REQUIRE(memcmp(consent.payload.bytes + 4U, sponsor_did, 32U) == 0);
    const char *credit = getenv("LAYERX_TEST_WITHDRAW_CREDIT");
    REQUIRE(credit != NULL && rotation_read(credit, encoded, sizeof(encoded), &encoded_length) == 0);
    REQUIRE(onboard_submit(descriptor, &run->history, encoded, encoded_length, LXP_OK, &receipt) == 0);
    REQUIRE(lxp_u128_is_zero(receipt.fee_charged)); ++run->history.owner_sequence;
    (void)memset(payload, 0, sizeof(payload));
    payload[0] = 0x71U; payload[1] = 1U; payload[3] = 2U;
    (void)memcpy(payload + 4U, sponsor_did, 32U); (void)memcpy(payload + 36U, sponsor->public_key, 32U);
    REQUIRE(onboard_execute(descriptor, &run->history, sponsor, &run->history.owner_sequence,
        0x00070001U, payload, 68U, LXP_OK, &receipt) == 0);
    payload[0] = 0x71U; payload[1] = 1U; payload[2] = 2U; payload[3] = 1U;
    REQUIRE(length <= sizeof(payload) - 8U);
    store_u32(payload + 4U, (uint32_t)length); (void)memcpy(payload + 8U, bytes, length);
    REQUIRE(onboard_encode(sponsor, run->history.owner_sequence, 0x00070001U,
        consent.timestamp_bound.not_before, consent.idempotency_key, (lxp_u128){0U, 67108864U},
        payload, length + 8U, encoded, &encoded_length) == 0);
    REQUIRE(onboard_submit(descriptor, &run->history, encoded, encoded_length, LXP_OK, &receipt) == 0);
    REQUIRE(receipt.fee_charged.hi == 0U && receipt.fee_charged.lo == 4U);
    ++run->history.owner_sequence;
    REQUIRE(rotation_state(&receipt, run, true) == 0);
    lxp_u128 balance; uint64_t sequence;
    REQUIRE(rotation_account(descriptor, run->account, run->original_key, &balance, &sequence) == 0);
    REQUIRE(lxp_u128_is_zero(balance) && sequence == 0U);
    return rotation_fund(descriptor, sponsor, run);
}

static int rotation_apply(int descriptor, rotation_run *run, const char *input, lxp_result expected)
{
    uint8_t bytes[ACTIVITY_CAPACITY], sponsor_account[32], treasury_account[32];
    size_t length;
    lxp_activity original;
    lxp_receipt receipt;
    signer sponsor;
    lxp_u128 before, after, wanted, sponsor_before, sponsor_after, treasury_before, treasury_after;
    uint64_t sequence_before, sequence_after, sponsor_sequence, sponsor_after_sequence, treasury_sequence, treasury_after_sequence;
    REQUIRE(onboard_owner(&sponsor) == 0 && onboard_main_id(&sponsor, sponsor_account) == 0);
    REQUIRE(rotation_account(descriptor, sponsor_account, sponsor.public_key, &sponsor_before, &sponsor_sequence) == 0);
    REQUIRE(rotation_account(descriptor, run->account, run->identity + 37U, &before, &sequence_before) == 0);
    REQUIRE(lx_account_id_from_string((const uint8_t *)"system:fees", 11U, treasury_account) == LXP_OK);
    REQUIRE(onboard_account(descriptor, treasury_account, onboarding_asset, &treasury_before, &treasury_sequence, true) == 0);
    REQUIRE(rotation_read(input, bytes, sizeof(bytes), &length) == 0);
    REQUIRE(lxp_activity_decode(bytes, length, &original) == LXP_OK &&
        original.actor_did.length == run->did_length && memcmp(original.actor_did.bytes, run->did, run->did_length) == 0 &&
        original.account_sequence == run->history.target_sequence &&
        (original.activity_type == 0x00070002U || original.activity_type == 0x00070003U));
    REQUIRE(onboard_submit(descriptor, &run->history, bytes, length, expected, &receipt) == 0);
    REQUIRE(receipt.fee_charged.hi == 0U && receipt.fee_charged.lo == 4U &&
        lxp_u128_is_zero(receipt.amount));
    ++run->history.target_sequence;
    REQUIRE(rotation_state(&receipt, run, expected == LXP_OK) == 0);
    REQUIRE(rotation_account(descriptor, run->account, run->identity + 37U, &after, &sequence_after) == 0);
    REQUIRE(lxp_u128_sub(before, receipt.fee_charged, &wanted) == LXP_OK && lxp_u128_cmp(after, wanted) == 0);
    REQUIRE(sequence_after == sequence_before);
    REQUIRE(onboard_account(descriptor, treasury_account, onboarding_asset, &treasury_after, &treasury_after_sequence, true) == 0);
    REQUIRE(lxp_u128_add(treasury_before, receipt.fee_charged, &wanted) == LXP_OK &&
        lxp_u128_cmp(treasury_after, wanted) == 0 && treasury_sequence < UINT64_MAX &&
        treasury_after_sequence == treasury_sequence + 1U);
    REQUIRE(rotation_account(descriptor, sponsor_account, sponsor.public_key, &sponsor_after, &sponsor_after_sequence) == 0);
    REQUIRE(lxp_u128_cmp(sponsor_before, sponsor_after) == 0 && sponsor_sequence == sponsor_after_sequence);
    return 0;
}

int main(int argc, char **argv)
{
    struct sockaddr_un address = {0};
    rotation_run run = {0};
    signer sponsor;
    REQUIRE(argc >= 4 && argc <= 7 && strlen(argv[1]) < sizeof(address.sun_path) && strlen(argv[2]) < 4000U);
    onboarding_path = argv[2];
    REQUIRE(onboard_owner(&sponsor) == 0);
    address.sun_family = AF_UNIX; (void)memcpy(address.sun_path, argv[1], strlen(argv[1]) + 1U);
    int descriptor = socket(AF_UNIX, SOCK_STREAM, 0);
    REQUIRE(descriptor >= 0 && connect(descriptor, (struct sockaddr *)&address, sizeof(address)) == 0 && handshake(descriptor) == 0);
    if (strcmp(argv[3], "--prepare") == 0) {
        REQUIRE(argc == 5 && rotation_prepare(descriptor, &sponsor, argv[4], &run) == 0);
    } else {
        char path[4096];
        REQUIRE(snprintf(path, sizeof(path), "%s.bin", argv[2]) > 0);
        FILE *input = fopen(path, "rb");
        REQUIRE(input != NULL && fread(&run, sizeof(run), 1U, input) == 1U && fgetc(input) == EOF && !ferror(input) && fclose(input) == 0);
        REQUIRE(run.did_length != 0U && run.did_length <= sizeof(run.did) && run.history.count < ONBOARD_RECEIPTS);
        if (strcmp(argv[3], "--apply") == 0) {
            REQUIRE(argc == 6 && rotation_apply(descriptor, &run, argv[4], (lxp_result)strtol(argv[5], NULL, 10)) == 0);
        } else if (strcmp(argv[3], "--recover") == 0) {
            uint8_t root[32]; lxp_receipt receipt;
            REQUIRE(argc == 4 && onboard_head(descriptor, 0U, root) == 0 && memcmp(root, run.history.root, 32U) == 0);
            for (size_t i = 0U; i < run.history.count; ++i)
                REQUIRE(onboard_receipt(descriptor, run.history.ids[i], run.history.results[i], i, false, &receipt) == 0);
        } else {
            uint8_t bytes[ACTIVITY_CAPACITY], root[32], id[32]; size_t length;
            REQUIRE(argc >= 5 && rotation_read(argv[4], bytes, sizeof(bytes), &length) == 0);
            REQUIRE(send_request(descriptor, LNI_MINOR, SUBMIT_REQUEST, 731U, bytes, length) == 0);
            if (strcmp(argv[3], "--refuse") == 0) {
                REQUIRE(argc == 7 && expect_error(descriptor, 731U, (uint8_t)strtoul(argv[5], NULL, 10), (lxp_result)strtol(argv[6], NULL, 10)) == 0);
            } else {
                REQUIRE(argc == 5 && strcmp(argv[3], "--replay") == 0);
                REQUIRE(lxp_activity_id(bytes, length, id) == LXP_OK);
                bool found = false;
                for (size_t i = 0U; i < run.history.count; ++i) if (memcmp(id, run.history.ids[i], 32U) == 0) found = true;
                REQUIRE(found && expect_ack(descriptor, 731U, bytes, length, id) == 0);
            }
            REQUIRE(onboard_head(descriptor, 0U, root) == 0 && memcmp(root, run.history.root, 32U) == 0);
        }
    }
    REQUIRE(rotation_save(descriptor, argv[2], &run) == 0 && close(descriptor) == 0);
    return 0;
}
