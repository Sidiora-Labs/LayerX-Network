#define main program_admission_main
#include "lxp_test_program_admission.c"
#undef main
#include "layerx/lx_asset.h"
#include "layerx/lxp_ledger.h"

static void print_hex(const uint8_t *bytes, size_t length)
{
    for (size_t i = 0U; i < length; ++i) printf("%02x", bytes[i]);
}

static void actor_did(const signer *key, uint8_t did[76])
{
    static const char digits[] = "0123456789abcdef";
    memcpy(did, "did:layerx:", 11U);
    for (size_t i = 0U; i < 32U; ++i) {
        did[11U + i * 2U] = (uint8_t)digits[key->public_key[i] >> 4U];
        did[12U + i * 2U] = (uint8_t)digits[key->public_key[i] & 15U];
    }
    did[75] = 0U;
}

static int account_id(const uint8_t did[76], const uint8_t asset[32], uint8_t id[32])
{
    static const char digits[] = "0123456789abcdef";
    char hex[65], name[160];
    for (size_t i = 0U; i < 32U; ++i) {
        hex[i * 2U] = digits[asset[i] >> 4U];
        hex[i * 2U + 1U] = digits[asset[i] & 15U];
    }
    hex[64] = 0;
    int length = snprintf(name, sizeof(name), "agent:%s:asset:%s", did, hex);
    REQUIRE(length > 0 && (size_t)length < sizeof(name));
    REQUIRE(lx_account_id_from_string((const uint8_t *)name, (size_t)length, id) ==
            LXP_OK);
    return 0;
}

static int read_state(int descriptor, uint16_t tag, const uint8_t *query, size_t length)
{
    wire_envelope response;
    REQUIRE(send_request(descriptor, 5U, tag, tag, query, length) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    if (response.tag == 25U && response.payload_length == 5U)
        fprintf(stderr, "read tag=%u refusal=%d\n", tag, (int32_t)load_u32(response.payload + 1U));
    REQUIRE(response.tag == tag + 1U && response.correlation_id == tag);
    printf("read tag=%u payload=", response.tag);
    print_hex(response.payload, response.payload_length);
    printf("\n");
    release_envelope(&response);
    return 0;
}

static int submit_pay(int descriptor, const signer *key, uint64_t sequence,
    uint16_t ordinal, const uint8_t *payload, size_t payload_length, bool wait)
{
    uint8_t encoded[ACTIVITY_CAPACITY], id[32], query[34] = {1U};
    static uint8_t storage[2U * LXP_MAX_ACTIVITY_BYTES];
    size_t length;
    signer sequencer;
    lxp_arena arena;
    wire_envelope response;
    REQUIRE(build_activity(key, sequence, ((uint32_t)1U << 16U) | ordinal,
        0U, payload, payload_length, encoded, sizeof(encoded), &length) == 0);
    REQUIRE(lxp_activity_id(encoded, length, id) == LXP_OK);
    REQUIRE(send_request(descriptor, 5U, 3U, sequence + 1U, encoded, length) == 0);
    REQUIRE(expect_ack(descriptor, sequence + 1U, encoded, length, id) == 0);
    memcpy(query + 1U, id, 32U); query[33] = 1U;
    REQUIRE(signer_init(&sequencer, 0x22U) == 0);
    for (unsigned attempt = 0U; attempt < 200U; ++attempt) {
        REQUIRE(send_request(descriptor, 5U, 5U, sequence + 1U, query, wait ? 34U : 33U) == 0);
        REQUIRE(receive_envelope(descriptor, &response) == 0);
        REQUIRE(response.tag == 6U);
        if (response.payload_length != 0U) {
            lxp_receipt receipt;
            REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
            REQUIRE(lxp_receipt_decode(response.payload, response.payload_length, true, &receipt) == LXP_OK);
            REQUIRE(lxp_receipt_verify(&receipt, sequencer.public_key, &arena) == LXP_OK);
            REQUIRE(memcmp(receipt.activity_id, id, 32U) == 0);
            printf("receipt ordinal=%u sequence=%llu result=%d root=", ordinal,
                (unsigned long long)receipt.global_sequence, (int)receipt.result_code);
            print_hex(receipt.resulting_state_root, 32U); printf("\n");
            REQUIRE(receipt.result_code == LXP_OK);
            release_envelope(&response);
            return 0;
        }
        release_envelope(&response);
        REQUIRE(!wait);
        const struct timespec delay = {0, 50000000};
        REQUIRE(nanosleep(&delay, NULL) == 0);
    }
    return 1;
}

int main(int argc, char **argv)
{
    signer alice, bob;
    uint8_t alice_did[76], bob_did[76], issuer[32], salt[32], asset[32], from[32], to[32];
    uint8_t payload[1024] = {0U}, message[512], digest[32];
    size_t length = 0U, message_length;
    struct sockaddr_un address = {0};
    lxp_hash_context hash;
    lxp_payer_grant grant = {0};
    REQUIRE(argc == 6 && strlen(argv[1]) < sizeof(address.sun_path));
    REQUIRE(signer_init(&alice, 0x11U) == 0 && signer_init(&bob, 0x12U) == 0);
    actor_did(&alice, alice_did); actor_did(&bob, bob_did);
    REQUIRE(lxp_did_id_derive(alice_did, 75U, issuer) == LXP_OK);
    FILE *salt_file = fopen(argv[2], "rb");
    REQUIRE(salt_file != NULL && fread(salt, 1U, 32U, salt_file) == 32U);
    REQUIRE(fgetc(salt_file) == EOF && fclose(salt_file) == 0);
    lxp_hash_init(&hash);
    REQUIRE(lxp_hash_update(&hash, "LX:ASSET:v1", 11U) == LXP_OK);
    REQUIRE(lxp_hash_update(&hash, issuer, 32U) == LXP_OK);
    REQUIRE(lxp_hash_update(&hash, salt, 32U) == LXP_OK);
    REQUIRE(lxp_hash_final(&hash, asset) == LXP_OK);
    REQUIRE(account_id(alice_did, asset, from) == 0 && account_id(bob_did, asset, to) == 0);
    memcpy(REGISTERED_DID, alice_did, 76U);
    uint64_t sequence = strtoull(argv[4], NULL, 10);
    bool wait = strcmp(argv[5], "wait") == 0;
    address.sun_family = AF_UNIX;
    memcpy(address.sun_path, argv[1], strlen(argv[1]) + 1U);
    int descriptor = socket(AF_UNIX, SOCK_STREAM, 0);
    REQUIRE(descriptor >= 0 && connect(descriptor, (struct sockaddr *)&address, sizeof(address)) == 0);
    wire_envelope response;
    REQUIRE(send_request(descriptor, 0U, 1U, 0U, NULL, 0U) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0 && response.tag == 2U && response.minor == 5U);
    release_envelope(&response);
    memcpy(grant.from, from, 32U); memcpy(grant.recipient, to, 32U); memcpy(grant.asset, asset, 32U);
    grant.per_draw_maximum.lo = 2U; grant.allowance.lo = 10U; grant.expiration = UINT64_MAX;
    grant.purpose_hash[0] = 1U; memcpy(grant.public_key, alice.public_key, 32U);
    REQUIRE(lxp_grant_authorization_message(&grant, message, sizeof(message), &message_length) == LXP_OK);
    REQUIRE(lxp_hash_authority(message, message_length, grant.grant_id) == LXP_OK);
    REQUIRE(lxp_hash_domain(LXP_DOMAIN_AUTHORITY_HASH, message, message_length, digest) == LXP_OK);
    REQUIRE(sign_raw(&alice, digest, 32U, grant.signature) == 0);
    if (strcmp(argv[3], "read") == 0) {
        uint8_t list[3] = {0U, 1U, 1U}, get[35] = {0U, 1U, 2U};
        uint8_t fee[30] = {0U, 1U}, accounts[37] = {0U, 1U, 3U};
        memcpy(get + 3U, asset, 32U);
        store_u32(fee + 2U, LX_ASSET_SEND); store_u64(fee + 6U, 400U);
        memcpy(accounts + 3U, issuer, 32U); accounts[35] = 1U; accounts[36] = 1U;
        REQUIRE(read_state(descriptor, 32U, list, sizeof(list)) == 0);
        REQUIRE(read_state(descriptor, 32U, get, sizeof(get)) == 0);
        REQUIRE(read_state(descriptor, 34U, fee, sizeof(fee)) == 0);
        REQUIRE(read_state(descriptor, 7U, accounts, sizeof(accounts)) == 0);
        REQUIRE(lxp_did_id_derive(bob_did, 75U, accounts + 3U) == LXP_OK);
        REQUIRE(read_state(descriptor, 7U, accounts, sizeof(accounts)) == 0);
    } else if (strcmp(argv[3], "register") == 0) {
        store_u16(payload, 1U); memcpy(payload + 2U, asset, 32U); memcpy(payload + 34U, salt, 32U);
        length = 66U; payload[length++] = 3U; memcpy(payload + length, "TOK", 3U); length += 3U;
        payload[length++] = 5U; memcpy(payload + length, "Token", 5U); length += 5U;
        payload[length++] = 6U;
        REQUIRE(lxp_u128_to_be((lxp_u128){0U, 10000U}, payload + length) == LXP_OK); length += 16U;
        payload[length++] = 1U; payload[length++] = 0U;
        REQUIRE(submit_pay(descriptor, &alice, sequence, 1U, payload, length, wait) == 0);
    } else if (strcmp(argv[3], "open") == 0 || strcmp(argv[3], "open-bob") == 0) {
        bool second = strcmp(argv[3], "open-bob") == 0;
        if (second) memcpy(REGISTERED_DID, bob_did, 76U);
        store_u16(payload, 1U); memcpy(payload + 2U, asset, 32U);
        REQUIRE(submit_pay(descriptor, second ? &bob : &alice, sequence, 4U, payload, 34U, wait) == 0);
    } else if (strcmp(argv[3], "mint") == 0 || strcmp(argv[3], "burn") == 0) {
        bool mint = strcmp(argv[3], "mint") == 0;
        store_u16(payload, 1U); memcpy(payload + 2U, asset, 32U); memcpy(payload + 34U, from, 32U);
        REQUIRE(lxp_u128_to_be((lxp_u128){0U, mint ? 9000U : 100U}, payload + 66U) == LXP_OK);
        REQUIRE(submit_pay(descriptor, &alice, sequence, mint ? 10U : 11U, payload, 82U, wait) == 0);
    } else if (strcmp(argv[3], "grant-issue") == 0) {
        REQUIRE(lxp_payer_grant_encode(&grant, payload, sizeof(payload), &length) == LXP_OK);
        REQUIRE(submit_pay(descriptor, &alice, sequence, 7U, payload, length, wait) == 0);
    } else if (strcmp(argv[3], "grant-revoke") == 0) {
        store_u16(payload, 1U); memcpy(payload + 2U, grant.grant_id, 32U); store_u64(payload + 34U, sequence);
        REQUIRE(submit_pay(descriptor, &alice, sequence, 8U, payload, 42U, wait) == 0);
    } else {
        REQUIRE(strcmp(argv[3], "sends") == 0);
        for (unsigned i = 0U; i < 20U; ++i) {
            lxp_send send = {0};
            memcpy(send.from, from, 32U); memcpy(send.to, to, 32U); memcpy(send.asset, asset, 32U);
            send.amount.lo = 1U; send.sequence = 1U + i; send.expires_at = UINT64_MAX;
            store_u64(send.idempotency_key, sequence + i); send.idempotency_key[31] = 0xa5U;
            uint8_t material[144];
            send.authorization.kind = LXP_AUTH_OWNER;
            send.authorization.network_id = NETWORK_ID;
            send.authorization.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
            memcpy(send.authorization.controller, from, 32U);
            memcpy(material, from, 32U); memcpy(material + 32U, to, 32U);
            memcpy(material + 64U, asset, 32U);
            REQUIRE(lxp_u128_to_be(send.amount, material + 96U) == LXP_OK);
            memcpy(material + 112U, send.idempotency_key, 32U);
            REQUIRE(lxp_hash_context_value(material, sizeof(material), send.context_hash) == LXP_OK);
            memcpy(send.authorization.signed_context_hash, send.context_hash, 32U);
            memcpy(send.authorization.public_key, alice.public_key, 32U);
            REQUIRE(lxp_send_authorization_message(&send, message, sizeof(message), &message_length) == LXP_OK);
            REQUIRE(lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, message, message_length, digest) == LXP_OK);
            REQUIRE(sign_raw(&alice, digest, 32U, send.authorization.signature) == 0);
            REQUIRE(lxp_send_encode(&send, payload, sizeof(payload), &length) == LXP_OK);
            REQUIRE(submit_pay(descriptor, &alice, sequence + i, 5U, payload, length, wait) == 0);
        }
    }
    REQUIRE(close(descriptor) == 0);
    return 0;
}
