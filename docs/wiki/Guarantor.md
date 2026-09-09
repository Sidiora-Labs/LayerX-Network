# Guarantor checkpoint production

`make layerx-guarantor` builds the C17 checkpoint producer. One process owns
one secp256k1 identity and an exclusive state-directory lock. It follows
signed batch headers over the node-local LNI socket, independently replays
sealed availability data, retains verified data, exchanges attestations,
and registers and feeds back certificates. The beta uses two identities
operated by the same cluster: **there is no operational independence**.
Production operator independence remains an onboarding requirement.

## Candidate verification

The producer pins the sequencer id, public key, network and batch range from
bootstrap configuration. LNI tag 12 supplies the header and signature. Tag 18
requests `05 || batch:u64be`; the selector is the single named constant
`LXP_GUARANTOR_CANDIDATE_SELECTOR`. Selectors 01–04 remain finalized-only.
The daemon serves selector 05 for one sealed signed candidate without a
registered certificate. The producer does not register a fixture certificate
to unlock retrieval.

Every tag-19 response must match the request correlation, batch, global chunk
index, claimed chunk hash and Merkle proof. Retrieval is bounded by a total
monotonic deadline, frame and arena limits, and the canonical 4096-chunk
limit. The empty tag-20 terminator is required. Reconstruction verifies all
five ordered classes and the signed header's availability root, using the
existing 65536-byte canonical chunk encoding.

The independent runtime loads and verifies the genesis manifest,
registration and snapshot, validates actor signatures and owner authority,
and executes the real kernel and Programs runtime. It compares recomputed
receipts, state diffs, recovery metadata and committed roots, including
`lxp_guarantor_recompute_roots`. Receipt signatures are verified using the
sequencer public key and retained only after independently comparing receipt
contents. The producer never needs the sequencer private key.

The adapter supports the node's direct-owner authority path. Nonempty oracle
inputs and incomplete rotated-signer history refuse explicitly. Restart
replays from genesis; persistent DA and attestations are retained. A replay
failure stops the process before signing. Stored attestations are reused
rather than replaced with newly randomized signatures for the same batch.

## Exchange and settlement

The two loopback HTTPS listeners use private-CA certificates with mandatory
client authentication. Peer connections verify the CA and hostname. GET
`/v1/attestations/{checkpoint_id}` returns concatenated 274-byte canonical
attestations; POST `/v1/attestations` accepts exactly one. The encoding matches
the fixed-width attestation fields carried in native finality evidence.
The HTTP parser rejects duplicate content lengths, transfer encoding,
oversized bodies and unsupported paths. TLS operations have bounded deadlines.

Incoming attestations undergo native signature/recovery verification,
chain-backed bonded-membership checks and checkpoint-id recomputation from
a locally verified header. Verified contradictory statements for the same
identity, epoch and batch produce native kind-1 equivocation evidence.
Canonical attestations and signed header records are durably retained;
historical GET responses are checked again before serving.

After the configured threshold is available, the native certificate
assembler sorts guarantor ids. `settlement.py`, invoked directly by the C
wrapper without a shell, encodes the exact Solidity ABI, checks the deployed
chain and registry binding, and signs with the dedicated funded checkpoint
submitter key. A shared file lock serializes the two local submitters.
Existing registration is accepted only for the exact recorded certificate.
Successful receipt status, transaction identity, canonical block and every
`CheckpointRegistered` event field are checked before tag-28 feedback.
The daemon independently verifies the finality bundle. The producer then
reads tag 14 and compares the stored evidence bytes with its submitted bundle.

Certificates use an **empty validity proof**. No validity-proof generation is
claimed. Implemented domains are `LXP/v2/checkpoint-certificate\0` and
`LXP/v2/guarantor-attestation\0`; these also apply to the occupancy protocol
header version. The older `LXP1/guarantor-attest` description in section 5 of
the guarantor specification is document drift; existing verifiers and vectors
remain authoritative and unchanged.

## Hosted configuration

Bootstrap generates two distinct signing keys and canonical sorted genesis
members. `deploy-contracts.sh` bonds both through the existing funding and
bond-controller path. `beta-cluster.sh` generates and funds a separate
`paxeer-checkpoint-submitter` key, provisions both mTLS identities and writes
the deployed beta domain to the mounted settlement file, preserving
`vectors`. The domain's `guarantor_bond` is the attestation settlement address;
`settlement_contract` identifies the deployed checkpoint registry. No contract
address is invented. Beta chain id is 125; local tests use disposable 31337.

Both containers run as LNI-authorized UID 4021. Guarantor 1 listens on 9451
and peers with `https://127.0.0.1:9452`; guarantor 2 does the reverse. RPC uses
the existing node-pod relay `127.0.0.1:18545`. The shared nonce lock is
`/var/lib/guarantor-submitter/submitter.lock`. Each identity has separate state,
DA, evidence and key mounts. The launcher restarts only its own child when
bootstrap atomically replaces the identity environment, and retains prior
identity state directories.

Required producer environment:

| Variable | Purpose |
| --- | --- |
| `LAYERX_GUARANTOR_STATE_DIR` | Private writable persistent state |
| `LAYERX_GUARANTOR_KEY_FILE`, `LAYERX_GUARANTOR_ID` | PEM signing key and canonical id |
| `LAYERX_GUARANTOR_LNI_SOCKET` | Authorized node-local Unix socket |
| `LAYERX_GUARANTOR_NODE_CONFIG` | Bootstrap node configuration |
| `LAYERX_GUARANTOR_TLS_CA_FILE`, `LAYERX_GUARANTOR_TLS_CERT_FILE`, `LAYERX_GUARANTOR_TLS_KEY_FILE` | Mutual-TLS identity and CA |
| `LAYERX_GUARANTOR_LISTEN_PORT`, `LAYERX_GUARANTOR_PEER_URL` | Loopback listener and pinned-CA peer |
| `LAYERX_GUARANTOR_SETTLEMENT_FILE`, `LAYERX_GUARANTOR_SETTLEMENT_DOMAIN` | Deployed domain, default name `beta` |
| `LAYERX_GUARANTOR_SUBMITTER_KEY_FILE`, `LAYERX_GUARANTOR_SUBMITTER_LOCK_FILE` | Private hex EVM key and shared lock |
| `LAYERX_GUARANTOR_PYTHON`, `LAYERX_GUARANTOR_SETTLEMENT_HELPER` | Python interpreter and ABI helper |

The launcher also supplies the existing `LAYERX_NODE_*` snapshot, manifest,
registration, identities, network, sequencer authorization and settlement/RPC
variables. `--once` processes one batch with a bounded peer wait;
`--fetch-only` verifies one candidate without opening the replay or signing
runtime. Neither mode disables verification.

## Qualification

`make test-daemon-guarantor-unit` covers real kernel replay, canonical chunk
proofs, codec round trips, root refusal without a signature, bonded-set
refusal, equivocation, real mTLS and ABI/event validation.
`make test-daemon-guarantor-settlement` uses actual contracts on Anvil to
exercise two competing submitter processes, one registration, duplicate
detection and unbonded/stale refusals.

`make test-daemon-guarantor-integration` requires root to launch distinct
LNI identities. It builds a real two-member genesis, deploys actual
contracts, submits real activities, independently qualifies replay from the
daemon's durable body log, and exercises selector 05. When that selector is
available, the subsequent assertions require both producers to attest, one
to register, the second to observe registration, tag-14 evidence equality
and acceptance by the existing `lxp_verify_main` entry point. A refused
candidate is an integration failure, not a skipped test or a passing gate.

No image, cluster or chain-125 deployment qualification is claimed from a
generic build host. Qualification outcomes are recorded in the beta ledger.

## Checkpoint authority publication

The cluster genesis provisioning path generates an Ed25519 authority through
`guarantor.sh --checkpoint-authority-public` in the first guarantor container.
Its private key remains in the shared persistent submitter volume at
`/var/lib/guarantor-submitter/checkpoint-authority.pem`, owned by UID 4021 with
mode 0600. Both producer containers receive this path through
`LAYERX_GUARANTOR_CHECKPOINT_AUTHORITY_KEY_FILE`. Concurrent provisioning and
restarts reuse the same file. Invalid permissions, symlinks and non-Ed25519 keys
are refused without rotating the authority.

Only the public key leaves the container. The provisioning path publishes
Secret `layerx-guarantor-checkpoint-authority` in `TESTNET_NAMESPACE`, with
`public.hex` containing `0x` followed by 64 lowercase hexadecimal characters.
The Human movement policy must consume that public key in its namespace.
The private key is never placed in a Kubernetes Secret or provisioning output.
This provisions authority identity only: settlement witness and deposit-root
publication remain blocked by the native composite-state versus settlement
Merkle-proof contract recorded in the qualification ledger.
