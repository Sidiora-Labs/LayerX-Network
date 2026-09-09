# Program porting crates

The `programs/` workspace members `porting/solana`, `porting/evm`, and
`porting/cosmwasm` (plus each crate's `guest`) are the LayerX Network program porting
kits (`programs/Cargo.toml:11-16`, `programs/README.md:51`,
`programs/README.md:210-217`). Package names are `layerx-porting-solana`,
`layerx-porting-evm`, and `layerx-porting-cosmwasm`; lib names are
`programs_porting_solana`, `programs_porting_evm`, and
`programs_porting_cosmwasm`
(`programs/porting/solana/Cargo.toml:2-9`,
`programs/porting/evm/Cargo.toml:2-9`,
`programs/porting/cosmwasm/Cargo.toml:2-9`).

`make programs-build` builds the whole programs workspace
(`Makefile:2922-2923`). `make programs-test` ends with
`programs-porting-v2-references` and then `cargo test --locked --workspace`
inside `programs/` (`Makefile:3088-3089`). `programs-porting-v2-references`
builds the three `reference-v2` guests for `wasm32-unknown-unknown` release,
runs `layerx-program-lint --abi-version 2` on each artifact, and passes the
three `.wasm` paths to `programs_call_activity` (`Makefile:3091-3101`).

On the testnet branch, the EVM and Solana migration guides describe how their
account models map to registered native Asset accounts and program-derived
accounts. See [Assets](Assets.md) and
[Payments developer path](PaymentsQuickstart.md).

Sources:

- `programs/porting/solana/` (`src/lib.rs`, `error.rs`, `qualify.rs`,
  `monetary.rs`, `account.rs`, `anchor.rs`, `pubkey.rs`, `reference.rs`,
  `shared_pool.rs`, `wasm.rs`, `MIGRATION.md`, `guest/`, `reference-v2/`)
- `programs/porting/evm/` (`src/lib.rs`, `error.rs`, `qualify.rs`,
  `monetary.rs`, `layout.rs`, `semantics.rs`, `value.rs`, `reference.rs`,
  `shared_supply.rs`, `MIGRATION.md`, `guest/`, `reference-v2/`)
- `programs/porting/cosmwasm/` (`src/lib.rs`, `error.rs`, `qualify.rs`,
  `monetary.rs`, `storage.rs`, `messages.rs`, `json.rs`, `reference.rs`,
  `shared_orderbook.rs`, `MIGRATION.md`, `guest/`, `reference-v2/`)
- `programs/Cargo.toml`
- `Makefile` (`programs-porting-v2-references`, `programs-test`,
  `programs-build`)

---

## `layerx-porting-solana`

### Purpose

Crate documentation (`programs/porting/solana/src/lib.rs:2-18`): the kit
carries a Solana program onto the LayerX programs ABI. Account data stays
byte-identical (Anchor discriminator first, `borsh` fields after). Instruction
and event discriminators stay byte-identical. Constructs the account model
assumes and LayerX does not provide - a program-held lamport balance, a
program-derived signing authority over somebody else's funds, an account
another program may mutate - are refused by name at translation time. The
[`reference`](../../programs/porting/solana/src/reference.rs) port emits a
deterministic module, deploys through the real lifecycle, rebuilds from
published source, and executes under the real metered executor.

`programs_porting_solana()` returns
`programs/porting/solana targeting layerx_v2 ABI version 2`
(`programs/porting/solana/src/lib.rs:51-54`).

Guest crate `layerx-porting-solana-guest` maps Anchor context onto ABI v2:
`AnchorContext::current` reads executing program, invoking principal, and
batch height; `sha256` calls the SHA-256 syscall
(`programs/porting/solana/guest/src/lib.rs:2-13`). On `wasm32`,
`anchor::context` re-exports that guest
(`programs/porting/solana/src/anchor.rs:17-19`).

### Public entry points and types

Re-exports (`programs/porting/solana/src/lib.rs:31-49`):

| Entry | Request | Result |
| --- | --- | --- |
| `ported_account` | `&AccountSchema`, `&SeedPath`, envelope seed indices | `(Vec<u8>, usize)` key and space (`programs/porting/solana/src/account.rs:307-314`) |
| `account_discriminator` / `instruction_discriminator` / `event_discriminator` | account, handler, or event name | `[u8; 8]` (`programs/porting/solana/src/anchor.rs:51-68`) |
| `InstructionAbi::new` / `data` | handler name, `FieldType`s; `FieldValue`s | discriminator-plus-`borsh` instruction bytes (`programs/porting/solana/src/anchor.rs:111-151`) |
| `AnchorEvent::new` / `data` | event name, field types; values | `borsh` payload; discriminator is the topic (`programs/porting/solana/src/anchor.rs:180-225`) |
| `cross_program_invocation` | `ProgramId`, `&InstructionAbi`, `&[FieldValue]` | `CpiRequest { callee, input, authority: Capability::Call }` (`programs/porting/solana/src/anchor.rs:229-262`) |
| `Pubkey::new` | `[u8; 32]` | `Pubkey` (`programs/porting/solana/src/pubkey.rs:37-42`) |
| `SeedPath::new` / `verify` / `storage_key` / `collapse` | seeds; bump, program, published address; envelope indices | path, `()`, framed key, collapsed path (`programs/porting/solana/src/pubkey.rs:64-161`) |
| `per_signer_import` | base path, signer index, program, envelope, `&[AccountHolder]` | `Vec<MigrationCell>` (`programs/porting/solana/src/pubkey.rs:218-239`) |
| `AccountRole::translate` | role | `AccountMapping` (`programs/porting/solana/src/account.rs:273-282`) |
| `ValueFlow::translate` | asset, principal | `Transfer402Plan` (`programs/porting/solana/src/monetary.rs:267-291`) |
| `ValueFlow::translate_with_program_account` | asset, principal, owner, seed, source | `TranslatedValueFlow` (`programs/porting/solana/src/monetary.rs:230-254`) |
| `translate_all` | `&[ValueFlow]`, asset, principal | `Vec<Transfer402Plan>` (`programs/porting/solana/src/monetary.rs:319-329`) |
| `Transfer402Plan::new` | asset, to, amount | plan (`programs/porting/solana/src/monetary.rs:126-131`) |
| `ProgramAccountTransferPlan::new` | owner, seed, source, asset, to, amount | plan (`programs/porting/solana/src/monetary.rs:49-74`) |
| `source_archive` | `&MintLimitPort` | `SourceArchive` (`programs/porting/solana/src/qualify.rs:152-175`) |
| `published_source` | `&MintLimitPort`, `uri` | `PublishedSource` (`programs/porting/solana/src/qualify.rs:183-188`) |
| `build_plan` | none | `BuildPlan` (`programs/porting/solana/src/qualify.rs:193-213`) |
| `validated_module` | `&MintLimitPort` | `ValidatedModule` (`programs/porting/solana/src/qualify.rs:222-225`) |
| `deploy_and_verify` | port, `Publication`, `&PublishedSource`, `&mut Lifecycle`, `&mut Registry` | `DeployedGuard` (`programs/porting/solana/src/qualify.rs:236-278`) |
| `execute_mint` | port, `&Invocation`, `&mut Storage`, `amount` | `AuthorizedExecutionRecord` (`programs/porting/solana/src/qualify.rs:290-312`) |
| `execute_mint_count` / `execute_mint_remaining` | `&Invocation`, `&mut Storage` | `AuthorizedExecutionRecord` (`programs/porting/solana/src/qualify.rs:319-336`) |
| `settle` | `PreparedAuthorizedActivity`, `&mut Storage`, `&mut impl KernelTransferPrimitive` | `VerifiedStorageAssignment` (`programs/porting/solana/src/qualify.rs:345-353`) |
| `import_accounts` | `&mut Storage`, `ProgramId`, `&[(MigrationCell, Vec<u8>)]` | `usize` changed cells (`programs/porting/solana/src/qualify.rs:367-380`) |
| `MintLimitPort::new` | `GuardTerms` | port (`programs/porting/solana/src/reference.rs:328-349`) |

`Invocation` carries `module`, `program`, `principal`, `receipts`
(`programs/porting/solana/src/qualify.rs:134-143`). `Publication` carries
`program`, `sequence`, `observed_at`
(`programs/porting/solana/src/qualify.rs:109-116`). `AbsentReceipts` returns
`AbiError::ReceiptMismatch` for every digest
(`programs/porting/solana/src/qualify.rs:53-57`). `AuthorizedExecutionRequest`
uses `CALL_ENTRY_EXPORT`, isolated composition, `response_capacity: 0`
(`programs/porting/solana/src/qualify.rs:299-310`).

### Typed refusals

`PortRefusal` (`programs/porting/solana/src/error.rs:16-73`):

| Variant | Condition that raises it |
| --- | --- |
| `ZeroPubkey` | `Pubkey::new` of the all-zero key (`programs/porting/solana/src/pubkey.rs:37-40`); `MintLimitPort::new` with zero asset or destination (`programs/porting/solana/src/reference.rs:329-331`); `per_signer_import` with zero principal (`programs/porting/solana/src/pubkey.rs:228-230`) |
| `InvalidSeeds` | empty path, more than `MAX_SEEDS` 16, or a seed longer than `MAX_SEED_BYTES` 32 (`programs/porting/solana/src/pubkey.rs:64-70`); framed key longer than `MAX_STORAGE_KEY_BYTES` (`programs/porting/solana/src/pubkey.rs:133-135`); collapse/with_seed index out of range (`programs/porting/solana/src/pubkey.rs:149-151`, `programs/porting/solana/src/pubkey.rs:170-174`); `MintLimitPort::code` when the storage key length does not fit `i32` (`programs/porting/solana/src/reference.rs:516`) |
| `DerivationMismatch` | `SeedPath::verify` when bump and program do not derive the published address (`programs/porting/solana/src/pubkey.rs:106-116`) |
| `DiscriminatorMismatch` | `AccountSchema::decode` when the first eight bytes are not the schema discriminator (`programs/porting/solana/src/account.rs:225-228`) |
| `AccountBounds` | schema `space()` above `MAX_STORAGE_VALUE_BYTES` (`programs/porting/solana/src/account.rs:147-149`); decode length not equal to declared space (`programs/porting/solana/src/account.rs:229-231`); bool byte other than 0 or 1, or a field slice shorter than its width (`programs/porting/solana/src/account.rs:318-337`) |
| `SchemaMismatch` | unnamed account, more than `MAX_FIELDS` 64, unnamed or repeated field (`programs/porting/solana/src/account.rs:128-140`); encode value count or type mismatch (`programs/porting/solana/src/account.rs:203-212`); unnamed instruction/event, more than `MAX_ARGUMENTS` 32, or argument list mismatch (`programs/porting/solana/src/anchor.rs:70-90`); `PoolReserve::decode` when the value list is not `(U64, U32, Pubkey)` (`programs/porting/solana/src/shared_pool.rs:100-105`) |
| `LamportMutation` | `AccountRole::TokenBalance` (`programs/porting/solana/src/account.rs:280`); `ValueFlow::translate` of `LamportWrite`, `ProgramAuthorityFunded`, or `RentSweep` (`programs/porting/solana/src/monetary.rs:276-278`); `ValueFlow::translate_with_program_account` of `LamportWrite` (`programs/porting/solana/src/monetary.rs:248`) |
| `InvalidProgramAccount` | `ProgramAccountTransferPlan::new` when asset/to/amount is reserved-zero or `derive_program_account` does not equal `source` (`programs/porting/solana/src/monetary.rs:57-65`); `ProgramAuthorityFunded` whose `authority` is not `source` (`programs/porting/solana/src/monetary.rs:239-241`) |
| `UnboundedRentSweep` | `RentSweep` through `translate_with_program_account` (`programs/porting/solana/src/monetary.rs:249`) |
| `DelegatedSpend` | `TokenTransfer` whose `authority` is not the invoking principal (`programs/porting/solana/src/monetary.rs:279-288`) |
| `OutOfRange` | `Transfer402Plan::new` with zero asset, recipient, or amount (`programs/porting/solana/src/monetary.rs:126-129`); `MintLimitPort::new` zero price/limit or limit above `MAX_LIMIT_BOUND` 65535, or `price * limit` outside `i64::MAX` (`programs/porting/solana/src/reference.rs:332-342`); mint amount not in `u16` (`programs/porting/solana/src/qualify.rs:297`); unknown query export (`programs/porting/solana/src/qualify.rs:390`) |
| `EventDataTooLarge` | `AnchorEvent::data` payload longer than `MAX_EVENT_DATA_BYTES` (`programs/porting/solana/src/anchor.rs:221-223`) |
| `InstructionDataTooLarge` | `InstructionAbi::data` longer than `MAX_CALL_INPUT_BYTES` (`programs/porting/solana/src/anchor.rs:147-149`) |
| `ModuleTooLarge` | emitted WASM longer than `DEFAULT_MAX_MODULE_BYTES` (`programs/porting/solana/src/reference.rs:576-580`) |
| `InvalidDescriptor` | `MintLimitPort::parse` malformed line, repeated key, unknown key, wrong version or program name, missing key, or non-integer number (`programs/porting/solana/src/reference.rs:478-505`, `programs/porting/solana/src/reference.rs:894-911`) |
| `UnverifiedSource` | `deploy_and_verify` when `registry.verify_source` is not `SourceStatus::Verified` (`programs/porting/solana/src/qualify.rs:268-270`) |
| `Abi` / `Storage` / `Engine` / `Validation` / `Lifecycle` / `Execution` / `Registry` / `Archive` / `Build` / `TransferLaw` | `From` wrappers (`programs/porting/solana/src/error.rs:128-185`). `settle` maps `strict_settle` failure through `PortRefusal::from` (`programs/porting/solana/src/qualify.rs:349-352`) |

`FailureMapping` is not a `PortRefusal`. `Require`, `ConstraintViolation`,
`DiscriminatorMismatch`, and `Panic` map to `RuntimeOutcome::Trap`;
`ComputeBudget` to `ResourceRefusal`; `CpiDepth` to `StackExhausted`
(`programs/porting/solana/src/anchor.rs:293-305`).

### Qualification path

`qualify.rs` documentation: the engine, lifecycle, registry, reproducible-build
pipeline, metered executor, and monetary law are production types; the kernel
transfer primitive and receipt oracle stay caller-supplied
(`programs/porting/solana/src/qualify.rs:1-9`).

Checks before a ported program is admitted:

1. `source_archive` packs Anchor source, descriptor, toolchain manifest, and
   dependency lock (`programs/porting/solana/src/qualify.rs:152-174`).
2. `build_plan` pins builder identity `LayerX/porting/solana/builder/v1`,
   toolchain and lock digests, `SOURCE_EPOCH` 1, and `BUILD_COMMAND`
   (`programs/porting/solana/src/qualify.rs:37-42`,
   `programs/porting/solana/src/qualify.rs:193-213`).
3. `validated_module` runs `WasmEngine::declared()?.validate(&port.code()?)`
   (`programs/porting/solana/src/qualify.rs:222-225`).
4. `deploy_and_verify` deploys with `ABI_VERSION` and
   `UpgradePolicy::Immutable`, journals `DeploymentRecord`, replays into the
   registry, rebuilds with `SourceVerifier::new(PortBuildRunner, REBUILD_ATTEMPTS)`
   where `REBUILD_ATTEMPTS` is 2, and refuses unless status is `Verified`
   (`programs/porting/solana/src/qualify.rs:35-37`,
   `programs/porting/solana/src/qualify.rs:243-270`).
5. `PortBuildRunner::run` requires the plan command's last word as descriptor
   path, UTF-8 descriptor text, `SOURCE_PATH` equal to `ANCHOR_SOURCE`, then
   `MintLimitPort::parse` and `port.code()`
   (`programs/porting/solana/src/qualify.rs:69-104`).

### Tests that prove refusals

| Test | File | Asserted outcome |
| --- | --- | --- |
| `invoke_signed_uses_derived_account_authority` | `programs/porting/solana/src/monetary.rs:335-354` | `ProgramAuthorityFunded` with matching PDA source is `TranslatedValueFlow::ProgramAccount`; `RentSweep` through `translate_with_program_account` is `Err(PortRefusal::UnboundedRentSweep)` |

`shared_pool` tests assert role mappings and round-trips; they do not assert a
`PortRefusal` (`programs/porting/solana/src/shared_pool.rs:170-239`). No other
`#[test]` in this crate asserts a `PortRefusal` variant.

---

## `layerx-porting-evm`

### Purpose

Crate documentation (`programs/porting/evm/src/lib.rs:2-16`): the kit carries
an EVM contract onto the LayerX programs ABI. Storage slot addresses stay
byte-identical. Event topics and four-byte selectors stay byte-identical.
Constructs the EVM model assumes and LayerX does not provide - a
contract-held balance, a clock, ambient authority over another account's funds
- are refused by name at translation time. The reference port in `reference`
emits, deploys, rebuilds, and executes on the real plane.

`programs_porting_evm()` returns
`programs/porting/evm targeting layerx_v2 ABI version 2`
(`programs/porting/evm/src/lib.rs:49-52`).

Guest crate `layerx-porting-evm-guest` maps `msg.sender`, `address(this)`,
`block.number`, `keccak256`, and `ecrecover` onto ABI v2
(`programs/porting/evm/guest/src/lib.rs:2-21`). On `wasm32`,
`semantics::context` re-exports that guest
(`programs/porting/evm/src/semantics.rs:13-15`).

### Public entry points and types

Re-exports (`programs/porting/evm/src/lib.rs:30-47`):

| Entry | Request | Result |
| --- | --- | --- |
| `value_slot` / `mapping_slot` / `nested_mapping_slot` / `array_slot` / `member_slot` | slot index and keys | `Word` EVM address (`programs/porting/evm/src/layout.rs:47-91`) |
| `storage_key` / `caller_indexed_key` / `shared_key` | slot | `[u8; 32]` (`programs/porting/evm/src/layout.rs:99-124`) |
| `caller_indexed_import` | slot, `&[(Address, [u8; 32])]` | `Vec<MigrationCell>` (`programs/porting/evm/src/layout.rs:145-161`) |
| `Address::new` / `Word::to_u64` / `Word::to_u128` | 20-byte address; 32-byte word | `Address` or narrowed integer (`programs/porting/evm/src/value.rs:66-87`, `programs/porting/evm/src/value.rs:116-121`) |
| `EventAbi::new` / `envelope_derived` / `data` | canonical signature; words | topic plus payload (`programs/porting/evm/src/semantics.rs:57-137`) |
| `MethodAbi::new` / `calldata` | canonical signature; words | selector-plus-words (`programs/porting/evm/src/semantics.rs:154-200`) |
| `external_call` | `ProgramId`, `&MethodAbi`, `&[Word]` | `CallRequest { callee, input, authority: Capability::Call }` (`programs/porting/evm/src/semantics.rs:223-234`) |
| `ValueFlow::translate` / `translate_with_program_account` / `translate_all` | asset, principal; plus owner/seed/source | `Transfer402Plan` or `TranslatedValueFlow` (`programs/porting/evm/src/monetary.rs:219-313`) |
| `source_archive` / `published_source` / `build_plan` / `validated_module` | `&PublicLockPort` (and URI for published source) | archive, `PublishedSource`, `BuildPlan`, `ValidatedModule` (`programs/porting/evm/src/qualify.rs:153-226`) |
| `deploy_and_verify` | port, `Publication`, source, lifecycle, registry | `DeployedLock` (`programs/porting/evm/src/qualify.rs:237-279`) |
| `execute_purchase` | port, `&Invocation`, `&mut Storage`, `periods` | `AuthorizedExecutionRecord` (`programs/porting/evm/src/qualify.rs:290-311`) |
| `execute_has_valid_key` / `execute_remaining_periods` | `&Invocation`, `&mut Storage` | `AuthorizedExecutionRecord` (`programs/porting/evm/src/qualify.rs:318-335`) |
| `settle` | `PreparedAuthorizedActivity`, storage, kernel | `VerifiedStorageAssignment` (`programs/porting/evm/src/qualify.rs:344-352`) |
| `import_state` | storage, program, `&[(MigrationCell, [u8; 32])]` | `usize` (`programs/porting/evm/src/qualify.rs:362-375`) |
| `PublicLockPort::new` | `LockTerms` | port (`programs/porting/evm/src/reference.rs:265-295`) |

`execute_purchase` builds calldata with `MethodAbi::new(PURCHASE_METHOD)` and
`Word::from_u64(periods)` (`programs/porting/evm/src/qualify.rs:296-297`).

### Typed refusals

`PortRefusal` (`programs/porting/evm/src/error.rs:15-66`):

| Variant | Condition that raises it |
| --- | --- |
| `ZeroAddress` | `Address::new` of the 20-byte zero address (`programs/porting/evm/src/value.rs:116-119`); `PublicLockPort::new` with zero asset or beneficiary (`programs/porting/evm/src/reference.rs:266-268`) |
| `WordTooWide` | `Word::to_u64` when bytes `[..24]` are not zero; `Word::to_u128` when bytes `[..16]` are not zero (`programs/porting/evm/src/value.rs:66-83`) |
| `InvalidSignature` | canonical ABI signature missing `(`, not ending `)`, empty name, longer than 256 bytes, or empty parameter token (`programs/porting/evm/src/semantics.rs:19-32`); topic longer than `MAX_EVENT_TOPIC_BYTES` (`programs/porting/evm/src/semantics.rs:84-86`) |
| `ArgumentCountMismatch` | empty nested-mapping key path (`programs/porting/evm/src/layout.rs:67-69`); `envelope_derived` with `derived` larger than argument count (`programs/porting/evm/src/semantics.rs:76-78`); `EventAbi::data` / `MethodAbi::calldata` wrong word count (`programs/porting/evm/src/semantics.rs:126-128`, `programs/porting/evm/src/semantics.rs:188-190`); `SharedSupplyPort::new` with zero `price_per_token` (`programs/porting/evm/src/shared_supply.rs:44-47`) |
| `EventDataTooLarge` | encoded event data longer than `MAX_EVENT_DATA_BYTES` (`programs/porting/evm/src/semantics.rs:133-135`) |
| `CalldataTooLarge` | encoded calldata longer than `MAX_CALL_INPUT_BYTES` (`programs/porting/evm/src/semantics.rs:196-198`) |
| `ContractHeldBalance` | `ValueFlow::translate` of `ContractFunded` or `SelfDestructSweep` (`programs/porting/evm/src/monetary.rs:258-260`) |
| `InvalidProgramAccount` | `ProgramAccountTransferPlan::new` reserved fields or source mismatch (`programs/porting/evm/src/monetary.rs:55-63`) |
| `UnboundedBalanceSweep` | `SelfDestructSweep` through `translate_with_program_account` (`programs/porting/evm/src/monetary.rs:232`) |
| `DelegatedSpend` | `AllowanceSpend` whose `owner` is not the invoking principal (`programs/porting/evm/src/monetary.rs:261-270`) |
| `OutOfRange` | `Transfer402Plan::new` zero asset/to/amount (`programs/porting/evm/src/monetary.rs:125-128`); `caller_indexed_import` zero principal (`programs/porting/evm/src/layout.rs:151-153`); `PublicLockPort::new` zero price/token_id/per-purchase bound, per-purchase above `MAX_PERIODS_BOUND` 4096, key bound below per-purchase or above `MAX_TOTAL_PERIODS_BOUND`, or price product outside `i64::MAX` (`programs/porting/evm/src/reference.rs:269-285`); unknown query export (`programs/porting/evm/src/qualify.rs:385`) |
| `ModuleTooLarge` | `PublicLockPort::code` or `SharedSupplyPort::code` WASM above `DEFAULT_MAX_MODULE_BYTES` (`programs/porting/evm/src/reference.rs:502-506`, `programs/porting/evm/src/shared_supply.rs:133-137`) |
| `InvalidDescriptor` | `PublicLockPort::parse` malformed/repeated/unknown/missing keys, wrong version or contract name, non-integer (`programs/porting/evm/src/reference.rs:410-440`, `programs/porting/evm/src/reference.rs:836-853`) |
| `UnverifiedSource` | `deploy_and_verify` when source status is not `Verified` (`programs/porting/evm/src/qualify.rs:269-271`) |
| `Abi` / `Storage` / `Engine` / `Validation` / `Lifecycle` / `Execution` / `Registry` / `Archive` / `Build` / `TransferLaw` | `From` wrappers (`programs/porting/evm/src/error.rs:113-170`). `settle` uses `PortRefusal::from(failure.error())` (`programs/porting/evm/src/qualify.rs:349-351`) |

`FailureMapping::Require` / `Revert` / `AssertPanic` map to `Trap`; `OutOfGas`
to `ResourceRefusal`; `CallDepth` to `StackExhausted`
(`programs/porting/evm/src/semantics.rs:263-272`).

### Qualification path

Same pipeline shape as Solana, with EVM names
(`programs/porting/evm/src/qualify.rs:1-9`):

1. Archive holds `SOLIDITY_SOURCE`, descriptor, toolchain, lock
   (`programs/porting/evm/src/qualify.rs:153-176`).
2. Builder domain `LayerX/porting/evm/builder/v1`, `REBUILD_ATTEMPTS` 2,
   `SOURCE_EPOCH` 1 (`programs/porting/evm/src/qualify.rs:38-44`,
   `programs/porting/evm/src/qualify.rs:194-214`).
3. `validated_module` is `WasmEngine::declared()?.validate(&port.code()?)`
   (`programs/porting/evm/src/qualify.rs:223-226`).
4. `deploy_and_verify` deploys immutable ABI-versioned WASM, journals, rebuilds
   via `PortBuildRunner`, refuses non-`Verified` status
   (`programs/porting/evm/src/qualify.rs:237-271`).
5. `PortBuildRunner` requires published Solidity equal to `SOLIDITY_SOURCE`,
   then `PublicLockPort::parse` and `port.code()`
   (`programs/porting/evm/src/qualify.rs:69-105`).

### Tests that prove refusals

| Test | File | Asserted outcome |
| --- | --- | --- |
| `accumulated_contract_value_uses_derived_account_authority` | `programs/porting/evm/src/monetary.rs:319-334` | `ContractFunded` through `translate_with_program_account` is `TranslatedValueFlow::ProgramAccount`; `SelfDestructSweep` is `Err(PortRefusal::UnboundedBalanceSweep)` |

`shared_supply` tests assert keys and capability sets; they do not assert a
`PortRefusal` (`programs/porting/evm/src/shared_supply.rs:291-356`).

---

## `layerx-porting-cosmwasm`

### Purpose

Crate documentation (`programs/porting/cosmwasm/src/lib.rs:2-23`): the kit
carries a CosmWasm contract onto the LayerX programs ABI. Raw storage keys stay
byte-identical (`cw-storage-plus` `Item` and `Map` prefix framing). JSON
message and event names stay byte-identical. Constructs the chain model assumes
and LayerX does not provide - a contract-held bank balance, spending somebody
else's allowance, a `Deps::querier` round trip, state shared across senders -
are refused by name at translation time. JSON stops at the edge; the running
program moves canonically framed bytes. The reference port emits, deploys,
rebuilds, and executes on the real plane.

`programs_porting_cosmwasm()` returns
`programs/porting/cosmwasm targeting layerx_v2 ABI version 2`
(`programs/porting/cosmwasm/src/lib.rs:56-59`).

Guest crate `layerx-porting-cosmwasm-guest` maps `Env` / `MessageInfo` onto
executing program, batch height, and invoking principal, and exposes BLAKE3
(`programs/porting/cosmwasm/guest/src/lib.rs:2-35`). On `wasm32`,
`messages::context` re-exports that guest
(`programs/porting/cosmwasm/src/messages.rs:25-27`).

`StateBinding::Shared` is portable and `shared()` is true
(`programs/porting/cosmwasm/src/storage.rs:122-133`). `PortRefusal::SharedState`
remains in the enum (`programs/porting/cosmwasm/src/error.rs:24-27`); no path
in this crate constructs it.

### Public entry points and types

Re-exports (`programs/porting/cosmwasm/src/lib.rs:36-54`):

| Entry | Request | Result |
| --- | --- | --- |
| `item_key` / `map_prefix` / `map_key` / `composite_map_key` | namespace and key bytes | raw key (`programs/porting/cosmwasm/src/storage.rs:34-88`) |
| `StateBinding::layerx_key` | namespace, leading key elements | LayerX key (`programs/porting/cosmwasm/src/storage.rs:114-120`) |
| `sender_indexed_import` | namespace, `&[StateHolder]` | `Vec<MigrationCell>` (`programs/porting/cosmwasm/src/storage.rs:171-191`) |
| `RecordSchema::new` / `encode` / `decode` / `encode_json` / `decode_json` / `transcode` | name, `FieldSchema`s; values or JSON | framed bytes or JSON (`programs/porting/cosmwasm/src/json.rs:144-169`) |
| `variant_tag` | `EntryPoint`, variant name | `[u8; 8]` (`programs/porting/cosmwasm/src/messages.rs:74-90`) |
| `MessageVariant::new` / `json` / `data` / `transcode` | entry, variant, `RecordSchema`; values or JSON | JSON document or tag-plus-body (`programs/porting/cosmwasm/src/messages.rs:108-201`) |
| `ContractEvent::response` / `custom` / `data` | attributes; event type | topic plus framed attributes (`programs/porting/cosmwasm/src/messages.rs:218-293`) |
| `execute_submessage` | `ProgramId`, `&MessageVariant`, `&[FieldValue]` | `CallRequest` (`programs/porting/cosmwasm/src/messages.rs:322-333`) |
| `ValueFlow::translate` / `translate_with_program_account` / `translate_all` | asset, principal; plus owner/seed/source | `Transfer402Plan` or `TranslatedValueFlow` (`programs/porting/cosmwasm/src/monetary.rs:233-330`) |
| `source_archive` / `published_source` / `build_plan` / `validated_module` | `&DonationPort` | archive, source, plan, module (`programs/porting/cosmwasm/src/qualify.rs:154-227`) |
| `deploy_and_verify` | port, `Publication`, source, lifecycle, registry | `DeployedContract` (`programs/porting/cosmwasm/src/qualify.rs:238-279`) |
| `execute_donate` | port, `&Invocation`, storage, `times` | `AuthorizedExecutionRecord` (`programs/porting/cosmwasm/src/qualify.rs:293-314`) |
| `execute_donations` / `execute_remaining` | `&Invocation`, storage | `AuthorizedExecutionRecord` (`programs/porting/cosmwasm/src/qualify.rs:321-338`) |
| `settle` | activity, storage, kernel | `VerifiedStorageAssignment` (`programs/porting/cosmwasm/src/qualify.rs:347-355`) |
| `import_state` | storage, program, `&[(MigrationCell, String)]` | `usize`; JSON transcoded through `donation_record()` (`programs/porting/cosmwasm/src/qualify.rs:371-386`) |
| `DonationPort::new` | `DonationTerms` | port (`programs/porting/cosmwasm/src/reference.rs:353-374`) |

### Typed refusals

`PortRefusal` (`programs/porting/cosmwasm/src/error.rs:16-82`):

| Variant | Condition that raises it |
| --- | --- |
| `EmptyAddress` | `DonationPort::new` with zero asset or beneficiary (`programs/porting/cosmwasm/src/reference.rs:354-356`); `sender_indexed_import` with empty address bytes or zero principal (`programs/porting/cosmwasm/src/storage.rs:177-183`) |
| `InvalidNamespace` | empty namespace or length above `MAX_NAMESPACE_BYTES` 65535 (`programs/porting/cosmwasm/src/storage.rs:193-197`); length-prefix element longer than `u16` (`programs/porting/cosmwasm/src/storage.rs:200-201`) |
| `KeyTooLong` | composed key empty or longer than `MAX_STORAGE_KEY_BYTES` (`programs/porting/cosmwasm/src/storage.rs:207-210`); `DonationPort::code` when the storage key length does not fit `i32` (`programs/porting/cosmwasm/src/reference.rs:538`) |
| `SharedState` | defined (`programs/porting/cosmwasm/src/error.rs:24-27`). No raise site in this crate |
| `SchemaMismatch` | unnamed/oversized/duplicate record or field (`programs/porting/cosmwasm/src/json.rs:151-163`); encode/decode width or type mismatch; unnamed `MessageVariant`; unnamed/duplicate/oversized event attribute; `ContractEvent::data` count/type mismatch (`programs/porting/cosmwasm/src/messages.rs:108-111`, `programs/porting/cosmwasm/src/messages.rs:228-249`, `programs/porting/cosmwasm/src/messages.rs:274-285`) |
| `InvalidJson` | malformed document, unknown/repeated/missing field, or values that do not match the declared JSON types (`programs/porting/cosmwasm/src/error.rs:31-33`, `programs/porting/cosmwasm/src/json.rs:268-305`, `programs/porting/cosmwasm/src/messages.rs:387-388`) |
| `ContractHeldBalance` | `ValueFlow::translate` of `BankSend`, `BankBurn`, or `SubMessageFunds` (`programs/porting/cosmwasm/src/monetary.rs:277-279`) |
| `InvalidProgramAccount` | `ProgramAccountTransferPlan::new` reserved fields or source mismatch (`programs/porting/cosmwasm/src/monetary.rs:57-65`) |
| `SupplyMutation` | `BankBurn` through `translate_with_program_account` (`programs/porting/cosmwasm/src/monetary.rs:248`) |
| `DelegatedSpend` | `AllowanceSpend` whose `owner` is not the invoking principal (`programs/porting/cosmwasm/src/monetary.rs:280-289`) |
| `ChainQuery` | `IbcTransfer` through `ValueFlow::translate` (`programs/porting/cosmwasm/src/monetary.rs:291`). No other raise site |
| `OutOfRange` | `Transfer402Plan::new` zeros (`programs/porting/cosmwasm/src/monetary.rs:126-129`); `DonationPort::new` zero price/cap, cap above `MAX_CAP_BOUND`, or product outside `i64::MAX` (`programs/porting/cosmwasm/src/reference.rs:357-367`); unknown query export (`programs/porting/cosmwasm/src/qualify.rs:396`) |
| `EventDataTooLarge` | `ContractEvent::data` above `MAX_EVENT_DATA_BYTES` (`programs/porting/cosmwasm/src/messages.rs:289-291`); `DonationPort::code` event template length or count offset not fitting `i32`/`u32` (`programs/porting/cosmwasm/src/reference.rs:542-546`) |
| `TopicTooLarge` | event topic longer than `MAX_EVENT_TOPIC_BYTES` (`programs/porting/cosmwasm/src/messages.rs:237-240`); `DonationPort::code` topic length not fitting `i32` (`programs/porting/cosmwasm/src/reference.rs:541`) |
| `MessageTooLarge` | `MessageVariant::data` longer than `MAX_CALL_INPUT_BYTES` (`programs/porting/cosmwasm/src/messages.rs:184-186`) |
| `ModuleTooLarge` | emitted WASM above `DEFAULT_MAX_MODULE_BYTES` (`programs/porting/cosmwasm/src/reference.rs:601-606`) |
| `InvalidDescriptor` | `DonationPort::parse` malformed/repeated/unknown/missing keys, wrong version or contract name, non-integer (`programs/porting/cosmwasm/src/reference.rs:500-527`, `programs/porting/cosmwasm/src/reference.rs:962-979`) |
| `UnverifiedSource` | `deploy_and_verify` when source status is not `Verified` (`programs/porting/cosmwasm/src/qualify.rs:270-272`) |
| `Abi` / `Storage` / `Engine` / `Validation` / `Lifecycle` / `Execution` / `Registry` / `Archive` / `Build` / `TransferLaw` | `From` wrappers (`programs/porting/cosmwasm/src/error.rs:135-192`). `settle` uses `PortRefusal::from(failure.error())` (`programs/porting/cosmwasm/src/qualify.rs:352-354`) |

`FailureMapping::ContractError` / `StdError` / `Panic` / `SubMessageFailure`
map to `Trap`; `OutOfGas` to `ResourceRefusal`; `CallDepth` to
`StackExhausted` (`programs/porting/cosmwasm/src/messages.rs:365-376`).

### Qualification path

Same pipeline shape (`programs/porting/cosmwasm/src/qualify.rs:1-9`):

1. Archive holds `COSMWASM_SOURCE`, descriptor, toolchain, lock
   (`programs/porting/cosmwasm/src/qualify.rs:154-177`).
2. Builder domain `LayerX/porting/cosmwasm/builder/v1`, `REBUILD_ATTEMPTS` 2,
   `SOURCE_EPOCH` 1 (`programs/porting/cosmwasm/src/qualify.rs:37-43`,
   `programs/porting/cosmwasm/src/qualify.rs:195-215`).
3. `validated_module` is `WasmEngine::declared()?.validate(&port.code()?)`
   (`programs/porting/cosmwasm/src/qualify.rs:224-227`).
4. `deploy_and_verify` deploys immutable ABI-versioned WASM, journals, rebuilds
   via `PortBuildRunner`, refuses non-`Verified` status
   (`programs/porting/cosmwasm/src/qualify.rs:238-272`).
5. `PortBuildRunner` requires published source equal to `COSMWASM_SOURCE`, then
   `DonationPort::parse` and `port.code()`
   (`programs/porting/cosmwasm/src/qualify.rs:70-106`). Relinking the original
   `wasmd` artifact is not a port (`programs/porting/cosmwasm/src/qualify.rs:61-66`).

### Tests that prove refusals

| Test | File | Asserted outcome |
| --- | --- | --- |
| `bank_send_uses_derived_account_authority` | `programs/porting/cosmwasm/src/monetary.rs:336-352` | `BankSend` through `translate_with_program_account` is `TranslatedValueFlow::ProgramAccount`; `BankBurn` is `Err(PortRefusal::SupplyMutation)` |

`shared_orderbook` tests assert `StateBinding` classification and keys; they
do not assert a `PortRefusal`
(`programs/porting/cosmwasm/src/shared_orderbook.rs:130-194`).

---

## Comparison

| | Solana | EVM | CosmWasm |
| --- | --- | --- | --- |
| Workspace member | `porting/solana` (`programs/Cargo.toml:15`) | `porting/evm` (`programs/Cargo.toml:13`) | `porting/cosmwasm` (`programs/Cargo.toml:11`) |
| Guest member | `porting/solana/guest` (`programs/Cargo.toml:16`) | `porting/evm/guest` (`programs/Cargo.toml:14`) | `porting/cosmwasm/guest` (`programs/Cargo.toml:12`) |
| Identifier | `programs/porting/solana targeting layerx_v2 ABI version 2` (`programs/porting/solana/src/lib.rs:53-54`) | `programs/porting/evm targeting layerx_v2 ABI version 2` (`programs/porting/evm/src/lib.rs:51-52`) | `programs/porting/cosmwasm targeting layerx_v2 ABI version 2` (`programs/porting/cosmwasm/src/lib.rs:58-59`) |
| Byte-identical carry | account / instruction / event discriminators and `borsh` account bytes (`programs/porting/solana/src/lib.rs:4-8`) | storage slots, event topics, four-byte selectors (`programs/porting/evm/src/lib.rs:4-7`) | `cw-storage-plus` raw keys; JSON message and event names (`programs/porting/cosmwasm/src/lib.rs:4-8`) |
| Reference port | `MintLimitPort` / `GuardTerms` (`programs/porting/solana/src/lib.rs:49`) | `PublicLockPort` / `LockTerms` (`programs/porting/evm/src/lib.rs:43`) | `DonationPort` / `DonationTerms` (`programs/porting/cosmwasm/src/lib.rs:50`) |
| Deployed type | `DeployedGuard` (`programs/porting/solana/src/qualify.rs:120`) | `DeployedLock` (`programs/porting/evm/src/qualify.rs:121`) | `DeployedContract` (`programs/porting/cosmwasm/src/qualify.rs:122`) |
| Execute entries | `execute_mint`, `execute_mint_count`, `execute_mint_remaining` (`programs/porting/solana/src/lib.rs:44-46`) | `execute_purchase`, `execute_has_valid_key`, `execute_remaining_periods` (`programs/porting/evm/src/lib.rs:39-40`) | `execute_donate`, `execute_donations`, `execute_remaining` (`programs/porting/cosmwasm/src/lib.rs:46-47`) |
| Import | `import_accounts` of `(MigrationCell, Vec<u8>)` (`programs/porting/solana/src/qualify.rs:367-371`) | `import_state` of `(MigrationCell, [u8; 32])` (`programs/porting/evm/src/qualify.rs:362-366`) | `import_state` of `(MigrationCell, String)` transcoded from JSON (`programs/porting/cosmwasm/src/qualify.rs:371-380`) |
| Rebuilds | `REBUILD_ATTEMPTS` 2 (`programs/porting/solana/src/qualify.rs:37`) | 2 (`programs/porting/evm/src/qualify.rs:38`) | 2 (`programs/porting/cosmwasm/src/qualify.rs:37`) |
| Provenance check | `ANCHOR_SOURCE` (`programs/porting/solana/src/qualify.rs:92-96`) | `SOLIDITY_SOURCE` (`programs/porting/evm/src/qualify.rs:93-97`) | `COSMWASM_SOURCE` (`programs/porting/cosmwasm/src/qualify.rs:94-98`) |
| Zero-identity refusal | `ZeroPubkey` | `ZeroAddress` | `EmptyAddress` |
| Direct program-balance write without derived-account context | `LamportMutation` | `ContractHeldBalance` | `ContractHeldBalance` |
| Unbounded close/sweep | `UnboundedRentSweep` | `UnboundedBalanceSweep` | no sweep variant; `BankBurn` is `SupplyMutation` |
| Third-party allowance | `DelegatedSpend` | `DelegatedSpend` | `DelegatedSpend` |
| Inter-chain / querier | no `ChainQuery` variant | no `ChainQuery` variant | `ChainQuery` on `IbcTransfer` only (`programs/porting/cosmwasm/src/monetary.rs:291`) |
| Emitter size refusal | `ModuleTooLarge` (`programs/porting/solana/src/reference.rs:576-580`) | `ModuleTooLarge` (`programs/porting/evm/src/reference.rs:502-506`) | `ModuleTooLarge` (`programs/porting/cosmwasm/src/reference.rs:601-606`) |
| Refusal test in crate | `Err(PortRefusal::UnboundedRentSweep)` (`programs/porting/solana/src/monetary.rs:349-353`) | `Err(PortRefusal::UnboundedBalanceSweep)` (`programs/porting/evm/src/monetary.rs:329-333`) | `Err(PortRefusal::SupplyMutation)` (`programs/porting/cosmwasm/src/monetary.rs:346-351`) |
| ABI v2 reference guest | `programs/porting/solana/reference-v2/src/lib.rs:1-9` | `programs/porting/evm/reference-v2/src/lib.rs:1-15` | `programs/porting/cosmwasm/reference-v2/src/lib.rs:1-9` |
| Make lint artifact | `layerx_anchor_context_reference.wasm` (`Makefile:3093`, `Makefile:3096`) | `layerx_evm_context_reference.wasm` (`Makefile:3092`, `Makefile:3095`) | `layerx_cosmwasm_context_reference.wasm` (`Makefile:3094`, `Makefile:3097`) |

[Home](Home.md)
