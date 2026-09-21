# Authenticated custody credit

The Human web app's custodial keystore, wallet-binding boundary, and
journey verification levels are a different surface. See
[Human custody](HumanCustody.md) and [Human journeys](HumanJourneys.md).
This page is Bridge module 8 custody credit on the native protocol.

Custody credit is Bridge module 8, ordinal 1, version 1, on protocol 3. It is
available only with an explicitly signed custody genesis profile. Ordinary
genesis still reconstructs Programs 4 and Asset 1, with unchanged encoding and
zero balances. A custody genesis adds Bridge 1; it does not preallocate value.
Natively issued tokens and per-asset accounts are a separate surface on
[Assets](Assets.md). Paxeer-custody asset ids stay their existing registered
values.
An existing deployment cannot acquire this profile through an environment
variable, snapshot substitution, or a new manifest with different roots.

## Trust and evidence

The genesis signer authorizes the Paxeer EVM chain ID `125`, the native custody
module address and its module identity, one asset, the canonical reserve
account, the Comet chain ID, a trusted Comet height with its next-validators
hash and header time, and a trusting period. There is no custody attestor key.
A deposit is admitted by a Tendermint light-client proof of the custody
module's own `Deposit` record against that trusted header
(`include/layerx/lxp_bridge_light.h`, `src/modules/bridge/lxp_bridge_light.c`).
Custody is the `layerxcustody` module behind the precompile at
`0x0000000000000000000000000000000000001013`; no vault, wrapped token or asset
registry contract is pinned.

The trust root is the validator set the profile commits to. An `LXLB1` bundle
is refused unless its signed header carries more than two thirds of that
header's own validator power, the header is no older than the trusted one, and,
when the validator set has changed, the signers overlap the trusted set by more
than one third of its power. Trust that has aged past the profile's trusting
period is refused outright rather than degraded to a weaker check. The profile
pins the module identity and the LayerX asset ID, not Paxeer governance or
token economics; those remain explicit custody trust assumptions.

`tests/bridge/custody_credit.py` is the evidence-producing implementation. It
refuses an `LXBC1` or `LXBC2` profile outright and delegates the Paxeer layout
to `tests/bridge/comet_credit.py`, which drives the `layerx-custody-proof`
producer. It accepts two distinct EVM RPC origins and one Comet RPC, and
verifies chain identity. Deposit facts come only from the successful
transaction's included `CustodyDeposit` event emitted by the pinned custody
precompile, and both origins must return the same one. The deposit ID preimage
is independently recomputed under the domain `LXP/Paxeer/custody-deposit/v1`.
The credit is produced by `layerx-custody-proof light-credit`, and the helper
re-checks every bound field of the returned head against the discovered deposit
before writing it. No caller-supplied root, log, or synthetic receipt can
replace those reads. HTTP is allowed only for literal loopback addresses; other
RPCs require HTTPS. RPC redirects are refused. Bounds are 16 MiB per RPC
response, 128 MiB aggregate RPC responses and 20,000 RPC calls; exceeding them
refuses issuance. The proof producer has a 120-second timeout. Distinct origins
do not establish independent backends; operators must assess that separately.

The native verifier checks the light-client bundle and every bound field of
the credit head: profile digest, LayerX network and protocol, deposit ID,
asset, beneficiary account, beneficiary key, depositor, amount, nonce, state
height, the proven header hash, its application root, the header height, the
header's validators hash, and the SHA-256 of the retained bundle. The proven
`Deposit` record must match the head field for field. The actor also signs the
full canonical LayerX activity. Identity resolution and existing sequence/fee
admission are not bypassed.
The beneficiary public key is a producer-supplied identity binding, not a field
carried by the proven deposit record. The kernel separately checks that key
against the authenticated actor DID and signed activity. The producer requires
an explicit expected amount and refuses a deposit with any different amount.

## Profile and payload

The profile is 223 bytes, stored under Bridge key `custody-credit-profile/v1`
(zero-padded to 32 bytes) in the signed genesis module values. Fields are:

| Offset | Bytes | Field |
|---:|---:|---|
| 0 | 5 | `LXBC3` |
| 5 | 8 | EVM chain ID (125) |
| 13 | 20 | custody precompile address |
| 33 | 32 | `sha256("LX:CUSTODY:MODULE:v1" \|\| "layerxcustody" \|\| address)` |
| 65 | 32 | trusted next-validators hash |
| 97 | 32 | asset ID |
| 129 | 32 | `system:paxeer-reserve` account ID |
| 161 | 8 | trusted Comet height |
| 169 | 32 | Comet chain ID, NUL-padded |
| 201 | 4 | LayerX network ID |
| 205 | 2 | LayerX protocol (3) |
| 207 | 8 | trusting period, seconds |
| 215 | 8 | trusted header time, Unix seconds |

`lxp_bridge_light_trust_load` seeds trust from those fields. An accepted later
header is stored under Bridge key `paxeer-light-trust/v1`, and a stored trust
at or below the profile's height is refused.

Profile-enabled genesis reconstruction also inserts `custody-issued:` followed
by the asset ID, with an exact 16-byte zero value. This is committed by the
signed genesis root and gives initial issuance a real inclusion proof. It adds
no balances. Without the profile, legacy reconstruction remains unchanged.

A credit payload is the 363-byte `LXDC3` head followed by the `LXLB1`
light-client bundle; the committed vectors in
`tests/fixtures/custody/paxeer-light-v1` are 1495 bytes. Nothing in the payload
carries a custody signature. The head is `LXDC3` (5), profile SHA-256 (32),
network (4), protocol (2), deposit ID (32), asset (32), beneficiary (32),
beneficiary key (32), depositor (20), amount (16), nonce (8), state height H
(8), signed header H+1 hash (32), that header's application root (32), header
height H+1 (8), its validators hash (32), SHA-256 of the bundle (32), and proof
kind `2` (4). All integers are unsigned big-endian. The 363-byte head is what
the module retains under `deposit-nullifier:`; the bundle is verified and
discarded. Legacy deposit-root registrations are not reinterpreted.

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
ownership. Use an isolated disposable Paxeer chain, never the persistent chain
on port 18545. Install the Python dependencies from
`tests/bridge/requirements.txt`. The actor key is a raw 32-byte Ed25519 seed
file with mode 0600; there is no attestor key to keep. The beneficiary is the
protocol-3 hash of `agent:<actor DID>:main`, not an EVM address. Choose the
same asset ID as the LayerX genesis/fee asset, distinct from the reserved USDL
asset.

1. Bring up a disposable single-validator Paxeer chain and make the deposit.
   `platform/hosted/paxeer/custody-genesis.py` writes the `layerxcustody`
   genesis for the chosen network id, sequencer identity and `<asset>:uhpx`
   map, `platform/hosted/paxeer/init-chain.sh` initialises a home from it, and
   `paxd start --home` runs it. The deposit is an ordinary EVM call to the
   custody precompile, with `--value` the `uhpx` amount times `10^12` wei:

   ```sh
   python3 platform/hosted/paxeer/evm.py send --rpc http://127.0.0.1:48545 \
     --chain 125 --key-file "$PAYER_KEY" --value "$WEI" \
     0x0000000000000000000000000000000000001013 \
     'deposit(bytes32)' "0x$BENEFICIARY"
   ```

   `tests/fixtures/custody/paxeer-light-v1/README` records the exact bring-up
   that produced the committed vectors, down to the paxd commit, chain
   identity, asset ID and deposit transaction.

2. Chain 125 is reachable to the helpers only through a verified disposable
   identity. `--disposable-identity` is the identity-file flag
   (`tests/bridge/deploy_local_custody.py:221`). When `--disposable-identity`
   is set, the helper requires `--ca-bundle` and `--key-file` and passes the
   RPC URL, CA bundle, and identity path to `disposable_rpc`
   (`tests/bridge/deploy_local_custody.py:224-226`). `disposable_rpc` reads
   the identity file as JSON (`tests/bridge/deploy_local_custody.py:109`).
   Required fields are `rpc_origins`
   (`tests/bridge/deploy_local_custody.py:111-112`), `genesis_sha256`
   (`tests/bridge/deploy_local_custody.py:113`), `comet_chain_id` as a
   non-empty string (`tests/bridge/deploy_local_custody.py:114-115`),
   `chain_id` as a positive `int` (`tests/bridge/deploy_local_custody.py:117`),
   `ca_sha256` (`tests/bridge/deploy_local_custody.py:118-119`), and
   `genesis_source` (`tests/bridge/deploy_local_custody.py:123`).

   When `genesis_source` is `boundary`, the helper GETs `/genesis` from the
   Paxeer boundary, reads at most `MAX_GENESIS_BYTES` (64 MiB), and requires
   `X-LayerX-Genesis-SHA256` to equal the SHA-256 hex digest of the body
   (`tests/bridge/deploy_local_custody.py:25`,
   `tests/bridge/deploy_local_custody.py:35-41`). The `chunked` alternative
   fetches `/genesis_chunked?chunk=` plus the chunk index and concatenates
   base64 `data` until the reported `total`
   (`tests/bridge/deploy_local_custody.py:44-63`).

   `disposable_rpc` refuses an origin not authorized by `rpc_origins`
   (`tests/bridge/deploy_local_custody.py:110-112`), a CA pin mismatch
   (`tests/bridge/deploy_local_custody.py:118-119`), the persistent genesis
   digest on the identity value or the fetched document
   (`tests/bridge/deploy_local_custody.py:23`,
   `tests/bridge/deploy_local_custody.py:116`,
   `tests/bridge/deploy_local_custody.py:126`), persistent blueprint code at
   the recorded address (`tests/bridge/deploy_local_custody.py:24`,
   `tests/bridge/deploy_local_custody.py:129-130`), a Comet chain id mismatch
   (`tests/bridge/deploy_local_custody.py:125-127`), and an EVM chain id
   mismatch (`tests/bridge/deploy_local_custody.py:128`). The shared Comet
   chain name alone does not identify the persistent host
   (`tests/bridge/deploy_local_custody.py:114-116`,
   `tests/bridge/deploy_local_custody.py:125-130`).

   The helper deploys the real `LayerXTimelock`, `AssetRegistry`, Solmate WETH
   and `LayerXVault`. It schedules and executes permission/registration calls
   through the real timelock, advancing only the isolated chain's clock. Actual
   ETH is wrapped, approved and deposited. It checks vault WETH and WETH native
   backing. There is no mock token, storage injection or LayerX prefunding.

3. Provide two distinct real RPC observers of that exact chain; the helpers
   refuse a single origin, and a second URL to the same listener or a
   forwarding-only relay is not an independent observer. Then produce the
   light-client profile. `custody_credit.py profile` refuses any chain other
   than 125, requires the custody precompile address and its module identity in
   place of a vault and runtime hash, and delegates to
   `layerx-custody-proof light-profile`:

   ```sh
   python tests/bridge/custody_credit.py profile \
     --rpc "$FIRST_RPC" --rpc "$SECOND_RPC" --comet-rpc "$COMET_RPC" \
     --ca-bundle "$CA_BUNDLE" --disposable-identity "$IDENTITY" \
     --chain-id 125 --network-id "$NETWORK_ID" \
     --vault 0x0000000000000000000000000000000000001013 \
     --runtime-sha256 "$MODULE_IDENTITY" \
     --asset "$ASSET" --trusted-height "$TRUSTED_HEIGHT" \
     --trusting-period-seconds 1209600 \
     --output custody.profile
   build/bin/layerx-genesis-build request.lxgb "$GENESIS_KEY" genesis-output \
     --custody-profile custody.profile
   ```

   Register this new signed genesis through the normal external genesis
   registration flow and start `layerxd` against those exact artifacts.

4. Produce evidence and sign the funding activity with the real actor key:

   ```sh
   python tests/bridge/custody_credit.py attest \
     --rpc "$FIRST_RPC" --rpc "$SECOND_RPC" --comet-rpc "$COMET_RPC" \
     --ca-bundle "$CA_BUNDLE" --disposable-identity "$IDENTITY" \
     --profile custody.profile --network-id "$NETWORK_ID" \
     --transaction "$TRANSACTION" --beneficiary "$BENEFICIARY" \
     --beneficiary-key "$ACTOR_PUBLIC_KEY" \
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
     --rpc "$FIRST_RPC" --rpc "$SECOND_RPC" \
     --profile custody.profile --network-id "$NETWORK_ID" \
     --transaction "$TRANSACTION" --beneficiary "$BENEFICIARY" \
     --beneficiary-key "$ACTOR_PUBLIC_KEY" \
     --expected-amount "$AMOUNT"
   ```

   `make test-light-credit` replays the committed
   `tests/fixtures/custody/paxeer-light-v1` vectors against the native verifier
   with no chain running.

   The native test requires real signed genesis and activity files; absent
   evidence is a failure, not a skip. It injects failures after issuance
   staging, reserve credit, transfer/event emission and account-root
   preparation, checking unchanged supply, nullifier, accounts, sequences and
   root. It checks every evidence and profile byte for tamper refusal and
   re-signs replay attempts with the real actor key. Real-node qualification
   must additionally verify custody receipt evidence, restart durability,
   deduplication and the funded escrow lifecycle.

   The Python regression suite verifies real fetched account proofs, corrupts
   nodes and canonical RLP encodings to prove refusal, and launches two isolated
   Anvil forks on ephemeral loopback ports. It mines different real histories,
   checks each finalized header, and requires refusal of their divergent
   finalized anchors. It never substitutes an RPC response or signs a synthetic
   custody root. These local forks are not independent production finality.

[Home](Home.md)
