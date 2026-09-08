#ifndef LAYERX_GUARANTOR_RUNTIME_H
#define LAYERX_GUARANTOR_RUNTIME_H
#include "layerx/lxp_guarantor.h"
typedef struct gp_runtime gp_runtime;
lxp_result gp_runtime_open(gp_runtime **, const char *, const char *);
lxp_result gp_runtime_prepare(gp_runtime *, const lxp_batch_body *);
lxp_replay_engine *gp_runtime_engine(gp_runtime *);
lxp_result gp_runtime_authority(void *, const lxp_activity *, lxp_byte_span,
                                lxp_guarantor_authority_verdict *);
lxp_result gp_runtime_oracle(void *, lxp_byte_span, bool *);
void gp_runtime_close(gp_runtime *);
#endif
