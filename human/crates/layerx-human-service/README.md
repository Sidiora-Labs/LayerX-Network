# Human service

The human service authenticates the principal and tenant before resolving movement inputs. Caller aliases and currency labels are selectors, not authority. Movement protocol 2 carries typed accounts, asset, amount, route relationship, actor, authority, custody key and opaque provider handle, custody binding digest, wallet binding receipt, identity-authority and balance evidence, protocol/network, sequences, and exact validity and fee limits. Protocol 1 movement messages are refused.

Planning uses the principal-scoped onboarding and binding journeys, KMS key identity, agent identity/account sequence and verified balance observations. A returned quote is stored inside that principal's Journeys table; load checks quote identity and expiry before rebinding the commit idempotency key. Execution context is persisted separately and reloaded from the authorized principal scope when advancing a journey. Execution requests carry principal, tenant, typed-account digest, bound wallet and originating plan identity. Protocol 3 account addresses use the explicit protocol-3 derivation; withdrawal records carry their selected protocol and old missing shapes are refused.

The movement executor prepares exact EIP-1559 fields. The service independently checks chain, target, calldata, zero value, gas and fee limits against its authorized context, then registers that exact principal-bound transaction with KMS. The executor's separate certificate cannot register authorizations. The KMS retains private keys, nonce reservations, signed bytes and submission acknowledgements. Claim signatures are verified against an authorized transaction; signed transaction bytes are carried separately from contract calldata. Native owner SEND authorizations use the dedicated custody signing operation and canonical protocol encoding.

## Required movement configuration

The service retains its existing authentication, custody, agent and movement socket configuration. Additional values are mandatory:

| Variable | Meaning |
| --- | --- |
| `LAYERX_HUMAN_PAXEER_RPC_URL` | Existing primary RPC for non-finality reads; does not replace the quorum array |
| `LAYERX_HUMAN_PAXEER_RPC_URLS` | JSON array of 2–8 independent HTTPS endpoint authorities |
| `LAYERX_HUMAN_PAXEER_MINIMUM_AGREEMENT` | At least 2, no greater than endpoint count |
| `LAYERX_HUMAN_PAXEER_TRUST_ANCHOR_DER` | Configured RPC TLS trust root |
| `LAYERX_HUMAN_PAXEER_RPC_TIMEOUT_SECONDS` | Bounded RPC timeout |
| `LAYERX_HUMAN_PAXEER_CHAIN_ID` | Explicit Paxeer chain identity |
| `LAYERX_HUMAN_PAXEER_EXIT_CONTRACT`, `LAYERX_HUMAN_PAXEER_WITHDRAWAL_CLAIMS_CONTRACT` | Deployed settlement contracts matching provider configuration |
| `LAYERX_HUMAN_EXIT_REQUIRED_CONFIRMATIONS`, `LAYERX_HUMAN_EXIT_POLL_CADENCE_SECONDS`, `LAYERX_HUMAN_EXIT_DELAYED_AFTER_POLLS` | Finality depth, cadence and stall threshold shared by withdrawal and exit |
| `LAYERX_HUMAN_EVM_GAS_LIMIT` | Authorized transaction gas limit |
| `LAYERX_HUMAN_EVM_MAX_FEE_PER_GAS` | Exact maximum gas price in wei |
| `LAYERX_HUMAN_EVM_MAX_PRIORITY_FEE_PER_GAS` | Exact priority fee in wei, at most maximum fee |

Withdrawal and emergency-exit finality retain the production minimum of two independent votes. A beta cluster must supply at least two independent Paxeer RPC endpoints. A single endpoint cannot make these journeys ready.

The native identity interface does not yet supply a canonical active textual authority reference bound to its verified key. Its account-sequence lookup does not authenticate the configured authority string. This remaining planning-authority contract must be supplied before the complete movement service is ready.

The movement provider's on-chain witness publication gap is recorded in the lane status and qualification observations. Endpoint reachability alone is insufficient for movement readiness. No cluster or live-chain qualification is implied by this source.

API and lint refactors retain checks: authentication helpers without receiver state are associated functions; credential-bearing wrappers remain redacted; bounded parsing and operation dispatch use smaller helpers; canonical wire bounds remain checked. Qualification commands and actual results belong in retained qualification logs, not inferred from these changes.

Service integration suites retain their original Cargo target names and share the `layerx-human-test-support` development library. Run a suite with `cargo test --locked --manifest-path human/Cargo.toml -p layerx-human-service --test withdraw`, or add `-- --list` to list its cases. Existing archive and reclaim includes continue executing their original journey-fault checks.
