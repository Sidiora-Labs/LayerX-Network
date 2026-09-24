# Human owner registration

The Human service consumes these configuration inputs:

| Variable | Encoding and source |
| --- | --- |
| `LAYERX_HUMAN_AGENT_ACTOR` | Exact LXIP-generated DID, admitted for its separately generated protected Ed25519 owner key before the first native execution. |
| `LAYERX_HUMAN_AGENT_AUTHORITY` | Lowercase 64-character public-key hex, or `did:layerx:` followed by that same hex. Agentd decodes this to native key32; preparation responses preserve the validated reference so Human can compare it with the request. Uppercase, whitespace, `0x`, DID fragments and malformed lengths are refused. |
| `LAYERX_HUMAN_AGENT_OWNER_ACCOUNT` | Canonical `agent:<actor-DID>:main` name. It must name the account actually established and authority-bound by a successful Bridge Credit, not just a locally derived name. |
| `LAYERX_HUMAN_AGENT_RECOVERY_ROOT` | Unpadded base64url of the exact registered 32-byte recovery commitment. Random bytes or a locally supplied policy do not establish protocol recovery. |
| `LAYERX_HUMAN_AGENT_RECOVERY_THRESHOLD` | Decimal u16 matching the registered recovery policy. |

The node bootstrap input `LAYERX_NODE_IDENTITIES` names a file with rows `hex(DID-UTF8):hex(public-key32):next_sequence`. Bootstrap admission does not create an account, install a recovery policy or produce a registration receipt. Never place private key material in this file.

The producer output is `$WORK_DIR/human-evidence-input/owner-registration.json`, with native H32 `owner_account`, textual `authority` and `identity`. `platform/hosted/human/provision.py` provides producer and validator modes. The local flow is qualified through a real vault deposit, native credit, Governance receipts, authenticated replica evidence and restart. Live cluster execution and positive checkpoint qualification remain outstanding.

Hosted principal policy `identities[]` requires evidence `{activity_id, receipt_digest}` linked to the same principal's `activities[]`. The digest is the canonical unsigned protocol receipt digest. Retained evidence includes `receipt_hex` and `replica_document`; verification authenticates the sequencer, batch and receipt inclusion. Identity, capability, rotation and recovery assertions additionally require committed state evidence. The hosted authority requires node-verified guarantor checkpoint certificates for positive identity, capability and key-policy responses; absent or mismatched certificates remain refused.

The native daemon and genesis builder register Governance ABI 1 for protocol 3 in matching order. The independent guarantor runtime still omits Governance and refuses this genesis with result -902; its snapshot check remains intact.

## Native Governance registration encoding

Governance ABI 1 registers module 7 ordinals 1, 2, 3, 5 and 6. The exact LXIP actor and its custody key must already be admitted through the protected native identity file. Registration certifies that existing actor/key into committed Governance state; it does not create an arbitrary new bootstrap identity.

Payloads use big-endian integers: `7101 0002 || did_id32 || primary_key32`; `7102 0004 || did_id32 || pending_key32 || not_before_ms:u64 || not_after_ms:u64 || effective_sequence:u64`; `7103 0003 || did_id32 || recovery_root32 || threshold:u16`. Recovery additionally accepts `7103 0005` followed by the same fields and explicit `required_delay_seconds:u64 || maximum_delay_seconds:u64`. The three-field form establishes no delay-policy evidence. Rotation is an announcement through the existing pending-key admission semantics.

Session registration is versioned: `7105 0103 || u32be(length) || canonical_2001_grant || expiry_sequence:u64 || u32be(32) || action_key32`. The old `7105 0001` shape is refused. Expiry must exceed execution sequence and the action key must be nonzero. Only canonical session-key grants are accepted, bound to the actor's current revocation generation and signed by the owner envelope. Revoke is `7106 0003 || grant_id32 || reason:u8 || effective_sequence:u64`; its effective sequence must equal execution's sequence. No grant or policy mutation is accepted from an unrelated owner.

Governance stages its state through module KV writes. The identity key is did_id32. Session and revoke keys are respectively `05 || grant_id32` and `06 || grant_id32`. Versioned session metadata is stored at `15 || grant_id32` and emitted in `7145` as the `LXGS2` summary documented in [Guardian enrollment, rotation and cluster bootstrap](#guardian-enrollment-rotation-and-cluster-bootstrap) below. Event `7110` contains the exact 223-byte committed identity value: `LXGI1`, DID32, primary32, revocation:u64, recovery_root32, threshold:u16, pending32, rotation_begin:u64, rotation_end:u64, rotation_effective_sequence:u64, rotation_revision:u64, recovery_revision:u64, recovery_delay_seconds:u64, recovery_maximum_seconds:u64, rotation_delay_ms:u64, rotation_maximum_ms:u64, execution_sequence:u64. Session bytes are split across events `7105` and `7125` to preserve the existing 256-byte effect bound; revoke emits `7106`.

`provision.py --produce-owner-registration --work-dir PATH` consumes protected `human-evidence-input/owner-native.json`, custody key references, `custody-credit.bin`, `recovery-policy.json`, `recovery-guardians.json` and the LXIP `human-owner-result.json`. It uses the real `layerxctl read-state`/`submit` client, checks successful signed native receipts, asks the HTTPS receipt authority to verify and retain their authorized batches, and validates the output with the existing owner-registration contract. Preserve its exclusive `owner-native-run` directory for reconciliation after failure. The cluster script generates and mounts these inputs in the ordered provisioning flow; its live rollout has not run on this build server.

The hosted identity verifier reports `checkpoint_finalised` only for matching native snapshots, complete retained receipt coverage and authenticated native checkpoint evidence. The positive test is blocked by the guarantor runtime's missing Governance registration; registration JSON validation alone does not prove this level.

`owner-native.json` has exactly `node_socket`, `network_id`, `owner_seed_file`, `pending_seed_file`, `sequencer_public_key`, `layerxctl`, `fee_limit`, `authority_url`, `authority_token_file`, `authority_ca_file` and `authority_state_root`. The producer requires owner-only regular input files (0600), a real local node socket, an absolute CLI path and HTTPS authority with a trusted CA. Key files contain custody-produced seed32 and must already match the exact post-LXIP admission binding. The LXIP result and recovery-policy file must agree exactly.

`recovery-guardians.json` contains `version`, `threshold`, `public_keys` and the signed `members`, and is the only guardian document the native producer input Secret and the container input list mount. `public_keys` remains a unique list of real lowercase H32 guardian public keys, and its root is SHA-256 of `LX:HUMAN:RECOVERY:v1` followed by NUL, threshold u16be, guardian-count u16be, then sorted public-key32 values. The threshold must not exceed the guardian count. Each member carries `role` (`guarantor-1`, `guarantor-2` or `sequencer`), the H32 operator `identity`, the H32 `public_key`, the u16 `epoch`, the absolute `custody` directory `<secrets>/human-guardians/<role>-e<epoch>` holding only that guardian's mode-0600 seed, the `signature` over `LX:HUMAN:GUARDIAN:BINDING:v1` plus NUL, epoch u16be, role-length u16be, role, identity32 and public-key32, and `rotation`, which is `null` at epoch one and otherwise the preceding member document with the `LX:HUMAN:GUARDIAN:ROTATION:v1` signature that authorized the successor. `version` equals the highest member epoch. `owner_native.guardian_set` verifies every binding and the whole rotation chain before the producer uses any key, and requires distinct roles, operator identities, public keys and custody directories, a custody directory whose name matches the role and epoch it claims, a `public_keys` manifest equal to the verified member keys sorted, and a document threshold equal to the recovery policy threshold. `recovery-guardian-bindings.json` holds the identical document and remains the ConfigMap copy. [Guardian enrollment, rotation and cluster bootstrap](#guardian-enrollment-rotation-and-cluster-bootstrap) below describes the per-guardian enrollment, rotation and publication flow. The real custody attestation producer supplies `custody-credit.bin`, bound to this owner account/key and the node's immutable custody profile.

## Guardian enrollment, rotation and cluster bootstrap

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
bootstrap` deploys nothing on the disposable Paxeer chain: custody is the native
`layerxcustody` module behind the precompile at
`0x0000000000000000000000000000000000001013`, configured in the Paxeer genesis.
It invokes the bridge custody-profile verifier, which proves the module's asset
mapping and writes a profile pinning the precompile address and the module
identity. The native genesis pins that exact profile. After LXIP and owner-key
provisioning, `deposit` calls `deposit(bytes32)` on the precompile with real
disposable-chain native funds for the produced account and invokes
`tests/bridge/custody_credit.py` attestation. On an Anvil chain the same script
keeps the timelock, asset registry, wrapped token and LayerXVault path.
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

## Hosted provisioning tests

`make human-test-hosted-provisioning` builds `layerx-human-identity-provider` from the separate `human/` Cargo workspace into `$(HUMAN_TARGET_DIR)/debug` and then runs `python3 -m pytest platform/hosted/human -q` with `LAYERX_HUMAN_IDENTITY_PROVIDER_BIN` set to that binary. `HUMAN_TARGET_DIR` is `CARGO_TARGET_DIR` when the environment sets one and `human/target` otherwise, and it may be overridden on the make command line; the target points the tests at whichever directory it built. `LAYERX_HUMAN_IDENTITY_PROVIDER_BIN` is the one variable `platform/hosted/human/test_provision.py` and `platform/hosted/human/test_owner_request.py` use to locate the provider; its default is `human/target/debug/layerx-human-identity-provider`, and an absent or non-regular binary refuses with that path instead of skipping the three tests that drive the real provider. `make human-test` runs this target, so the hosted Human provisioning suite is reachable from the aggregate Human test gate and from `workspace-test`; the target requires `pytest`, `PyYAML` and `cryptography` in the invoking `python3` and refuses with that list when any of them is absent.
