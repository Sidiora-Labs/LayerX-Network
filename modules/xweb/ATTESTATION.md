# xweb attestation digest

Web attestors sign one raw digest with secp256k1 ECDSA. The digest is
`keccak256` over the packed 188-byte preimage below: no EIP-191
`"\x19Ethereum Signed Message"` prefix and no EIP-712 domain separator or
struct hashing. Every integer is big-endian and fixed width; the domain is the
ASCII string with no length prefix and no terminator. This is Solidity
`abi.encodePacked` of the listed types. `types.Preimage` / `types.Digest` build
these bytes, `types/attestation_test.go` recomputes the vectors below, and
`types/testdata/preimage-vectors.json` carries the same vectors for the
x-websearch sidecar and the kernel to assert.

The same layout serves both origins. Origin 1 is a request made by a contract
through the xweb precompile (`0x0000000000000000000000000000000000001019`) and
fulfilled by `modules/xweb`; origin 2 is a request made by a kernel program and
committed by the kernel web observation activity.

## Preimage

Domain `PAXEERX_WEB_V1`. Preimage length 188 bytes.

| Offset | Size | Field          | Encoding                                                                 |
|-------:|-----:|----------------|--------------------------------------------------------------------------|
|      0 |   14 | domain         | ASCII `PAXEERX_WEB_V1`                                                   |
|     14 |    1 | origin         | uint8, 1 for the EVM precompile, 2 for a kernel program                  |
|     15 |   32 | networkId      | uint256, the EVM chain id for origin 1, the kernel network id for origin 2 |
|     47 |   32 | requester      | bytes32, the EVM address left-padded with zeros for origin 1, the 32-byte program id for origin 2 |
|     79 |    8 | requestId      | uint64, the request id the module or the kernel assigned                 |
|     87 |    1 | kind           | uint8, 1 fetch, 2 search, 3 api                                          |
|     88 |   32 | payloadHash    | bytes32, keccak256 of the request payload bytes                          |
|    120 |   32 | contentDigest  | bytes32, keccak256 of the canonical content bytes                        |
|    152 |   32 | responseHash   | bytes32, keccak256 of the stored response bytes (at most 4096)           |
|    184 |    4 | fullLength     | uint32, length of the full text the stored response is taken from       |

`digest = keccak256(preimage)`.

## Signature rules

Enforced by `modules/xweb` fulfil in the shape of the bridge's verification,
and by the kernel web intake:

- each signature is 65 bytes `r (32) || s (32) || v (1)` with `v` in {27, 28}
  and `s <= secp256k1n / 2`;
- signatures are ordered by strictly ascending recovered signer address, which
  also rejects a repeated signer;
- every recovered signer must be a registered web attestor and at least
  `threshold` signatures must be supplied; the threshold is always a strict
  majority of the registered set;
- the stored response is at most 4096 bytes and `fullLength` is never shorter
  than it;
- a request is fulfilled at most once, and a refunded request is never
  fulfilled.

An api request (kind 3) names its attestation level inside its payload, so the
level is bound through `payloadHash` and the preimage keeps its 188 bytes.
Under the single level a fulfilment carries exactly one signature, which must
recover to the attestor the payload names; the threshold rule above applies to
the majority level. `types/api.go` specifies the payload and
`types/testdata/api-vectors.json` pins it.

## Vectors

Vector `evm-fetch` (origin 1):

- networkId `713714`
- requester `0x00000000000000000000000000000000000000000000000000000000000a11ce`
- requestId `7`, kind `1`
- payload `https://paxeer.app/`, payloadHash
  `0x62da65e513a2dc07e8c56bb6e148a96d6d259e496ec215d58c89522fa9649ce1`
- contentDigest `0x2222222222222222222222222222222222222222222222222222222222222222`
- response `Paxeer X Network`, responseHash
  `0xe75d956cdf14f4a1e94d2ca208572632a4b4a0cc5323a2de37e86f08c9f0acfd`
- fullLength `16`
- digest `0x21e2a70f243f13f7e2b72d14cc842c5f6b1779b532144b01c1d61d47b886a0be`
- signer `0x767eb87feba9e1851bd2f5d6fc931d2476b1db90`, signature
  `0x1e95bb99402e2ae4b06e98eb3ce29a00973ce759afeac27c3d0253ebba3211a7555a8954e3d0d46bce57952e7648f08acbb3f84f2a1741056974a3e96c7961611b`

Vector `program-search` (origin 2):

- networkId `1`
- requester `0x5555555555555555555555555555555555555555555555555555555555555555`
- requestId `42`, kind `2`
- payload `paxeer x network`, payloadHash
  `0x637c790bf5fe77888f9696cd67ea9c582f9be57c0b16c9ee11d1cf62ee25c2f8`
- contentDigest `0x3333333333333333333333333333333333333333333333333333333333333333`
- response `[{"url":"https://paxeer.app/","title":"Paxeer","snippet":"Paxeer X Network"}]`,
  responseHash `0x09b4dc1f0fb623c5ffb240fef88176c4706aad87d7a63e622777c029d552fb3a`
- fullLength `5000`
- digest `0xc634942bc32e91ff577e48527c40c951d7f00b0808764b080540b57292343379`
- signer `0x767eb87feba9e1851bd2f5d6fc931d2476b1db90`, signature
  `0x7893009709dc168f1378cc7296bca594594b522e937a17b1948efb399726e78c4532a5ce1715617c5ce302722588935b996f8b8d115093c7475932a081717aa91b`

Vector `evm-api-single` (origin 1):

- networkId `713714`
- requester `0x00000000000000000000000000000000000000000000000000000000000a11ce`
- requestId `9`, kind `3`
- payload: the `post-single-credential` api vector, payloadHash
  `0xd6c5bc752f9769fa30ebf29d2e9b8fc6ae47892c6bdffb659612f490d89b7ca5`
- contentDigest `0x4444444444444444444444444444444444444444444444444444444444444444`
- response `["3.114"]`, responseHash
  `0x2c85cb58c805de4b2edb12efa1506788e69c5d32749f03c9e9d402b09013e9ec`
- fullLength `9`
- digest `0x18ef556323e81c5d30bff202abbf20910cc82809cf280e08279e658441ac865e`
- signer `0xe35182c595c7db5baf6237c02cf6f8177831f384`, signature
  `0x9144452b00ca7e3d2ff7909a87bc504d45409126175bee22aacf06eb64e8470e0ad0adf0f3c1b1c2ebb9887ae5fb5b293bdb0cbea5462f9bc54500064f7bbd9f1c`

The full preimage bytes of each vector are in `types/testdata/preimage-vectors.json`.
