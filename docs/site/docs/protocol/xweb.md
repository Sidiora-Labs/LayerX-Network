# Web search and xweb

Web data for agents, contracts and programs on Paxeer X Network. One sidecar on every node, one content digest, and three ways to reach it.

Every node of Paxeer X Network can run a sidecar, `x-websearch`, that crawls its operator's seed list into its own index and serves web search and page fetch. No third-party search vendor and no API key sit anywhere in the path. Agents pay the sidecar per request over 402LXP; contracts reach the same data through the xweb precompile; kernel programs read it through the `web_read` host import. This page is the read of how each of those works. [Unified network](../overview/unified-network.md) describes how the chain and the LayerX kernel domain sit behind one interface.

---

## What it is

| Field | Value |
| --- | --- |
| Sidecar | `x-websearch`, one binary and one JSON configuration file |
| Index | the sidecar's own crawler and its own tantivy index |
| Accepted assets | `SID`, `PAX`, `USDC` and `USDL`, each by its registered kernel asset id |
| Price | an operator-set amount per asset per request, one tenth of a US cent at the time the operator configures it |
| Precompile | `0x0000000000000000000000000000000000001019` |
| Module | `x/xweb`, holding the requests, the results, the attestor set and the parameters |
| Program import | `web_read(i32,i32,i32,i32)->i32` in the `layerx_v4` group |
| Stored response | at most 4096 bytes, with the 32-byte content digest and the full length |

### The sidecar

`x-websearch` is one binary started as `x-websearch --config PATH`. The configuration names the listen address, the data directory for the index and the content store, the seed list and the crawl budget, the fetch limits, the four accepted assets with their asset ids and prices, the gateway endpoint and the sequencer trust inputs for settlement, the EVM endpoint and chain id an attestor watches, the kernel network id, and the peer sidecars it exchanges content with.

The loader refuses rather than guesses. An unknown field, a missing field, a placeholder value and anything that looks like key material are each a refusal naming the field. Keys never appear in the configuration: the 402 receiver key, the attestor key and the submitter key are read only from the files that `X_WEBSEARCH_RECEIVER_KEY_FILE`, `X_WEBSEARCH_ATTESTOR_KEY_FILE` and `X_WEBSEARCH_SUBMITTER_KEY_FILE` name. The receiver key is required; the other two are held only by a sidecar that attests or submits.

The HTTP server uses the standard library only, with a bounded worker pool, request-line and header limits, read and write timeouts and one response per connection. It answers `GET` and nothing else.

### How it reads the web

- **The crawler walks the seed list inside its budget.** The budget is pages per cycle, pages per host, a maximum depth and a per-host politeness delay. A cycle starts when the sidecar starts and again every fifteen minutes, and every page it keeps goes into the index with its URL, title and body.
- **robots.txt is honoured.** Every crawl and every fetch checks robots.txt for the user-agent token `x-websearch`, and a disallowed URL is refused.
- **A fetch is bounded.** Only `http` and `https` are accepted. Loopback, private, link-local and multicast destinations are refused after name resolution unless the configuration's `allow_loopback` flag is set for a local test. The connect limit is 3 seconds, the total limit 10 seconds, the body limit 2 MiB and the redirect limit 3, each redirect under the same checks.
- **Extraction is deterministic.** `text/html` drops `script`, `style`, `noscript` and `template` elements, turns block elements into line breaks and decodes entities; `text/plain` and `application/json` pass through; every other media type is refused.

---

## Paying for a request

Every paid route answers an unpaid request with `402` and a `PAYMENT-REQUIRED` header. The offer lists each of the four assets at the operator's price: an `exact` payment always, and a `metered` draw against the payer's grant when the request names its payer in the `LAYERX-PAYER-DID` header. The flow is the 402LXP wire contract in [`spec/402lxp/protocol.md`](https://github.com/Sidiora-Labs/Paxeer-X-Network/blob/main/spec/402lxp/protocol.md):

1. **The buyer pays.** It repeats the request with its payment in `PAYMENT-SIGNATURE`.
2. **The sidecar settles through the gateway.** The receiver key signs the draw, the gateway executes it with `lx_sendActivity`, and a pending result is recovered with `lx_getActivityStatus` or `lx_getReceipt`, never replaced.
3. **The receipt is verified before anything is released.** The receipt's sequencer signature is checked against the trust inputs in the configuration, never against anything in the response, and it must name the draw, the payer, the asset and the amount.
4. **The resource comes back with `PAYMENT-RESPONSE`.** Its settlement reference is `lxp:<receipt_digest>`.

A pending settlement answers `503` with `Retry-After`, and releases nothing. A retry of the same request reuses the same signed activity and idempotency key. A receipt releases one request only; presenting it again is refused.

PAX is paid into the receiver's main account and drawn from the payer's; SID, USDC and USDL move between the per-asset accounts.

### The price rule

An operator sets each asset's price per request, in that asset's base units, so that it equals one tenth of a US cent at the time of configuration. The rule is fixed; the number is the operator's. The committed configuration example carries placeholders the loader refuses, and a zero price is refused too, so no sidecar starts with a price nobody chose. The loader also refuses a missing asset, an extra asset, a zero asset id, a placeholder asset id and two assets sharing one id.

---

## The routes

| Route | Payment | What it returns |
| --- | --- | --- |
| `GET /health` | none | `{"status":"ok"}` |
| `GET /search?q=` | 402LXP | the query, the media type, the content digest and at most ten results, each with `url`, `title` and `snippet` |
| `GET /fetch?url=` | 402LXP | the URL, the final URL after redirects, the media type, the content digest, the text length and the extracted text |
| `GET /content/<digest>` | none | the canonical bytes stored under that digest |

A search queries the index and returns at most ten results ordered by score descending and then by URL ascending. A search and a fetch both write their canonical bytes to the content store under their digest before they answer, so the same bytes can be asked for again later.

### Content by digest

`GET /content/<digest>` takes the digest as 64 hexadecimal characters and returns exactly the canonical bytes, as `application/octet-stream`.

- A sidecar that holds the digest serves it from its own store. A stored file whose bytes no longer hash to its name is removed and reads as absent.
- A sidecar that does not hold it asks its configured peer sidecars in turn, recomputes the digest of what each one answers, keeps the first answer whose digest matches, and serves it.
- The answer is `404` only when no peer holds bytes with that digest. A request coming from a peer is answered from the local store only and is never forwarded further.

The full content of any attested answer is therefore retrievable from any sidecar that can reach one that holds it, whatever the 4096-byte bound left on chain.

---

## The canonical content digest

Two sidecars that saw the same page produce the same 32 bytes. No clock, header, host name or node identity enters them.

The extracted text is canonicalised first: decoded to UTF-8 from the declared charset with invalid input refused, CRLF and CR turned into LF, control characters other than LF and TAB removed, runs of spaces and tabs collapsed to one space, trailing spaces removed from each line, three or more consecutive LF collapsed to two, leading and trailing whitespace trimmed, and the result put in Unicode NFC. For a search, the text is the compact JSON array of the results with the keys `url`, `title` and `snippet` in that order, and no score in the bytes.

The canonical bytes are then:

| Part | Encoding |
| --- | --- |
| domain | the ASCII bytes `PAXEERX_WEB_CONTENT_V1` |
| kind | one byte: `1` fetch, `2` search |
| payload | a big-endian `uint32` length, then the request payload: the URL or the query |
| media type | a big-endian `uint32` length, then the media type in lower case without parameters |
| text | a big-endian `uint64` length, then the canonical text |

The content digest is `keccak256` of those bytes.

---

## The contract path

Contracts call the xweb precompile at `0x0000000000000000000000000000000000001019`. The Solidity interface is `contracts/src/precompiles/IXWeb.sol`, and `contracts/src/xweb/XWebConsumer.sol` is a reference consumer.

| Call | What it does |
| --- | --- |
| `request(uint8 kind, bytes payload, uint64 callbackGas)` | payable with exactly `fee()`; stores the request under the next id and returns it |
| `fulfil(uint64 requestId, bytes response, bytes32 contentDigest, uint32 fullLength, bytes[] signatures)` | verifies the attestor signatures, stores the result, pays the signers and delivers the callback |
| `refund(uint64 requestId)` | returns the fee of a request not fulfilled by its timeout height to its requester |
| `getRequest`, `getResult`, `getAttestors`, `threshold`, `fee`, `getParams` | the views a contract or an auditor needs |

### Request

A contract names the kind (`1` fetch, `2` search, `3` api), the payload - the URL, the query or the api payload of [Calling an API](#calling-an-api) - and the gas its callback may use, and sends exactly the current fee in PAX. The module refuses the request while it is paused, for an unknown kind, for an empty payload or one over the payload cap, and for a callback gas of zero or over the callback cap. Otherwise it takes the fee into the module account, stores the request under the next id with the requester, the kind, the payload hash, the callback gas, the fee, the height and the timeout height, and emits `XWebRequested` with every field an attestor needs.

### Fulfil

`fulfil` rebuilds the 188-byte attestation preimage from the stored request and the submitted response, digest and length, and verifies the signatures over its `keccak256`. It refuses a response longer than 4096 bytes, a full length shorter than the response, a request that is unknown, fulfilled or refunded, and a paused module. On success it stores the response, the digest, the full length and the signers, marks the request fulfilled, and splits the fee equally among the signing attestors' payout accounts, with the integer remainder to the lowest signer.

### The callback

The precompile then calls the requester, from `0x0000000000000000000000000000000000001019`:

```solidity
function onXWebResponse(uint64 requestId, bytes32 contentDigest, uint32 fullLength, bytes calldata response) external;
```

The callback's gas is bounded by the smaller of the request's `callbackGas` and the module's maximum, and `fulfil` refuses unless the gas left covers that full bound. A callback that reverts or runs out of gas is recorded in the result - delivered, reverted or out of gas, with the gas it used - and the fulfilment stands. `XWebFulfilled` carries the same outcome together with the attestation level, `0` majority or `1` single, which `getResult` also returns.

### Refund

A request not fulfilled by its timeout height is refundable exactly once. Anyone may trigger the refund; the fee only ever goes back to the requester. A refunded request is never fulfilled, and refunds stay open while the module is paused, so a pause never holds a fee.

### Gas

Every call costs a base plus 16 gas per byte of calldata after the selector. The base is 3000 for a view and 30000 for `request`, `fulfil` and `refund`; `fulfil` adds 8000 per signature and then the callback's gas used plus 10000 to record it.

---

## The attestors

Attestors are sidecars holding a web-attestor secp256k1 key, separate from any consensus, validator or bridge key. Governance registers each attestor's 20-byte signer address together with its payout account and, for an attestor that accepts credential envelopes, the 33-byte compressed public key of the same key, which must derive the signer address. `getAttestors` returns all three.

**The threshold is always a majority.** Only an api request under the single level is answered by one attestor, as [Calling an API](#calling-an-api) describes. The keeper refuses a threshold at or below half of the registered set or above it. Registering an attestor raises the threshold to the majority of the new set when it would otherwise fall below it; removing one lowers a threshold above the new set size to that size, which is still a majority.

Each attestor watches the request event, fetches independently, and signs one raw digest: `keccak256` of a 188-byte preimage, with no EIP-191 prefix and no EIP-712 domain.

| Field | Size | Encoding |
| --- | --- | --- |
| domain | 14 | the ASCII bytes `PAXEERX_WEB_V1` |
| origin | 1 | `1` for the EVM precompile, `2` for a kernel program |
| network id | 32 | the EVM chain id for origin 1, the kernel network id for origin 2 |
| requester | 32 | the EVM address left-padded with zeros for origin 1, the program id for origin 2 |
| request id | 8 | `uint64` |
| kind | 1 | `1` fetch, `2` search, `3` api |
| payload hash | 32 | `keccak256` of the request payload |
| content digest | 32 | the canonical content digest |
| response hash | 32 | `keccak256` of the stored response bytes |
| full length | 4 | `uint32`, the length of the full text |

Every integer is big-endian. Each signature is 65 bytes, `r` then `s` then `v`, with `v` in 27 or 28 and `s` at or below half the secp256k1 order. Signatures are ordered by strictly ascending recovered signer, which also refuses a repeated signer; every signer must be registered and there must be at least the threshold of them. The attestors exchange signatures, and a submitter that is part of the sidecar posts `fulfil` once the threshold is reached, ordering the signatures by ascending signer and treating an already-fulfilled request as completed. The preimage is specified once, in [`modules/xweb/ATTESTATION.md`](https://github.com/Sidiora-Labs/Paxeer-X-Network/blob/main/modules/xweb/ATTESTATION.md), with shared vectors asserted in Go, Rust and C.

---

## The kernel path

Kernel programs read web data through the `web_read` host import, fed by a web observation activity. Program execution never dials out.

- **A program asks.** It emits a web request record through `event_emit` - the topic `PAXEERX_WEB_REQUEST_V1` followed by the request id, the kind and the payload - and pays the web fee account with `transfer_402` in the same call. The call bridge records the pending request only when both succeed.
- **Attestors answer.** Attestor sidecars watch program web requests through the gateway, fetch independently, sign the origin-2 digest, exchange signatures and post the web observation activity once the threshold is reached.
- **The kernel checks the answer.** Intake rebuilds the preimage from the observation, recovers each signer with `lxp_secp256k1_recover_address`, requires strictly ascending registered signers at the kernel web attestor threshold, and requires a pending request with the same program id, request id, kind and payload hash. The response is bounded at 4096 bytes and a request is answered once.
- **The answer is committed.** The sequencer folds committed observations into a web root in the batch header, the way oracle observations are folded, so a replay reproduces them.
- **The program reads it.** `web_read` takes a request id and an output buffer and returns the committed record: the 32-byte content digest, the full length and the returned length, then the response. A request with no committed answer, or one owned by another program, reads as absent.

`web_read(i32,i32,i32,i32)->i32` is appended to the ABI as the new `layerx_v4` group, beside `oracle_read`; the frozen `layerx_v1` to `layerx_v3` groups do not change. In the Rust programs SDK, `web::read` returns `None` for an absent answer and `Answer::is_truncated` says whether the stored response is a prefix of a longer text. [Programs](../programs/index.md) describes the runtime around it.

---

## Calling an API

A contract can ask for an HTTP API call beyond the network with the request kind `3`, api. The payload names the method (`GET` or `POST`), an `https` URL, public request headers, a request body, the JSON fields to attest and the attestation level, and it may carry credential envelopes. The Solidity library `XWebApi` builds that payload and submits it in one statement:

```solidity
import {IXWebConsumer, XWEB_PRECOMPILE_ADDRESS} from "../../precompiles/IXWeb.sol";
import {XWebApi} from "../XWebApi.sol";

contract ApiConsumer is IXWebConsumer {
    mapping(uint64 => bytes) public prices;

    function askPrice(bytes calldata envelope, address attestor) external payable returns (uint64) {
        return XWebApi.get("https://paxeer.app/api/v1/price?asset=PAX").select("/data/price").withCredential(envelope)
            .single(attestor).submit(200_000);
    }

    function onXWebResponse(uint64 requestId, bytes32, uint32, bytes calldata response) external {
        require(msg.sender == XWEB_PRECOMPILE_ADDRESS, "only xweb");
        prices[requestId] = response;
    }
}
```

`get` and `post` start the call, `header` adds a public header, `select` adds a JSON pointer, `withCredential` adds an envelope and `single` names the one attestor that answers. `submit` pays the current `fee()` from the contract's balance and returns the request id. The example lives at [`contracts/src/xweb/examples/ApiConsumer.sol`](https://github.com/Sidiora-Labs/Paxeer-X-Network/blob/main/contracts/src/xweb/examples/ApiConsumer.sol).

| Part | Bound |
| --- | --- |
| URL | 1 to 2048 bytes of printable ASCII, `https://` with a lower-case host, no fragment |
| public headers | at most 16, names of at most 64 token characters, values of at most 1024 bytes; `Host`, `Content-Length`, `Transfer-Encoding`, `Connection`, `Keep-Alive`, `TE`, `Trailer` and `Upgrade` are refused |
| body | at most 4096 bytes, empty under `GET` |
| JSON pointers | at most 16, each at most 256 bytes |
| envelopes | at most one per registered attestor, never two for one attestor |
| whole payload | the payload cap, `8192` bytes by default |

The module refuses a payload that breaks one of those bounds, names an attestor outside the registered set or carries more envelopes than there are registered attestors.

### The envelope rule

A credential - an API key, a bearer token, any header the API needs - never travels in the clear. It never lives in chain state, in the request event or in a file on any sidecar. It travels as one envelope per attestor, sealed to that attestor's registered public key:

- **The seal.** A fresh ephemeral secp256k1 key per envelope; the shared secret is the x coordinate of the ephemeral key times the attestor's key; HKDF-SHA256 over it, salted with the compressed ephemeral key under the info `PAXEERX_WEB_API_ENVELOPE_V1`, gives the AES-256-GCM key; a fresh 12-byte nonce.
- **The binding.** The associated data is the attestor's address and the origin of the URL - `https://` and its host and port - so an envelope copied into a request for another origin does not open.
- **The layout.** The attestor's 20-byte address in the clear, so each sidecar picks its own, then the 33-byte ephemeral key, the nonce, the ciphertext and the 16-byte tag.
- **The use.** The sidecar opens its envelope in memory for the one call and adds the credential headers to the public ones. A request that carries envelopes but none for a sidecar is refused by that sidecar rather than called without the credential.

Under the majority level every attestor that makes the call needs its own envelope. An attestor replaced through governance stops receiving envelopes as soon as `getAttestors` stops returning it, and the holder of a credential rotates it at the API as with any other client.

### The selector rule

The attested answer is not the whole response. Each JSON pointer (RFC 6901) selects one field of a JSON response, and the selected values are put in RFC 8785 canonical form, so attestors agree on the answer even when an API's full responses differ from call to call. A response that is not JSON when pointers are given, or a pointer that resolves to nothing, is refused and the pointer is named. With no pointer the attested answer is the raw bounded body.

### The two attestation levels

| Level | Who answers | What `fulfil` needs |
| --- | --- | --- |
| majority (`0`) | every attestor holding an envelope, independently | the threshold of signatures over the same answer, exactly as for a fetch |
| single (`1`) | the one attestor `single` names | one signature, from that attestor only |

The single level is for an API that tolerates one call only, or one credential that only one operator holds. The level is stored in the result and carried in `XWebFulfilled`, so a consumer can tell a single-attestor answer from a majority one. An attested answer is public on chain like every other result, whatever credential produced it.

### Building envelopes off chain

`agent/sdk/typescript` and `agent/sdk/python` build the payload and its envelopes from the attestor set `getAttestors` returns, so a developer needs no encoding knowledge:

```typescript
const set = decodeXWebAttestors(await provider.call({ to: XWEB_PRECOMPILE, data: xwebGetAttestorsCallData() }));
const request = buildXWebApiRequest(
  { method: "GET", url: "https://paxeer.app/api/v1/price?asset=PAX", pointers: ["/data/price"] },
  set,
  { credential: [{ name: "X-Api-Key", value: apiKey }] },
);
const call = xwebApiRequestCall(request.payload, 200_000n, fee);
```

In Python the same steps are `decode_xweb_attestors`, `build_xweb_api_request` with `credential=` and `xweb_api_request_call`. Under the majority level the helpers seal one envelope to every attestor that registered a public key and refuse when fewer of them than the threshold did; with `single` they seal only to the named attestor. Every public key is checked against its signer address before anything is sealed, and the ephemeral key and nonce come from the operating system's secure random source. The payload bytes and the envelopes are pinned against the same vectors the module's Go codec is tested with.

---

## Agent clients

`agent/sdk/typescript` and `agent/sdk/python` carry web search clients that perform the full 402LXP exchange against a sidecar for search, fetch and content by digest, verify the `PAYMENT-RESPONSE` settlement before returning content, and recompute the content digest locally. The MCP server exposes `web.search`, `web.fetch` and `web.content`; each spends through the server's approval boundary, and everything they return is untrusted external content. [SDKs](../agents/sdks.md) and [MCP](../agents/mcp.md) describe the clients around them.

---

## The governance surface

The module parameters and the attestor set change only through governance messages carrying the module authority, which is the gov module account.

| Message | What it changes |
| --- | --- |
| `MsgRegisterAttestor` | adds a signer address with its payout account and, optionally, its envelope public key |
| `MsgRemoveAttestor` | removes a signer |
| `MsgSetThreshold` | sets the threshold, within the majority rule |
| `MsgSetParams` | sets the fee, the payload cap, the callback cap and the timeout |
| `MsgPause`, `MsgUnpause` | stops and restarts requests and fulfilments; refunds stay open |

| Parameter | Default |
| --- | --- |
| payload cap | `8192` bytes |
| callback gas cap | `500000` |
| timeout | `3600` blocks |
| attestor set | empty |
| paused | yes |

A pending request keeps the fee and the timeout height it was stored with when the parameters change. The module starts paused with no attestors: governance registers the attestors and unpauses it.

---

## Where it stands

- The sidecar, its payment gate, its index and its content store work without any chain change.
- The xweb module, the precompile at `0x0000000000000000000000000000000000001019`, its registration, the kernel web observation activity and `web_read` are consensus changes. They reach a running chain only through the `v6.8` upgrade plan and a governance proposal that carries it.

Source for everything on this page lives in the [repository](https://github.com/Sidiora-Labs/Paxeer-X-Network): the sidecar under `interop/crates/x-websearch`, the module under `modules/xweb`, the precompile under `precompiles/xweb`, the interface and the reference consumer under `contracts/src`, the kernel side under `include/layerx/lx_web.h`, `src/modules/web`, `src/network` and `src/sequencer`, `web_read` under `programs/crates/layerx-programs-runtime` and `programs/sdk/rust`, and the clients under `agent/`. More about the network is at [paxeer.app](https://paxeer.app).
