#define _POSIX_C_SOURCE 200809L
#include "lxp_daemon_finality_authority.h"
#include "layerx/lxp_crypto.h"
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <unistd.h>

#define FAIL() do { (void)fprintf(stderr, "fixture failure at line %d\n", __LINE__); return 1; } while (0)

static uint8_t memory[512U * 1024U];
static lxp_daemon_evidence_store store;
static lxp_guarantor_cert certificate;
static lxp_guarantor_set bonded_set;
static lxp_finalisation_requirements requirements;
static lxp_daemon_settlement_registration_evidence registration;

static int log_bootstrap(void)
{
    char directory[] = "/tmp/lxp-bootstrap-log-XXXXXX";
    char path[128];
    const uint8_t body[] = {1U, 2U, 3U};
    uint8_t readback[3];
    lxp_log_record_header header;
    lxp_log log;
    struct stat metadata;
    uint64_t offset;
    unsigned mode;
    if (mkdtemp(directory) == NULL ||
        snprintf(path, sizeof(path), "%s/log", directory) < 0) FAIL();
    for (mode = 0U; mode < 2U; ++mode) {
        if (mode == 1U) {
            int fd = open(path, O_CREAT | O_EXCL | O_RDWR, 0600);
            if (fd < 0 || close(fd) != 0) FAIL();
        }
        if (lxp_log_open_or_create(&log, path, 4096U) != LXP_OK ||
            !log.has_durable_marker || log.capacity == 0U ||
            lxp_log_append(&log, LXP_LOG_ACTIVITY, 1U, body, sizeof(body), &offset) != LXP_OK ||
            lxp_log_append(&log, LXP_LOG_RECEIPT, 1U, body, sizeof(body), NULL) != LXP_OK ||
            lxp_log_sync(&log) != LXP_OK || lxp_log_close(&log) != LXP_OK ||
            lxp_log_open_or_create(&log, path, 16384U) != LXP_OK ||
            fstat(log.descriptor, &metadata) != 0 || metadata.st_size != 4096 ||
            lxp_log_recover(&log, NULL, NULL) != LXP_OK ||
            lxp_log_read(&log, offset, &header, readback, sizeof(readback)) != LXP_OK ||
            memcmp(body, readback, sizeof(body)) != 0 ||
            lxp_log_close(&log) != LXP_OK || unlink(path) != 0)
            FAIL();
    }
    return rmdir(directory) == 0 ? 0 : 1;
}

static int decode(const char *text, uint8_t *out, size_t length)
{
    size_t i;
    if (text == NULL) FAIL();
    if (strncmp(text, "0x", 2U) == 0) text += 2U;
    if (strlen(text) != length * 2U) FAIL();
    for (i = 0U; i < length; ++i) {
        unsigned value;
        if (sscanf(text + i * 2U, "%2x", &value) != 1) FAIL();
        out[i] = (uint8_t)value;
    }
    return 0;
}

static void hex(const uint8_t *bytes, size_t length)
{
    size_t i;
    (void)printf("0x");
    for (i = 0U; i < length; ++i) (void)printf("%02x", bytes[i]);
}

static int fixture(lxp_daemon_finality_authority *authority)
{
    static const char *const public_keys[2] = {
        "0279be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
        "02c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5"
    };
    static const uint8_t proof[] = {'P', 'R', 'O', 'O', 'F'};
    lxp_checkpoint_certificate checkpoint = {0};
    lxp_guarantor_attestation attestations[2];
    lxp_batch_header *header = &checkpoint.header;
    lxp_arena arena;
    size_t i;
    if (lxp_arena_init(&arena, memory, sizeof(memory)) != LXP_OK ||
        lxp_daemon_finality_authority_init(authority, &store) != LXP_OK)
        FAIL();
    header->protocol_version = 2U;
    header->network_id = 42U;
    header->epoch = 1U;
    header->batch_number = 1U;
    header->first_sequence = 1U;
    header->last_sequence = 1000000U;
    header->previous_state_root[0] = 0x11U;
    header->resulting_state_root[0] = 0x22U;
    header->activity_merkle_root[0] = 0x33U;
    header->receipt_merkle_root[0] = 0x44U;
    header->event_merkle_root[0] = 0x55U;
    header->data_availability_root[0] = 0x66U;
    header->oracle_root[0] = 0x77U;
    header->sequencer_id[0] = 0x88U;
    header->timestamp_ms = 1000000U;
    checkpoint.validity_proof = (lxp_byte_span){proof, sizeof(proof)};
    store.network_id = 42U;
    store.initialized = true;
    store.registry.finalisation.settlement_anchor[0] = 0x11U;
    if (lxp_guarantor_set_init(&bonded_set) != LXP_OK) FAIL();
    for (i = 0U; i < 2U; ++i) {
        lxp_guarantor_ctx guarantor = {0};
        lxp_guarantor_bond_state bond = {0};
        guarantor.guarantor_id[31] = (uint8_t)(i + 1U);
        guarantor.paxeer_private_key[31] = (uint8_t)(i + 1U);
        if (decode(public_keys[i], guarantor.paxeer_public_key, 33U) != 0)
            FAIL();
        guarantor.protocol_version = 2U;
        guarantor.network_id = 42U;
        guarantor.paxeer_chain_id = authority->paxeer_chain_id;
        (void)memcpy(guarantor.paxeer_settlement_contract,
                     authority->settlement_contract, 20U);
        guarantor.ready_to_sign = true;
        guarantor.possesses_availability = true;
        guarantor.bond_view.bonded = true;
        (void)memcpy(bond.guarantor_id, guarantor.guarantor_id, 32U);
        (void)memcpy(bond.public_key, guarantor.paxeer_public_key, 33U);
        bond.joined_epoch = 1U;
        bond.active = true;
        if (lxp_guarantor_set_apply(&bonded_set, i * 2U + 1U, true, &bond) != LXP_OK)
            FAIL();
        bond.bond_amount = (lxp_u128){0U, 1000U};
        if (lxp_guarantor_set_apply(&bonded_set, i * 2U + 2U, true, &bond) != LXP_OK ||
            lxp_guarantor_attest(&guarantor, &checkpoint, true, true,
                1001000U, &arena, &attestations[i]) != LXP_OK)
            FAIL();
    }
    if (lxp_guarantor_cert_assemble(&checkpoint, attestations, 2U, 2U,
                                    &certificate) != LXP_OK) FAIL();
    requirements.checkpoint_epoch = 1U;
    requirements.challenge_window_end_ms = 1000100U;
    requirements.checkpoint_deadline_ms = 1002000U;
    requirements.now_ms = 1001500U;
    requirements.threshold = 2U;
    requirements.minimum_bond = (lxp_u128){0U, 500U};
    requirements.availability_challenges_answered = true;
    registration.paxeer_chain_id = authority->paxeer_chain_id;
    (void)memcpy(registration.settlement_contract,
                 authority->settlement_contract, 20U);
    registration.observed_at_ms = 1001500U;
    return lxp_checkpoint_certificate_hash(&checkpoint, &arena,
        registration.checkpoint_id) == LXP_OK ? 0 : 1;
}

static void prepare(void)
{
    const lxp_batch_header *h = &certificate.checkpoint.header;
    const uint8_t *roots[9] = {h->previous_state_root, h->resulting_state_root,
        h->activity_merkle_root, h->receipt_merkle_root, h->event_merkle_root,
        h->data_availability_root, h->oracle_root, h->sequencer_id,
        registration.checkpoint_id};
    size_t i;
    (void)printf("{\"header\":\"(2,42,1,1,1,1000000");
    for (i = 0U; i < 7U; ++i) { (void)printf(","); hex(roots[i], 32U); }
    (void)printf(",1000000,"); hex(roots[7], 32U);
    (void)printf(")\",\"checkpoint_id\":\""); hex(roots[8], 32U);
    (void)printf("\",\"attestations\":\"[");
    for (i = 0U; i < certificate.attestation_count; ++i) {
        const lxp_guarantor_attestation *a = &certificate.attestations[i];
        (void)printf("%s(2,42,%" PRIu64 ",", i == 0U ? "" : ",", a->paxeer_chain_id);
        hex(a->paxeer_settlement_contract, 20U);
        (void)printf(",1,"); hex(a->checkpoint_id, 32U);
        (void)printf(","); hex(a->checkpoint_hash, 32U);
        (void)printf(","); hex(a->guarantor_id, 32U);
        (void)printf(",1,"); hex(a->data_availability_root, 32U);
        (void)printf(",true,true,31,1001000,"); hex(a->signer, 20U);
        (void)printf(","); hex(a->signature, 32U);
        (void)printf(","); hex(a->signature + 32U, 32U);
        (void)printf(",%u)", (unsigned)a->signature_v);
    }
    (void)printf("]\"}\n");
}

static int check(lxp_daemon_finality_authority *authority, const char *name,
                  bool success, bool unavailable)
{
    lxp_finalisation_state before = store.registry.finalisation;
    lxp_result status = lxp_daemon_finality_authority_verify(authority,
        &certificate, &bonded_set, &requirements, &registration);
    if ((success && status != LXP_OK) || (!success && status == LXP_OK) ||
        (unavailable && status != LXP_ERR_IO) ||
        memcmp(&before, &store.registry.finalisation, sizeof(before)) != 0) {
        (void)fprintf(stderr, "%s: unexpected status %d or mutated store\n", name, (int)status);
        FAIL();
    }
    (void)printf("%s passed\n", name);
    return 0;
}

int main(int argc, char **argv)
{
    lxp_daemon_finality_authority authority;
    lxp_daemon_settlement_registration_evidence original;
    int failed = 0;
    if (log_bootstrap() != 0 || fixture(&authority) != 0) FAIL();
    if (argc == 2 && strcmp(argv[1], "prepare") == 0) { prepare(); return 0; }
    if (argc != 6 || strcmp(argv[1], "verify") != 0 ||
        decode(argv[2], registration.transaction_id, 32U) != 0) FAIL();
    registration.observed_block_number = strtoull(argv[3], NULL, 10);
    original = registration;
    failed |= check(&authority, "registered checkpoint", true, false);
    ++registration.paxeer_chain_id;
    failed |= check(&authority, "wrong chain", false, false);
    registration = original;
    registration.settlement_contract[0] ^= 1U;
    failed |= check(&authority, "wrong settlement", false, false);
    registration = original;
    registration.checkpoint_id[0] ^= 1U;
    failed |= check(&authority, "wrong checkpoint", false, false);
    registration = original;
    ++registration.observed_block_number;
    failed |= check(&authority, "wrong block", false, false);
    registration = original;
    certificate.attestations[0].signature[0] ^= 1U;
    failed |= check(&authority, "invalid signature", false, false);
    certificate.attestations[0].signature[0] ^= 1U;
    authority.checkpoint_registry[0] ^= 1U;
    failed |= check(&authority, "wrong registry", false, false);
    authority.checkpoint_registry[0] ^= 1U;
    if (decode(argv[4], registration.transaction_id, 32U) != 0) FAIL();
    registration.observed_block_number = strtoull(argv[5], NULL, 10);
    failed |= check(&authority, "reverted transaction", false, false);
    registration = original;
    authority.rpc_port = 1U;
    failed |= check(&authority, "unreachable chain", false, true);
    return failed;
}
