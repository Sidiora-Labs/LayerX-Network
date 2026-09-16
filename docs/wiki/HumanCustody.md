# Human custody and verification

The Human web app is custodial by default. This page is the product
custody model from `human/schema/human-api/identity.kvx`,
`journeys.kvx`, `home.kvx`, and `spec/layerx-platform/spec.kvx`.
Native Bridge custody credit is a different surface:
[Authenticated custody credit](Custody.md). Protocol settlement on
Paxeer is [Finality](Finality.md) and
[Paxeer boundary](PaxeerBoundary.md).

The published tree does not contain
`spec/layerx-agent-interface/spec.kvx`. The agent-boundary spec that
is present is `spec/.beta/layerx-agent-interface/spec.kvx`.

---

## Three identities

`identity.kvx` distinguishes:

1. **Application identity** — the passkey that opens a Human session.
2. **Protocol identity** — the LayerX DID whose primary key is held
   by the custody service.
3. **Linked payout wallet** — a Paxeer EVM address used only at the
   custody boundary.

The browser never holds signing power over the protocol identity.
The wallet is payout and provenance only.

`spec/layerx-platform/spec.kvx` states the same split:

- Human authentication is passkey-first.
- Human and managed-agent DID primary keys are generated and held
  inside `layerx-human-service`'s KMS-backed keystore. They never
  leave it, never reach the browser, and are never derivable from
  the login credential.
- Key export and self-custody are out of scope for v1.
- The custody service signs exclusively through the disclosure-bound
  signing path: the exact canonical byte string plus a structured
  disclosure that must re-encode byte-identically, or the signer
  refuses.
- The Paxeer EVM wallet signs only the domain-separated binding
  statement, the deposit transaction, the withdrawal claim, and the
  emergency exit claim. It is never a login method. Binding grants
  the EVM key no authority over LayerX balances.

Task 5.8 in that spec is recorded as **implemented** with a
production-KMS gap: the service still has a file-envelope development
path. This page does not claim a finished hardware KMS deployment.

---

## Settlement domain

`movement.kvx` names a single settlement domain: `paxeer`.
Custody-boundary journeys (`deposit`, `withdraw`, `exit`) may carry
`settlement_domain`. Internal `move` never names a domain.

`spec/layerx-protocol/spec.kvx` places custody, deposits,
withdrawals, checkpoints, and emergency exit on Paxeer. Deposits
credit after a finalized custody proof. Withdrawals debit LayerX
then pay on Paxeer with a nullifier.

---

## Verification levels

Journey and evidence types use
(`human/schema/human-api/journeys.kvx`):

`unverified` < `receipt-verified` < `checkpoint-finalised` <
`paxeer-finalised`

Declaration order is the ordering. The same enum appears on
`AccountBalance.verification`, `AgentSpend.verification`, and
`VerifiedMoney.verification`. A higher plane must not raise a
level the evidence does not justify.

`EvidenceClass` variants: `local-journey-state`,
`submission-record`, `layerx-receipt`, `checkpoint-proof`,
`paxeer-finality`, `typed-refusal`, `approval-hold`, `wallet-ack`.

`EvidenceMaterial` from `GET /v1/evidence/{evidence_id}` carries
`evidence_id`, `class`, `verification`, `content_type`,
`bytes_base64`, and optional `settlement_domain` (`paxeer` when
present).

Onboarding is active only when the protocol-identity stage carries
`layerx-receipt` at `receipt-verified` or higher. No stage is
presented as done from local state alone
(`identity.kvx` `operation.onboarding.status`).

These four Human-plane labels are not the gateway JSON-RPC
commitment names `executed` / `batched` / `finalised`
([Commitment levels](CommitmentLevels.md)) and are not the public
account-read labels `state_proven` / `checkpoint_finalised` /
`settlement_anchored` ([Public JSON-RPC](PublicRpc.md)). Do not
substitute one vocabulary for another.

---

## Wallet sign moments

When a journey needs a wallet signature it carries `wallet_request`:
`stage_id`, `copy_key`, `from_address`, `to_sign_base64`, optional
`settlement_domain`. Goldens name
`deposit.sign.custody-transaction` and `exit.sign.exit-claim`.
The wallet opens only at those explicit moments.

---

## Authorization classes

`v1.kvx` authorization classes include `read`, `money-movement`,
`approval`, `withdrawal`, `exit`, `security-settings`,
`secret-reveal`, `wallet-rebind`, and `agent-archive`. Withdrawal,
exit, rebind, and secret reveal require step-up evidence.

[Human journeys](HumanJourneys.md) · [Home](Home.md)
