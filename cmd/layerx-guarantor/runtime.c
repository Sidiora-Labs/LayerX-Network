#define _POSIX_C_SOURCE 200809L
#include "runtime.h"
#include "../layerxd/lxp_daemon_batch_wal.h"
#include "layerx/lx_asset.h"
#include "layerx/lxp_activity.h"
#include "layerx/lxp_batch_identity.h"
#include "layerx/lxp_bridge_credit.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_fee.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_module_ctx.h"
#include "layerx/lxp_snapshot.h"
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

struct gp_runtime {
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_daemon_configuration configuration;
    lx_account_registry accounts;
    lxp_transfer_asset_state assets[LX_ASSET_REGISTRY_CAPACITY];
    lx_asset_record send_assets[LX_ASSET_REGISTRY_CAPACITY];
    lx_asset_runtime asset_runtime;
    size_t asset_count;
    lx_programs_transfer_runtime programs;
    lxp_identity_store identities;
    lxp_fee_params fees;
    lxp_verified_receipt_index verified_receipts;
    lxp_sequencer_authorization sequencer_authorization;
    lxp_replay_engine engine;
    lxp_arena execution_arena;
    lxp_arena preparation_arena;
    uint8_t *execution_bytes;
    uint8_t *preparation_bytes;
    uint16_t protocol_version;
    uint32_t parameter_version;
    uint32_t network_id;
    bool custody_credit_enabled;
    bool state_open;
    bool mutex_open;
    bool poisoned;
    uint64_t prepared_batch;
    uint64_t prepared_first_sequence;
    uint64_t prepared_last_sequence;
    uint64_t prepared_timestamp;
    uint64_t last_batch;
    lxp_byte_span *published_receipts;
    lxp_byte_span *published_activities;
    size_t receipt_count;
    size_t activity_count;
    lxp_receipt expected;
    lxp_kernel_execution *batch_bindings;
    lx_programs_state_feed feed;
    pthread_mutex_t feed_mutex;
    FILE *feed_file;
    off_t feed_session_start;
    uint64_t feed_sequence;
    uint32_t feed_ordinal;
};
static lxp_result parse_u64_text(const char *text, uint64_t *value)
{
    uint64_t parsed = 0;
    if (!text || !*text || !value)
        return LXP_ERR_NON_CANONICAL;
    for (const unsigned char *p = (const unsigned char *)text; *p; p++) {
        if (*p < '0' || *p > '9' || parsed > (UINT64_MAX - (uint64_t)(*p - '0')) / 10U)
            return LXP_ERR_NON_CANONICAL;
        parsed = parsed * 10U + (uint64_t)(*p - '0');
    }
    *value = parsed;
    return LXP_OK;
}
static void write_u64_be(uint8_t bytes[8], uint64_t value)
{
    for (size_t i = 0; i < 8; i++)
        bytes[7U - i] = (uint8_t)(value >> (i * 8U));
}
static int hex_nibble(char value)
{
    if (value >= '0' && value <= '9')
        return value - '0';
    if (value >= 'a' && value <= 'f')
        return value - 'a' + 10;
    if (value >= 'A' && value <= 'F')
        return value - 'A' + 10;
    return -1;
}

static lxp_result decode_hex(const char *text, uint8_t *output, size_t output_length)
{
    size_t index;
    if (text == NULL || output == NULL || strlen(text) != output_length * 2U)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < output_length; ++index) {
        int high = hex_nibble(text[index * 2U]);
        int low = hex_nibble(text[index * 2U + 1U]);
        if (high < 0 || low < 0)
            return LXP_ERR_NON_CANONICAL;
        output[index] = (uint8_t)(((unsigned int)high << 4U) | (unsigned int)low);
    }
    return LXP_OK;
}

static lxp_result load_identities(const char *path, lxp_identity_store *identities)
{
    FILE *file;
    char line[4096];
    lxp_result status = LXP_OK;
    if (path == NULL || identities == NULL)
        return LXP_ERR_NON_CANONICAL;
    file = fopen(path, "rb");
    if (file == NULL)
        return LXP_ERR_IO;
    (void)memset(identities, 0, sizeof(*identities));
    while (status == LXP_OK && fgets(line, sizeof(line), file) != NULL) {
        char *key_separator = strchr(line, ':');
        char *sequence_separator;
        char *end;
        uint8_t did[LXP_MAX_DID_LENGTH];
        uint8_t key[32];
        size_t did_length;
        uint64_t next_sequence;
        lxp_identity *identity;
        if (key_separator == NULL) {
            status = LXP_ERR_NON_CANONICAL;
            break;
        }
        sequence_separator = strchr(key_separator + 1, ':');
        if (sequence_separator == NULL || strchr(sequence_separator + 1, ':') != NULL) {
            status = LXP_ERR_NON_CANONICAL;
            break;
        }
        end = strchr(sequence_separator + 1, '\n');
        if (end != NULL)
            *end = '\0';
        *key_separator = '\0';
        *sequence_separator = '\0';
        if ((size_t)(key_separator - line) == 0U || ((size_t)(key_separator - line) & 1U) != 0U ||
            (size_t)(key_separator - line) / 2U > sizeof(did)) {
            status = LXP_ERR_NON_CANONICAL;
            break;
        }
        did_length = (size_t)(key_separator - line) / 2U;
        status = decode_hex(line, did, did_length);
        if (status == LXP_OK)
            status = decode_hex(key_separator + 1, key, 32U);
        if (status == LXP_OK)
            status = parse_u64_text(sequence_separator + 1, &next_sequence);
        if (status == LXP_OK)
            status = lxp_identity_register(identities, did, did_length, key, &identity);
        if (status == LXP_OK)
            identity->next_sequence = next_sequence;
    }
    if (status == LXP_OK && ferror(file))
        status = LXP_ERR_IO;
    if (status == LXP_OK && identities->count == 0U)
        status = LXP_ERR_UNKNOWN_DID;
    if (fclose(file) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    return status;
}

static lxp_result collect_assets(gp_runtime *process)
{
    size_t account_index;
    process->asset_count = 0U;
    for (account_index = 0U; account_index < process->accounts.count; ++account_index) {
        lx_account *account = &process->accounts.accounts[account_index];
        size_t asset_index;
        if (!account->has_asset)
            continue;
        for (asset_index = 0U; asset_index < process->asset_count; ++asset_index)
            if (lxp_ct_memcmp(process->assets[asset_index].asset_id, account->asset_id, 32U) == 0)
                break;
        if (asset_index != process->asset_count)
            continue;
        if (process->asset_count == LX_ASSET_REGISTRY_CAPACITY)
            return LXP_ERR_LENGTH_LIMIT;
        (void)memcpy(process->assets[process->asset_count].asset_id, account->asset_id, 32U);
        process->assets[process->asset_count].registered = true;
        process->assets[process->asset_count].paused = false;
        (void)memcpy(process->send_assets[process->asset_count].asset_id, account->asset_id, 32U);
        ++process->asset_count;
    }
    return process->asset_count == 0U ? LXP_ERR_ASSET_MISMATCH : LXP_OK;
}

static lxp_result occupancy_parameters(void *context, uint32_t recorded_fee_schedule_version,
                                       lx_programs_fee_schedule *schedule,
                                       uint8_t occupancy_asset_id[32])
{
    gp_runtime *process = (gp_runtime *)context;
    if (process == NULL)
        return LXP_ERR_NON_CANONICAL;
    return lxp_programs_fee_governance_resolve_runtime(
        &process->kernel, recorded_fee_schedule_version, schedule, occupancy_asset_id);
}

static lxp_result principal_authority(gp_runtime *process, const lxp_activity *activity,
                                      const uint8_t account_key[32],
                                      uint8_t principal_id[32], lxp_u128 *fee_balance)
{
    static const uint8_t prefix[] = "agent:";
    static const uint8_t suffix[] = ":main";
    uint8_t name[LX_ACCOUNT_NAME_MAX];
    size_t length;
    size_t index;
    lxp_result status;
    if (process == NULL || activity == NULL || account_key == NULL ||
        principal_id == NULL || fee_balance == NULL ||
        activity->actor_did.bytes == NULL || activity->actor_did.length == 0U ||
        activity->authority.length != 32U ||
        activity->actor_did.length > sizeof(name) - sizeof(prefix) - sizeof(suffix) + 2U)
        return LXP_ERR_NON_CANONICAL;
    length = sizeof(prefix) - 1U;
    (void)memcpy(name, prefix, length);
    (void)memcpy(name + length, activity->actor_did.bytes, activity->actor_did.length);
    length += activity->actor_did.length;
    (void)memcpy(name + length, suffix, sizeof(suffix) - 1U);
    length += sizeof(suffix) - 1U;
    status = lx_account_id_from_string(name, length, principal_id);
    if (status != LXP_OK)
        return status;
    *fee_balance = (lxp_u128){0U, 0U};
    for (index = 0U; index < process->accounts.count; ++index) {
        const lx_account *account = &process->accounts.accounts[index];
        if (lxp_ct_memcmp(account->id, principal_id, 32U) != 0)
            continue;
        if (account->kind != LX_ACCOUNT_AGENT_MAIN || !account->has_authority_key ||
            lxp_ct_memcmp(account->authority_key, account_key, 32U) != 0)
            return LXP_ERR_BAD_SIGNATURE;
        *fee_balance = account->balance;
        return LXP_OK;
    }
    return LXP_OK;
}

static lxp_result load_schedule(gp_runtime *process)
{
    static const uint8_t key[32] = {'p', 'a', 'r', 'a', 'm', 'e', 't', 'e', 'r',
                                    '-', 'v', 'e', 'r', 's', 'i', 'o', 'n'};
    const lxp_module_kv_entry *parameter = NULL;
    uint32_t parameter_version;
    size_t index;
    if (process == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < process->kernel.module_kv_count; ++index) {
        const lxp_module_kv_entry *entry = &process->kernel.module_kv[index];
        if (entry->module_id == LXP_MODULE_GOVERNANCE && entry->key_length == sizeof(key) &&
            memcmp(entry->key, key, sizeof(key)) == 0) {
            if (parameter != NULL)
                return LXP_ERR_SEQUENCE_REUSED;
            parameter = entry;
        }
    }
    if (parameter == NULL || parameter->value_length != 32U ||
        !lxp_ct_is_zero(parameter->value, 28U))
        return LXP_ERR_VERSION_UNSUPPORTED;
    parameter_version = ((uint32_t)parameter->value[28] << 24U) |
                        ((uint32_t)parameter->value[29] << 16U) |
                        ((uint32_t)parameter->value[30] << 8U) | parameter->value[31];
    if (parameter_version == 0U || parameter_version > UINT16_MAX)
        return LXP_ERR_VERSION_UNSUPPORTED;
    process->parameter_version = parameter_version;
    return lxp_fee_committed_schedule(&process->kernel, parameter_version,
                                      &process->fees);
}

static lxp_result replay_execute_activity(gp_runtime *process, uint64_t global_sequence,
                                          const uint8_t *canonical_activity, size_t activity_length,
                                          const uint8_t *canonical_receipt, size_t receipt_length,
                                          const lxp_receipt *expected, uint64_t timestamp,
                                          uint64_t batch_number, lxp_activity *activity,
                                          lxp_receipt *receipt)
{
    lxp_identity *identity;
    uint8_t principal_id[32];
    lxp_u128 principal_balance = {0U, 0U};
    lxp_authority_grant grant;
    lxp_authority_resolved authority;
    lxp_kernel_execution execution;
    lxp_byte_span encoded_receipt;
    uint8_t activity_id[32];
    lxp_result status;
    if (process == NULL || canonical_activity == NULL || canonical_receipt == NULL ||
        activity == NULL || receipt == NULL || expected == NULL || activity_length == 0U ||
        receipt_length == 0U || timestamp == 0U ||
        batch_number < process->sequencer_authorization.first_batch_number ||
        batch_number > process->sequencer_authorization.last_batch_number ||
        global_sequence != process->state.next_sequence ||
        expected->global_sequence != global_sequence)
        return LXP_ERR_SEQUENCE_GAP;
    if ((expected->module_id != LXP_MODULE_PROGRAMS && expected->module_id != LXP_MODULE_ASSET &&
         expected->module_id != LXP_MODULE_GOVERNANCE &&
         !(process->custody_credit_enabled && expected->module_id == LXP_MODULE_BRIDGE)) ||
        expected->module_version == 0U ||
        expected->parameter_version != process->parameter_version ||
        process->fees.version != expected->parameter_version)
        return LXP_ERR_VERSION_UNSUPPORTED;
    status = lxp_activity_decode(canonical_activity, activity_length, activity);
    if (status == LXP_OK && activity->protocol_version != process->protocol_version)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK)
        status = lxp_activity_check_envelope(activity, process->network_id);
    if (status == LXP_OK)
        status = lxp_activity_verify_payload_hash(activity);
    if (status == LXP_OK)
        status = lxp_activity_verify_signature(activity);
    if (status == LXP_OK &&
        lxp_activity_module_id(activity->activity_type) != LXP_MODULE_PROGRAMS &&
        activity->activity_type != LX_ASSET_SEND &&
        activity->activity_type != LX_ASSET_WITHDRAW &&
        !lxp_governance_activity(activity->activity_type) &&
        !(process->custody_credit_enabled && activity->activity_type == LXP_BRIDGE_CREDIT))
        status = LXP_ERR_UNKNOWN_ACTIVITY;
    if (status == LXP_OK &&
        (expected->module_id != lxp_activity_module_id(activity->activity_type) ||
         ((activity->activity_type == LX_ASSET_SEND || activity->activity_type == LX_ASSET_WITHDRAW) &&
          expected->module_version != lx_asset_module_iface()->abi_version) ||
         ((activity->activity_type == LXP_BRIDGE_CREDIT ||
           lxp_governance_activity(activity->activity_type)) && expected->module_version != 1U)))
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK)
        status = lxp_identity_resolve(&process->identities, activity->actor_did.bytes,
                                      activity->actor_did.length, &identity);
    if (status == LXP_OK)
        status = lxp_governance_identity_refresh(&process->kernel, identity);
    if (status == LXP_OK && activity->authority.length != 32U)
        status = LXP_ERR_BAD_SIGNATURE;
    if (status == LXP_OK)
        status = lxp_authority_resolve_activity(
            &process->kernel, identity, activity,
            lxp_identity_key_valid(identity, activity->authority.bytes, timestamp,
                                   global_sequence),
            true, timestamp, UINT64_C(300000), global_sequence, &grant, &authority);
    if (status == LXP_OK)
        status = principal_authority(process, activity,
            grant.kind == LXP_AUTHORITY_OWNER ? grant.key : identity->primary_key,
            principal_id, &principal_balance);
    if (status == LXP_OK)
        status = lxp_activity_id(canonical_activity, activity_length, activity_id);
    if (status != LXP_OK)
        return status;
    (void)memcpy(authority.principal, principal_id, 32U);
    (void)memset(&execution, 0, sizeof(execution));
    status = lxp_batch_identity_activity(
        process->kernel.current_state_root, activity_id, global_sequence,
        batch_number, execution.batch_id);
    if (status != LXP_OK)
        return status;
    if (!lxp_ct_is_zero(expected->activity_root, 32U)) {
        size_t index = (size_t)(global_sequence - process->prepared_first_sequence);
        if (index >= process->activity_count || !process->batch_bindings ||
            lxp_ct_memcmp(expected->activity_root, process->batch_bindings[index].activity_root,
                          32U))
            return LXP_ERR_CONTEXT_MISMATCH;
        memcpy(execution.activity_root, process->batch_bindings[index].activity_root, 32U);
        memcpy(execution.batch_id, process->batch_bindings[index].batch_id, 32U);
    } else if (process->activity_count != 1U)
        return LXP_ERR_CONTEXT_MISMATCH;
    execution.network_id = process->network_id;
    execution.batch_number = batch_number;
    execution.batch_timestamp_ms = timestamp;
    execution.maximum_timestamp_window = UINT64_C(300000);
    execution.epoch = process->kernel.epoch;
    execution.global_sequence = global_sequence;
    execution.recorded_module_version = expected->module_version;
    execution.recorded_metering_schedule_version =
        expected->program_outcome.present ? expected->program_outcome.metering_schedule_version
                                          : 0U;
    execution.recorded_fee_schedule_version =
        expected->program_outcome.present ? expected->program_outcome.fee_schedule_version : 0U;
    execution.parameter_version = expected->parameter_version;
    execution.signature_valid = true;
    execution.identities = &process->identities;
    execution.authority = &authority;
    execution.fee_parameters = &process->fees;
    execution.fee_balance = principal_balance;
    execution.gas_limit = UINT64_MAX;
    execution.arena = &process->execution_arena;
    execution.sequencer_private_key = NULL;
    execution.verified_receipts = &process->verified_receipts;
    (void)memset(receipt, 0, sizeof(*receipt));
    status = lxp_kernel_execute_activity(&process->kernel, activity, &execution, receipt);
    if (status == LXP_OK)
        (void)memcpy(receipt->sequencer_signature, expected->sequencer_signature, 64U);
    if (status == LXP_OK)
        status = lxp_receipt_encode(receipt, true, &process->execution_arena, &encoded_receipt);
    if (status == LXP_OK &&
        (encoded_receipt.length != receipt_length ||
         lxp_ct_memcmp(encoded_receipt.bytes, canonical_receipt, receipt_length) != 0))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    return status;
}
static lxp_result read_artifact(const char *path, size_t maximum, uint8_t **bytes, size_t *length)
{
    struct stat st;
    int fd;
    size_t offset = 0;
    if (!path || !bytes || !length)
        return LXP_ERR_NON_CANONICAL;
    fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (fd < 0)
        return LXP_ERR_IO;
    if (fstat(fd, &st) || !S_ISREG(st.st_mode) || st.st_size <= 0 ||
        (uintmax_t)st.st_size > maximum) {
        close(fd);
        return LXP_ERR_LENGTH_LIMIT;
    }
    *length = (size_t)st.st_size;
    *bytes = malloc(*length);
    if (!*bytes) {
        close(fd);
        return LXP_ERR_IO;
    }
    while (offset < *length) {
        ssize_t n = read(fd, *bytes + offset, *length - offset);
        if (n < 0 && errno == EINTR)
            continue;
        if (n <= 0) {
            free(*bytes);
            *bytes = NULL;
            close(fd);
            return LXP_ERR_IO;
        }
        offset += (size_t)n;
    }
    if (close(fd)) {
        free(*bytes);
        *bytes = NULL;
        return LXP_ERR_IO;
    }
    return LXP_OK;
}
static lxp_result compare_unsigned(gp_runtime *runtime, const lxp_receipt *receipt)
{
    lxp_byte_span expected, actual;
    size_t mark = lxp_arena_mark(&runtime->execution_arena);
    lxp_result status =
        lxp_receipt_encode(&runtime->expected, false, &runtime->execution_arena, &expected);
    if (status == LXP_OK)
        status = lxp_receipt_encode(receipt, false, &runtime->execution_arena, &actual);
    if (status == LXP_OK && (actual.length != expected.length ||
                             lxp_ct_memcmp(actual.bytes, expected.bytes, actual.length)))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    (void)lxp_arena_reset(&runtime->execution_arena, mark);
    return status;
}
static lxp_result feed_record(gp_runtime *runtime, uint8_t kind, const uint8_t *bytes,
                              size_t length)
{
    uint8_t header[9];
    header[0] = kind;
    write_u64_be(header + 1, length);
    if (!runtime->feed_file ||
        fwrite(header, 1, sizeof(header), runtime->feed_file) != sizeof(header) ||
        (length && fwrite(bytes, 1, length, runtime->feed_file) != length) ||
        fflush(runtime->feed_file) || fsync(fileno(runtime->feed_file)))
        return LXP_ERR_IO;
    return LXP_OK;
}
static lxp_result feed_lock(void *context)
{
    gp_runtime *runtime = context;
    return pthread_mutex_lock(&runtime->feed_mutex) ? LXP_ERR_IO : LXP_OK;
}
static lxp_result feed_unlock(void *context)
{
    gp_runtime *runtime = context;
    return pthread_mutex_unlock(&runtime->feed_mutex) ? LXP_ERR_IO : LXP_OK;
}
static lxp_result feed_begin(void *context, const lxp_activity *activity,
                             const lxp_receipt *receipt)
{
    gp_runtime *runtime = context;
    lxp_result status = compare_unsigned(runtime, receipt);
    lxp_byte_span canonical;
    size_t index;
    (void)activity;
    if (status != LXP_OK)
        return status;
    if (receipt->global_sequence < runtime->prepared_first_sequence)
        return LXP_ERR_SEQUENCE_GAP;
    index = (size_t)(receipt->global_sequence - runtime->prepared_first_sequence);
    if (index >= runtime->activity_count)
        return LXP_ERR_SEQUENCE_GAP;
    canonical = runtime->published_receipts[index];
    status = feed_record(runtime, 1U, canonical.bytes, canonical.length);
    if (status == LXP_OK) {
        runtime->feed_sequence = receipt->global_sequence;
        runtime->feed_ordinal = 0U;
    }
    return status;
}
static lxp_result feed_append(void *context, uint64_t sequence, uint32_t ordinal,
                              const uint8_t program[32], uint32_t activity_type,
                              uint16_t event_type, const lxp_receipt *receipt)
{
    gp_runtime *runtime = context;
    uint8_t notice[94];
    lxp_result status;
    if (sequence != runtime->feed_sequence || ordinal != runtime->feed_ordinal ||
        sequence != receipt->global_sequence)
        return LXP_ERR_SEQUENCE_GAP;
    write_u64_be(notice, sequence);
    write_u64_be(notice + 8, ordinal);
    memcpy(notice + 16, program, 32U);
    write_u64_be(notice + 48, activity_type);
    notice[56] = (uint8_t)(event_type >> 8U);
    notice[57] = (uint8_t)event_type;
    size_t mark = lxp_arena_mark(&runtime->execution_arena);
    status = lxp_receipt_digest(&runtime->expected, &runtime->execution_arena, notice + 58);
    (void)lxp_arena_reset(&runtime->execution_arena, mark);
    if (status == LXP_OK)
        status = feed_record(runtime, 2U, notice, 90U);
    if (status == LXP_OK)
        runtime->feed_ordinal++;
    return status;
}
static lxp_result feed_advance(void *context, const lxp_activity *activity,
                               const lxp_receipt *receipt)
{
    gp_runtime *runtime = context;
    uint8_t boundary[48];
    (void)activity;
    if (receipt->global_sequence != runtime->feed_sequence)
        return LXP_ERR_SEQUENCE_GAP;
    write_u64_be(boundary, receipt->global_sequence);
    write_u64_be(boundary + 8, runtime->feed_ordinal);
    memcpy(boundary + 16, receipt->resulting_state_root, 32U);
    return feed_record(runtime, 3U, boundary, sizeof(boundary));
}
static lxp_result durable_receipt_facts(void *context, const uint8_t digest[32],
                                        lxp_verified_receipt_facts *facts)
{
    gp_runtime *runtime = context;
    struct stat info;
    off_t offset = runtime->feed_session_start;
    int fd = fileno(runtime->feed_file);
    lxp_result status = LXP_ERR_UNKNOWN_FIELD;
    size_t mark = lxp_arena_mark(&runtime->execution_arena);
    lxp_receipt *receipt = malloc(sizeof(*receipt));
    uint8_t *body = NULL;
    if (!receipt || fstat(fd, &info)) {
        free(receipt);
        return LXP_ERR_IO;
    }
    while (offset < info.st_size) {
        uint8_t header[9], candidate[32];
        uint64_t length = 0;
        if (info.st_size - offset < 9 ||
            pread(fd, header, sizeof(header), offset) != (ssize_t)sizeof(header)) {
            status = LXP_ERR_IO;
            break;
        }
        offset += 9;
        for (size_t i = 1; i < 9; i++)
            length = (length << 8U) | header[i];
        if (length > LXP_MAX_REPLAY_FIELD_BYTES || length > (uint64_t)(info.st_size - offset)) {
            status = LXP_ERR_LENGTH_LIMIT;
            break;
        }
        if (header[0] != 1U) {
            offset += (off_t)length;
            continue;
        }
        body = malloc((size_t)length);
        if (!body) {
            status = LXP_ERR_IO;
            break;
        }
        if (pread(fd, body, (size_t)length, offset) != (ssize_t)length) {
            status = LXP_ERR_IO;
            break;
        }
        status = lxp_receipt_decode(body, (size_t)length, true, receipt);
        if (status == LXP_OK)
            status = lxp_receipt_verify(receipt, runtime->sequencer_authorization.public_key,
                                        &runtime->execution_arena);
        if (status == LXP_OK)
            status = lxp_receipt_digest(receipt, &runtime->execution_arena, candidate);
        if (status != LXP_OK)
            break;
        if (!lxp_ct_memcmp(candidate, digest, 32U) &&
            receipt->global_sequence < runtime->state.next_sequence) {
            memset(facts, 0, sizeof(*facts));
            memcpy(facts->receipt_digest, candidate, 32U);
            facts->result_code = receipt->result_code;
            facts->global_sequence = receipt->global_sequence;
            facts->timestamp = receipt->timestamp;
            memcpy(facts->asset, receipt->asset, 32U);
            facts->amount = receipt->amount;
            memcpy(facts->resulting_state_root, receipt->resulting_state_root, 32U);
            status = LXP_OK;
            break;
        }
        free(body);
        body = NULL;
        (void)lxp_arena_reset(&runtime->execution_arena, mark);
        offset += (off_t)length;
        status = LXP_ERR_UNKNOWN_FIELD;
    }
    free(body);
    free(receipt);
    (void)lxp_arena_reset(&runtime->execution_arena, mark);
    return status;
}
static lxp_result parameter_version(void *context, uint64_t epoch, uint32_t *version)
{
    gp_runtime *runtime = context;
    if (!runtime || !version || epoch != runtime->kernel.epoch)
        return LXP_ERR_CONTEXT_MISMATCH;
    lxp_result status = load_schedule(runtime);
    if (status == LXP_OK)
        *version = runtime->parameter_version;
    return status;
}
static lxp_result replay_transition(void *context, uint16_t version, uint32_t parameters,
                                    uint64_t timestamp, uint64_t sequence,
                                    lxp_byte_span canonical_activity,
                                    const uint8_t previous_root[32], lxp_arena *arena,
                                    lxp_replay_activity_output *output)
{
    gp_runtime *runtime = context;
    lxp_activity activity;
    lxp_receipt *receipt = NULL;
    lxp_byte_span canonical;
    size_t index;
    lxp_result status;
    if (!runtime || !arena || !output || runtime->poisoned ||
        version != runtime->protocol_version || parameters != runtime->parameter_version ||
        timestamp != runtime->prepared_timestamp || sequence < runtime->prepared_first_sequence ||
        sequence > runtime->prepared_last_sequence ||
        lxp_ct_memcmp(previous_root, runtime->kernel.current_state_root, 32U))
        return LXP_ERR_CONTEXT_MISMATCH;
    index = (size_t)(sequence - runtime->prepared_first_sequence);
    if (index >= runtime->activity_count)
        return LXP_ERR_SEQUENCE_GAP;
    canonical = runtime->published_receipts[index];
    (void)lxp_arena_reset(&runtime->execution_arena, 0U);
    status = lxp_receipt_decode(canonical.bytes, canonical.length, true, &runtime->expected);
    if (status == LXP_OK)
        status = lxp_receipt_verify(&runtime->expected, runtime->sequencer_authorization.public_key,
                                    &runtime->execution_arena);
    if (status == LXP_OK) {
        receipt = malloc(sizeof(*receipt));
        if (!receipt)
            status = LXP_ERR_IO;
    }
    if (status == LXP_OK)
        status = replay_execute_activity(runtime, sequence, canonical_activity.bytes,
                                         canonical_activity.length, canonical.bytes,
                                         canonical.length, &runtime->expected, timestamp,
                                         runtime->prepared_batch, &activity, receipt);
    if (status == LXP_OK)
        status = lxp_verified_receipt_index_add(&runtime->verified_receipts, receipt,
                                                runtime->sequencer_authorization.public_key,
                                                &runtime->execution_arena);
    if (status == LXP_OK) {
        memset(output, 0, sizeof(*output));
        output->result_code = receipt->result_code;
        output->fee_charged = receipt->fee_charged;
        memcpy(output->resulting_state_root, receipt->resulting_state_root, 32U);
        status = lxp_receipt_encode(receipt, true, arena, &output->canonical_receipt);
    }
    if (status == LXP_OK)
        status = lxp_programs_project_receipt_events(receipt, arena, &output->canonical_events);
    free(receipt);
    if (status != LXP_OK)
        runtime->poisoned = true;
    return status;
}
lxp_result gp_runtime_prepare(gp_runtime *runtime, const lxp_batch_body *body)
{
    lxp_byte_span *events;
    size_t event_count;
    lxp_result status;
    if (!runtime || !body || runtime->poisoned)
        return LXP_ERR_NON_CANONICAL;
    if (body->header.protocol_version != runtime->protocol_version ||
        body->header.network_id != runtime->network_id ||
        body->header.first_sequence != runtime->state.next_sequence ||
        lxp_ct_memcmp(body->header.previous_state_root, runtime->kernel.current_state_root, 32U) ||
        body->header.batch_number != runtime->last_batch + 1U)
        return LXP_ERR_CONTEXT_MISMATCH;
    (void)lxp_arena_reset(&runtime->preparation_arena, 0U);
    status = lxp_replica_validate_header(
        body, runtime->network_id, &runtime->sequencer_authorization, &runtime->preparation_arena);
    if (status == LXP_OK)
        status =
            lxp_replay_section_decode(&body->activities, &runtime->preparation_arena,
                                      &runtime->published_activities, &runtime->activity_count);
    if (status == LXP_OK)
        status = lxp_da_receipt_section_decode(body->receipts, &runtime->preparation_arena,
                                               &runtime->published_receipts,
                                               &runtime->receipt_count, &events, &event_count);
    if (status == LXP_OK && (event_count != runtime->activity_count ||
                             runtime->receipt_count < runtime->activity_count ||
                             runtime->receipt_count > runtime->activity_count + 1U))
        status = LXP_ERR_BATCH_GAP;
    if (status == LXP_OK && runtime->activity_count) {
        void *allocated = NULL;
        lxp_batch_roots roots;
        uint8_t batch_id[32];
        if (runtime->activity_count > LXP_DAEMON_BATCH_WAL_MAX_ITEMS)
            status = LXP_ERR_LENGTH_LIMIT;
        if (status == LXP_OK)
            status = lxp_arena_alloc(&runtime->preparation_arena,
                                     runtime->activity_count * sizeof(*runtime->batch_bindings),
                                     _Alignof(lxp_kernel_execution), &allocated);
        if (status == LXP_OK) {
            runtime->batch_bindings = allocated;
            memset(runtime->batch_bindings, 0,
                   runtime->activity_count * sizeof(*runtime->batch_bindings));
            status = lxp_daemon_batch_bind_prefix(
                runtime->published_activities, runtime->activity_count,
                body->header.previous_state_root, body->header.first_sequence,
                body->header.batch_number, &runtime->preparation_arena, runtime->batch_bindings,
                &roots, batch_id);
        }
        if (status == LXP_OK &&
            lxp_ct_memcmp(roots.activity_merkle_root, body->header.activity_merkle_root, 32U))
            status = LXP_ERR_ROOT_MISMATCH;
    }
    if (status == LXP_OK) {
        runtime->prepared_batch = body->header.batch_number;
        runtime->prepared_first_sequence = body->header.first_sequence;
        runtime->prepared_last_sequence = body->header.last_sequence;
        runtime->prepared_timestamp = body->header.timestamp_ms;
        runtime->last_batch = body->header.batch_number;
    }
    return status;
}
lxp_replay_engine *gp_runtime_engine(gp_runtime *runtime)
{
    return runtime ? &runtime->engine : NULL;
}
void gp_runtime_close(gp_runtime *runtime)
{
    if (!runtime)
        return;
    if (runtime->feed_file)
        (void)fclose(runtime->feed_file);
    if (runtime->mutex_open)
        (void)pthread_mutex_destroy(&runtime->feed_mutex);
    if (runtime->state_open)
        (void)lxp_state_store_destroy(&runtime->state);
    lx_account_registry_release(&runtime->accounts);
    free(runtime->execution_bytes);
    free(runtime->preparation_bytes);
    free(runtime);
}
lxp_result gp_runtime_open(gp_runtime **output, const char *configuration,
                           const char *state_directory)
{
    gp_runtime *runtime;
    lxp_genesis_manifest *genesis = NULL;
    lxp_genesis_bootstrap_registration registration;
    lxp_snapshot_manifest_record manifest;
    lxp_byte_span snapshot;
    lxp_bridge_profile profile;
    uint8_t *bytes = NULL;
    size_t length = 0;
    char path[4096];
    bool enabled = false;
    int fd = -1;
    lxp_result status;
    if (!output || !configuration || !state_directory)
        return LXP_ERR_NON_CANONICAL;
    *output = NULL;
    runtime = calloc(1, sizeof(*runtime));
    genesis = malloc(sizeof(*genesis));
    if (!runtime || !genesis) {
        free(runtime);
        free(genesis);
        return LXP_ERR_IO;
    }
    runtime->execution_bytes = malloc(3U * LXP_MAX_ACTIVITY_BYTES);
    runtime->preparation_bytes = malloc(LXP_DAEMON_PROTOCOL_SCRATCH_MIN_BYTES);
    status = runtime->execution_bytes && runtime->preparation_bytes ? LXP_OK : LXP_ERR_IO;
    if (status == LXP_OK)
        status = lxp_arena_init(&runtime->execution_arena, runtime->execution_bytes,
                                3U * LXP_MAX_ACTIVITY_BYTES);
    if (status == LXP_OK)
        status = lxp_arena_init(&runtime->preparation_arena, runtime->preparation_bytes,
                                LXP_DAEMON_PROTOCOL_SCRATCH_MIN_BYTES);
    if (status == LXP_OK)
        status = lxp_daemon_config_load(configuration, &runtime->configuration);
    if (status == LXP_OK)
        status = read_artifact(getenv("LAYERX_NODE_GENESIS_MANIFEST"),
                               LXP_GENESIS_MAX_ENCODED_BYTES, &bytes, &length);
    if (status == LXP_OK)
        status = lxp_genesis_parse(bytes, length, LXP_GENESIS_INPUT_MANIFEST, genesis);
    if (status == LXP_OK)
        status = lxp_genesis_verify_signature(genesis, &runtime->preparation_arena);
    free(bytes);
    bytes = NULL;
    if (status == LXP_OK)
        status = read_artifact(getenv("LAYERX_NODE_GENESIS_REGISTRATION"),
                               LXP_GENESIS_REGISTRATION_BYTES, &bytes, &length);
    if (status == LXP_OK)
        status = lxp_genesis_registration_parse(bytes, length, &registration);
    free(bytes);
    bytes = NULL;
    if (status == LXP_OK && genesis->network_id != runtime->configuration.network_id)
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK) {
        runtime->network_id = genesis->network_id;
        runtime->protocol_version = genesis->protocol_version;
        status = lxp_bridge_genesis_profile(genesis, &profile, &runtime->custody_credit_enabled);
    }
    if (status == LXP_OK)
        status = lx_account_registry_init(&runtime->accounts);
    if (status == LXP_OK) {
        status = lxp_state_store_init(&runtime->state, 1U);
        runtime->state_open = status == LXP_OK;
    }
    if (status == LXP_OK)
        status = lxp_state_store_bind_accounts(&runtime->state, &runtime->accounts);
    if (status == LXP_OK)
        status = lxp_kernel_create(&runtime->kernel, &runtime->state, &runtime->journal,
                                   &runtime->configuration, 1U);
    if (status == LXP_OK) {
        lxp_genesis_module_plan genesis_module_plan;
        status = lxp_genesis_module_plan_resolve(genesis, &genesis_module_plan);
        if (status == LXP_OK)
            status = lxp_genesis_module_plan_register(&genesis_module_plan,
                                                      &runtime->kernel);
    }
    if (status == LXP_OK)
        status =
            lxp_kernel_set_capabilities(&runtime->kernel, NULL, lxp_kernel_canonical_ledger_apply);
    if (status == LXP_OK)
        status = lxp_snapshot_store_read(getenv("LAYERX_NODE_SNAPSHOT"),
                                         &runtime->preparation_arena, &manifest, &snapshot);
    if (status == LXP_OK)
        status = lxp_snapshot_load(snapshot.bytes, snapshot.length, &manifest, &runtime->kernel);
    if (status == LXP_OK)
        status = lxp_genesis_bootstrap_verify(genesis, &registration, runtime->network_id, true,
                                              &manifest, &runtime->kernel,
                                              &runtime->preparation_arena, &enabled);
    if (status == LXP_OK && !enabled)
        status = LXP_ERR_CONTEXT_MISMATCH;
    free(genesis);
    if (status == LXP_OK)
        status = collect_assets(runtime);
    if (status == LXP_OK && runtime->protocol_version == LXP_PROTOCOL_VERSION_STATE_COMMITMENT) {
        runtime->asset_runtime = (lx_asset_runtime){
            &runtime->accounts,   runtime->send_assets, runtime->asset_count,     runtime->assets,
            runtime->asset_count, runtime->network_id,  runtime->protocol_version};
        status = lxp_kernel_bind_module_runtime(&runtime->kernel, LXP_MODULE_ASSET,
                                                &runtime->asset_runtime);
    }
    if (status == LXP_OK)
        status = load_schedule(runtime);
    if (status == LXP_OK)
        status = load_identities(getenv("LAYERX_NODE_IDENTITIES"), &runtime->identities);
    if (status == LXP_OK)
        status = decode_hex(getenv("LAYERX_NODE_SEQUENCER_ID"),
                            runtime->sequencer_authorization.sequencer_id, 32U);
    if (status == LXP_OK)
        status = decode_hex(getenv("LAYERX_NODE_SEQUENCER_PUBLIC_KEY"),
                            runtime->sequencer_authorization.public_key, 32U);
    if (status == LXP_OK)
        status = parse_u64_text(getenv("LAYERX_NODE_FIRST_BATCH"),
                                &runtime->sequencer_authorization.first_batch_number);
    if (status == LXP_OK)
        status = parse_u64_text(getenv("LAYERX_NODE_LAST_BATCH"),
                                &runtime->sequencer_authorization.last_batch_number);
    if (status == LXP_OK && (!runtime->sequencer_authorization.first_batch_number ||
                             runtime->sequencer_authorization.first_batch_number >
                                 runtime->sequencer_authorization.last_batch_number))
        status = LXP_ERR_BATCH_GAP;
    if (status == LXP_OK) {
        runtime->sequencer_authorization.authorized = 1U;
        runtime->last_batch = runtime->sequencer_authorization.first_batch_number - 1U;
        status = lxp_verified_receipt_index_init(&runtime->verified_receipts);
    }
    if (status == LXP_OK) {
        int n = snprintf(path, sizeof(path), "%s/replay-feed.log", state_directory);
        if (n < 0 || (size_t)n >= sizeof(path))
            status = LXP_ERR_LENGTH_LIMIT;
    }
    if (status == LXP_OK) {
        fd = open(path, O_RDWR | O_CREAT | O_APPEND | O_NOFOLLOW | O_CLOEXEC, 0600);
        if (fd < 0)
            status = LXP_ERR_IO;
        else {
            struct stat info;
            if (fstat(fd, &info) || !S_ISREG(info.st_mode) || info.st_nlink != 1 ||
                info.st_uid != geteuid() || (info.st_mode & 0777U) != 0600U)
                status = LXP_ERR_IO;
            if (status == LXP_OK) {
                runtime->feed_session_start = info.st_size;
                runtime->feed_file = fdopen(fd, "a+b");
            }
            if (!runtime->feed_file) {
                close(fd);
                status = LXP_ERR_IO;
            }
        }
    }
    if (status == LXP_OK)
        status = lxp_verified_receipt_index_bind_fallback(&runtime->verified_receipts,
                                                          durable_receipt_facts, runtime);
    if (status == LXP_OK) {
        status = pthread_mutex_init(&runtime->feed_mutex, NULL) ? LXP_ERR_IO : LXP_OK;
        runtime->mutex_open = status == LXP_OK;
    }
    if (status == LXP_OK) {
        runtime->feed = (lx_programs_state_feed){feed_begin, feed_append, feed_advance,
                                                 feed_lock,  feed_unlock, runtime};
        runtime->programs.accounts = &runtime->accounts;
        runtime->programs.assets = runtime->assets;
        runtime->programs.asset_count = runtime->asset_count;
        runtime->programs.resolve_occupancy_parameters = occupancy_parameters;
        runtime->programs.occupancy_parameter_context = runtime;
        runtime->programs.resolve_metering_schedule = lxp_programs_metering_resolve_runtime;
        runtime->programs.metering_schedule_context = &runtime->kernel;
        runtime->programs.state_feed = &runtime->feed;
        status = lxp_kernel_bind_module_runtime(&runtime->kernel, LXP_MODULE_PROGRAMS,
                                                &runtime->programs);
    }
    if (status == LXP_OK) {
        lx_programs_fee_schedule schedule;
        lx_programs_metering_schedule metering;
        uint8_t asset[32];
        status =
            lxp_programs_fee_governance_resolve_runtime(&runtime->kernel, 0U, &schedule, asset);
        if (status == LXP_OK)
            status = lxp_programs_metering_schedule_current(
                &runtime->kernel, runtime->sequencer_authorization.first_batch_number, &metering);
    }
    if (status == LXP_OK)
        status = lxp_replay_engine_init(&runtime->engine, parameter_version, runtime);
    if (status == LXP_OK)
        status = lxp_programs_replay_engine_bind(&runtime->engine, &runtime->kernel);
    if (status == LXP_OK)
        status = lxp_replay_engine_register(&runtime->engine, runtime->protocol_version,
                                            replay_transition);
    if (status != LXP_OK) {
        gp_runtime_close(runtime);
        return status;
    }
    (void)lxp_arena_reset(&runtime->preparation_arena, 0U);
    *output = runtime;
    return LXP_OK;
}

lxp_result gp_runtime_authority(void *context, const lxp_activity *activity,
                                lxp_byte_span canonical, lxp_guarantor_authority_verdict *verdict)
{
    gp_runtime *runtime = context;
    lxp_identity *identity = NULL;
    lxp_authority_grant grant;
    lxp_authority_resolved authority;
    uint8_t principal[32];
    uint8_t expected_authority_hash[32];
    lxp_u128 balance;
    size_t index;
    uint64_t sequence;
    lxp_result status;
    if (!runtime || !activity || !verdict || runtime->poisoned || !runtime->prepared_batch)
        return LXP_ERR_NON_CANONICAL;
    memset(verdict, 0, sizeof(*verdict));
    for (index = 0; index < runtime->activity_count; index++) {
        lxp_byte_span candidate = runtime->published_activities[index];
        if (candidate.length == canonical.length &&
            !lxp_ct_memcmp(candidate.bytes, canonical.bytes, candidate.length))
            break;
    }
    if (index == runtime->activity_count || activity->protocol_version != runtime->protocol_version)
        return LXP_ERR_CONTEXT_MISMATCH;
    status = lxp_activity_check_envelope(activity, runtime->network_id);
    if (status == LXP_OK)
        status = lxp_activity_verify_payload_hash(activity);
    if (status == LXP_OK)
        status = lxp_activity_verify_signature(activity);
    if (status == LXP_OK)
        status = lxp_identity_resolve(&runtime->identities, activity->actor_did.bytes,
                                      activity->actor_did.length, &identity);
    if (status == LXP_OK)
        status = lxp_governance_identity_refresh(&runtime->kernel, identity);
    if (status == LXP_OK && activity->authority.length != 32U)
        status = LXP_ERR_BAD_SIGNATURE;
    sequence = runtime->prepared_first_sequence + index;
    if (status == LXP_OK)
        status = lxp_authority_resolve_activity(
            &runtime->kernel, identity, activity,
            lxp_identity_key_valid(identity, activity->authority.bytes,
                                   runtime->prepared_timestamp, sequence),
            true, runtime->prepared_timestamp, UINT64_C(300000), sequence, &grant,
            &authority);
    if (status == LXP_OK)
        status = principal_authority(runtime, activity,
            grant.kind == LXP_AUTHORITY_OWNER ? grant.key : identity->primary_key,
            principal, &balance);
    if (status == LXP_OK)
        status = lxp_authority_hash(authority.kind, grant.grant_id, grant.key,
                                    expected_authority_hash);
    if (status == LXP_OK) {
        verdict->actor_signature =
            lxp_ct_memcmp(authority.verified_key, activity->authority.bytes, 32U) == 0 &&
            lxp_ct_memcmp(authority.actor, identity->did_id, 32U) == 0;
        verdict->session_key =
            lxp_authority_is_live(&grant, identity->revocation_sequence,
                                  runtime->prepared_timestamp, sequence) == LXP_OK;
        verdict->capability_grant =
            lxp_ct_memcmp(authority.authority_hash, expected_authority_hash, 32U) == 0;
        verdict->delegated_authority =
            lxp_ct_memcmp(authority.actor, grant.grantee, 32U) == 0 &&
            lxp_ct_memcmp(grant.grantor, identity->did_id, 32U) == 0;
    }
    return status;
}
lxp_result gp_runtime_oracle(void *context, lxp_byte_span canonical, bool *valid)
{
    (void)context;
    (void)canonical;
    if (!valid)
        return LXP_ERR_NON_CANONICAL;
    *valid = false;
    return LXP_ERR_MODULE_DISABLED;
}

lxp_result gp_runtime_state_proof(gp_runtime *runtime, uint16_t module_id,
                                  lxp_byte_span key, lxp_state_witness *proof)
{
    if (runtime == NULL) return LXP_ERR_NON_CANONICAL;
    return lxp_state_proof_build(&runtime->kernel, module_id, key, proof);
}

static lxp_result settlement_witness(gp_runtime *runtime, FILE *output,
                                     uint16_t module_id, lxp_byte_span key)
{
    lxp_state_witness *proof = malloc(sizeof(*proof));
    uint8_t *wire = malloc(LXP_STATE_WITNESS_MAX_BYTES);
    size_t length = 0U;
    lxp_result status = proof == NULL || wire == NULL ? LXP_ERR_ARENA_EXHAUSTED : LXP_OK;
    if (status == LXP_OK) status = gp_runtime_state_proof(runtime, module_id, key, proof);
    if (status == LXP_OK) status = lxp_state_proof_verify(proof, runtime->kernel.current_state_root);
    if (status == LXP_OK) status = lxp_state_proof_encode(proof, wire, LXP_STATE_WITNESS_MAX_BYTES, &length);
    if (status == LXP_OK) {
        (void)fputs("\"0x", output);
        for (size_t i = 0U; i < length; ++i) (void)fprintf(output, "%02x", wire[i]);
        (void)fputc('"', output);
        if (ferror(output)) status = LXP_ERR_IO;
    }
    free(wire);
    free(proof);
    return status;
}

lxp_result gp_runtime_settlement_facts(gp_runtime *runtime, FILE *output)
{
    lxp_result status = LXP_OK;
    size_t count = 0U;
    if (runtime == NULL || output == NULL || runtime->poisoned)
        return LXP_ERR_NON_CANONICAL;
    (void)fputs("{\"balances\":[", output);
    for (size_t i = 0U; status == LXP_OK && i < runtime->accounts.count; ++i) {
        const lx_account *account = &runtime->accounts.accounts[i];
        uint8_t key[33] = {4U};
        if (account->kind != LX_ACCOUNT_AGENT_MAIN || !account->has_asset) continue;
        if (!account->has_authority_key) return LXP_ERR_UNAUTHORIZED_DEBIT;
        if (count++ != 0U) (void)fputc(',', output);
        (void)memcpy(key + 1U, account->id, 32U);
        status = settlement_witness(runtime, output, 0U, (lxp_byte_span){key, sizeof(key)});
    }
    (void)fputs("],\"withdrawals\":[", output);
    count = 0U;
    for (size_t i = 0U; status == LXP_OK && i < runtime->kernel.module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &runtime->kernel.module_kv[i];
        lx_withdrawal_record record;
        if (entry->module_id != LXP_MODULE_ASSET || entry->key_length != LX_WITHDRAWAL_STATE_KEY_BYTES ||
            memcmp(entry->key, "withdrawal:", 11U) != 0) continue;
        status = lx_withdrawal_state_decode(entry->key, entry->key_length, entry->value,
                                             entry->value_length, &record);
        if (status != LXP_OK) break;
        if (count++ != 0U) (void)fputc(',', output);
        status = settlement_witness(runtime, output, entry->module_id,
                                    (lxp_byte_span){entry->key, entry->key_length});
    }
    (void)fputs("],\"deposits\":[", output);
    count = 0U;
    for (size_t i = 0U; status == LXP_OK && i < runtime->kernel.module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &runtime->kernel.module_kv[i];
        if (entry->module_id != LXP_MODULE_BRIDGE || entry->key_length != 50U ||
            memcmp(entry->key, "deposit-nullifier:", 18U) != 0) continue;
        if (entry->value_length != LXP_BRIDGE_CREDIT_BYTES) return LXP_ERR_NON_CANONICAL;
        if (count++ != 0U) (void)fputc(',', output);
        status = settlement_witness(runtime, output, entry->module_id,
                                    (lxp_byte_span){entry->key, entry->key_length});
    }
    (void)fputs("],\"profile\":", output);
    if (runtime->custody_credit_enabled && status == LXP_OK)
        status = settlement_witness(runtime, output, LXP_MODULE_BRIDGE,
                                    (lxp_byte_span){lxp_bridge_profile_key, 32U});
    else (void)fputs("null", output);
    (void)fputc('}', output);
    return ferror(output) ? LXP_ERR_IO : status;
}
