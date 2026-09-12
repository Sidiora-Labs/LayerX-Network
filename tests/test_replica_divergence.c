#include "layerx/lxp_replica.h"

#include <openssl/evp.h>
#include <stdint.h>
#include <string.h>

static int public_key_for(const uint8_t private_key[32], uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                  private_key, 32U);
    size_t length = 32U;
    int ok = key != NULL && EVP_PKEY_get_raw_public_key(
        key, public_key, &length) == 1 && length == 32U;
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static int one_component(lxp_divergence_component component,
                         uint64_t sequence)
{
    uint8_t expected[] = { 1U, 2U, 3U };
    uint8_t produced[] = { 1U, 2U, 3U };
    uint8_t later[] = { 9U };
    lxp_divergence_state state;
    (void)memset(&state, 0, sizeof(state));
    if (lxp_divergence_detect(&state, 7U, sequence - 1U, component,
            (lxp_byte_span){expected, sizeof(expected)},
            (lxp_byte_span){produced, sizeof(produced)}) != LXP_OK)
        return 1;
    produced[1] ^= 1U;
    if (lxp_divergence_detect(&state, 7U, sequence, component,
            (lxp_byte_span){expected, sizeof(expected)},
            (lxp_byte_span){produced, sizeof(produced)}) !=
            LXP_FATAL_REPLAY_DIVERGENCE || !state.detected ||
        state.global_sequence != sequence ||
        lxp_divergence_detect(&state, 8U, sequence + 1U, component,
            (lxp_byte_span){expected, sizeof(expected)},
            (lxp_byte_span){later, sizeof(later)}) !=
            LXP_FATAL_REPLAY_DIVERGENCE || state.batch_number != 7U ||
        state.global_sequence != sequence) return 1;
    return 0;
}

int main(void)
{
    uint8_t private_key[32] = { 5U };
    uint8_t public_key[32];
    uint8_t replica_id[32] = { 6U };
    uint8_t expected[] = { 1U };
    uint8_t produced[] = { 2U };
    lxp_divergence_state state;
    lxp_divergence_report_record report;
    lxp_replica replica;
    if (one_component(LXP_DIVERGENCE_RECEIPT, 11U) != 0 ||
        one_component(LXP_DIVERGENCE_STATE_DIFF, 12U) != 0 ||
        one_component(LXP_DIVERGENCE_STATE_ROOT, 13U) != 0 ||
        public_key_for(private_key, public_key) != 0) return 1;
    (void)memset(&state, 0, sizeof(state));
    if (lxp_divergence_detect(&state, 9U, 22U,
            LXP_DIVERGENCE_STATE_ROOT,
            (lxp_byte_span){expected, sizeof(expected)},
            (lxp_byte_span){produced, sizeof(produced)}) !=
            LXP_FATAL_REPLAY_DIVERGENCE ||
        lxp_divergence_report(&state, replica_id, private_key, &report) !=
            LXP_OK ||
        lxp_divergence_report_verify(&report, public_key) != LXP_OK)
        return 1;
    report.divergence.produced[0] ^= 1U;
    if (lxp_divergence_report_verify(&report, public_key) !=
        LXP_ERR_BAD_SIGNATURE) return 1;
    report.divergence.expected_length = sizeof(report.divergence.expected) + 1U;
    if (lxp_divergence_report_verify(&report, public_key) !=
        LXP_ERR_LENGTH_LIMIT) return 1;
    report.divergence.expected_length = SIZE_MAX;
    if (lxp_divergence_report_verify(&report, public_key) !=
        LXP_ERR_LENGTH_LIMIT) return 1;
    state.expected_length = sizeof(state.expected) + 1U;
    if (lxp_divergence_report(&state, replica_id, private_key, &report) !=
        LXP_ERR_LENGTH_LIMIT) return 1;
    (void)memset(&state, 0, sizeof(state));
    if (lxp_divergence_detect(&state, 1U, 1U, LXP_DIVERGENCE_RECEIPT,
            (lxp_byte_span){expected, SIZE_MAX},
            (lxp_byte_span){produced, SIZE_MAX}) != LXP_ERR_LENGTH_LIMIT ||
        state.detected) return 1;
    (void)memset(&replica, 0, sizeof(replica));
    replica.execution_enabled = true;
    replica.acknowledgements_enabled = true;
    replica.serving_current_state = true;
    replica.serving_finalised_history = true;
    if (lxp_replica_halt(&replica) != LXP_OK || !replica.halted ||
        replica.execution_enabled || replica.acknowledgements_enabled ||
        replica.serving_current_state || !replica.serving_finalised_history ||
        lxp_replica_halt(&replica) != LXP_OK) return 1;
    return 0;
}
