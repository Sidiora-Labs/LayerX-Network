#ifndef LXP_DAEMON_ALLOWANCE_H
#define LXP_DAEMON_ALLOWANCE_H

#include "layerx/lxp_authority.h"
#include "layerx/lxp_transfer.h"

/* The allowance every transfer an activity emits from its principal draws
 * against: the resolved grant's live scope, bound to the grant kind, to the
 * grant identifier and to the principal account the debit leaves. The scope
 * is charged in place and the kernel persists the charged counters beside the
 * grant on commit, so grant must outlive the execution it authorizes. */
void lxp_daemon_live_allowance(lxp_authority_grant *grant,
                               const lxp_authority_resolved *authority,
                               lxp_transfer_allowance *allowance);

#endif
