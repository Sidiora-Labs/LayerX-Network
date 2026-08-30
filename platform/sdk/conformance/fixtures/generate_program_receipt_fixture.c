#include "layerx/lxp_crypto.h"
#include "layerx/lxp_module.h"
#include "layerx/lxp_receipt.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <string.h>

static const uint8_t sequencer_private_key[32] = {
    0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
    0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
    0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
    0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33
};

typedef struct fixture_evidence {
    uint8_t sequencer_public_key[32];
    uint8_t batch_id[32];
    uint8_t asset[32];
    uint8_t previous_state_root[32];
    uint8_t resulting_state_root[32];
} fixture_evidence;

static int public_key_for(
    const uint8_t private_key[32], uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(
        EVP_PKEY_ED25519, NULL, private_key, 32U);
    size_t length = 32U;
    int ok = key != NULL && EVP_PKEY_get_raw_public_key(
        key, public_key, &length) == 1 && length == 32U;
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static void write_hex(FILE *output, const uint8_t *bytes, size_t length)
{
    size_t index;
    for (index = 0U; index < length; ++index)
        (void)fprintf(output, "%02x", bytes[index]);
}

static void write_hex_field(FILE *output, const char *indent,
                            const char *name, const uint8_t *bytes,
                            size_t length, const char *suffix)
{
    (void)fprintf(output, "%s\"%s\": \"", indent, name);
    write_hex(output, bytes, length);
    (void)fprintf(output, "\"%s\n", suffix);
}

static void write_authority(FILE *output, const fixture_evidence *evidence,
                            const char *suffix)
{
    (void)fprintf(output, "  \"authorized_batch\": {\n");
    write_hex_field(output, "    ", "batch_id_hex", evidence->batch_id,
                    32U, ",");
    write_hex_field(output, "    ", "asset_hex", evidence->asset, 32U,
                    ",");
    write_hex_field(output, "    ", "previous_state_root_hex",
                    evidence->previous_state_root, 32U, ",");
    write_hex_field(output, "    ", "resulting_state_root_hex",
                    evidence->resulting_state_root, 32U, ",");
    write_hex_field(output, "    ", "sequencer_public_key_hex",
                    evidence->sequencer_public_key, 32U, "");
    (void)fprintf(output, "  }%s\n", suffix);
}

static int build_receipt(lxp_receipt *receipt, fixture_evidence *evidence)
{
    uint8_t activity_id[32];
    uint8_t activity_root[32];
    lxp_effect_buffer effects;
    lxp_result status;
    (void)memset(receipt, 0, sizeof(*receipt));
    (void)memset(evidence, 0, sizeof(*evidence));
    (void)memset(activity_id, 0x41, sizeof(activity_id));
    (void)memset(evidence->previous_state_root, 0x51,
                 sizeof(evidence->previous_state_root));
    (void)memset(evidence->resulting_state_root, 0x61,
                 sizeof(evidence->resulting_state_root));
    (void)memset(activity_root, 0x71, sizeof(activity_root));
    (void)memset(evidence->batch_id, 0x81, sizeof(evidence->batch_id));
    (void)memset(evidence->asset, 0x91, sizeof(evidence->asset));
    status = lxp_effect_buffer_init(&effects);
    receipt->protocol_version = LXP_PROTOCOL_VERSION_LEGACY;
    if (status == LXP_OK)
        status = lxp_receipt_build(
            receipt, activity_id, UINT64_C(7),
            evidence->previous_state_root, evidence->resulting_state_root,
            activity_root, LXP_OK, &effects, (lxp_u128){0U, 0U},
            evidence->batch_id, LXP_MODULE_PROGRAMS, 1U, 1U);
    if (status != LXP_OK) return 1;
    receipt->operation = 3U;
    (void)memcpy(receipt->asset, evidence->asset, 32U);
    receipt->amount = (lxp_u128){0U, 0U};
    (void)memset(receipt->from, 0xa1, 32U);
    receipt->from_balance_before = (lxp_u128){0U, 100U};
    receipt->from_balance_after = receipt->from_balance_before;
    receipt->from_sequence = 6U;
    (void)memset(receipt->to, 0xb1, 32U);
    receipt->to_balance_before = (lxp_u128){0U, 200U};
    receipt->to_balance_after = receipt->to_balance_before;
    (void)memset(receipt->authorization_hash, 0xd1, 32U);
    (void)memset(receipt->context_hash, 0xe1, 32U);
    receipt->timestamp = UINT64_C(1726000000001);
    return public_key_for(sequencer_private_key,
                          evidence->sequencer_public_key);
}

static int bind_program_outcome(lxp_receipt *receipt)
{
    lxp_program_outcome outcome;
    (void)memset(&outcome, 0, sizeof(outcome));
    outcome.present = true;
    outcome.encoding_version = 3U;
    outcome.terminal_kind = LXP_PROGRAM_TERMINAL_SUCCESS;
    outcome.result_code = LXP_OK;
    outcome.runtime_version = 1U;
    outcome.abi_version = 1U;
    outcome.fee_schedule_version = 1U;
    outcome.metering_schedule_version =
        LXP_PROGRAM_METERING_SCHEDULE_VERSION_V1;
    outcome.cpu_fuel = 11U;
    outcome.memory_bytes = 12U;
    outcome.storage_read_bytes = 13U;
    outcome.storage_write_bytes = 14U;
    outcome.output_values = 1U;
    outcome.output_bytes = 15U;
    outcome.fee_units = (lxp_u128){0U, 16U};
    (void)memset(outcome.call_graph_root, 0x11, 32U);
    (void)memset(outcome.terminal_payload_root, 0x22, 32U);
    return lxp_receipt_bind_program_outcome(receipt, &outcome) == LXP_OK ?
        0 : 1;
}

static int encode_signed(lxp_receipt *receipt, lxp_arena *arena,
                         const uint8_t sequencer_public_key[32],
                         lxp_byte_span *canonical, uint8_t digest[32])
{
    lxp_receipt decoded;
    if (lxp_receipt_sign(receipt, sequencer_private_key, arena) != LXP_OK ||
        lxp_receipt_digest(receipt, arena, digest) != LXP_OK ||
        lxp_receipt_encode(receipt, true, arena, canonical) != LXP_OK ||
        lxp_receipt_decode(canonical->bytes, canonical->length, true,
                           &decoded) != LXP_OK)
        return 1;
    return lxp_receipt_verify(&decoded, sequencer_public_key, arena) == LXP_OK ?
        0 : 1;
}

static int write_program_fixture(const char *path, lxp_receipt *receipt,
                                 const fixture_evidence *evidence,
                                 lxp_arena *arena)
{
    lxp_byte_span canonical;
    uint8_t receipt_digest[32];
    FILE *output;
    if (encode_signed(receipt, arena, evidence->sequencer_public_key,
                      &canonical, receipt_digest) != 0)
        return 1;
    output = fopen(path, "w");
    if (output == NULL) return 1;
    (void)fprintf(output,
        "{\n"
        "  \"name\": \"receipt-programs-positive-v1\",\n"
        "  \"provenance\": {\n"
        "    \"generator\": \"platform/sdk/conformance/fixtures/generate_program_receipt_fixture.c\",\n"
        "    \"command\": \"make platform-receipt-fixture\",\n"
        "    \"description\": \"Canonical signed protocol receipt constructed through the real LayerX C receipt builder, Programs outcome binder, validator, signer, encoder and decoder. It is a receipt-codec vector and does not claim runtime execution.\"\n"
        "  },\n");
    write_hex_field(output, "  ", "canonical_receipt_hex", canonical.bytes,
                    canonical.length, ",");
    write_authority(output, evidence, ",");
    (void)fprintf(output,
        "  \"expected\": {\n"
        "    \"level\": \"sequencer-signed\",\n"
        "    \"result_code\": 0,\n"
        "    \"protocol_version\": 1,\n"
        "    \"operation\": 3,\n"
        "    \"module_id\": 9,\n"
        "    \"module_version\": 1,\n"
        "    \"global_sequence\": 7,\n"
        "    \"timestamp_ms\": 1726000000001,\n"
        "    \"program_outcome_encoding_version\": 3,\n"
        "    \"program_outcome_runtime_version\": 1,\n"
        "    \"program_outcome_abi_version\": 1,\n"
        "    \"program_outcome_fee_units\": \"16\",\n");
    write_hex_field(output, "    ", "program_outcome_call_graph_root_hex",
                    receipt->program_outcome.call_graph_root, 32U, ",");
    write_hex_field(output, "    ",
                    "program_outcome_terminal_payload_root_hex",
                    receipt->program_outcome.terminal_payload_root, 32U, ",");
    write_hex_field(output, "    ", "receipt_digest_hex", receipt_digest,
                    32U, "");
    (void)fprintf(output, "  }\n}\n");
    return fclose(output) == 0 ? 0 : 1;
}

static int write_refusal_vector(FILE *output, const char *name,
                                const char *check, lxp_receipt *candidate,
                                lxp_arena *arena, const char *suffix)
{
    lxp_byte_span canonical;
    uint8_t ignored_digest[32];
    if (lxp_receipt_sign(candidate, sequencer_private_key, arena) != LXP_OK ||
        lxp_receipt_digest(candidate, arena, ignored_digest) != LXP_OK ||
        lxp_receipt_encode(candidate, true, arena, &canonical) != LXP_OK)
        return 1;
    (void)fprintf(output,
        "    {\"name\": \"%s\", \"expected_check\": \"%s\", \"canonical_receipt_hex\": \"",
        name, check);
    write_hex(output, canonical.bytes, canonical.length);
    (void)fprintf(output, "\"}%s\n", suffix);
    return 0;
}

static int write_refusal_fixture(const char *path,
                                 const lxp_receipt *base,
                                 const fixture_evidence *evidence,
                                 lxp_arena *arena)
{
    lxp_receipt candidate;
    FILE *output = fopen(path, "w");
    if (output == NULL) return 1;
    (void)fprintf(output,
        "{\n"
        "  \"name\": \"receipt-refusals-v1\",\n"
        "  \"provenance\": {\"generator\": \"platform/sdk/conformance/fixtures/generate_program_receipt_fixture.c\", \"command\": \"make platform-receipt-fixture\"},\n");
    write_authority(output, evidence, ",");
    (void)fprintf(output, "  \"vectors\": [\n");
    candidate = *base;
    candidate.global_sequence = 0U;
    if (write_refusal_vector(output, "zero-global-sequence",
                             "global-sequence", &candidate, arena, ",") != 0)
        return 1;
    candidate = *base;
    candidate.module_id = 0U;
    if (write_refusal_vector(output, "zero-module-id", "module-id",
                             &candidate, arena, ",") != 0)
        return 1;
    candidate = *base;
    candidate.module_version = 0U;
    if (write_refusal_vector(output, "zero-module-version",
                             "module-version", &candidate, arena, ",") != 0)
        return 1;
    candidate = *base;
    candidate.timestamp = 0U;
    if (write_refusal_vector(output, "zero-timestamp", "timestamp",
                             &candidate, arena, ",") != 0)
        return 1;
    candidate = *base;
    (void)memset(candidate.activity_id, 0, 32U);
    if (write_refusal_vector(output, "zero-activity-id", "activity-id",
                             &candidate, arena, ",") != 0)
        return 1;
    candidate = *base;
    (void)memset(candidate.resulting_state_root, 0, 32U);
    if (write_refusal_vector(output, "zero-resulting-state-root",
                             "resulting-state-root", &candidate, arena,
                             "") != 0)
        return 1;
    (void)fprintf(output, "  ]\n}\n");
    return fclose(output) == 0 ? 0 : 1;
}

int main(int argc, char **argv)
{
    static uint8_t arena_bytes[8U * LXP_MAX_ACTIVITY_BYTES];
    const char *program_path = argc > 1 ? argv[1] :
        "platform/sdk/conformance/fixtures/receipt-programs-positive-v1.json";
    const char *refusal_path = argc > 2 ? argv[2] :
        "platform/sdk/conformance/fixtures/receipt-refusals-v1.json";
    fixture_evidence evidence;
    lxp_receipt base;
    lxp_receipt programs;
    lxp_arena arena;
    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        build_receipt(&base, &evidence) != 0)
        return 1;
    programs = base;
    if (bind_program_outcome(&programs) != 0 ||
        write_program_fixture(program_path, &programs, &evidence, &arena) != 0 ||
        write_refusal_fixture(refusal_path, &base, &evidence, &arena) != 0)
        return 1;
    (void)fprintf(stdout, "fixture: wrote %s and %s\n", program_path,
                  refusal_path);
    return 0;
}
