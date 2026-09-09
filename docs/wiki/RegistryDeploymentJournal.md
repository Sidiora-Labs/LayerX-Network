# Registry deployment journal

`POST /__registry/deployments` accepts a canonical signed Programs deploy or
upgrade activity as an octet-stream body under the existing registry bearer
policy. It submits those exact bytes through `LAYERX_REGISTRY_LNI_SOCKET` when configured,
or through the authenticated `LAYERX_REGISTRY_NODE_ENDPOINT` boundary, and
requests native proof-bundle tag 16, payload version 1, selector 4, followed by
the 32-byte activity ID. Tag 17 returns the canonical `DeploymentProof` as its
payload and an empty proof-material field. Selectors 1–3 remain unchanged.

The proof carries the signed activity and its inclusion path, signed receipt
and its inclusion path, signed batch header, Programs subtree root and outer
membership path, exact program-record leaf and membership path, and lifecycle
membership or adjacent absence witnesses. The node derives all witnesses from
its real kernel and retained batch evidence. It refuses a failed deployment or
a live state root different from the authenticated proof root. Historical state
reconstruction is not provided by this selector.

Protocol 3 uses `LayerX/programs/deployment-proof/v2\0`: the v1 fields followed
by a length-prefixed occupancy-maintenance receipt and length-prefixed Merkle
proof. Both receipts must be included in the same signed header at their exact
indices and count. The maintained-outcome verifier binds the activity transition
to occupancy maintenance; Programs witnesses bind to the signed final root.
The deployment receipt root need not equal that final root. Legacy v1 encoding
and protocol 1/2 refusal checks remain unchanged.

The registry verifies the full proof with its protected sequencer history and
checks the receipt against the independent receipt authority before publishing.
An admission acknowledgement alone never produces a deployment record. An
unavailable or indeterminate result remains unavailable; clients reconcile with
the same signed activity and idempotency key.

`LAYERX_REGISTRY_JOURNAL` holds the existing sealed `.envelope` commit units.
The `pairs/` directory exports `<unsigned-receipt-digest>.admission` using
`DeploymentProof::canonical_encoding()` and the matching `.deployment` using
`DeploymentRecord::canonical_encoding()`, with lowercase 64-digit digest names.
These are precisely the legacy pair encodings consumed by Human. Files are
mode 0600, the export directory is mode 0700, and each file is staged, fsynced,
renamed and followed by a directory fsync. The envelope remains the atomic
commit authority. Verified startup replay regenerates exports, including an
interrupted pair; a consumer presented an incomplete pair must refuse it.
Temporary export files remain outside `pairs/`.

Configure the Human assembler's journal input to the exported `pairs/`
directory, not the internal journal containing envelopes and head metadata.
The boundary exposes registry-only `POST /internal/v1/programs/deploy` and
`/upgrade` for canonical admission, and `GET /internal/v1/deployment-proof/<activity-id>`
for selector 4. Submission returns the native acknowledged activity ID; proof
responses retain native refusals or carry canonical proof bytes as `proof_hex`.
The existing node bearer and outbound CA settings authenticate this bridge.
The registry network policies allow only its required boundary port 9443.
The evidence assembler still runs on the invoking host and needs journal export. Protocol-3 trust histories are accepted explicitly alongside legacy versions 1/2.
