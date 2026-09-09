#ifndef LAYERX_GUARANTOR_SETTLEMENT_H
#define LAYERX_GUARANTOR_SETTLEMENT_H
#include "layerx/lxp_daemon.h"
#include "runtime.h"
#include "layerx/lxp_guarantor.h"
typedef struct gp_settlement_config {
    const char *python;
    const char *helper;
    const char *state_dir;
    const char *rpc_url;
    const char *submitter_key_file;
    const char *submitter_lock_file;
    const char *publication_inputs_dir;
    char rpc_url_storage[128];
    uint32_t network_id;
    uint64_t chain_id;
    uint8_t settlement_contract[20];
    uint8_t checkpoint_registry[20];
    size_t member_count;
    lxp_guarantor_key_record members[LXP_MAX_GUARANTOR_ATTESTATIONS];
} gp_settlement_config;
lxp_result gp_settlement_config_from_env(gp_settlement_config *, const char *);
lxp_result gp_settlement_membership(const gp_settlement_config *, uint64_t, lxp_guarantor_set *,
                                    size_t *, uint64_t *, lxp_u128 *);
lxp_result gp_settlement_register(const gp_settlement_config *, const lxp_guarantor_cert *, gp_runtime *,
                                  lxp_daemon_settlement_registration_evidence *, bool *,
                                  uint64_t *);
#endif
