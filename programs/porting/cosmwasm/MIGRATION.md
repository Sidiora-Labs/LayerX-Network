# Porting a CosmWasm contract to LayerX programs

## ABI v2 `Env`, `MessageInfo` and crypto

`messages::context::current` maps `info.sender` to the invoking principal,
`env.contract.address` to the executing program and `env.block.height` to the
authenticated batch height. Contracts call `layerx_program_sdk::crypto`
instead of a native host library, retaining typed refusals and bounded inputs.

`reference-v2/src/lib.rs` is the executable message-info and BLAKE3 reference.
`make programs-porting-v2-references` builds, validates and executes it through
the production ABI v2 engine alongside the EVM and Anchor references.

This guide is written for someone who knows `cosmwasm_std` and
`cw-storage-plus` and has never written a LayerX program. It maps the contract
vocabulary you already use onto the version-one programs ABI, and it is
explicit about the constructs that do not carry over, because the kit refuses
those by name rather than emulating them.

The worked example is `src/contract.rs` in the published archive: the donation
contract from the CosmWasm book - an `Item` of configuration, a `Map` keyed by
`info.sender`, funds in and funds straight back out. Its complete port lives in
`src/reference.rs` and is deployed, source-verified and executed by
`src/qualify.rs`.

A CosmWasm contract is already WebAssembly, so the first question is always
whether the existing artifact can simply be relinked. It cannot. It imports the
`wasmd` host, it allocates, and every one of its entry points takes and returns
a JSON document. The port replaces the host, the boundary and the encoding, and
keeps the names.

## What stays byte-identical

| Artifact | CosmWasm | After the port |
| --- | --- | --- |
| `Item` raw key | the namespace bytes verbatim | the same bytes, as the storage key |
| `Map` raw key | `u16` big-endian length, namespace, key | the same framing, `storage::map_key` |
| Message variant name | `{"donate":{ ... }}` | `donate`, unchanged |
| Field names | `times`, `count` | the same names, in the same declaration order |
| Event type | `wasm-donation` | the same bytes, as the program event topic |
| Attribute key | `count` | the same bytes, inside the payload |

What changes is encoding, never naming. There is one exception, and it is
forced: CosmWasm has no on-the-wire selector to preserve, because routing is
done by matching a JSON object key. The kit therefore gives each variant an
eight-byte tag, `sha256("<entry point>:<variant>")[..8]`, derived from the name
you already published. Renaming a variant is a breaking change after the port
exactly as it is before it.

## Storage

### The namespace replaces `info.sender`

A CosmWasm contract owns one flat key-value store that every caller shares, so
per-user state has to be keyed by the address the contract is storing state
*about*. A LayerX program owns a byte-keyed map inside a namespace that is
`(program, principal)`, and that namespace is fixed by the runtime *before your
code runs*. You cannot choose it, widen it, or read another principal's cell by
choosing a key.

The direct consequence: **a key element that carries `info.sender` tells the
port nothing the runtime has not already fixed, and therefore drops out of the
key.**

```rust
pub const CONFIG: Item<Config> = Item::new("config");
pub const DONATIONS: Map<&Addr, DonationRecord> = Map::new("donations");
```

```text
CosmWasm  63 6f 6e 66 69 67                               CONFIG, one global cell
CosmWasm  00 09 64 6f 6e 61 74 69 6f 6e 73 <addr bytes>   one DONATIONS entry
LayerX    00 09 64 6f 6e 61 74 69 6f 6e 73                the same entry, collapsed
```

Every donor's record collapses onto that same eleven-byte key. The cells do not
collide, because each one is written in a different namespace.

### Raw keys keep their shape

`src/storage.rs` reproduces the `cw-storage-plus` composition rules exactly, so
an exported state dump can be located key for key before anything is written:

| Task | Function |
| --- | --- |
| The raw key of an `Item` | `item_key(namespace)` |
| The prefix a `Map` writes before every key | `map_prefix(namespace)` |
| The raw key of one `Map` entry | `map_key(namespace, key)` |
| The raw key of a composite-keyed entry | `composite_map_key(namespace, leading, last)` |
| The key the ported cell occupies | `StateBinding::layerx_key(namespace, leading)` |

The framing is the one `cw-storage-plus` uses: every element except the last is
written as a two-byte big-endian length followed by its bytes, and the last
element is written bare.

### What a piece of state becomes

| CosmWasm state | After the port |
| --- | --- |
| `Item<T>` holding deployment configuration | pinned into the module by the port descriptor |
| `Item<T>` holding per-caller state | one cell per principal, at the namespace bytes |
| `Map<&Addr, T>` keyed by `info.sender` | collapses onto the map's namespace prefix |
| `Map<(&Addr, u64), T>` ending in `info.sender` | the trailing element collapses; the leading ones stay |
| `Map<K, T>` every account must be able to read | a cell in the `(program)` shared namespace, using `layerx_program_sdk::storage::shared` |

`StateBinding::layerx_key` returns exactly that mapping. A name registry, an
order book, or a global leaderboard now has an honest port instead of being
refused.

### Shared state is now portable

A name registry, an order book, a global leaderboard - anything one account
writes and another account reads - maps onto the program-shared namespace
`(program)`. Use `StateBinding::Shared` to classify it, and
`layerx_program_sdk::storage::shared` to read and write it. Every principal
invoking the program sees the same cell.

State that belongs to the deploying configuration is still best pinned into the
module by the port descriptor, which is what makes the artifact reproducible.
State that must be readable and writable by every account now has a port.

### Values: JSON at the edge, framed bytes inside

`cw-storage-plus` stores values as JSON, because a contract has serde and an
allocator. A deterministic module has neither. So JSON stops at the edge: a
client still sends a document, an exported dump still holds one, and everything
in between moves canonically framed bytes.

| JSON type | `cosmwasm_std` | Canonical framing |
| --- | --- | --- |
| number | `u64` | 8 bytes, little-endian |
| decimal string | `Uint128` | 16 bytes, little-endian |
| string | `String`, `Addr` | `u16` little-endian length, then UTF-8 |
| boolean | `bool` | one byte |

`RecordSchema` owns both views of the same declaration: `encode`/`decode` for
the framed bytes the module reads and writes, `encode_json`/`decode_json` for
the document CosmWasm itself would produce or accept, and `transcode` to turn
one straight into the other.

```text
DonationRecord { count: u64 }

  JSON     {"count":3}
  framed   03 00 00 00 00 00 00 00
```

The JSON reader is strict on purpose: unknown fields, repeated fields, missing
fields, a `Uint128` written as a number, and trailing content after the object
are all refused. A migration that silently dropped a field would be a data-loss
bug wearing a success message.

### Migrating existing state

`sender_indexed_import` builds the plan. For each holder it emits a cell naming
the raw key to read out of a state dump and the collapsed key to write, in that
holder's own namespace. `qualify::import_state` performs the writes through the
real storage transaction, transcoding each exported document into the canonical
framing on the way in.

### What has no equivalent

| CosmWasm | Why it cannot carry over |
| --- | --- |
| `Map::range`, `Map::prefix(..).range` | the ABI has `storage_read`, `storage_write` and `storage_delete` and no cursor |
| `IndexedMap` secondary indexes | an index is shared state; see above |
| `Item` used as a global aggregate | an `Item` becomes one cell per principal, not one cell |
| storage read of another contract's namespace | structural: another program cannot reach your namespace at all |
| a contract's own balance | a registered program-derived value account; see **Money** |

## Messages and dispatch

The three entry points do not survive as entry points. `instantiate` is
replaced by deployment: the configuration a CosmWasm contract writes on
instantiation is pinned into the module by the port descriptor, because on
LayerX each deployment is its own program. `execute` and `query` variants each
become an export, and the whole set is also reachable through one dispatcher.

```text
{"donate":{"times":3}}   16 bytes   sha256("execute:donate")[..8]    . u64 le
{"donations":{}}          8 bytes   sha256("query:donations")[..8]
{"remaining":{}}          8 bytes   sha256("query:remaining")[..8]
```

A calling program first calls `layerx_reserve(len)` to obtain the region to
write the message into; `layerx_call` refuses any other pointer and any length
that does not match the selected variant. `MessageVariant::transcode` converts
the JSON a client already builds into that input, so the adapter lives at the
edge and not in the program.

Each variant is also exported under its own name, so an activity that invokes
the program directly names the export instead of encoding a tag.

Query variants lose their address arguments. A CosmWasm query has no sender, so
the contract has to be told whose state to read; a ported query is invoked by a
principal the runtime already authenticated, and reads that principal's
namespace. `{"donations":{"donor":"wasm1..."}}` becomes `{"donations":{}}`.
`reference::chain_query_message` still declares the chain's shape, which is what
an adapter drops the argument from.

Return data does not cross the composition boundary: `layerx_call` returns an
`i32` code, and a negative code is a refusal. The reference port's handlers
return counts bounded by the configured cap, so they fit.

## Responses and events

A `Response` is not a return value after the port; its two halves separate.
Attributes become a program event, and messages become authenticated transfer
requests or narrowed calls.

```text
Response::new().add_attribute("action", "donate")
  topic   wasm

Event::new("donation").add_attribute("count", total)
  topic   wasm-donation
  data    05 63 6f 75 6e 74   then count, little-endian u64
```

The topic is the chain's own event type verbatim - `wasm` for a bare
`Response`, `wasm-<type>` for a custom `Event` - so an indexer that filters on
it keeps matching. Each attribute is framed as its one-byte key length, the key
verbatim, then the value in canonical framing.

Two things a `Response` carries do not port. `set_data` has no equivalent,
because return data does not cross the boundary; put the value in an attribute.
And attributes stringify everything on a chain, while the payload here is
typed - which is strictly more information, and why `ContractEvent` declares
attribute types.

## Sub-messages

```rust
execute_submessage(callee, &variant, &values)?
```

returns the callee, the message in its ported encoding, and **one** narrowed
`Call` capability. The callee reaches only its own namespace, and it holds only
the authority the caller explicitly narrowed.

`funds` on a `WasmMsg::Execute` does not come along: those coins would be paid
out of the contract's own balance, which no program has. See **Money**.

`ReplyOn` and the `reply` entry point have no port either. The boundary is a
call that returns a code, so a refused call is handled where it is made,
immediately, rather than in a separate handler reached by a reply id.

## Money

This is the part of the port that bends least, so read it before you plan one.

A LayerX program **writes no balance**. A contract balance maps to a real
account derived from the LayerX program and a public seed. Contract-funded
messages become bounded owner-frame 402LXP requests from that account, and the
kernel remains the only balance writer.

| CosmWasm | Port |
| --- | --- |
| `info.funds` forwarded on in the same invocation | `ValueFlow::SentFunds` - carried over |
| `BankMsg::Send` paid from the contract's balance | `translate_with_program_account` and a rederived contract account |
| `BankMsg::Burn` | **refused**, `SupplyMutation`; a conserved transfer cannot burn supply |
| `WasmMsg::Execute { funds, .. }` | `translate_with_program_account` and a rederived contract account |
| `cw20` `TransferFrom` where the owner is the caller | `ValueFlow::AllowanceSpend` - carried over |
| `cw20` `TransferFrom` on a third party's allowance | `ValueFlow::AllowanceSpend` - **refused**, `DelegatedSpend` |
| `IbcMsg::Transfer` | `ValueFlow::IbcTransfer` - **refused**, `ChainQuery` |

`ValueFlow::portable()` answers the same question without building the leg.

Denominations do not carry over either: a program is paid in an authenticated
402LXP asset, and the port descriptor names which asset stands in for the
contract's denom. The price is otherwise unchanged - the reference port charges
`price * times` for `times` donations, exactly as the contract charges
`minimal_donation * times`.

The funds check inverts, and this is the single most important sentence in the
guide. On a chain the caller attaches coins and the handler *checks* that
`info.funds` equals what is due. After the port the program *requests* the exact
amount due, and the activity's capability caps what it may request. Nobody can
underpay, nobody can overpay, and the "sent funds do not equal the donation"
branch disappears because the state it guarded against cannot arise.

### Escrow, vaults and withdraw

The most common CosmWasm shape that has no port is: accumulate `info.funds` in
the contract, then let `ExecuteMsg::Withdraw {}` pay the owner later. That is
not a gap in the kit; it is the monetary law. The reference contract
deliberately forwards the donation to the beneficiary in the same message it
arrives in, which is why it ports at all.

Model a payout as a transfer the paying principal authorises in the invocation
that performs it, and keep the accounting - who is owed what - as ordinary
program state.

## Failure and gas

| CosmWasm | Runtime behaviour |
| --- | --- |
| `Err(ContractError::Variant)` | `unreachable` |
| a `StdError` from storage, serde or an overflow check | `unreachable` |
| `panic!`, overflow, index out of bounds | `unreachable` |
| an uncaught sub-message failure | `unreachable` |
| out of gas | metered resource refusal |
| contract-call depth limit | declared stack bound exhausted |

Every one of those discards every staged write and every staged effect of the
whole invocation, which is exactly a CosmWasm transaction's all-or-nothing
behaviour. `FailureMapping::outcome` returns the mapping.

The ported module checks every host status and traps unless it is zero, so a
refused write or a refused transfer can never be silently skipped. Note the
difference from a status code: a host function returning a nonzero status is
*not* an error the program may ignore, and `Code::trap_unless_ok` is what the
emitter writes for every `?` in the original handler.

## Constructs with no equivalent

| CosmWasm | Why it cannot carry over |
| --- | --- |
| `env.block.height`, `env.block.time` | the deterministic runtime has no clock; count instead, as the reference port does |
| `env.contract.address` | the program identifier travels in the effect envelope already |
| `deps.api.addr_validate`, `addr_humanize` | an address is a 32-byte identifier the runtime authenticates; there is no bech32 to validate |
| `deps.querier` bank, smart and staking queries | there is no chain view inside an execution |
| `migrate`, `sudo`, `ibc_*` entry points | versioning belongs to the deployment lifecycle and its `UpgradePolicy`, not to a guest handler |
| `Decimal`, `Decimal256`, any float | refused at validation as non-deterministic |
| randomness, block-derived entropy | refused for the same reason |

## The reference port, line by line

`DonationPort::new` takes the `Config` a CosmWasm deployment writes on
instantiation - asset, beneficiary, minimal donation and cap - and pins it into
the module, because on LayerX each deployment is its own program and its
configuration is immutable for that program.

`DonationPort::code()` emits the module. `DonationPort::code_hash()` is the
digest the deployment authenticates.

| Contract line | Emitted program |
| --- | --- |
| `CONFIG.load(deps.storage)?` | nothing; the configuration is pinned into the module |
| `if times == 0 \|\| times > config.donation_cap` | `i64.lt_s 1` and `i64.gt_s` the cap, then `unreachable` |
| `minimal_donation.checked_mul(times)` | `i64.mul` on values already bounded by the cap |
| `if sent != due` | nothing; the program requests `price * times` and the capability caps it |
| `DONATIONS.may_load(deps.storage, &info.sender)?` | `storage_read` at the collapsed key, `0` for an absent cell |
| `.map(\|r\| r.count).unwrap_or_default()` | the absent case answers `0` |
| `held.checked_add(times)` | `i64.add` |
| `if total > config.donation_cap` | `i64.gt_s` the cap, then `unreachable` |
| `DONATIONS.save(..., &DonationRecord { count: total })` | `storage_write` of a little-endian `u64` |
| `BankMsg::Send { to_address: beneficiary, amount }` | `transfer_402(0, price * times, asset, beneficiary)` |
| the `?` on every fallible call | trap unless the status is `0` |
| `Event::new("donation").add_attribute("count", total)` | `event_emit` with `wasm-donation` as the topic |
| `Ok(Response::new()...)` | the export returns `i64` |

Four divergences are deliberate and are the honest translation rather than an
emulation:

- **An absent record reads as zero.** `may_load` returns `None` for a donor who
  has never donated; namespaced storage reports absence, and the query handlers
  answer `0` donations and the full cap remaining.
- **The configuration is not stored.** On a chain the beneficiary, the denom,
  the minimal donation and the cap live in the `config` cell and are read on
  every execution. Here they are pinned into the module by the descriptor,
  which is what makes the artifact reproducible from published source.
  `DonationPort::config_key()` still returns the raw key they occupied, so a
  migration can compare them.
- **The funds check becomes a request.** See **Money**.
- **The query argument disappears.** See **Messages and dispatch**.

The capabilities a donation needs are exactly four: `StorageRead`,
`StorageWrite`, `EmitEvent`, and one `Transfer402` capped at `price * times` to
the configured beneficiary. `DonationPort::donate_capabilities` builds that set,
and a query needs `StorageRead` alone.

## Deploying and verifying a port

`src/qualify.rs` runs the real pipeline end to end:

1. `source_archive` assembles the canonical archive: the CosmWasm source, the
   port descriptor, the pinned toolchain manifest and the pinned dependency
   lock.
2. `build_plan` declares the recipe - builder identity, toolchain and lock
   digests, the exact command and the artifact path.
3. `deploy_and_verify` deploys through the real lifecycle, journals the
   deployment as a canonical record, replays that record into the registry,
   then rebuilds the published source in independent hermetic attempts through
   the real `SourceVerifier` and refuses anything short of
   `SourceStatus::Verified`.

The build is genuinely reproducible because the descriptor is the compiler's
only input: `PortBuildRunner` reads the descriptor named by the plan's pinned
command out of the archive, checks the published CosmWasm source is the
contract this kit ports, parses the descriptor and re-emits the module.

Execution goes through the real metered executor: `execute_donate`,
`execute_donations` and `execute_remaining` build the authorisation context
from the capabilities above and run the deployed module. Monetary effects leave
as typed requests; `authorize_transfers` closes them into one atomic set, and
`settle` applies them through the kernel's own primitive, which stays a
caller-supplied boundary.

## Checklist before you port

1. List every `Item` and every `Map` and classify it with `StateBinding`.
   Principal-scoped state collapses; shared state uses the shared namespace.
2. For each `Map`, mark whether the key is `info.sender` or ends in it. Those
   halves collapse; everything else stays in the key.
3. Confirm every remaining raw key frames to a key inside the storage bound.
4. List every value-moving statement and run it through `ValueFlow`. Anything
   refused has to be redesigned, not worked around - starting with any balance
   the contract accumulates.
5. Keep the variant names, the field names, the event types and the attribute
   keys. Renaming any of them breaks every existing client and indexer, exactly
   as it does on a chain.
6. Replace every `env.block` read and every `querier` round trip with something
   counted or passed in, and say so in your published source.
7. Replace every `Map::range` with a key you can address directly.
8. Publish the source, the descriptor, the toolchain manifest and the lock, and
   verify the deployment reproduces before you announce it.
