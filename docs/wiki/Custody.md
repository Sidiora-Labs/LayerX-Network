# Authenticated custody credit

Custody credit is Bridge module 8, ordinal 1, version 1, on protocol 3. It is
available only with an explicitly signed custody genesis profile. Ordinary
genesis still reconstructs Programs 4 and Asset 1, with unchanged encoding and
zero balances. A custody genesis adds Bridge 1; it does not preallocate value.
An existing deployment cannot acquire this profile through an environment
variable, snapshot substitution, or a new manifest with different roots.

## Trust and evidence

The genesis signer authorizes one Ed25519 custody attestor, one external chain
ID **and chain genesis hash**, one vault address and SHA-256 runtime-code hash,
one asset, the canonical reserve account, and a minimum finalized confirmation
count. This is an explicit attestor trust model, not an Ethereum consensus light
client. Attestor compromise can authorize false custody; protect its key
separately from actor and sequencer keys. A vault proxy is not covered by pinning
its proxy runtime alone; use the directly deployed `LayerXVault` implementation.
The profile pins the vault and LayerX asset ID, not the registry's governance,
ERC-20 implementation, redemption promise, or token economics. Those remain
explicit custody trust assumptions: registry governors can select tokens whose
semantics are not wrapped native currency. General attestations prove the
included vault deposit under this trust model, not WETH solvency. Only the local
workflow below establishes wrapped-native backing through an actual ETH deposit
into the real WETH contract and its transfer into the real vault.

`tests/bridge/custody_credit.py` is the evidence-producing implementation. It
accepts two distinct RPC origins, verifies chain identity and finalized header
ancestry, reconstructs Ethereum header hashes, and checks the pinned vault
runtime at deposit and finality blocks using account Patricia proofs from
`eth_getProof` against each pinned header's state root, then matches the proven
Keccak code hash to the returned runtime. It reconstructs the complete receipt
Patricia trie and transaction trie against the block's roots. Deposit facts
come only from the successful transaction's included `CustodyDeposit` event.
The Solidity deposit ID preimage is independently recomputed. No caller-supplied
root, log, confirmation flag, or synthetic receipt can replace those reads.
HTTP is allowed only for literal loopback addresses; other RPCs require HTTPS.
RPC redirects are refused. Bounds are 4,096 transactions per block, 8,192
ancestry blocks, 16 MiB per RPC response and aggregate raw transactions,
128 MiB aggregate RPC responses and 20,000 RPC calls; exceeding them refuses
issuance. Local deployment commands have a 120-second timeout and 8 MiB combined
output limit. Distinct origins do not establish independent backends; operators
must assess that separately. Every observed finalized tip must descend from the
selected common finalized header.

The native verifier checks the attestor signature and all signed bindings,
including profile digest, LayerX network/protocol, beneficiary account and key,
asset, payer, amount, nonce, external transaction, receipt root and finality.
The actor also signs the full canonical LayerX activity. Identity resolution
and existing sequence/fee admission are not bypassed.
The beneficiary public key is an attestor-supplied identity binding, not a field
proven by the on-chain deposit event. The kernel separately checks that key
against the authenticated actor DID and signed activity. The producer requires
an explicit expected amount and refuses a receipt with any different amount.

## Profile and payload

The profile is 207 bytes, stored under Bridge key `custody-credit-profile/v1`
(zero-padded to 32 bytes) in the signed genesis module values. Fields are:

| Offset | Bytes | Field |
|---:|---:|---|
| 0 | 5 | `LXBC1` |
| 5 | 8 | external chain ID |
| 13 | 20 | vault address |
| 33 | 32 | SHA-256 of deployed vault runtime |
| 65 | 32 | Ed25519 attestor public key |
| 97 | 32 | asset ID |
| 129 | 32 | `system:paxeer-reserve` account ID |
| 161 | 8 | minimum finalized confirmations |
| 169 | 32 | external genesis block hash |
| 201 | 4 | LayerX network ID |
| 205 | 2 | LayerX protocol (3) |

Profile-enabled genesis reconstruction also inserts `custody-issued:` followed
by the asset ID, with an exact 16-byte zero value. This is committed by the
signed genesis root and gives initial issuance a real inclusion proof. It adds
no balances. Without the profile, legacy reconstruction remains unchanged.

Credit payloads are 427 bytes. Their first 363 bytes are signed under
`LX:CUSTODY:CREDIT:v1` (without a trailing NUL), followed by a 64-byte Ed25519
signature. The signed fields are `LXDC1`, profile SHA-256 (32), network (4),
protocol (2), deposit ID (32), asset (32), beneficiary (32), beneficiary key
(32), payer (20), amount (16), nonce (8), deposit block number (8), deposit block
hash (32), receipt root (32), finalized block number (8), finalized block hash
(32), transaction hash (32), and log index within that transaction (4). All
integers are unsigned big-endian. This is a new domain and layout; legacy
deposit-root registrations are not reinterpreted.

Idempotency is `SHA256("LX:DEPOSIT:NULLIFIER:v1" || deposit_id)`. The activity's
idempotency field must equal it. The HTTP `Idempotency-Key` uses its hexadecimal
spelling. A different activity sequence cannot credit the deposit again, and
changing the idempotency field is refused.

## Monetary transaction

Before issuing, the kernel verifies that its committed `custody-issued:` total
equals the sum of all account balances for this asset. It checks overflow,
asset pause state, reserve identity, beneficiary identity/key and replay state.
It stages an exact issuance increase equal to the verified custody amount,
snapshots the reserve, credits that amount into the reserve, then runs the
ordinary **conserved** reserve-to-beneficiary transfer. The recipient is created
at zero, with its real authority key, in the same staged transaction if absent.
The final summed balance must equal the new issued total.

Supply, full replay evidence, beneficiary registration, balances and sequences
commit together through the existing module/state journal. Failures roll them
back. No change is made to generic transfer conservation or bootstrap balance
semantics. Receipts carry the conserved transfer root in the transfer effect and a 208-byte custody
event binding deposit, asset, beneficiary, amount, profile/evidence digests and
before/after issued supply. A second 112-byte event binds reserve ID (32), actual
reserve before/after balances (16 each), beneficiary before/after balances
(16 each), and reserve before/after sequences (8 each). These are transition
boundary balances: the reserve does not claim newly issued funds at the previous
state root. The custody receipt has module 8, version 1 and operation 0; all
canonical conserved-ledger projection fields remain zero. Existing ledger
receipt assertions are unchanged and intentionally do not accept this as a
conserved Asset transfer. Verify canonical receipt signature and signed-header
inclusion, then the Bridge effects and account/issued-state proofs instead.
The native binding checks event balances against real transaction snapshots and
issued values against committed/staged KV. Before/after issuance values have
ordinary Bridge subtree inclusion proofs, including the initial zero. Absence of
the beneficiary derives from reconstruction of the complete signed empty genesis;
no non-inclusion proof is fabricated for missing accounts. Account proofs bind through `account-tree` and
the universal subtree to the receipt root.
Canonical activity replay restores the same state.

## Reproducible local funding

Run these commands only in a qualification phase, with exclusive native build
ownership. Use isolated local chains, never the persistent chain on port 18545.
Install the Python dependencies from `tests/bridge/requirements.txt`; Foundry
must be on `PATH`. Actor/attestor keys are separate raw 32-byte Ed25519 seed
files with mode 0600. The beneficiary is the protocol-3 hash of
`agent:<actor DID>:main`, not an EVM address. Choose the same asset ID as the
LayerX genesis/fee asset, distinct from the reserved USDL asset for this WETH
workflow.

1. Start a fresh Anvil on port 19545 with chain ID 31337. Supply `ASSET`,
   `BENEFICIARY`, and a positive integer `AMOUNT` within the unlocked account's
   native balance. Deploy and fund actual contracts:

   ```sh
   python tests/bridge/deploy_local_custody.py --allow-local-chain \
     --rpc http://127.0.0.1:19545 --asset "$ASSET" \
     --beneficiary "$BENEFICIARY" --amount "$AMOUNT" --output custody.json
   ```

   The helper deploys the real `LayerXTimelock`, `AssetRegistry`, Solmate WETH
   and `LayerXVault`. It schedules and executes permission/registration calls
   through the real timelock, advancing only the isolated chain's clock. Actual
   ETH is wrapped, approved and deposited. It checks vault WETH and WETH native
   backing. There is no mock token, storage injection or LayerX prefunding.

2. Provide a second real RPC observer of that exact chain. For local-only
   replication, an additional Anvil can fork port 19545 at `fork_block` from
   `custody.json`, using port 19546 and the same chain ID. Fork observers share
   an upstream trust source: they test local replication, **not independent
   production quorum or production finality**. A second URL to the same
   listener or a forwarding-only relay is not an independent observer.

3. Read `vault`, `runtime_sha256` and `transaction` from `custody.json` into
   `VAULT`, `RUNTIME_SHA256` and `TRANSACTION`. Produce the explicit profile:

   ```sh
   python tests/bridge/custody_credit.py profile \
     --rpc http://127.0.0.1:19545 --rpc http://127.0.0.1:19546 \
     --chain-id 31337 --network-id "$NETWORK_ID" --vault "$VAULT" --runtime-sha256 "$RUNTIME_SHA256" \
     --asset "$ASSET" --confirmations 2 --attestor-key "$ATTESTOR_KEY" \
     --output custody.profile
   build/bin/layerx-genesis-build request.lxgb "$GENESIS_KEY" genesis-output \
     --custody-profile custody.profile
   ```

   Register this new signed genesis through the normal external genesis
   registration flow and start `layerxd` against those exact artifacts.

4. Produce evidence and sign the funding activity with the real actor key:

   ```sh
   python tests/bridge/custody_credit.py attest \
     --rpc http://127.0.0.1:19545 --rpc http://127.0.0.1:19546 \
     --profile custody.profile --network-id "$NETWORK_ID" \
     --transaction "$TRANSACTION" --beneficiary "$BENEFICIARY" \
     --beneficiary-key "$ACTOR_PUBLIC_KEY" --attestor-key "$ATTESTOR_KEY" \
     --expected-amount "$AMOUNT" \
     --output custody.credit
   make build/tests/bridge/sign-credit build/tests/bridge/test-credit
   build/tests/bridge/sign-credit custody.profile custody.credit "$ACTOR_DID" \
     "$ACTOR_KEY" "$ACTOR_SEQUENCE" "$TIMESTAMP_MS" funding.activity
   ```

   Submit `funding.activity` as `application/octet-stream` to the authenticated
   boundary's existing `POST /v1/activities`, with the key from
   `custody.credit.nullifier`. Explicit boundary module registries must declare
   module 8 ordinal 1. The native ingress is the authenticated LNI submit path;
   no new unauthenticated funding HTTP endpoint exists. Fetch and verify the
   resulting normal receipt before spending. This signer uses fee limit zero;
   the current node's generic fee schedule is zero. It does not waive fees if
   that schedule changes.

5. Qualify the native transaction against real produced evidence:

   ```sh
   build/tests/bridge/test-credit genesis-output/genesis.manifest \
     funding.activity "$ACTOR_KEY"
   python tests/bridge/test_evidence.py \
     --rpc http://127.0.0.1:19545 --rpc http://127.0.0.1:19546 \
     --profile custody.profile --network-id "$NETWORK_ID" \
     --transaction "$TRANSACTION" --beneficiary "$BENEFICIARY" \
     --beneficiary-key "$ACTOR_PUBLIC_KEY" --attestor-key "$ATTESTOR_KEY" \
     --expected-amount "$AMOUNT"
   ```

   The native test requires real signed genesis and activity files; absent
   evidence is a failure, not a skip. It injects failures after issuance
   staging, reserve credit, transfer/event emission and account-root
   preparation, checking unchanged supply, nullifier, accounts, sequences and
   root. It checks every signed evidence/profile byte for tamper refusal and
   re-signs replay attempts with the real actor key. Real-node qualification
   must additionally verify custody receipt evidence, restart durability,
   deduplication and the funded escrow lifecycle.

   The Python regression suite verifies real fetched account proofs, corrupts
   nodes and canonical RLP encodings to prove refusal, and launches two isolated
   Anvil forks on ephemeral loopback ports. It mines different real histories,
   checks each finalized header, and requires refusal of their divergent
   finalized anchors. It never substitutes an RPC response or signs a synthetic
   custody root. These local forks are not independent production finality.
