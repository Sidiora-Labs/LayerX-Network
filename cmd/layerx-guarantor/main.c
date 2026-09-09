#define _POSIX_C_SOURCE 200809L
#include "exchange.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_kernel.h"
#include "lni.h"
#include "producer.h"
#include "runtime.h"
#include "settlement.h"
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#define GP_ARENA_BYTES (128U * 1024U * 1024U)
struct producer {
    pthread_mutex_t mutex;
    struct gp_exchange exchange;
    struct gp_exchange_tls tls;
    lxp_checkpoint_certificate checkpoint;
    lxp_guarantor_attestation attestations[LXP_MAX_GUARANTOR_ATTESTATIONS];
    size_t count;
    gp_settlement_config settlement;
    lxp_guarantor_set set;
    size_t threshold;
    uint64_t delay;
    lxp_u128 minimum_bond;
    lxp_sequencer_authorization authority;
    bool prepared;
    char evidence[4096];
    const char *state;
};
static volatile sig_atomic_t stopped;
static void stop(int signum)
{
    (void)signum;
    stopped = 1;
}
static const char *required(const char *name)
{
    const char *value = getenv(name);
    if (value == NULL || value[0] == '\0') {
        fprintf(stderr, "missing configuration: %s\n", name);
        return NULL;
    }
    return value;
}
static int number(const char *text, uint64_t *value)
{
    uint64_t n = 0U;
    if (text == NULL || *text == '\0')
        return -1;
    for (; *text != '\0'; ++text) {
        unsigned d = (unsigned)(*text - '0');
        if (d > 9U || n > (UINT64_MAX - d) / 10U)
            return -1;
        n = n * 10U + d;
    }
    *value = n;
    return 0;
}
static int unhex(const char *text, uint8_t *bytes, size_t length)
{
    if (text == NULL)
        return -1;
    if (strncmp(text, "0x", 2U) == 0)
        text += 2U;
    if (strlen(text) != length * 2U)
        return -1;
    for (size_t i = 0U; i < length; ++i) {
        unsigned value = 0U;
        for (size_t j = 0U; j < 2U; ++j) {
            char c = text[2U * i + j];
            unsigned d = c >= '0' && c <= '9'   ? (unsigned)(c - '0')
                         : c >= 'a' && c <= 'f' ? (unsigned)(c - 'a') + 10U
                         : c >= 'A' && c <= 'F' ? (unsigned)(c - 'A') + 10U
                                                : 16U;
            if (d == 16U)
                return -1;
            value = value * 16U + d;
        }
        bytes[i] = (uint8_t)value;
    }
    return 0;
}
static int path_for(char out[4096], const char *state, uint64_t batch, const uint8_t id[32])
{
    char hex[65];
    int n;
    for (size_t i = 0U; i < 32U; ++i)
        (void)snprintf(hex + i * 2U, 3U, "%02x", id[i]);
    n = snprintf(out, 4096U, "%s/%020llu-%s.attestation", state, (unsigned long long)batch, hex);
    return n >= 0 && n < 4096 ? 0 : -1;
}
static int stored(const char *path, lxp_guarantor_attestation *a)
{
    uint8_t bytes[GP_ATTESTATION_BYTES + 1U];
    size_t used = 0U;
    int fd = open(path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
    if (fd < 0)
        return errno == ENOENT ? 0 : -1;
    while (used < sizeof(bytes)) {
        ssize_t n = read(fd, bytes + used, sizeof(bytes) - used);
        if (n < 0 && errno == EINTR)
            continue;
        if (n < 0) {
            (void)close(fd);
            return -1;
        }
        if (n == 0)
            break;
        used += (size_t)n;
    }
    if (close(fd) != 0 || gp_attestation_decode(bytes, used, a) != LXP_OK)
        return -1;
    return 1;
}
static int id_path(char out[4096], const char *state, const uint8_t id[32], const char *suffix)
{
    char hex[65];
    for (size_t i = 0U; i < 32U; ++i)
        (void)snprintf(hex + 2U * i, 3U, "%02x", id[i]);
    int n = snprintf(out, 4096U, "%s/%s.%s", state, hex, suffix);
    return n >= 0 && n < 4096 ? 0 : -1;
}
static int read_bytes(const char *path, uint8_t *bytes, size_t capacity, size_t *length)
{
    struct stat info;
    int fd = open(path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
    size_t used = 0U;
    if (fd < 0)
        return -1;
    if (fstat(fd, &info) != 0 || !S_ISREG(info.st_mode) || info.st_size <= 0 ||
        (uintmax_t)info.st_size > capacity) {
        (void)close(fd);
        return -1;
    }
    while (used < (size_t)info.st_size) {
        ssize_t n = read(fd, bytes + used, (size_t)info.st_size - used);
        if (n < 0 && errno == EINTR)
            continue;
        if (n <= 0) {
            (void)close(fd);
            return -1;
        }
        used += (size_t)n;
    }
    *length = used;
    return close(fd);
}
static int historical(struct producer *p, const uint8_t id[32], uint8_t *out, size_t capacity,
                      size_t *length)
{
    uint8_t header_bytes[LXP_BATCH_HEADER_ENCODED_SIZE + 64U], digest[32];
    uint8_t *memory = malloc(1024U * 1024U);
    lxp_arena arena;
    lxp_checkpoint_certificate checkpoint = {0};
    lxp_guarantor_set set;
    size_t threshold, size;
    uint64_t delay;
    lxp_u128 minimum;
    char path[4096];
    lxp_result status = LXP_ERR_IO;
    if (memory == NULL)
        return -1;
    if (id_path(path, p->state, id, "header") != 0 ||
        read_bytes(path, header_bytes, sizeof(header_bytes), &size) != 0 ||
        size != sizeof(header_bytes))
        goto finish;
    status = lxp_arena_init(&arena, memory, 1024U * 1024U);
    if (status == LXP_OK)
        status = lxp_batch_header_decode(header_bytes, LXP_BATCH_HEADER_ENCODED_SIZE,
                                         &checkpoint.header);
    if (status == LXP_OK)
        status = lxp_batch_verify_signature(&checkpoint.header,
                                            header_bytes + LXP_BATCH_HEADER_ENCODED_SIZE, 64U,
                                            &p->authority, &arena);
    if (status == LXP_OK)
        status = lxp_checkpoint_certificate_hash(&checkpoint, &arena, digest);
    if (status == LXP_OK && memcmp(digest, id, 32U) != 0)
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK)
        status = gp_settlement_membership(&p->settlement, checkpoint.header.epoch, &set, &threshold,
                                          &delay, &minimum);
    if (status != LXP_OK)
        goto finish;
    if (id_path(path, p->state, id, "attestations") != 0 ||
        read_bytes(path, out, capacity, length) != 0 || *length % GP_ATTESTATION_BYTES != 0U ||
        *length / GP_ATTESTATION_BYTES > LXP_MAX_GUARANTOR_ATTESTATIONS) {
        status = LXP_ERR_IO;
        goto finish;
    }
    for (size_t i = 0U; status == LXP_OK && i < *length; i += GP_ATTESTATION_BYTES) {
        lxp_guarantor_attestation a;
        status = gp_attestation_decode(out + i, GP_ATTESTATION_BYTES, &a);
        if (status == LXP_OK)
            status = gp_attestation_accept(&checkpoint, p->settlement.chain_id,
                                           p->settlement.settlement_contract, &set, &a, NULL, 0U,
                                           p->evidence, &arena);
    }
finish:
    free(memory);
    return status == LXP_OK ? 0 : -1;
}
static int receive_attestation(void *context, const uint8_t *bytes, size_t length)
{
    struct producer *p = context;
    lxp_guarantor_attestation a;
    uint8_t *memory = malloc(1024U * 1024U);
    lxp_arena arena;
    lxp_result status = gp_attestation_decode(bytes, length, &a);
    char path[4096];
    if (memory == NULL)
        return -1;
    if (status == LXP_OK)
        status = lxp_arena_init(&arena, memory, 1024U * 1024U);
    if (pthread_mutex_lock(&p->mutex) != 0) {
        free(memory);
        return -1;
    }
    if (status == LXP_OK && !p->prepared)
        status = LXP_ERR_DA_MISSING;
    if (status == LXP_OK)
        status = gp_settlement_membership(&p->settlement, p->checkpoint.header.epoch, &p->set,
                                          &p->threshold, &p->delay, &p->minimum_bond);
    if (status == LXP_OK)
        status = gp_attestation_accept(&p->checkpoint, p->settlement.chain_id,
                                       p->settlement.settlement_contract, &p->set, &a,
                                       p->attestations, p->count, p->evidence, &arena);
    if (status == LXP_OK) {
        size_t i;
        for (i = 0U; i < p->count; ++i)
            if (memcmp(p->attestations[i].guarantor_id, a.guarantor_id, 32U) == 0)
                break;
        if (i < p->count) {
            uint8_t existing[GP_ATTESTATION_BYTES];
            status = gp_attestation_encode(&p->attestations[i], existing);
            if (status == LXP_OK && memcmp(existing, bytes, length) != 0)
                status = LXP_ERR_CONTEXT_MISMATCH;
        } else if (p->count == LXP_MAX_GUARANTOR_ATTESTATIONS)
            status = LXP_ERR_LENGTH_LIMIT;
        else {
            if (path_for(path, p->state, a.batch_number, a.guarantor_id) != 0)
                status = LXP_ERR_LENGTH_LIMIT;
            if (status == LXP_OK)
                status = gp_file_write(path, bytes, length);
            if (status == LXP_OK)
                p->attestations[p->count++] = a;
        }
    }
    if (status == LXP_OK) {
        uint8_t all[GP_ATTESTATION_BYTES * LXP_MAX_GUARANTOR_ATTESTATIONS];
        for (size_t i = 0U; status == LXP_OK && i < p->count; ++i)
            status = gp_attestation_encode(&p->attestations[i], all + i * GP_ATTESTATION_BYTES);
        if (id_path(path, p->state, a.checkpoint_id, "attestations") != 0)
            status = LXP_ERR_LENGTH_LIMIT;
        if (status == LXP_OK)
            status = gp_file_write(path, all, p->count * GP_ATTESTATION_BYTES);
    }
    (void)pthread_mutex_unlock(&p->mutex);
    free(memory);
    if (status != LXP_OK)
        fprintf(stderr, "peer attestation refused: %d\n", (int)status);
    return status == LXP_OK ? 0 : -1;
}
static int get_attestations(void *context, const uint8_t id[32], uint8_t *out, size_t capacity,
                            size_t *length)
{
    struct producer *p = context;
    int result = -1;
    if (pthread_mutex_lock(&p->mutex) != 0)
        return -1;
    *length = 0U;
    if (p->prepared && p->count != 0U && memcmp(p->attestations[0].checkpoint_id, id, 32U) == 0 &&
        capacity >= p->count * GP_ATTESTATION_BYTES) {
        result = 0;
        for (size_t i = 0U; i < p->count; ++i) {
            if (gp_attestation_encode(&p->attestations[i], out + *length) != LXP_OK)
                result = -1;
            *length += GP_ATTESTATION_BYTES;
        }
    }
    if (result != 0)
        result = historical(p, id, out, capacity, length);
    (void)pthread_mutex_unlock(&p->mutex);
    return result;
}
static void *serve(void *context)
{
    struct producer *p = context;
    while (!stopped)
        (void)gp_exchange_poll(&p->exchange, 250);
    return NULL;
}
static uint64_t milliseconds(void)
{
    struct timespec t;
    if (clock_gettime(CLOCK_REALTIME, &t) != 0 || t.tv_sec < 0)
        return 0U;
    return (uint64_t)t.tv_sec * 1000U + (uint64_t)t.tv_nsec / 1000000U;
}
static int peer_url(const char *url, char host[256], uint16_t *port)
{
    const char *colon;
    uint64_t n;
    size_t length;
    if (url == NULL || strncmp(url, "https://", 8U) != 0)
        return -1;
    url += 8U;
    colon = strrchr(url, ':');
    if (colon == NULL || number(colon + 1U, &n) != 0 || n == 0U || n > UINT16_MAX)
        return -1;
    length = (size_t)(colon - url);
    if (length == 0U || length >= 256U || memchr(url, '/', length) != NULL)
        return -1;
    memcpy(host, url, length);
    host[length] = '\0';
    *port = (uint16_t)n;
    return 0;
}
static lxp_result feedback(lxp_guarantor_lni *client, struct producer *p,
                           const lxp_guarantor_cert *certificate,
                           const lxp_daemon_settlement_registration_evidence *registration,
                           lxp_arena *arena)
{
    lxp_finalisation_requirements requirements = {0};
    lxp_byte_span payload, proof;
    char path[4096];
    int n;
    if (certificate->checkpoint.header.batch_number < p->authority.first_batch_number)
        return LXP_ERR_BATCH_GAP;
    lxp_result status =
        gp_checkpoint_requirements(&certificate->checkpoint.header, registration->observed_at_ms,
                                   p->threshold, p->minimum_bond, &requirements);
    if (status == LXP_OK)
        status = lxp_daemon_finality_evidence_encode(certificate, &p->set, &requirements,
                                                     certificate->checkpoint.header.batch_number -
                                                         p->authority.first_batch_number,
                                                     registration, arena, &payload, &proof);
    n = snprintf(path, sizeof(path), "%s/%020llu.checkpoint", p->state,
                 (unsigned long long)certificate->checkpoint.header.batch_number);
    if (n < 0 || (size_t)n >= sizeof(path))
        return LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK)
        status = gp_file_write(path, payload.bytes, payload.length);
    n = snprintf(path, sizeof(path), "%s/%020llu.finality", p->state,
                 (unsigned long long)certificate->checkpoint.header.batch_number);
    if (n < 0 || (size_t)n >= sizeof(path))
        return LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK)
        status = gp_file_write(path, proof.bytes, proof.length);
    if (status == LXP_OK)
        status = lxp_guarantor_lni_feedback(client, payload, proof, arena);
    if (status == LXP_OK) {
        lxp_byte_span served, served_proof;
        status = lxp_guarantor_lni_checkpoint(client, certificate->checkpoint.header.batch_number,
                                              arena, &served, &served_proof);
        if (status == LXP_OK &&
            (served.length != payload.length || served_proof.length != proof.length ||
             memcmp(served.bytes, payload.bytes, payload.length) != 0 ||
             memcmp(served_proof.bytes, proof.bytes, proof.length) != 0))
            status = LXP_ERR_CONTEXT_MISMATCH;
    }
    return status;
}
int main(int argc, char **argv)
{
    struct producer *p = calloc(1U, sizeof(*p));
    gp_runtime *runtime = NULL;
    lxp_guarantor_ctx ctx = {0};
    lxp_sequencer_authorization authority = {0};
    lxp_guarantor_lni client = {-1, 0U, 30000U};
    lxp_da_store store;
    lxp_arena arena;
    uint8_t *memory = malloc(GP_ARENA_BYTES);
    const char *socket_path, *field = "configuration", *state, *node_config;
    char path[4096], host[256];
    uint16_t peer_port = 0U;
    uint64_t batch = 0U, number_value;
    lxp_result status = LXP_ERR_NON_CANONICAL;
    pthread_t server;
    bool serving = false, mutex_ready = false;
    int lock_fd = -1;
    bool once = argc == 2 && strcmp(argv[1], "--once") == 0;
    bool fetch_only = argc == 2 && strcmp(argv[1], "--fetch-only") == 0;
    if (argc > 2 || (argc == 2 && !once && !fetch_only) || p == NULL || memory == NULL)
        goto done;
    (void)signal(SIGPIPE, SIG_IGN);
    (void)signal(SIGTERM, stop);
    (void)signal(SIGINT, stop);
    state = required("LAYERX_GUARANTOR_STATE_DIR");
    socket_path = required("LAYERX_GUARANTOR_LNI_SOCKET");
    if (state == NULL || socket_path == NULL ||
        number(required("LAYERX_NODE_FIRST_BATCH"), &batch) != 0 || batch == 0U ||
        number(required("LAYERX_NODE_LAST_BATCH"), &authority.last_batch_number) != 0 ||
        unhex(required("LAYERX_NODE_SEQUENCER_ID"), authority.sequencer_id, 32U) != 0 ||
        unhex(required("LAYERX_NODE_SEQUENCER_PUBLIC_KEY"), authority.public_key, 32U) != 0 ||
        number(required("LAYERX_NODE_NETWORK_ID"), &number_value) != 0 || number_value == 0U ||
        number_value > UINT32_MAX)
        goto done;
    ctx.network_id = (uint32_t)number_value;
    authority.first_batch_number = batch;
    authority.authorized = 1U;
    ctx.sequencer_authorization = &authority;
    p->authority = authority;
    p->state = state;
    if (mkdir(state, 0700) != 0 && errno != EEXIST)
        goto done;
    if (snprintf(path, sizeof(path), "%s/producer.lock", state) >= (int)sizeof(path))
        goto done;
    lock_fd = open(path, O_RDWR | O_CREAT | O_NOFOLLOW | O_CLOEXEC, 0600);
    if (lock_fd < 0 || flock(lock_fd, LOCK_EX | LOCK_NB) != 0)
        goto done;
    status = lxp_arena_init(&arena, memory, GP_ARENA_BYTES);
    if (status != LXP_OK)
        goto done;
    if (!fetch_only) {
        status = gp_settlement_config_from_env(&p->settlement, state);
        if (status != LXP_OK)
            goto done;
        ctx.paxeer_chain_id = p->settlement.chain_id;
        memcpy(ctx.paxeer_settlement_contract, p->settlement.settlement_contract, 20U);
        if (unhex(required("LAYERX_GUARANTOR_ID"), ctx.guarantor_id, 32U) != 0) {
            status = LXP_ERR_NON_CANONICAL;
            goto done;
        }
        status = gp_key_load(required("LAYERX_GUARANTOR_KEY_FILE"), &ctx);
        if (status != LXP_OK)
            goto done;
        node_config = required("LAYERX_GUARANTOR_NODE_CONFIG");
        status = gp_runtime_open(&runtime, node_config, state);
        if (status != LXP_OK)
            goto done;
        ctx.replay_engine = gp_runtime_engine(runtime);
        memcpy(ctx.independent_state_root, ctx.replay_engine->kernel->current_state_root, 32U);
        ctx.verify_authority = gp_runtime_authority;
        ctx.authority_context = runtime;
        ctx.verify_oracle = gp_runtime_oracle;
        ctx.oracle_context = runtime;
        if (snprintf(path, sizeof(path), "%s/da", state) >= (int)sizeof(path) ||
            (mkdir(path, 0700) != 0 && errno != EEXIST) ||
            snprintf(p->evidence, sizeof(p->evidence), "%s/evidence", state) >=
                (int)sizeof(p->evidence) ||
            (mkdir(p->evidence, 0700) != 0 && errno != EEXIST)) {
            status = LXP_ERR_IO;
            goto done;
        }
        status = lxp_da_store_init(&store, path);
        if (status != LXP_OK)
            goto done;
        p->tls = (struct gp_exchange_tls){required("LAYERX_GUARANTOR_TLS_CERT_FILE"),
                                          required("LAYERX_GUARANTOR_TLS_KEY_FILE"),
                                          required("LAYERX_GUARANTOR_TLS_CA_FILE")};
        if (number(required("LAYERX_GUARANTOR_LISTEN_PORT"), &number_value) != 0 ||
            number_value == 0U || number_value > UINT16_MAX ||
            peer_url(required("LAYERX_GUARANTOR_PEER_URL"), host, &peer_port) != 0 ||
            pthread_mutex_init(&p->mutex, NULL) != 0) {
            status = LXP_ERR_NON_CANONICAL;
            goto done;
        }
        mutex_ready = true;
        struct gp_exchange_callbacks callbacks = {receive_attestation, get_attestations, p};
        if (gp_exchange_open(&p->exchange, &p->tls, (uint16_t)number_value, &callbacks) != 0 ||
            pthread_create(&server, NULL, serve, p) != 0) {
            status = LXP_ERR_IO;
            goto done;
        }
        serving = true;
    }
    while (!stopped) {
        lxp_batch_header header;
        uint8_t signature[64], encoded[GP_ATTESTATION_BYTES];
        lxp_da_bundle bundle;
        lxp_batch_body body;
        lxp_guarantor_attestation own;
        int cached;
        bool runtime_prepared = false;
        ctx.last_completed_duty = LXP_GUARANTOR_DUTY_NONE;
        (void)lxp_arena_reset(&arena, 0U);
        field = "LNI connection";
        status = lxp_guarantor_lni_open(&client, socket_path, 30000U);
        if (status != LXP_OK)
            goto batch_failed;
        field = "tag12 signed header";
        status = lxp_guarantor_lni_header(&client, batch, &authority, ctx.network_id, &arena,
                                          &header, signature);
        if (status != LXP_OK)
            goto batch_failed;
        field = "tag18 selector05 candidate";
        status = lxp_guarantor_lni_fetch(&client, &header, &arena, &bundle);
        if (status != LXP_OK)
            goto batch_failed;
        lxp_guarantor_lni_close(&client);
        if (fetch_only) {
            fprintf(stdout, "candidate verified batch=%llu chunks=%zu\n", (unsigned long long)batch,
                    bundle.chunk_count);
            break;
        }
        ctx.last_completed_duty = LXP_GUARANTOR_DUTY_NONE;
        ctx.protocol_version = header.protocol_version;
        field = "bonded membership";
        (void)pthread_mutex_lock(&p->mutex);
        p->prepared = false;
        status = gp_settlement_membership(&p->settlement, header.epoch, &p->set, &p->threshold,
                                          &p->delay, &p->minimum_bond);
        (void)pthread_mutex_unlock(&p->mutex);
        if (status != LXP_OK)
            goto batch_failed;
        ctx.bond_view.bonded = false;
        for (size_t i = 0U; i < p->set.count; ++i)
            if (memcmp(p->set.records[i].guarantor_id, ctx.guarantor_id, 32U) == 0 &&
                memcmp(p->set.records[i].public_key, ctx.paxeer_public_key, 33U) == 0)
                ctx.bond_view.bonded = p->set.records[i].active;
        if (!ctx.bond_view.bonded || p->delay != lxp_checkpoint_maximum_attestation_delay_ms()) {
            status = LXP_ERR_ATTESTATION_THRESHOLD;
            goto batch_failed;
        }
        field = "independent replay";
        status = lxp_da_bundle_body(&bundle, &header, &arena, &body);
        if (status == LXP_OK) {
            memcpy(body.sequencer_signature, signature, 64U);
            status = gp_runtime_prepare(runtime, &body);
        }
        if (status != LXP_OK)
            goto batch_failed;
        runtime_prepared = true;
        if (path_for(path, state, batch, ctx.guarantor_id) != 0) {
            status = LXP_ERR_LENGTH_LIMIT;
            goto batch_failed;
        }
        cached = stored(path, &own);
        if (cached < 0) {
            status = LXP_ERR_IO;
            goto batch_failed;
        }
        if (cached == 1)
            status = gp_verify_replay(&ctx, &bundle, &header, signature, &store, &arena, &field);
        else
            status = gp_verify_attest(&ctx, &bundle, &header, signature, &store, milliseconds(),
                                      &arena, &own, &field);
        if (status != LXP_OK)
            goto batch_failed;
        (void)pthread_mutex_lock(&p->mutex);
        p->checkpoint = (lxp_checkpoint_certificate){header, {NULL, 0U}};
        p->count = 0U;
        p->prepared = true;
        (void)pthread_mutex_unlock(&p->mutex);
        lxp_byte_span canonical_header;
        uint8_t saved_header[LXP_BATCH_HEADER_ENCODED_SIZE + 64U];
        status = lxp_batch_header_encode(&header, &arena, &canonical_header);
        if (status == LXP_OK && canonical_header.length != LXP_BATCH_HEADER_ENCODED_SIZE)
            status = LXP_ERR_NON_CANONICAL;
        if (status == LXP_OK) {
            memcpy(saved_header, canonical_header.bytes, canonical_header.length);
            memcpy(saved_header + canonical_header.length, signature, 64U);
            if (id_path(path, state, own.checkpoint_id, "header") != 0)
                status = LXP_ERR_LENGTH_LIMIT;
            else
                status = gp_file_write(path, saved_header, sizeof(saved_header));
        }
        if (status == LXP_OK)
            status = gp_attestation_encode(&own, encoded);
        if (status == LXP_OK && receive_attestation(p, encoded, sizeof(encoded)) != 0)
            status = LXP_ERR_BAD_SIGNATURE;
        if (status != LXP_OK)
            goto batch_failed;
        fprintf(stdout, "attested batch=%llu\n", (unsigned long long)batch);
        (void)fflush(stdout);
        uint64_t peer_deadline = milliseconds() + 30000U;
        while (!stopped) {
            uint8_t remote[GP_EXCHANGE_MAX_BODY];
            size_t length = 0U, count;
            lxp_guarantor_cert certificate;
            lxp_daemon_settlement_registration_evidence registration;
            bool already = false;
            uint64_t registered_version = 0U;
            (void)gp_exchange_peer(&p->tls, host, peer_port, NULL, encoded, sizeof(encoded), remote,
                                   sizeof(remote), &length);
            if (gp_exchange_peer(&p->tls, host, peer_port, own.checkpoint_id, NULL, 0U, remote,
                                 sizeof(remote), &length) == 0 &&
                length % GP_ATTESTATION_BYTES == 0U) {
                for (size_t offset = 0U; offset < length; offset += GP_ATTESTATION_BYTES)
                    (void)receive_attestation(p, remote + offset, GP_ATTESTATION_BYTES);
            }
            (void)pthread_mutex_lock(&p->mutex);
            count = p->count;
            status = count >= p->threshold
                         ? lxp_guarantor_cert_assemble(&p->checkpoint, p->attestations, count,
                                                       p->threshold, &certificate)
                         : LXP_ERR_ATTESTATION_THRESHOLD;
            (void)pthread_mutex_unlock(&p->mutex);
            if (status == LXP_OK) {
                field = "checkpoint registration";
                (void)pthread_mutex_lock(&p->mutex);
                status = gp_settlement_register(&p->settlement, &certificate, runtime, &registration,
                                                &already, &registered_version);
                if (status != LXP_OK) {
                    (void)pthread_mutex_unlock(&p->mutex);
                    goto batch_failed;
                }
                if (registered_version != p->set.version) {
                    status = LXP_ERR_CONTEXT_MISMATCH;
                    (void)pthread_mutex_unlock(&p->mutex);
                    goto batch_failed;
                }
                field = "tag28 feedback";
                status = lxp_guarantor_lni_open(&client, socket_path, 30000U);
                if (status == LXP_OK)
                    status = feedback(&client, p, &certificate, &registration, &arena);
                (void)pthread_mutex_unlock(&p->mutex);
                if (status != LXP_OK)
                    goto batch_failed;
                fprintf(stdout, "%s batch=%llu\n", already ? "observed registration" : "registered",
                        (unsigned long long)batch);
                (void)fflush(stdout);
                break;
            }
            if (once && milliseconds() >= peer_deadline) {
                field = "peer threshold";
                goto batch_failed;
            }
            struct timespec delay = {0, 250000000};
            (void)nanosleep(&delay, NULL);
        }
        lxp_guarantor_lni_close(&client);
        if (once || batch == UINT64_MAX)
            break;
        ++batch;
        continue;
    batch_failed:
        lxp_guarantor_lni_close(&client);
        fprintf(stderr, "refused batch=%llu field=%s result=%d\n", (unsigned long long)batch, field,
                (int)status);
        if (once || fetch_only || runtime_prepared ||
            (ctx.last_completed_duty >= LXP_GUARANTOR_DUTY_SIGNATURES && status != LXP_OK))
            break;
        struct timespec delay = {1, 0};
        (void)nanosleep(&delay, NULL);
    }
done:
    stopped = 1;
    if (serving) {
        (void)pthread_join(server, NULL);
        gp_exchange_close(&p->exchange);
    }
    if (mutex_ready)
        (void)pthread_mutex_destroy(&p->mutex);
    lxp_guarantor_lni_close(&client);
    gp_runtime_close(runtime);
    if (lock_fd >= 0)
        (void)close(lock_fd);
    lxp_secure_zero(ctx.paxeer_private_key, 32U);
    free(memory);
    free(p);
    if (status != LXP_OK)
        fprintf(stderr, "layerx-guarantor: %s refused (%d)\n", field, (int)status);
    return status == LXP_OK ? 0 : 1;
}
