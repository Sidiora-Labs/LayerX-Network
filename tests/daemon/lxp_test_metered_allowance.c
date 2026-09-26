#define main program_admission_client_main
int program_admission_client_main(int argc, char **argv);
#include "lxp_test_program_admission.c"
#undef main
#include "layerx/lxp_authority.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_merkle.h"
#include <sys/wait.h>

enum { METERED_RECEIPTS = 33 };

typedef struct metered_run {
    uint64_t account_sequence;
    uint64_t generation;
    size_t receipt_count;
    uint8_t receipt_ids[METERED_RECEIPTS][32];
    lxp_result results[METERED_RECEIPTS];
    uint8_t root[32];
    uint8_t session_ids[4][32];
    uint8_t replacement_id[32];
    lxp_u128 replacement_spent;
} metered_run;

static const uint8_t metered_asset[32] = {
    0xb5,0xa3,0x2b,0x12,0x02,0x9f,0x8d,0xdf,0xb9,0x05,0xf9,0x0f,0x28,0x0f,0x66,0x4b,
    0x46,0x39,0x0d,0xe0,0xfc,0x62,0x77,0x0f,0xc1,0x97,0xdd,0x87,0xb1,0x8c,0xd8,0x98
};
static const uint8_t metered_program[32] = {0x49U};
static const char *metered_state_path;
static int metered_transfer(const lxp_receipt *receipt, lxp_result expected);

static int metered_artifacts(lxp_receipt *receipt)
{
    const char *port = getenv("LAYERX_TEST_METERED_PROGRAM_PORT");
    const char *token = getenv("LAYERX_TEST_METERED_PROGRAM_TOKEN_FILE");
    const char *script = getenv("LAYERX_TEST_METERED_ARTIFACT_SCRIPT");
    const char *python = getenv("LAYERX_TEST_METERED_PYTHON");
    uint8_t digest[32], arena_bytes[2U * LXP_MAX_ACTIVITY_BYTES];
    uint8_t artifacts[2U * LXP_MAX_ACTIVITY_BYTES + 8U];
    char activity_hex[65], digest_hex[65];
    static const char digits[] = "0123456789abcdef";
    lxp_arena arena;
    int pipes[2], child_status;
    size_t length = 0U;
    REQUIRE(port != NULL && token != NULL && script != NULL && python != NULL);
    REQUIRE(lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) == LXP_OK);
    REQUIRE(lxp_receipt_digest(receipt, &arena, digest) == LXP_OK);
    for (size_t i = 0U; i < 32U; ++i) {
        activity_hex[2U * i] = digits[receipt->activity_id[i] >> 4U];
        activity_hex[2U * i + 1U] = digits[receipt->activity_id[i] & 15U];
        digest_hex[2U * i] = digits[digest[i] >> 4U];
        digest_hex[2U * i + 1U] = digits[digest[i] & 15U];
    }
    activity_hex[64] = '\0'; digest_hex[64] = '\0';
    REQUIRE(pipe(pipes) == 0);
    pid_t child = fork();
    REQUIRE(child >= 0);
    if (child == 0) {
        if (close(pipes[0]) != 0 || dup2(pipes[1], STDOUT_FILENO) != STDOUT_FILENO ||
            close(pipes[1]) != 0) _exit(125);
        execl(python, python, script, port, token, activity_hex, digest_hex, (char *)NULL);
        _exit(126);
    }
    REQUIRE(close(pipes[1]) == 0);
    for (;;) {
        REQUIRE(length < sizeof(artifacts));
        ssize_t count = read(pipes[0], artifacts + length, sizeof(artifacts) - length);
        if (count < 0 && errno == EINTR) continue;
        REQUIRE(count >= 0);
        if (count == 0) break;
        length += (size_t)count;
    }
    REQUIRE(close(pipes[0]) == 0 && waitpid(child, &child_status, 0) == child);
    REQUIRE(WIFEXITED(child_status) && WEXITSTATUS(child_status) == 0);
    REQUIRE(length >= 8U);
    size_t terminal_length = load_u32(artifacts);
    REQUIRE(terminal_length <= length - 8U);
    size_t graph_length = load_u32(artifacts + 4U + terminal_length);
    REQUIRE(graph_length == length - terminal_length - 8U);
    REQUIRE(lxp_receipt_bind_program_artifacts(receipt,
        (lxp_byte_span){artifacts + 4U, terminal_length},
        (lxp_byte_span){artifacts + 8U + terminal_length, graph_length}, (lxp_byte_span){NULL, 0U}) == LXP_OK);
    REQUIRE(metered_transfer(receipt, receipt->result_code) == 0);
    receipt->program_outcome.terminal_payload = (lxp_byte_span){NULL, 0U};
    receipt->program_outcome.call_graph_payload = (lxp_byte_span){NULL, 0U};
    return 0;
}

static int metered_simulation_evidence(const wire_envelope *response, bool refused)
{
    char path[4096];
    const char *suffixes[] = {"simulation-payload", "simulation-proof"};
    const uint8_t *bytes[] = {response->payload, response->proof};
    const size_t lengths[] = {response->payload_length, response->proof_length};
    for (size_t i = 0U; i < 2U; ++i) {
        int length = snprintf(path, sizeof(path), "%s.%s%s", metered_state_path,
                              refused ? "refused-" : "", suffixes[i]);
        REQUIRE(length > 0 && (size_t)length < sizeof(path));
        FILE *output = fopen(path, "wbx");
        REQUIRE(output != NULL && fwrite(bytes[i], 1U, lengths[i], output) == lengths[i] &&
                fflush(output) == 0 && fsync(fileno(output)) == 0 && fclose(output) == 0);
    }
    return 0;
}

static int metered_identity(signer *owner)
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

static int metered_encode(const signer *key, uint64_t sequence, uint32_t type,
                            uint64_t timestamp, const uint8_t *payload,
                            size_t payload_length, uint8_t *out, size_t *length)
{
    uint8_t storage[2U * LXP_MAX_ACTIVITY_BYTES], digest[32], signature[64];
    lxp_activity activity;
    lxp_arena arena;
    lxp_byte_span encoded;
    size_t initial_length;
    REQUIRE(build_activity(key, sequence, type, timestamp, payload, payload_length,
                            out, ACTIVITY_CAPACITY, &initial_length) == 0);
    REQUIRE(lxp_activity_decode(out, initial_length, &activity) == LXP_OK);
    activity.fee_limit = (lxp_u128){0U, 67108864U};
    REQUIRE(lxp_activity_signing_preimage(&activity, digest) == LXP_OK);
    REQUIRE(sign_raw(key, digest, sizeof(digest), signature) == 0);
    activity.signature = (lxp_byte_span){signature, sizeof(signature)};
    REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
    REQUIRE(lxp_activity_encode(&activity, &arena, &encoded) == LXP_OK);
    REQUIRE(encoded.length <= ACTIVITY_CAPACITY);
    (void)memcpy(out, encoded.bytes, encoded.length);
    *length = encoded.length;
    return 0;
}

static int metered_head(int descriptor, uint64_t minimum, uint8_t root[32])
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
            (void)fprintf(stderr, "metered preparation attempt=%u refusal=%d\n",
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

static int metered_transfer(const lxp_receipt *receipt, lxp_result expected)
{
    static const uint8_t domain[] = "LXP/programs/terminal-applied-legs/v1";
    lxp_byte_span terminal = receipt->program_outcome.terminal_payload;
    uint8_t source[32], destination[32], digest[32], name[86];
    size_t offset = sizeof(domain);
    REQUIRE(receipt->program_outcome.encoding_version == 4U);
    REQUIRE(terminal.length >= sizeof(domain) + 8U &&
            memcmp(terminal.bytes, domain, sizeof(domain)) == 0);
    uint32_t detail_length = load_u32(terminal.bytes + offset);
    offset += 4U;
    REQUIRE(detail_length <= terminal.length - offset - 4U);
    offset += detail_length;
    uint32_t legs_length = load_u32(terminal.bytes + offset);
    offset += 4U;
    REQUIRE(legs_length == terminal.length - offset);
    if (expected != LXP_OK) {
        REQUIRE(legs_length == 0U && lxp_ct_is_zero(receipt->program_outcome.transfer_root, 32U));
        return 0;
    }
    REQUIRE(legs_length == 115U);
    (void)memcpy(name, "agent:", 6U);
    (void)memcpy(name + 6U, REGISTERED_DID, 75U);
    (void)memcpy(name + 81U, ":main", 5U);
    REQUIRE(lx_account_id_from_string(name, sizeof(name), source) == LXP_OK);
    REQUIRE(lx_account_id_from_string((const uint8_t *)"system:fees", 11U, destination) == LXP_OK);
    const uint8_t *leg = terminal.bytes + offset;
    REQUIRE(leg[0] == 0U && memcmp(leg + 1U, source, 32U) == 0 &&
            memcmp(leg + 33U, destination, 32U) == 0 &&
            memcmp(leg + 65U, metered_asset, 32U) == 0);
    REQUIRE(load_u64(leg + 97U) == 0U && load_u64(leg + 105U) == 2U &&
            load_u16(leg + 113U) == 1U);
    REQUIRE(lxp_hash_sha256(leg, legs_length, digest) == LXP_OK);
    REQUIRE(memcmp(digest, receipt->program_outcome.applied_legs_digest, 32U) == 0);
    REQUIRE(lxp_merkle_leaf_hash(leg, legs_length, digest) == LXP_OK);
    REQUIRE(memcmp(digest, receipt->program_outcome.transfer_root, 32U) == 0);
    return 0;
}

static int metered_receipt(int descriptor, const uint8_t id[32],
                             lxp_result expected, lxp_receipt *receipt)
{
    uint8_t query[34] = {1U};
    (void)memcpy(query + 1U, id, 32U);
    for (unsigned attempt = 0U; attempt < 200U; ++attempt) {
        wire_envelope response;
        REQUIRE(send_request(descriptor, LNI_MINOR, 5U, 601U, query, sizeof(query)) == 0);
        REQUIRE(receive_envelope(descriptor, &response) == 0);
        REQUIRE(response.tag == 6U && response.correlation_id == 601U);
        if (response.payload_length != 0U) {
            uint8_t storage[2U * LXP_MAX_ACTIVITY_BYTES];
            lxp_arena arena;
            signer sequencer;
            REQUIRE(signer_init(&sequencer, 0x22U) == 0);
            REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
            REQUIRE(lxp_receipt_decode(response.payload, response.payload_length, true, receipt) == LXP_OK);
            REQUIRE(lxp_receipt_verify(receipt, sequencer.public_key, &arena) == LXP_OK);
            REQUIRE(memcmp(receipt->activity_id, id, 32U) == 0);
            if (receipt->result_code != expected)
                (void)fprintf(stderr, "metered receipt: %d expected %d\n", receipt->result_code, expected);
            REQUIRE(receipt->result_code == expected);
            if (receipt->module_id == LXP_MODULE_PROGRAMS && receipt->program_outcome.present) {
                REQUIRE(receipt->program_outcome.result_code == expected);
                REQUIRE(receipt->program_outcome.terminal_kind ==
                    (expected == LXP_OK ? LXP_PROGRAM_TERMINAL_SUCCESS : LXP_PROGRAM_TERMINAL_FAILURE));
                REQUIRE(metered_artifacts(receipt) == 0);
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

static int metered_submit(int descriptor, metered_run *run, const uint8_t *encoded,
                            size_t length, lxp_result expected)
{
    REQUIRE(run->receipt_count < METERED_RECEIPTS);
    uint8_t *id = run->receipt_ids[run->receipt_count];
    REQUIRE(lxp_activity_id(encoded, length, id) == LXP_OK);
    REQUIRE(send_request(descriptor, LNI_MINOR, SUBMIT_REQUEST, 600U, encoded, length) == 0);
    REQUIRE(expect_ack(descriptor, 600U, encoded, length, id) == 0);
    run->results[run->receipt_count++] = expected;
    ++run->account_sequence;
    return 0;
}

static int metered_execute(int descriptor, const signer *key, metered_run *run,
                             uint32_t type, const uint8_t *payload, size_t payload_length,
                             lxp_result expected, lxp_receipt *receipt)
{
    uint8_t encoded[ACTIVITY_CAPACITY];
    size_t length;
    REQUIRE(metered_encode(key, run->account_sequence, type, 0U, payload,
                            payload_length, encoded, &length) == 0);
    REQUIRE(metered_submit(descriptor, run, encoded, length, expected) == 0);
    REQUIRE(metered_receipt(descriptor, run->receipt_ids[run->receipt_count - 1U],
                             expected, receipt) == 0);
    if (type == LX_PROGRAMS_CALL) REQUIRE(receipt->program_outcome.present);
    REQUIRE(metered_head(descriptor, receipt->global_sequence + 1U, run->root) == 0);
    return 0;
}

static void metered_leb(uint8_t *out, size_t *cursor, uint32_t value)
{
    do {
        uint8_t byte = (uint8_t)(value & 0x7fU);
        value >>= 7U;
        out[(*cursor)++] = value == 0U ? byte : (uint8_t)(byte | 0x80U);
    } while (value != 0U);
}

static void metered_name(uint8_t *out, size_t *cursor, const char *name)
{
    size_t length = strlen(name);
    metered_leb(out, cursor, (uint32_t)length);
    (void)memcpy(out + *cursor, name, length);
    *cursor += length;
}

static void metered_section(uint8_t *out, size_t *cursor, uint8_t id,
                             const uint8_t *body, size_t length)
{
    out[(*cursor)++] = id;
    metered_leb(out, cursor, (uint32_t)length);
    (void)memcpy(out + *cursor, body, length);
    *cursor += length;
}

static size_t metered_wasm(uint8_t out[512], const uint8_t destination[32])
{
    static const uint8_t types[] = {
        3U, 0x60U, 6U, 0x7eU, 0x7eU, 0x7fU, 0x7fU, 0x7fU, 0x7fU, 1U, 0x7fU,
        0x60U, 1U, 0x7fU, 1U, 0x7fU, 0x60U, 2U, 0x7fU, 0x7fU, 1U, 0x7fU
    };
    static const uint8_t functions[] = {2U, 1U, 2U};
    static const uint8_t memory[] = {1U, 1U, 1U, 1U};
    static const uint8_t code[] = {
        2U, 4U, 0U, 0x41U, 0U, 0x0bU,
        18U, 0U, 0x42U, 0U, 0x42U, 2U,
        0x41U, 0x80U, 1U, 0x41U, 32U, 0x41U, 0xa0U, 1U, 0x41U, 32U,
        0x10U, 0U, 0x0bU
    };
    uint8_t section[128];
    size_t cursor = 8U, length = 0U;
    (void)memcpy(out, "\0asm\1\0\0\0", 8U);
    metered_section(out, &cursor, 1U, types, sizeof(types));
    section[length++] = 1U;
    metered_name(section, &length, "layerx_v1");
    metered_name(section, &length, "transfer_402");
    section[length++] = 0U; section[length++] = 0U;
    metered_section(out, &cursor, 2U, section, length);
    metered_section(out, &cursor, 3U, functions, sizeof(functions));
    metered_section(out, &cursor, 5U, memory, sizeof(memory));
    length = 0U; section[length++] = 3U;
    metered_name(section, &length, "layerx_reserve");
    section[length++] = 0U; section[length++] = 1U;
    metered_name(section, &length, "layerx_call");
    section[length++] = 0U; section[length++] = 2U;
    metered_name(section, &length, "memory");
    section[length++] = 2U; section[length++] = 0U;
    metered_section(out, &cursor, 7U, section, length);
    metered_section(out, &cursor, 10U, code, sizeof(code));
    length = 0U; section[length++] = 1U; section[length++] = 0U;
    section[length++] = 0x41U; section[length++] = 0x80U;
    section[length++] = 1U; section[length++] = 0x0bU;
    section[length++] = 64U;
    (void)memcpy(section + length, metered_asset, 32U); length += 32U;
    (void)memcpy(section + length, destination, 32U); length += 32U;
    metered_section(out, &cursor, 11U, section, length);
    return cursor;
}

static size_t metered_call(uint8_t payload[512], const uint8_t destination[32])
{
    static const uint8_t entrypoint[] = "layerx_call";
    static const uint8_t access[] = "LayerX/programs/access-declaration/v1\0";
    static const uint64_t budgets[] = {
        1000000U, 16777216U, 1048576U, 1048576U, 64U, 1048576U, 4096U
    };
    (void)memset(payload, 0, 512U);
    (void)memcpy(payload, metered_program, 32U);
    store_u16(payload + 32U, LX_PROGRAMS_ACCOUNT_ABI_VERSION);
    store_u16(payload + 34U, (uint16_t)(sizeof(entrypoint) - 1U));
    store_u16(payload + 40U, 83U);
    store_u32(payload + 42U, (uint32_t)sizeof(access));
    store_u32(payload + 46U, 16U);
    for (size_t i = 0U; i < 7U; ++i) store_u64(payload + 50U + 8U * i, budgets[i]);
    (void)memcpy(payload + 106U, entrypoint, sizeof(entrypoint) - 1U);
    size_t cursor = 106U + sizeof(entrypoint) - 1U;
    store_u16(payload + cursor, 1U);
    payload[cursor + 2U] = 5U;
    (void)memcpy(payload + cursor + 3U, metered_asset, 32U);
    (void)memcpy(payload + cursor + 35U, destination, 32U);
    store_u64(payload + cursor + 75U, 2U);
    cursor += 83U;
    (void)memcpy(payload + cursor, access, sizeof(access));
    return cursor + sizeof(access);
}

static int metered_issue(int descriptor, const signer *owner, metered_run *run,
                           uint8_t seed, lxp_authority_kind kind, bool wrong_asset)
{
    uint8_t storage[4096], payload[1024];
    signer delegate;
    lxp_authority_grant grant = {0};
    lxp_arena arena;
    lxp_byte_span body;
    lxp_codec_writer writer;
    lxp_receipt receipt;
    struct timespec now;
    REQUIRE(signer_init(&delegate, seed) == 0);
    REQUIRE(clock_gettime(CLOCK_REALTIME, &now) == 0);
    REQUIRE(lxp_did_id_derive(REGISTERED_DID, sizeof(REGISTERED_DID) - 1U, grant.grantor) == LXP_OK);
    (void)memcpy(grant.grantee, grant.grantor, 32U);
    (void)memcpy(grant.key, delegate.public_key, 32U);
    grant.kind = kind;
    grant.scope.module_mask = UINT64_C(1) << LXP_MODULE_PROGRAMS;
    grant.scope.activity_ordinal_min = 3U;
    grant.scope.activity_ordinal_max = 3U;
    (void)memcpy(grant.scope.asset_id, metered_asset, 32U);
    if (wrong_asset) grant.scope.asset_id[31] ^= 1U;
    grant.scope.maximum_per_activity = (lxp_u128){0U, 2U};
    grant.scope.maximum_total = (lxp_u128){0U, 5U};
    grant.scope.purpose_hash[0] = seed;
    grant.not_before = (uint64_t)now.tv_sec * 1000U - 60000U;
    grant.not_after = grant.not_before + 3600000U;
    grant.grantor_revocation_sequence = run->generation;
    grant.fee_budget.present = seed != 0x49U;
    (void)memcpy(grant.fee_budget.asset_id, metered_asset, 32U);
    grant.fee_budget.maximum_per_activity = (lxp_u128){0U, 67108864U};
    grant.fee_budget.maximum_total = (lxp_u128){0U, 536870912U};
    if (seed == 0x47U) grant.fee_budget.maximum_total = grant.fee_budget.maximum_per_activity;
    if (seed == 0x48U) {
        grant.fee_budget.maximum_total = (lxp_u128){0U, 134217728U};
        grant.fee_budget.period_length = 3600000U;
        grant.fee_budget.period_start = grant.not_before;
        grant.fee_budget.maximum_per_period = grant.fee_budget.maximum_per_activity;
    }
    if (kind == LXP_AUTHORITY_BUDGET_ALLOWANCE) {
        grant.scope.period_length = 3600000U;
        grant.scope.period_start = grant.not_before;
        grant.scope.maximum_per_period = (lxp_u128){0U, 3U};
    }
    REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
    REQUIRE(lxp_grant_encode(&grant, &arena, &body) == LXP_OK);
    REQUIRE(lxp_codec_writer_init(&writer, &arena, sizeof(payload)) == LXP_OK);
    REQUIRE(lxp_codec_write_u16(&writer, 0x7108U) == LXP_OK);
    REQUIRE(lxp_codec_write_u16(&writer, 0x0101U) == LXP_OK);
    REQUIRE(lxp_codec_write_bytes(&writer, body.bytes, body.length, 1024U) == LXP_OK);
    REQUIRE(writer.length <= sizeof(payload));
    (void)memcpy(payload, writer.bytes, writer.length);
    REQUIRE(metered_execute(descriptor, owner, run, 0x00070008U, payload,
                             writer.length, LXP_OK, &receipt) == 0);
    REQUIRE(receipt.effects.count == 2U + (body.length + 255U) / 256U &&
            receipt.effects.effects[0].event_type == 0x7148U);
    return 0;
}

static int metered_simulate(int descriptor, const signer *delegate, metered_run *run,
                              const uint8_t *payload, size_t payload_length,
                              lxp_result expected)
{
    static const uint8_t domain[] = "LayerX/agent/program-simulation-evidence/v1";
    static const uint8_t boundary_domain[] = "LayerX/emulator/simulation-boundary/v1";
    uint8_t preparation[79], encoded[ACTIVITY_CAPACITY], id[32], root[32];
    uint8_t digest_input[sizeof(domain) + 145U], digest[32], query[34] = {1U};
    uint8_t boundary_input[sizeof(boundary_domain) + 32U];
    uint8_t receipt_storage[2U * LXP_MAX_ACTIVITY_BYTES];
    wire_envelope response;
    lxp_receipt receipt;
    lxp_arena receipt_arena;
    signer sequencer;
    size_t length;
    uint64_t timestamp;
    store_u16(preparation, 1U); store_u16(preparation + 2U, 75U);
    (void)memcpy(preparation + 4U, REGISTERED_DID, 75U);
    REQUIRE(send_request(descriptor, LNI_MINOR, 26U, 610U, preparation, sizeof(preparation)) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    REQUIRE(response.tag == 27U && response.payload_length >= 99U);
    timestamp = load_u64(response.payload + 91U);
    REQUIRE(timestamp != 0U);
    release_envelope(&response);
    REQUIRE(metered_encode(delegate, run->account_sequence, LX_PROGRAMS_CALL,
                            timestamp, payload, payload_length, encoded, &length) == 0);
    REQUIRE(lxp_activity_id(encoded, length, id) == LXP_OK);
    REQUIRE(send_request(descriptor, LNI_MINOR, 30U, 611U, encoded, length) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    REQUIRE(response.tag == 31U && response.correlation_id == 611U);
    REQUIRE(response.payload_length >= 46U && response.proof_length == 242U);
    REQUIRE(metered_simulation_evidence(&response, expected != LXP_OK) == 0);
    REQUIRE(load_u16(response.payload) == 1U && load_u16(response.proof) == 1U);
    REQUIRE(memcmp(response.payload + 2U, id, 32U) == 0 && memcmp(response.proof + 34U, id, 32U) == 0);
    uint32_t receipt_length = load_u32(response.payload + 34U);
    REQUIRE(receipt_length <= response.payload_length - 46U);
    REQUIRE(lxp_receipt_decode(response.payload + 38U, receipt_length, true, &receipt) == LXP_OK);
    REQUIRE(signer_init(&sequencer, 0x22U) == 0);
    REQUIRE(lxp_arena_init(&receipt_arena, receipt_storage, sizeof(receipt_storage)) == LXP_OK);
    REQUIRE(lxp_receipt_verify(&receipt, sequencer.public_key, &receipt_arena) == LXP_OK);
    if (receipt.result_code != expected)
        (void)fprintf(stderr, "metered simulation receipt result=%d terminal=%u outcome=%d\n",
                      receipt.result_code, (unsigned)receipt.program_outcome.terminal_kind,
                      receipt.program_outcome.result_code);
    REQUIRE(memcmp(receipt.activity_id, id, 32U) == 0);
    size_t cursor = 38U + receipt_length;
    uint32_t terminal_length = load_u32(response.payload + cursor);
    cursor += 4U;
    REQUIRE(terminal_length <= response.payload_length - cursor - 4U);
    lxp_byte_span terminal = {response.payload + cursor, terminal_length};
    cursor += terminal_length;
    uint32_t graph_length = load_u32(response.payload + cursor);
    cursor += 4U;
    REQUIRE(graph_length == response.payload_length - cursor);
    lxp_byte_span graph = {response.payload + cursor, graph_length};
    REQUIRE(lxp_receipt_bind_program_artifacts(&receipt, terminal, graph, (lxp_byte_span){NULL, 0U}) == LXP_OK);
    if (expected == LXP_OK) {
        REQUIRE(receipt.result_code == LXP_OK && receipt.program_outcome.present &&
                receipt.program_outcome.terminal_kind == LXP_PROGRAM_TERMINAL_SUCCESS);
        REQUIRE(metered_transfer(&receipt, LXP_OK) == 0);
    } else {
        static const uint8_t failure_domain[] = "LXP/programs/pre-runtime-failure/v1";
        static const uint8_t empty_graph[] = "LXP/programs/empty-call-graph/v1";
        size_t hash_domain_length = 0U;
        const uint8_t *hash_domain = lxp_domain_tag(LXP_DOMAIN_CONTEXT_HASH, &hash_domain_length);
        uint8_t expected_terminal[256], expected_graph[128], payload_hash[32], empty_digest[32];
        size_t offset = 0U;
        lxp_activity original;
        REQUIRE(expected == LXP_ERR_NON_CANONICAL && receipt.result_code == expected &&
                receipt.program_outcome.present &&
                receipt.program_outcome.result_code == expected &&
                receipt.program_outcome.terminal_kind == LXP_PROGRAM_TERMINAL_FAILURE);
        REQUIRE(lxp_activity_decode(encoded, length, &original) == LXP_OK);
        REQUIRE(receipt.protocol_version == original.protocol_version && receipt.protocol_version == 3U &&
                receipt.module_id == LXP_MODULE_PROGRAMS && original.network_id == 77U);
        REQUIRE(lxp_hash_payload(payload, payload_length, payload_hash) == LXP_OK);
        REQUIRE(memcmp(original.payload_hash, payload_hash, 32U) == 0);
        REQUIRE(hash_domain != NULL && hash_domain_length + sizeof(failure_domain) + 109U <= sizeof(expected_terminal));
        (void)memcpy(expected_terminal, hash_domain, hash_domain_length); offset += hash_domain_length;
        (void)memcpy(expected_terminal + offset, failure_domain, sizeof(failure_domain)); offset += sizeof(failure_domain);
        (void)memcpy(expected_terminal + offset, id, 32U); offset += 32U;
        (void)memcpy(expected_terminal + offset, payload_hash, 32U); offset += 32U;
        store_u32(expected_terminal + offset, (uint32_t)expected); offset += 4U;
        store_u32(expected_terminal + offset, receipt.module_version); offset += 4U;
        store_u32(expected_terminal + offset, receipt.parameter_version); offset += 4U;
        expected_terminal[offset++] = 4U;
        REQUIRE(lxp_hash_sha256("", 0U, empty_digest) == LXP_OK);
        (void)memcpy(expected_terminal + offset, empty_digest, 32U); offset += 32U;
        REQUIRE(terminal.length == offset && memcmp(terminal.bytes, expected_terminal, offset) == 0);
        REQUIRE(hash_domain_length + sizeof(empty_graph) <= sizeof(expected_graph));
        (void)memcpy(expected_graph, hash_domain, hash_domain_length);
        (void)memcpy(expected_graph + hash_domain_length, empty_graph, sizeof(empty_graph));
        REQUIRE(graph.length == hash_domain_length + sizeof(empty_graph) &&
                memcmp(graph.bytes, expected_graph, graph.length) == 0);
        REQUIRE(receipt.program_outcome.encoding_version == 4U &&
                receipt.program_outcome.abi_version == load_u16(payload + 32U) &&
                lxp_ct_is_zero(receipt.program_outcome.transfer_root, 32U) &&
                memcmp(receipt.program_outcome.applied_legs_digest, empty_digest, 32U) == 0);
    }
    REQUIRE(memcmp(response.proof + 146U, sequencer.public_key, 32U) == 0);
    REQUIRE(memcmp(response.proof + 66U, receipt.previous_state_root, 32U) == 0 &&
            memcmp(response.proof + 66U, run->root, 32U) == 0 &&
            memcmp(response.proof + 98U, receipt.resulting_state_root, 32U) == 0);
    REQUIRE(load_u64(response.proof + 130U) < UINT64_MAX &&
            load_u64(response.proof + 130U) + 1U == receipt.global_sequence &&
            load_u64(response.proof + 138U) == timestamp);
    (void)memcpy(boundary_input, boundary_domain, sizeof(boundary_domain));
    (void)memcpy(boundary_input + sizeof(boundary_domain), sequencer.public_key, 32U);
    REQUIRE(lxp_hash_sha256(boundary_input, sizeof(boundary_input), digest) == LXP_OK);
    REQUIRE(memcmp(response.proof + 2U, digest, 32U) == 0);
    (void)memcpy(digest_input, domain, sizeof(domain));
    (void)memcpy(digest_input + sizeof(domain), response.proof + 2U, 144U);
    digest_input[sizeof(digest_input) - 1U] = 0U;
    REQUIRE(lxp_hash_sha256(digest_input, sizeof(digest_input), digest) == LXP_OK);
    REQUIRE(lxp_ed25519_verify_raw(sequencer.public_key, response.proof + 178U, digest, sizeof(digest)) == LXP_OK);
    release_envelope(&response);
    (void)memcpy(query + 1U, id, 32U);
    REQUIRE(send_request(descriptor, LNI_MINOR, 5U, 612U, query, sizeof(query)) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    REQUIRE(response.tag == 6U && response.payload_length == 0U);
    release_envelope(&response);
    REQUIRE(metered_head(descriptor, 0U, root) == 0);
    REQUIRE(memcmp(root, run->root, 32U) == 0);
    return 0;
}

static int metered_fee_refusals(int descriptor, metered_run *run,
                                  const uint8_t *payload, size_t payload_length)
{
    uint8_t encoded[ACTIVITY_CAPACITY], root[32];
    size_t length;
    for (uint8_t seed = 0x47U; seed <= 0x49U; ++seed) {
        signer delegate;
        REQUIRE(signer_init(&delegate, seed) == 0);
        REQUIRE(metered_encode(&delegate, run->account_sequence, LX_PROGRAMS_CALL,
            0U, payload, payload_length, encoded, &length) == 0);
        REQUIRE(send_request(descriptor, LNI_MINOR, SUBMIT_REQUEST, 620U, encoded, length) == 0);
        REQUIRE(expect_error(descriptor, 620U, 4U,
            seed == 0x49U ? LXP_ERR_AUTH_ALLOWANCE : LXP_ERR_GRANT_EXHAUSTED) == 0);
        REQUIRE(metered_head(descriptor, 0U, root) == 0);
        REQUIRE(memcmp(root, run->root, 32U) == 0);
    }
    return 0;
}

static int metered_next_sequence(int descriptor, uint64_t *sequence)
{
    uint8_t preparation[79];
    wire_envelope response;
    store_u16(preparation, 1U); store_u16(preparation + 2U, 75U);
    (void)memcpy(preparation + 4U, REGISTERED_DID, 75U);
    REQUIRE(send_request(descriptor, LNI_MINOR, 26U, 630U, preparation, sizeof(preparation)) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    REQUIRE(response.tag == 27U && response.payload_length >= 139U);
    uint64_t previous = load_u64(response.payload + 99U);
    REQUIRE(previous < UINT64_MAX);
    *sequence = previous + 1U;
    release_envelope(&response);
    return 0;
}

static int metered_session_issue(int descriptor, const signer *owner, metered_run *run,
    lxp_authority_grant *grant, const uint8_t *predecessor, const uint8_t *commitment,
    lxp_result expected)
{
    uint8_t storage[4096], payload[1024], action[32] = {0};
    lxp_arena arena;
    lxp_byte_span body;
    lxp_codec_writer writer;
    lxp_receipt receipt;
    uint64_t sequence;
    REQUIRE(metered_next_sequence(descriptor, &sequence) == 0);
    REQUIRE(sequence < UINT64_MAX - 1024U);
    store_u64(action + 24U, sequence);
    REQUIRE(lxp_grant_id_compute(grant, grant->grant_id) == LXP_OK);
    REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
    REQUIRE(lxp_grant_encode(grant, &arena, &body) == LXP_OK);
    REQUIRE(lxp_codec_writer_init(&writer, &arena, sizeof(payload)) == LXP_OK);
    REQUIRE(lxp_codec_write_u16(&writer, 0x7105U) == LXP_OK);
    REQUIRE(lxp_codec_write_u16(&writer, predecessor == NULL ? 0x0103U : 0x0205U) == LXP_OK);
    REQUIRE(lxp_codec_write_bytes(&writer, body.bytes, body.length, 1024U) == LXP_OK);
    REQUIRE(lxp_codec_write_u64(&writer, sequence + 1024U) == LXP_OK);
    REQUIRE(lxp_codec_write_bytes(&writer, action, 32U, 32U) == LXP_OK);
    if (predecessor != NULL) {
        REQUIRE(commitment != NULL);
        REQUIRE(lxp_codec_write_bytes(&writer, predecessor, 32U, 32U) == LXP_OK);
        REQUIRE(lxp_codec_write_bytes(&writer, commitment, 32U, 32U) == LXP_OK);
    }
    REQUIRE(writer.length <= sizeof(payload));
    (void)memcpy(payload, writer.bytes, writer.length);
    REQUIRE(metered_execute(descriptor, owner, run, 0x00070005U, payload,
        writer.length, expected, &receipt) == 0);
    REQUIRE(receipt.result_code == expected);
    return 0;
}

static int metered_session_client(int descriptor, const uint8_t id[32],
                                  const wire_envelope *response)
{
    const char *client = getenv("LAYERX_TEST_SESSION_FEE_CLIENT");
    const char *clock = getenv("LAYERX_TEST_SESSION_FEE_CLOCK");
    const char *clock_directory = getenv("LAYERX_TEST_SESSION_FEE_CLOCK_DIRECTORY");
    struct sockaddr_un address;
    socklen_t address_length = sizeof(address);
    char id_hex[65], payload_hex[2801];
    static const char digits[] = "0123456789abcdef";
    int child_status;
    if (client == NULL) return 0;
    REQUIRE(clock != NULL && clock_directory != NULL);
    REQUIRE(response->payload_length <= 1400U);
    REQUIRE(getpeername(descriptor, (struct sockaddr *)&address, &address_length) == 0);
    for (size_t i = 0U; i < 32U; ++i) {
        id_hex[i * 2U] = digits[id[i] >> 4U];
        id_hex[i * 2U + 1U] = digits[id[i] & 15U];
    }
    id_hex[64] = '\0';
    for (size_t i = 0U; i < response->payload_length; ++i) {
        payload_hex[i * 2U] = digits[response->payload[i] >> 4U];
        payload_hex[i * 2U + 1U] = digits[response->payload[i] & 15U];
    }
    payload_hex[response->payload_length * 2U] = '\0';
    pid_t child = fork();
    REQUIRE(child >= 0);
    if (child == 0) {
        if (setenv("LAYERX_TEST_SESSION_FEE_SOCKET", address.sun_path, 1) != 0 ||
            setenv("LAYERX_TEST_SESSION_FEE_GRANT", id_hex, 1) != 0 ||
            setenv("LAYERX_TEST_SESSION_FEE_EXPECTED", payload_hex, 1) != 0) _exit(125);
        execl(clock, clock, "--runtime-dir", clock_directory, "--", client,
              "--exact", "real_daemon_session_fee_state", "--nocapture",
              "--test-threads=1", (char *)NULL);
        _exit(126);
    }
    REQUIRE(waitpid(child, &child_status, 0) == child);
    REQUIRE(WIFEXITED(child_status) && WEXITSTATUS(child_status) == 0);
    return 0;
}

static int metered_session_read_refusals(int descriptor, const uint8_t authentication_id[32])
{
    uint8_t request[34] = {0};
    store_u16(request, 1U);
    REQUIRE(send_request(descriptor, LNI_MINOR, 36U, 636U, request, sizeof(request)) == 0);
    REQUIRE(expect_error(descriptor, 636U, 1U, LXP_ERR_NON_CANONICAL) == 0);
    (void)memcpy(request + 2U, authentication_id, 32U);
    REQUIRE(send_request(descriptor, TYPED_READ_MINOR - 1U, 36U, 637U, request, sizeof(request)) == 0);
    REQUIRE(expect_error(descriptor, 637U, 1U, LXP_ERR_NON_CANONICAL) == 0);
    REQUIRE(send_request(descriptor, LNI_MINOR, 36U, 638U, request, sizeof(request) - 1U) == 0);
    REQUIRE(expect_error(descriptor, 638U, 1U, LXP_ERR_NON_CANONICAL) == 0);
    REQUIRE(send_request(descriptor, LNI_MINOR, 36U, 639U, request, sizeof(request)) == 0);
    REQUIRE(expect_error(descriptor, 639U, 4U, LXP_ERR_AUTH_SCOPE) == 0);
    return 0;
}

static int metered_session_read(int descriptor, const uint8_t id[32],
    lxp_authority_grant *grant, uint8_t successor[32], uint8_t commitment[32])
{
    uint8_t request[34], computed[32], expected_id[32];
    wire_envelope response;
    (void)memcpy(expected_id, id, sizeof(expected_id));
    store_u16(request, 1U); (void)memcpy(request + 2U, expected_id, 32U);
    REQUIRE(send_request(descriptor, LNI_MINOR, 36U, 632U, request, sizeof(request)) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    REQUIRE(response.tag == 37U && response.correlation_id == 632U && response.payload_length >= 188U);
    REQUIRE(response.proof_length == 0U && load_u16(response.payload) == 1U && !lxp_ct_is_zero(response.payload + 10U, 32U));
    size_t length = load_u16(response.payload + 42U);
    REQUIRE(length <= 1024U && response.payload_length == 44U + length + 144U);
    REQUIRE(lxp_grant_decode(response.payload + 44U, length, grant) == LXP_OK);
    REQUIRE(lxp_grant_id_compute(grant, computed) == LXP_OK && memcmp(computed, expected_id, 32U) == 0);
    (void)memcpy(grant->grant_id, expected_id, 32U);
    const uint8_t *facts = response.payload + 44U + length;
    grant->revoked_at_sequence = load_u64(facts);
    grant->revoked = grant->revoked_at_sequence != 0U;
    REQUIRE(grant->revoked_at_sequence <= load_u64(response.payload + 2U));
    if (grant->fee_budget.present) {
        lxp_authority_scope counters = {0};
        REQUIRE(lxp_authority_charge_record_decode(facts + 8U, 72U, expected_id, &counters) == LXP_OK);
        grant->fee_budget.spent_total = counters.spent_total;
        grant->fee_budget.spent_this_period = counters.spent_this_period;
        grant->fee_budget.period_start = counters.period_start;
    } else REQUIRE(lxp_ct_is_zero(facts + 8U, 72U));
    (void)memcpy(successor, facts + 80U, 32U);
    (void)memcpy(commitment, facts + 112U, 32U);
    if (grant->fee_budget.present && grant->revoked) {
        REQUIRE(lxp_authority_session_charge_commitment(grant, computed) == LXP_OK);
        REQUIRE(memcmp(commitment, computed, 32U) == 0);
    } else REQUIRE(lxp_ct_is_zero(commitment, 32U));
    REQUIRE(metered_session_client(descriptor, expected_id, &response) == 0);
    release_envelope(&response);
    return 0;
}

static int metered_session_refuse(int descriptor, metered_run *run, uint8_t seed,
    const uint8_t *payload, size_t payload_length, lxp_result expected)
{
    uint8_t encoded[ACTIVITY_CAPACITY], root[32];
    size_t length;
    signer session;
    REQUIRE(signer_init(&session, seed) == 0);
    REQUIRE(metered_encode(&session, run->account_sequence, LX_PROGRAMS_CALL, 0U,
        payload, payload_length, encoded, &length) == 0);
    REQUIRE(send_request(descriptor, LNI_MINOR, SUBMIT_REQUEST, 633U, encoded, length) == 0);
    REQUIRE(expect_error(descriptor, 633U, 4U, expected) == 0);
    REQUIRE(metered_head(descriptor, 0U, root) == 0 && memcmp(root, run->root, 32U) == 0);
    return 0;
}

static int metered_sessions_initial(int descriptor, const signer *owner, metered_run *run,
    const uint8_t *payload, size_t payload_length)
{
    uint8_t did[32];
    struct timespec now;
    lxp_authority_grant grant;
    lxp_receipt receipt;
    REQUIRE(clock_gettime(CLOCK_REALTIME, &now) == 0);
    uint64_t starts = (uint64_t)now.tv_sec * 1000U - 60000U;
    REQUIRE(lxp_did_id_derive(REGISTERED_DID, sizeof(REGISTERED_DID) - 1U, did) == LXP_OK);
    for (uint8_t seed = 0x51U; seed <= 0x54U; ++seed) {
        signer session;
        REQUIRE(signer_init(&session, seed) == 0);
        if (seed == 0x53U) {
            REQUIRE(lxp_authentication_key_bind(&grant, did, session.public_key,
                starts, starts + 3600000U, run->generation) == LXP_OK);
        } else {
            REQUIRE(lxp_session_key_bind(&grant, did, session.public_key,
                UINT64_C(1) << LXP_MODULE_PROGRAMS, 3U, 3U,
                starts, starts + 3600000U, run->generation) == LXP_OK);
            if (seed != 0x51U) {
                grant.fee_budget.present = true;
                (void)memcpy(grant.fee_budget.asset_id, metered_asset, 32U);
                grant.fee_budget.maximum_per_activity = (lxp_u128){0U, 67108864U};
                grant.fee_budget.maximum_total = (lxp_u128){0U, seed == 0x54U ? 67108864U : 134217728U};
                grant.fee_budget.period_length = 3600000U;
                grant.fee_budget.maximum_per_period = grant.fee_budget.maximum_total;
                grant.fee_budget.period_start = starts;
            }
        }
        REQUIRE(metered_session_issue(descriptor, owner, run, &grant, NULL, NULL, LXP_OK) == 0);
        (void)memcpy(run->session_ids[seed - 0x51U], grant.grant_id, 32U);
        if (seed == 0x52U || seed == 0x54U) {
            REQUIRE(metered_execute(descriptor, &session, run, LX_PROGRAMS_CALL,
                payload, payload_length, LXP_OK, &receipt) == 0);
            REQUIRE(!lxp_u128_is_zero(receipt.fee_charged));
            uint8_t successor[32], commitment[32];
            REQUIRE(metered_session_read(descriptor, grant.grant_id, &grant, successor, commitment) == 0);
            REQUIRE(lxp_u128_cmp(grant.fee_budget.spent_total, receipt.fee_charged) == 0 &&
                lxp_u128_cmp(grant.fee_budget.spent_this_period, receipt.fee_charged) == 0 &&
                lxp_ct_is_zero(successor, 32U) && !grant.revoked);
        }
    }
    REQUIRE(metered_session_read_refusals(descriptor, run->session_ids[2]) == 0);
    REQUIRE(metered_session_refuse(descriptor, run, 0x51U, payload, payload_length, LXP_ERR_AUTH_ALLOWANCE) == 0);
    REQUIRE(metered_session_refuse(descriptor, run, 0x53U, payload, payload_length, LXP_ERR_AUTH_SCOPE) == 0);
    REQUIRE(metered_session_refuse(descriptor, run, 0x54U, payload, payload_length, LXP_ERR_GRANT_EXHAUSTED) == 0);
    puts("owner-issued fee sessions execute and persist charges; legacy paid and authentication-only spending refuse");
    return 0;
}

static int metered_session_revoke(int descriptor, const signer *owner, metered_run *run, const uint8_t id[32])
{
    uint8_t payload[45] = {0x71U, 6U, 0U, 3U};
    uint64_t sequence;
    lxp_receipt receipt;
    REQUIRE(metered_next_sequence(descriptor, &sequence) == 0);
    (void)memcpy(payload + 4U, id, 32U); payload[36] = 1U;
    store_u64(payload + 37U, sequence);
    REQUIRE(metered_execute(descriptor, owner, run, 0x00070006U, payload, sizeof(payload), LXP_OK, &receipt) == 0);
    REQUIRE(receipt.global_sequence == sequence);
    run->generation = sequence;
    return 0;
}

static int metered_session_replace(int descriptor, const signer *owner, metered_run *run,
    const uint8_t *payload, size_t payload_length)
{
    uint8_t successor[32], commitment[32], changed[32];
    lxp_authority_grant prior, next, invalid;
    lxp_receipt receipt;
    signer replacement;
    REQUIRE(metered_session_refuse(descriptor, run, 0x51U, payload, payload_length, LXP_ERR_AUTH_ALLOWANCE) == 0);
    REQUIRE(metered_session_refuse(descriptor, run, 0x53U, payload, payload_length, LXP_ERR_AUTH_SCOPE) == 0);
    REQUIRE(metered_session_refuse(descriptor, run, 0x54U, payload, payload_length, LXP_ERR_GRANT_EXHAUSTED) == 0);
    REQUIRE(metered_session_revoke(descriptor, owner, run, run->session_ids[2]) == 0);
    REQUIRE(metered_session_refuse(descriptor, run, 0x53U, payload, payload_length, LXP_ERR_AUTH_REVOKED) == 0);
    REQUIRE(metered_session_revoke(descriptor, owner, run, run->session_ids[1]) == 0);
    REQUIRE(metered_session_read(descriptor, run->session_ids[1], &prior, successor, commitment) == 0);
    REQUIRE(prior.revoked && lxp_ct_is_zero(successor, 32U) && !lxp_ct_is_zero(commitment, 32U));
    REQUIRE(signer_init(&replacement, 0x55U) == 0);
    next = prior;
    (void)memcpy(next.key, replacement.public_key, 32U);
    next.revoked = false; next.revoked_at_sequence = 0U;
    next.grantor_revocation_sequence = run->generation;
    next.fee_budget.spent_total = (lxp_u128){0U, 0U};
    next.fee_budget.spent_this_period = (lxp_u128){0U, 0U};
    next.fee_budget.period_start = next.not_before;
    (void)memcpy(changed, commitment, 32U); changed[0] ^= 1U;
    REQUIRE(metered_session_issue(descriptor, owner, run, &next, prior.grant_id, changed, LXP_ERR_AUTH_SCOPE) == 0);
    invalid = next; invalid.fee_budget.maximum_total.lo++;
    REQUIRE(metered_session_issue(descriptor, owner, run, &invalid, prior.grant_id, commitment, LXP_ERR_AUTH_SCOPE) == 0);
    REQUIRE(metered_session_read(descriptor, prior.grant_id, &prior, successor, changed) == 0);
    REQUIRE(lxp_ct_is_zero(successor, 32U) && memcmp(commitment, changed, 32U) == 0);
    REQUIRE(metered_session_issue(descriptor, owner, run, &next, prior.grant_id, commitment, LXP_OK) == 0);
    (void)memcpy(run->replacement_id, next.grant_id, 32U);
    REQUIRE(metered_session_read(descriptor, next.grant_id, &next, successor, changed) == 0);
    REQUIRE(lxp_u128_cmp(next.fee_budget.spent_total, prior.fee_budget.spent_total) == 0 &&
        lxp_u128_cmp(next.fee_budget.spent_this_period, prior.fee_budget.spent_this_period) == 0 &&
        next.fee_budget.period_start == prior.fee_budget.period_start);
    REQUIRE(metered_execute(descriptor, &replacement, run, LX_PROGRAMS_CALL, payload, payload_length, LXP_OK, &receipt) == 0);
    lxp_u128 charged;
    REQUIRE(lxp_u128_add(prior.fee_budget.spent_total, receipt.fee_charged, &charged) == LXP_OK);
    REQUIRE(metered_session_read(descriptor, next.grant_id, &next, successor, changed) == 0);
    REQUIRE(lxp_u128_cmp(next.fee_budget.spent_total, charged) == 0);
    run->replacement_spent = charged;
    REQUIRE(signer_init(&replacement, 0x56U) == 0);
    invalid = next;
    (void)memcpy(invalid.key, replacement.public_key, 32U);
    invalid.fee_budget.spent_total = (lxp_u128){0U, 0U};
    invalid.fee_budget.spent_this_period = (lxp_u128){0U, 0U};
    invalid.fee_budget.period_start = invalid.not_before;
    REQUIRE(metered_session_issue(descriptor, owner, run, &invalid, prior.grant_id, commitment, LXP_ERR_SEQUENCE_REUSED) == 0);
    REQUIRE(metered_session_read(descriptor, prior.grant_id, &prior, successor, changed) == 0);
    REQUIRE(memcmp(successor, run->replacement_id, 32U) == 0 && memcmp(commitment, changed, 32U) == 0);
    puts("session replacement inherits exact committed fee counters; changed commitment, widened budget and second successor refuse");
    return 0;
}

static int metered_fee_policy(int descriptor, const metered_run *run)
{
    const uint8_t query[] = {0U, 1U, 3U};
    wire_envelope response;
    lx_asset_record record;
    REQUIRE(send_request(descriptor, LNI_MINOR, 32U, 634U, query, sizeof(query)) == 0);
    REQUIRE(receive_envelope(descriptor, &response) == 0);
    REQUIRE(response.tag == 33U && response.correlation_id == 634U && response.proof_length == 0U &&
        response.payload_length > 47U && load_u16(response.payload) == 1U &&
        load_u16(response.payload + 42U) == 1U && response.payload[44U] == 2U &&
        memcmp(response.payload + 10U, run->root, 32U) == 0);
    size_t length = load_u16(response.payload + 45U);
    REQUIRE(response.payload_length == 47U + length);
    REQUIRE(lx_asset_record_decode(response.payload + 47U, length, &record) == LXP_OK);
    REQUIRE(memcmp(record.asset_id, metered_asset, 32U) == 0 && record.decimals <= 38U && record.symbol_length != 0U);
    release_envelope(&response);
    return 0;
}

static int metered_initial(int descriptor, const signer *owner, metered_run *run)
{
    uint8_t encoded[ACTIVITY_CAPACITY], payload[1024], wasm[512], destination[32];
    size_t length, wasm_length;
    lxp_receipt receipt;
    signer capability, budget, mismatched;
    const char *credit_path = getenv("LAYERX_TEST_WITHDRAW_CREDIT");
    FILE *credit;
    REQUIRE(credit_path != NULL && (credit = fopen(credit_path, "rb")) != NULL);
    length = fread(encoded, 1U, sizeof(encoded), credit);
    REQUIRE(length != 0U && length < sizeof(encoded) && !ferror(credit) && fclose(credit) == 0);
    REQUIRE(metered_submit(descriptor, run, encoded, length, LXP_OK) == 0);
    REQUIRE(metered_receipt(descriptor, run->receipt_ids[0], LXP_OK, &receipt) == 0);
    REQUIRE(metered_head(descriptor, receipt.global_sequence + 1U, run->root) == 0);
    (void)memset(payload, 0, sizeof(payload));
    payload[0] = 0x71U; payload[1] = 1U; payload[3] = 2U;
    REQUIRE(lxp_did_id_derive(REGISTERED_DID, sizeof(REGISTERED_DID) - 1U, payload + 4U) == LXP_OK);
    (void)memcpy(payload + 36U, owner->public_key, 32U);
    REQUIRE(metered_execute(descriptor, owner, run, 0x00070001U, payload, 68U, LXP_OK, &receipt) == 0);
    run->generation = receipt.global_sequence;
    REQUIRE(metered_fee_policy(descriptor, run) == 0);
    REQUIRE(metered_issue(descriptor, owner, run, 0x44U, LXP_AUTHORITY_DELEGATED_CAPABILITY, false) == 0);
    REQUIRE(metered_issue(descriptor, owner, run, 0x45U, LXP_AUTHORITY_BUDGET_ALLOWANCE, false) == 0);
    REQUIRE(metered_issue(descriptor, owner, run, 0x46U, LXP_AUTHORITY_DELEGATED_CAPABILITY, true) == 0);
    REQUIRE(lx_account_id_from_string((const uint8_t *)"system:fees", 11U, destination) == LXP_OK);
    wasm_length = metered_wasm(wasm, destination);
    (void)memset(payload, 0, sizeof(payload));
    (void)memcpy(payload, metered_program, 32U);
    store_u16(payload + 32U, LX_PROGRAMS_ACCOUNT_ABI_VERSION);
    payload[34U] = 1U;
    REQUIRE(lxp_did_id_derive(REGISTERED_DID, sizeof(REGISTERED_DID) - 1U, payload + 36U) == LXP_OK);
    REQUIRE(lxp_hash_sha256(wasm, wasm_length, payload + 68U) == LXP_OK);
    store_u32(payload + 100U, (uint32_t)wasm_length);
    (void)memcpy(payload + 104U, wasm, wasm_length);
    REQUIRE(metered_execute(descriptor, owner, run, LX_PROGRAMS_DEPLOY, payload,
                             104U + wasm_length, LXP_OK, &receipt) == 0);
    length = metered_call(payload, destination);
    REQUIRE(signer_init(&capability, 0x44U) == 0 && signer_init(&budget, 0x45U) == 0 &&
            signer_init(&mismatched, 0x46U) == 0);
    {
        uint8_t truncated[512];
        (void)memcpy(truncated, payload, length);
        store_u16(truncated + 34U, 10U);
        (void)memmove(truncated + 116U, truncated + 117U, length - 117U);
        REQUIRE(metered_simulate(descriptor, &capability, run, truncated, length - 1U,
                                  LXP_ERR_NON_CANONICAL) == 0);
    }
    REQUIRE(metered_simulate(descriptor, &capability, run, payload, length, LXP_OK) == 0);
    size_t first = run->receipt_count;
    for (size_t i = 0U; i < 2U; ++i) {
        size_t encoded_length;
        (void)fprintf(stderr, "metered queued ProgramCall index=%zu account_sequence=%llu\n",
            i, (unsigned long long)run->account_sequence);
        REQUIRE(metered_encode(&capability, run->account_sequence, LX_PROGRAMS_CALL,
                                0U, payload, length, encoded, &encoded_length) == 0);
        REQUIRE(metered_submit(descriptor, run, encoded, encoded_length, LXP_OK) == 0);
    }
    for (size_t i = first; i < run->receipt_count; ++i) {
        REQUIRE(metered_receipt(descriptor, run->receipt_ids[i], LXP_OK, &receipt) == 0);
        REQUIRE(receipt.program_outcome.present);
    }
    REQUIRE(metered_head(descriptor, receipt.global_sequence + 1U, run->root) == 0);
    REQUIRE(metered_execute(descriptor, &capability, run, LX_PROGRAMS_CALL, payload,
                             length, LXP_ERR_PROGRAM_REFUSED, &receipt) == 0);
    REQUIRE(metered_execute(descriptor, &mismatched, run, LX_PROGRAMS_CALL, payload,
                             length, LXP_ERR_PROGRAM_REFUSED, &receipt) == 0);
    REQUIRE(metered_execute(descriptor, &budget, run, LX_PROGRAMS_CALL, payload,
                             length, LXP_OK, &receipt) == 0);
    REQUIRE(metered_execute(descriptor, &budget, run, LX_PROGRAMS_CALL, payload,
                             length, LXP_ERR_PROGRAM_REFUSED, &receipt) == 0);
    REQUIRE(metered_issue(descriptor, owner, run, 0x47U, LXP_AUTHORITY_DELEGATED_CAPABILITY, true) == 0);
    REQUIRE(metered_issue(descriptor, owner, run, 0x48U, LXP_AUTHORITY_DELEGATED_CAPABILITY, true) == 0);
    REQUIRE(metered_issue(descriptor, owner, run, 0x49U, LXP_AUTHORITY_DELEGATED_CAPABILITY, false) == 0);
    for (uint8_t seed = 0x47U; seed <= 0x48U; ++seed) {
        signer fee_delegate;
        REQUIRE(signer_init(&fee_delegate, seed) == 0);
        REQUIRE(metered_execute(descriptor, &fee_delegate, run, LX_PROGRAMS_CALL,
            payload, length, LXP_ERR_PROGRAM_REFUSED, &receipt) == 0);
        REQUIRE(!lxp_u128_is_zero(receipt.fee_charged));
    }
    REQUIRE(metered_fee_refusals(descriptor, run, payload, length) == 0);
    REQUIRE(run->receipt_count == 17U && run->account_sequence == 17U);
    REQUIRE(metered_sessions_initial(descriptor, owner, run, payload, length) == 0);
    puts("live signed capability and budget grants charge Programs transfers; simulation, repeated spending, exhaustion and asset mismatch verified");
    return 0;
}

static int metered_recovered(int descriptor, const signer *owner, metered_run *run)
{
    uint8_t root[32], destination[32], payload[512];
    lxp_receipt receipt;
    signer capability, budget;
    REQUIRE(run->receipt_count == 23U && run->account_sequence == 23U);
    REQUIRE(metered_head(descriptor, 0U, root) == 0);
    REQUIRE(memcmp(root, run->root, 32U) == 0);
    for (size_t i = 0U; i < run->receipt_count; ++i)
        REQUIRE(metered_receipt(descriptor, run->receipt_ids[i], run->results[i], &receipt) == 0);
    REQUIRE(lx_account_id_from_string((const uint8_t *)"system:fees", 11U, destination) == LXP_OK);
    size_t length = metered_call(payload, destination);
    REQUIRE(signer_init(&capability, 0x44U) == 0 && signer_init(&budget, 0x45U) == 0);
    REQUIRE(metered_execute(descriptor, &capability, run, LX_PROGRAMS_CALL, payload,
                             length, LXP_ERR_PROGRAM_REFUSED, &receipt) == 0);
    REQUIRE(metered_execute(descriptor, &budget, run, LX_PROGRAMS_CALL, payload,
                             length, LXP_ERR_PROGRAM_REFUSED, &receipt) == 0);
    REQUIRE(metered_fee_refusals(descriptor, run, payload, length) == 0);
    REQUIRE(metered_session_replace(descriptor, owner, run, payload, length) == 0);
    REQUIRE(run->receipt_count == 32U && run->account_sequence == 32U);
    puts("daemon and authority replica replay preserve authenticated roots, receipts and exhausted capability and budget scopes");
    return 0;
}

static int metered_session_replayed(int descriptor, const metered_run *run)
{
    uint8_t root[32], successor[32], commitment[32];
    lxp_receipt receipt;
    lxp_authority_grant original, replacement;
    REQUIRE(run->receipt_count == 32U && run->account_sequence == 32U);
    REQUIRE(metered_head(descriptor, 0U, root) == 0 && memcmp(root, run->root, 32U) == 0);
    for (size_t i = 0U; i < run->receipt_count; ++i)
        REQUIRE(metered_receipt(descriptor, run->receipt_ids[i], run->results[i], &receipt) == 0);
    REQUIRE(metered_session_read(descriptor, run->session_ids[1], &original, successor, commitment) == 0);
    REQUIRE(original.revoked && memcmp(successor, run->replacement_id, 32U) == 0 && !lxp_ct_is_zero(commitment, 32U));
    REQUIRE(metered_session_read(descriptor, run->replacement_id, &replacement, successor, commitment) == 0);
    REQUIRE(!replacement.revoked && lxp_ct_is_zero(successor, 32U) && lxp_ct_is_zero(commitment, 32U) &&
        lxp_u128_cmp(replacement.fee_budget.spent_total, run->replacement_spent) == 0 &&
        lxp_u128_cmp(replacement.fee_budget.spent_this_period, run->replacement_spent) == 0 &&
        replacement.fee_budget.period_start == original.fee_budget.period_start &&
        lxp_u128_cmp(replacement.fee_budget.maximum_total, original.fee_budget.maximum_total) == 0 &&
        lxp_u128_cmp(replacement.fee_budget.maximum_per_period, original.fee_budget.maximum_per_period) == 0);
    puts("second daemon and authority replica restart preserves exact signed replacement history and inherited fee counters");
    return 0;
}

int main(int argc, char **argv)
{
    struct sockaddr_un address = {0};
    metered_run run = {0};
    signer owner;
    FILE *state;
    REQUIRE(argc == 4 && strlen(argv[1]) < sizeof(address.sun_path));
    metered_state_path = argv[3];
    bool replacement_replay = strcmp(argv[2], "--metered-session-recovered") == 0;
    bool recovered = strcmp(argv[2], "--metered-allowance-recovered") == 0;
    REQUIRE(recovered || replacement_replay || strcmp(argv[2], "--metered-allowance") == 0);
    REQUIRE(metered_identity(&owner) == 0);
    address.sun_family = AF_UNIX;
    (void)memcpy(address.sun_path, argv[1], strlen(argv[1]) + 1U);
    int descriptor = socket(AF_UNIX, SOCK_STREAM, 0);
    REQUIRE(descriptor >= 0 && connect(descriptor, (struct sockaddr *)&address, sizeof(address)) == 0);
    REQUIRE(handshake(descriptor) == 0);
    if (recovered || replacement_replay) {
        state = fopen(argv[3], "rb");
        REQUIRE(state != NULL && fread(&run, sizeof(run), 1U, state) == 1U &&
                fgetc(state) == EOF && !ferror(state) && fclose(state) == 0);
        if (replacement_replay) {
            REQUIRE(metered_session_replayed(descriptor, &run) == 0);
            REQUIRE(close(descriptor) == 0);
            return 0;
        }
        REQUIRE(metered_recovered(descriptor, &owner, &run) == 0);
        char final_state[4096];
        int final_length = snprintf(final_state, sizeof(final_state), "%s.session-replacement", argv[3]);
        REQUIRE(final_length > 0 && (size_t)final_length < sizeof(final_state));
        state = fopen(final_state, "wbx");
        REQUIRE(state != NULL && fwrite(&run, sizeof(run), 1U, state) == 1U &&
            fflush(state) == 0 && fsync(fileno(state)) == 0 && fclose(state) == 0);
    } else {
        uint8_t preparation[79];
        store_u16(preparation, 1U); store_u16(preparation + 2U, 75U);
        (void)memcpy(preparation + 4U, REGISTERED_DID, 75U);
        REQUIRE(send_request(descriptor, LNI_MINOR, 26U, 0U,
                             preparation, sizeof(preparation)) == 0);
        REQUIRE(expect_error(descriptor, 0U, 1U, LXP_ERR_MALFORMED_ENVELOPE) == 0);
        REQUIRE(metered_initial(descriptor, &owner, &run) == 0);
        state = fopen(argv[3], "wbx");
        REQUIRE(state != NULL && fwrite(&run, sizeof(run), 1U, state) == 1U &&
                fflush(state) == 0 && fsync(fileno(state)) == 0 && fclose(state) == 0);
    }
    REQUIRE(close(descriptor) == 0);
    return 0;
}
