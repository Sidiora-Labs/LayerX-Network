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
