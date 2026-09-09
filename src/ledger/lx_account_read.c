#include "layerx/lxp_ledger.h"
#include "layerx/lxp_identity.h"

#include <string.h>

lxp_result lx_account_list_did(const lx_account_registry *registry,
    const uint8_t did_id[32], uint8_t (*account_ids)[32], size_t capacity,
    size_t *count)
{
    size_t found = 0U;
    if (registry == NULL || did_id == NULL || account_ids == NULL || count == NULL ||
        registry->count > LX_ACCOUNT_REGISTRY_CAPACITY) return LXP_ERR_NON_CANONICAL;
    for (size_t i = 0U; i < registry->count; ++i) {
        const lx_account *account = &registry->accounts[i];
        uint8_t owner[32];
        size_t length;
        lxp_result status;
        if ((account->kind < LX_ACCOUNT_AGENT_MAIN ||
             account->kind > LX_ACCOUNT_AGENT_MARGIN) &&
            account->kind != LX_ACCOUNT_AGENT_ASSET) continue;
        status = lx_account_validate_canonical(account);
        if (status != LXP_OK) return status;
        if (account->name_length > 11U &&
            memcmp(account->name + account->name_length - 5U, ":main", 5U) == 0)
            length = account->name_length - 11U;
        else if (account->name_length > 77U &&
            memcmp(account->name + account->name_length - 71U, ":asset:", 7U) == 0)
            length = account->name_length - 77U;
        else {
            const char *marker = account->kind == LX_ACCOUNT_AGENT_BUDGET ? ":budget:" :
                account->kind == LX_ACCOUNT_AGENT_ESCROW ? ":escrow:" :
                account->kind == LX_ACCOUNT_AGENT_STREAM ? ":stream:" :
                account->kind == LX_ACCOUNT_AGENT_MARGIN ? ":margin:" : NULL;
            size_t marker_length;
            length = 0U;
            if (marker == NULL) return LXP_ERR_NON_CANONICAL;
            marker_length = strlen(marker);
            for (size_t offset = 7U; offset + marker_length < account->name_length; ++offset)
                if (memcmp(account->name + offset, marker, marker_length) == 0)
                    length = offset - 6U;
            if (length == 0U) return LXP_ERR_NON_CANONICAL;
        }
        status = lxp_did_id_derive(account->name + 6U, length, owner);
        if (status != LXP_OK) return status;
        if (memcmp(owner, did_id, 32U) != 0) continue;
        if (found == capacity) return LXP_ERR_LENGTH_LIMIT;
        (void)memcpy(account_ids[found++], account->id, 32U);
    }
    for (size_t i = 1U; i < found; ++i) {
        uint8_t id[32];
        size_t j = i;
        (void)memcpy(id, account_ids[i], 32U);
        while (j != 0U && memcmp(account_ids[j - 1U], id, 32U) > 0) {
            (void)memcpy(account_ids[j], account_ids[j - 1U], 32U);
            --j;
        }
        (void)memcpy(account_ids[j], id, 32U);
    }
    *count = found;
    return LXP_OK;
}
