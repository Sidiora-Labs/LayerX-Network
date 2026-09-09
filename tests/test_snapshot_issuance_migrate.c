#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_genesis_builder.h"

#include "layerx/lx_asset.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_snapshot.h"
#include "layerx/lxp_state.h"
#include "layerx/programs.h"

#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define REQUIRE(condition) \
    do { \
        if (!(condition)) { \
            fprintf(stderr, "issuance migrate line %d: %s\n", \
                    __LINE__, #condition); \
            return 1; \
        } \
    } while (0)

enum { FIXTURE_ARENA_BYTES = 4 * 1024 * 1024 };

static int write_exclusive_bytes(const char *path, const uint8_t *bytes,
                                 size_t length)
{
    int descriptor = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    int failed = descriptor < 0;
    if (!failed && write(descriptor, bytes, length) != (ssize_t)length)
        failed = 1;
    if (!failed && fsync(descriptor) != 0) failed = 1;
    if (descriptor >= 0 && close(descriptor) != 0) failed = 1;
    return failed;
}

static int read_exact(const char *path, uint8_t *bytes, size_t length)
{
    int descriptor = open(path, O_RDONLY | O_CLOEXEC);
    ssize_t count;
    if (descriptor < 0) return 1;
    count = read(descriptor, bytes, length);
    if (count != (ssize_t)length || close(descriptor) != 0) return 1;
    return 0;
}

static void retired_issuance_name(const uint8_t asset_id[32], uint8_t name[79])
{
    static const uint8_t hex[] = "0123456789abcdef";
    size_t index;
    (void)memcpy(name, "asset:", 6U);
    for (index = 0U; index < 32U; ++index) {
        name[6U + index * 2U] = hex[asset_id[index] >> 4U];
        name[7U + index * 2U] = hex[asset_id[index] & 15U];
    }
    (void)memcpy(name + 70U, ":issuance", 9U);
}

static int fill_retired_issuance(lx_account *account, const uint8_t asset_id[32],
                                 lxp_u128 balance)
{
    uint8_t current_name[LX_ASSET_ISSUANCE_NAME_BYTES];
    uint8_t expected_id[32];
    uint8_t retired[79];
    (void)memset(account, 0, sizeof(*account));
    retired_issuance_name(asset_id, retired);
    if (lx_asset_issuance_name(asset_id, current_name, expected_id) != LXP_OK)
        return 1;
    (void)memcpy(account->name, retired, sizeof(retired));
    account->name_length = (uint16_t)sizeof(retired);
    (void)memcpy(account->id, expected_id, sizeof(expected_id));
    account->kind = LX_ACCOUNT_MODULE_VALUE;
    account->balance = balance;
    (void)memcpy(account->asset_id, asset_id, 32U);
    account->has_asset = true;
    account->next_sequence = 4U;
    account->created_at_sequence = 2U;
    return lx_account_validate_canonical(account) == LXP_OK ? 0 : 1;
}

static int open_system_fees(lx_account_registry *accounts,
                            const uint8_t asset_id[32])
{
    static const uint8_t name[] = "system:fees";
    lx_account *account;
    uint8_t id[32];
    if (lx_account_id_from_string(name, sizeof(name) - 1U, id) != LXP_OK ||
        lx_account_open(accounts, name, sizeof(name) - 1U, id, 1U,
                        LX_ACCOUNT_OPEN_GENESIS, NULL, &account) != LXP_OK ||
        lxp_ledger_bootstrap_balance(account, asset_id, (lxp_u128){0U, 7U},
                                     1U) != LXP_OK)
        return 1;
    return 0;
}

static int write_occupancy_snapshot(lx_account_registry *accounts,
                                    const char *directory,
                                    lxp_snapshot_manifest_record *manifest)
{
    static uint8_t arena_bytes[FIXTURE_ARENA_BYTES];
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_arena arena;
    lxp_byte_span snapshot;
    uint64_t parameters = 1U;
    uint8_t canonical[32];
    (void)memset(&state, 0, sizeof(state));
    (void)memset(&journal, 0, sizeof(journal));
    (void)memset(&kernel, 0, sizeof(kernel));
    if (lxp_state_store_init(&state, 1U) != LXP_OK ||
        lxp_state_store_bind_accounts(&state, accounts) != LXP_OK ||
        lxp_state_store_require_account_root(&state) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 1U) !=
            LXP_OK ||
        lxp_kernel_register_module(&kernel,
                                   programs_module_registration_v4()) !=
            LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_asset_module_iface()) !=
            LXP_OK ||
        lxp_state_root(&kernel, canonical) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_snapshot_write(&kernel, 0U, &arena, &snapshot) != LXP_OK ||
        lxp_snapshot_manifest_build(snapshot.bytes, snapshot.length, 0U,
                                    canonical, canonical, manifest) !=
            LXP_OK ||
        lxp_snapshot_store_write(directory, manifest, snapshot.bytes,
                                 snapshot.length) != LXP_OK ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}

static int load_occupancy_snapshot(const char *path, lx_account_registry *accounts,
                                   lxp_snapshot_manifest_record *manifest)
{
    static uint8_t arena_bytes[FIXTURE_ARENA_BYTES];
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_arena arena;
    lxp_byte_span snapshot;
    uint64_t parameters = 1U;
    (void)memset(&state, 0, sizeof(state));
    (void)memset(&journal, 0, sizeof(journal));
    (void)memset(&kernel, 0, sizeof(kernel));
    if (lx_account_registry_init(accounts) != LXP_OK ||
        lxp_state_store_init(&state, 1U) != LXP_OK ||
        lxp_state_store_bind_accounts(&state, accounts) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 1U) !=
            LXP_OK ||
        lxp_kernel_register_module(&kernel,
                                   programs_module_registration_v4()) !=
            LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_asset_module_iface()) !=
            LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_snapshot_store_read(path, &arena, manifest, &snapshot) != LXP_OK ||
        lxp_snapshot_load(snapshot.bytes, snapshot.length, manifest,
                          &kernel) != LXP_OK ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}

static int signed_retired_fixture(void)
{
    static const uint8_t signer_key[32] = {7U};
    uint8_t asset_id[32];
    uint8_t current_name[LX_ASSET_ISSUANCE_NAME_BYTES];
    uint8_t expected_id[32];
    uint8_t authorization[LXP_ISSUANCE_MIGRATION_BYTES];
    uint8_t tampered[LXP_ISSUANCE_MIGRATION_BYTES];
    lx_account_registry source_accounts;
    lx_account_registry migrated_accounts;
    lxp_snapshot_manifest_record old_manifest;
    lxp_snapshot_manifest_record new_manifest;
    char base[] = "/tmp/lxp-issuance-migrate-XXXXXX";
    char source_dir[160];
    char source_path[192];
    char key_path[160];
    char output_dir[160];
    char migrated_path[192];
    char authorization_path[192];
    char *argv[6];
    size_t index;
    (void)memset(asset_id, 0x11, sizeof(asset_id));
    REQUIRE(lx_asset_issuance_name(asset_id, current_name, expected_id) ==
            LXP_OK);
    REQUIRE(mkdtemp(base) != NULL);
    REQUIRE(snprintf(source_dir, sizeof(source_dir), "%s/source", base) > 0);
    REQUIRE(snprintf(source_path, sizeof(source_path),
                     "%s/00000000000000000000.lxs", source_dir) > 0);
    REQUIRE(snprintf(key_path, sizeof(key_path), "%s/signer.key", base) > 0);
    REQUIRE(snprintf(output_dir, sizeof(output_dir), "%s/migrated", base) > 0);
    REQUIRE(snprintf(migrated_path, sizeof(migrated_path),
                     "%s/00000000000000000000.lxs", output_dir) > 0);
    REQUIRE(snprintf(authorization_path, sizeof(authorization_path),
                     "%s/issuance-migration.lxim", output_dir) > 0);
    REQUIRE(mkdir(source_dir, 0700) == 0);
    REQUIRE(lx_account_registry_init(&source_accounts) == LXP_OK);
    REQUIRE(fill_retired_issuance(&source_accounts.accounts[0], asset_id,
                                  (lxp_u128){0U, 100U}) == 0);
    source_accounts.count = 1U;
    REQUIRE(open_system_fees(&source_accounts, asset_id) == 0);
    REQUIRE(write_occupancy_snapshot(&source_accounts, source_dir,
                                     &old_manifest) == 0);
    REQUIRE(write_exclusive_bytes(key_path, signer_key, sizeof(signer_key)) ==
            0);
    argv[0] = (char *)"layerx-genesis-build";
    argv[1] = (char *)"--migrate-issuance-names";
    argv[2] = source_path;
    argv[3] = key_path;
    argv[4] = output_dir;
    argv[5] = NULL;
    REQUIRE(lxp_genesis_builder_cli_main(5, argv) == 0);
    REQUIRE(load_occupancy_snapshot(migrated_path, &migrated_accounts,
                                    &new_manifest) == 0);
    REQUIRE(migrated_accounts.count == 2U);
    REQUIRE(memcmp(old_manifest.snapshot_digest, new_manifest.snapshot_digest,
                   32U) != 0);
    REQUIRE(memcmp(old_manifest.canonical_state_root,
                   new_manifest.canonical_state_root, 32U) != 0);
    for (index = 0U; index < migrated_accounts.count; ++index) {
        const lx_account *account = &migrated_accounts.accounts[index];
        if (memcmp(account->id, expected_id, 32U) == 0) {
            REQUIRE(account->name_length == LX_ASSET_ISSUANCE_NAME_BYTES);
            REQUIRE(memcmp(account->name, current_name,
                           LX_ASSET_ISSUANCE_NAME_BYTES) == 0);
            REQUIRE(lxp_u128_cmp(account->balance, (lxp_u128){0U, 100U}) == 0);
            REQUIRE(memcmp(account->asset_id, asset_id, 32U) == 0);
            REQUIRE(account->kind == LX_ACCOUNT_MODULE_VALUE);
            REQUIRE(account->next_sequence == 4U);
            REQUIRE(account->created_at_sequence == 2U);
            REQUIRE(!account->frozen);
            REQUIRE(!account->has_authority_key);
        } else {
            REQUIRE(account->kind == LX_ACCOUNT_SYSTEM_FEES);
            REQUIRE(lxp_u128_cmp(account->balance, (lxp_u128){0U, 7U}) == 0);
        }
    }
    REQUIRE(read_exact(authorization_path, authorization,
                       sizeof(authorization)) == 0);
    REQUIRE(lxp_genesis_issuance_migration_verify(
                authorization, old_manifest.global_sequence,
                old_manifest.snapshot_digest, old_manifest.canonical_state_root,
                old_manifest.receipt_state_root, new_manifest.snapshot_digest,
                new_manifest.canonical_state_root,
                new_manifest.receipt_state_root) == LXP_OK);
    (void)memcpy(tampered, authorization, sizeof(authorization));
    tampered[237] ^= 1U;
    REQUIRE(lxp_genesis_issuance_migration_verify(
                tampered, old_manifest.global_sequence,
                old_manifest.snapshot_digest, old_manifest.canonical_state_root,
                old_manifest.receipt_state_root, new_manifest.snapshot_digest,
                new_manifest.canonical_state_root,
                new_manifest.receipt_state_root) != LXP_OK);
    REQUIRE(lxp_genesis_issuance_migration_verify(
                authorization, old_manifest.global_sequence + 1U,
                old_manifest.snapshot_digest, old_manifest.canonical_state_root,
                old_manifest.receipt_state_root, new_manifest.snapshot_digest,
                new_manifest.canonical_state_root,
                new_manifest.receipt_state_root) == LXP_ERR_ROOT_MISMATCH);
    REQUIRE(unlink(migrated_path) == 0);
    REQUIRE(unlink(authorization_path) == 0);
    REQUIRE(rmdir(output_dir) == 0);
    REQUIRE(unlink(source_path) == 0);
    REQUIRE(rmdir(source_dir) == 0);
    REQUIRE(unlink(key_path) == 0);
    REQUIRE(rmdir(base) == 0);
    return 0;
}

static int refuse_without_retired_name(void)
{
    static const uint8_t signer_key[32] = {8U};
    uint8_t asset_id[32];
    lx_account_registry accounts;
    lxp_snapshot_manifest_record manifest;
    char base[] = "/tmp/lxp-issuance-refuse-XXXXXX";
    char source_dir[160];
    char source_path[192];
    char key_path[160];
    char output_dir[160];
    char *argv[6];
    (void)memset(asset_id, 0x22, sizeof(asset_id));
    REQUIRE(mkdtemp(base) != NULL);
    REQUIRE(snprintf(source_dir, sizeof(source_dir), "%s/source", base) > 0);
    REQUIRE(snprintf(source_path, sizeof(source_path),
                     "%s/00000000000000000000.lxs", source_dir) > 0);
    REQUIRE(snprintf(key_path, sizeof(key_path), "%s/signer.key", base) > 0);
    REQUIRE(snprintf(output_dir, sizeof(output_dir), "%s/migrated", base) > 0);
    REQUIRE(mkdir(source_dir, 0700) == 0);
    REQUIRE(lx_account_registry_init(&accounts) == LXP_OK);
    REQUIRE(open_system_fees(&accounts, asset_id) == 0);
    REQUIRE(write_occupancy_snapshot(&accounts, source_dir, &manifest) == 0);
    REQUIRE(write_exclusive_bytes(key_path, signer_key, sizeof(signer_key)) ==
            0);
    argv[0] = (char *)"layerx-genesis-build";
    argv[1] = (char *)"--migrate-issuance-names";
    argv[2] = source_path;
    argv[3] = key_path;
    argv[4] = output_dir;
    argv[5] = NULL;
    REQUIRE(lxp_genesis_builder_cli_main(5, argv) != 0);
    REQUIRE(access(output_dir, F_OK) != 0);
    REQUIRE(unlink(source_path) == 0);
    REQUIRE(rmdir(source_dir) == 0);
    REQUIRE(unlink(key_path) == 0);
    REQUIRE(rmdir(base) == 0);
    return 0;
}

int main(void)
{
    REQUIRE(signed_retired_fixture() == 0);
    REQUIRE(refuse_without_retired_name() == 0);
    (void)puts("issuance snapshot migration: signed fixture and refusals passed");
    return 0;
}
