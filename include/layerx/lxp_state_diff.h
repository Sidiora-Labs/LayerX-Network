#ifndef LAYERX_LXP_STATE_DIFF_H
#define LAYERX_LXP_STATE_DIFF_H

#include "layerx/lxp_codec.h"
#include "layerx/lxp_ledger.h"

typedef struct lxp_state_diff_entry {
    uint8_t account_id[32];
    lxp_byte_span leaf;
} lxp_state_diff_entry;

lxp_result lxp_state_diff_encode(const lx_account_registry *before,
                                 const lx_account_registry *after,
                                 lxp_arena *arena, lxp_byte_span *encoded);
lxp_result lxp_state_diff_decode(lxp_byte_span encoded, lxp_arena *arena,
                                 lxp_state_diff_entry **entries,
                                 size_t *count);

#endif
