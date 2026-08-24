# Mirror remote signer protocol

The publisher sends one unsigned big-endian length followed by this bounded request:

`LXCS | version:u16=1 | algorithm:u8 | handle_len:u16 | handle | domain_len:u16 | domain | digest:bytes32 | message_len:u32 | message`

Algorithm `1` is recoverable low-S secp256k1 over `digest`; `message_len` must be zero. Algorithm `2` is Ed25519 over the exact non-empty `message`; `digest` is SHA-256 of that message. Handles and domains are UTF-8 policy identifiers, not key material. The signer must reject an algorithm, domain, digest/message mismatch, unauthorized client certificate, unknown handle, or policy violation.

The response is one unsigned big-endian length and either `01` (refused) or `00 | signature`. Signatures are 65-byte `r | s | recovery_id` for algorithm 1 and 64-byte Ed25519 for algorithm 2. The publisher verifies the result against its independently configured public key before persisting or broadcasting it.

UDS deployments rely on filesystem peer isolation. Remote deployments require server-authenticated TLS plus a client certificate. Chain private keys never enter publisher configuration, files, logs, status, or memory.
