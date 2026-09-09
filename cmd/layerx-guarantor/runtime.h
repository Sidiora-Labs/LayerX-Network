#ifndef LAYERX_GUARANTOR_RUNTIME_H
#define LAYERX_GUARANTOR_RUNTIME_H
#include <stdio.h>
#include "layerx/lxp_guarantor.h"
#include "layerx/lxp_state_proof.h"
typedef struct gp_runtime gp_runtime;
lxp_result gp_runtime_open(gp_runtime **, const char *, const char *);
lxp_result gp_runtime_prepare(gp_runtime *, const lxp_batch_body *);
lxp_replay_engine *gp_runtime_engine(gp_runtime *);
lxp_result gp_runtime_authority(void *, const lxp_activity *, lxp_byte_span,
                                lxp_guarantor_authority_verdict *);
lxp_result gp_runtime_oracle(void *, lxp_byte_span, bool *);
lxp_result gp_runtime_state_proof(gp_runtime *, uint16_t, lxp_byte_span, lxp_state_witness *);
lxp_result gp_runtime_settlement_facts(gp_runtime *, FILE *);
void gp_runtime_close(gp_runtime *);
#endif
