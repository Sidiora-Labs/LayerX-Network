#define main finality_evidence_fixture_main
#include "../storage/lxp_test_finality_evidence.c"
#undef main
#include "layerx/lxp_state_proof.h"
#include "layerx/programs.h"

#define CHECK(x) do { if (!(x)) { fprintf(stderr, "LNI module evidence line %d\n", __LINE__); return 1; } } while (0)

static test_fixture fixture;
static uint8_t memory[TEST_ARENA_BYTES];

static void print_hex_field(const char *name, lxp_byte_span bytes, bool final)
{
    printf("\"%s\":\"0x", name);
    for (size_t i = 0U; i < bytes.length; ++i) printf("%02x", bytes.bytes[i]);
    printf("\"%s", final ? "" : ",");
}

static int module_read_cases(lxp_daemon_evidence_store *store,
    lxp_daemon_signed_header_evidence *signed_header, lxp_arena *arena, bool output)
{
    lxp_byte_span key = {(const uint8_t *)"sequence", 8U};
    lxp_byte_span value, proof;
    lxp_daemon_signed_header_evidence changed;
    size_t mark = lxp_arena_mark(arena);
    uint8_t absent[32] = {1U};
    CHECK(lxp_daemon_module_evidence_wire_encode(store, &fixture.kernel, signed_header,
        0U, key, 1U, 0U, NULL, 3U, arena, &value, &proof) == LXP_OK);
    CHECK(value.length == 8U && proof.length > 8U && proof.bytes[2] == 4U && proof.bytes[3] == 1U);
    if (output) {
        printf("{");
        print_hex_field("key", key, false);
        print_hex_field("value", value, false);
        print_hex_field("proof", proof, false);
        print_hex_field("sequencer", (lxp_byte_span){store->authorization.public_key, 32U}, false);
        print_hex_field("root", (lxp_byte_span){fixture.kernel.current_state_root, 32U}, true);
        printf("}\n");
    }
    CHECK(lxp_arena_reset(arena, mark) == LXP_OK);
    CHECK(lxp_daemon_module_evidence_wire_encode(store, &fixture.kernel, signed_header,
        0U, key, 2U, TEST_BATCH_NUMBER, NULL, 3U, arena, &value, &proof) == LXP_OK);
    CHECK(proof.bytes[3] == 2U);
    CHECK(lxp_arena_reset(arena, mark) == LXP_OK);
    CHECK(lxp_daemon_module_evidence_wire_encode(store, &fixture.kernel, signed_header,
        0U, key, 2U, TEST_BATCH_NUMBER + 1U, NULL, 3U, arena, &value, &proof) == LXP_ERR_PROJECTION_STALE);
    CHECK(lxp_daemon_module_evidence_wire_encode(store, &fixture.kernel, signed_header,
        0U, key, 1U, 0U, NULL, 4U, arena, &value, &proof) == LXP_ERR_UNKNOWN_FIELD);
    CHECK(lxp_daemon_module_evidence_wire_encode(store, &fixture.kernel, signed_header,
        0U, key, 3U, 0U, absent, 4U, arena, &value, &proof) == LXP_ERR_UNKNOWN_FIELD);
    CHECK(lxp_daemon_module_evidence_wire_encode(store, &fixture.kernel, signed_header,
        0U, key, 1U, 0U, NULL, 5U, arena, &value, &proof) == LXP_ERR_NON_CANONICAL);
    CHECK(lxp_daemon_module_evidence_wire_encode(store, &fixture.kernel, signed_header,
        10U, key, 1U, 0U, NULL, 3U, arena, &value, &proof) == LXP_ERR_NON_CANONICAL);
    CHECK(lxp_daemon_module_evidence_wire_encode(store, &fixture.kernel, signed_header,
        0U, (lxp_byte_span){(const uint8_t *)"missing", 7U}, 1U, 0U, NULL, 3U, arena, &value, &proof) == LXP_ERR_UNKNOWN_FIELD);
    changed = *signed_header;
    changed.signature[0] ^= 1U;
    CHECK(lxp_daemon_module_evidence_wire_encode(store, &fixture.kernel, &changed,
        0U, key, 1U, 0U, NULL, 3U, arena, &value, &proof) != LXP_OK);
    changed = *signed_header;
    changed.authorization.public_key[0] ^= 1U;
    CHECK(lxp_daemon_module_evidence_wire_encode(store, &fixture.kernel, &changed,
        0U, key, 1U, 0U, NULL, 3U, arena, &value, &proof) != LXP_OK);
    fixture.kernel.current_state_root[0] ^= 1U;
    CHECK(lxp_daemon_module_evidence_wire_encode(store, &fixture.kernel, signed_header,
        0U, key, 1U, 0U, NULL, 3U, arena, &value, &proof) == LXP_ERR_PROJECTION_STALE);
    fixture.kernel.current_state_root[0] ^= 1U;
    ++fixture.state.next_sequence;
    CHECK(lxp_daemon_module_evidence_wire_encode(store, &fixture.kernel, signed_header,
        0U, key, 1U, 0U, NULL, 3U, arena, &value, &proof) == LXP_ERR_PROJECTION_STALE);
    --fixture.state.next_sequence;
    return 0;
}

int main(int argc, char **argv)
{
    lxp_arena arena;
    lxp_daemon_evidence_store store;
    lxp_log log = {.descriptor = -1};
    lxp_batch_header header;
    lxp_daemon_signed_header_evidence signed_header;
    lxp_byte_span encoded, value, proof;
    char path[] = "/tmp/lxp-lni-module-XXXXXX";
    int descriptor;
    CHECK(argc == 1 || (argc == 2 && strcmp(argv[1], "--vector") == 0));
    CHECK(lxp_arena_init(&arena, memory, sizeof(memory)) == LXP_OK);
    CHECK(build_account_and_batch(&fixture, &arena, 0x13U) == 0);
    descriptor = mkstemp(path);
    CHECK(descriptor >= 0 && close(descriptor) == 0);
    CHECK(lxp_log_open_or_create(&log, path, TEST_LOG_BYTES) == LXP_OK);
    CHECK(lxp_daemon_evidence_open(&store, &log, TEST_NETWORK_ID, &fixture.authorization,
        fixture.initial_anchor, true, NULL, NULL, &arena) == LXP_OK);
    signed_header.authorization = fixture.authorization;
    signed_header.canonical_header = (lxp_byte_span){fixture.canonical_header, sizeof(fixture.canonical_header)};
    memcpy(signed_header.signature, fixture.header_signature, 64U);
    CHECK(lxp_daemon_module_evidence_wire_encode(&store, &fixture.kernel, &signed_header,
        0U, (lxp_byte_span){(const uint8_t *)"sequence", 8U}, 1U, 0U, NULL, 3U,
        &arena, &value, &proof) == LXP_ERR_VERSION_UNSUPPORTED);
    CHECK(lxp_batch_header_decode(fixture.canonical_header, sizeof(fixture.canonical_header), &header) == LXP_OK);
    header.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    CHECK(lxp_batch_sign(&header, fixture.sequencer_private, &fixture.authorization,
        signed_header.signature, &arena) == LXP_OK);
    CHECK(lxp_batch_header_encode(&header, &arena, &encoded) == LXP_OK);
    signed_header.canonical_header = encoded;
    CHECK(module_read_cases(&store, &signed_header, &arena, argc == 2) == 0);
    CHECK(lxp_log_close(&log) == LXP_OK);
    CHECK(unlink(path) == 0);
    CHECK(lxp_state_store_destroy(&fixture.state) == LXP_OK);
    return 0;
}
