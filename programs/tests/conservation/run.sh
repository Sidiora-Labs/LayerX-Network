#!/bin/sh

set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
programs_cargo=${PROGRAMS_CARGO:-cargo}
cd "$repository_root"

programs_conservation_suite()
{
    make --no-print-directory programs-core-test
    make --no-print-directory \
        test-ledger-set \
        test-ledger-send \
        test-ledger-receive \
        test-gateway-send \
        test-gateway-receive \
        test-idempotency
    make --no-print-directory test-storage qualify-faults
    make --no-print-directory programs-differential
    (
        cd programs
        "$programs_cargo" test --locked -p layerx-programs-runtime --test monetary_law
        "$programs_cargo" test --locked -p layerx-programs-sandbox \
            long_lease_receipt_chain_conserves_every_charge
        "$programs_cargo" test --locked -p layerx-programs-sandbox \
            canonical_state_preserves_conservation_and_replay_marker
        "$programs_cargo" test --locked -p layerx-programs-market \
            finalization_is_after_window_and_conserves_escrow
        "$programs_cargo" test --locked -p layerx-programs-market \
            both_arbiter_outcomes_conserve_escrow_and_challenge_stake
        "$programs_cargo" test --locked -p layerx-programs-market \
            provider_absence_refunds_expired_lease
        "$programs_cargo" test --locked -p layerx-programs-market \
            unfunded_and_mid_work_expiry_are_refused
    )
}

programs_conservation_suite
