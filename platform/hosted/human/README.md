# Hosted Human service

The API, components, identity/security/movement providers, KMS and Human owner run in the node pod. The service selects `layerx-node` and forwards HTTPS to port 9447. Provider binaries run as UID/GID 4020 and admit component UID 4020. Their sockets are `/run/layerx/human/{identity,security,movement}.sock`, in a 4020-owned 0750 directory. The Human owner runs as UID 4021/GID 4020, matching the native LNI admission policy, with its socket in the separately owned 0750 directory `/run/layerx/human/owner`. The shared process namespace preserves real peer PID checks. The pod is one trusted local boundary; same-UID processes are not isolated from each other.

The retained `layerx-human-state` PVC is mounted by the node. The cluster script deletes the old standalone Human Deployment before applying the node workload and retains the PVC. Private state directories belong to each process; KMS uses UID 4026. The authority state directory belongs to UID 4021. Runtime containers drop all capabilities and use read-only root filesystems. The directory initializer has only CHOWN, FOWNER and DAC_OVERRIDE. It does not read credentials.

Each runtime entrypoint stages its projected Secret material as regular 0600 files in a private memory volume. Identity receives its established recovery policy; security receives sequencer trust history; movement receives the cluster CA and a distinct KMS executor certificate/key. KMS pins that executor independently of the components certificate. The authority initializer stages the Human token, principal policy and the same module registry ConfigMap consumed by the gateway, as UID 4021. The Human token is absent from the legacy authority token list.

Agentd explicitly uses `human-owner` mode and the cluster DER CA for both authority settings. Its authenticated health endpoint verifies the node LNI handshake and each peer's authority registry. This mode does not start the Programs reader. HTTPS hostname checks remain enabled. Movement uses both `paxeer-boundary` and `paxeer-observer-boundary` HTTPS Services with minimum agreement 2. Node egress permits the observer's actual container port 9444, as well as primary port 9443 and the co-resident authority port 9445. `test_material.py` exercises the real observer renderer and topology evaluator, including removal of observer egress.

## Material generation

`material.sh` generates bootstrap cryptographic material, including separately pinned KMS clients. `bootstrap.py` renders the node's initial genesis/settlement workload without Human runtime containers or the optional Human authority configuration group. After contract deployment, `human_policy_publish` assembles `$WORK_DIR/human-policy.json`, assigns `LAYERX_BETA_HUMAN_POLICY_FILE`, generates runtime configuration, publishes Secrets and applies the complete node manifest. It never starts Human from an incomplete policy. Retained encrypted state requires preservation of its matching cryptographic material; the existing cluster-wide secret-regeneration lifecycle is not a key migration mechanism.

`material.py --assemble EVIDENCE_DIR DEPLOYMENT REGISTRY OUTPUT NETWORK CHAIN` takes contract addresses from the actual deployment record and translates the rendered version-2 registry to the KMS module snapshot. It requires these protected, owner-owned 0600 JSON evidence files under `$WORK_DIR/human-evidence`:

- `components.json`: `AGENT_ACTOR`, `AGENT_AUTHORITY`, `AGENT_OWNER_ACCOUNT`, `AGENT_RECOVERY_ROOT` (unpadded base64url), `AGENT_RECOVERY_THRESHOLD`.
- `agent.json`: `HUMAN_PEERS`, `HUMAN_LIMIT_SCOPE`, `HUMAN_LIMIT_SCOPE_ID`, `HUMAN_LIMIT_ID`, `HUMAN_LIMIT_NAME`, `HUMAN_LIMIT_CEILING`, `HUMAN_LIMIT_CONSUMED`. The peer must exactly match `4020:principal:tenant` from the authority binding.
- `authority.json`: `tenant`, `principal`, `core-clock-horizon` (positive sequence horizon).
- `principal-policy.json`: the authority README's complete principal-policy schema. It must contain the scoped tenant/principal.
- `recovery-policy.json`: the identity README's established recovery `root` (32-byte integer array), positive `threshold` and `delay_seconds`. Root and threshold must match components.
- `purpose-catalog.json`: the real `PurposePresetCatalog` accepted by components.
- `movement-policy.json`: `PAXEER_CHECKPOINT_AUTHORITY` and `CUSTODY_REFERENCE` (nonzero 0x-prefixed 32-byte values), positive `PAXEER_CONFIRMATIONS`, `CHECKPOINT_INTERVAL_SECONDS`, `PAXEER_BLOCK_SECONDS`, `REMINDER_INTERVAL_SECONDS`.
- `journal/`: the actual protected Programs admission/deployment pairs. Packaging retains the existing 128-record/512-KiB bounds. Human-owner mode does not load this journal.

Contract fields are derived from `paxeer/deployment.json`: vault, checkpoint registry, withdrawal claims and emergency exit. Missing source files, mismatched network/chain, unversioned registry, missing policy bindings or unsafe files refuse generation. Component limits and movement finality remain enforced by the real consumers. The assembly utility does not establish state proofs or checkpoint finality.

## Remaining integration inputs

The current cluster script does not produce the evidence files listed above or a version-2 live module registry; its existing example-registry copy is incompatible with the authority. These missing producers prevent `human_policy_publish` from succeeding. Movement also documents an incomplete online deposit/withdrawal/exit evidence producer, and several authority routes deliberately refuse absent state/checkpoint proofs. Provider packaging cannot close those source gaps. No complete Human readiness or cluster execution is claimed.

The image builds all real providers, components, service, KMS and agentd. API readiness requires the real component graph; provider probes use real binaries; KMS readiness is exercised through LXKP. `human/apps/web` remains a separate website and is not deployed by this pod.
