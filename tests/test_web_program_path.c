#define OPENSSL_API_COMPAT 0x10100000L
#define main web_program_path_reference_main
int web_program_path_reference_main(int argc, char **argv);
#include "programs/test_call_activity.c"
#undef main

#include "layerx/lx_batch.h"
#include "layerx/lx_web.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/ecdsa.h>
#include <openssl/obj_mac.h>

#define PATH_CHECK(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "web program path check failed at line %d\n", \
                  __LINE__); \
    return 1; } } while (0)

enum {
    PATH_NETWORK = 7,
    PATH_ATTESTORS = 3,
    PATH_THRESHOLD = 2,
    PATH_WASM_CAPACITY = 262144,
    PATH_CALLDATA_BYTES = 512,
    PATH_PENDING_KEY_BYTES = 11 + 32 + 8,
    PATH_PENDING_RECORD_BYTES = 123,
    PATH_FEE = 100,
    PATH_OP_REQUEST = 1,
    PATH_OP_READ = 2
};

static const uint8_t path_response[] = "Paxeer X Network";
static const uint8_t path_payload[] = "https://paxeer.app/status";
static const uint64_t path_request = UINT64_C(0x0102030405060708);
static const uint64_t path_unpaid_request = UINT64_C(0x0102030405060709);

enum {
    PATH_RESPONSE_BYTES = sizeof(path_response) - 1U,
    PATH_PAYLOAD_BYTES = sizeof(path_payload) - 1U
};

typedef struct path_attestor {
    uint8_t private_key[32];
    uint8_t signer[LX_WEB_SIGNER_BYTES];
    uint8_t payout[32];
    lx_account *account;
} path_attestor;

static int path_read_file(const char *path, uint8_t *out, size_t capacity,
                          size_t *length)
{
    FILE *file = fopen(path, "rb");
    size_t read;
    if (file == NULL) return 1;
    read = fread(out, 1U, capacity, file);
    if (ferror(file) || !feof(file)) {
        (void)fclose(file);
        return 1;
    }
    (void)fclose(file);
    *length = read;
    return read == 0U ? 1 : 0;
}

static int path_signer(const uint8_t private_key[32],
                       uint8_t signer[LX_WEB_SIGNER_BYTES])
{
    EC_KEY *key = EC_KEY_new_by_curve_name(NID_secp256k1);
    BIGNUM *value = BN_bin2bn(private_key, 32, NULL);
    const EC_GROUP *group;
    EC_POINT *point = NULL;
    uint8_t public_key[65];
    int result = 1;
    if (key != NULL && value != NULL) {
        group = EC_KEY_get0_group(key);
        point = EC_POINT_new(group);
        if (point != NULL &&
            EC_POINT_mul(group, point, value, NULL, NULL, NULL) == 1 &&
            EC_POINT_point2oct(group, point, POINT_CONVERSION_UNCOMPRESSED,
                               public_key, sizeof(public_key), NULL) == 65U &&
            lxp_secp256k1_address(public_key, sizeof(public_key), signer) ==
                LXP_OK)
            result = 0;
    }
    EC_POINT_free(point);
    BN_free(value);
    EC_KEY_free(key);
    return result;
}

/* Signs the observation digest with a real secp256k1 key, normalizes s to
 * the low half and appends the Ethereum recovery byte the signer recovers
 * from. */
static int path_sign(const uint8_t private_key[32],
                     const uint8_t signer[LX_WEB_SIGNER_BYTES],
                     const uint8_t digest[32],
                     uint8_t signature[LX_WEB_SIGNATURE_BYTES])
{
    EC_KEY *key = EC_KEY_new_by_curve_name(NID_secp256k1);
    BIGNUM *value = BN_bin2bn(private_key, 32, NULL);
    BIGNUM *order = BN_new();
    BIGNUM *low = BN_new();
    ECDSA_SIG *signed_digest = NULL;
    const BIGNUM *r;
    const BIGNUM *s;
    uint8_t recovered[LX_WEB_SIGNER_BYTES];
    uint8_t recovery;
    int result = 1;
    if (key == NULL || value == NULL || order == NULL || low == NULL ||
        EC_KEY_set_private_key(key, value) != 1 ||
        EC_GROUP_get_order(EC_KEY_get0_group(key), order, NULL) != 1)
        goto done;
    signed_digest = ECDSA_do_sign(digest, 32, key);
    if (signed_digest == NULL) goto done;
    ECDSA_SIG_get0(signed_digest, &r, &s);
    if (BN_bn2binpad(r, signature, 32) != 32 ||
        BN_bn2binpad(s, signature + 32U, 32) != 32)
        goto done;
    if (!lxp_secp256k1_sig_is_low_s(signature)) {
        if (BN_sub(low, order, s) != 1 ||
            BN_bn2binpad(low, signature + 32U, 32) != 32)
            goto done;
    }
    for (recovery = 0U; recovery < 2U; ++recovery)
        if (lxp_secp256k1_recover_address(signature, recovery, digest,
                                          recovered) == LXP_OK &&
            memcmp(recovered, signer, LX_WEB_SIGNER_BYTES) == 0) {
            signature[64] = (uint8_t)(27U + recovery);
            result = 0;
            break;
        }
done:
    ECDSA_SIG_free(signed_digest);
    BN_free(low);
    BN_free(order);
    BN_free(value);
    EC_KEY_free(key);
    return result;
}

static lxp_result path_fee_account(const uint8_t asset[32], uint8_t id[32])
{
    static const uint8_t domain[] = "PAXEERX_WEB_FEES_V1";
    uint8_t preimage[sizeof(domain) - 1U + 32U];
    (void)memcpy(preimage, domain, sizeof(domain) - 1U);
    (void)memcpy(preimage + sizeof(domain) - 1U, asset, 32U);
    return lxp_hash_sha256(preimage, sizeof(preimage), id);
}

static void path_pending_key(const uint8_t program_id[32],
                             uint64_t request_id,
                             uint8_t key[PATH_PENDING_KEY_BYTES])
{
    (void)memcpy(key, "web/pending", 11U);
    (void)memcpy(key + 11U, program_id, 32U);
    write_u64(key + 43U, request_id);
}

static const lxp_module_kv_entry *path_committed(const lxp_kernel *kernel,
                                                 const uint8_t *key,
                                                 size_t key_length)
{
    size_t index;
    for (index = 0U; index < kernel->module_kv_count; ++index) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[index];
        if (entry->module_id == LXP_MODULE_PROGRAMS &&
            entry->key_length == key_length &&
            memcmp(entry->key, key, key_length) == 0)
            return entry;
    }
    return NULL;
}

static uint32_t path_read_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | (uint32_t)bytes[3];
}

/* Reads the web request record back from the call's full event list after
 * checking the list hashes to the call outcome event's envelope digest. */
static int path_event_list_request(const lxp_receipt *receipt,
                                   uint64_t *request_id, uint8_t *kind,
                                   lxp_byte_span *payload)
{
    static const uint8_t domain[] = "LayerX/programs/events/v1";
    const lxp_byte_span list =
        receipt->program_outcome.event_envelope_payload;
    const uint8_t *digest = NULL;
    uint8_t list_root[32];
    size_t cursor = sizeof(domain);
    size_t found = 0U;
    size_t index;
    uint32_t count;
    PATH_CHECK(list.bytes != NULL && list.length >= sizeof(domain) + 4U);
    for (index = 0U; index < receipt->effects.count; ++index) {
        const lxp_effect *effect = &receipt->effects.effects[index];
        if (effect->module_id != LXP_MODULE_PROGRAMS ||
            effect->kind != LXP_EFFECT_EVENT ||
            effect->event_type != LX_PROGRAMS_EVENT_CALL_OUTCOME)
            continue;
        PATH_CHECK(digest == NULL && effect->body_length == 255U);
        digest = effect->body + effect->body_length - 32U;
    }
    PATH_CHECK(digest != NULL);
    PATH_CHECK(lxp_hash_sha256(list.bytes, list.length, list_root) == LXP_OK &&
               memcmp(list_root, digest, 32U) == 0);
    PATH_CHECK(memcmp(list.bytes, domain, sizeof(domain)) == 0);
    count = path_read_u32(list.bytes + cursor);
    cursor += 4U;
    for (index = 0U; index < count; ++index) {
        uint32_t topic_length;
        uint32_t data_length;
        const uint8_t *topic;
        PATH_CHECK(list.length - cursor >= 32U + 32U + 8U + 1U + 4U);
        cursor += 32U + 32U + 8U + 1U;
        topic_length = path_read_u32(list.bytes + cursor);
        cursor += 4U;
        PATH_CHECK(list.length - cursor >= (size_t)topic_length + 4U);
        topic = list.bytes + cursor;
        cursor += topic_length;
        data_length = path_read_u32(list.bytes + cursor);
        cursor += 4U;
        PATH_CHECK(list.length - cursor >= data_length);
        if (topic_length == LX_WEB_REQUEST_TOPIC_BYTES &&
            memcmp(topic, LX_WEB_REQUEST_TOPIC,
                   LX_WEB_REQUEST_TOPIC_BYTES) == 0) {
            PATH_CHECK(lx_web_request_record_decode(list.bytes + cursor,
                                                    data_length, request_id,
                                                    kind, payload) == LXP_OK);
            ++found;
        }
        cursor += data_length;
    }
    PATH_CHECK(cursor == list.length && found == 1U);
    return 0;
}

static lx_account *path_account(lx_account_registry *accounts,
                                const uint8_t id[32])
{
    size_t index;
    for (index = 0U; index < accounts->count; ++index)
        if (memcmp(accounts->accounts[index].id, id, 32U) == 0)
            return &accounts->accounts[index];
    return NULL;
}

static size_t path_capabilities(uint8_t *out, const uint8_t asset[32],
                                const uint8_t to[32])
{
    size_t cursor = 0U;
    out[cursor++] = 0U;
    out[cursor++] = 2U;
    out[cursor++] = 3U;
    out[cursor++] = 5U;
    (void)memcpy(out + cursor, asset, 32U);
    cursor += 32U;
    (void)memcpy(out + cursor, to, 32U);
    cursor += 32U;
    (void)memset(out + cursor, 0, 16U);
    out[cursor + 15U] = PATH_FEE;
    return cursor + 16U;
}

static size_t path_request_calldata(uint8_t *out, uint64_t request_id,
                                    const uint8_t asset[32],
                                    const uint8_t to[32])
{
    size_t cursor = 0U;
    out[cursor++] = 1U;
    out[cursor++] = PATH_OP_REQUEST;
    write_u64(out + cursor, request_id);
    cursor += 8U;
    out[cursor++] = LX_WEB_KIND_FETCH;
    (void)memcpy(out + cursor, asset, 32U);
    cursor += 32U;
    (void)memcpy(out + cursor, to, 32U);
    cursor += 32U;
    (void)memset(out + cursor, 0, 16U);
    out[cursor + 15U] = PATH_FEE;
    cursor += 16U;
    (void)memcpy(out + cursor, path_payload, PATH_PAYLOAD_BYTES);
    return cursor + PATH_PAYLOAD_BYTES;
}

static size_t path_read_calldata(uint8_t *out, uint64_t request_id,
                                 const uint8_t digest[32])
{
    size_t cursor = 0U;
    out[cursor++] = 1U;
    out[cursor++] = PATH_OP_READ;
    write_u64(out + cursor, request_id);
    cursor += 8U;
    (void)memcpy(out + cursor, digest, 32U);
    cursor += 32U;
    write_u32(out + cursor, PATH_RESPONSE_BYTES);
    cursor += 4U;
    (void)memcpy(out + cursor, path_response, PATH_RESPONSE_BYTES);
    return cursor + PATH_RESPONSE_BYTES;
}

typedef struct path_chain {
    lxp_kernel kernel;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel_execution execution;
    lxp_identity_store identities;
    lxp_identity *identity;
    lxp_authority_resolved authority;
    lxp_authority_scope scope;
    lxp_fee_params fees;
    lx_account_registry accounts;
    lxp_transfer_asset_state asset_state;
    lx_programs_transfer_runtime runtime;
    lxp_arena arena;
    uint8_t arena_bytes[2U * LXP_MAX_ACTIVITY_BYTES + 4096U];
    uint8_t primary_key[32];
    uint8_t payer_id[32];
    uint8_t activity_ordinal;
} path_chain;

static const uint8_t path_did[] = "did:lxp:web-program-path";

static int path_call(path_chain *chain, const uint8_t program_id[32],
                     const uint8_t *capabilities, size_t capabilities_length,
                     const uint8_t *calldata, size_t calldata_length,
                     lxp_receipt *receipt)
{
    static const uint8_t absent_access[] =
        "LayerX/programs/access-declaration/v1\0";
    static uint8_t call[CALL_FIXED_BYTES + 1024U];
    lxp_activity activity;
    lx_account *payer;
    size_t length = call_payload_with_data(call, program_id, capabilities,
                                           capabilities_length, absent_access,
                                           sizeof(absent_access), calldata,
                                           calldata_length);
    write_u16(call + 32U, LX_PROGRAMS_GUEST_ABI_V4_VERSION);
    fill_activity(&activity, LX_PROGRAMS_CALL, call, length, path_did,
                  sizeof(path_did) - 1U, chain->primary_key);
    activity.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    activity.account_sequence = chain->identity->next_sequence;
    activity.idempotency_key[31] = ++chain->activity_ordinal;
    activity.fee_limit = (lxp_u128){0U, 67108864U};
    payer = path_account(&chain->accounts, chain->payer_id);
    PATH_CHECK(payer != NULL);
    chain->execution.fee_balance = payer->balance;
    chain->execution.global_sequence = chain->state.next_sequence;
    PATH_CHECK(lxp_arena_reset(&chain->arena, 0U) == LXP_OK);
    PATH_CHECK(execute_artifact_fixture_activity(&chain->kernel, &activity,
                                                 &chain->execution,
                                                 receipt) == LXP_OK);
    return 0;
}

static int web_program_path(const char *wasm_path, bool genesis_fee_account)
{
    static const uint8_t actor_name[] =
        "agent:did:lxp:web-program-path:main";
    static const uint8_t treasury_name[] = "system:fees";
    static const char *const payout_names[PATH_ATTESTORS] = {
        "agent:did:lxp:web-attestor-one:main",
        "agent:did:lxp:web-attestor-two:main",
        "agent:did:lxp:web-attestor-three:main"
    };
    static const uint8_t actor_seed[32] = {0x33U};
    static const uint8_t grant_id[32] = {0};
    static const uint8_t no_capabilities[] = {0U, 0U};
    static path_chain chain;
    static uint8_t wasm[PATH_WASM_CAPACITY];
    static uint8_t deploy[104U + PATH_WASM_CAPACITY];
    static uint8_t web_arena_bytes[65536];
    static lx_web_store store;
    static lx_web_store fresh_store;
    static lx_web_attestor_set attestors;
    static lx_web_observation observation;
    static lx_web_answer answer;
    static uint8_t observation_bytes[LX_WEB_OBSERVATION_MAX_BYTES];
    path_attestor signers[PATH_ATTESTORS];
    uint8_t capabilities[128];
    uint8_t calldata[PATH_CALLDATA_BYTES];
    uint8_t program_id[32];
    uint8_t code_hash[32];
    uint8_t fee_asset[32] = {9U};
    uint8_t fee_account_id[32];
    uint8_t actor_id[32];
    uint8_t treasury_id[32];
    uint8_t pending_key[PATH_PENDING_KEY_BYTES];
    uint8_t payload_hash[32];
    uint8_t digest[32];
    uint8_t amount_be[16];
    uint8_t attestor_bytes[LX_WEB_ATTESTOR_SET_MAX_BYTES];
    lxp_arena web_arena;
    lxp_module_ctx web_ctx;
    lxp_effect_buffer web_effects;
    lxp_authority_resolved governance;
    lxp_activity web_activity;
    const lxp_module_registration *web_registration;
    lxp_result web_result;
    lx_account *actor;
    lx_account *treasury;
    lx_account *fee_account;
    const lxp_module_kv_entry *entry;
    lxp_activity activity;
    lxp_receipt receipt;
    lxp_u128 actor_before;
    uint64_t parameters = 1U;
    uint64_t paid_sequence;
    size_t accounts_before;
    size_t wasm_length;
    size_t length;
    size_t index;
    size_t observation_length;
    size_t attestor_length;

    PATH_CHECK(path_read_file(wasm_path, wasm, sizeof(wasm),
                              &wasm_length) == 0);
    (void)memset(program_id, 0x71, sizeof(program_id));
    (void)memset(&chain, 0, sizeof(chain));
    PATH_CHECK(path_fee_account(fee_asset, fee_account_id) == LXP_OK);
    PATH_CHECK(lxp_keccak256(path_payload, PATH_PAYLOAD_BYTES,
                             payload_hash) == LXP_OK);
    PATH_CHECK(executed_public_key(actor_seed, chain.primary_key) == 0);
    PATH_CHECK(lx_account_registry_init(&chain.accounts) == LXP_OK);
    PATH_CHECK(lx_account_id_from_string(actor_name, sizeof(actor_name) - 1U,
                                         actor_id) == LXP_OK);
    (void)memcpy(chain.payer_id, actor_id, 32U);
    PATH_CHECK(lx_account_id_from_string(treasury_name,
                                         sizeof(treasury_name) - 1U,
                                         treasury_id) == LXP_OK);
    PATH_CHECK(lx_account_open(&chain.accounts, actor_name,
        sizeof(actor_name) - 1U, actor_id, 1U, LX_ACCOUNT_OPEN_GENESIS, NULL,
        &actor) == LXP_OK);
    PATH_CHECK(lx_account_open(&chain.accounts, treasury_name,
        sizeof(treasury_name) - 1U, treasury_id, 2U, LX_ACCOUNT_OPEN_GENESIS,
        NULL, &treasury) == LXP_OK);
    PATH_CHECK(lxp_ledger_bootstrap_balance(actor, fee_asset,
        (lxp_u128){0U, UINT64_MAX}, 1U) == LXP_OK);
    PATH_CHECK(lxp_ledger_bootstrap_balance(treasury, fee_asset,
        (lxp_u128){0U, 0U}, 0U) == LXP_OK);
    (void)memset(signers, 0, sizeof(signers));
    for (index = 0U; index < PATH_ATTESTORS; ++index) {
        size_t name_length = strlen(payout_names[index]);
        signers[index].private_key[31] = (uint8_t)(index + 1U);
        PATH_CHECK(path_signer(signers[index].private_key,
                               signers[index].signer) == 0);
        PATH_CHECK(lx_account_id_from_string(
                       (const uint8_t *)payout_names[index], name_length,
                       signers[index].payout) == LXP_OK);
        PATH_CHECK(lx_account_open(&chain.accounts,
                       (const uint8_t *)payout_names[index], name_length,
                       signers[index].payout, 3U + index,
                       LX_ACCOUNT_OPEN_GENESIS, NULL,
                       &signers[index].account) == LXP_OK);
    }
    /* The web fee account is a programs module value account holding the fee
     * asset. One run provisions it at genesis beside the payout accounts; the
     * other leaves it to the first paying call to create. */
    if (genesis_fee_account) {
        static const uint8_t programs_name[] = "programs";
        lx_account_registration fee_registration;
        bool fee_created = false;
        PATH_CHECK(lx_account_module_value_prepare(&chain.accounts,
                       programs_name, sizeof(programs_name) - 1U,
                       fee_account_id, fee_asset, 3U + PATH_ATTESTORS,
                       &fee_registration, &fee_account,
                       &fee_created) == LXP_OK &&
                   fee_created);
        PATH_CHECK(lx_account_registration_commit(&chain.accounts,
                       &fee_registration, &fee_account) == LXP_OK);
    }
    /* The registry may have moved while accounts were opened; resolve every
     * balance holder again before binding balances. */
    actor = path_account(&chain.accounts, actor_id);
    treasury = path_account(&chain.accounts, treasury_id);
    PATH_CHECK(actor != NULL && treasury != NULL);
    for (index = 0U; index < PATH_ATTESTORS; ++index) {
        signers[index].account = path_account(&chain.accounts,
                                               signers[index].payout);
        PATH_CHECK(signers[index].account != NULL);
        PATH_CHECK(lxp_ledger_bootstrap_balance(signers[index].account,
            fee_asset, (lxp_u128){0U, 0U}, 0U) == LXP_OK);
    }
    /* Signers sorted ascending, as the attestor set and intake require. */
    for (index = 1U; index < PATH_ATTESTORS; ++index) {
        size_t cursor = index;
        while (cursor > 0U &&
               memcmp(signers[cursor - 1U].signer, signers[cursor].signer,
                      LX_WEB_SIGNER_BYTES) > 0) {
            path_attestor swap = signers[cursor];
            signers[cursor] = signers[cursor - 1U];
            signers[cursor - 1U] = swap;
            --cursor;
        }
    }
    (void)memset(&attestors, 0, sizeof(attestors));
    for (index = 0U; index < PATH_ATTESTORS; ++index) {
        (void)memcpy(attestors.attestors[index].signer, signers[index].signer,
                     LX_WEB_SIGNER_BYTES);
        (void)memcpy(attestors.attestors[index].payout_account,
                     signers[index].payout, 32U);
    }
    attestors.count = PATH_ATTESTORS;
    attestors.threshold = PATH_THRESHOLD;
    PATH_CHECK(lx_web_attestor_set_validate(&attestors) == LXP_OK);

    PATH_CHECK(lxp_did_id_derive(path_did, sizeof(path_did) - 1U,
                                 chain.authority.principal) == LXP_OK);
    (void)memcpy(chain.authority.actor, chain.authority.principal, 32U);
    (void)memcpy(chain.authority.verified_key, chain.primary_key, 32U);
    chain.authority.kind = LXP_AUTHORITY_OWNER;
    chain.scope.module_mask = UINT64_C(1) << LXP_MODULE_PROGRAMS;
    chain.scope.activity_ordinal_min = 1U;
    chain.scope.activity_ordinal_max = 7U;
    chain.scope.maximum_per_activity = (lxp_u128){UINT64_MAX, UINT64_MAX};
    chain.scope.maximum_total = chain.scope.maximum_per_activity;
    chain.scope.maximum_per_period = chain.scope.maximum_per_activity;
    chain.authority.scope = &chain.scope;
    PATH_CHECK(lxp_authority_hash(chain.authority.kind, grant_id,
                                  chain.primary_key,
                                  chain.authority.authority_hash) == LXP_OK);
    (void)memcpy(chain.asset_state.asset_id, fee_asset, 32U);
    chain.asset_state.registered = true;
    chain.runtime.accounts = &chain.accounts;
    chain.runtime.assets = &chain.asset_state;
    chain.runtime.asset_count = 1U;
    chain.runtime.fee_schedule = (lx_programs_fee_schedule){
        1U, 1U, 1U, 2U, 4U, 1U, 1U, 1U
    };
    chain.runtime.resolve_metering_schedule =
        lxp_programs_metering_resolve_runtime;
    chain.runtime.metering_schedule_context = &chain.kernel;
    (void)memcpy(chain.runtime.occupancy_asset_id, fee_asset, 32U);
    chain.runtime.resolve_occupancy_parameters = occupancy_parameters;
    chain.runtime.occupancy_parameter_context = &chain.runtime;
    chain.fees.version = 1U;
    chain.fees.multiplier_basis_points = 10000U;
    PATH_CHECK(lxp_state_store_init(&chain.state, 1U) == LXP_OK);
    PATH_CHECK(lxp_identity_register(&chain.identities, path_did,
                                     sizeof(path_did) - 1U,
                                     chain.primary_key,
                                     &chain.identity) == LXP_OK);
    PATH_CHECK(lxp_kernel_create(&chain.kernel, &chain.state, &chain.journal,
                                 &parameters, 0U) == LXP_OK);
    PATH_CHECK(install_metering_v1(&chain.kernel) == LXP_OK);
    PATH_CHECK(lxp_kernel_register_module(&chain.kernel,
        programs_module_registration_v4()) == LXP_OK);
    PATH_CHECK(lxp_kernel_bind_module_runtime(&chain.kernel,
        LXP_MODULE_PROGRAMS, &chain.runtime) == LXP_OK);
    PATH_CHECK(lxp_kernel_register_module(&chain.kernel,
        lx_web_module_iface()) == LXP_OK);
    (void)memset(&store, 0, sizeof(store));
    store.network_id = PATH_NETWORK;
    PATH_CHECK(lxp_kernel_bind_module_runtime(&chain.kernel, LXP_MODULE_WEB,
                                              &store) == LXP_OK);
    PATH_CHECK(lxp_programs_bind_fee_transaction(&chain.kernel) == LXP_OK);
    PATH_CHECK(lxp_kernel_set_capabilities(&chain.kernel, NULL,
        lxp_kernel_canonical_ledger_apply) == LXP_OK);
    PATH_CHECK(lxp_state_root(&chain.kernel,
                              chain.kernel.current_state_root) == LXP_OK);
    PATH_CHECK(lxp_arena_init(&chain.arena, chain.arena_bytes,
                              sizeof(chain.arena_bytes)) == LXP_OK);
    chain.execution.network_id = PATH_NETWORK;
    chain.execution.batch_number = 1U;
    chain.execution.batch_timestamp_ms = 10U;
    chain.execution.maximum_timestamp_window = 100U;
    chain.execution.recorded_module_version =
        LX_PROGRAMS_SANDBOX_DESTROY_ABI_VERSION;
    chain.execution.parameter_version = 1U;
    chain.execution.signature_valid = true;
    chain.execution.identities = &chain.identities;
    chain.execution.authority = &chain.authority;
    chain.execution.fee_parameters = &chain.fees;
    chain.execution.gas_limit = 1000000U;
    chain.execution.arena = &chain.arena;

    /* Deploy the built web-reader reference program at ABI v4. */
    length = program_spend_deploy_payload(deploy, program_id,
                                          chain.authority.principal, wasm,
                                          wasm_length, code_hash);
    write_u16(deploy + 32U, LX_PROGRAMS_GUEST_ABI_V4_VERSION);
    fill_activity(&activity, LX_PROGRAMS_DEPLOY, deploy, length, path_did,
                  sizeof(path_did) - 1U, chain.primary_key);
    activity.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    activity.account_sequence = chain.identity->next_sequence;
    activity.idempotency_key[31] = ++chain.activity_ordinal;
    chain.execution.global_sequence = chain.state.next_sequence;
    PATH_CHECK(lxp_arena_reset(&chain.arena, 0U) == LXP_OK);
    PATH_CHECK(execute_artifact_fixture_activity(&chain.kernel, &activity,
                                                 &chain.execution,
                                                 &receipt) == LXP_OK);
    PATH_CHECK(receipt.result_code == LXP_OK);
    actor = path_account(&chain.accounts, actor_id);
    PATH_CHECK(actor != NULL);

    /* A request whose payment goes anywhere but the web fee account is
     * refused and records nothing. */
    length = path_capabilities(capabilities, fee_asset, treasury_id);
    PATH_CHECK(path_call(&chain, program_id, capabilities, length, calldata,
                         path_request_calldata(calldata, path_unpaid_request,
                                               fee_asset, treasury_id),
                         &receipt) == 0);
    PATH_CHECK(receipt.result_code == LXP_ERR_PROGRAM_REFUSED);
    path_pending_key(program_id, path_unpaid_request, pending_key);
    PATH_CHECK(path_committed(&chain.kernel, pending_key,
                              sizeof(pending_key)) == NULL);

    /* Reading before any answer is committed refuses the call. */
    (void)memset(digest, 0x5a, sizeof(digest));
    PATH_CHECK(path_call(&chain, program_id, no_capabilities,
                         sizeof(no_capabilities), calldata,
                         path_read_calldata(calldata, path_request, digest),
                         &receipt) == 0);
    PATH_CHECK(receipt.result_code == LXP_ERR_PROGRAM_REFUSED);

    /* The paid request: transfer_402 into the fee account and the request
     * record in one call stage the pending record. */
    actor = path_account(&chain.accounts, actor_id);
    PATH_CHECK(actor != NULL);
    actor_before = actor->balance;
    PATH_CHECK((path_account(&chain.accounts, fee_account_id) != NULL) ==
               genesis_fee_account);
    accounts_before = chain.accounts.count;
    paid_sequence = chain.state.next_sequence;
    length = path_capabilities(capabilities, fee_asset, fee_account_id);
    PATH_CHECK(path_call(&chain, program_id, capabilities, length, calldata,
                         path_request_calldata(calldata, path_request,
                                               fee_asset, fee_account_id),
                         &receipt) == 0);
    PATH_CHECK(receipt.result_code == LXP_OK);
    fee_account = path_account(&chain.accounts, fee_account_id);
    PATH_CHECK(fee_account != NULL &&
               fee_account->kind == LX_ACCOUNT_MODULE_VALUE &&
               fee_account->has_asset &&
               memcmp(fee_account->asset_id, fee_asset, 32U) == 0 &&
               fee_account->balance.hi == 0U &&
               fee_account->balance.lo == PATH_FEE);
    if (genesis_fee_account) {
        PATH_CHECK(chain.accounts.count == accounts_before);
    } else {
        PATH_CHECK(chain.accounts.count == accounts_before + 1U &&
                   fee_account->created_at_sequence == paid_sequence);
    }
    actor = path_account(&chain.accounts, actor_id);
    PATH_CHECK(actor != NULL && actor->balance.lo < actor_before.lo);
    path_pending_key(program_id, path_request, pending_key);
    entry = path_committed(&chain.kernel, pending_key, sizeof(pending_key));
    PATH_CHECK(entry != NULL &&
               entry->value_length == PATH_PENDING_RECORD_BYTES);
    PATH_CHECK(entry->value[0] == 1U && entry->value[1] == LX_WEB_KIND_FETCH &&
               memcmp(entry->value + 2U, payload_hash, 32U) == 0 &&
               memcmp(entry->value + 34U, fee_asset, 32U) == 0 &&
               memcmp(entry->value + 66U, fee_account_id, 32U) == 0 &&
               entry->value[122] == 0U);
    (void)memset(amount_be, 0, sizeof(amount_be));
    amount_be[15] = PATH_FEE;
    PATH_CHECK(memcmp(entry->value + 98U, amount_be, 16U) == 0);
    {
        /* The request call's full event list carries the record's raw bytes,
         * and its payload hashes to the pending request's payload hash. */
        uint64_t listed_request = 0U;
        uint8_t listed_kind = 0U;
        lxp_byte_span listed_payload = {NULL, 0U};
        uint8_t listed_hash[32];
        PATH_CHECK(path_event_list_request(&receipt, &listed_request,
                                           &listed_kind,
                                           &listed_payload) == 0);
        PATH_CHECK(listed_request == path_request &&
                   listed_kind == LX_WEB_KIND_FETCH &&
                   listed_payload.length == PATH_PAYLOAD_BYTES &&
                   memcmp(listed_payload.bytes, path_payload,
                          PATH_PAYLOAD_BYTES) == 0);
        PATH_CHECK(lxp_keccak256(listed_payload.bytes, listed_payload.length,
                                 listed_hash) == LXP_OK &&
                   memcmp(listed_hash, entry->value + 2U, 32U) == 0);
    }

    /* The same request id again is refused while it is pending. */
    PATH_CHECK(path_call(&chain, program_id, capabilities, length, calldata,
                         path_request_calldata(calldata, path_request,
                                               fee_asset, fee_account_id),
                         &receipt) == 0);
    PATH_CHECK(receipt.result_code == LXP_ERR_PROGRAM_REFUSED);
    fee_account = path_account(&chain.accounts, fee_account_id);
    PATH_CHECK(fee_account != NULL && fee_account->balance.lo == PATH_FEE);

    /* Attestation intake in the programs module context: the store has
     * never seen the request, so the recorded request is what admits it. */
    (void)memset(&observation, 0, sizeof(observation));
    observation.origin = LX_WEB_ORIGIN_PROGRAM;
    observation.network_id = PATH_NETWORK;
    (void)memcpy(observation.program_id, program_id, 32U);
    observation.request_id = path_request;
    observation.kind = LX_WEB_KIND_FETCH;
    (void)memcpy(observation.payload_hash, payload_hash, 32U);
    (void)memcpy(observation.content_digest, digest, 32U);
    observation.full_length = PATH_RESPONSE_BYTES;
    observation.response_length = PATH_RESPONSE_BYTES;
    (void)memcpy(observation.response, path_response, PATH_RESPONSE_BYTES);
    PATH_CHECK(lx_web_observation_digest(&observation, digest) == LXP_OK);
    observation.signature_count = PATH_ATTESTORS;
    for (index = 0U; index < PATH_ATTESTORS; ++index)
        PATH_CHECK(path_sign(signers[index].private_key,
                             signers[index].signer, digest,
                             observation.signatures[index]) == 0);
    PATH_CHECK(lx_web_observation_encode(&observation, observation_bytes,
                                         sizeof(observation_bytes),
                                         &observation_length) == LXP_OK);

    /* The attestor set is registered by governance through dispatch before
     * any observation is delivered. */
    chain.kernel.handover.enabled = true;
    (void)memset(chain.kernel.handover.governance_public_key, 0x42, 32U);
    (void)memset(&governance, 0, sizeof(governance));
    governance.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(governance.verified_key,
                 chain.kernel.handover.governance_public_key, 32U);
    PATH_CHECK(lx_web_attestor_set_encode(&attestors, attestor_bytes,
                                          sizeof(attestor_bytes),
                                          &attestor_length) == LXP_OK);
    (void)memset(&web_activity, 0, sizeof(web_activity));
    web_activity.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    web_activity.network_id = PATH_NETWORK;
    web_activity.activity_type = LX_WEB_ATTESTOR_SET_ACTIVITY;
    web_activity.payload = (lxp_byte_span){attestor_bytes, attestor_length};
    PATH_CHECK(lxp_hash_payload(attestor_bytes, attestor_length,
                                web_activity.payload_hash) == LXP_OK);
    PATH_CHECK(lxp_kernel_module_for_activity(&chain.kernel,
        LX_WEB_ATTESTOR_SET_ACTIVITY, chain.kernel.epoch,
        &web_registration) == LXP_OK);
    PATH_CHECK(lxp_state_journal_open(&chain.state, chain.state.next_sequence,
                                      &chain.journal) == LXP_OK);
    PATH_CHECK(lxp_arena_init(&web_arena, web_arena_bytes,
                              sizeof(web_arena_bytes)) == LXP_OK);
    PATH_CHECK(lxp_module_ctx_init(&web_ctx, &chain.kernel,
        LXP_MODULE_PROGRAMS, 10U, 0U, chain.state.next_sequence, 100000U,
        &web_arena, true) == LXP_OK);
    web_ctx.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    PATH_CHECK(lxp_effect_buffer_init(&web_effects) == LXP_OK);
    PATH_CHECK(lxp_module_ctx_bind_effects(&web_ctx, &web_effects) == LXP_OK);
    PATH_CHECK(lxp_kernel_dispatch(web_registration, &web_ctx, &web_activity,
                                   &governance, &web_effects,
                                   &web_result) == LXP_OK &&
               web_result == LXP_OK);
    PATH_CHECK(lxp_module_ctx_prepare_commit(&web_ctx) == LXP_OK);
    PATH_CHECK(lxp_state_journal_commit(&chain.journal) == LXP_OK);
    PATH_CHECK(lxp_module_ctx_commit(&web_ctx) == LXP_OK);

    /* The observation activity is delivered through kernel dispatch in the
     * programs module context. */
    web_activity.activity_type = LX_WEB_OBSERVATION_ACTIVITY;
    web_activity.payload = (lxp_byte_span){observation_bytes,
                                           observation_length};
    PATH_CHECK(lxp_hash_payload(observation_bytes, observation_length,
                                web_activity.payload_hash) == LXP_OK);
    PATH_CHECK(lxp_kernel_module_for_activity(&chain.kernel,
        LX_WEB_OBSERVATION_ACTIVITY, chain.kernel.epoch,
        &web_registration) == LXP_OK);
    PATH_CHECK(lxp_state_journal_open(&chain.state, chain.state.next_sequence,
                                      &chain.journal) == LXP_OK);
    PATH_CHECK(lxp_arena_init(&web_arena, web_arena_bytes,
                              sizeof(web_arena_bytes)) == LXP_OK);
    PATH_CHECK(lxp_module_ctx_init(&web_ctx, &chain.kernel,
        LXP_MODULE_PROGRAMS, 10U, 0U, chain.state.next_sequence, 100000U,
        &web_arena, true) == LXP_OK);
    web_ctx.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    PATH_CHECK(lxp_effect_buffer_init(&web_effects) == LXP_OK);
    PATH_CHECK(lxp_module_ctx_bind_effects(&web_ctx, &web_effects) == LXP_OK);
    PATH_CHECK(lxp_kernel_dispatch(web_registration, &web_ctx, &web_activity,
                                   &chain.authority, &web_effects,
                                   &web_result) == LXP_OK &&
               web_result == LXP_OK);
    PATH_CHECK(store.committed_count == 1U &&
               store.committed[0].signer_count == PATH_ATTESTORS &&
               store.pending_count == 1U && store.pending[0].fulfilled &&
               store.committed_count == 1U);
    PATH_CHECK(lxp_module_ctx_prepare_commit(&web_ctx) == LXP_OK);
    PATH_CHECK(lxp_state_journal_commit(&chain.journal) == LXP_OK);
    PATH_CHECK(lxp_module_ctx_commit(&web_ctx) == LXP_OK);
    PATH_CHECK(lxp_state_root(&chain.kernel,
                              chain.kernel.current_state_root) == LXP_OK);
    /* The batch header carries the root of the committed observation. */
    {
        static lx_batch_header header;
        uint8_t web_root[32];
        PATH_CHECK(lxp_arena_init(&web_arena, web_arena_bytes,
                                  sizeof(web_arena_bytes)) == LXP_OK);
        PATH_CHECK(lx_web_root(&store, &web_arena, web_root) == LXP_OK);
        PATH_CHECK(lxp_arena_reset(&web_arena, 0U) == LXP_OK);
        PATH_CHECK(lx_batch_header_set_web_root(&header,
            (const lx_web_store *)chain.kernel.module_runtime[LXP_MODULE_WEB],
            &web_arena) == LXP_OK);
        PATH_CHECK(memcmp(header.web_root, web_root, 32U) == 0);
    }

    /* The fee split equally, the remainder to the lowest signer. */
    fee_account = path_account(&chain.accounts, fee_account_id);
    PATH_CHECK(fee_account != NULL && fee_account->balance.hi == 0U &&
               fee_account->balance.lo == 0U);
    for (index = 0U; index < PATH_ATTESTORS; ++index) {
        lx_account *payout = path_account(&chain.accounts,
                                          signers[index].payout);
        uint64_t expected = PATH_FEE / PATH_ATTESTORS +
            (index == 0U ? PATH_FEE % PATH_ATTESTORS : 0U);
        PATH_CHECK(payout != NULL && payout->balance.hi == 0U &&
                   payout->balance.lo == expected);
    }
    entry = path_committed(&chain.kernel, pending_key, sizeof(pending_key));
    PATH_CHECK(entry != NULL && entry->value[122] == 1U);

    /* The answer committed at intake reads back through web_read in the
     * program, and through the committed reader. */
    (void)memset(digest, 0x5a, sizeof(digest));
    PATH_CHECK(path_call(&chain, program_id, no_capabilities,
                         sizeof(no_capabilities), calldata,
                         path_read_calldata(calldata, path_request, digest),
                         &receipt) == 0);
    PATH_CHECK(receipt.result_code == LXP_OK);
    PATH_CHECK(lxp_arena_init(&web_arena, web_arena_bytes,
                              sizeof(web_arena_bytes)) == LXP_OK);
    PATH_CHECK(lxp_module_ctx_init(&web_ctx, &chain.kernel,
        LXP_MODULE_PROGRAMS, 10U, 0U, chain.state.next_sequence, 100000U,
        &web_arena, false) == LXP_OK);
    PATH_CHECK(lx_web_committed_read(&web_ctx, program_id, path_request,
                                     &answer) == LXP_OK);
    PATH_CHECK(answer.full_length == PATH_RESPONSE_BYTES &&
               answer.response_length == PATH_RESPONSE_BYTES &&
               memcmp(answer.content_digest, digest, 32U) == 0 &&
               memcmp(answer.response, path_response,
                      PATH_RESPONSE_BYTES) == 0);

    /* A replay against a store that never tracked the request is refused by
     * the fulfilled record alone. */
    (void)memset(&fresh_store, 0, sizeof(fresh_store));
    fresh_store.network_id = PATH_NETWORK;
    PATH_CHECK(lxp_kernel_bind_module_runtime(&chain.kernel, LXP_MODULE_WEB,
                                              &fresh_store) == LXP_OK);
    PATH_CHECK(lxp_state_journal_open(&chain.state, chain.state.next_sequence,
                                      &chain.journal) == LXP_OK);
    PATH_CHECK(lxp_arena_init(&web_arena, web_arena_bytes,
                              sizeof(web_arena_bytes)) == LXP_OK);
    PATH_CHECK(lxp_module_ctx_init(&web_ctx, &chain.kernel,
        LXP_MODULE_PROGRAMS, 10U, 0U, chain.state.next_sequence, 100000U,
        &web_arena, true) == LXP_OK);
    web_ctx.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    PATH_CHECK(lxp_module_ctx_bind_effects(&web_ctx, &web_effects) == LXP_OK);
    PATH_CHECK(lxp_kernel_dispatch(web_registration, &web_ctx, &web_activity,
                                   &chain.authority, &web_effects,
                                   &web_result) == LXP_OK &&
               web_result == LXP_ERR_SEQUENCE_REUSED);
    /* An observation for a request no call recorded is unknown. */
    observation.request_id = path_unpaid_request;
    PATH_CHECK(lx_web_observation_encode(&observation, observation_bytes,
                                         sizeof(observation_bytes),
                                         &observation_length) == LXP_OK);
    web_activity.payload = (lxp_byte_span){observation_bytes,
                                           observation_length};
    PATH_CHECK(lxp_hash_payload(observation_bytes, observation_length,
                                web_activity.payload_hash) == LXP_OK);
    PATH_CHECK(lxp_kernel_dispatch(web_registration, &web_ctx, &web_activity,
                                   &chain.authority, &web_effects,
                                   &web_result) == LXP_OK &&
               web_result == LXP_ERR_UNKNOWN_FIELD);
    PATH_CHECK(fresh_store.pending_count == 0U &&
               fresh_store.committed_count == 0U);
    PATH_CHECK(lxp_state_store_destroy(&chain.state) == LXP_OK);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc != 2) {
        (void)fprintf(stderr, "usage: %s web-reader.wasm\n", argv[0]);
        return 2;
    }
    if (web_program_path(argv[1], true) != 0) return 1;
    return web_program_path(argv[1], false);
}
