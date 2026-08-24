#include "layerx/lxp_protocol.h"

typedef struct domain_tag_entry {
    const uint8_t *bytes;
    size_t length;
} domain_tag_entry;

#define LXP_TAG(value) { (const uint8_t *)(value), sizeof(value) - 1U }
static const domain_tag_entry domain_tags[LXP_DOMAIN_TAG_COUNT] = {
    LXP_TAG("LXP/v1/activity-id\000"),
    LXP_TAG("LXP/v1/payload-hash\000"),
    LXP_TAG("LXP/v1/signature-preimage\000"),
    LXP_TAG("LXP/v1/authority-hash\000"),
    LXP_TAG("LXP/v1/context-hash\000"),
    LXP_TAG("LXP/v1/merkle-leaf\000"),
    LXP_TAG("LXP/v1/merkle-internal\000"),
    LXP_TAG("LXP/v1/batch-header\000"),
    LXP_TAG("LXP/v1/receipt\000"),
    LXP_TAG("LXP/v1/checkpoint-certificate\000"),
    LXP_TAG("LXP/v1/account-id\000"),
    LXP_TAG("LXP/v1/did-id\000"),
    LXP_TAG("LXP/v1/evm-payout-binding\000"),
    LXP_TAG("LXP/v1/state-leaf\000"),
    LXP_TAG("LXP/v1/state-node\000"),
    LXP_TAG("LXP/v1/state-root-chain\000"),
    LXP_TAG("LXP/v1/snapshot\000"),
    LXP_TAG("LXP/v1/da-chunk\000"),
    LXP_TAG("LXP/v1/da-challenge\000")
};
#undef LXP_TAG

bool lxp_protocol_version_supported(uint16_t protocol_version)
{
    return protocol_version == (uint16_t)LXP_PROTOCOL_VERSION_LEGACY ||
           protocol_version == (uint16_t)LXP_PROTOCOL_VERSION_OCCUPANCY;
}

bool lxp_network_id_matches(uint32_t configured_network_id,
                            uint32_t presented_network_id)
{
    return configured_network_id != UINT32_C(0) &&
           configured_network_id == presented_network_id;
}

const uint8_t *lxp_domain_tag(lxp_domain_tag_id id, size_t *length)
{
    if (length == NULL || id >= (lxp_domain_tag_id)LXP_DOMAIN_TAG_COUNT) {
        return NULL;
    }
    *length = domain_tags[id].length;
    return domain_tags[id].bytes;
}
