# Money and accounts

There is one ledger. Every balance on it belongs to a principal, and the only thing that ever changes a balance is an authenticated 402LXP transfer applied by the kernel transfer primitive. That single sentence is the reason most of the guarantees on this site are worth anything.

## Amounts

An amount is a decimal string of base units, and it always travels with its currency code. It is never a JSON number, because a JSON number is a double and a double silently loses money.

```text
{ "amount": "1250000", "currency": "USD" }
```

Each SDK maps that to the widest exact integer its language has: `u128` in Rust, `bigint` in TypeScript, `int` in Python, `BigInteger` on the JVM, and a checked decimal string type everywhere else. If you find yourself parsing an amount into a float, stop; the SDK already gave you an exact type.

## Principals and accounts

A principal is who the protocol thinks is acting. An account is the balance record that principal owns. On the human plane you never see either directly: you see your own profile, the counterparties you move money to, and journeys describing what happened. On the agent plane you address accounts directly through `read.account` and `read.balance`.

## Movements

Money inside LayerX Network moves as one of four mechanisms, and the vocabulary is fixed everywhere - API, logs and user-facing copy alike:

| Mechanism | What it does |
|---|---|
| `fund` | Money enters a container from its owner |
| `allocate` | Money is set aside for a specific purpose |
| `return` | Allocated money goes back to where it came from |
| `transfer` | Money changes owner |

`deposit`, `withdraw` and `exit` name journeys that cross the Paxeer custody boundary, and never an internal movement. Keeping those two vocabularies apart is what stops a bridge event from being read as a settled payment.

You do not choose a mechanism. You say who gets how much, and the route resolver picks. See [Paying for things](concepts-paying.html).

## What the protocol will refuse

- A transfer that would take a balance below zero.
- A state change that alters a balance outside a 402LXP transfer. The transition aborts.
- A resubmission of an activity whose sequence has already been consumed.
- A spend past a funded budget in the budget module.

None of those are checks your code performs. They are transitions that do not exist.

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Conserved supply | `protocol` | The kernel transfer primitive is the only balance mutation. Total supply is conserved by construction, not by reconciliation. |
| Atomic settlement | `protocol` | An activity applies completely or not at all. |
| Protocol budget ceilings | `protocol` | A budget is a protocol object. Spending past it is refused by the transition function, not by a policy layer you could bypass. |
| Done means verified | `service` | The human service reports a journey as done only against a verified receipt or a Paxeer finality proof. |
