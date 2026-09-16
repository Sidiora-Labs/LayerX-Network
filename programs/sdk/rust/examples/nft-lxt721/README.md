# LXT-721 collection reference

This ABI-v2 reference records non-fungible ownership entirely inside the
program's own shared namespace. A token identifier is a nonzero big-endian
u128, its owner is a DID id32, and ownership changes only by rewriting
`owner:<token>` in that namespace. The program moves no 402LXP value, holds no
authority beyond its own storage and never calls another program.

The example configuration lives in `programs/sdk/rust/src/lxt721.rs`: the issuer
is `did:lxp:nft-issuer`, the collection is capped at `REFERENCE_MAX_SUPPLY`
tokens, `REFERENCE_BASE_URI` prefixes every token uri and `REFERENCE_METADATA`
carries the collection name and symbol. Configure these source constants for
another deployment and regenerate its interface.

Only `REFERENCE_ISSUER` mints, only into an unowned identifier, and only while
the minted count stays inside the maximum supply. `transfer` admits the recorded
owner, the token's single approved principal, or an operator the owner approved
for all, and clears the single approval as the owner changes. There is no burn,
no royalty hook, no enumeration and no transfer callback.

The nine LXT-721 methods use `lxt721::Request` canonical calldata: the
discriminator `'L' 'X' 0xd1 <ordinal>` followed by LayerX bounded bytes
`[1, 0x20] || length:u32 || payload`. Responses use the same bounded-bytes
convention: `owner_of` carries the owner id32, `balance_of` and `total_supply`
carry a 16-byte big-endian u128, `token_uri` carries the base uri followed by
32 lowercase hexadecimal digits, `metadata` carries `REFERENCE_METADATA`, and
every mutation returns an empty payload. The guest refuses nested calls.

Build:

```
sh programs/sdk/rust/examples/nft-lxt721/build.sh
```

`layerx-programs-registry::lxt721::reference_interface` binds the real module to
all nine exports and declares nothing but the storage capabilities each export
actually reaches. The registry example `lxt721_interface` writes the canonical
interface and the registry state value for a supplied program id and prints its
digest. The committed fixtures use program id `55` repeated 32 times.
