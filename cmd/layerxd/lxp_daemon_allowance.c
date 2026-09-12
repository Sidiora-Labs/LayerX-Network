#include "lxp_daemon_allowance.h"

void lxp_daemon_live_allowance(lxp_authority_grant *grant,
                               const lxp_authority_resolved *authority,
                               lxp_transfer_allowance *allowance)
{
    lxp_authority_allowance_bind(grant, authority, allowance);
}
