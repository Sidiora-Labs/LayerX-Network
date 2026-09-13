#ifndef LXP_DAEMON_FINALITY_AUTHORITY_H
#define LXP_DAEMON_FINALITY_AUTHORITY_H
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_handover.h"

typedef struct lxp_daemon_finality_authority {
    lxp_daemon_evidence_store *store;
    uint64_t paxeer_chain_id;
    uint8_t settlement_contract[20];
    uint8_t checkpoint_registry[20];
    uint16_t rpc_port;
} lxp_daemon_finality_authority;

lxp_result lxp_daemon_finality_authority_init(
    lxp_daemon_finality_authority *authority,
    lxp_daemon_evidence_store *store);
lxp_result lxp_daemon_finality_authority_init_pins(
    lxp_daemon_finality_authority *authority);
lxp_result lxp_daemon_finality_authority_verify(
    void *context, const lxp_guarantor_cert *certificate,
    const lxp_guarantor_set *bonded_set,
    const lxp_finalisation_requirements *requirements,
    const lxp_daemon_settlement_registration_evidence *registration);
lxp_result lxp_daemon_finality_authority_verify_explicit(
    const lxp_daemon_finality_authority *authority,
    const lxp_finalisation_state *finalisation,
    const lxp_guarantor_cert *certificate, const lxp_guarantor_set *bonded_set,
    const lxp_finalisation_requirements *requirements,
    const lxp_daemon_settlement_registration_evidence *registration);
lxp_result lxp_daemon_handover_finality_verify(
    const lxp_daemon_finality_authority *authority,
    const lxp_finalisation_state *known_finalisation,
    const lxp_batch_header *authenticated_predecessor,
    const uint8_t predecessor_signature[64],
    const lxp_handover_evidence *evidence, lxp_arena *arena);
#endif
