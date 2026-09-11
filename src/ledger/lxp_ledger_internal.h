#ifndef LAYERX_LXP_LEDGER_INTERNAL_H
#define LAYERX_LXP_LEDGER_INTERNAL_H

#include "layerx/lxp_transfer.h"

lxp_result lxp_balance_restore_snapshot(lxp_ledger_journal *journal);
lxp_result lxp_sequence_terminal_check(const lxp_transfer_context *context,
                                       const lxp_transfer_leg *leg);
/* Charges the debit leg against the grant the context presents, recording the
 * pre-charge scope in the journal so a rollback restores it. */
lxp_result lxp_allowance_charge_leg(const lxp_transfer_leg *leg,
                                    const lxp_transfer_context *context,
                                    lxp_ledger_journal *journal);

#endif
