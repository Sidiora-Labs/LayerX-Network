#ifndef LAYERX_LX_ASSET_INTERNAL_H
#define LAYERX_LX_ASSET_INTERNAL_H

#include "layerx/lx_asset.h"

struct lx_checkpoint_registry {
    lx_finalized_checkpoint checkpoints[LX_CHECKPOINT_CAPACITY];
    size_t count;
    uint8_t paxeer_checkpoint_authority[32];
    uint32_t network_id;
    uint16_t protocol_version;
};

#endif
