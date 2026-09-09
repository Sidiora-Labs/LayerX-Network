# LayerX Node Interface v1

[`v1.kvx`](v1.kvx) is the machine-readable LNI v1 contract. This document is
the current human-readable message index and admission contract used by the
client schema checks.

## Version negotiation

NodeInfo is the mandatory first exchange. A client sends `NodeInfoRequest` in
the stable v1.0 bootstrap envelope (major 1, minor 0), then validates the
version returned by `NodeInfoResponse` against the version it implements.
Minor releases within major version 1 are additive. Version 1.3 adds the
`authenticated_durable_submit` capability; its absence keeps the beta write
gate closed while preserving compatible read and legacy capability discovery.
Version 1.4 adds the `simulate` capability: `SimulateRequest` carries one
canonical signed program-call activity, the sequencer executes it against the
current head through the programs runtime without committing anything, and
`SimulateResponse` returns the execution (activity id, sequencer-signed
receipt, terminal payload, call graph) with sequencer-signed simulation
evidence as proof material.
Version 1.5 adds `asset_read`, `fee_estimate`, and event-driven receipt
publication waiting. Clients negotiate minor 5 before using these additions.

## Authenticated durable submission

A server advertising both `submit` and `authenticated_durable_submit` applies
the following contract to every `SubmitRequest`:

1. Decode the canonical signed activity and derive its activity ID.
2. Verify its signature and its signing authority against current state.
3. Insert it into the bounded daemon queue through admission storage in the
   configured persistent checkpoint directory. The admission record is written
   completely and successfully synchronized with `fdatasync` before it is
   treated as durable.
4. Send `SubmitResponse` only after current authorization, durable admission,
   and in-memory queue insertion all succeed. The response echoes the exact
   request payload and carries exactly the 32-byte derived activity ID as proof
   material.

An acknowledged record is recovered after process restart. The checkpoint
directory must reside on storage whose `fdatasync` durability guarantee meets
the deployment's persistence requirement. A retry is re-authorized against
current state and does not create a second queue or journal entry.

Authentication failures return `ErrorResponse` with refusal class 6 followed
by the big-endian native result code. This typed authentication refusal is
terminal for that request, increments the stable SO_PEERCRED peer counter, and
does not change queue occupancy or admission storage. A failure after mutation
can no longer be proven absent closes the connection and fail-stops admission;
it is never reported as a terminal refusal.

The tag-4 wire shape and its general v1 meaning remain an admission
acknowledgement. Capability-aware clients may rely on the stronger
authentication-and-durability guarantee only when
`authenticated_durable_submit` was advertised.

## Message index

| Tag | Message | Kind | Capability |
| ---: | --- | --- | --- |
| 1 | `NodeInfoRequest` | request | `node_info` |
| 2 | `NodeInfoResponse` | response | `node_info` |
| 3 | `SubmitRequest` | request | `submit` |
| 4 | `SubmitResponse` | response | `submit` |
| 5 | `ReceiptLookupRequest` | request | `receipt_lookup` |
| 6 | `ReceiptLookupResponse` | response | `receipt_lookup` |
| 7 | `AccountReadRequest` | request | `account_read` |
| 8 | `AccountReadResponse` | response | `account_read` |
| 9 | `HistoryRangeRequest` | request | `history_range` |
| 10 | `HistoryItem` | stream | `history_range` |
| 11 | `HistoryEnd` | stream | `history_range` |
| 12 | `BatchHeaderRequest` | request | `batch_header` |
| 13 | `BatchHeaderResponse` | response | `batch_header` |
| 14 | `CheckpointRequest` | request | `checkpoint` |
| 15 | `CheckpointResponse` | response | `checkpoint` |
| 16 | `ProofBundleRequest` | request | `proof_bundle` |
| 17 | `ProofBundleResponse` | response | `proof_bundle` |
| 18 | `AvailabilityFetchRequest` | request | `availability_fetch` |
| 19 | `AvailabilityChunk` | stream | `availability_fetch` |
| 20 | `AvailabilityEnd` | stream | `availability_fetch` |
| 21 | `EventSubscribeRequest` | request | `event_subscribe` |
| 22 | `EventRecord` | stream | `event_subscribe` |
| 23 | `EventGap` | stream | `event_subscribe` |
| 24 | `EventHeartbeat` | stream | `event_subscribe` |
| 25 | `ErrorResponse` | response | `node_info` |
| 26 | `PreparationStateRequest` | request | `preparation_state` |
| 27 | `PreparationStateResponse` | response | `preparation_state` |
| 28 | `FinalityEvidenceRegisterRequest` | request | `finality_evidence_register` |
| 29 | `FinalityEvidenceRegisterResponse` | response | `finality_evidence_register` |
| 30 | `SimulateRequest` | request | `simulate` |
| 31 | `SimulateResponse` | response | `simulate` |
| 32 | `AssetReadRequest` | request | `asset_read` |
| 33 | `AssetReadResponse` | response | `asset_read` |
| 34 | `FeeEstimateRequest` | request | `fee_estimate` |
| 35 | `FeeEstimateResponse` | response | `fee_estimate` |

AvailabilityFetchRequest carries only the canonical selector and empty proof material. AvailabilityChunk carries exact chunk bytes and inclusion metadata. AvailabilityEnd has empty canonical payload and empty proof material.

AvailabilityFetchRequest selector `05 || batch:u64be` fetches one durable, sealed, header-signed candidate before finalization. It uses the same authenticated UID/GID principal set as FinalityEvidenceRegisterRequest (tag 28); other principals retain the existing unauthorized refusal. AvailabilityChunk and AvailabilityEnd encoding and all verification checks are unchanged. Selectors 01–04 remain finalized-only. Candidate retrieval does not register or finalize a checkpoint.

## Committed reads in LNI 1.5

All integers are big-endian. `AssetReadRequest` contains version u16 = 1,
kind u8 (list = 1, get = 2), and a nonzero 32-byte asset id for get only.
`AssetReadResponse` contains version u16 = 1, observed sequence u64,
committed state root (32 bytes), count u16, then length-prefixed (u16)
canonical version-3 Asset records sorted by asset id. Get returns exactly
one record. Records preserve issuer, salt, cap, pause and circulating supply.
The bounded list is returned in full or refused.

`FeeEstimateRequest` contains version u16 = 1, activity type u32, canonical
encoded byte count u64, execution units u64 and storage units u64.
`FeeEstimateResponse` contains version u16 = 1, observed sequence u64,
committed state root (32 bytes), parameter version u32, estimated fee u128,
and a u16-length-prefixed canonical fee schedule. Version-2 schedules name
register, account_open, send, receive, grant_issue, grant_revoke, mint and
burn in that order. The supplied meter determines an estimate; execution
still determines the actual charge.

These responses read an authenticated same-process committed snapshot and
carry empty proof material. Their sequence and root identify the observation;
they do not establish a metadata Merkle proof or checkpoint finality.

`AccountReadRequest` kind 3 enumerates the derived DID id32's main and
per-asset accounts at the latest root, with requested verification rank at
most 3. `AccountReadResponse` returns sorted account ids, canonical values
and individual existing account evidence proofs. Clients verify each proof.
Historical enumeration and response overflow are refused without partial
results. Kinds 1 and 2 retain balance and account reads, including per-asset
account ids and the selected balance asset id.

## Event-driven receipt publication wait

At negotiated minor 5, `ReceiptLookupRequest` may append
`wait_publication:u8 = 1` to its canonical activity-id, idempotency-key or
sequence selector. The daemon waits on queue and publication notifications
until the receipt is found, the queue drains, execution fails, or the request
deadline expires. It does not use a fixed polling interval. A successful
lookup returns the canonical signed receipt through `ReceiptLookupResponse`;
an admission acknowledgement is never substituted for an executed receipt.
Legacy selectors remain supported without the trailing wait field.
