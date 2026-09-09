#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_kernel.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_protocol.h"
#include "layerx/programs.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>

static int payer_failure(int line)
{
    (void)fprintf(stderr, "payer balance lookup failed at line %d\n", line);
    return 1;
}

static int open_named_account(lx_account_registry *accounts, const char *name,
                              uint64_t created, lx_account **opened)
{
    uint8_t id[32];
    if (lx_account_id_from_string((const uint8_t *)name, strlen(name), id) !=
            LXP_OK ||
        lx_account_open(accounts, (const uint8_t *)name, strlen(name), id,
                        created, LX_ACCOUNT_OPEN_CREDIT, NULL, opened) !=
            LXP_OK)
        return 1;
    return 0;
}

static int snapshot_payer_cases(void)
{
    static const char funded_name[] = "agent:did:lxp:payer-funded:main";
    static const char unfunded_name[] = "agent:did:lxp:payer-unfunded:main";
    static const char wrong_name[] = "agent:did:lxp:payer-wrong:main";
    static const char missing_name[] = "did:lxp:payer-missing";
    uint8_t occupancy[32];
    uint8_t other_asset[32];
    uint8_t funded_did[32];
    uint8_t unfunded_did[32];
    uint8_t wrong_did[32];
    uint8_t missing_did[32];
    lx_account_registry accounts;
    lx_account *funded;
    lx_account *unfunded;
    lx_account *wrong;
    lx_account *found;
    lxp_u128 balance;

    (void)memset(occupancy, 0x21, sizeof(occupancy));
    (void)memset(other_asset, 0x22, sizeof(other_asset));
    if (lx_account_registry_init(&accounts) != LXP_OK)
        return payer_failure(__LINE__);
    if (open_named_account(&accounts, funded_name, 7U, &funded) != 0 ||
        open_named_account(&accounts, unfunded_name, 8U, &unfunded) != 0 ||
        open_named_account(&accounts, wrong_name, 9U, &wrong) != 0)
        return payer_failure(__LINE__);
    if (lxp_ledger_bootstrap_balance(funded, occupancy,
                                     (lxp_u128){0U, 1000U}, 7U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(unfunded, occupancy,
                                     (lxp_u128){0U, 0U}, 8U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(wrong, other_asset,
                                     (lxp_u128){0U, 500U}, 9U) != LXP_OK)
        return payer_failure(__LINE__);
    if (lxp_did_id_derive((const uint8_t *)"did:lxp:payer-funded",
                          sizeof("did:lxp:payer-funded") - 1U,
                          funded_did) != LXP_OK ||
        lxp_did_id_derive((const uint8_t *)"did:lxp:payer-unfunded",
                          sizeof("did:lxp:payer-unfunded") - 1U,
                          unfunded_did) != LXP_OK ||
        lxp_did_id_derive((const uint8_t *)"did:lxp:payer-wrong",
                          sizeof("did:lxp:payer-wrong") - 1U,
                          wrong_did) != LXP_OK ||
        lxp_did_id_derive((const uint8_t *)missing_name, strlen(missing_name),
                          missing_did) != LXP_OK)
        return payer_failure(__LINE__);

    found = NULL;
    if (lxp_kernel_program_payment_account(
            &accounts, funded_did, occupancy,
            LXP_PROTOCOL_VERSION_STATE_COMMITMENT, &found) != LXP_OK ||
        found != funded || found->balance.lo != 1000U || found->balance.hi != 0U)
        return payer_failure(__LINE__);
    balance = found->balance;

    found = NULL;
    if (lxp_kernel_program_payment_account(
            &accounts, unfunded_did, occupancy,
            LXP_PROTOCOL_VERSION_STATE_COMMITMENT, &found) != LXP_OK ||
        found != unfunded || found->balance.lo != 0U || found->balance.hi != 0U)
        return payer_failure(__LINE__);

    found = (lx_account *)(uintptr_t)1U;
    if (lxp_kernel_program_payment_account(
            &accounts, wrong_did, occupancy,
            LXP_PROTOCOL_VERSION_STATE_COMMITMENT, &found) !=
            LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE ||
        found != NULL)
        return payer_failure(__LINE__);

    found = (lx_account *)(uintptr_t)1U;
    if (lxp_kernel_program_payment_account(
            &accounts, missing_did, occupancy,
            LXP_PROTOCOL_VERSION_STATE_COMMITMENT, &found) !=
            LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE ||
        found != NULL)
        return payer_failure(__LINE__);

    found = NULL;
    if (lxp_kernel_program_payment_account(
            &accounts, funded->id, occupancy, LXP_PROTOCOL_VERSION_OCCUPANCY,
            &found) != LXP_OK ||
        found != funded ||
        memcmp(&found->balance, &balance, sizeof(balance)) != 0)
        return payer_failure(__LINE__);

    found = (lx_account *)(uintptr_t)1U;
    if (lxp_kernel_program_payment_account(
            &accounts, funded_did, occupancy, LXP_PROTOCOL_VERSION_OCCUPANCY,
            &found) != LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE ||
        found != NULL)
        return payer_failure(__LINE__);
    return 0;
}

int main(void)
{
    return snapshot_payer_cases();
}
