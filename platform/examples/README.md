# LayerX reference applications

The four applications in `reference-apps.json` are complete Node.js projects selected by a checked-in environment profile. Clone this repository, install the locked workspace once with `npm ci`, then use one declared command.

Asset encodings, public RPC, and 402 commitment extras are documented on
[`docs/wiki/PaymentsQuickstart.md`](../../docs/wiki/PaymentsQuickstart.md)
and [`docs/wiki/CommitmentLevels.md`](../../docs/wiki/CommitmentLevels.md).

| Application | Emulator | Testnet |
|---|---|---|
| Buyer agent | `npm run start:emulator --workspace @sidiora/layerx-example-buyer-agent` | `npm run start:testnet --workspace @sidiora/layerx-example-buyer-agent` |
| Paid API | `npm run start:emulator --workspace @sidiora/layerx-example-paid-api` | `npm run start:testnet --workspace @sidiora/layerx-example-paid-api` |
| Merchant shop | `npm run start:emulator --workspace @sidiora/layerx-example-merchant-shop` | `npm run start:testnet --workspace @sidiora/layerx-example-merchant-shop` |
| Programs marketplace | `npm run start:emulator --workspace @sidiora/layerx-example-marketplace` | `npm run start:testnet --workspace @sidiora/layerx-example-marketplace` |

Each `layerx.example.json` contains public endpoints, network names, and the names of environment variables that supply account-specific values. Tokens and signing material are never stored in these files and every application runs on Node.js, not in a browser. The buyer, seller, and merchant applications resolve batch authority from the selected environment and independently verify canonical receipts. Pending, Unknown, and Refused remain distinct responses.

`merchant-checkout` remains a workspace compatibility name for consumers created before the canonical `merchant-shop` name. It launches the same receipt-backed service and has its own declared profile.

The marketplace is a real no-std LayerX Program. Its shared listing state, receipt-read grant, receipt replay record, bounded transfer, deletion, and events execute in the Programs runtime. Its launch command builds deterministic WASM with the LayerX CLI, deploys it directly to the endpoint selected by the checked-in profile, resolves the returned receipt, and verifies it before reporting completion. List and buy commands are also declared in its package.

The committed runner checks all four projects with `make platform-test-reference-apps`.

## Running the whole journey

`node platform/examples/run-reference-apps.mjs --scenario emulator` is self-contained on any host that has the `layerx` CLI built. It resolves the binary from the first of `$LAYERX_BIN`, `build/bin/layerx`, and `layerx` on `PATH`; runs `layerx emulator provision` into a scratch profile directory; creates four real signing keys with `layerx key create`; starts `layerx emulator up` on the loopback address the buyer profile names, prefunded against the published sequencer trust anchor; waits for `GET /healthz` to report `ready`; and terminates the emulator and deletes the scratch profile when the run ends, including on failure.

Every `LAYERX_EMULATOR_*` input is then derived from that run instead of being hand-set:

| Input | Derived from |
|---|---|
| `LAYERX_EMULATOR_ASSET` | the `authority.asset` of a real seeding transfer receipt read back from `GET /v1/receipts/{id}` |
| `LAYERX_EMULATOR_SOURCE` | `agent:<did>:main` for the key `layerx key create reference-buyer` produced |
| `LAYERX_EMULATOR_SELLER`, `LAYERX_EMULATOR_MERCHANT` | the canonical account identifiers `GET /v1/state` reports for those DIDs |
| `LAYERX_EMULATOR_MARKETPLACE_RECEIPT_DIGEST` | the receipt digest of the same seeding transfer |
| `LAYERX_EMULATOR_MARKETPLACE_PROGRAM_ID`, `LAYERX_EMULATOR_MARKETPLACE_LISTING_ID` | one value per run, reused across the deploy, list, and buy steps |
| `LAYERX_EMULATOR_TOKEN`, `LAYERX_EMULATOR_PAYMENT_KEY`, `LAYERX_EMULATOR_MERCHANT_CHECKOUT_KEY`, `LAYERX_EMULATOR_MARKETPLACE_KEY` | operating-system randomness, fresh for every run |
| `LAYERX_EMULATOR_WEBHOOK_PUBLIC_KEYS_JSON` | a generated Ed25519 key pair; the run publishes only its public key |
| `LAYERX_EMULATOR_PRICE` | `1000`, the one value that is a seller's own choice rather than protocol material |

Any of those variables that is already set in the environment is used verbatim, so an owner can override any single input. `LAYERX_EMULATOR_SEED_FILE` skips provisioning and starts the emulator from an existing seed, reading the trust anchor published beside it. `LAYERX_CREDENTIAL_STORE` defaults to `file` with a per-run passphrase so the run needs no operating-system keyring; set `LAYERX_CREDENTIAL_STORE` and `LAYERX_CREDENTIAL_PASSPHRASE` to use your own credential storage.

`--scenario testnet` is unchanged and still expects the `LAYERX_TESTNET_*` inputs. `LAYERX_EXAMPLE_ENDPOINT` overrides the hosted endpoint every testnet profile carries, so the same runner can be pointed at a beta cluster gateway URL.
