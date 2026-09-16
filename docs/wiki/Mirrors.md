# Ethereum and Solana mirrors

`layerx-mirror` builds batch archives, publishes them to Ethereum or
Solana, and verifies receipts from those mirror sources
(`interop/crates/layerx-mirror/src/lib.rs`). Binaries:
`layerx-mirror-publisher` and `layerx-mirror-verify`.

`spec/layerx-platform/spec.kvx` tasks 25.1 and 25.2 are
**implemented**. The spec `reality` fields record missing production
RPC/signer clients and incomplete non-Rust SDK mirror surfaces. This
page does not claim those clients are finished.

---

## Public types

Modules: `ethereum`, `node`, `rpc`, `runtime`, `signer`, `solana`,
`source`, `store`, plus re-exports from `publisher` and `verify`.

Key types: `Archive`, `ArchiveCommitment`, `Publisher`,
`GenericPublisher`, `PublicationReport`,
`DurablePublicationReport`, `MirrorCursor`, `MirrorState`,
`MirrorVerifier`, `MirrorVerification`, `MirrorVerifyError`,
`MirrorSource`, `MirrorSources`, `EthereumArchiveClient`,
`SolanaArchiveClient`, `LniArchiveSource`, `RemoteChainSigner`,
`PublicationJournal`.

Contracts used by the publisher live under `interop/contracts/`.

---

## Operator binaries

```text
layerx-mirror-publisher
layerx-mirror-verify
```

`layerx-mirror-verify` checks a receipt out of an untrusted mirror
archive: it validates the archive, checks the batch header against
trust the operator configures, proves inclusion, and returns
freshness alongside the result. That path needs no hosted gateway.

SDK wrappers that exist today are Rust-first
(`layerx_sdk::production` mirror helpers). Other language SDKs
expose local receipt verifiers; a complete multi-SDK mirror surface
is the gap recorded in the spec.

[Interop](Interop.md) · [Portable receipt verifier](PortableVerifier.md) · [Home](Home.md)
