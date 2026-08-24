# LayerX reference applications

The four applications in `reference-apps.json` are complete Node.js projects selected by a checked-in environment profile. Clone this repository, install the locked workspace once with `npm ci`, then use one declared command:

| Application | Emulator | Testnet |
|---|---|---|
| Buyer agent | `npm run start:emulator --workspace @sidiora/layerx-example-buyer-agent` | `npm run start:testnet --workspace @sidiora/layerx-example-buyer-agent` |
| Paid API | `npm run start:emulator --workspace @sidiora/layerx-example-paid-api` | `npm run start:testnet --workspace @sidiora/layerx-example-paid-api` |
| Merchant shop | `npm run start:emulator --workspace @sidiora/layerx-example-merchant-shop` | `npm run start:testnet --workspace @sidiora/layerx-example-merchant-shop` |
| Programs marketplace | `npm run start:emulator --workspace @sidiora/layerx-example-marketplace` | `npm run start:testnet --workspace @sidiora/layerx-example-marketplace` |

Each `layerx.example.json` contains public endpoints, network names, and the names of environment variables that supply account-specific values. Tokens and signing material are never stored in these files and every application runs on Node.js, not in a browser. The buyer, seller, and merchant applications resolve batch authority from the selected environment and independently verify canonical receipts. Pending, Unknown, and Refused remain distinct responses.

`merchant-checkout` remains a workspace compatibility name for consumers created before the canonical `merchant-shop` name. It launches the same receipt-backed service and has its own declared profile.

The marketplace is a real no-std LayerX Program. Its shared listing state, receipt-read grant, receipt replay record, bounded transfer, deletion, and events execute in the Programs runtime. Its launch command builds deterministic WASM with the LayerX CLI, deploys it directly to the endpoint selected by the checked-in profile, resolves the returned receipt, and verifies it before reporting completion. List and buy commands are also declared in its package.

The committed runner checks all four projects with `make platform-test-reference-apps`. A configured emulator or hosted environment can exercise the service journey with `node platform/examples/run-reference-apps.mjs --scenario emulator` or `--scenario testnet`.
