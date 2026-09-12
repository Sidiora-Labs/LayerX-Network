#include "lxp_daemon_allowance.h"

#include <string.h>

void lxp_daemon_live_allowance(lxp_authority_grant *grant,
                               const lxp_authority_resolved *authority,
                               lxp_transfer_allowance *allowance)
{
    (void)memset(allowance, 0, sizeof(*allowance));
    allowance->scope = &grant->scope;
    allowance->kind = grant->kind;
    (void)memcpy(allowance->grantor, authority->principal, 32U);
    (void)memcpy(allowance->grant_id, grant->grant_id, 32U);
}
