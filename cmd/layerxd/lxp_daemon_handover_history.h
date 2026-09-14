#ifndef LXP_DAEMON_HANDOVER_HISTORY_H
#define LXP_DAEMON_HANDOVER_HISTORY_H

#include "layerx/lxp_handover.h"
#include "lxp_daemon_batch_wal.h"

lxp_result lxp_daemon_handover_wal_verify(const lxp_handover_trust_chain *chain,
    const lxp_log *log, const lxp_daemon_batch_wal_input *input,
    lxp_handover_trust_finality_fn verify, void *context, lxp_arena *arena);
lxp_result lxp_daemon_handover_history_load(lxp_handover_trust_chain *chain,
    lxp_log *log, const char *checkpoint_directory,
    lxp_handover_trust_finality_fn verify, void *context, lxp_arena *arena);

#endif
