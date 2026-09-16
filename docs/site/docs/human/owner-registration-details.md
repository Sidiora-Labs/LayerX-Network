
Beta recovery uses three dedicated, randomly generated Ed25519 guardian keys,
threshold two, one for each of the roles `guarantor-1`, `guarantor-2` and
`sequencer`. Each guardian is enrolled by its own `platform/hosted/human/guardians.py
enroll` process into its own mode-0700 custody directory
`$SECRETS_DIR/human-guardians/<role>-e<epoch>`, which holds nothing but that
guardian's mode-0600 32-byte seed. Epochs start at one and are bounded at 64.
The enrolling key signs its own binding over `LX:HUMAN:GUARDIAN:BINDING:v1` plus
NUL, big-endian u16 epoch, big-endian u16 role length, the ASCII role, the
32-byte operator identity and its own 32-byte public key. These keys are not
derived from custody keys.

`guardians.py assemble` is a separate process that opens no custody directory.
It reads the public enrollment documents, verifies every signature and publishes
`recovery-guardians.json`, `recovery-guardian-bindings.json` and
`recovery-policy.json`. Rotation enrols the successor at the next epoch and a
separate `guardians.py authorize` process reads only the outgoing guardian's
custody directory to sign `LX:HUMAN:GUARDIAN:ROTATION:v1` over the epoch, role,
operator identity, outgoing key and successor key; assembly rebuilds the whole
chain from epoch one and refuses a missing, mismatched or foreign-signed
authorization.

`recovery-guardians.json` and the identical `recovery-guardian-bindings.json`
carry `version`, `threshold`, `public_keys` and the signed `members`. Each
member has `role`, `identity`, `public_key`, `epoch`, `custody`, `signature` and
`rotation`; `rotation` is `null` at epoch one and otherwise
`{"predecessor": <the member document of the preceding epoch>, "signature":
<rotation signature>}`. `version` equals the highest member epoch. The former
per-member cluster operator label and `independent` flag are gone: the document
states the operator identity, the custody directory, the epoch, the binding
signature and the rotation chain, which are verifiable, rather than a claim of
independence. `owner_native.guardian_set` re-verifies the whole document before
the native producer uses any guardian key, requiring distinct roles, operator
identities, public keys and custody directories, a custody directory named
`<role>-e<epoch>` under `human-guardians`, a published `public_keys` manifest
equal to the verified member keys sorted, and a document threshold equal to the
recovery policy threshold.

The private seeds stay in separate Secrets `layerx-human-guardian-guarantor-1`,
`layerx-human-guardian-guarantor-2` and `layerx-human-guardian-sequencer`, each
published from its own custody directory; `recovery-guardian-bindings.json` is
published as ConfigMap `layerx-human-guardian-bindings`. Three provisioning
processes with disjoint custody directories on one host are not independent
operators: separation here is enforced by directory ownership and signature
verification, not by separate machines or people. Independent operation and one
pod per guardian remain production onboarding requirements. The recovery
commitment is unchanged: SHA-256 of `LX:HUMAN:RECOVERY:v1` plus NUL, big-endian
u16 threshold and count, then sorted Ed25519 public keys. Existing material is
refused instead of replaced.

Run LXIP `provision-owner` first. It returns the exact `did:layerx:act_...` and
recovery policy, not a signing key. `--prepare-owner-admission` then generates
protected owner and pending Ed25519 seeds, and exports the exact DID/key binding
in `owner-admission.txt` and its native account in `owner-admission.json`.
The operator atomically appends the public admission row to the node's protected
`LAYERX_NODE_IDENTITIES` file. A protocol-3 daemon at genesis admits additions
only: existing identities cannot change, new sequences must be zero, and keys
must be canonical Ed25519. Admission closes at the first execution. The producer
must observe the exact DID through native preparation before submitting credit.
The retained identity file remains necessary for restart and replay; checkpoint
identity matching is unchanged.

The cluster starts disposable Paxeer before native genesis. `owner_custody.py
bootstrap` deploys the real governance timelock, asset registry, wrapped native
token and LayerXVault, then invokes the bridge custody-profile verifier. The
native genesis pins that exact profile. After LXIP and owner-key provisioning,
`deposit` wraps real disposable-chain native funds, approves the vault, deposits
for the produced account and invokes `tests/bridge/custody_credit.py` attestation.
Both RPC origins must prove the disposable genesis and CA identity; protected
host endpoints remain refused. `custody-bootstrap.started` and
`custody-deposit.started` prevent blind repetition after unknown outcomes.

`owner-native.json` supplies `node_socket`, `network_id`, `owner_seed_file`,
`pending_seed_file`, `sequencer_public_key`, `layerxctl`, `fee_limit`,
`authority_url`, `authority_token_file`, `authority_ca_file` and
`authority_state_root`. The cluster produces it with container-local paths.
The `owner-producer` container uses UID 4021 for the authenticated LNI peer;
Secret `layerx-human-native-input` supplies the protected inputs, copied into
private mode-0600 files. Authenticated replica records go into the authority's
existing protected state volume. Public activities and receipts are retained
under `owner-native-run`. `human-owner.env` exports the exact produced
`LAYERX_HUMAN_AGENT_ACTOR`, `_AUTHORITY`, `_OWNER_ACCOUNT`, `_RECOVERY_ROOT` and
`_RECOVERY_THRESHOLD`. Never repeat an interrupted producer without reconciling
`owner-native.started` and the retained execution evidence.

Governance ordinal 5 now requires payload version 1: `71 05 01 03`, followed
by a length-prefixed canonical session grant, big-endian `expiry_sequence`
(u64), and a length-prefixed 32-byte `action_key`. Version 0 is refused.
Expiry must be later than execution sequence and the action key must be nonzero.
The canonical grant remains under key `05 || grant_id`; the committed `LXGS2`
summary is stored under `15 || grant_id` and emitted as event `7145`. Its fields
are grant ID, grantor DID hash, owner public key, action key, session key, expiry
sequence, module mask, ordinal minimum/maximum, not-before/not-after milliseconds,
and revocation sequence. Hashes and keys are 32 bytes; numbers are big-endian.

The hosted capability route requires that summary, the native identity snapshot,
a complete receipt suffix, and a node-verified guarantor checkpoint certificate.
It refuses old shapes, expired grants, wrong action/authority/capability bindings,
revoked or changed identity policy, and unsupported scope representations. The
initial positive representation is restricted to the Asset module's ordinals,
with empty counterparties/assets, zero amount ceiling and no claimed native
capability enforcement dimensions. It does not authorize a spend or satisfy the
Human capability install contract's required nonempty monetary bounds.
