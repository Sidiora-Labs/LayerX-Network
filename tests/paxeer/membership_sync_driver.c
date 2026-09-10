#define _POSIX_C_SOURCE 200809L

#include "../../cmd/layerx-guarantor/settlement.h"
#include "layerx/lxp_paxeer.h"

#include <errno.h>
#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int parse(const char *text, uint64_t limit, uint64_t *value)
{
    unsigned long long parsed;
    char *end = NULL;
    errno = 0;
    parsed = strtoull(text, &end, 10);
    if (errno != 0 || end == text || *end != '\0' || (uint64_t)parsed > limit)
        return 0;
    *value = (uint64_t)parsed;
    return 1;
}

static void emit(const char *label, const uint8_t *bytes, size_t length)
{
    size_t i;
    (void)printf("%s=0x", label);
    for (i = 0U; i < length; ++i)
        (void)printf("%02x", bytes[i]);
    (void)fputc('\n', stdout);
}

static lxp_result emit_amount(const char *label, lxp_u128 value)
{
    uint8_t bytes[16];
    lxp_result status = lxp_u128_to_be(value, bytes);
    if (status != LXP_OK)
        return status;
    emit(label, bytes, sizeof(bytes));
    return LXP_OK;
}

int main(int argc, char **argv)
{
    gp_settlement_config config;
    lxp_paxeer_bond_state state;
    lxp_paxeer_membership_observation observation;
    lxp_paxeer_membership_sync_availability availability =
        LXP_PAXEER_MEMBERSHIP_SYNC_UNAVAILABLE;
    lxp_paxeer_membership_sync_availability repeated =
        LXP_PAXEER_MEMBERSHIP_SYNC_UNAVAILABLE;
    uint8_t commitment[32];
    uint8_t contract[20];
    uint64_t epoch, custodied, bps, protocol, chain_override = 0U;
    uint64_t network_override = 0U;
    uint32_t network_id;
    uint64_t chain_id;
    size_t i;
    lxp_result status;
    if (argc != 6 && argc != 8) {
        (void)fprintf(stderr,
                      "usage: %s STATE_DIR EPOCH CUSTODIED_VALUE BOND_BPS "
                      "PROTOCOL_VERSION [CHAIN_ID_OVERRIDE NETWORK_ID_OVERRIDE]\n",
                      argv[0]);
        return 2;
    }
    if (!parse(argv[2], UINT64_MAX, &epoch) ||
        !parse(argv[3], UINT64_MAX, &custodied) || !parse(argv[4], UINT32_MAX, &bps) ||
        !parse(argv[5], UINT16_MAX, &protocol))
        return 2;
    if (argc == 8 &&
        (!parse(argv[6], UINT64_MAX, &chain_override) ||
         !parse(argv[7], UINT32_MAX, &network_override)))
        return 2;
    status = gp_settlement_config_from_env(&config, argv[1]);
    if (status != LXP_OK) {
        (void)printf("stage=config\nstatus=%d\n", (int)status);
        return 1;
    }
    chain_id = chain_override != 0U ? chain_override : config.chain_id;
    network_id = network_override != 0U ? (uint32_t)network_override : config.network_id;
    (void)memcpy(contract, config.settlement_contract, sizeof(contract));
    status = lxp_paxeer_bond_init(&state, (uint16_t)protocol, network_id, chain_id, contract,
                                  (lxp_u128){0U, custodied}, (uint32_t)bps);
    if (status != LXP_OK) {
        (void)printf("stage=init\nstatus=%d\n", (int)status);
        return 1;
    }
    status = lxp_paxeer_membership_sync_status(&state, &availability);
    if (status != LXP_OK) {
        (void)printf("stage=initial-status\nstatus=%d\n", (int)status);
        return 1;
    }
    (void)printf("pre_availability=%d\n", (int)availability);
    status = gp_settlement_membership_sync(&config, epoch, &state, &availability);
    if (status != LXP_OK) {
        (void)printf("stage=sync\nstatus=%d\n", (int)status);
        return 1;
    }
    status = gp_settlement_membership_sync(&config, epoch, &state, &repeated);
    if (status != LXP_OK) {
        (void)printf("stage=resync\nstatus=%d\n", (int)status);
        return 1;
    }
    (void)memset(&observation, 0, sizeof(observation));
    observation.paxeer_chain_id = state.membership.paxeer_chain_id;
    (void)memcpy(observation.guarantor_bond_contract, state.membership.guarantor_bond_contract,
                 20U);
    observation.membership_version = state.membership.membership_version;
    observation.observed_epoch = state.membership.observed_epoch;
    observation.minimum_bond = state.minimum_bond;
    observation.members = state.guarantors;
    status = lxp_paxeer_membership_commitment(&observation, commitment);
    if (status != LXP_OK) {
        (void)printf("stage=commitment\nstatus=%d\n", (int)status);
        return 1;
    }
    (void)printf("stage=complete\nstatus=0\navailability=%d\nrepeated_availability=%d\n",
                 (int)availability, (int)repeated);
    (void)printf("commitment_matches=%d\n",
                 memcmp(commitment, state.membership.commitment, 32U) == 0 ? 1 : 0);
    (void)printf("chain_id=%" PRIu64 "\nnetwork_id=%" PRIu32 "\n",
                 state.membership.paxeer_chain_id, state.network_id);
    emit("contract", state.membership.guarantor_bond_contract, 20U);
    (void)printf("membership_version=%" PRIu64 "\nobserved_epoch=%" PRIu64
                 "\nmirror_version=%" PRIu64 "\nmember_count=%zu\n",
                 state.membership.membership_version, state.membership.observed_epoch,
                 state.mirror_version, state.guarantors.count);
    emit("commitment", state.membership.commitment, 32U);
    status = emit_amount("minimum_bond", state.minimum_bond);
    if (status == LXP_OK)
        status = emit_amount("custodied_value", state.custodied_value);
    if (status != LXP_OK) {
        (void)printf("stage=amount\nstatus=%d\n", (int)status);
        return 1;
    }
    for (i = 0U; i < state.guarantors.count; ++i) {
        lxp_guarantor_bond_state bond;
        bool eligible = false;
        char label[64];
        status = lxp_paxeer_bond_state_read(&state, state.guarantors.records[i].guarantor_id,
                                            &bond, &eligible);
        if (status != LXP_OK) {
            (void)printf("stage=read\nstatus=%d\n", (int)status);
            return 1;
        }
        (void)snprintf(label, sizeof(label), "member%zu_id", i);
        emit(label, bond.guarantor_id, 32U);
        (void)snprintf(label, sizeof(label), "member%zu_key", i);
        emit(label, bond.public_key, 33U);
        (void)snprintf(label, sizeof(label), "member%zu_bond", i);
        status = emit_amount(label, bond.bond_amount);
        if (status != LXP_OK) {
            (void)printf("stage=amount\nstatus=%d\n", (int)status);
            return 1;
        }
        (void)printf("member%zu_joined=%" PRIu64 "\nmember%zu_set_version=%" PRIu64
                     "\nmember%zu_eligible=%d\n",
                     i, bond.joined_epoch, i, bond.signer_authorizations[0].set_version, i,
                     eligible ? 1 : 0);
    }
    return 0;
}
