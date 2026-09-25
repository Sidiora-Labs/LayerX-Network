# Sidiora

The second official coin of Paxeer X Network. One address, six decimals, and two ways to pay gas with it.

Sidiora is a coin people already hold, and this page is the read of what it is at the protocol level: what the token is, who may mint it, who may change the contract behind it, how gas can be paid in it, and which of the two gas paths works on the network as it runs today. Paxeer X Network is a Cosmos chain with a native EVM; [Unified network](../overview/unified-network.md) describes how the chain and the LayerX kernel domain sit behind one interface.

---

## What it is

| Field | Value |
| --- | --- |
| Symbol | `SID` |
| Decimals | `6` - one SID is `1000000` base units |
| Supply | approximately 458.86 million SID |
| Address | `0x21f7b20a555199fa73A238B1a91FD0f549068fEe` |
| Native denom | `factory/pax1dzfx9mk4fl9kl2mysjmtvk2xp75ljumk6nynhf/usid` - a tokenfactory denom whose creator is the bridge module account and whose subdenom is `usid` |
| Minter and burner | the `x/layerxbridge` module account, and nothing else |
| Contract administration | a timelock whose only proposer is the chain governance authority |

For contrast, the network coin is PAX. Its base denom is `uhpx` and it carries eighteen decimals, so a Sidiora amount and a network-coin amount are never the same scale.

### One address, a new implementation behind it

The address does not move. What changes is the implementation behind that proxy: `contracts/src/SidioraNativeERC20.sol` is an ERC-20 whose balances are not its own. `balanceOf` reads the bank balance of the native denom, `totalSupply` reads the bank supply of it, and every transfer moves the denom through the bank precompile and reverts the call if that move fails or returns false. The contract's own state - its initialization flag, the denom, the name, the symbol, the decimals and its allowances - lives in a single ERC-7201 namespace, so the slots the deployed implementation already occupies are neither read nor written; the allowances it keeps start empty and legacy slots stay untouched.

The denom string the contract derives is the same string `x/layerxbridge` derives for it, and a pointer record ties that denom to this same address, so the native denom and the ERC-20 people already hold resolve to one asset rather than two.

Nothing in this design replaces, redeploys or migrates the address. The implementation behind it is upgraded and the address stays.

### Who may mint it

Once the upgrade that makes Sidiora a native denom is in force, the only account that may mint or burn it is the `x/layerxbridge` module account. The denom is created with that account as its tokenfactory admin; creation refuses to continue if the admin is anything else, if the derived creator is not the bridge module account, or if the remote asset already maps to another denom. Tokenfactory mints and burns a denom only for that denom's admin, so no other account has the right.

The bridge mints when it credits an attested bridge-in and burns when it sends an amount out, and it holds the amount for no longer than that one operation. Most holders bring SID in and out across the bridge, which is why the minting right sits with the bridge rather than with an ordinary account.

### Who may change the contract behind the address

Administration of the proxy sits with governance behind `contracts/governance/SidioraProxyTimelock.sol`, and the timelock is deliberately narrow:

- Only the chain governance authority may schedule an operation, and the only call it may schedule on the proxy is `upgradeToAndCall(address,bytes)` with zero value. Any other target, any other selector, any value, oversized calldata or calldata that does not re-encode to exactly that call is refused.
- The scheduled call carries the new implementation and its initialization together, so an upgrade and the initializer it needs run as one operation. The implementation must be a contract, and it may be neither the proxy nor the timelock.
- Only the designated executor may execute, only after the delay has elapsed, and only before the grace period ends. The handover proposal in the repository names a two-day minimum delay, a floor of one day it may not be set below, and a one-week window in which an operation stays executable.
- Only the guardian may cancel, and only while the operation is not yet ready.

---

## Two ways to pay gas in Sidiora

|  | Sponsored | Native |
| --- | --- | --- |
| Where it runs | an account contract plus an off-chain quote service | the chain's own fee check |
| Changes a consensus rule | no | yes |
| Needs a chain upgrade | no | yes |
| How the account pays | a SID transfer to the sponsor inside its own batch | the fee is debited in SID and refunded in SID |
| What the account holds | SID, and the network coin only for value it sends | the same |

**The sponsored path is the one that is available without a chain upgrade.** It is an EIP-7702 batch, two signatures and an ERC-20 transfer; it changes no consensus rule, so a running network needs nothing new to support it. The native path changes how the chain charges fees, so it reaches a running network only through an upgrade handler carried by a governance proposal.

### The sponsored path

`contracts/src/BatchCallAndSponsor.sol` executes a batch on behalf of the account and repays a sponsor in SID inside the same transaction.

- **The account signs the batch.** Its signature covers the account's own batch nonce, the hash of the calls, and the digest of the quote, and it must recover to the account itself.
- **The relayer signs the quote.** The quote digest binds the chain id, the paymaster address, the sponsor, the token, the maximum token amount, the offered token amount, the deadline, the quote nonce and the declared gas cost. That signature must recover to the sponsor named in the quote.
- **Replay protection covers both.** The batch nonce advances with every batch, and a quote nonce is recorded per sponsor and refused if it has been used.
- **The quote is bounded.** Execution is refused if the deadline has passed, if the offered amount exceeds the quote's maximum, if either amount is zero, if the token is not Sidiora, or if the sponsor is the zero address or the account itself.
- **The price is checked on chain.** The paymaster holds its own rate, set by its owner, together with the time it was set and a maximum age. A zero rate, or one older than that age, reverts. The offered amount must then land within five percent either side of the amount that rate implies for the declared gas cost.
- **Repayment is atomic.** After the calls run, the batch transfers exactly the quoted amount of SID to the sponsor. A failed call, a transfer that fails or returns anything other than one, and the whole batch reverts - so the sponsor is never left unpaid for work that happened.

The service that signs quotes lives under `interop/crates/layerx-gas-station`. It signs with a relayer key supplied through a named environment variable, enforces per-account, per-interval and per-quote budgets and a balance floor before it hands out a quote, and prices the quote from exchange-rate data for the two denoms with a configured maximum age, spread and margin. Missing, malformed, invalid, stale or out-of-spread data is a refusal; it never substitutes a rate. `agent/sdk/typescript` carries the calls a person signs with: the quote request, the quote and batch digests, the EIP-7702 authorization, and the `executeSponsored` call itself. The SDK re-checks a returned quote against the configured sponsor, token, decimals, chain and deadline, and verifies both signatures locally, before the call is built.

### The native path

Here the chain charges the fee in the denom the account chose.

- **The preference is per account.** The fee-token precompile at `0x0000000000000000000000000000000000001018` exposes `setFeeDenom`, `getFeeDenom` and `clearFeeDenom`. An account sets only its own preference, because the caller is the account the preference is written for. The precompile is not payable, refuses a delegatecall, and refuses its two state changes from a static call. Setting a preference is refused while the fee-token switch is off and refused for a denom that is not in the allowed list. An account with no preference pays in the network coin.
- **The fee check converts before it charges.** The EVM fee check reads the payer's preference, takes the transaction's maximum fee - the gas limit times the gas price, plus the blob fee where the transaction carries blobs - and converts it into the chosen denom at the governed rate, rounding up in the network's favour. It refuses the transaction if the payer's spendable balance in that denom cannot cover the converted amount, and it still requires the network coin to cover any value the transaction sends: only the fee moves to SID.
- **The debit replaces the network-coin gas buy.** When gas is bought, the converted amount is debited from the payer in the fee denom instead of the network coin.
- **The refund comes back in the same denom.** Unused gas is returned in the fee denom, rounded up in the payer's favour, and the collected part is credited rounding down, so a refund and a collection together can never exceed what was debited.
- **The Cosmos fee path takes the same denoms.** For a Cosmos transaction, an allowed fee denom in the offered fee is converted into network-coin terms at the same governed rate, the ordinary minimum-gas-price rule is applied to the converted total, and the coins the sender offered are what get charged. A denom that is neither an allowed fee denom nor already approved for fees is refused rather than ignored.

Every one of those is a consensus change, so none of it reaches a running network by itself: it arrives through an upgrade handler carried by a governance proposal. Until that happens the switch is off and the allowed list is empty, and the fee check behaves exactly as it does now.

### How a Sidiora fee is priced

The native path does not price through an oracle. The oracle precompile is retired and every one of its methods returns a retired error, and the fee path takes its price from a governed rate instead:

- Each allowed fee denom carries a rate expressed in that denom's base units per whole network coin, and the block height at which the rate was set. The code documents 3.114 SID per network coin, at six decimals, as the rate a first proposal would set.
- A separate parameter bounds how old a rate may be, counted in blocks.
- Reading a rate refuses rather than falling back. A denom that is not in the allowed list is unavailable. A rate that is unset or not positive, a height above the current block, or an age bound that is not positive is invalid. A rate older than the age bound is stale. There is no last-known price, no zero fee and no silent switch to the network coin: the transaction fails.

The same rate governs the conversion in both directions. The charge rounds in the network's favour and the refund in the payer's.

---

## The governance surface

The fee-token parameters live in the `x/evm` parameter store, so every change to any of them is a governance proposal. Their defaults leave the whole native path inert.

| Parameter | Default | What its validator refuses |
| --- | --- | --- |
| `fee_token_enabled` | `false` | anything that is not a boolean |
| `allowed_fee_denoms` | empty | an invalid denom; the network coin's own denom, which needs no conversion; a duplicate denom; a rate that is unset or not positive; a negative rate-update height |
| `max_fee_token_rate_age` | `1000` blocks | anything that is not an integer; zero or a negative age |
| `max_fee_token_spread` | `0.05` | an unset spread; a negative one; one at or above one |
| `fee_token_distribution` | `false` | anything that is not a boolean |

Two of those decide whether the path exists at all: with the switch off, nothing converts, and with the allowed list empty, no denom has a rate, so a preference cannot even be set.

**Where collected fee tokens go** is the destination parameter, and it has two options for what the fee collector holds at the end of a block:

- **Held**, which is the default. Each allowed fee denom the fee collector holds is moved to a named holding module account, where governance decides what happens to it. If that account is not registered, the block fails rather than the coins going somewhere unintended.
- **Distributed.** The fee tokens are left at the fee collector and are distributed with everything else the collector holds.

Under either option the network coin's own fees keep their existing path: the routing step skips the base denom entirely and changes nothing about how network-coin fees are collected or distributed.

---

## Where it stands

- The sponsored path changes no consensus rule. It is the path that works without a chain upgrade.
- The native path is written and its parameters default to off, so a running network keeps charging exactly as it does today until a governance proposal turns it on.
- The quote-signing service reads its exchange-rate data through an interface the chain no longer serves, so it refuses to quote rather than pricing from anything else. A served rate source is what it needs.
- The pointer record for the denom and the handover of the proxy to the timelock exist as proposal documents in the repository, not as applied state.
- The routing step decides what happens to fee tokens the fee collector holds. The end-of-block sweep of a transaction's collected fees moves the network coin only.

Source for everything on this page lives in the [repository](https://github.com/Sidiora-Labs/Paxeer-X-Network): the denom and its metadata under `modules/layerxbridge`, the fee parameters, conversion and collection under `modules/evm`, the preference precompile under `precompiles/feetoken`, the token and paymaster contracts under `contracts/`, the quote service under `interop/crates`, and the signing calls under `agent/sdk/typescript`. The SDKs that carry them are described in [SDKs](../agents/sdks.md).
