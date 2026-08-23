#include "layerx/lxp_fee.h"

#include <stdint.h>

int main(void)
{
    lxp_fee_params parameters = {
        1U,
        { 0U, 10U },
        { 0U, 1U },
        { 0U, 2U },
        { 0U, 3U },
        { 0U, 4U },
        10001U
    };
    lxp_fee_meter meter = {
        .canonical_encoded_bytes = 5U,
        .execution_units = 6U,
        .storage_units = 7U,
        .exact_program_fee_present = false,
        .program_fee_schedule_version = 0U,
        .exact_program_fee_units = {0U, 0U}
    };
    lxp_u128 fee;
    lxp_u128 unchanged = { 7U, 9U };
    if (lxp_fee_compute(&parameters, 2U, meter, &fee) != LXP_OK ||
        fee.hi != 0U || fee.lo != 69U) return 1;
    if (lxp_fee_limit_check(fee, (lxp_u128){ 0U, 69U },
                            (lxp_u128){ 0U, 69U }) != LXP_OK ||
        lxp_fee_limit_check(fee, (lxp_u128){ 0U, 68U },
                            (lxp_u128){ 0U, 100U }) != LXP_ERR_FEE_LIMIT ||
        lxp_fee_limit_check(fee, (lxp_u128){ 0U, 69U },
                            (lxp_u128){ 0U, 68U }) != LXP_ERR_FEE_UNPAYABLE)
        return 1;
    parameters.version = 2U;
    if (lxp_fee_compute(&parameters, 2U, meter, &fee) !=
        LXP_ERR_VERSION_UNSUPPORTED) return 1;
    parameters.version = 1U;
    parameters.base_fee = (lxp_u128){ UINT64_MAX, UINT64_MAX };
    fee = unchanged;
    if (lxp_fee_compute(&parameters, 2U, meter, &fee) != LXP_ERR_OVERFLOW ||
        fee.hi != unchanged.hi || fee.lo != unchanged.lo) return 1;
    return 0;
}
