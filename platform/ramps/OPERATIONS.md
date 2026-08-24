# Ramp operator and recovery runbook

## Enablement

Keep the deployment disabled until the custody, compliance, provider and LayerX owners have published the exact production coordinates. Provision dedicated least-privilege identities for identity introspection, compliance decisions, provider settlement, gateway activity write/receipt read, independent receipt authority, LayerX activity signing and Paxeer inventory broadcast. Put each token or PKCS#12 password in a separate read-only secret file with mode `0600`. Configure only KMS/HSM key handles; private keys must not enter the pod, configuration, journal, API or browser.

The shipped NetworkPolicy permits only cluster DNS, trusted-boundary pods in the ramp or LayerX platform namespace, and the internal Paxeer RPC pods. Terminate any owner-approved external provider connection in a dedicated mTLS egress boundary labelled `layerx-plane=trusted-boundary`; do not widen ramp-pod egress to the public Internet. The service names in `config.example.json` are deployment coordinates for those boundaries, not test fallbacks.

The LayerX signer receives only the 32-byte `layerx-signature-preimage-v1` digest encoded as standard padded base64 at `POST /v1/signatures`; the request carries the configured non-exporting handle and `algorithm: ed25519`, and the response carries only the base64 signature. Provision the handle for LayerX activity and direct-send authorization signing and make the configured public key an independently reviewed fact. The ramp verifies every returned signature before submission.

Verify the quote catalog is operator-owned and immutable for its validity interval. Quotes use integer minor units and a rational numerator/denominator; floating-point conversion is not accepted. Rotate quotes instead of editing a quote used by an existing order.

## Worker procedure

The reconciler automatically performs receipt/status reads for provider and LayerX pending/unknown states. It never performs a second submission. Operator workers use `/internal/v1/work` to:

1. obtain the signed compliance decision;
2. submit the first required leg after the previous durable stage is present;
3. supply `account_sequence` from the authoritative account-state control plane for `submit_layerx`: the operator debit sequence for an on-ramp direct send and the operator receiver sequence for an off-ramp payer-grant draw;
4. leave transport loss in `submitted_unknown` for automatic receipt/status reconciliation.

The operator token is not a customer token and the worker service is the only NetworkPolicy principal allowed to reach internal routes.

## Refusals, unknowns and reversals

Do not retry a `provider_submission_planned`, `provider_submitted_unknown`, `layerx_submission_planned` or `layerx_submitted_unknown` record by creating new identifiers. Reconcile the provider operation under `OrderDigest` or the LayerX activity under its persisted activity ID. A provider refusal, compliance refusal or LayerX refusal is terminal until a separately reviewed new order and quote are created.

Manual-review decisions remain non-terminal. Signed provider callbacks are deduplicated by callback ID and exact body facts. Conflicting reuse is rejected. Out-of-order callbacks cannot move a settled/reversed workflow backwards.

If an external payout or credit reverses after the LayerX receipt, the service reports `reversed`, never `done`. Follow the operator's disclosed reversal agreement. Any compensating LayerX transfer is a new ordinary-principal activity with its own customer authorization and must not rewrite the original receipt or journal.

## Paxeer inventory

Paxeer rebalancing is independent from customer orders. Submit only through `/internal/v1/rebalances` using the configured operator account, wallet, vault and remote signer handle. The journal records intent before broadcast and preserves broadcast-unknown. Poll the exact transaction to bridge-required finality. A changed inclusion block is recorded as displaced and must return to non-final handling; do not credit inventory from a stale inclusion. Paxeer is the only custody workflow described as LayerX custody.

## Recovery

On restart, the service acquires a non-blocking operating-system exclusive lock on the mode-`0600` writer-lock file and verifies every journal sequence, predecessor hash and record digest before listening. The kernel releases the advisory lock on graceful exit or process death; the lock file itself remains and must never be deleted to bypass a live writer. Corruption, truncation ambiguity, insecure permissions, concurrent writers or a conflicting idempotency binding fails startup. Restore a journal only from the operator's authenticated durable volume and reconcile all planned/unknown/pending records before enabling new submissions. Never edit journal lines.

Readiness indicates local configuration and journal availability, not provider settlement, LayerX finality, Paxeer finality or successful qualification. Production activation requires the protected sandbox journey and human fault/reorg qualification.
