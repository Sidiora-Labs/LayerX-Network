#ifndef LXP_GUARANTOR_PRODUCER_H
#define LXP_GUARANTOR_PRODUCER_H
#include "layerx/lxp_guarantor.h"
enum { GP_ATTESTATION_BYTES = 274 };
lxp_result gp_attestation_encode(const lxp_guarantor_attestation *, uint8_t[GP_ATTESTATION_BYTES]);
lxp_result gp_attestation_decode(const uint8_t *, size_t, lxp_guarantor_attestation *);
lxp_result gp_key_load(const char *, lxp_guarantor_ctx *);
lxp_result gp_file_write(const char *, const uint8_t *, size_t);
lxp_result gp_verify_replay(lxp_guarantor_ctx *, const lxp_da_bundle *, const lxp_batch_header *,
                            const uint8_t[64], const lxp_da_store *, lxp_arena *, const char **);
lxp_result gp_verify_attest(lxp_guarantor_ctx *, const lxp_da_bundle *, const lxp_batch_header *,
                            const uint8_t[64], const lxp_da_store *, uint64_t, lxp_arena *,
                            lxp_guarantor_attestation *, const char **);
lxp_result gp_attestation_accept(const lxp_checkpoint_certificate *, uint64_t, const uint8_t[20],
                                 const lxp_guarantor_set *, const lxp_guarantor_attestation *,
                                 const lxp_guarantor_attestation *, size_t, const char *,
                                 lxp_arena *);
lxp_result gp_checkpoint_requirements(const lxp_batch_header *, uint64_t, size_t, lxp_u128,
                                      lxp_finalisation_requirements *);
#endif
