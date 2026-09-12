#define main program_admission_client_main
int program_admission_client_main(int argc, char **argv);
#include "lxp_test_program_admission.c"
#undef main
#include "layerx/lxp_authority.h"

typedef struct grant_run {
    uint64_t account_sequence;
    uint64_t next_global_sequence;
    uint64_t generation;
    lxp_authority_grant capability;
    lxp_authority_grant budget;
} grant_run;

static int setup_identity(signer *owner)
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

static int new_grant(lxp_authority_grant *grant, uint8_t seed,
                       lxp_authority_kind kind, uint64_t generation, uint64_t now)
{
    signer delegate;
    (void)memset(grant, 0, sizeof(*grant));
    REQUIRE(signer_init(&delegate, seed) == 0);
    REQUIRE(lxp_did_id_derive(REGISTERED_DID, sizeof(REGISTERED_DID) - 1U,
                             grant->grantor) == LXP_OK);
    (void)memcpy(grant->grantee, grant->grantor, 32U);
    (void)memcpy(grant->key, delegate.public_key, 32U);
    grant->kind = kind;
    grant->scope.module_mask = UINT64_C(1) << LXP_MODULE_GOVERNANCE;
    grant->scope.activity_ordinal_min = 1U;
    grant->scope.activity_ordinal_max = 8U;
    grant->scope.asset_id[0] = 6U;
    grant->scope.purpose_hash[0] = 5U;
    grant->scope.maximum_per_activity = (lxp_u128){0U, 10U};
    grant->scope.maximum_total = (lxp_u128){0U, 30U};
    grant->not_before = now;
    grant->not_after = now + 3600000U;
    grant->grantor_revocation_sequence = generation;
    if (kind == LXP_AUTHORITY_BUDGET_ALLOWANCE) {
        grant->scope.period_length = 60000U;
        grant->scope.maximum_per_period = (lxp_u128){0U, 20U};
        grant->scope.period_start = now;
    }
    return 0;
}

static int grant_payload(const lxp_authority_grant *grant, uint8_t payload[1024],
                           size_t *length)
{
    uint8_t storage[4096];
    lxp_arena arena;
    lxp_byte_span body;
    lxp_codec_writer writer;
    REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
    REQUIRE(lxp_grant_encode(grant, &arena, &body) == LXP_OK);
    REQUIRE(lxp_codec_writer_init(&writer, &arena, 1024U) == LXP_OK);
    REQUIRE(lxp_codec_write_u16(&writer, 0x7108U) == LXP_OK);
    REQUIRE(lxp_codec_write_u16(&writer, 0x0101U) == LXP_OK);
    REQUIRE(lxp_codec_write_bytes(&writer, body.bytes, body.length, 1024U) == LXP_OK);
    REQUIRE(writer.length <= 1024U);
    (void)memcpy(payload, writer.bytes, writer.length);
    *length = writer.length;
    return 0;
}

static int grant_maintenance_head(int descriptor, uint64_t sequence, uint64_t batch)
{
    struct sockaddr_un address;
    socklen_t address_length = sizeof(address);
    REQUIRE(getpeername(descriptor, (struct sockaddr *)&address, &address_length) == 0);
    for (unsigned int attempt = 0U; attempt < 200U; ++attempt) {
        wire_envelope response;
        int current = socket(AF_UNIX, SOCK_STREAM, 0);
        REQUIRE(current >= 0 && connect(current, (struct sockaddr *)&address, address_length) == 0);
        REQUIRE(send_request(current, LNI_MINOR, NODE_INFO_REQUEST, 0U, NULL, 0U) == 0);
        REQUIRE(receive_envelope(current, &response) == 0);
        REQUIRE(response.tag == NODE_INFO_RESPONSE && response.payload_length >= 93U);
        bool reached = load_u64(response.payload + 11U) == sequence &&
                       load_u64(response.payload + 19U) == batch;
        release_envelope(&response);
        REQUIRE(close(current) == 0);
        if (reached) return 0;
        const struct timespec pause = {0, 50000000L};
        REQUIRE(nanosleep(&pause, NULL) == 0);
    }
    REQUIRE(false);
}

static int execute_grant_activity(int descriptor, const signer *key, grant_run *run,
                                   uint16_t ordinal, const uint8_t *payload,
                                   size_t payload_length, lxp_result expected,
                                   lxp_receipt *receipt)
{
    uint8_t encoded[ACTIVITY_CAPACITY], activity_id[32], query[33] = {1U};
    size_t length;
    REQUIRE(build_activity(key, run->account_sequence, 0x00070000U | ordinal, 0U,
                            payload, payload_length, encoded, sizeof(encoded), &length) == 0);
    REQUIRE(lxp_activity_id(encoded, length, activity_id) == LXP_OK);
    REQUIRE(send_request(descriptor, LNI_MINOR, SUBMIT_REQUEST, 400U, encoded, length) == 0);
    REQUIRE(expect_ack(descriptor, 400U, encoded, length, activity_id) == 0);
    (void)memcpy(query + 1U, activity_id, 32U);
    for (unsigned int attempt = 0U; attempt < 200U; ++attempt) {
        wire_envelope response;
        REQUIRE(send_request(descriptor, LNI_MINOR, 5U, 401U, query, sizeof(query)) == 0);
        REQUIRE(receive_envelope(descriptor, &response) == 0);
        REQUIRE(response.tag == 6U && response.correlation_id == 401U);
        if (response.payload_length != 0U) {
            uint8_t storage[2U * LXP_MAX_ACTIVITY_BYTES];
            lxp_arena arena;
            signer sequencer;
            REQUIRE(signer_init(&sequencer, 0x22U) == 0);
            REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
            REQUIRE(lxp_receipt_decode(response.payload, response.payload_length, true, receipt) == LXP_OK);
            REQUIRE(lxp_receipt_verify(receipt, sequencer.public_key, &arena) == LXP_OK);
            REQUIRE(memcmp(receipt->activity_id, activity_id, 32U) == 0);
            REQUIRE(receipt->global_sequence == run->next_global_sequence);
            REQUIRE(receipt->module_id == LXP_MODULE_GOVERNANCE && receipt->module_version == 1U);
            if (receipt->result_code != expected)
                (void)fprintf(stderr, "grant receipt: %d expected %d\n", receipt->result_code, expected);
            REQUIRE(receipt->result_code == expected);
            if (expected != LXP_OK) REQUIRE(receipt->effects.count == 0U);
            ++run->account_sequence;
            REQUIRE(grant_maintenance_head(descriptor, receipt->global_sequence + 1U,
                                             run->account_sequence) == 0);
            run->next_global_sequence += 2U;
            release_envelope(&response);
            return 0;
        }
        release_envelope(&response);
        const struct timespec pause = {0, 50000000L};
        REQUIRE(nanosleep(&pause, NULL) == 0);
    }
    REQUIRE(false);
}

static int issue(int descriptor, const signer *owner, grant_run *run,
                  lxp_authority_grant *grant, lxp_result expected)
{
    uint8_t payload[1024], storage[1024], grant_id[32];
    lxp_arena arena;
    lxp_byte_span encoded;
    lxp_receipt receipt;
    size_t length;
    REQUIRE(grant_payload(grant, payload, &length) == 0);
    REQUIRE(execute_grant_activity(descriptor, owner, run, 8U, payload, length,
                                    expected, &receipt) == 0);
    if (expected != LXP_OK) return 0;
    REQUIRE(lxp_grant_id_compute(grant, grant_id) == LXP_OK);
    (void)memcpy(grant->grant_id, grant_id, 32U);
    REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
    REQUIRE(lxp_grant_encode(grant, &arena, &encoded) == LXP_OK);
    REQUIRE(receipt.effects.count == 4U);
    REQUIRE(receipt.effects.effects[0].event_type == 0x7148U);
    REQUIRE(receipt.effects.effects[0].body_length == 32U);
    REQUIRE(memcmp(receipt.effects.effects[0].body, grant_id, 32U) == 0);
    REQUIRE(receipt.effects.effects[1].event_type == 0x7108U);
    REQUIRE(receipt.effects.effects[1].body_length == 256U);
    REQUIRE(memcmp(receipt.effects.effects[1].body, encoded.bytes, 256U) == 0);
    REQUIRE(receipt.effects.effects[2].event_type == 0x7128U);
    REQUIRE(receipt.effects.effects[2].body_length == encoded.length - 256U);
    REQUIRE(memcmp(receipt.effects.effects[2].body, encoded.bytes + 256U, encoded.length - 256U) == 0);
    REQUIRE(receipt.effects.effects[3].event_type == 0x7110U);
    REQUIRE(load_u64(receipt.effects.effects[3].body + 69U) == run->generation);
    return 0;
}

static int unbounded_grants(int descriptor, const signer *owner, grant_run *run)
{
    uint8_t payload[1024], malformed[1024];
    size_t length, offsets[6], widths[6] = {32U, 16U, 16U, 32U, 8U, 8U};
    lxp_codec_reader outer, reader;
    lxp_byte_span body, field;
    uint64_t wide;
    uint16_t narrow;
    uint8_t tag;
    lxp_u128 amount;
    lxp_receipt receipt;
    REQUIRE(grant_payload(&run->capability, payload, &length) == 0);
    REQUIRE(lxp_codec_reader_init(&outer, payload + 4U, length - 4U) == LXP_OK);
    REQUIRE(lxp_codec_read_bytes(&outer, &body, 1024U) == LXP_OK);
    REQUIRE(lxp_codec_reader_init(&reader, body.bytes, body.length) == LXP_OK);
    REQUIRE(lxp_codec_read_struct_header(&reader, 0x2001U) == LXP_OK);
    REQUIRE(lxp_codec_read_u8(&reader, &tag) == LXP_OK);
    REQUIRE(lxp_codec_read_bytes(&reader, &field, 32U) == LXP_OK);
    REQUIRE(lxp_codec_read_bytes(&reader, &field, 32U) == LXP_OK);
    REQUIRE(lxp_codec_read_u8(&reader, &tag) == LXP_OK);
    REQUIRE(lxp_codec_read_bytes(&reader, &field, 32U) == LXP_OK);
    REQUIRE(lxp_codec_read_u64(&reader, &wide) == LXP_OK);
    REQUIRE(lxp_codec_read_u16(&reader, &narrow) == LXP_OK);
    REQUIRE(lxp_codec_read_u16(&reader, &narrow) == LXP_OK);
    REQUIRE(lxp_codec_read_bytes(&reader, &field, 32U) == LXP_OK);
    offsets[0] = (size_t)(field.bytes - payload);
    offsets[1] = (size_t)(body.bytes - payload) + reader.offset;
    REQUIRE(lxp_codec_read_u128(&reader, &amount) == LXP_OK);
    offsets[2] = (size_t)(body.bytes - payload) + reader.offset;
    REQUIRE(lxp_codec_read_u128(&reader, &amount) == LXP_OK);
    REQUIRE(lxp_codec_read_u128(&reader, &amount) == LXP_OK);
    REQUIRE(lxp_codec_read_u64(&reader, &wide) == LXP_OK);
    REQUIRE(lxp_codec_read_u128(&reader, &amount) == LXP_OK);
    REQUIRE(lxp_codec_read_u128(&reader, &amount) == LXP_OK);
    REQUIRE(lxp_codec_read_u64(&reader, &wide) == LXP_OK);
    REQUIRE(lxp_codec_read_bytes(&reader, &field, 32U) == LXP_OK);
    offsets[3] = (size_t)(field.bytes - payload);
    REQUIRE(lxp_codec_read_u64(&reader, &wide) == LXP_OK);
    offsets[4] = (size_t)(body.bytes - payload) + reader.offset;
    REQUIRE(lxp_codec_read_u64(&reader, &wide) == LXP_OK);
    offsets[5] = (size_t)(body.bytes - payload) + reader.offset;
    for (size_t i = 0U; i < 6U; ++i) {
        REQUIRE(offsets[i] <= length && widths[i] <= length - offsets[i]);
        (void)memcpy(malformed, payload, length);
        (void)memset(malformed + offsets[i], 0, widths[i]);
        REQUIRE(execute_grant_activity(descriptor, owner, run, 8U, malformed, length,
                                        LXP_ERR_MALFORMED_GRANT, &receipt) == 0);
    }
    return 0;
}

static int invalid_grants(int descriptor, const signer *owner, grant_run *run)
{
    for (unsigned int mutation = 0U; mutation < 19U; ++mutation) {
        lxp_authority_grant invalid = run->capability;
        switch (mutation) {
        case 0U: invalid.grantor[0] ^= 1U; break;
        case 1U: invalid.grantee[0] ^= 1U; break;
        case 2U: (void)memcpy(invalid.key, owner->public_key, 32U); break;
        case 3U: (void)memset(invalid.key, 0xff, 32U); break;
        case 4U: ++invalid.grantor_revocation_sequence; break;
        case 5U: invalid.scope.spent_total.lo = 1U; break;
        case 6U: invalid.scope.spent_this_period.lo = 1U; break;
        case 7U: invalid.revoked = true; break;
        case 8U: invalid.revoked_at_sequence = 1U; break;
        case 9U: invalid.grantor_signature[0] = 1U; break;
        case 10U: invalid.scope.module_mask = UINT64_C(1) << 63U; break;
        case 11U: invalid.scope.activity_ordinal_max = UINT16_MAX; break;
        case 12U: invalid.scope.activity_ordinal_min = 0U; break;
        case 13U: invalid.scope.maximum_per_activity.lo = 31U; break;
        case 14U: invalid.not_after = UINT64_MAX; break;
        case 15U: invalid.not_before = 1U; invalid.not_after = 2U; break;
        case 16U: invalid.scope.period_start = 1U; break;
        case 17U: invalid.scope.maximum_per_period.lo = 20U; break;
        default: invalid.kind = LXP_AUTHORITY_SESSION_KEY; break;
        }
        REQUIRE(issue(descriptor, owner, run, &invalid, LXP_ERR_AUTH_SCOPE) == 0);
    }
    uint8_t payload[1024];
    size_t length;
    lxp_receipt receipt;
    REQUIRE(grant_payload(&run->capability, payload, &length) == 0);
    payload[2] = 0U;
    REQUIRE(execute_grant_activity(descriptor, owner, run, 8U, payload, length,
                                    LXP_ERR_NON_CANONICAL, &receipt) == 0);
    payload[2] = 1U; payload[3] = 2U;
    REQUIRE(execute_grant_activity(descriptor, owner, run, 8U, payload, length,
                                    LXP_ERR_NON_CANONICAL, &receipt) == 0);
    payload[3] = 1U; payload[length] = 0U;
    REQUIRE(execute_grant_activity(descriptor, owner, run, 8U, payload, length + 1U,
                                    LXP_ERR_TRAILING_BYTES, &receipt) == 0);
    REQUIRE(execute_grant_activity(descriptor, owner, run, 8U, payload, length - 1U,
                                    LXP_ERR_TRUNCATED, &receipt) == 0);
    return 0;
}

static int delegate_cannot_issue(int descriptor, grant_run *run,
                                  const lxp_authority_grant *grant, uint8_t seed)
{
    signer delegate;
    uint8_t payload[1024];
    size_t length;
    lxp_receipt receipt;
    REQUIRE(signer_init(&delegate, seed) == 0);
    REQUIRE(grant_payload(grant, payload, &length) == 0);
    REQUIRE(execute_grant_activity(descriptor, &delegate, run, 8U, payload, length,
                                    LXP_ERR_AUTH_SCOPE, &receipt) == 0);
    return 0;
}

static int grant_revoke(int descriptor, const signer *owner, grant_run *run,
                   const lxp_authority_grant *grant, lxp_result expected)
{
    uint8_t payload[45] = {0x71U, 6U, 0U, 3U};
    lxp_receipt receipt;
    (void)memcpy(payload + 4U, grant->grant_id, 32U);
    payload[36] = 1U;
    store_u64(payload + 37U, run->next_global_sequence);
    REQUIRE(execute_grant_activity(descriptor, owner, run, 6U, payload, sizeof(payload),
                                    expected, &receipt) == 0);
    if (expected == LXP_OK) {
        run->generation = receipt.global_sequence;
        REQUIRE(receipt.effects.count == 2U);
        REQUIRE(receipt.effects.effects[0].event_type == 0x7106U);
        REQUIRE(memcmp(receipt.effects.effects[0].body, grant->grant_id, 32U) == 0);
        REQUIRE(load_u64(receipt.effects.effects[1].body + 69U) == run->generation);
    }
    return 0;
}

static int delegate_admission_refused(int descriptor, const grant_run *run,
                                     const lxp_authority_grant *grant, uint8_t seed, lxp_result expected)
{
    signer delegate;
    uint8_t payload[1024], encoded[ACTIVITY_CAPACITY];
    size_t payload_length, length;
    wire_envelope response;
    REQUIRE(signer_init(&delegate, seed) == 0);
    REQUIRE(grant_payload(grant, payload, &payload_length) == 0);
    REQUIRE(build_activity(&delegate, run->account_sequence, 0x00070008U, 0U,
                            payload, payload_length, encoded, sizeof(encoded), &length) == 0);
    REQUIRE(send_request(descriptor, LNI_MINOR, SUBMIT_REQUEST, 402U, encoded, length) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    REQUIRE(response.tag == ERROR_RESPONSE && response.correlation_id == 402U);
    REQUIRE(response.payload_length == 5U && response.proof_length == 0U);
    REQUIRE((lxp_result)load_u32(response.payload + 1U) == expected);
    release_envelope(&response);
    return 0;
}

static int initial_run(int descriptor, const signer *owner, grant_run *run)
{
    uint8_t registration[68] = {0x71U, 1U, 0U, 2U};
    lxp_receipt receipt;
    struct timespec now;
    REQUIRE(clock_gettime(CLOCK_REALTIME, &now) == 0);
    run->next_global_sequence = 1U;
    REQUIRE(new_grant(&run->capability, 0x33U, LXP_AUTHORITY_DELEGATED_CAPABILITY,
                       1U, (uint64_t)now.tv_sec * 1000U) == 0);
    REQUIRE(issue(descriptor, owner, run, &run->capability, LXP_ERR_UNKNOWN_FIELD) == 0);
    (void)memcpy(registration + 4U, run->capability.grantor, 32U);
    (void)memcpy(registration + 36U, owner->public_key, 32U);
    REQUIRE(execute_grant_activity(descriptor, owner, run, 1U, registration, sizeof(registration),
                                    LXP_OK, &receipt) == 0);
    run->generation = receipt.global_sequence;
    REQUIRE(run->generation > 0U);
    REQUIRE(receipt.effects.count == 1U && receipt.effects.effects[0].event_type == 0x7110U);
    REQUIRE(load_u64(receipt.effects.effects[0].body + 69U) == run->generation);
    run->capability.grantor_revocation_sequence = run->generation;
    REQUIRE(delegate_admission_refused(descriptor, run, &run->capability, 0x66U, LXP_ERR_BAD_SIGNATURE) == 0);
    REQUIRE(unbounded_grants(descriptor, owner, run) == 0);
    REQUIRE(invalid_grants(descriptor, owner, run) == 0);
    REQUIRE(issue(descriptor, owner, run, &run->capability, LXP_OK) == 0);
    REQUIRE(issue(descriptor, owner, run, &run->capability, LXP_ERR_SEQUENCE_REUSED) == 0);
    lxp_authority_grant duplicate = run->capability;
    duplicate.scope.purpose_hash[0] ^= 1U;
    REQUIRE(issue(descriptor, owner, run, &duplicate, LXP_ERR_SEQUENCE_REUSED) == 0);
    REQUIRE(delegate_cannot_issue(descriptor, run, &run->capability, 0x33U) == 0);
    REQUIRE(grant_revoke(descriptor, owner, run, &run->capability, LXP_OK) == 0);
    REQUIRE(delegate_admission_refused(descriptor, run, &run->capability, 0x33U, LXP_ERR_AUTH_REVOKED) == 0);
    REQUIRE(grant_revoke(descriptor, owner, run, &run->capability, LXP_ERR_AUTH_REVOKED) == 0);
    duplicate.grantor_revocation_sequence = run->generation;
    REQUIRE(issue(descriptor, owner, run, &duplicate, LXP_ERR_SEQUENCE_REUSED) == 0);
    REQUIRE(new_grant(&run->budget, 0x44U, LXP_AUTHORITY_BUDGET_ALLOWANCE,
                       run->generation, run->capability.not_before) == 0);
    REQUIRE(issue(descriptor, owner, run, &run->budget, LXP_OK) == 0);
    REQUIRE(delegate_cannot_issue(descriptor, run, &run->budget, 0x44U) == 0);
    return 0;
}

int main(int argc, char **argv)
{
    signer owner;
    grant_run run = {0};
    REQUIRE(setup_identity(&owner) == 0);
    if (argc == 2 && (strcmp(argv[1], "--encode-capability") == 0 ||
                      strcmp(argv[1], "--encode-budget") == 0)) {
        uint8_t storage[1024];
        lxp_arena arena;
        lxp_byte_span encoded;
        REQUIRE(new_grant(&run.capability, 0x33U,
            strcmp(argv[1], "--encode-budget") == 0 ? LXP_AUTHORITY_BUDGET_ALLOWANCE :
                LXP_AUTHORITY_DELEGATED_CAPABILITY, 7U, 1000U) == 0);
        REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
        REQUIRE(lxp_grant_encode(&run.capability, &arena, &encoded) == LXP_OK);
        REQUIRE(fwrite(encoded.bytes, 1U, encoded.length, stdout) == encoded.length);
        return 0;
    }
    struct sockaddr_un address = {0};
    REQUIRE(argc == 4 && strlen(argv[1]) < sizeof(address.sun_path));
    address.sun_family = AF_UNIX;
    (void)memcpy(address.sun_path, argv[1], strlen(argv[1]) + 1U);
    int descriptor = socket(AF_UNIX, SOCK_STREAM, 0);
    REQUIRE(descriptor >= 0 && connect(descriptor, (struct sockaddr *)&address, sizeof(address)) == 0);
    REQUIRE(handshake(descriptor) == 0);
    if (strcmp(argv[2], "--grant-issuance") == 0) {
        REQUIRE(initial_run(descriptor, &owner, &run) == 0);
        FILE *saved = fopen(argv[3], "wb");
        REQUIRE(saved != NULL && fwrite(&run, sizeof(run), 1U, saved) == 1U);
        REQUIRE(fflush(saved) == 0 && fsync(fileno(saved)) == 0 && fclose(saved) == 0);
    } else {
        REQUIRE(strcmp(argv[2], "--grant-issuance-recovered") == 0);
        FILE *saved = fopen(argv[3], "rb");
        REQUIRE(saved != NULL && fread(&run, sizeof(run), 1U, saved) == 1U);
        REQUIRE(fgetc(saved) == EOF && fclose(saved) == 0);
        REQUIRE(delegate_admission_refused(descriptor, &run, &run.capability, 0x33U, LXP_ERR_AUTH_REVOKED) == 0);
        REQUIRE(delegate_cannot_issue(descriptor, &run, &run.budget, 0x44U) == 0);
        REQUIRE(issue(descriptor, &owner, &run, &run.budget, LXP_ERR_SEQUENCE_REUSED) == 0);
        REQUIRE(grant_revoke(descriptor, &owner, &run, &run.budget, LXP_OK) == 0);
        REQUIRE(delegate_admission_refused(descriptor, &run, &run.budget, 0x44U, LXP_ERR_AUTH_REVOKED) == 0);
        lxp_authority_grant stale = run.budget;
        REQUIRE(issue(descriptor, &owner, &run, &stale, LXP_ERR_AUTH_SCOPE) == 0);
        REQUIRE(new_grant(&stale, 0x55U, LXP_AUTHORITY_BUDGET_ALLOWANCE,
                           run.generation, run.budget.not_before) == 0);
        REQUIRE(issue(descriptor, &owner, &run, &stale, LXP_OK) == 0);
    }
    REQUIRE(close(descriptor) == 0);
    (void)puts("canonical owner-issued capability and budget grants, complete refusal receipts, revocation and recovery passed");
    return 0;
}
