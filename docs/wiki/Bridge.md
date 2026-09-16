# Bridge module

Module ID `8` (`LXP_MODULE_BRIDGE`). The registered module activity is
`LXP_BRIDGE_CREDIT` only (`include/layerx/lxp_bridge_credit.h`). Withdraw
requests are Asset ordinal 9. Emergency exit is a library path, not a
`0x0008xxxx` activity.

Paxeer holds custody. LayerX credits and debits the reserve mirror so
conservation still holds (`spec/layerx-protocol/spec.kvx` requirements 1
and 24).

Sources: `include/layerx/lxp_bridge.h`, `include/layerx/lxp_bridge_credit.h`,
`src/modules/bridge/`, `spec/layerx-protocol/spec.kvx` requirement 24.

---

## Activities

| Constant | Value | Role |
| --- | --- | --- |
| `LXP_BRIDGE_CREDIT` | `0x00080001` | Credit a proven Paxeer deposit |

Deposit proof helpers (`lx_deposit_proof`, `lxp_deposit_proof_verify`) and
withdraw finalize (`lxp_bridge_withdraw_finalize`) are library APIs used by
custody and settlement, not additional module ordinals.

---

## CREDIT

Payload is exactly `LXP_BRIDGE_CREDIT_BYTES` (427). Decode copies the bytes
(`src/modules/bridge/lxp_bridge_credit.c`). `lxp_bridge_credit_verify`
checks:

- magic `LXDC1` or `LXDC2`
- profile digest, network, protocol, deposit digest, asset id, beneficiary,
  attestation key, amounts, block/finality fields
- Ed25519 over the 363-byte signed prefix (`LXP_BRIDGE_CREDIT_SIGNED_BYTES`)

Requirement 24 acceptance 11: Comet chain `hyperpax_125-1` mapped to EVM
chain 125 uses profile `LXBC2` and credit `LXDC2` with signature domain
`LX:CUSTODY:CREDIT:v2`. The 207-byte profile (`LXP_BRIDGE_PROFILE_BYTES`) and
427-byte credit preserve identity, beneficiary, asset, amount, nonce, and
deposit-nullifier fields. Offsets 215, 223, 255, 287, 295, 327, and 359 are
state height H, signed header H+1 hash, H application root, finalized height
F, signed header F+1 hash, retained proof-bundle SHA-256, and proof-kind 1.
Those fields are not a transaction hash, log index, or Ethereum receipt
root. `LXBC1` / `LXDC1` keep the Ethereum layout. Profile and credit versions
must match.

Execute (`lxp_ctx_bridge_credit`) refusals include
`LXP_ERR_UNAUTHORIZED_DEBIT`, envelope and signature errors,
`LXP_ERR_ASSET_MISMATCH`, `LXP_ERR_ASSET_PAUSED`,
`LXP_ERR_DEPOSIT_PROOF_NOT_FINAL`, `LXP_ERR_CONTEXT_MISMATCH` (nullifier vs
idempotency key), `LXP_ERR_DEPOSIT_ALREADY_CREDITED`,
`LXP_ERR_ACCOUNT_ID_MISMATCH`, `LXP_FATAL_SUPPLY_MISMATCH`.

A credit is a `402LXP` transfer from `system:paxeer-reserve` to the
beneficiary. Replay of a consumed deposit identifier is refused.

---

## Withdraw request

See [Asset](Asset.md#withdraw). The LayerX debit lands in
`system:paxeer-withdrawals` before any Paxeer payout. Settlement binds the
asset recorded at request time (`spec/layerx-beta/spec.kvx` requirement 10).
A caller-supplied asset that disagrees is refused with no transfer.

Finalize (`lxp_bridge_withdraw_finalize`) requires a finalised checkpoint,
guarantor certificate, membership proof, and a closed challenge window.
Open windows are `LXP_ERR_CHALLENGE_WINDOW_OPEN`. Cancelled payouts are
`LXP_ERR_WITHDRAWAL_CANCELLED`. A consumed nullifier is
`LXP_ERR_WITHDRAWAL_ALREADY_SETTLED`.

---

## Emergency exit

`lxp_exit_eligibility`, `lxp_exit_declare`, and `lxp_exit_claim_build`
(`include/layerx/lxp_bridge.h`) allow an agent to exit against the last
finalised checkpoint when the sequencer is unavailable or the liveness bound
is exceeded. The claim is a balance proof plus the recorded guarantor
signatures. It is not an ordinary activity ordinal.

---

## Start here

- [Asset](Asset.md)
- [Finality](Finality.md)
- [Guarantor](Guarantor.md)
- [Sequencing](Sequencing.md)
- [Paxeer boundary](PaxeerBoundary.md)
