# The emulator

The emulator is the local LayerX environment. It is not a mock: it runs the real protocol core transition, produces real receipts, and refuses everything the real chain refuses. What it adds is control - you can set the clock, prefund an account, inject a fault, and snapshot the whole world.

## Starting it

The emulator has no compiled-in signing authority. `layerx emulator provision` generates the sequencer seed under your profile directory, readable only by you, and publishes the matching trust anchor beside it; `layerx emulator up` requires the seed path.

Secure provisioning is supported on Linux, Android and Apple hosts. Other host targets fail closed before writing profile material because they do not provide the atomic owner-only directory publication this command requires.

```
layerx emulator provision
layerx emulator up --sequencer-seed-file "$HOME/.config/layerx/emulator/sequencer.seed"
```

Defaults: listen on `127.0.0.1:9402`, network id `402`, clock at `1700000000000` milliseconds. Override any of them:

```
layerx emulator up \
  --sequencer-seed-file "$HOME/.config/layerx/emulator/sequencer.seed" \
  --listen 127.0.0.1:9402 \
  --network-id 402 \
  --time-ms 1700000000000 \
  --prefund did:layerx:alice,<64-hex-public-key>,1000000
```

`--prefund` may be repeated. Its format is `did,64-hex-public-key,amount`, where the amount is either a plain integer or `high:low` for values beyond 64 bits.

Point your app at it and nothing else changes:

```
export LAYERX_API_URL=http://127.0.0.1:9402
layerx environment use emulator --endpoint http://127.0.0.1:9402 --network-id 402 \
  --sequencer-trust-anchor-file "$HOME/.config/layerx/emulator/sequencer.anchor"
```

The CLI reads the published anchor, fetches the identity the running emulator advertises on `GET /v1/sequencer`, and saves the profile only when the network id and the sequencer public key both agree.

Loopback `http://` is accepted here and only here. Every non-loopback endpoint must be `https://`, in the CLI and in the middleware transport alike, and that is a refusal rather than a warning.

## The protocol surface

| Method and path | Purpose |
|---|---|
| `GET /healthz` | Readiness |
| `GET /v1/sequencer` | Network id and the sequencer public key receipts are signed with |
| `POST /v1/activities` | Submit a canonical activity, receive a receipt |
| `GET /v1/state` | State root, next sequence, batch number, clock, cell and account counts |
| `GET /v1/receipts/{id}` | Exact receipt material |

These are the real endpoints. Receipts fetched here verify with `layerx receipt verify` exactly as production receipts do, against the batch facts the emulator reports.

## The control surface

Everything under `/__emulator` exists only here. It is what makes tests deterministic instead of flaky.

| Method and path | Purpose |
|---|---|
| `POST /__emulator/accounts/prefund` | Fund an account without a faucet |
| `POST /__emulator/time/set` | Set the clock to an exact millisecond |
| `POST /__emulator/time/advance` | Move the clock forward by a delta |
| `POST /__emulator/faults` | Inject a fault |
| `GET /__emulator/snapshot` | Export the whole state |
| `PUT /__emulator/snapshot` | Import a previously exported state |

Fault kinds are `reject`, `drop_receipt` and `corrupt_receipt`, each with an optional `count` (default 1).

Those three are the ones worth building tests around, because they are the failure modes that separate correct integrations from lucky ones:

- **`reject`** - the activity is refused. Does your code release the budget reservation, or does it leak?
- **`drop_receipt`** - the activity may have applied, but no receipt comes back. This is the `unknown` outcome, and the correct behaviour is to leave it unknown and resolve it by receipt lookup under the idempotency key. Code that retries here double-pays in production.
- **`corrupt_receipt`** - a receipt arrives and does not verify. `verification-failure`, not success.

Snapshot export and import make a scenario reproducible: set up the world once, export it, and import it at the start of every test.

## Advancing time

Timeouts, expiries and settlement windows are real, and waiting for them in wall-clock time makes a test suite slow and non-deterministic. Set the clock, submit, advance the clock past the window, and assert. The core sees exactly the timestamp you set.

## What the emulator is not

It is a single local process holding state in memory. It is not a network, so it has no consensus, no other validators, and no external anchoring - `settlement-anchored` is not something a local run establishes. Differential conformance against the hosted testnet is what closes that gap, and it is a separate exercise from local development.

For an environment with real finality and other participants, use the [testnet](environments-testnet.html).

## Enforced by

| Capability | Layer | What that means here |
|---|---|---|
| Atomic settlement | `protocol` | The emulator runs the real core transition; a rejected activity leaves no partial state. |
| Conserved supply | `protocol` | Prefunding creates balance explicitly; nothing else does. |
| Offline receipt verification | `protocol` | Emulator receipts verify by the same pure function as production receipts. |
| Replay refusal | `protocol` | Resubmitting a canonical activity is refused here as it is in production. |
| Unknown is a real outcome | `agent-layer` | `drop_receipt` produces a genuine unknown, so you can test that you keep it unknown. |
| Testnet faucet funding | `hosted-surface` | Not present locally - `--prefund` and the prefund endpoint replace it. |
