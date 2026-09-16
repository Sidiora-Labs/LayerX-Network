# Identity and authority

Identity is kernel state, not a module. The protocol specification
(`spec/layerx-protocol/spec.kvx`, requirement 3) uses the agent DID as the
native account identity. Authorization is evaluated inside the deterministic
state machine: primary keys, session keys, scoped capability grants, rotation,
recovery, revocation, and expiry.

The C header and wiki read of that design are in [Protocol](../protocol/index.md).
Account names derived from a DID are in [Accounts](accounts.md).

## What the protocol requires

Requirement 3 of `spec/layerx-protocol/spec.kvx` binds these rules:

- Every account address is derived from the DID under the namespaces
  `agent:<did>:main`, `agent:<did>:budget:<id>`, `agent:<did>:escrow:<id>`, and
  `agent:<did>:margin:<position>`. An activity whose `actor_did` is not a
  registered identity is rejected.
- An identity binds exactly one primary key and any number of session keys.
  Each session key must carry an explicit expiration, an explicit set of
  permitted activity types, and an explicit revocation sequence.
- A capability grant used as authority must name the authorized holder, the
  asset, the maximum single amount, the total or recurring allowance, the
  expiration, the permitted purpose, the optional service or invoice identifier,
  and the revocation sequence.
- Revocation at a global sequence rejects later activities under that key or
  grant. Already executed activities stay executed.
- Expiry is compared to the batch timestamp, never a node wall clock.
- Possession of a key does not imply authority over an unnamed activity type.
- Primary-key rotation and recovery require the current primary key or a
  pre-registered recovery authority, then a challenge delay measured in batch
  timestamps.
- An optional EVM payout binding is a domain-separated signature naming the DID
  and `network_id`. That EVM key has no authority to move LayerX balances.

Requirement 3 also records Governance activity `7:8` for delegated capability
(kind 3) or budget allowance (kind 4). The payload shape and refusal rules are
in that requirement text; they are not restated here.

## Agent-layer identity

The agent-interface specification (checked in at
`spec/.beta/layerx-agent-interface/spec.kvx`; the platform and beta specs refer
to it as `spec/layerx-agent-interface`) requires `layerx-agentd` to bind each
registered agent to exactly one LayerX DID that exists in current protocol
state. Session tokens authenticate to the daemon only. They are not protocol
authority. See [Agentd](../agents/agentd.md) and [Running an agent](../agents/running.md).

## Human-plane identity

The platform specification (`spec/layerx-platform/spec.kvx`, requirement 3)
establishes a passkey-backed application identity distinct from the LayerX DID
and from any wallet. Human and managed-agent primary keys are generated inside
`layerx-human-service`'s KMS-backed keystore and never reach the browser.
Onboarding is active only after the DID-registration receipt verifies.
Recovery is pre-registered in the same journey.

Hosted principal and session storage is [Hosted identity](../platform/identity.md).
Owner registration encodings are
[Human owner registration](../human/owner-registration.md).
The web application planes are [Human web app](../human/web-app.md).

## Status

DID registration, session keys, grants, rotation, and recovery are specified
and implemented in the protocol kernel and hosted identity service. There is no
LayerX mainnet. Public testnet wallet registration through `layerx wallet create`
is unavailable; that command is emulator-only
([Payments developer path](../overview/payments.md)).
