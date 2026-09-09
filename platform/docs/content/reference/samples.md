<!-- Generated from platform/docs/samples.kvx by platform/docs/build/build_site.py. Do not hand-edit. -->

# Sample index

Every code block in this documentation is extracted from one of these directories. The site build re-extracts each block from its source file and fails when a page and its sample disagree, so a stale sample cannot survive a build.

| Sample | Language | Directory | Run it | Integration lines |
|---|---|---|---|---|
| First payment in C# | `csharp` | `platform/docs/samples/first-payment-csharp` | `dotnet build --configuration Release && dotnet run --configuration Release` | 4 of 9 |
| First payment in Go | `go` | `platform/docs/samples/first-payment-go` | `go mod download && go run .` | 9 of 9 |
| First payment in Java | `java` | `platform/docs/samples/first-payment-jvm` | `mvn -q package && mvn -q exec:java` | 6 of 9 |
| First payment in Python | `python` | `platform/docs/samples/first-payment-python` | `python3 -m pip install -r requirements.txt && python3 main.py` | 7 of 9 |
| First payment in Swift | `swift` | `platform/docs/samples/first-payment-swift` | `swift build && swift run FirstPayment` | 5 of 9 |
| First payment in TypeScript | `typescript` | `platform/docs/samples/first-payment-typescript` | `npm install && node index.mjs` | 8 of 9 |
| First payment on Android | `java` | `platform/docs/samples/mobile-payment-android` | `mvn -q package && mvn -q exec:java` | 8 of 9 |
| First payment on iOS | `swift` | `platform/docs/samples/mobile-payment-ios` | `swift build && swift run MobilePayment` | 5 of 9 |
| Paid endpoint on Express | `typescript` | `platform/docs/samples/paid-endpoint-express` | `npm install && node index.mjs` | 8 of 9 |
| Paid endpoint on FastAPI | `python` | `platform/docs/samples/paid-endpoint-fastapi` | `python3 -m pip install -r requirements.txt && uvicorn app:app --host 127.0.0.1 --port 8080` | 7 of 9 |
| Paid endpoint on Spring Boot | `java` | `platform/docs/samples/paid-endpoint-spring` | `mvn -q package && mvn -q spring-boot:run` | 8 of 9 |
| Paid route on Next.js | `typescript` | `platform/docs/samples/paid-route-next` | `npm install && npm run dev` | 8 of 9 |
| Independent receipt verification in Rust | `rust` | `platform/docs/samples/verify-receipt-rust` | `cargo build --release && cargo run --release` | 8 of 9 |

## What each sample needs

| Sample | Requires |
|---|---|
| First payment in C# | .NET 8 or newer, plus LAYERX_API_URL, LAYERX_API_TOKEN, LAYERX_SOURCE, LAYERX_DESTINATION, LAYERX_AMOUNT, LAYERX_CURRENCY and LAYERX_PAYMENT_KEY in the environment. |
| First payment in Go | Go 1.21 or newer, plus LAYERX_API_URL, LAYERX_API_TOKEN, LAYERX_SOURCE, LAYERX_DESTINATION, LAYERX_AMOUNT, LAYERX_CURRENCY and LAYERX_PAYMENT_KEY in the environment. |
| First payment in Java | JDK 21 and Maven, plus LAYERX_API_URL, LAYERX_API_TOKEN, LAYERX_SOURCE, LAYERX_DESTINATION, LAYERX_AMOUNT, LAYERX_CURRENCY and LAYERX_PAYMENT_KEY in the environment. |
| First payment in Python | Python 3.11 or newer, plus LAYERX_API_URL, LAYERX_API_TOKEN, LAYERX_SOURCE, LAYERX_DESTINATION, LAYERX_AMOUNT, LAYERX_CURRENCY and LAYERX_PAYMENT_KEY in the environment. |
| First payment in Swift | Swift 5.9 or newer, plus LAYERX_API_URL, LAYERX_API_TOKEN, LAYERX_SOURCE, LAYERX_DESTINATION, LAYERX_AMOUNT, LAYERX_CURRENCY and LAYERX_PAYMENT_KEY in the environment. |
| First payment in TypeScript | Node.js 22 or newer, plus LAYERX_API_URL, LAYERX_API_TOKEN, LAYERX_SOURCE, LAYERX_DESTINATION, LAYERX_AMOUNT, LAYERX_CURRENCY and LAYERX_PAYMENT_KEY in the environment. |
| First payment on Android | JDK 21 and Maven, the same publishable configuration as the iOS sample, and the move inputs LAYERX_SOURCE, LAYERX_DESTINATION, LAYERX_AMOUNT, LAYERX_CURRENCY and LAYERX_PAYMENT_KEY. The app holds no long-lived credential. |
| First payment on iOS | Swift 5.9 or newer, LAYERX_SERVICE_URL and LAYERX_SESSION_BROKER_URL, at least one LAYERX_EVENT_PUBLIC_KEY_<key-id>, and the move inputs LAYERX_SOURCE, LAYERX_DESTINATION, LAYERX_AMOUNT, LAYERX_CURRENCY and LAYERX_PAYMENT_KEY. The app holds no long-lived credential. |
| Paid endpoint on Express | Node.js 22 or newer and the declared seller keys including `LAYERX_PAYER` (plus grant `LAYERX_PURPOSE_HASH` / `LAYERX_WINDOW_SECONDS` when the scheme is not exact). |
| Paid endpoint on FastAPI | Python 3.11 or newer, uvicorn, and the declared seller keys including `LAYERX_PAYER` (plus grant `LAYERX_PURPOSE_HASH` / `LAYERX_WINDOW_SECONDS` when the scheme is not exact). |
| Paid endpoint on Spring Boot | JDK 21 and Maven. The bundled application.yaml binds the layerx.* properties from LAYERX_PRINCIPAL, LAYERX_PROTECTED_PATH, the LAYERX_RESOURCE_* fields, the payment fields, the five LAYERX_*_STATE_ROOT and batch facts, and the webhook key material. |
| Paid route on Next.js | Node.js 22 or newer and the same declared seller keys as the Express sample. LAYERX_TOKEN is the one declared secret; the bundle scanner fails a build whose client bundle carries it or any NEXT_PUBLIC_ variable holding the same value. |
| Independent receipt verification in Rust | A Rust toolchain, a receipt file named by LAYERX_RECEIPT_FILE, and the batch facts LAYERX_BATCH_ID, LAYERX_BATCH_ASSET, LAYERX_PREVIOUS_STATE_ROOT, LAYERX_RESULTING_STATE_ROOT and LAYERX_SEQUENCER_PUBLIC_KEY as 32-byte hex. |
