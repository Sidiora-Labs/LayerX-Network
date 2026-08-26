# Porting a Solana program to LayerX programs

## ABI v2 account context and native crypto

`anchor::context::AnchorContext::current` supplies the executing program as
`program_id`, the invoking principal as the signer account, and batch height as
the deterministic clock slot. Hashes and signature checks must use
`layerx_program_sdk::crypto`; fixed key and signature types prevent malformed
syscall shapes from being constructed and refusals remain typed.

ABI v2 does not expose an enumerable authenticated instruction account list.
Keep declared account authority in explicit capabilities; do not reconstruct
an Anchor `Accounts` list from guest calldata. This missing canonical producer
is recorded in `qualification.kvx` rather than represented as an empty list.

The executable reference is `reference-v2/src/lib.rs`. The
`programs-porting-v2-references` path builds its locked ABI v2 guest. Runtime
execution uses the production C transition's authenticated context; the
remaining qualification gate is recorded rather than replaced by local facts.

This guide is written for someone who knows Anchor and has never written a
LayerX program. It maps the Solana account vocabulary you already use onto the
programs ABI, and it is explicit about the constructs that do not
carry over, because the kit refuses those by name rather than emulating them.

The worked example is `programs/mint-limit/src/lib.rs` in the published
archive: the mint-limit and SOL-payment guards in the shape Metaplex Candy
Guard uses. Its complete port lives in `src/reference.rs` and is deployed,
source-verified and executed by `src/qualify.rs`.

## What stays byte-identical

Four things survive the port unchanged, and the kit computes all of them with a
real `sha256`:

| Artifact | Anchor | After the port |
| --- | --- | --- |
| Account discriminator | `sha256("account:MintCounter")[..8]` | the same 8 bytes, first in the stored cell |
| Instruction discriminator | `sha256("global:mint")[..8]` | the same 8 bytes, dispatched by `layerx_call` |
| Event discriminator | `sha256("event:MintPerformed")[..8]` | the same 8 bytes, used as the program event topic |
| Field layout | `borsh`, little-endian | the same bytes, after the discriminator |

Keeping these exact is the whole point of the compatibility layer: an exported
account snapshot imports byte for byte, an existing indexer's filter keeps
matching, and a client that already encodes instruction data keeps working.

## Accounts

### The namespace replaces the payer and program seeds

A Solana program addresses state by deriving addresses: an account lives at a
program-derived address, and the seeds usually carry whoever the state is
*about* plus whichever program instance owns it. A LayerX program owns a
byte-keyed map inside a namespace that is `(program, principal)`, and that
namespace is fixed by the runtime *before your code runs*. You cannot choose
it, widen it, or read another principal's cell by choosing a key.

The direct consequence: **a seed that carries the signer's public key, or the
program's own account, tells the port nothing the runtime has not already
fixed, and therefore drops out of the key.**

```rust
seeds = [
    COUNTER_SEED,               // b"mint_limit"
    &[GUARD_ID],                // 0x03
    payer.key().as_ref(),       // the signer
    candy_guard.key().as_ref()  // the guard instance
]
```

```text
Solana  a 32-byte PDA derived by sha256 over the four seeds + bump + program
LayerX  0a 6d 69 6e 74 5f 6c 69 6d 69 74 01 03      the two seeds that remain
```

`SeedPath::collapse(&[2, 3])` performs the drop and `SeedPath::storage_key()`
frames what is left. The framing writes each seed as a one-byte length followed
by its bytes, so two distinct seed paths can never produce the same key by
running into one another. No derivation runs at execution time at all.

### Every other derivation keeps its shape

`src/pubkey.rs` implements `create_program_address` exactly - the
`seeds . bump . program . "ProgramDerivedAddress"` preimage - so you can check a
published address before you migrate it:

| Task | Function |
| --- | --- |
| Derive an address from seeds and a bump | `SeedPath::address(bump, program)` |
| Check a published bump really derives a published address | `SeedPath::verify(bump, program, published)` |
| Drop the envelope-supplied seeds | `SeedPath::collapse(envelope)` |
| Frame the remaining seeds into a storage key | `SeedPath::storage_key()` |
| Rebuild a per-signer path for one holder | `SeedPath::with_seed(index, seed)` |

Solana also requires a program-derived address to lie off the ed25519 curve.
That is a property of the address the chain already published rather than
something a port re-decides, so the kit checks the derivation against the
published address instead - which is the check that can actually be wrong.

### What an account in an `Accounts` struct becomes

| Anchor account | After the port |
| --- | --- |
| `#[account(init)]` / `init_if_needed` program state | a cell in the `(program, principal)` namespace |
| program-owned account holding shared state | a cell in the `(program)` shared namespace, using `layerx_program_sdk::storage::shared` |
| `Signer<'info>` | the invoking principal, authenticated before your code runs |
| `SystemAccount` / `UncheckedAccount` credited by a transfer | the recipient of an authenticated 402LXP transfer |
| `Program<'info, System>` and other program handles | a narrowed `Call` capability naming the callee |
| a token account whose balance you move | **refused**, `LamportMutation` - balance is kernel-owned |

`AccountRole::translate` returns exactly that mapping, and refuses the last row
by name.

A program-owned account holding a total supply, a pool reserve, or any other
state every principal must reach is ported as shared state in the `(program)`
namespace instead of being refused.

### Account data carries over unchanged

`AccountSchema` encodes and decodes exactly what Anchor writes: the eight-byte
discriminator, then every field in `borsh` little-endian order. `decode` checks
the discriminator first, which is the check Anchor performs on every account
load, and the ported module performs the same check on every read.

```text
MintCounter { count: u16 }   space = 8 + 2

  00..08   sha256("account:MintCounter")[..8]
  08..10   count, little-endian u16
```

### Migrating existing accounts

`per_signer_import` builds the plan. For each holder it rebuilds that holder's
concrete seed path, verifies the published bump really derives the published
address, and emits a cell naming the Solana address to read from a snapshot and
the collapsed key to write. Every holder writes the *same* key, because the
seed that distinguished them was the signer's public key; the cells do not
collide, because each is written in a different namespace.

`qualify::import_accounts` performs the writes through the real storage
transaction, one namespace per holder, carrying the bytes across untouched.

### What has no equivalent

| Solana | Why it cannot carry over |
| --- | --- |
| lamports on a PDA | a registered program-derived value account; see **Money** |
| rent and rent exemption | there is no rent; a cell costs its bytes against the declared value bound |
| `#[account(mut, realloc = ...)]` | a value is written whole, up to the declared bound |
| `#[account(close = recipient)]` | delete the cell; the rent sweep is **refused**, `LamportMutation` |
| an account owner check | structural: another program cannot reach your namespace at all |
| `remaining_accounts` | there is no account list; a call carries data and one capability |

## Instructions and dispatch

`InstructionAbi` keeps the Anchor encoding exactly: `sha256("global:name")[..8]`
followed by the `borsh` arguments. The ported module exports `layerx_call`,
which loads the first eight bytes as a little-endian `i64` and compares it
against each handler's discriminator, so instruction data a client already
builds keeps working.

```text
mint(amount: u16)      10 bytes   sha256("global:mint")[..8]           . u16 le
mint_count()            8 bytes   sha256("global:mint_count")[..8]
mint_remaining()        8 bytes   sha256("global:mint_remaining")[..8]
```

A calling program first calls `layerx_reserve(len)` to obtain the region to
write instruction data into; the entry point refuses any other pointer and any
length that does not match the selected handler.

Each handler is also exported under its own name, so an activity that invokes
the program directly names the export instead of encoding a discriminator.

Return data does not cross the composition boundary: `layerx_call` returns an
`i32` code, and a negative code is a refusal. The reference port's handlers all
return counts bounded by the account's `u16`, so they fit.

## Events

`emit!` writes a program log holding the event discriminator followed by the
`borsh` fields. The port keeps the discriminator as the event topic and the
`borsh` fields as the payload:

```text
emit!(MintPerformed { count })

  topic   sha256("event:MintPerformed")[..8]
  data    count, little-endian u16
```

An indexer that filters on the discriminator keeps matching, and a client
decodes the payload with the generated type unchanged. The emitting program and
the invoking principal already travel in the event envelope, so a field that
only ever held `payer.key()` does not need to be repeated in the payload -
`AnchorEvent` still declares it in the type, which is what keeps the
discriminator identical.

## Cross-program invocation

A Solana CPI passes account handles the callee may mutate, and `invoke_signed`
additionally lends the caller's program-derived signing authority. Neither
survives.

```rust
cross_program_invocation(callee, &instruction, &arguments)?
```

returns the callee, the instruction data in Anchor's own encoding, and **one**
narrowed `Call` capability. The callee reaches only its own namespace, and it
holds only the authority the caller explicitly narrowed - never the caller's
ambient reach, and never a PDA signature borrowed from the caller.

## Money

This is the part of the port that bends least, so read it before you plan one.

A LayerX program **writes no balance**. A PDA that holds value maps to a real
account derived from the LayerX program and the PDA seed path. The owner frame
can request a bounded 402LXP debit from that account; the kernel rederives the
source and remains the only balance writer.

| Solana | Port |
| --- | --- |
| `system_program::transfer` from a `Signer` | `ValueFlow::SignerFunded` - carried over |
| `**account.try_borrow_mut_lamports()? -= x` | `ValueFlow::LamportWrite` - **refused**, `LamportMutation` |
| `invoke_signed` paying from a PDA the program controls | `translate_with_program_account` and a rederived program account |
| `#[account(close = recipient)]` rent sweep | **refused**, `UnboundedRentSweep`; no exact bounded amount is declared |
| SPL token `transfer` whose authority is the signer | `ValueFlow::TokenTransfer` - carried over |
| SPL token `transfer` under a delegate authority | `ValueFlow::TokenTransfer` - **refused**, `DelegatedSpend` |

`ValueFlow::portable()` answers the same question without building the leg, so
a porting tool can report every unportable statement in one pass.

Lamports themselves do not carry over either: a program is paid in an
authenticated 402LXP asset, and the port descriptor names which asset stands in
for SOL. The price is otherwise unchanged - the reference port charges
`price * amount` for `amount` mints, exactly as `SolPayment` charges
`lamports * amount`.

### Escrow, vaults and refunds

A program that accumulates lamports in a PDA vault and pays them out later has
no port. That is not a gap in the kit; it is the monetary law. Model the payout
as a transfer the paying principal authorises in the invocation that performs
it, and keep the accounting - who is owed what - as ordinary program state.

## Failure and compute

| Anchor | Runtime behaviour |
| --- | --- |
| `require!(cond, MyError::Variant)` | `unreachable` |
| `err!(MyError::Variant)` | `unreachable` |
| a failed `#[account(...)]` constraint | `unreachable` |
| a discriminator mismatch on account load | `unreachable` |
| `panic!`, overflow, index out of bounds | `unreachable` |
| compute budget exhausted | metered resource refusal |
| CPI depth limit | declared stack bound exhausted |

Every one of those discards every staged write and every staged effect of the
whole invocation, which is exactly Solana's all-or-nothing transaction
behaviour. `FailureMapping::outcome` returns the mapping.

The ported module checks every host status and traps unless it is zero, so a
refused write or a refused transfer can never be silently skipped.

## Context mappings and unavailable constructs

| Solana | Why it cannot carry over |
| --- | --- |
| `Clock::get()?.slot` | `AnchorContext::current()?.slot`, backed by authenticated batch height |
| `unix_timestamp` and other wall-clock fields | unavailable; use explicit protocol facts or a counted quantity |
| `recent_blockhashes`, `SlotHashes` | there is no chain view inside an execution |
| `invoke_signed` PDA authority | a program signs for nothing; authority is a capability the caller grants |
| account reallocation and closure for rent | there is no rent |
| `remaining_accounts` fan-out | a call carries data and one narrowed capability |
| ed25519 / secp256k1 precompiles | signature verification is protocol authority, not program logic |
| floating point, randomness | refused at validation as non-deterministic |

## The reference port, line by line

`MintLimitPort::new` takes the `GuardConfig` a Solana deployment stores in the
candy guard account - asset, destination, limit and price - and pins it into
the module, because on LayerX each deployment is its own program and its
configuration is immutable for that program.

`MintLimitPort::code()` emits the module. `MintLimitPort::code_hash()` is the
digest the deployment authenticates.

| Anchor line | Emitted program |
| --- | --- |
| `require!(amount > 0, ...)` | `i64.lt_s 1` then `unreachable` |
| `require!(amount <= config.limit, ...)` | `i64.gt_s` the bound then `unreachable` |
| `ctx.accounts.mint_counter` load | `storage_read` at the collapsed key, `0` for an absent cell |
| the Anchor discriminator check | `i64.load` compared against `sha256("account:MintCounter")[..8]`, then `unreachable` |
| `counter.count.checked_add(amount)` | `i64.add` on values already bounded by the limit |
| `require!(taken <= config.limit, ...)` | `i64.gt_s` the bound then `unreachable` |
| `counter.count = taken` | `storage_write` of the discriminator plus a little-endian `u16` |
| `system_program::transfer(..., lamports * amount)` | `transfer_402(0, price * amount, asset, destination)` |
| the `?` on the transfer | trap unless the status is `0` |
| `emit!(MintPerformed { count: taken })` | `event_emit` with the unchanged event discriminator as the topic |
| `Ok(taken)` | the export returns `i64` |

Two divergences are deliberate and are the honest translation rather than an
emulation:

- **An absent counter reads as zero.** Anchor fails to load an account that
  does not exist; namespaced storage reports absence, and the query handlers
  answer `0` taken and the full limit remaining. `init_if_needed` on the mint
  path behaves identically either way.
- **The guard configuration is not an account.** On Solana the limit, the price
  and the destination live in the candy guard account and are read at
  execution time. Here they are pinned into the module by the descriptor, which
  is what makes the artifact reproducible from published source.

The capabilities a mint needs are exactly four: `StorageRead`, `StorageWrite`,
`EmitEvent`, and one `Transfer402` capped at `price * amount` to the configured
destination. `MintLimitPort::mint_capabilities` builds that set, and a query
needs `StorageRead` alone.

## Deploying and verifying a port

`src/qualify.rs` runs the real pipeline end to end:

1. `source_archive` assembles the canonical archive: the Anchor source, the
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
command out of the archive, checks the published Anchor source is the program
this kit ports, parses the descriptor and re-emits the module.

Execution goes through the real metered executor:
`execute_mint`, `execute_mint_count` and `execute_mint_remaining` build the
authorisation context from the capabilities above and run the deployed module.
Monetary effects leave as typed requests; `authorize_transfers` closes them
into one atomic set, and `settle` applies them through the kernel's own
primitive, which stays a caller-supplied boundary.

## Checklist before you port

1. List every account in every `Accounts` struct and classify it with
   `AccountRole`. A token balance stops the port here.
2. For each program-derived address, write down the seeds and mark the ones the
   runtime supplies: the signer, and the program instance. Those collapse.
3. Confirm every remaining seed path frames to a key inside the storage bound.
4. List every value-moving statement and run it through `ValueFlow`. Anything
   refused has to be redesigned, not worked around.
5. Keep the account, instruction and event discriminators. If you rename a
   struct or a handler, every existing client and indexer breaks.
6. Map slot reads to authenticated batch height. Replace wall-clock reads with
   explicit protocol facts or a counted quantity and say so in published source.
7. Publish the source, the descriptor, the toolchain manifest and the lock, and
   verify the deployment reproduces before you announce it.
