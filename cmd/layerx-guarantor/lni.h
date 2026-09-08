#ifndef LXP_GUARANTOR_LNI_H
#define LXP_GUARANTOR_LNI_H
#include "layerx/lxp_guarantor.h"

enum { LXP_GUARANTOR_CANDIDATE_SELECTOR = 0x05, LXP_GUARANTOR_FRAME_MAX = 2 * 1024 * 1024 };
typedef struct lxp_guarantor_lni {
    int fd;
    uint64_t correlation;
    uint32_t timeout_ms;
} lxp_guarantor_lni;
lxp_result lxp_guarantor_lni_open(lxp_guarantor_lni *client, const char *path, uint32_t timeout_ms);
void lxp_guarantor_lni_close(lxp_guarantor_lni *client);
lxp_result lxp_guarantor_lni_header(lxp_guarantor_lni *client, uint64_t batch,
                                    const lxp_sequencer_authorization *authority, uint32_t network,
                                    lxp_arena *arena, lxp_batch_header *header,
                                    uint8_t signature[64]);
lxp_result lxp_guarantor_chunk_verify(lxp_byte_span bytes, lxp_byte_span proof,
                                      const lxp_batch_header *header, uint32_t index,
                                      lxp_da_chunk *chunk, uint32_t *count);
lxp_result lxp_guarantor_lni_fetch(lxp_guarantor_lni *client, const lxp_batch_header *header,
                                   lxp_arena *arena, lxp_da_bundle *bundle);
lxp_result lxp_guarantor_lni_feedback(lxp_guarantor_lni *client, lxp_byte_span certificate,
                                      lxp_byte_span proof, lxp_arena *arena);
lxp_result lxp_guarantor_lni_checkpoint(lxp_guarantor_lni *, uint64_t, lxp_arena *, lxp_byte_span *,
                                        lxp_byte_span *);
#endif
