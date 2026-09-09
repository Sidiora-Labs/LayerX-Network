# Hosted Human material

Human policy assembly consumes the same generated schema-version-2 asset and
module registry published to `layerx-core-module-registry`. The native producer
uses the module interfaces registered by the daemon. Beta asset metadata lives
next to `ASSET_ID` in `platform/hosted/node/bootstrap.sh`: `LXT`, currency `LXT`,
and 18 protocol decimals. Bootstrap exports these alongside `LAYERX_NODE_ASSET_ID`.
These metadata do not constitute an on-chain asset registration or asset proof.

A fresh cluster generates its Human KMS seal and policy material. Repeating
`up` with `LAYERX_BETA_RETAIN_MATERIAL=1` reuses the complete saved material set.
The driver refuses a missing inventory, missing or extra files, symbolic links,
wrong ownership, files outside mode 0600, or directories outside mode 0700.
Before applying Secrets it compares the live `layerx-human-kms-material`
Secret's `kms-seal` digest with the retained disk file, without printing either
seal or digest. A mismatch refuses bring-up.

The complete inventory is saved after policy assembly, principal provisioning,
and the node registry comparison. A partial earlier bring-up has no complete
inventory and is refused. Retained mode also preserves settlement bindings,
builder digests and principal credentials; it does not regenerate Human policy
or redeploy settlement contracts. `render` validates retained material and the
live seal but only renders manifests, as in fresh mode.

`down` still deletes the cluster (or owner namespaces), including the Human
state PVC, and removes local material. Retention supports repeated `up` against
a live cluster; it is not recovery after `down` or a backup protocol.

The owner agent consumes `LAYERX_AGENT_HUMAN_PEERS` from
`layerx-human-agent-config`. Its encoding is comma-separated
`uid=<u32>;tenant=<tenant>;principal=<principal>` entries. The component binding
uses UID 4020 and the same tenant and DID as the authority principal policy.
The evidence and material assemblers validate this named-field encoding before
publishing it. The owner Programs listener uses pod-local port 9453, separate
from guarantor exchange ports 9451 and 9452.

# Hosted Human evidence

The beta provisioning contract and output sources are documented in
[the hosted Human README](../../platform/hosted/human/README.md#evidence-provisioning).

The owner identity is provisioned through LXIP inside a Job with the runtime identity
PVC. The cluster publishes its v2 registry and guarantor checkpoint public-key Secret,
then provisions and preserves the identity tenant binding before validating
evidence inputs and launching the Job. Protocol registration, the owner request,
recovery policy, custody reference and registry journal pairs still need upstream
producers. The assembler publishes
one complete protected set; unavailable inputs refuse by path. A locally loadable
catalog or passing package tests do not prove deployment or full evidence integration.
