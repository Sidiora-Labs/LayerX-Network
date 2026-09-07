# LayerX Network for developers

LayerX Network moves money between software at machine speed and hands you evidence you can check without trusting the people who ran the transaction. This site is the developer surface: how to add payments to something you already have, what each guarantee is actually worth, and which layer is holding it up.

Two numbers govern everything here, and both are gates in this repository's build rather than claims on a page.

- Adding LayerX payments takes **fewer than ten lines** of integration code. Every quickstart below is measured by the same build that publishes this page, and the count is printed on the [samples page](reference-samples.html).
- Going from a clean machine to a **verified test payment takes under five minutes**, following only what is published here.

## Start in your language

| Language | Package | Integration size |
|---|---|---|
| [TypeScript](quickstart-typescript.html) | `@sidiora/layerx-sdk` | 8 lines |
| [Python](quickstart-python.html) | `layerx-sdk` | 7 lines |
| [Go](quickstart-go.html) | `platform/sdk/go` | 9 lines |
| [Java and Kotlin](quickstart-jvm.html) | `com.sidiora.layerx:layerx-sdk` | 6 lines |
| [Swift](quickstart-swift.html) | `LayerXSDK` | 5 lines |
| [C#](quickstart-csharp.html) | `LayerX.Sdk` | 4 lines |
| [Rust](quickstart-rust.html) | `layerx-sdk` and `layerx-proof` | 8 lines |

Every SDK is generated from the same two schemas in the same build. The idiom differs; the wire behaviour, the error taxonomy and the receipt verification do not.

## Or start in your framework

| Framework | What you get | Integration size |
|---|---|---|
| [Express](framework-express.html) | A paid route and a verified webhook endpoint | 8 lines |
| [Next.js](framework-next.html) | A paid route handler, plus a bundle scanner that fails a build shipping a secret | 8 lines |
| [FastAPI](framework-fastapi.html) | A paid route and a verified webhook endpoint | 7 lines |
| [Spring Boot](framework-spring.html) | Auto-configured payment gate and webhook filter | 8 lines |
| [iOS](framework-ios.html) | Payments from an app holding no long-lived credential | 5 lines |
| [Android](framework-android.html) | Payments from an app holding no long-lived credential | 8 lines |

## What to read next

- [Money and accounts](concepts-money.html) if you want the model before the code.
- [Paying for things](concepts-paying.html) for the quote-commit-journey shape every surface shares.
- [Receipts and verification](concepts-receipts.html) if the interesting part is proving a payment happened.
- [Who enforces what](concepts-enforcement.html) before you rely on any guarantee in production.

## Enforced by

The four labels on this site are not stylistic. They tell you what survives a compromise of everything above the named layer.

| Capability | Layer | What that means here |
|---|---|---|
| Atomic settlement | `protocol` | A payment applies completely or not at all, whatever your client, the daemon or a gateway does. |
| Offline receipt verification | `protocol` | You can check a settlement claim with no LayerX component in the path. |
| Quote then commit | `service` | The ordering is enforced by `layerx-human-service`; it binds callers of that service. |
| API keys, usage and request logs | `hosted-surface` | An operational control of the hosted deployment, not a protocol property. |

The [enforcement reference](reference-enforcement.html) lists every capability this documentation states, grouped by the layer that holds it.
