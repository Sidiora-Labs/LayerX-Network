# x402 transport

`layerx-x402` is the interop adapter for x402 v2. Seller issuance is
edge translation; settlement success requires a gateway-verified
canonical LayerX Network receipt (`interop/crates/layerx-x402/src/seller.rs:1-3`;
`interop/crates/layerx-x402/src/facilitator.rs:1-3`). Interop adapters
translate foreign protocols into LayerX-shaped evidence and never write
balances (`interop/README.md:3`). The adapter id is `x402`, the spec version is
`2.0.0`, and the pin is upstream revision
`7d5363a6d51750dc246041f2b0ed5819dd46a0d7`
(`interop/crates/layerx-x402/src/lib.rs:21-22, 39-50`).
The compiled SHA-256 comment names `specs/x402-specification-v2.md`
(`interop/crates/layerx-x402/src/lib.rs:23`); the bytes compiled in are
`specs/vendor/x402/x402-specification-v2.md`
(`interop/crates/layerx-x402/src/lib.rs:28-29`). Those two paths differ.

Wire version is `X402_VERSION = 2`
(`interop/crates/layerx-x402/src/model.rs:12`). Monetary values are
decimal JSON strings on the wire and exact `u128` in memory; there is
no floating-point path (`interop/crates/layerx-x402/src/model.rs:1-3,
23-27`).

This page covers that crate, the gateway routes that mount it, and the
local conformance matrix. It does not cover AP2, UCP, Visa TAP, or fiat.

---

## Payment-required flow

A seller issues a 402 payment-required signal. A buyer selects one
accepted offer and returns a payment payload. Settlement reports
success only after `layerx_proof::receipt::verify` accepts a canonical
LayerX receipt and `GatewayCore::settle_with_receipt` records
`TranslationStatus::ReceiptVerified`.

Seller HTTP signal (`interop/crates/layerx-x402/src/seller.rs:113-124`):

| Field | Value |
| --- | --- |
| `status` | `402` |
| `header` | Standard-base64 JSON of the validated `PaymentRequired` body |
| `body` | The same `PaymentRequired` value |

Buyer construction (`interop/crates/layerx-x402/src/buyer.rs:91-146`):
decode the payment-required header, pick the first `accepts` entry
whose `(scheme, network)` is in the buyer's closed `SupportedKind`
list (seller order), ask `BuyerPaymentPlane::construct` for a JSON
object scheme payload, echo required extensions byte-for-value, encode
the `PaymentPayload` as the payment-signature header.

Facilitator messages (`interop/crates/layerx-x402/src/facilitator.rs:30-37,
54-65, 78-85`): `/verify`, `/settle`, and `/supported` share JSON
bodies on every transport. `/verify` is a read-only translation
(`TranslationKind::ReadOnly` at
`interop/crates/layerx-x402/src/facilitator.rs:232-277`). `/settle` is
state-changing (`TranslationKind::StateChanging` at
`interop/crates/layerx-x402/src/facilitator.rs:279-353`). The plane
trait splits `verify` and `settle` so the read-only route cannot call
settlement (`interop/crates/layerx-x402/src/facilitator.rs:182-207`).

---

## Header and body encodings

Header names (`interop/crates/layerx-x402/src/model.rs:13-15`):

| Constant | Wire name | Value |
| --- | --- | --- |
| `PAYMENT_REQUIRED_HEADER` | `PAYMENT-REQUIRED` | `PaymentRequired` |
| `PAYMENT_SIGNATURE_HEADER` | `PAYMENT-SIGNATURE` | `PaymentPayload` |
| `PAYMENT_RESPONSE_HEADER` | `PAYMENT-RESPONSE` | `SettlementResponse` |

HTTP payment signals: serde JSON, then RFC 4648 standard base64, bound
at `MAXIMUM_HEADER_BYTES = 64 * 1_024` JSON bytes
(`interop/crates/layerx-x402/src/codec.rs:8-28`;
`interop/crates/layerx-x402/src/transport.rs:259-268`). Empty or
oversize header text is `Bounds`; invalid base64 or JSON is `Decode`.

MCP and A2A payment signals: the same validated JSON objects as
`TransportValue::Json`, bound at `MAXIMUM_JSON_BYTES = 64 * 1_024`
(`interop/crates/layerx-x402/src/transport.rs:1-4, 19, 269, 293-310`).
A transport/value mismatch (HTTP value on MCP, JSON on HTTP, wrong
header name) is `Decode`
(`interop/crates/layerx-x402/src/transport.rs:277-290`).

Facilitator `/verify`, `/settle`, and `/supported` are JSON bodies on
HTTP, MCP, and A2A. They are not the three payment headers
(`interop/crates/layerx-x402/src/transport.rs:150-157`;
`interop/crates/layerx-x402/COMPATIBILITY.md:3-4, 15`).

Structs use `serde(deny_unknown_fields, rename_all = "camelCase")`
(`interop/crates/layerx-x402/src/model.rs:80, 137, 180, 213, 241`).
`Seller::payment_required` always calls `encode_header` (HTTP header
form) even when the gateway route is MCP or A2A
(`interop/crates/layerx-x402/src/seller.rs:118-123`). Transport-neutral
encode/decode of the same core values is `transport.rs`, not
`Seller::payment_required`.

`PaymentRequirements` fields: `scheme`, `network` (CAIP-2
`namespace:reference`), `amount`, `asset`, `payTo`,
`maxTimeoutSeconds`, optional `extra`
(`interop/crates/layerx-x402/src/model.rs:136-147`). `layerx_facts`
requires namespace `layerx` and 32-byte hex `asset` / `pay_to`
(`interop/crates/layerx-x402/src/model.rs:165-176`).

Successful settlement body
(`interop/crates/layerx-x402/src/seller.rs:269-287`;
`interop/crates/layerx-x402/src/facilitator.rs:468-486`):

| Field | Value |
| --- | --- |
| `success` | `true` |
| `errorReason` | absent |
| `payer` | hex of `protocol.from()` |
| `transaction` | `lxp:` plus hex receipt digest |
| `network` | accepted requirements network |
| `amount` | accepted amount |
| `extensions.layerx.receipt` | standard-base64 canonical receipt |
| `extensions.layerx.receiptDigest` | hex digest |
| `extensions.layerx.verificationLevel` | literal `sequencer-signed` |

Pending settlement: `success` false, `errorReason`
`settlement_pending`, non-empty `transaction`
(`interop/crates/layerx-x402/src/facilitator.rs:397-411`;
`interop/crates/layerx-x402/src/model.rs:274-289`). Other failures:
`success` false, a safe `errorReason`, empty `transaction`.

---

## Roles

Codify anchors (`interop/crates/layerx-x402/src/lib.rs:52-68`):

| Role | Anchor | What the role verifies |
| --- | --- | --- |
| Seller | `x402-v2-receipt-verified-seller` | Offer validity; `PAYMENT-SIGNATURE` decodes and matches an issued `accepts` entry and required extensions; plane execution; canonical receipt verifies and binds asset, recipient, and amount; gateway records `ReceiptVerified` |
| Buyer | `x402-v2-evidence-bound-buyer` | `PAYMENT-REQUIRED` decodes; a supported `(scheme, network)` exists; scheme payload is a JSON object; `PAYMENT-RESPONSE` success carries `extensions.layerx` with `verificationLevel` `sequencer-signed`; local `verify` plus digest/`lxp:` transaction binding |
| Facilitator | `x402-v2-receipt-backed-facilitator-http-mcp-a2a` | `/supported` declaration; `/verify` read-only; `/settle` success unreachable without matching locally verified protocol evidence; settlement identity independent of transport and separated by `SettlementStep` |

Local matrix rows (`interop/crates/layerx-x402/src/transport.rs:48-67`;
`interop/crates/layerx-x402/COMPATIBILITY.md:5-9`):

| Transport | Buyer | Seller | Facilitator | Evidence |
| --- | --- | --- | --- | --- |
| HTTP | yes | yes | yes | `TRANSPORT_MATRIX[0]`; `tests/transports.rs` |
| MCP | yes | yes | yes | `TRANSPORT_MATRIX[1]`; `tests/transports.rs` |
| A2A | yes | yes | yes | `TRANSPORT_MATRIX[2]`; `tests/transports.rs` |

`every_role_round_trips_on_every_transport` asserts every matrix row
has buyer, seller, and facilitator true, and round-trips
payment-required, payment payload, settlement, facilitator request,
verify response, facilitator settlement, and supported response on
HTTP, MCP, and A2A
(`interop/crates/layerx-x402/tests/transports.rs:84-165`).

Gateway route registration
(`interop/crates/layerx-interop-gateway/src/server.rs:184-224`):

| Method | Path | Route |
| --- | --- | --- |
| `GET` | `/v1/{http\|mcp\|a2a}/x402/supported` or `.../facilitator/supported` | `X402Supported` |
| `POST` | `/v1/{http\|mcp\|a2a}/x402/buyer/build` | `X402BuyerBuild` |
| `POST` | `/v1/{http\|mcp\|a2a}/x402/seller/offer` | `X402SellerOffer` |
| `POST` | `/v1/{http\|mcp\|a2a}/x402/verify` or `.../facilitator/verify` | `X402Verify` |
| `POST` | `/v1/{http\|mcp\|a2a}/x402/settle` or `.../{seller\|facilitator}/settle` | `X402Settle` |

`X402Settle` is the only x402 state-changing route
(`interop/crates/layerx-interop-gateway/src/server.rs:107-118`). The
service dispatches those variants onto `x402_supported`, `x402_buyer`,
`x402_seller`, `x402_verify`, and `x402_settle`
(`interop/crates/layerx-interop-service/src/server.rs:414-423`).
Startup requires adapters `x402`, `ap2`, `ucp`, `visa-tap`, `fiat` and
transports `http`, `mcp`, `a2a`
(`interop/crates/layerx-interop-service/src/config.rs:31-32`). x402
evidence policy is `LayerXReceipt`
(`interop/crates/layerx-interop-service/src/config.rs:537-540`). The
configured pin must be version `2.0.0` and `X402_SPEC_SHA256`
(`interop/crates/layerx-interop-service/src/config.rs:482-490`).

---

## Typed refusals

`X402Error` (`interop/crates/layerx-x402/src/model.rs:297-314`). No
variant carries external payload bytes or secrets
(`interop/crates/layerx-x402/src/model.rs:295-296`).
`PaymentPending` is declared and Display-mapped
(`interop/crates/layerx-x402/src/model.rs:309, 333`) and is not
constructed anywhere in `src/`. Pending settlement is
`SellerOutcome::Pending`, `FacilitatorSettlementOutcome::Pending`, or
`errorReason` `settlement_pending`, not `X402Error::PaymentPending`.

| Refusal | Condition that raises it |
| --- | --- |
| `Decode` | Empty/invalid standard-base64 header; JSON parse failure; HTTP/MCP/A2A representation mismatch (`interop/crates/layerx-x402/src/codec.rs:18-28`; `interop/crates/layerx-x402/src/transport.rs:277-310`) |
| `Encode` | serde JSON serialize failure (`interop/crates/layerx-x402/src/codec.rs:10-11`; `interop/crates/layerx-x402/src/transport.rs:294-298`; seller/facilitator canonicalization) |
| `WrongVersion` | `x402Version != 2` (`interop/crates/layerx-x402/src/model.rs:344-349`; `interop/crates/layerx-x402/src/facilitator.rs:42-43`) |
| `Bounds` | Header/JSON over `64 KiB`; URL not `http://` or `https://` or containing CR/LF/NUL; text/tag/extension limits; settlement network/transaction/payer/reason bounds (`interop/crates/layerx-x402/src/codec.rs:12-13, 19-26`; `interop/crates/layerx-x402/src/model.rs:98-123, 260-272, 352-360, 385-392`; `interop/crates/layerx-x402/src/transport.rs:295-308`) |
| `InvalidAmount` | Amount empty, longer than 39 digits, leading zero on length greater than 1, non-digits, or `u128` overflow (`interop/crates/layerx-x402/src/model.rs:35-46`) |
| `InvalidRequirements` | Bad scheme/network/asset/payee, amount `0`, `maxTimeoutSeconds == 0`, empty or more than 32 `accepts`, bad error text, or non-32-byte hex in `layerx_facts` (`interop/crates/layerx-x402/src/model.rs:152-160, 194-204, 396-398`) |
| `InvalidPayload` | Payment `payload` not a JSON object; buyer zero idempotency key; non-object scheme payload from the plane; failed settlement missing `errorReason`; pending settlement with empty `transaction`; failed settlement with non-empty `transaction` when reason is not `settlement_pending`; facilitator zero `stable_identity`; verify response where `is_valid == invalid_reason.is_some()` (`interop/crates/layerx-x402/src/model.rs:234-235, 278-288`; `interop/crates/layerx-x402/src/buyer.rs:107-108, 129-130`; `interop/crates/layerx-x402/src/facilitator.rs:140-141, 491-507`) |
| `UnsupportedOffer` | `layerx_facts` namespace is not `layerx`; buyer empty/duplicate/oversized support list or no matching `(scheme, network)`; facilitator kind not in `/supported` (`interop/crates/layerx-x402/src/model.rs:172-174`; `interop/crates/layerx-x402/src/buyer.rs:76-85, 112-121`; `interop/crates/layerx-x402/src/facilitator.rs:355-364`) |
| `RequirementsMismatch` | Payload `accepted` is not an issued `accepts` entry; facilitator payload `accepted` != request `payment_requirements` (`interop/crates/layerx-x402/src/seller.rs:204-209`; `interop/crates/layerx-x402/src/facilitator.rs:47-48`) |
| `ExtensionsMismatch` | A required payment-required extension is missing or unequal on the payload (`interop/crates/layerx-x402/src/seller.rs:210-213`) |
| `PaymentRefused` | Facilitator `begin_translation` returns `Refused` on `/verify`; buyer `capture_settlement` sees `success == false`; service verify-plane `settle` / settle-plane `verify` (`interop/crates/layerx-x402/src/facilitator.rs:262-263`; `interop/crates/layerx-x402/src/buyer.rs:164-165`; `interop/crates/layerx-interop-service/src/server.rs:659-664, 729-734`) |
| `EvidenceMissing` | Successful settlement with empty `transaction` or with `errorReason`; gateway settle status is not `ReceiptVerified`; facilitator pending/refused after an already `ReceiptVerified` open (`interop/crates/layerx-x402/src/model.rs:274-276`; `interop/crates/layerx-x402/src/seller.rs:266-267`; `interop/crates/layerx-x402/src/facilitator.rs:328-329, 465-466`) |
| `EvidenceMismatch` | `verify` fails; receipt lacks protocol body; receipt asset/to/amount != requirements; buyer evidence not `sequencer-signed`, bad receipt base64, payer/network/amount/digest/`lxp:` mismatch; facilitator refused after already `ReceiptVerified` (`interop/crates/layerx-x402/src/seller.rs:243-254`; `interop/crates/layerx-x402/src/buyer.rs:174-203`; `interop/crates/layerx-x402/src/facilitator.rs:334-335, 442-453`) |
| `Gateway(_)` | Adapter id, translation request, `begin_translation`, `refuse_translation`, `complete_read_only`, or `settle_with_receipt` failure |

Named conditions that are **not** `X402Error` variants:

| Condition | What the code does |
| --- | --- |
| Malformed payment header | `Decode` or `Bounds` from `decode_header` (`interop/crates/layerx-x402/src/codec.rs:18-28`). Seller `settle` and buyer `build_payment` / `capture_settlement` all start there (`interop/crates/layerx-x402/src/seller.rs:144`; `interop/crates/layerx-x402/src/buyer.rs:110, 162`). |
| Wrong network or asset | Invalid CAIP-2 → `InvalidRequirements`. Namespace not `layerx` → `UnsupportedOffer`. Receipt `asset`/`to` != hex requirements → `EvidenceMismatch`. Buyer/facilitator unsupported `(scheme, network)` → `UnsupportedOffer`. Service transfer vs requirements → plane reason `payment_requirements_mismatch` (`interop/crates/layerx-interop-service/src/server.rs:614-620`). |
| Insufficient amount | Amount `0` → `InvalidRequirements`. Receipt amount != accepted amount → `EvidenceMismatch`. There is no `InsufficientAmount` variant. Seller tests feed plane reason `insufficient_balance` (`interop/crates/layerx-x402/tests/seller.rs:192-194`; `interop/crates/layerx-x402/tests/e2e.rs:485-487`). Service transfer amount mismatch is `payment_requirements_mismatch`, not an x402 amount error. |
| Expired authorization | The crate does not compare `max_timeout_seconds` to `now`. `max_timeout_seconds == 0` is `InvalidRequirements`. The service plane refuses when `observed_at < not_before` or `observed_at > not_after` or `observed_at > expires_at` with reason `activity_time_window_refused` (`interop/crates/layerx-interop-service/src/server.rs:621-626`). |
| Replayed authorization | Same settlement identity retries through `GatewayCore::settle_with_receipt`: identical digest returns the existing `ReceiptVerified`; a different digest is `GatewayError::IdempotencyConflict` (`interop/crates/layerx-interop-gateway/src/gateway.rs:446-454`). Facilitator identity omits transport (`interop/crates/layerx-x402/src/facilitator.rs:117-133, 134-152`). Tests: `duplicate_delivery_of_the_same_settlement_does_not_double_charge`, `a_swapped_receipt_for_a_settled_identity_is_refused`. |
| Signature failures | `layerx_proof::receipt::verify` maps every `VerificationFailure` (including `SequencerSignature` and `MissingSignature`) to `EvidenceMismatch` (`interop/crates/layerx-x402/src/seller.rs:243-244`; `interop/crates/layerx-x402/src/facilitator.rs:442-443`; `interop/crates/layerx-x402/src/buyer.rs:180-181`; `agent/crates/layerx-proof/src/receipt.rs:224-235, 297-305`). The crate does not expose `ReceiptCheck` to x402 callers. Service verify also maps `verify_submission` failure to plane reason `activity_authorization_refused` (`interop/crates/layerx-interop-service/src/server.rs:602-609`). |
| Receipt binding failures | `EvidenceMismatch` / `EvidenceMissing` as in the table above. Gateway `settle_with_receipt` independently calls the same `verify` (`interop/crates/layerx-interop-gateway/src/gateway.rs:412-445`). |

Safe plane reasons: lowercase ASCII plus `_`, length `1..=64`; otherwise
rewritten to `payment_refused`
(`interop/crates/layerx-x402/src/seller.rs:317-327`;
`interop/crates/layerx-x402/src/facilitator.rs:575-586`).

Service verify/settle plane reasons
(`interop/crates/layerx-interop-service/src/server.rs:586-626, 742-772`):
`typed_intent_required`, `protocol_idempotency_required`,
`activity_authorization_refused`, `protocol_idempotency_mismatch`,
`typed_transfer_required`, `payment_requirements_mismatch`,
`activity_time_window_refused`, `unsupported_offer`.

---

## Receipt binding

A settled request binds to a LayerX receipt in this order.

1. Plane returns `ExecutedPayment { canonical_receipt, authorised_batch }`
   (`interop/crates/layerx-x402/src/seller.rs:46-50`). The adapter does
   not construct LayerX payload bytes
   (`interop/crates/layerx-x402/src/seller.rs:27-29`).
2. Seller `settled` and facilitator `settle_executed` call
   `layerx_proof::receipt::verify(&canonical_receipt, &authorised_batch)`
   (`interop/crates/layerx-x402/src/seller.rs:243-244`;
   `interop/crates/layerx-x402/src/facilitator.rs:442-443`).
3. `verify` runs `verify_outcome` then refuses `result_code != 0`
   (`agent/crates/layerx-proof/src/receipt.rs:224-235`).
   `verify_outcome` decodes, re-encodes, checks protocol shape, version,
   operation, activity id, batch/asset/roots, debit/credit, and Ed25519
   sequencer signature
   (`agent/crates/layerx-proof/src/receipt.rs:246-310`).
4. Protocol `asset`, `to`, and `amount` must equal
   `PaymentRequirements::layerx_facts()` plus `amount.value()`
   (`interop/crates/layerx-x402/src/seller.rs:249-254`;
   `interop/crates/layerx-x402/src/facilitator.rs:448-453`).
5. `GatewayCore::settle_with_receipt` verifies the same bytes again and
   stores `TranslationStatus::ReceiptVerified { receipt_digest }`
   (`interop/crates/layerx-interop-gateway/src/gateway.rs:412-463`;
   `interop/crates/layerx-x402/src/seller.rs:256-267`;
   `interop/crates/layerx-x402/src/facilitator.rs:455-466`). Any other
   status is `EvidenceMissing`.
6. The settlement `transaction` is `lxp:` plus hex of that digest
   (`interop/crates/layerx-x402/src/seller.rs:269-283`).

Buyer `capture_settlement` is a second, local verifier. It decodes
`PAYMENT-RESPONSE`, requires `success`, parses `extensions.layerx`,
requires `verificationLevel == "sequencer-signed"`, base64-decodes the
receipt, calls `verify`, checks asset/to/amount/payer/network, recomputes
`leaf_hash` of the canonical bytes, and requires
`receiptDigest` and `transaction == lxp:{digest}`
(`interop/crates/layerx-x402/src/buyer.rs:148-209`).

---

## Conformance vectors

`make interop-test-x402` does not load `platform/sdk/conformance/`.
Vectors are inline in `interop/crates/layerx-x402/tests/vectors.rs`.
`interop/crates/layerx-x402/COMPATIBILITY.md:11` states the local
matrix does not claim upstream
reference-implementation conformance or live-service settlement, and
that neither the pinned upstream vector corpus nor a live service is
embedded here. `interop/crates/layerx-x402/tests/vectors.rs:1-3` states the same file verifies
"interoperability with independent x402 implementations". Those two
sentences disagree. The executable tests parse and validate the inline
JSON; they do not contact an independent implementation.

`all_payment_required_vectors_validate_correctly` /
`all_payment_payload_vectors_validate_correctly` /
`all_settlement_response_vectors_validate_correctly`
(`interop/crates/layerx-x402/tests/vectors.rs:353-440`): `valid: true`
must parse and `validate()` / `validate_wire()` `Ok`; `valid: false`
must fail parse or validation.

| Vector | File | What it proves | Expected outcome |
| --- | --- | --- | --- |
| `minimal_valid_payment_required` | `tests/vectors.rs:32-49` | Version 2, URL, one `exact` / `layerx:testnet` offer | parse + `validate` `Ok` |
| `payment_required_with_all_optional_fields` | `tests/vectors.rs:50-75` | Optional error, resource metadata, `extra`, empty `extensions` | parse + `validate` `Ok` |
| `payment_required_with_multiple_accepts` | `tests/vectors.rs:76-103` | Two offers (`exact` and `402lxp`) | parse + `validate` `Ok` |
| `payment_required_wrong_version` | `tests/vectors.rs:109-126` | `x402Version` 1 | parse may succeed; `validate` `WrongVersion` |
| `payment_required_empty_accepts` | `tests/vectors.rs:127-137` | `accepts: []` | `InvalidRequirements` |
| `payment_required_zero_amount` | `tests/vectors.rs:138-155` | amount `"0"` | `InvalidRequirements` |
| `payment_required_negative_amount` | `tests/vectors.rs:156-173` | amount `"-100"` | deserialize/`InvalidAmount` fail |
| `payment_required_empty_url` | `tests/vectors.rs:174-191` | `resource.url` empty | `Bounds` |
| `minimal_valid_payment_payload` | `tests/vectors.rs:197-212` | Version 2, object payload, accepted offer | parse + `validate` `Ok` |
| `payment_payload_with_resource` | `tests/vectors.rs:213-232` | Optional resource on payload | parse + `validate` `Ok` |
| `payment_payload_with_extensions` | `tests/vectors.rs:233-254` | Named extension with `info`/`schema` | parse + `validate` `Ok` |
| `payment_payload_wrong_version` | `tests/vectors.rs:255-270` | `x402Version` 3 | `WrongVersion` |
| `payment_payload_non_object_payload` | `tests/vectors.rs:271-286` | `payload` a string | `InvalidPayload` |
| `successful_settlement` | `tests/vectors.rs:292-309` | `success` true, `lxp:` transaction, `layerx` extension | `validate_wire` `Ok` |
| `pending_settlement` | `tests/vectors.rs:310-319` | `settlement_pending` plus transaction | `validate_wire` `Ok` |
| `refused_settlement` | `tests/vectors.rs:320-329` | `insufficient_balance`, empty transaction | `validate_wire` `Ok` |
| `settlement_success_with_error_reason` | `tests/vectors.rs:330-340` | `success` true with `errorReason` | `EvidenceMissing` |
| `settlement_failed_without_error_reason` | `tests/vectors.rs:341-349` | `success` false, no reason | `InvalidPayload` |

Additional vector tests in the same file:

| Test | File | What it proves | Expected outcome |
| --- | --- | --- | --- |
| `atomic_amount_canonical_encoding_round_trips` | `tests/vectors.rs:443-463` | `0`, `1`, …, `u128::MAX` as JSON strings | deserialize equals original |
| `atomic_amount_refuses_non_canonical_strings` | `tests/vectors.rs:465-488` | empty, negative, fractional, exponent, padded, overflow | `parse` `Err` |
| `atomic_amount_accepts_canonical_strings` | `tests/vectors.rs:490-505` | `"0"` … max `u128` | `parse` `Ok` matching value |
| `payment_required_http_transport_encoding_is_base64_json` | `tests/vectors.rs:507-546` | HTTP encode | header name `PAYMENT-REQUIRED`; base64 JSON version 2 |
| `payment_required_mcp_transport_encoding_is_json` | `tests/vectors.rs:548-583` | MCP encode | `TransportValue::Json` equals input |
| `resource_info_validates_url_format` | `tests/vectors.rs:585-616` | `https://` ok; no scheme and newline refused | `Ok` / `Err` / `Err` |
| `payment_requirements_validates_layerx_network_format` | `tests/vectors.rs:618-654` | `layerx:testnet` facts `Ok`; `ethereum:mainnet` validates but `layerx_facts` `Err`; missing `:` invalid | as stated |
| `wire_encoding_round_trip_preserves_all_fields` | `tests/vectors.rs:656-698` | HTTP, MCP, A2A encode/decode | decoded equals original |

Pinned spec (`interop/crates/layerx-x402/tests/pinned_spec.rs:16-43`):
vendored `x402-specification-v2.md` matches `X402_SPEC_SHA256`; an
appended newline is `AdapterError::DocumentDigestMismatch`.

Transport-matrix settlement tests
(`interop/crates/layerx-x402/tests/transports.rs`) drive a
sequencer-signed receipt whose asset `05..`, recipient `07..`, amount
`25` match `requirements()` (`interop/crates/layerx-x402/tests/transports.rs:37-47, 196-204,
268-282`):

| Test | File | What it proves | Expected outcome |
| --- | --- | --- | --- |
| `every_role_round_trips_on_every_transport` | `tests/transports.rs:84-165` | Encode/decode parity on HTTP, MCP, A2A | all `Ok` and equal; matrix all true |
| `settlement_identity_is_transport_independent_and_step_separated` | `tests/transports.rs:167-194` | Identity ignores transport; `Single` / `EscrowDeposit` / `EscrowCharge` keys differ | equal across transports; step keys unequal |
| `confirmed_settlement_records_exactly_one_receipt_verified_effect` | `tests/transports.rs:435-468` | One successful `/settle` | `success`; `lxp:` digest; one plane call |
| `duplicate_delivery_of_the_same_settlement_does_not_double_charge` | `tests/transports.rs:470-516` | Three redeliveries, one identity | same `transaction`; one stored digest |
| `transport_independent_identity_settles_once_across_http_mcp_and_a2a` | `tests/transports.rs:518-562` | Same request over three transports | same `lxp:` digest |
| `settlement_recovers_after_a_crash_without_a_second_economic_effect` | `tests/transports.rs:564-660` | Pending, then `Decode` crash, then confirm, then duplicate | pending then one digest unchanged |
| `a_swapped_receipt_for_a_settled_identity_is_refused` | `tests/transports.rs:662-714` | Alternate activity id after settle | `Err`; original digest unchanged |

`interop/crates/layerx-x402/COMPATIBILITY.md:17-27` lists those
facilitator rows as local offline pass results. That file also says
they do not attest third-party facilitator interoperability.

Seller, buyer, and e2e tests
(`interop/crates/layerx-x402/tests/seller.rs`,
`interop/crates/layerx-x402/tests/buyer.rs`,
`interop/crates/layerx-x402/tests/e2e.rs`) exercise 402 issuance,
requirements mismatch, extension echo, pending/refused plane outcomes,
capture refusals (failed settlement, missing `layerx` evidence, wrong
`verificationLevel`, malformed receipt), and HTTP/MCP buyer-seller
flows that stop at `SellerOutcome::Pending` when the test plane does
not return a signed receipt.
`interop/crates/layerx-x402/tests/seller.rs:1-3` and
`interop/crates/layerx-x402/tests/e2e.rs:1-3` claim tests run
"against … independent x402 implementations". The files construct
in-crate types and test planes; they do not load an independent
implementation. That comment disagrees with
`interop/crates/layerx-x402/COMPATIBILITY.md:11`.

---

## `make interop-test-x402`

```
interop-test-x402:
	$(INTEROP_CARGO) test --manifest-path interop/Cargo.toml --locked -p layerx-x402
```

(`Makefile:2879-2880`; `INTEROP_CARGO` defaults to `cargo` at
`Makefile:2860-2861`). That command builds the `layerx-x402` package
and runs every test in `src/` and `tests/` (`e2e.rs`, `pinned_spec.rs`,
`seller.rs`, `vectors.rs`, `buyer.rs`, `transports.rs`). It does not
run `interop-test`, other adapter packages, clippy, or
`platform/sdk/conformance`.

`.github/workflows/interop-x402.yml:54-55` runs the same target with
`INTEROP_CARGO="cargo +1.91.1"`.
`interop/crates/layerx-x402/COMPATIBILITY.md:15` cites that workflow
and `make interop-test-x402` as the re-run path for the facilitator
table.

---

## Sources

- `interop/crates/layerx-x402/src/lib.rs:21-68`
- `interop/crates/layerx-x402/src/model.rs:1-416`
- `interop/crates/layerx-x402/src/codec.rs:8-28`
- `interop/crates/layerx-x402/src/transport.rs:1-311`
- `interop/crates/layerx-x402/src/seller.rs:1-328`
- `interop/crates/layerx-x402/src/buyer.rs:1-221`
- `interop/crates/layerx-x402/src/facilitator.rs:1-605`
- `interop/crates/layerx-x402/COMPATIBILITY.md:1-28`
- `interop/crates/layerx-x402/tests/vectors.rs:1-698`
- `interop/crates/layerx-x402/tests/transports.rs:84-714`
- `interop/crates/layerx-interop-gateway/src/server.rs:184-224`
- `interop/crates/layerx-interop-gateway/src/gateway.rs:412-463`
- `interop/crates/layerx-interop-service/src/server.rs:414-423, 445-891`
- `interop/crates/layerx-interop-service/src/config.rs:31-32, 482-490, 537-540`
- `agent/crates/layerx-proof/src/receipt.rs:224-310`
- `Makefile:2860-2880`

[Home](Home.md)
