#ifndef LAYERX_LXP_DAEMON_H
#define LAYERX_LXP_DAEMON_H

#include "layerx/lxp_result.h"
#include "layerx/lxp_activity.h"
#include "layerx/lxp_batch.h"
#include "layerx/lxp_history.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_merkle.h"
#include "layerx/programs.h"

#include <pthread.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct lxp_daemon lxp_daemon;

enum {
    LXP_DAEMON_MAX_WORKERS = 16,
    LXP_DAEMON_MAX_BATCH_ACTIVITIES = 64,
    LXP_DAEMON_QUEUE_CAPACITY = 4096,
    LXP_DAEMON_QUEUE_MAX_BYTES = 64 * LXP_MAX_ACTIVITY_BYTES,
    LXP_DAEMON_AUTHORITY_CACHE_RECEIPTS = 256,
    LXP_DAEMON_BEARER_MAX_BYTES = 128,
    LXP_DAEMON_PROTOCOL_MAX_CONNECTIONS = 4,
    LXP_DAEMON_PROTOCOL_SCRATCH_MIN_BYTES = 48 * 1024 * 1024,
    LXP_DAEMON_LNI_MAX_FRAME_BYTES = LXP_MAX_ACTIVITY_BYTES + 22 + 32,
    LXP_DAEMON_LNI_SOCKET_PATH_BYTES = 108
};

typedef struct lxp_daemon_receipt_authority_entry {
    uint8_t receipt_digest[32];
    uint8_t batch_id[32];
    uint64_t global_sequence;
    uint64_t record_offset;
    uint32_t body_length;
} lxp_daemon_receipt_authority_entry;

typedef struct lxp_daemon_receipt_authority_store {
    lxp_log *log;
    lxp_sequencer_authorization authorization;
    lxp_daemon_receipt_authority_entry
        cache[LXP_DAEMON_AUTHORITY_CACHE_RECEIPTS];
    size_t cache_count;
    size_t cache_next;
    uint64_t record_count;
    uint64_t last_global_sequence;
    uint64_t last_batch_number;
    uint64_t active_batch_last_sequence;
    uint8_t active_canonical_header[LXP_BATCH_HEADER_ENCODED_SIZE];
    uint8_t active_header_signature[64];
    uint64_t replay_offset;
} lxp_daemon_receipt_authority_store;

typedef struct lxp_daemon_receipt_evidence {
    lxp_byte_span canonical_receipt;
    lxp_byte_span canonical_header;
    uint8_t header_signature[64];
    lxp_merkle_proof receipt_proof;
    uint8_t receipt_digest[32];
    uint8_t batch_id[32];
    uint64_t global_sequence;
} lxp_daemon_receipt_evidence;

typedef struct lxp_daemon_protocol_owner {
    lxp_kernel *kernel;
    lx_programs_transfer_runtime *programs_runtime;
    lxp_history *history;
    lxp_verified_receipt_index *verified_receipts;
    lxp_daemon_receipt_authority_store *receipt_authority;
    lxp_arena *scratch;
    lx_programs_state_feed_store feed_store;
    uint8_t bearer_token[LXP_DAEMON_BEARER_MAX_BYTES];
    size_t bearer_token_length;
    pthread_mutex_t mutex;
    pthread_cond_t listener_changed;
    pthread_t listener_thread;
    int listener_connections[LXP_DAEMON_PROTOCOL_MAX_CONNECTIONS];
    size_t listener_active_connections;
    int listener_descriptor;
    uint16_t listener_port;
    lxp_result listener_failure;
    bool listener_started;
    bool listener_stopping;
    bool attached;
} lxp_daemon_protocol_owner;

typedef struct lxp_daemon_protocol_response {
    uint16_t status;
    lxp_byte_span body;
} lxp_daemon_protocol_response;

typedef struct lxp_daemon_lni_configuration {
    const char *socket_path;
    uint32_t allowed_peer_uid;
    uint32_t allowed_peer_gid;
    uint32_t frame_bytes;
    uint32_t deadline_milliseconds;
    uint32_t socket_mode;
} lxp_daemon_lni_configuration;

typedef struct lxp_daemon_lni_server {
    lxp_daemon *daemon;
    lxp_daemon_protocol_owner *owner;
    char socket_path[LXP_DAEMON_LNI_SOCKET_PATH_BYTES];
    char parent_path[LXP_DAEMON_LNI_SOCKET_PATH_BYTES];
    uint32_t allowed_peer_uid;
    uint32_t allowed_peer_gid;
    uint32_t frame_bytes;
    uint32_t deadline_milliseconds;
    pthread_t thread;
    pthread_mutex_t mutex;
    int listener_descriptor;
    int connection_descriptor;
    int parent_descriptor;
    int lifetime_lock_descriptor;
    uint64_t parent_device;
    uint64_t parent_inode;
    uint64_t socket_device;
    uint64_t socket_inode;
    uint64_t lifetime_lock_device;
    uint64_t lifetime_lock_inode;
    lxp_result failure;
    bool started;
    bool stopping;
    bool mutex_initialized;
} lxp_daemon_lni_server;

typedef lxp_result (*lxp_daemon_protocol_replay_fn)(
    void *context, lxp_daemon_protocol_owner *owner);

lxp_result lxp_daemon_receipt_authority_open(
    lxp_daemon_receipt_authority_store *store, lxp_log *log,
    const lxp_sequencer_authorization *authorization);
lxp_result lxp_daemon_receipt_authority_append(
    lxp_daemon_receipt_authority_store *store,
    const uint8_t *canonical_receipt, size_t receipt_length,
    const uint8_t *canonical_header, size_t header_length,
    const uint8_t header_signature[64],
    const lxp_merkle_proof *receipt_proof, lxp_arena *arena);
lxp_result lxp_daemon_receipt_authority_lookup(
    const lxp_daemon_receipt_authority_store *store,
    const uint8_t receipt_digest[32], lxp_arena *arena,
    lxp_daemon_receipt_evidence *evidence);
lxp_result lxp_daemon_receipt_authority_scan(
    const lxp_daemon_receipt_authority_store *store, uint64_t *record_offset,
    lxp_arena *arena, lxp_daemon_receipt_evidence *evidence,
    bool *present);
lxp_result lxp_daemon_protocol_owner_attach(
    lxp_daemon_protocol_owner *owner, lxp_kernel *kernel,
    lx_programs_transfer_runtime *programs_runtime, lxp_log *feed_log,
    lxp_log *canonical_log, lxp_history *history,
    lxp_verified_receipt_index *verified_receipts,
    lxp_daemon_receipt_authority_store *receipt_authority,
    lxp_arena *scratch, lxp_daemon_protocol_replay_fn replay,
    void *replay_context, const uint8_t *bearer_token,
    size_t bearer_token_length);
lxp_result lxp_daemon_protocol_owner_detach(
    lxp_daemon_protocol_owner *owner);
lxp_result lxp_daemon_protocol_publish_receipt(
    lxp_daemon_protocol_owner *owner,
    const uint8_t *canonical_receipt, size_t receipt_length,
    const uint8_t *canonical_header, size_t header_length,
    const uint8_t header_signature[64],
    const lxp_merkle_proof *receipt_proof);
lxp_result lxp_daemon_protocol_route(
    lxp_daemon_protocol_owner *owner, const uint8_t *bearer_token,
    size_t bearer_token_length, const char *method, const char *path,
    lxp_arena *response_arena, lxp_daemon_protocol_response *response);
lxp_result lxp_daemon_protocol_listener_start(
    lxp_daemon_protocol_owner *owner, const char *loopback_address,
    uint16_t port);
lxp_result lxp_daemon_protocol_listener_stop(
    lxp_daemon_protocol_owner *owner);
lxp_result lxp_daemon_lni_serve(
    lxp_daemon_lni_server *server, lxp_daemon *daemon,
    lxp_daemon_protocol_owner *owner,
    const lxp_daemon_lni_configuration *configuration);
lxp_result lxp_daemon_lni_stop(lxp_daemon_lni_server *server);
lxp_result lxp_daemon_lni_status(lxp_daemon_lni_server *server);

typedef enum lxp_daemon_role_kind {
    LXP_DAEMON_SEQUENCER = 1,
    LXP_DAEMON_REPLICA = 2,
    LXP_DAEMON_GUARANTOR = 3
} lxp_daemon_role_kind;

typedef struct lxp_daemon_configuration {
    lxp_daemon_role_kind role;
    uint32_t network_id;
    uint64_t start_sequence;
    size_t verify_workers;
    size_t network_workers;
    size_t projection_workers;
    size_t checkpoint_workers;
    bool serial_execution;
} lxp_daemon_configuration;

typedef struct lxp_daemon_activity {
    uint8_t *bytes;
    size_t length;
} lxp_daemon_activity;

typedef lxp_result (*lxp_daemon_apply_fn)(
    void *context, uint64_t global_sequence,
    const uint8_t *activity, size_t activity_length);

typedef lxp_result (*lxp_daemon_apply_batch_fn)(
    void *context, uint64_t first_global_sequence,
    const lxp_daemon_activity *activities, size_t offered_count,
    size_t *consumed_count);

struct lxp_daemon {
    lxp_daemon_configuration config;
    lxp_daemon_apply_fn apply;
    lxp_daemon_apply_batch_fn apply_batch;
    void *apply_context;
    lxp_daemon_protocol_owner *protocol_owner;
    pthread_t executor_thread;
    pthread_t workers[LXP_DAEMON_MAX_WORKERS * 4U];
    size_t worker_count;
    pthread_mutex_t mutex;
    pthread_cond_t queue_changed;
    lxp_daemon_activity queue[LXP_DAEMON_QUEUE_CAPACITY];
    size_t queue_head;
    size_t queue_count;
    size_t queue_bytes;
    uint64_t next_sequence;
    lxp_result failure;
    bool accepting;
    bool stop_requested;
    bool executor_started;
    bool primitives_initialized;
};

lxp_result lxp_daemon_config_load(
    const char *path, lxp_daemon_configuration *config);
lxp_result lxp_daemon_config(
    const char *path, lxp_daemon_configuration *config);
lxp_result lxp_daemon_role(
    const lxp_daemon_configuration *config,
    lxp_daemon_role_kind *role);
lxp_result lxp_daemon_start(
    lxp_daemon *daemon, const lxp_daemon_configuration *config,
    lxp_daemon_apply_fn apply, void *apply_context);
lxp_result lxp_daemon_start_batch(
    lxp_daemon *daemon, const lxp_daemon_configuration *config,
    lxp_daemon_apply_batch_fn apply_batch, void *apply_context);
lxp_result lxp_daemon_start_protocol(
    lxp_daemon *daemon, const lxp_daemon_configuration *config,
    lxp_daemon_apply_fn apply, void *apply_context,
    lxp_daemon_protocol_owner *protocol_owner,
    const char *loopback_address, uint16_t port);
lxp_result lxp_daemon_start_protocol_batch(
    lxp_daemon *daemon, const lxp_daemon_configuration *config,
    lxp_daemon_apply_batch_fn apply_batch, void *apply_context,
    lxp_daemon_protocol_owner *protocol_owner,
    const char *loopback_address, uint16_t port);
lxp_result lxp_daemon_submit(
    lxp_daemon *daemon, const uint8_t *activity, size_t activity_length);
lxp_result lxp_daemon_shutdown(lxp_daemon *daemon);
lxp_result lxp_daemon_main(int argc, char **argv);
lxp_result lxp_daemon_serve(const char *configuration_path);
lxp_result lxp_daemon_authority_replica_serve(
    const char *configuration_path);
lxp_result lxp_daemon_authority_replica_publish(
    const char *loopback_address, uint16_t port,
    const uint8_t *bearer_token, size_t bearer_token_length,
    const uint8_t expected_replica_id[32],
    const uint8_t *canonical_receipt, size_t receipt_length,
    const uint8_t *canonical_header, size_t header_length,
    const uint8_t header_signature[64],
    const lxp_merkle_proof *receipt_proof);

#endif
