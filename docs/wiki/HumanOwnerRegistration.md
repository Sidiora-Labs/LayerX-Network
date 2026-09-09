# Human owner registration

The Human service consumes these configuration inputs:

| Variable | Encoding and source |
| --- | --- |
| `LAYERX_HUMAN_AGENT_ACTOR` | Exact DID admitted for the custody-generated Ed25519 owner key. Bootstrap uses `did:layerx:<lowercase-public-key-hex>`. |
| `LAYERX_HUMAN_AGENT_AUTHORITY` | Lowercase 64-character public-key hex, or `did:layerx:` followed by that same hex. Agentd decodes this to native key32; preparation responses preserve the validated reference so Human can compare it with the request. Uppercase, whitespace, `0x`, DID fragments and malformed lengths are refused. |
| `LAYERX_HUMAN_AGENT_OWNER_ACCOUNT` | Canonical `agent:<actor-DID>:main` name. It must name the account actually established and authority-bound by a successful Bridge Credit, not just a locally derived name. |
| `LAYERX_HUMAN_AGENT_RECOVERY_ROOT` | Unpadded base64url of the exact registered 32-byte recovery commitment. Random bytes or a locally supplied policy do not establish protocol recovery. |
| `LAYERX_HUMAN_AGENT_RECOVERY_THRESHOLD` | Decimal u16 matching the registered recovery policy. |

The node bootstrap input `LAYERX_NODE_IDENTITIES` names a file with rows `hex(DID-UTF8):hex(public-key32):next_sequence`. Bootstrap admission does not create an account, install a recovery policy or produce a registration receipt. Never place private key material in this file.

The intended producer output is `$WORK_DIR/human-evidence-input/owner-registration.json`, with `owner_account`, `authority` and `identity`. This checkout has no `platform/hosted/human/provision.py` or existing `--validate-owner-registration` implementation. The output is not currently produced by this lane.

Hosted principal policy `identities[]` requires evidence `{activity_id, receipt_digest}` linked to the same principal's `activities[]`. The digest is the canonical unsigned protocol receipt digest. Retained evidence includes `receipt_hex` and `replica_document`; verification authenticates the sequencer, batch and receipt inclusion. Identity, capability, rotation and recovery assertions additionally require committed state evidence. The current hosted authority policy route refuses these assertions even after authenticating receipt inclusion.

Native Governance activity ordinals referenced by Human are 7/1 for DID registration, 7/2 for rotation and 7/3 for recovery registration. This checkout does not register a Governance handler in the daemon. An executable recovery encoding and positive policy verifier remain prerequisites for the full producer; this document does not declare them implemented.
