# SDK terminal verification

A Programs CALL receipt carries one terminal: the `terminal_payload` bytes
whose SHA-256 is `terminal_payload_root` on the program outcome
(`agent/sdk/python/layerx_sdk/program_wire.py:173-174`,
`include/layerx/lxp_receipt.h:69-70`). Kernel terminal kinds are
`LXP_PROGRAM_TERMINAL_SUCCESS = 1`, `FAILURE = 2`, `RESOURCE = 3`
(`include/layerx/lxp_receipt.h:34-38`). The outcome fields describe that
one runtime terminal; they are not a second receipt
(`include/layerx/lxp_receipt.h:43-44`).

Python returns `DecodedProgramTerminal` with `outcome`, `usage`, and
`transfer_verification` of `"reconstructed"` or
`"recorded_terminal_root_not_locally_reconstructable"`
(`agent/sdk/python/layerx_sdk/program_wire.py:54-57, 285-286`). The
reference entry point is `decode_and_verify_program_terminal`
(`agent/sdk/python/layerx_sdk/program_wire.py:164-170`). Shared vectors
are verified after `verify_receipt_outcome` on protocol 3
(`platform/sdk/conformance/terminal-v4.test.py:25-33`).

---

## Check order

`decode_and_verify_program_terminal` runs these steps in order.

### Framing and payload digest binding

1. Refuse an empty `call_graph` or a graph whose SHA-256 is not
   `receipt.call_graph_root` (`program_wire.py:171-172`).
2. Refuse an empty `terminal_payload`, a payload longer than
   `1_048_576` bytes, or a payload whose SHA-256 is not
   `receipt.terminal_payload_root` (`program_wire.py:173-174`).
3. When `encoding_version == 4`, require the domain
   `LXP/programs/terminal-applied-legs/v1\0`, a nonempty inner detail
   of at most `1_048_576` bytes, an applied-leg span of at most
   `256 * 115` bytes, and no trailing bytes after that wrapper
   (`program_wire.py:176-185, 804-805`).
4. When `encoding_version != 4`, leave `inner` equal to the full
   payload (`program_wire.py:175-176`;
   `platform/sdk/go/programs.go:2124-2126`).

A truncated or trailing-byte `executed-v4` payload fails step 2 because
the SHA-256 no longer matches `terminal_payload_root`
(`program_wire.py:173-174`;
`platform/sdk/conformance/terminal-v4.test.py:34-39`).

### Signature and digest binding

Shared tests bind the signed receipt before the terminal decoder
(`platform/sdk/conformance/terminal-v4.test.py:25-27`):

- Shared tests pass `protocol_version=3`. The verifier refuses unless
  that argument is 2 or 3 and equals `receipt.protocol_version`
  (`agent/sdk/python/layerx_sdk/verifier.py:889-890`;
  `terminal-v4.test.py:25`).
- The sequencer signs SHA-256 of domain `LXP/v1/receipt\0` plus the
  unsigned receipt (`verifier.py:22, 66-71, 930-936`).
- `receipt_digest` must equal the fixture digest
  (`terminal-v4.test.py:26`).
- `activity_id` must equal SHA-256 of `LXP/v1/activity-id\0` plus the
  signed activity (`program_wire.py:12`; `terminal-v4.test.py:27`).

Encoding 4 is wire tag `0x50524734`. It appends a 32-byte
`applied_legs_digest` on the program outcome and is admitted only on
protocol 3 (`verifier.py:614-615, 648, 668`). Encoding 4 requires a
nonzero `applied_legs_digest`; encodings 1–3 require it all-zero
(`verifier.py:670`). Those outcome bytes sit in the unsigned receipt
hashed for the sequencer signature (`verifier.py:857, 930-936`).

Encoding tags 1–3 are `PROGRAM_OUTCOME_TAGS`
(`agent/sdk/python/layerx_sdk/generated/receipt.py:6`;
`verifier.py:32, 610-619`). Protocol 1 admits encodings 1 and 3;
protocols 2 and 3 admit encodings 2 and 3; protocol 3 also admits
encoding 4 (`verifier.py:665-668`).

### Applied-leg reconstruction

On encoding 4, after the wrapper parses, SHA-256 of the raw leg bytes
must equal `receipt.applied_legs_digest` (`program_wire.py:186-187`).
`_verify_applied_legs` then reconstructs the transfer root
(`program_wire.py:289-298`):

- Length is a multiple of 115 and at most `256 * 115`
  (`program_wire.py:290-291`).
- Each 115-byte leaf is reserved `0`, 32-byte source, 32-byte
  destination, 32-byte asset, 16-byte amount, kind `0x00 0x01`; every
  address and the amount are nonzero (`program_wire.py:293-296`).
- Merkle root over domain `LXP/v1/merkle-leaf\0` / `LXP/v1/merkle-internal\0`
  must equal `receipt.transfer_root`. An empty list is the 32-byte zero
  root (`program_wire.py:34-35, 297-298, 743-749`).

Empty legs against a zero root pass; empty legs against a nonzero root
fail (`terminal-v4.test.py:41-44`).

### Historical selection

After the encoding-4 wrapper (or the skip), unwrap at most one
authority wrapper `LXP/program-execution-with-transfer-authority/v2\0`
then at most one occupancy wrapper
`LXP/program-execution-with-occupancy/v1\0`. A second authority or
occupancy prefix is `program terminal wrapper order`
(`program_wire.py:18-19, 189-204`).

Inner domain then selects the terminal body
(`program_wire.py:15-17, 20-23, 209-253`):

| Prefix | Requirement | Outcome kind |
| --- | --- | --- |
| `LXP/program-execution/v2\0` or `/v3\0` | `terminal_kind == 1` and `abi_version == 1` | `legacy_completed` |
| `LXP/program-execution/v4\0` | `terminal_kind` matches, `abi_version == 2`, program id matches, embedded graph equals `call_graph` | `completed` / `refused` |
| `LXP/programs/failure-detail/v1\0` | `terminal_kind == 2` | `refused` / `guest_refused` |
| `LXP/programs/resource-detail/v1\0` | `terminal_kind == 3` | `refused` / `resource` |
| `LXP/programs/settlement-failure/v1\0` | `terminal_kind == 2`, length domain+1, last byte in `1..12` | `refused` / `guest_refused` |
| `LXP/programs/callback-failure/v1\0` | `terminal_kind == 2`, length domain+5 | `refused` / `guest_refused` |

Occupancy is required iff `protocol_version in (2, 3)` and the terminal
is successful (`program_wire.py:255-257`). Transfer authority is
required for a candidate (`/v4`) body, or for encoding 4 on a
successful terminal, unless the historical recorded path applies
(`program_wire.py:272-276`). The Python and TypeScript decoders on the testnet
branch require encoding-4 authority bytes to start directly with
`LayerX/programs/402LXP/transfer-set/v2\0`
(`agent/sdk/python/layerx_sdk/program_wire.py:24-25, 278-281`;
`agent/sdk/typescript/src/program-wire.ts:18-19, 240-247`).
`protocol_version` must be 1, 2, or 3 (`program_wire.py:283-284`).

The runtime and the Go, JVM, Swift, and .NET SDKs are documented as also
accepting `LayerX/programs/402LXP/account-bound-set/v1\0`. That wrapper tag
appears nowhere in this tree, so nothing here implements or rejects it yet. That wrapper contains
the u32 length and bytes of the original transfer set, followed by one
u16-length-prefixed canonical account name per leg. Verification refuses a
nested wrapper or trailing bytes, recomputes principal/program-funding source
accounts from those names, requires an empty name for a program-account debit,
rebuilds the 115-byte applied legs, and compares their Merkle root with the
receipt transfer root
(`programs/crates/layerx-programs-runtime/src/transfer.rs:882-955`;
`platform/sdk/go/programs.go:1212-1275`;
`platform/sdk/jvm/src/main/java/com/sidiora/layerx/sdk/ProgramsClient.java:1034-1072`;
`platform/sdk/swift/Sources/LayerXSDK/Programs.swift:730-769`;
`platform/sdk/dotnet/Programs.cs:727-766`). The original set inside the wrapper
retains the signer principal and invocation authority; the account names bind
the actual debit endpoints.

`recorded` is true when `encoding_version != 4`, no authority wrapper
is present, and `transfer_root` is nonzero. That path returns
`"recorded_terminal_root_not_locally_reconstructable"` without
rebuilding legs from the terminal. Every other accepted path returns
`"reconstructed"` (`program_wire.py:272-273, 285-286`).

---

## Refusals

Python raises `ValueError("invalid {boundary}")`
(`program_wire.py:772-773`). The terminal decoder and applied-leg
checker use these boundaries:

| Boundary | Input |
| --- | --- |
| `program call graph root` | empty graph, or SHA-256 ≠ `call_graph_root` (`program_wire.py:171-172`) |
| `program terminal root` | empty payload, payload longer than `1_048_576`, or SHA-256 ≠ `terminal_payload_root` (`program_wire.py:173-174`) |
| `applied terminal domain` | encoding 4 payload that does not start with `LXP/programs/terminal-applied-legs/v1\0` (`program_wire.py:176-180`) |
| `empty applied terminal detail` | encoding 4 inner length 0 (`program_wire.py:182-183`) |
| `applied legs digest` | SHA-256 of the leg span ≠ `applied_legs_digest` (`program_wire.py:186-187`) |
| `trailing canonical bytes` | leftover bytes after the applied-leg, authority, or occupancy wrapper (`program_wire.py:185, 197, 202, 804-805`) |
| `applied legs bounds` | leg span length not a multiple of 115, or greater than `256 * 115` (`program_wire.py:290-291`) |
| `applied leg canonical fields` | reserved byte, kind, or a zero source, destination, asset, or amount (`program_wire.py:293-296`) |
| `applied transfer root` | reconstructed Merkle root ≠ `transfer_root` (`program_wire.py:297-298`) |
| `program terminal wrapper order` | authority or occupancy prefix after those wrappers were already consumed (`program_wire.py:203-204`) |
| `legacy terminal kind` | `/v2` or `/v3` body with `terminal_kind != 1` or `abi_version != 1` (`program_wire.py:209-211`) |
| `candidate terminal binding` | `/v4` body whose kind, ABI 2, or program id does not match the receipt (`program_wire.py:221-222`) |
| `candidate call graph` | `/v4` embedded graph ≠ supplied `call_graph` (`program_wire.py:224-225`) |
| `failure terminal kind` | failure-detail body with `terminal_kind != 2` (`program_wire.py:235-236`) |
| `resource terminal kind` | resource-detail body with `terminal_kind != 3` (`program_wire.py:240-241`) |
| `settlement terminal` | settlement body with `terminal_kind != 2`, wrong length, or code outside `1..12` (`program_wire.py:245-246`) |
| `callback terminal` | callback body with `terminal_kind != 2` or length ≠ domain+5 (`program_wire.py:249-250`) |
| `unknown terminal domain` | inner prefix not one of the domains above (`program_wire.py:252-253`) |
| `occupancy attachment presence` | occupancy wrapper present XOR not (`protocol 2 or 3` and success) (`program_wire.py:255-257`) |
| `empty occupancy attachment` | empty occupancy bytes with a nonzero occupancy commitment (`program_wire.py:259-261`) |
| `occupancy evidence digest` | SHA-256 of occupancy bytes ≠ `occupancy_evidence_digest` (`program_wire.py:263-264`) |
| `occupancy receipt binding` | occupancy usage or occupancy transfer root ≠ receipt (`program_wire.py:266-269`) |
| `unexpected occupancy commitment` | no occupancy wrapper, but receipt occupancy fields are nonzero (`program_wire.py:270-271`) |
| `transfer authority presence` | authority wrapper presence disagrees with `transfer_root` outside the recorded path (`program_wire.py:272-276`) |
| `transfer authority root` | empty authority bytes, or wrapper root ≠ `transfer_root` (`program_wire.py:278-279`) |
| `V2 transfer authority required` | encoding 4 authority that does not start with transfer-set v2 (`program_wire.py:280-281`) |
| `account-bound transfer authority` | nested wrapper, malformed original-set length, missing/extra account names, invalid canonical account name, nonempty program-debit name, trailing bytes, or rebuilt root mismatch (`programs/crates/layerx-programs-runtime/src/transfer.rs:882-955`) |
| `program receipt protocol` | `protocol_version` not in `(1, 2, 3)` (`program_wire.py:283-284`) |
| `terminal receipt metadata` | runtime, ABI, fee schedule, metering, or usage disagrees with the receipt (`program_wire.py:484-486`) |

Shared-vector refusals:

- `mutated-leg-v4` raises `applied transfer root`
  (`terminal-v4.test.py:29-31`). The fixture retains the original
  transfer root after toggling an applied amount byte and re-signing
  with recomputed applied and terminal digests
  (`platform/sdk/conformance/fixtures/receipt-programs-mutated-leg-v4.json:20`).
- Every proper prefix of `executed-v4` `terminal_payload` raises
  `ValueError` (`terminal-v4.test.py:34-37`).
- `executed-v4` payload plus a trailing `0x00` raises `ValueError`
  (`terminal-v4.test.py:38-39`).
- Empty applied legs against a nonzero root raise `ValueError`
  (`terminal-v4.test.py:41-44`).

Go unwrap reports `"Programs applied transfer root mismatch"`
(`platform/sdk/go/programs.go:2160-2161`). JVM
`unwrapAppliedTerminal` throws `IllegalArgumentException("applied transfer root")`
(`platform/sdk/jvm/src/main/java/com/sidiora/layerx/sdk/ProgramsClient.java:1422`);
`verifyTerminal` wraps that as `PlatformSdkException`
(`ProgramsClient.java:786-789`). .NET `VerifyAppliedLegs` throws
`InvalidDataException("applied transfer root")`
(`platform/sdk/dotnet/Programs.cs:975`). TypeScript rejects with
`/applied transfer root/`
(`agent/sdk/typescript/test/program-executed-v3.test.ts:51`).

---

## Stored V3 without regeneration

Encoding 3 is selected by the outcome tag, not by rewriting the
terminal. `receipt-programs-executed-v3.json` stores
`program_outcome_encoding_version` 3
(`platform/sdk/conformance/fixtures/receipt-programs-executed-v3.json:35`).
The packager verifies and decodes the original evidence; it does not
construct outcomes, change receipt fields, or sign receipts
(`receipt-programs-executed-v3.json:5-6`;
`platform/sdk/conformance/fixtures/generate_executed_program_fixture.py:55-72, 114-117`).

On encoding 3 the applied-leg wrapper is not consumed: Go returns the
payload unchanged (`platform/sdk/go/programs.go:2125-2126`); Python
skips the encoding-4 block (`program_wire.py:175-176`). The V3 vector
has no authority wrapper and a nonzero `transfer_root`, so `recorded`
is true and `transfer_verification` is
`"recorded_terminal_root_not_locally_reconstructable"`
(`program_wire.py:272-273, 285-286`;
`terminal-v4.test.py:19, 33`). Legs are not rebuilt from the terminal.

Encoding 1 uses the same skip: it is not encoding 4
(`verifier.py:666`; `program_wire.py:176`;
`programs.go:2125-2126`). Protocol 1 admits encodings 1 and 3
(`verifier.py:666`). The shared four-vector test does not load a V1
receipt; it loads `executed-v3` for the historical recorded path
(`terminal-v4.test.py:18-21`).

---

## Shared fixtures

The Python/TypeScript conformance tests load
`platform/sdk/conformance/fixtures/receipt-programs-{name}.json` for
`executed-v4`, `principal-v4`, `mutated-leg-v4`, and `executed-v3`
(`terminal-v4.test.py:18-21`). The Go, JVM, Swift, and .NET receipt tests are
documented as additionally loading
`programs/fixtures/pay5/receipt-account-bound-v4.json`, which is not in this
tree. Each V4 file carries
`canonical_receipt_hex`, `signed_activity_hex`, `program_id_hex`,
`receipt_digest_hex`, `terminal_payload_hex`, `call_graph_hex`,
`authorized_batch`, and `provenance`
(`receipt-programs-executed-v4.json:2-17`).

| File | What it proves |
| --- | --- |
| `receipt-programs-executed-v4.json` | Encoding 4 CALL with reconstructed applied legs. `transfer_verification == "reconstructed"`. Truncation of every prefix and a trailing byte are refused (`terminal-v4.test.py:18, 33-39`; provenance generator `--dump-executed-v4`, mutation `none` at `receipt-programs-executed-v4.json:18-20`). |
| `receipt-programs-principal-v4.json` | Encoding 4 CALL whose inner body is the ABI-1 legacy path (`legacy_completed` in JVM/Swift/.NET; Python `/v2` or `/v3` requires `abi_version == 1`). `transfer_verification == "reconstructed"` (`terminal-v4.test.py:18, 33`; `platform/sdk/jvm/src/test/java/com/sidiora/layerx/sdk/TerminalV4Test.java:34-36`; `program_wire.py:209-217`; provenance `--dump-principal-v4` at `receipt-programs-principal-v4.json:18-20`). |
| `receipt-programs-mutated-leg-v4.json` | Encoding 4 CALL whose applied amount byte was toggled after execution; applied and terminal digests were recomputed and the receipt signed again; `transfer_root` retained. Decoder refuses `applied transfer root` (`receipt-programs-mutated-leg-v4.json:18-20`; `terminal-v4.test.py:29-31`). |
| `receipt-programs-executed-v3.json` | Stored encoding 3 CALL, ABI 2, module 9 version 4, operation 3. Verified without regeneration. `transfer_verification == "recorded_terminal_root_not_locally_reconstructable"` (`receipt-programs-executed-v3.json:2-6, 26-28, 35, 37`; `terminal-v4.test.py:19, 33`). |
| `programs/fixtures/pay5/receipt-account-bound-v4.json` | Encoding 4 CALL with a native per-Asset account name bound into transfer authority. The four platform SDK test suites accept the source vector, then require reconstructed transfer verification (`platform/sdk/go/terminal_v4_test.go:12-18, 83-85`; `platform/sdk/jvm/src/test/java/com/sidiora/layerx/sdk/TerminalV4Test.java:30-31, 54-56`; `platform/sdk/swift/Tests/LayerXSDKTests/ReceiptFixtureTests.swift:24-25, 56-58`; `platform/sdk/dotnet/tests/LayerX.Sdk.Tests/ReceiptFixtureTests.cs:40-42, 67-69`). |

---

## SDK entry points

| SDK | Function | Test |
| --- | --- | --- |
| Python | `decode_and_verify_program_terminal` (`agent/sdk/python/layerx_sdk/program_wire.py:164`) | `platform/sdk/conformance/terminal-v4.test.py` (`TerminalV4.test_signed_shared_vectors` at `:17`) |
| TypeScript | `decodeAndVerifyProgramTerminal` (`agent/sdk/typescript/src/program-wire.ts:135`) | `agent/sdk/typescript/test/program-executed-v3.test.ts` (four-vector loop at `:36-56`) |
| Go | `verifyProgramTerminal` (`platform/sdk/go/programs.go:479`) | `platform/sdk/go/terminal_v4_test.go` (`TestSignedTerminalV4Vectors` at `:12`) |
| JVM | `ProgramsClient.verifyTerminal` (`platform/sdk/jvm/src/main/java/com/sidiora/layerx/sdk/ProgramsClient.java:652`) | `platform/sdk/jvm/src/test/java/com/sidiora/layerx/sdk/TerminalV4Test.java` (`signedSharedVectors` at `:19`) |
| Swift | `verifyTerminal` (`platform/sdk/swift/Sources/LayerXSDK/Programs.swift:556`) | `platform/sdk/swift/Tests/LayerXSDKTests/ReceiptFixtureTests.swift` (`testSignedTerminalV4Vectors` at `:7`) |
| .NET | `ProgramsClient.VerifyTerminal` (`platform/sdk/dotnet/Programs.cs:546`) | `platform/sdk/dotnet/tests/LayerX.Sdk.Tests/ReceiptFixtureTests.cs` (`SignedTerminalV4Vectors` at `:14`) |

Go also unwraps through `unwrapAppliedProgramTerminal` before
`decodeProgramTerminal` (`programs.go:491, 2124`;
`terminal_v4_test.go:48`). JVM, Swift, and .NET unwrap through
`unwrapAppliedTerminal` / `UnwrapAppliedTerminal` then match a
document outcome (`ProgramsClient.java:657`;
`Programs.swift:563`; `Programs.cs:553`). Python and TypeScript decode
the outcome from the terminal bytes (`program_wire.py:209-253`;
`program-wire.ts:178-219`).

Status strings match Go `ProgramTransfersReconstructed` /
`ProgramTransfersRecorded` (`platform/sdk/go/programs.go:287-288, 533-536`).

---

## Disagreements left intact

1. C `lxp_program_outcome` has no `applied_legs_digest` field
   (`include/layerx/lxp_receipt.h:45-72`). Python encoding 4 reads that
   digest from tag `0x50524734` (`verifier.py:614-615, 648`).
2. Python `_fail` raises `ValueError` (`program_wire.py:772-773`). JVM
   `verifyTerminal` wraps into `PlatformSdkException`
   (`ProgramsClient.java:786-789`). .NET catches and throws the SDK
   verification error (`Programs.cs:643`). Swift throws
   `programVerification()` (`Programs.swift:649`). Go returns `error`
   strings (`programs.go:488-531`).
3. Python occupancy uses the `protocol_version` argument
   (`program_wire.py:169, 255`). Go occupancy uses
   `receipt.ProtocolVersion` (`programs.go:502`).
4. The account-bound transfer-authority wrapper is not recognized by any SDK
   in this tree. Python and TypeScript require direct `transfer-set/v2`
   authority bytes, and no account-bound fixture exists in the shared-vector
   loop.

Sources:

- `include/layerx/lxp_receipt.h:34-72`
- `agent/sdk/python/layerx_sdk/program_wire.py:12-19, 54-57, 164-298, 484-486, 743-749, 772-773, 804-805`
- `agent/sdk/python/layerx_sdk/verifier.py:22, 32, 66-71, 279-302, 610-716, 857, 877-942`
- `agent/sdk/python/layerx_sdk/generated/receipt.py:6`
- `agent/sdk/typescript/src/program-wire.ts:135-155, 178-219`
- `platform/sdk/go/programs.go:287-288, 479-536, 2124-2163`
- `platform/sdk/jvm/src/main/java/com/sidiora/layerx/sdk/ProgramsClient.java:652-657, 785-789, 1405-1423`
- `platform/sdk/swift/Sources/LayerXSDK/Programs.swift:556-649, 962-972`
- `platform/sdk/dotnet/Programs.cs:546-643, 950-975`
- `platform/sdk/conformance/terminal-v4.test.py:16-44`
- `platform/sdk/conformance/fixtures/receipt-programs-executed-v4.json:2-20`
- `platform/sdk/conformance/fixtures/receipt-programs-principal-v4.json:18-20`
- `platform/sdk/conformance/fixtures/receipt-programs-mutated-leg-v4.json:18-20`
- `platform/sdk/conformance/fixtures/receipt-programs-executed-v3.json:2-6, 26-28, 35, 37`
- `platform/sdk/conformance/fixtures/generate_executed_program_fixture.py:55-72, 114-117`
- `programs/crates/layerx-programs-runtime/src/transfer.rs:882-955`
- `programs/fixtures/pay5/receipt-account-bound-v4.json`
- `programs/fixtures/pay5/account-authorization-vectors.json`

[Home](Home.md)
