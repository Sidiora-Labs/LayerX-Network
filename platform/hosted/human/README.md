# Hosted Human service

The API, components, identity/security/movement providers, KMS and Human owner run in the node pod. The service selects `layerx-node` and forwards HTTPS to port 9447. Provider binaries run as UID/GID 4020 and admit component UID 4020. Their sockets are `/run/layerx/human/{identity,security,movement}.sock`, in a 4020-owned 0750 directory. The Human owner runs as UID 4021/GID 4020, matching the native LNI admission policy, with its socket in the separately owned 0750 directory `/run/layerx/human/owner`. The shared process namespace preserves real peer PID checks. The pod is one trusted local boundary; same-UID processes are not isolated from each other.

The retained `layerx-human-state` PVC is mounted by the node. The cluster script deletes the old standalone Human Deployment before applying the node workload and retains the PVC. Private state directories belong to each process; KMS uses UID 4026. The authority state directory belongs to UID 4021. Runtime containers drop all capabilities and use read-only root filesystems. The directory initializer has only CHOWN, FOWNER and DAC_OVERRIDE. It does not read credentials.

Each runtime entrypoint stages its projected Secret material as regular 0600 files in a private memory volume. Identity receives its established recovery policy; security receives sequencer trust history; movement receives the cluster CA and a distinct KMS executor certificate/key. KMS pins that executor independently of the components certificate. The authority initializer stages the Human token, principal policy and the same module registry ConfigMap consumed by the gateway, as UID 4021. The Human token is absent from the legacy authority token list.

Agentd explicitly uses `human-owner` mode and the cluster DER CA for both authority settings. Its authenticated health endpoint verifies the node LNI handshake and each peer's authority registry. This mode does not start the Programs reader. HTTPS hostname checks remain enabled. Movement uses both `paxeer-boundary` and `paxeer-observer-boundary` HTTPS Services with minimum agreement 2. Node egress permits the observer's actual container port 9444, as well as primary port 9443 and the co-resident authority port 9445. `test_material.py` exercises the real observer renderer and topology evaluator, including removal of observer egress.

## Material generation

`material.sh` generates bootstrap cryptographic material, including separately pinned KMS clients. `bootstrap.py` renders the node's initial genesis/settlement workload without Human runtime containers or the optional Human authority configuration group. After contract deployment, `human_policy_publish` assembles `$WORK_DIR/human-policy.json`, assigns `LAYERX_BETA_HUMAN_POLICY_FILE`, generates runtime configuration, publishes Secrets and applies the complete node manifest. It never starts Human from an incomplete policy. Retained encrypted state requires preservation of its matching cryptographic material; the existing cluster-wide secret-regeneration lifecycle is not a key migration mechanism.

`material.py --assemble EVIDENCE_DIR DEPLOYMENT REGISTRY OUTPUT NETWORK CHAIN` takes contract addresses from the actual deployment record and translates the rendered version-2 registry to the KMS module snapshot. It requires these protected, owner-owned 0600 JSON evidence files under `$WORK_DIR/human-evidence`:

- `components.json`: `AGENT_ACTOR`, `AGENT_AUTHORITY`, `AGENT_OWNER_ACCOUNT`, `AGENT_RECOVERY_ROOT` (unpadded base64url), `AGENT_RECOVERY_THRESHOLD`.
- `agent.json`: `HUMAN_PEERS`, `HUMAN_LIMIT_SCOPE`, `HUMAN_LIMIT_SCOPE_ID`, `HUMAN_LIMIT_ID`, `HUMAN_LIMIT_NAME`, `HUMAN_LIMIT_CEILING`, `HUMAN_LIMIT_CONSUMED`. The single peer must exactly match `uid=4020;tenant=<tenant>;principal=<principal>` from the authority binding, in that field order. Tenant is 1–128 ASCII letters, digits, hyphens or underscores. Principal requires `did:<method>:<id>` with a nonempty lowercase ASCII letter/digit method and nonempty identifier, at most 255 UTF-8 bytes. Whitespace, control characters, commas, semicolons, extra or duplicate fields, positional entries and additional peers are refused.
- `authority.json`: `tenant`, `principal`, `core-clock-horizon` (positive sequence horizon).
- `principal-policy.json`: the authority README's complete principal-policy schema. It must contain the scoped tenant/principal.
- `recovery-policy.json`: the identity README's established recovery `root` (32-byte integer array), positive `threshold` and `delay_seconds`. Root and threshold must match components.
- `purpose-catalog.json`: the real `PurposePresetCatalog` accepted by components.
- `movement-policy.json`: `PAXEER_CHECKPOINT_AUTHORITY` and `CUSTODY_REFERENCE` (nonzero 0x-prefixed 32-byte values), positive `PAXEER_CONFIRMATIONS`, `CHECKPOINT_INTERVAL_SECONDS`, `PAXEER_BLOCK_SECONDS`, `REMINDER_INTERVAL_SECONDS`.
- `journal/`: the actual protected Programs admission/deployment pairs. Packaging retains the existing 128-record/512-KiB bounds. Human-owner mode does not load this journal.

Contract fields are derived from `paxeer/deployment.json`: vault, checkpoint registry, withdrawal claims and emergency exit. Missing source files, mismatched network/chain, unversioned registry, missing policy bindings or unsafe files refuse generation. Component limits and movement finality remain enforced by the real consumers. The assembly utility does not establish state proofs or checkpoint finality.

## Remaining integration inputs

The cluster script generates the version-2 module registry using the node image tool and bootstrap asset metadata. It does not produce the evidence files listed above; those missing producers prevent `human_policy_publish` from succeeding. Registry image execution remains unqualified here. Movement also documents an incomplete online deposit/withdrawal/exit evidence producer, and several authority routes deliberately refuse absent state/checkpoint proofs. Provider packaging cannot close those source gaps. No complete Human readiness or cluster execution is claimed.

The image builds all real providers, components, service, KMS and agentd. API readiness requires the real component graph; provider probes use real binaries; KMS readiness is exercised through LXKP. `human/apps/web` remains a separate website and is not deployed by this pod.

## Offline owner provisioning

With the identity provider stopped, `layerx-human-identity-provider provision-owner` uses the same `LAYERX_HUMAN_IDENTITY_PROVIDER_STATE_ROOT` and `LAYERX_HUMAN_IDENTITY_PROVIDER_RECOVERY_POLICY_FILE` as `bind-device`. Supply stdin JSON with exactly `email`, `display_name`, `idempotency_key`, and `now` (unsigned seconds), at most 16384 bytes. The command acquires the existing exclusive state lock and calls the LXIP operation-1 implementation. Repeating the same idempotency key and identity returns the durable owner; conflicting inputs refuse.

Compact JSON stdout contains exactly `principal`, `did`, `recovery_root` (32-byte array), `recovery_threshold`, and `recovery_delay_seconds`. These are all five fields returned by LXIP op 1. It does not return an authority reference, protocol owner account, capability evidence or rotation/recovery key-policy receipts because the underlying operation creates none. Its output therefore cannot by itself produce `components.json` or `principal-policy.json`. Preserve the same state for the runtime provider; a host-only state root is not a deployed identity.

Recovery receipt ingest verifies signed historical receipt inclusion; no operator key-set derivation is required for provisioning. The corrected provisioning hook runs after identity/gateway provisioning and before policy publication. Identity principal responses now echo the required tenant. The catalog uses treasury and sequencer authority accounts. Missing external producer inputs still prevent a complete evidence set.

## Owner registration input contract

The protocol registration producer must write `$WORK_DIR/human-evidence-input/owner-registration.json` before evidence assembly. It must be an absolute canonical path to an invoking-UID-owned regular file, mode 0600, one link, at most 1 MiB. Missing, malformed, duplicate-field or unprotected JSON refuses with that exact path; input values are never printed. Validate it with `python3 platform/hosted/human/provision.py --validate-owner-registration --work-dir "$WORK_DIR"`.

The object has exactly `owner_account`, `authority`, and `identity`. `owner_account` is a nonzero lowercase 64-digit hexadecimal H32. `authority` is the producer's complete AuthorityRef string, passed through unchanged; the current AuthorityRef constructor only requires nonempty text. This input additionally refuses control characters. Do not invent an authority encoding or derive it from the LXIP principal. `identity` is the complete `identities[]` object documented in `platform/hosted/authority/README.md`: exactly `did`, `authorities`, `revocation_sequence`, `frozen`, `evidence`, `capabilities`, `rotation`, `recovery`, including every nested field. H32s use lowercase canonical text; capabilities use U16 activity types, U64 expiry, and a decimal U128 amount string. Nested unknown fields, duplicate capability bindings, unlisted capability authorities and invalid key delays refuse. `owner_registration` can additionally check evidence activity membership against the principal policy and DID equality against the LXIP result. Standalone validation does not verify receipt inclusion or establish live registration.

Recovery root, threshold and delay must be copied exactly from `provision-owner`; no operator key-set derivation is required or provided. The registration input supplies protocol account and authority policy independently of those LXIP fields.

## Protected catalog and Job staging

`provision.py --catalog --work-dir "$WORK_DIR" --registry "$SECRETS_DIR/module-registry.json" --treasury "$WORK_DIR/human-evidence-input/treasury.json" --sequencer "$WORK_DIR/human-evidence-input/sequencer.json" --asset "$LAYERX_NODE_ASSET_ID" --output "$WORK_DIR/purpose-catalog.json"` generates the runtime catalog from its identifier-free template. Inputs must satisfy the protected-file checks; the v2 registry must contain the selected asset. Account IDs come from the existing protocol derivation. The checked-in template is not itself a loadable catalog.

The provisioning Job converts the exported treasury and sequencer DIDs through the real protocol account function and saves separate protected `treasury.json` and `sequencer.json` files. It does not modify node bootstrap.

Source `provision.sh` and invoke `human_owner_provision` in the cluster script's environment to stage the real `provision-owner-job.yaml`. Before any cluster mutation it validates protected `human-evidence-input/owner-request.json` and `human-evidence-input/recovery-policy.json`; the latter is the provider's established policy, not a derived operator key set. The request has exactly the four fields documented above. The function refuses existing result files, enabled Human runtime containers, multiple bootstrap pods and unscheduled bootstrap pods. It pins the Job to the bootstrap pod's node to use its ReadWriteOnce PVC and uses the runtime's identical `identity` subPath and state-root environment. The provider's exclusive state lock remains authoritative. Job retries are disabled. It waits for completion, captures the result privately, checks the exact five-field single-line result and recovery-policy equality, then publishes `$WORK_DIR/human-owner-result.json`. No Job logs or input values are printed. Failed or repeated attempts require explicit state reconciliation; the function does not delete a Job or overwrite a result.

The Job requires the bootstrap initializer to have created the PVC identity directory. Its input Secret is `layerx-human-provision-owner-input`; its image comes from `image_ref layerx-human`. The established recovery policy is required before LXIP opens state. Kubernetes execution remains unqualified on the build server.

`provision.py --preserve-binding --work-dir "$WORK_DIR" --request REQUEST --response RESPONSE --output OUTPUT` preserves the response tenant and matching sub only after checking both against the creation request. All files must be protected and output creation is exclusive.

## Evidence provisioning

`human_evidence_provision` runs after identity and gateway provisioning and before
`human_policy_publish` and enabling the Human node containers. The owner Job uses
the same PVC subPath and identity state root as the runtime. It refuses an already
enabled Human runtime. Recovery is taken unchanged from the established input
policy and LXIP result; no recovery key-set derivation is performed.

The protected `human-evidence-input/owner-registration.json` and
`recovery-policy.json` must be supplied by the protocol registration producer,
along with `owner-request.json`. The assembler validates the registration's complete
identity entry, DID binding and evidence references. The authority currently has
no independent config-validation command: the Python validator reparses the exact
serialized principal policy against its documented schema. It does not certify
registration receipt evidence.

| Published file | Source |
| --- | --- |
| `components.json` | LXIP owner DID/recovery plus registration authority/account |
| `authority.json` | Identity response binding and beta owner clock horizon |
| `agent.json` | Same binding, beta owner limit, registration account and verified first-batch head |
| `principal-policy.json` | Registration identity and its evidence activities, configured budgets, deployed asset |
| `recovery-policy.json` | Unchanged LXIP root, threshold and delay |
| `purpose-catalog.json` | Identifier-free template, v2 module registry, node asset and treasury/sequencer accounts |
| `movement-policy.json` | Protected movement custody reference, guarantor public key and owner timing policy |
| `journal/` | Unmodified pairs from `LAYERX_REGISTRY_JOURNAL` |

`beta-owner-policy.json` defines an agent-scoped limit with the registration account
as scope ID. Its limit ID names configuration, not a deployed budget. Its activity
selector includes only activity IDs supplied by registration evidence; the empty
budget allowlist grants no budgets.

`provision-account` reads `{"did":"did:layerx:<public key>"}` on stdin and calls
`layerx_wire::hash::account_id_for_protocol` with protocol 3. The Job invokes it
separately for the treasury and sequencer keys from the bootstrap exports; it
writes no key material to logs. `validate-account-head` verifies the head's receipt
inclusion and signed header against the bootstrap sequencer pin, network and first
batch. It emits only `{"consumed":0}`; later batches refuse. The fetch uses the
agent boundary `/v1/protocol/account-state/head` with the registry bearer token.

Custody is read from
`$SECRETS_DIR/human/movement-config/LAYERX_HUMAN_MOVEMENT_PROVIDER_CUSTODY_REFERENCE`.
If absent, `deployment.json` must contain a produced `custody_reference`; contract
addresses are not converted into references. The guarantor source is Secret
`layerx-guarantor-checkpoint-authority`, key `public.hex`. Missing journal pairs,
custody, registration or first-batch evidence refuse without publishing a partial
`human-evidence` directory. Existing sets are never overwritten. Publication uses
a private sibling staging directory, fsync and one rename under an exclusive lock.

Run local generated-set material integration only against a complete real input set:

```sh
python3 platform/hosted/human/provision.py --qualify-generated-set \
  --work-dir "$WORK_DIR" --registry "$SECRETS_DIR/module-registry.json" \
  --secrets-dir "$SECRETS_DIR" --network "$NODE_NETWORK_ID" --chain "$PAXEER_CHAIN_ID"
```

This runs the unchanged material assembler and reader on the generated set, followed
by `test_material.py`. Absence is a failure, not a skipped test. The generated-catalog
Rust test uses freshly generated Ed25519 keys, the real protocol derivation and the
production module list with test-owned v2 registry metadata; it proves parser
loadability, not deployed registry availability.
