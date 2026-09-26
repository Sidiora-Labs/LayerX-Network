# Solana identity in the Paxeer X Network bridge attestations

The bridge has one attestation contract and every destination speaks it. An
attestor signs `keccak256` of a packed preimage with secp256k1 and hands over 65
bytes of `r || s || v`; the EVM vault recovers it with `ecrecover`, the
layerxBridge precompile recovers it in the keeper, and the Solana custody program
has it recovered for it by the native secp256k1 program. The preimages, the
digest and the signature rules are specified byte for byte in
`modules/layerxbridge/ATTESTATION.md` and built by
`modules/layerxbridge/types.InboundPreimage` and `OutboundPreimage`. Those are
the fixed inputs of this document. The Paxeer-side copy under `modules/` is read
by this feature and edited by nothing in it.

This document adds **no** second domain, no second hash and no second signature
format for Solana. The domains stay `PAXEERX_BRIDGE_IN_V1` (196-byte preimage)
and `PAXEERX_BRIDGE_OUT_V1` (185-byte preimage), the digest stays plain
`keccak256` of the preimage with no EIP-191 prefix and no EIP-712 domain, and an
attestation stays a secp256k1 signature with `v` in `{27, 28}`, `s` at or below
half the curve order, ordered by strictly ascending recovered signer, every
signer a current attestor and at least `threshold` of them. The ed25519 keys on
Solana pay transaction fees and nothing else; the program never accepts an
ed25519 signature as an attestation.

What Solana needs, and all this document adds, is a mapping: a Solana key is 32
bytes and the digests carry 20.

## The mapping

| Digest field | On Solana |
|--------------|-----------|
| chain id     | the reserved `uint64` `91600046870081` |
| vault        | the handle of the custody program's vault-authority PDA |
| asset        | the 20-byte asset id the program's asset registry holds for the mint, whose default is the handle of the mint |
| txHash (inbound) | `keccak256` of the 64-byte Solana transaction signature that carried the deposit |
| logIndex (inbound) | the deposit nonce recorded in the deposit-receipt PDA |
| recipient (inbound) | the 32-byte Paxeer recipient of the deposit, an EVM address in its low 20 bytes with its high 12 bytes zero |
| recipient (outbound) | the handle of the full 32-byte Solana pubkey the release instruction names |

### The handle of a 32-byte key

    handle(key) = keccak256(key)[12:32]

The last 20 bytes of `keccak256` of the key's 32 raw bytes: the same truncation
an EVM address is to a public key. `bridge/vectors.Handle` is the only Go
implementation. The custody program is to carry the only Rust one, in its
`identity` module, so that no second derivation of a handle exists anywhere.

### The vault

The vault Paxeer registers for Solana is `handle(vault-authority PDA)`, where the
vault-authority PDA is the program address of the single seed `vault-authority`
under the custody program's id:

    vault_authority, bump = find_program_address([b"vault-authority"], program_id)
    vault                 = handle(vault_authority)

That PDA holds every bridged token on Solana and signs their transfers, so the
identity that custodies the funds is the identity the attestors sign for. A
deployment's program id, its vault-authority PDA and that PDA's handle are
written down by `bridge/deploy/deploy-solana-program.sh`, and governance
registers the handle through `MsgRegisterChain` like any other chain's vault.

### The asset

An asset's 20 bytes come from the program's owner-set asset registry, one PDA per
mint. Registering a mint without an explicit id defaults it to `handle(mint)`;
the owner may register an explicit 20-byte id instead. An unregistered or
disabled mint is refused, so no asset reaches a digest without the owner having
recorded it.

Sidiora is the one asset with an explicit id, because the chain fixed the pair
first. `EnsureSidioraDenom` in `modules/layerxbridge/keeper/sidiora.go` registers
the bridged asset (chain id, `0x21f7b20a555199fa73A238B1a91FD0f549068fEe`, the
`usid` denom), and governance calls it with the Solana chain id. Registering
Sidiora's mint with that id is therefore what makes an inbound SID deposit
resolve to the denom the bridge module already administers, with no alias and no
chain change. Solana is Sidiora's foreign home: it exists on exactly two chains,
Paxeer X and Solana.

Native SOL bridges as the wrapped SOL mint, registered with its derived handle,
so one code path serves every asset and the default pair PAX against SOL works
the moment the chain is opened.

### The inbound transaction hash and log index

Solana has no 32-byte transaction hash, and the custody program never learns the
signature of the transaction it runs in. The relayer supplies both halves of the
nullifier instead:

    txHash   = keccak256(transaction_signature)   // the 64 raw bytes, not the base58 text
    logIndex = deposit nonce from the deposit-receipt PDA

The deposit increments the config's nonce and writes a receipt PDA seeded by it,
so the nonce is monotonic, auditable and unique per deposit. `(chain id, txHash,
logIndex)` is the nullifier the precompile consumes once.

### The outbound recipient

A release instruction names the recipient's **full 32-byte pubkey** and the
program derives `handle(pubkey)` for the digest it verifies. The attestors sign
20 bytes; the program can only pay the key whose handle is those 20 bytes. A
release to any other account rebuilds a different preimage and fails
verification.

### The chain id

Solana's chain id on the Paxeer side is the reserved `uint64`:

    "SOLANA" -> 0x53 0x4f 0x4c 0x41 0x4e 0x41
    left-padded to eight bytes -> 0x0000534f4c414e41
    big-endian                 -> 91600046870081

It cannot collide with an EIP-155 id. EIP-155 ids in use are small integers - the
bridge's own configurations use 1, 10, 56, 137, 999, 8453, 42161 and 43114 - and
the reserved value is above 9.1e13, nine orders of magnitude clear of the largest
of them; nothing assigns EIP-155 ids in that range, and the padded-ASCII scheme
keeps any future non-EVM chain in the same unreachable band. Governance registers
it through `MsgRegisterChain` exactly like an EVM chain.

## Worked vectors

`bridge/vectors/solana.go` holds these values and `bridge/vectors/solana_test.go`
recomputes every digest below through `modules/layerxbridge/types.InboundPreimage`
and `OutboundPreimage`, the chain's own builders, so this document cannot claim a
digest the chain would not produce. That Go check is the only one in place
today. The custody program's tests and the relayer's Solana tests are to assert
these same values as they are written, and from then on changing one of the
three implementations fails the other two; until then this document and
`bridge/vectors` are the record they must be written against.

Every example input is derived from a documented ASCII label with `keccak256`,
so each of the three implementations can recompute it instead of copying an
unexplained value. The labels stand in for what a deployment produces - a program id, a
transaction signature, an account - and none of them is a deployment value:

| Label | Derives |
|-------|---------|
| `PAXEERX_BRIDGE_SOLANA_VECTOR_PROGRAM` | the program id these vectors are computed against |
| `PAXEERX_BRIDGE_SOLANA_VECTOR_DEPOSIT_SIGNATURE` | the 64-byte deposit transaction signature, as `keccak256(label \|\| 0x01) \|\| keccak256(label \|\| 0x02)` |
| `PAXEERX_BRIDGE_SOLANA_VECTOR_PAXEER_RECIPIENT` | the Paxeer EVM address credited by the inbound deposit, as `keccak256(label)[12:32]` |
| `PAXEERX_BRIDGE_SOLANA_VECTOR_RECIPIENT` | the 32-byte Solana pubkey the outbound release pays |
| `PAXEERX_BRIDGE_SOLANA_VECTOR_BURN` | the Paxeer transaction hash of the burn the outbound release answers |

### Identities

| Identity | Value | 20 bytes in the digest |
|----------|-------|------------------------|
| Sidiora mint, 6 decimals | `5w3wVdJaESaJKyLmStM6Hv9UyUkmZ1b9DLQquAqqpump` | `0x21f7b20a555199fa73A238B1a91FD0f549068fEe`, the id the chain fixed, **not** the mint's handle `0x9232467d43fd9edd0bf250fe24ca0c9380b3ca86` |
| wrapped SOL mint, 9 decimals | `So11111111111111111111111111111111111111112` | `0xcf996523b5d068a26f0aa8a116602fe5033ee3a1`, its derived handle |
| vector program id | `A7SZbByPYuHpunZ9pyMDMrhMYvK44ANT1AVqb8U1FpM9` | not in a digest |
| vault-authority PDA, bump 255 | `GxxA9Cs9v5pAGVsaCe2jjDrtmieeBijcY4S5HHTY8Vq6` | `0x334121a65b47bd45c3f6381537d9180e98e445bc`, the registered vault |
| outbound recipient pubkey | `59TLtNdRpCZysEHkGDMPFHkHiHXkBqQVqAVxAupzNoQb` | `0xfb02125a3275d53a9f6538626b49894d2aae80cc` |

### Inbound: a Sidiora deposit on Solana, 12.345678 SID

Domain `PAXEERX_BRIDGE_IN_V1`, preimage 196 bytes.

| Offset | Size | Field     | Value |
|-------:|-----:|-----------|-------|
|      0 |   20 | domain    | ASCII `PAXEERX_BRIDGE_IN_V1` |
|     20 |   32 | chainId   | `91600046870081` |
|     52 |   20 | vault     | `0x334121a65b47bd45c3f6381537d9180e98e445bc` |
|     72 |   32 | txHash    | `0x4219bc1e7d357618e0c662c494981e70d02920d8f7c3f850dd25712ef87e48d3` |
|    104 |    8 | logIndex  | `7`, the deposit nonce in the receipt PDA |
|    112 |   32 | recipient | `0x000000000000000000000000b65aa00b0baa8fe2abc2d188312fb0a6d00e3ed5` |
|    144 |   20 | asset     | `0x21f7b20a555199fa73A238B1a91FD0f549068fEe` |
|    164 |   32 | amount    | `12345678` base units |

The transaction signature the txHash hashes is
`0xbe5dcb8ac1c1153020e5002e376da27e2c817c11725d6b37ea46deadb92b55efedb2c44ad7ec9132c7685ed84b08fcefb65b76a9912b1f0cc8f82eeb379b96e2`,
base58 `4okVsRAC31AiwNv6HUY1upyAF5hqTn6VownC1j7CKXcau7KovRyFXnnEkpUibUm1E2LQU5zxKwrGp8XVCZEBegsB`.

    preimage = 504158454552585f4252494447455f494e5f5631
               0000000000000000000000000000000000000000000000000000534f4c414e41
               334121a65b47bd45c3f6381537d9180e98e445bc
               4219bc1e7d357618e0c662c494981e70d02920d8f7c3f850dd25712ef87e48d3
               0000000000000007
               000000000000000000000000b65aa00b0baa8fe2abc2d188312fb0a6d00e3ed5
               21f7b20a555199fa73a238b1a91fd0f549068fee
               0000000000000000000000000000000000000000000000000000000000bc614e

    digest   = 0x3333b122e2a4e61ad324d5c875a8a9c72cf98a2724da26c211235e30a8d294ef

### Outbound: a release of 4.2 SID to a Solana account

Domain `PAXEERX_BRIDGE_OUT_V1`, preimage 185 bytes.

| Offset | Size | Field        | Value |
|-------:|-----:|--------------|-------|
|      0 |   21 | domain       | ASCII `PAXEERX_BRIDGE_OUT_V1` |
|     21 |   32 | chainId      | `91600046870081` |
|     53 |   20 | vault        | `0x334121a65b47bd45c3f6381537d9180e98e445bc` |
|     73 |   32 | paxeerTxHash | `0x6f79d9a61a77030bbeba5f435907f53b09321779310326b9faaebb391b3b5d5f` |
|    105 |    8 | paxeerNonce  | `11` |
|    113 |   20 | recipient    | `0xfb02125a3275d53a9f6538626b49894d2aae80cc`, the handle of `59TLtNdRpCZysEHkGDMPFHkHiHXkBqQVqAVxAupzNoQb` |
|    133 |   20 | asset        | `0x21f7b20a555199fa73A238B1a91FD0f549068fEe` |
|    153 |   32 | amount       | `4200000` base units |

    preimage = 504158454552585f4252494447455f4f55545f5631
               0000000000000000000000000000000000000000000000000000534f4c414e41
               334121a65b47bd45c3f6381537d9180e98e445bc
               6f79d9a61a77030bbeba5f435907f53b09321779310326b9faaebb391b3b5d5f
               000000000000000b
               fb02125a3275d53a9f6538626b49894d2aae80cc
               21f7b20a555199fa73a238b1a91fd0f549068fee
               0000000000000000000000000000000000000000000000000000000000401640

    digest   = 0xc583652dd9b59e0fcef102cfc8866a52beadb77b82de25445d86900baf6d1c4e

The release consumes the nullifier `keccak256(paxeerTxHash || paxeerNonce)` =
`0xd653f4968eb9b70e1eaef15fb134f2f7da8c15c38335af0069c94585e521ea3a`, which the
custody program records as a PDA: a replayed release fails because the account
already exists.
