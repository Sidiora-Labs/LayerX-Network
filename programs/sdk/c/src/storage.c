#include "layerx/program.h"

#include "host.h"
#include "internal.h"

/*
 * Storage is always addressed inside the invoking program and principal
 * namespace. No call in this file accepts a namespace, so neither an adjacent
 * program nor an adjacent principal can be reached by choosing a key.
 */

lxp_program_status lxp_program_storage_read(const uint8_t *key,
                                            size_t key_length, uint8_t *out,
                                            size_t capacity, size_t *length,
                                            bool *found)
{
    lxp_program_status status;
    int32_t outcome;
    if (length == NULL || found == NULL) return LXP_PROGRAM_ERR_NULL_ARGUMENT;
    *length = 0U;
    *found = false;
    status = lxp_program_check_key(key, key_length);
    if (status != LXP_PROGRAM_OK) return status;
    if (capacity > 0U && out == NULL) return LXP_PROGRAM_ERR_NULL_ARGUMENT;
    if (capacity > (size_t)LXP_PROGRAM_MAX_STORAGE_VALUE_BYTES)
        return LXP_PROGRAM_ERR_VALUE_TOO_LARGE;
    outcome = lxp_program_host_storage_read(
        lxp_program_pointer(key), lxp_program_length(key_length),
        lxp_program_pointer(out), lxp_program_length(capacity));
    if (outcome < 0) return outcome;
    if (outcome == 0) return LXP_PROGRAM_OK;
    if ((size_t)(uint32_t)(outcome - 1) > capacity)
        return LXP_PROGRAM_ERR_BUFFER_TOO_SMALL;
    *found = true;
    *length = (size_t)(uint32_t)(outcome - 1);
    return LXP_PROGRAM_OK;
}

lxp_program_status lxp_program_storage_write(const uint8_t *key,
                                             size_t key_length,
                                             const uint8_t *value,
                                             size_t value_length)
{
    lxp_program_status status;
    status = lxp_program_check_key(key, key_length);
    if (status != LXP_PROGRAM_OK) return status;
    if (value_length > 0U && value == NULL)
        return LXP_PROGRAM_ERR_NULL_ARGUMENT;
    if (value_length > (size_t)LXP_PROGRAM_MAX_STORAGE_VALUE_BYTES)
        return LXP_PROGRAM_ERR_VALUE_TOO_LARGE;
    return lxp_program_host_storage_write(
        lxp_program_pointer(key), lxp_program_length(key_length),
        lxp_program_pointer(value), lxp_program_length(value_length));
}

lxp_program_status lxp_program_storage_delete(const uint8_t *key,
                                              size_t key_length)
{
    lxp_program_status status;
    status = lxp_program_check_key(key, key_length);
    if (status != LXP_PROGRAM_OK) return status;
    return lxp_program_host_storage_delete(lxp_program_pointer(key),
                                           lxp_program_length(key_length));
}
