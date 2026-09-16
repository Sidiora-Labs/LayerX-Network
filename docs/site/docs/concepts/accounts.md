# Accounts

Every locked, escrowed, budgeted, or margined unit sits in a real named
account. The protocol specification (`spec/layerx-protocol/spec.kvx`,
requirement 10) forbids representing immobilized value as a reserved or pending
field on another account.

The Asset module's encodings and per-asset records are in
[Assets](assets.md). Module IDs and the money map are in
[Modules](../protocol/modules.md).

## Namespace

Requirement 10 accepts exactly these namespaces (plus the protocol-3 extensions
below):

| Name | Role |
| --- | --- |
| `agent:<did>:main` | Native-asset account for a DID |
| `agent:<did>:budget:<id>` | Budget ceiling |
| `agent:<did>:escrow:<id>` | Escrow hold |
| `agent:<did>:margin:<position>` | Perps margin |
| `system:liquidity:<market>` | Market liquidity |
| `system:insurance` | Insurance pool |
| `system:fees` | Fee treasury |
| `system:paxeer-reserve` | Paxeer reserve mirror |
| `system:paxeer-withdrawals` | Withdrawal staging |

An identifier is a domain-separated commitment over the canonical encoding of
that string. The wiki and kernel also document per-asset accounts
`agent:<did>:asset:<lowercase hex64 asset_id>` and issuance accounts stored as
`module:asset:value:<issuance-account-id>` ([Assets](assets.md),
[Modules](../protocol/modules.md)).

Protocol 3 additionally derives:

- `agent:<signed actor DID>:escrow|budget|stream:<lowercase hex64 object id>`
  on signed Escrow OPEN, Budget CREATE, and Stream OPEN
- `system:liquidity:<lowercase hex64 market id>` and
  `system:funding:<lowercase hex64 market id>:long|short` on signed Perps
  `MARKET_CREATE`

Those rules are requirement 10 `ac_11` of `spec/layerx-protocol/spec.kvx`.

## Identifier hash

Named accounts use:

```text
account_id32 = SHA-256("LX:ACCOUNT:v1" || u32_be(name_length) || name_bytes)
```

All hexadecimal text in account names is lowercase. See [Assets](assets.md).

## Sequences

The activity envelope's `account_sequence` is checked against the DID's
identity counter. A transfer set's `actor_sequence` is checked against the
sequence account's own counter — by default the debited record, so per
`(DID, asset)`. `asset.receive` advances the recipient's counter,
`asset.mint` the issuance account's, and the protocol fee leg the treasury's.
A newly opened per-asset account starts at zero however far its owner's
`:main` account has run ([Modules](../protocol/modules.md)).

## Subaccount debit rule

Requirement 10 rejects a debit from an agent budget, escrow, margin, or stream
subaccount that presents only an owner direct signature or a session key. Those
subaccounts are debited only under the escrow authority, budget allowance, or
protocol-module capability that owns them. System accounts are created only by
genesis or a governance activity.

## Agent-layer types

The agent-interface specification (`spec/.beta/layerx-agent-interface/spec.kvx`,
requirement 3) represents account identifiers only through the protocol
namespaces and rejects construction of any identifier outside that namespace at
the type boundary.
