#ifndef LAYERX_PROGRAMS_OCCUPANCY_EVIDENCE_H
#define LAYERX_PROGRAMS_OCCUPANCY_EVIDENCE_H

#include "occupancy.h"

lxp_result lxp_programs_occupancy_validate_output(
    lxp_programs_occupancy_bridge *bridge);
lxp_result lxp_programs_occupancy_validate_receipt_evidence(
    const lxp_programs_occupancy_receipt *receipt);

#endif
