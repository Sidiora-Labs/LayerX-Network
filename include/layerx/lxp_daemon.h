#ifndef LAYERX_LXP_DAEMON_H
#define LAYERX_LXP_DAEMON_H

#include "layerx/lxp_result.h"
#include "layerx/lxp_activity.h"
#include "layerx/lxp_batch.h"
#include "layerx/lxp_history.h"
#include "layerx/lxp_guarantor.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_merkle.h"
#include "layerx/lxp_paxeer.h"
#include "layerx/programs.h"

#include <pthread.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct lxp_daemon lxp_daemon;
typedef struct lxp_daemon_receipt_authority_store
    lxp_daemon_receipt_authority_store;

enum {
    LXP_DAEMON_MAX_WORKERS = 16,
    LXP_DAEMON_MAX_BATCH_ACTIVITIES = 64,
    LXP_DAEMON_QUEUE_CAPACITY = 4096,
    LXP_DAEMON_QUEUE_MAX_BYTES = 64 * LXP_MAX_ACTIVITY_BYTES,
    LXP_DAEMON_AUTHORITY_CACHE_RECEIPTS = 256,
    LXP_DAEMON_BEARER_MAX_BYTES = 128,
    LXP_DAEMON_PROTOCOL_MAX_CONNECTIONS = 4,
    LXP_DAEMON_PROTOCOL_SCRATCH_MIN_BYTES = 48 * 1024 * 1024,
    LXP_DAEMON_FINALITY_REGISTER_MAX_BYTES =
        LXP_MAX_VALIDITY_PROOF_BYTES + 96 * 1024,
    LXP_DAEMON_LNI_MAX_FRAME_BYTES =
        LXP_DAEMON_FINALITY_REGISTER_MAX_BYTES + 64 * 1024,
    LXP_DAEMON_LNI_SOCKET_PATH_BYTES = 108,
    LXP_DAEMON_LNI_ADMISSION_PATH_BYTES = 4096,
    LXP_DAEMON_LNI_MAX_OBSERVED_PEERS = 64
};

typedef enum lxp_daemon_evidence_kind {
    LXP_DAEMON_EVIDENCE_ACCOUNT = 1,
    LXP_DAEMON_EVIDENCE_ACTIVITY = 2,
    LXP_DAEMON_EVIDENCE_FINALITY = 3
} lxp_daemon_evidence_kind;

struct lxp_daemon_settlement_registration_evidence;
typedef lxp_result (*lxp_daemon_finality_authority_verify_fn)(
    void *context, const lxp_guarantor_cert *certificate,
    const lxp_guarantor_set *bonded_set,
    const lxp_finalisation_requirements *requirements,
    const struct lxp_daemon_settlement_registration_evidence *registration);

typedef struct lxp_daemon_evidence_store {
    lxp_log *log;
    lxp_checkpoint_registry_state registry;
    lxp_sequencer_authorization authorization;
    uint32_t network_id;
    uint64_t record_count;
    uint64_t last_ordinal;
    uint64_t latest_finalized_batch;
    uint64_t latest_bonded_set_version;
    uint8_t latest_checkpoint_id[32];
    lxp_daemon_finality_authority_verify_fn verify_finality_authority;
    void *finality_authority_context;
    bool initialized;
} lxp_daemon_evidence_store;

typedef struct lxp_daemon_signed_header_evidence {
    lxp_sequencer_authorization authorization;
    lxp_byte_span canonical_header;
    uint8_t signature[64];
} lxp_daemon_signed_header_evidence;

typedef struct lxp_daemon_account_evidence {
    uint8_t account_id[32];
    uint8_t receipt_digest[32];
    uint64_t observed_sequence;
    uint64_t observed_at_ms;
    uint8_t account_leaf_key[LX_ACCOUNT_STATE_LEAF_KEY_BYTES];
    uint8_t account_leaf_value[LX_ACCOUNT_STATE_LEAF_VALUE_MAX_BYTES];
    size_t account_leaf_value_length;
    uint8_t account_root[32];
    uint8_t universal_root[32];
    uint8_t resulting_state_root[32];
    lxp_state_proof account_proof;
    lxp_state_proof account_tree_proof;
    lxp_state_proof universal_root_proof;
    lxp_byte_span canonical_receipt;
    lxp_merkle_proof receipt_proof;
    lxp_daemon_signed_header_evidence signed_header;
} lxp_daemon_account_evidence;

typedef struct lxp_daemon_activity_evidence {
    uint8_t activity_id[32];
    uint8_t receipt_digest[32];
    uint64_t global_sequence;
    uint64_t batch_number;
    lxp_byte_span canonical_activity;
    lxp_merkle_proof activity_proof;
    lxp_byte_span canonical_receipt;
    lxp_merkle_proof receipt_proof;
    lxp_daemon_signed_header_evidence signed_header;
} lxp_daemon_activity_evidence;

typedef struct lxp_daemon_finality_evidence {
    uint8_t checkpoint_id[32];
    uint8_t resulting_state_root[32];
    uint8_t record_digest[32];
    uint64_t batch_number;
    uint64_t bonded_set_version;
    uint64_t resulting_registration_count;
    lxp_byte_span checkpoint_payload;
    lxp_byte_span finality_proof;
} lxp_daemon_finality_evidence;

typedef struct lxp_daemon_settlement_registration_evidence {
    uint64_t paxeer_chain_id;
    uint8_t settlement_contract[20];
    uint8_t checkpoint_id[32];
    uint8_t transaction_id[32];
    uint64_t observed_block_number;
    uint64_t observed_at_ms;
} lxp_daemon_settlement_registration_evidence;

lxp_result lxp_daemon_evidence_open(
    lxp_daemon_evidence_store *store, lxp_log *log, uint32_t network_id,
    const lxp_sequencer_authorization *authorization,
    const uint8_t initial_settlement_anchor[32], bool allow_initialize,
    lxp_daemon_finality_authority_verify_fn verify_finality_authority,
    void *finality_authority_context, lxp_arena *arena);
lxp_result lxp_daemon_evidence_bind_finality_authority(
    lxp_daemon_evidence_store *store,
    lxp_daemon_finality_authority_verify_fn verify, void *context);
lxp_result lxp_daemon_account_evidence_build(
    const lxp_kernel *kernel, uint32_t network_id,
    const uint8_t account_id[32],
    const uint8_t receipt_digest[32], uint64_t observed_at_ms,
    lxp_byte_span canonical_receipt,
    const lxp_merkle_proof *receipt_proof,
    const lxp_sequencer_authorization *authorization,
    lxp_byte_span canonical_header, const uint8_t header_signature[64],
    lxp_arena *arena, lxp_daemon_account_evidence *evidence);
lxp_result lxp_daemon_account_evidence_publish(
    lxp_daemon_evidence_store *store,
    const lxp_daemon_account_evidence *evidence, lxp_arena *arena,
    uint8_t record_digest[32]);
lxp_result lxp_daemon_account_evidence_publish_batch(
    lxp_daemon_evidence_store *store, const lxp_kernel *kernel,
    lxp_byte_span canonical_head_receipt,
    const lxp_merkle_proof *head_receipt_proof,
    const lxp_sequencer_authorization *authorization,
    lxp_byte_span canonical_header, const uint8_t header_signature[64],
    lxp_arena *arena);
lxp_result lxp_daemon_account_evidence_lookup(
    const lxp_daemon_evidence_store *store, const uint8_t account_id[32],
    const uint8_t resulting_state_root[32], lxp_arena *arena,
    lxp_daemon_account_evidence *evidence);
lxp_result lxp_daemon_account_evidence_lookup_batch(
    const lxp_daemon_evidence_store *store, const uint8_t account_id[32],
    uint64_t batch_number, lxp_arena *arena,
    lxp_daemon_account_evidence *evidence);
lxp_result lxp_daemon_account_evidence_wire_encode(
    const lxp_daemon_evidence_store *store,
    const lxp_daemon_account_evidence *latest_evidence,
    const lxp_kernel *latest_kernel, uint32_t network_id,
    const uint8_t account_id[32], uint8_t selector_kind,
    uint64_t selector_batch,
    const uint8_t selector_checkpoint_id[32],
    lxp_arena *arena, lxp_byte_span *canonical_value,
    lxp_byte_span *proof_material);
lxp_result lxp_daemon_activity_evidence_publish(
    lxp_daemon_evidence_store *store, lxp_byte_span canonical_activity,
    const lxp_merkle_proof *activity_proof,
    lxp_byte_span canonical_receipt,
    const lxp_merkle_proof *receipt_proof,
    const lxp_sequencer_authorization *authorization,
    lxp_byte_span canonical_header, const uint8_t header_signature[64],
    lxp_arena *arena, uint8_t record_digest[32]);
lxp_result lxp_daemon_activity_evidence_lookup(
    const lxp_daemon_evidence_store *store, const uint8_t activity_id[32],
    lxp_arena *arena, lxp_daemon_activity_evidence *evidence);
lxp_result lxp_daemon_activity_evidence_recover_batch(
    lxp_daemon_evidence_store *store, const lxp_log *canonical_log,
    const lxp_daemon_receipt_authority_store *receipt_authority,
    const lxp_sequencer_authorization *authorization,
    lxp_byte_span canonical_header, const uint8_t header_signature[64],
    lxp_arena *arena);
lxp_result lxp_daemon_activity_evidence_wire_encode(
    const lxp_daemon_activity_evidence *evidence, uint32_t network_id,
    uint8_t response_kind, lxp_arena *arena,
    lxp_byte_span *canonical_value, lxp_byte_span *proof_material);
lxp_result lxp_daemon_finality_evidence_register(
    lxp_daemon_evidence_store *store, lxp_byte_span checkpoint_payload,
    lxp_byte_span finality_proof, lxp_arena *arena,
    lxp_daemon_finality_evidence *evidence);
lxp_result lxp_daemon_finality_evidence_encode(
    const lxp_guarantor_cert *certificate,
    const lxp_guarantor_set *bonded_set,
    const lxp_finalisation_requirements *requirements,
    uint64_t expected_registration_count,
    const lxp_daemon_settlement_registration_evidence *registration,
    lxp_arena *arena, lxp_byte_span *checkpoint_payload,
    lxp_byte_span *finality_proof);
lxp_result lxp_daemon_finality_evidence_lookup(
    const lxp_daemon_evidence_store *store,
    const uint8_t checkpoint_id[32], uint64_t batch_number,
    lxp_arena *arena, lxp_daemon_finality_evidence *evidence);

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
    uint64_t last_sealed_timestamp;
    uint64_t active_batch_last_sequence;
    uint8_t active_canonical_header[LXP_BATCH_HEADER_ENCODED_SIZE];
    uint8_t active_header_signature[64];
    uint64_t replay_offset;
} lxp_daemon_receipt_authority_store;

typedef struct lxp_daemon_receipt_evidence {
    uint8_t format_version;
    lxp_byte_span terminal_payload;
    lxp_byte_span call_graph;
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
    lxp_identity_store *identities;
    lx_programs_transfer_runtime *programs_runtime;
    lxp_history *history;
    lxp_verified_receipt_index *verified_receipts;
    lxp_daemon_receipt_authority_store *receipt_authority;
    lxp_daemon_evidence_store *evidence_store;
    lxp_arena *scratch;
    lx_programs_state_feed_store feed_store;
    uint8_t bearer_token[LXP_DAEMON_BEARER_MAX_BYTES];
    size_t bearer_token_length;
    uint32_t network_id;
    uint16_t protocol_version;
    uint64_t latest_sealed_timestamp;
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
    const char *admission_directory;
    uint32_t allowed_peer_uid;
    uint32_t allowed_peer_gid;
    uint32_t frame_bytes;
    uint32_t deadline_milliseconds;
    uint32_t socket_mode;
} lxp_daemon_lni_configuration;

typedef struct lxp_daemon_lni_peer_observation {
    /* A peer is one stable kernel SO_PEERCRED pid/uid/gid triple. */
    uint32_t pid;
    uint32_t uid;
    uint32_t gid;
    uint64_t latest_connection_generation;
    uint64_t authentication_refusals;
    uint32_t active_connections;
    bool active;
} lxp_daemon_lni_peer_observation;

typedef struct lxp_daemon_lni_observability {
    lxp_daemon_lni_peer_observation
        peers[LXP_DAEMON_LNI_MAX_OBSERVED_PEERS];
    size_t peer_count;
    uint64_t evicted_peers;
    uint64_t evicted_authentication_refusals;
} lxp_daemon_lni_observability;

typedef struct lxp_daemon_lni_journal_entry {
    uint8_t activity_id[32];
    uint64_t global_sequence;
    uint64_t file_offset;
    uint32_t activity_length;
} lxp_daemon_lni_journal_entry;

typedef struct lxp_daemon_lni_server {
    lxp_daemon *daemon;
    lxp_daemon_protocol_owner *owner;
    char socket_path[LXP_DAEMON_LNI_SOCKET_PATH_BYTES];
    char parent_path[LXP_DAEMON_LNI_SOCKET_PATH_BYTES];
    char admission_directory[LXP_DAEMON_LNI_ADMISSION_PATH_BYTES];
    uint32_t allowed_peer_uid;
    uint32_t allowed_peer_gid;
    uint32_t frame_bytes;
    uint32_t deadline_milliseconds;
    pthread_t thread;
    pthread_mutex_t mutex;
    int listener_descriptor;
    int connection_descriptor;
    int parent_descriptor;
    int admission_parent_descriptor;
    int lifetime_lock_descriptor;
    uint64_t parent_device;
    uint64_t parent_inode;
    uint64_t admission_parent_device;
    uint64_t admission_parent_inode;
    uint64_t socket_device;
    uint64_t socket_inode;
    uint64_t lifetime_lock_device;
    uint64_t lifetime_lock_inode;
    uint64_t journal_device;
    uint64_t journal_inode;
    uint64_t journal_end;
    uint64_t connection_generation;
    uint64_t expected_admission_sequence;
    uint8_t expected_admission_activity_id[32];
    pthread_t expected_admission_submitter;
    uint64_t evicted_peers;
    uint64_t evicted_authentication_refusals;
    lxp_daemon_lni_journal_entry
        journal_entries[LXP_DAEMON_QUEUE_CAPACITY];
    lxp_daemon_lni_peer_observation
        observed_peers[LXP_DAEMON_LNI_MAX_OBSERVED_PEERS];
    size_t journal_entry_count;
    size_t observed_peer_count;
    size_t observed_peer_next;
    int journal_descriptor;
    lxp_result failure;
    bool started;
    bool stopping;
    bool admission_sequence_expected;
    bool journal_bound;
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
lxp_result lxp_daemon_receipt_authority_append_artifacts(
    lxp_daemon_receipt_authority_store *store,
    const uint8_t *canonical_receipt, size_t receipt_length,
    const uint8_t *canonical_header, size_t header_length,
    const uint8_t header_signature[64],
    const lxp_merkle_proof *receipt_proof, lxp_arena *arena,
    lxp_byte_span terminal_payload, lxp_byte_span call_graph);
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
    lxp_identity_store *identities, uint32_t network_id,
    uint64_t bootstrap_sealed_timestamp,
    lx_programs_transfer_runtime *programs_runtime, lxp_log *feed_log,
    lxp_log *canonical_log, lxp_history *history,
    lxp_verified_receipt_index *verified_receipts,
    lxp_daemon_receipt_authority_store *receipt_authority,
    lxp_arena *scratch, lxp_daemon_protocol_replay_fn replay,
    void *replay_context, const uint8_t *bearer_token,
    size_t bearer_token_length);
lxp_result lxp_daemon_protocol_owner_detach(
    lxp_daemon_protocol_owner *owner);
lxp_result lxp_daemon_protocol_owner_bind_evidence(
    lxp_daemon_protocol_owner *owner,
    lxp_daemon_evidence_store *evidence_store);
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
lxp_result lxp_daemon_lni_observability_snapshot(
    lxp_daemon_lni_server *server,
    lxp_daemon_lni_observability *observability);
lxp_result lxp_daemon_lni_preparation_state(
    lxp_daemon_protocol_owner *owner, const uint8_t *request,
    size_t request_length,
    uint8_t *response, size_t response_capacity,
    size_t *response_length);

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
    uint8_t activity_id[32];
    uint64_t global_sequence;
    bool durable_admission;
} lxp_daemon_activity;

typedef lxp_result (*lxp_daemon_admission_persist_fn)(
    void *context, uint64_t global_sequence,
    const uint8_t activity_id[32],
    const uint8_t *activity, size_t activity_length);

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
    lxp_daemon_admission_persist_fn persist_admission;
    void *persist_admission_context;
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
