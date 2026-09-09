
Beta recovery uses three dedicated, randomly generated Ed25519 guardian keys,
threshold two. Each public key is bound to one genesis guarantor identity or the
sequencer identity in `recovery-guardian-bindings.json`, published as ConfigMap
`layerx-human-guardian-bindings`. The private seeds stay in separate Secrets
`layerx-human-guardian-guarantor-1`, `layerx-human-guardian-guarantor-2` and
`layerx-human-guardian-sequencer`. These keys are not derived from custody keys.
All three guardians are cluster-operated and non-independent. Independent
operation is a production onboarding requirement. The recovery commitment is
SHA-256 of `LX:HUMAN:RECOVERY:v1` plus NUL, big-endian u16 threshold and count,
then sorted Ed25519 public keys. Existing material is refused instead of replaced.

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
