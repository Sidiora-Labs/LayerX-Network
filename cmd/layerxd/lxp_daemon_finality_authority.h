#ifndef LXP_DAEMON_FINALITY_AUTHORITY_H
#define LXP_DAEMON_FINALITY_AUTHORITY_H
#include "layerx/lxp_daemon.h"

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
lxp_result lxp_daemon_finality_authority_verify(
    void *context, const lxp_guarantor_cert *certificate,
    const lxp_guarantor_set *bonded_set,
    const lxp_finalisation_requirements *requirements,
    const lxp_daemon_settlement_registration_evidence *registration);
#endif
