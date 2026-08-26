# Porting a Solidity contract to LayerX programs

This guide is written for someone who knows Solidity and has never written a
LayerX program. It maps the EVM vocabulary you already use onto the version-one
programs ABI, and it is explicit about the constructs that do not carry over,
because the kit refuses those by name rather than emulating them.

The worked example is `contracts/PublicLock.sol` in the published archive: a
paid-membership lock in the shape Unlock Protocol uses. Its complete port lives
in `src/reference.rs` and is deployed, source-verified and executed by
`src/qualify.rs`.

## What stays byte-identical

Three things survive the port unchanged, and the kit computes all of them with
a real `keccak256`:

| Artifact | Solidity | After the port |
| --- | --- | --- |
| Storage slot address | `keccak256(key . slot)` | the same 32 bytes, used directly as the storage key |
| Event `topic0` | `keccak256("Transfer(address,address,uint256)")` | the same 32 bytes, used as the program event topic |
| Function selector | `bytes4(keccak256("purchase(uint256)"))` | the same 4 bytes, dispatched by `layerx_call` |

Keeping these exact is the whole point of the compatibility layer: an exported
state dump imports cell for cell, an existing indexer's filters keep matching,
and a client that already encodes calldata keeps working.

## Storage

### The namespace replaces `msg.sender` in your keys

An EVM contract owns one flat `uint256 => uint256` array and derives an address
inside it for every composite variable. A LayerX program owns a byte-keyed map
inside a namespace that is `(program, principal)`, and that namespace is fixed
by the runtime *before your code runs*. You cannot choose it, widen it, or read
another principal's cell by choosing a key.

The direct consequence: **a `mapping(address => V)` indexed only by
`msg.sender` collapses onto its declared slot.**

```solidity
mapping(address => uint256) public remainingPeriodsOf;   // slot 0
remainingPeriodsOf[msg.sender] = extended;
```

```text
EVM key    keccak256(bytes32(msg.sender) . bytes32(0))    derived at runtime
LayerX key bytes32(0)                                     the slot itself
```

`layout::caller_indexed_key(0)` returns that key. The `keccak256` disappears
from the hot path entirely, because the principal half of the address is
already carried by the namespace.

### Slot derivation for everything else

`src/layout.rs` implements the EVM rules exactly, so state that is *not*
caller-indexed keeps its EVM address:

| Declaration | Address | Function |
| --- | --- | --- |
| `uint256 x` at slot `n` | `n` | `value_slot(n)` |
| `mapping(K => V)` at slot `n` | `keccak256(k . n)` | `mapping_slot(n, k)` |
| `mapping(K1 => mapping(K2 => V))` | applied outermost first | `nested_mapping_slot(n, [k1, k2])` |
| `T[] a` at slot `n` | `keccak256(n) + i` | `array_slot(n, i)` |
| struct member at offset `m` | `base + m` | `member_slot(base, m)` |

Values stay 32-byte big-endian words. `storage_key(slot)` is the identity
function on the slot address: LayerX keys are arbitrary bounded byte strings, so
there is nothing to re-derive.

### Shared state

State that is not caller-indexed - a value slot, a constant, or any mapping
that does not collapse onto `msg.sender` - belongs in the program-shared
namespace `(program)` instead of the principal-scoped namespace
`(program, principal)`. Use `shared_key(slot)` to address it and the
`layerx_program_sdk::storage::shared` module to read and write it.

A total supply, a pool reserve, an order book, or any other cell every account
must be able to read and write now has an honest representation.

### Migrating existing state

`layout::caller_indexed_import(slot, holders)` turns your live mapping into an
import plan: one `MigrationCell` per holder, naming the EVM slot to read from
your dump and the namespaced key to write. `qualify::import_state` applies the
plan, writing each cell into the namespace of the principal that owns it. Each
holder's cell lands in that holder's own namespace; there is no shared cell
anywhere, which is exactly why the mapping key was redundant.

### What has no equivalent

- **`address(this).balance`.** A program has no account. See *Money*.

## Events

The version-one event shape is one topic plus a data payload:

```text
event_emit(topic_pointer, topic_length, data_pointer, data_length)
```

Every emitted event already carries its program and its invoking principal, so
the port drops the arguments the envelope supplies and keeps the rest:

```solidity
event Transfer(address indexed from, address indexed to, uint256 indexed tokenId);
emit Transfer(address(0), msg.sender, TOKEN_ID);
```

```text
topic  keccak256("Transfer(address,address,uint256)")   unchanged
data   bytes32(TOKEN_ID)                                 32 bytes
```

`from` is the mint sentinel and `to` is `msg.sender`; both are known from the
envelope, so `EventAbi::envelope_derived(TRANSFER_EVENT, 2)` declares them and
the payload carries only `tokenId`. For an event with nothing derivable, use
`EventAbi::new`, which carries every argument in declaration order.

Solidity's *additional* indexed topics have no version-one equivalent: version
one has one topic. Indexed arguments become payload words in declaration order.
An indexer filtering on `topic0` is unaffected; an indexer filtering on an
indexed argument reads it out of the payload instead.

## Calling other contracts

`IOther(target).method(args)` becomes `program_call`, and `src/semantics.rs`
builds the request:

```rust
external_call(callee_program, &MethodAbi::new("purchase(uint256)")?, &[Word::from_u64(3)])?
```

The calldata is the EVM's own head-only encoding - the four-byte selector
followed by one 32-byte word per value-typed argument - so the callee's
dispatcher sees exactly the bytes an EVM `CALL` would have delivered.

Two differences you must design for:

1. **Authority is not ambient.** A Solidity call inherits the caller's whole
   authority. A program call carries only what the caller explicitly narrows,
   and narrowing can never raise a transfer limit. `CallRequest::authority`
   names the single grant the call needs; the caller must already hold it.
2. **Return data does not cross the boundary.** `layerx_call` returns one
   non-negative `i32` result code. A negative code refuses the whole call
   graph. Results that used to come back as ABI-encoded return data are read
   from state or from the emitted event instead.

The ported contract exports both shapes: `layerx_reserve`/`layerx_call` for
program-to-program calls with real calldata, and one named export per function
for direct activity invocation.

## Money

This is the part of the port that is not negotiable.

**No program writes a balance.** Value held for a contract lives in a real
account derived from the LayerX program identifier and a public seed. Guest
code can only request a bounded 402LXP transfer; the kernel rederives the
source, verifies owner-frame `ProgramSpend` authority, applies the whole set
atomically and issues the receipt.

| Solidity | Port |
| --- | --- |
| `payable` function funded by `msg.value` | `transfer_402` debiting the caller in the same invocation |
| `recipient.transfer(msg.value)` (forwarding what just arrived) | `ValueFlow::CallerFunded` - carried over |
| `recipient.transfer(x)` out of accumulated balance | `translate_with_program_account` and a rederived contract account |
| `selfdestruct(recipient)` | **refused**, `UnboundedBalanceSweep`; the source does not declare an exact bounded amount |
| `transferFrom(msg.sender, to, x)` | `ValueFlow::AllowanceSpend` with `owner == caller` - carried over |
| `transferFrom(other, to, x)` under an allowance | `ValueFlow::AllowanceSpend` - **refused**, `DelegatedSpend` |

`ValueFlow::translate_with_program_account` carries contract-funded payouts;
`ValueFlow::translate` remains the context-free principal-only mapping and
refuses a contract payout when no owner, seed and source were supplied. The
remaining refusals are not a limitation to work around: a
shadow ledger inside program storage that tracks "balances" would be a second,
unauthenticated money supply, and the monetary law exists to make that
impossible. Delegated spending is expressed the LayerX way - the payer grants a
capped `Transfer402` capability at invocation time - not as allowance state a
contract keeps about a third party.

The amount crosses the integer-only ABI as two `i64` limbs, high first;
`Transfer402Plan::amount_limbs` produces them.

### Escrow, splitting and refunds

A contract that takes money now and pays it out later cannot be ported as-is,
because "holding" is the part that does not exist. Restructure it so the payer
is present at payout: one invocation that debits the payer and credits the
recipients in one atomic transfer set. `PaymentSplitter`, for instance, becomes
a program that emits one leg per payee inside a single execution, each capped by
its own capability.

## Failure and gas

| Solidity | Runtime behaviour |
| --- | --- |
| `require(cond)` | `unreachable` |
| `revert CustomError()` / `revert("reason")` | `unreachable` |
| `assert(cond)` and compiler panics | `unreachable` |
| out of gas | metered resource refusal |
| call-depth limit | declared stack bound exhausted |

`FailureMapping::outcome` states the mapping. Every one of those outcomes
discards every staged storage write and every effect of the whole invocation,
which is exactly Solidity's all-or-nothing revert. Note the
difference from a status code - a host function returning a nonzero status is
*not* a revert. The port checks every status and traps, which is what
`Code::trap_unless_ok` emits.

Gas is deterministic fuel. There is no `gasleft()`, no gas stipend, and no
reentrancy window created by a 2300-gas transfer, because a transfer is a typed
request applied after your code returns rather than a call into the recipient.

## Constructs with no equivalent

| Solidity | Why it cannot carry over |
| --- | --- |
| `block.timestamp`, `block.number`, `blockhash` | the deterministic runtime has no clock and no chain view; use counted periods, as the reference port does |
| `tx.origin` | there is one authenticated principal, and it is `msg.sender` |
| `address(this).balance` | a program has no account |
| `delegatecall` | there is no shared storage frame; every call runs in the callee's own namespace |
| `create`/`create2` | programs are deployed through the lifecycle, not from inside an execution |
| `ecrecover` | signature verification is protocol authority, not program logic |
| floating point, randomness, `block.prevrandao` | refused at validation as non-deterministic |

`keccak256` inside a contract is not forbidden, but the reference port shows the
better answer: every hash a port needs - slot addresses, topics, selectors - is
constant, so the kit computes them at port time and the emitted module carries
them as data. No hash function runs at execution time at all.

## The reference port, line by line

```solidity
function purchase(uint256 periods) external payable returns (uint256) {
    require(periods > 0);
    require(periods <= maxPeriodsPerPurchase);
    require(msg.value == keyPrice * periods);
    uint256 held = remainingPeriodsOf[msg.sender];
    uint256 extended = held + periods;
    require(extended <= maxPeriodsPerKey);
    remainingPeriodsOf[msg.sender] = extended;
    (bool paid, ) = beneficiary.call{value: msg.value}("");
    require(paid);
    if (held == 0) {
        emit Transfer(address(0), msg.sender, TOKEN_ID);
    }
    emit KeyExtended(TOKEN_ID, extended);
    return extended;
}
```

| Solidity line | Emitted program |
| --- | --- |
| `require(periods > 0)` | `i64.lt_s 1` then `unreachable` |
| `require(periods <= maxPeriodsPerPurchase)` | `i64.gt_s` the bound then `unreachable` |
| `require(msg.value == keyPrice * periods)` | there is no `msg.value`; the program *requests* `keyPrice * periods`, and the capability caps it at exactly that |
| `remainingPeriodsOf[msg.sender]` | `storage_read` at the collapsed key, returning `0` for an absent cell |
| `remainingPeriodsOf[msg.sender] = extended` | `storage_write` of a 32-byte big-endian word |
| `beneficiary.call{value: ...}("")` | `transfer_402(0, price, asset, beneficiary)` |
| `require(paid)` | trap unless the status is `0` |
| `emit Transfer(...)` on first purchase | `event_emit` with the unchanged `topic0` |
| `emit KeyExtended(...)` | `event_emit` with the token id and new period count |
| `return extended` | the export returns `i64` |

`block.timestamp` expiry became a period count, which is the one place the
semantics changed rather than being preserved, and it changed because the
deterministic runtime has no clock. `getHasValidKey(address)` became
`getHasValidKey()`: a program's state view is scoped to the invoking principal,
so the only owner it can answer for is the caller.

## Deploying and verifying a port

```rust
let port = PublicLockPort::new(LockTerms { .. })?;
let source = published_source(&port, "https://example.org/public-lock")?;
let deployed = deploy_and_verify(&port, publication, &source, &mut lifecycle, &mut registry)?;
```

`deploy_and_verify` runs the real pipeline end to end:

1. emit the module and hash it with `SHA-256` - that digest is the code hash the
   deployment activity authenticates;
2. deploy through the lifecycle, which validates the module against the
   deterministic subset before any code becomes callable;
3. journal the deployment as a canonical record and replay it into the registry;
4. rebuild the published archive twice in independent attempts through the
   reproducible-build pipeline, refusing any build whose own output is not
   stable;
5. compare the rebuilt artifact digest with the registered code hash and record
   `SourceStatus::Verified`, or refuse.

The archive carries the Solidity as provenance, the port descriptor as the
build input, and the pinned toolchain and dependency lock the plan commits to by
digest. The descriptor round-trips: `PublicLockPort::parse(&port.encode())` is
the original port, which is why the rebuild is reproducible.

## Checklist before you port

1. Classify every variable as caller-indexed or shared. Caller-indexed
   mappings collapse; shared state uses the shared namespace and its dedicated
   capabilities.
2. Does any function pay out of the contract's balance? If yes, restructure it
   so the payer is present.
3. Does any function depend on time? Replace it with a counted quantity.
4. Does any external call need the callee to act with your authority? Narrow the
   capability explicitly; there is no ambient reach.
5. Do you need return data from a call? Move the result into state or an event.
