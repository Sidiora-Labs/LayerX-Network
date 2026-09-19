# LayerX Node Interface (LNI) v1.7

LNI is the stable node boundary. Protocol bytes stay opaque: LNI does not
invent an activity, receipt, or checkpoint layout
(`agent/schema/lni/v1.kvx`, `spec/.beta/layerx-agent-interface/spec.kvx`
requirement 2).

Schema: major 1, minor 7. Frame:

```text
major:u16be || minor:u16be || message_tag:u16be
|| correlation_id:u64be || canonical_payload:bytes32 || proof_material:bytes32
```

Within major 1, minors add tags, optional trailing fields, and capabilities
only. Transport is length-prefixed frames over Unix sockets by default,
mutual-TLS TCP remotely, or the separately versioned C ABI.

NodeInfo is the first exchange. It carries LNI version, protocol version,
network id, role, chain head, latest sealed batch, latest finalised
checkpoint, authorised sequencer key, and capabilities.

Sources: `agent/schema/lni/v1.kvx`, `agent/schema/lni/README.md`.

---

## Capabilities

`[capabilities].values` in `v1.kvx`:

| Capability | Introduced |
| --- | --- |
| `node_info` | 1.0 |
| `submit` | 1.0 |
| `receipt_lookup` | 1.0 |
| `account_read` | 1.0 |
| `history_range` | 1.0 |
| `batch_header` | 1.0 |
| `checkpoint` | 1.0 |
| `proof_bundle` | 1.0 |
| `availability_fetch` | 1.0 |
| `event_subscribe` | 1.0 |
| `historical_proofs` | 1.0 |
| `preparation_state` | 1.0 |
| `finality_evidence_register` | 1.2 (tags 28–29) |
| `authenticated_durable_submit` | 1.3 |
| `simulate` | 1.4 (tags 30–31) |
| `asset_read` | 1.5 (tags 32–33) |
| `fee_estimate` | 1.5 (tags 34–35) |
| `session_fee_state` | 1.5 (tags 36–37) |
| `program_read` | 1.6 (tags 38–39) |
| `program_head_attest` | 1.7 (tags 40–41) |

`authenticated_durable_submit` means Submit authenticates and `fdatasync`s
the admission record before the acknowledgement. Authentication failures
return `ErrorResponse` class 6 plus the native result code and do not
consume queue capacity (`spec/layerx-beta/spec.kvx` requirement 3).

`simulate` decodes one signed activity, refuses anything other than
`LX_PROGRAMS_CALL`, prepares a one-activity batch against the current head,
and does not commit the durable log, sequence, or occupancy ledger.

`program_read` reuses the signed simulation result shape and binds a
minimum sequence plus an optional canonical state root. Receipt lookup
trailing `wait_publication:u8=1` is 1.5. Activity-id `wait_mode:u8` is 1.6:
0 immediate, 1 published, 2 durable-or-published.

`program_head_attest` names one registered program and a staleness bound. The
authorised signing sequencer answers with its current account-state head, the
program's registered version, code hash and ABI at that head, and an Ed25519
signature over the `LayerX/program-discovery-proof/v1` digest of those facts.
The hosted program registry publishes that signature as
`discovery_public_key`/`discovery_signature`.

---

## Message tags

| Tag | Name | Kind | Capability |
| ---: | --- | --- | --- |
| 1 | NodeInfoRequest | request | `node_info` |
| 2 | NodeInfoResponse | response | `node_info` |
| 3 | SubmitRequest | request | `submit` |
| 4 | SubmitResponse | response | `submit` |
| 5 | ReceiptLookupRequest | request | `receipt_lookup` |
| 6 | ReceiptLookupResponse | response | `receipt_lookup` |
| 7 | AccountReadRequest | request | `account_read` |
| 8 | AccountReadResponse | response | `account_read` |
| 9 | HistoryRangeRequest | request | `history_range` |
| 10 | HistoryItem | stream | `history_range` |
| 11 | HistoryEnd | stream | `history_range` |
| 12 | BatchHeaderRequest | request | `batch_header` |
| 13 | BatchHeaderResponse | response | `batch_header` |
| 14 | CheckpointRequest | request | `checkpoint` |
| 15 | CheckpointResponse | response | `checkpoint` |
| 16 | ProofBundleRequest | request | `proof_bundle` |
| 17 | ProofBundleResponse | response | `proof_bundle` |
| 18 | AvailabilityFetchRequest | request | `availability_fetch` |
| 19 | AvailabilityChunk | stream | `availability_fetch` |
| 20 | AvailabilityEnd | stream | `availability_fetch` |
| 21 | EventSubscribeRequest | request | `event_subscribe` |
| 22 | EventRecord | stream | `event_subscribe` |
| 23 | EventGap | stream | `event_subscribe` |
| 24 | EventHeartbeat | stream | `event_subscribe` |
| 25 | ErrorResponse | response | typed boundary error |
| 26 | PreparationStateRequest | request | `preparation_state` |
| 27 | PreparationStateResponse | response | `preparation_state` |
| 28 | FinalityEvidenceRegisterRequest | request | `finality_evidence_register` |
| 29 | FinalityEvidenceRegisterResponse | response | `finality_evidence_register` |
| 30 | SimulateRequest | request | `simulate` |
| 31 | SimulateResponse | response | `simulate` |
| 32 | AssetReadRequest | request | `asset_read` |
| 33 | AssetReadResponse | response | `asset_read` |
| 34 | FeeEstimateRequest | request | `fee_estimate` |
| 35 | FeeEstimateResponse | response | `fee_estimate` |
| 36 | SessionFeeStateRequest | request | `session_fee_state` |
| 37 | SessionFeeStateResponse | response | `session_fee_state` |
| 38 | ProgramReadRequest | request | `program_read` |
| 39 | ProgramReadResponse | response | `program_read` |
| 40 | ProgramHeadAttestRequest | request | `program_head_attest` |
| 41 | ProgramHeadAttestResponse | response | `program_head_attest` |

Payload semantics for each tag are the `[message.N]` blocks in
`agent/schema/lni/v1.kvx`.

---

## Start here

- [Protocol](Protocol.md)
- [Programs](Programs.md)
- [Agent API](AgentApi.md)
- [Agentd](Agentd.md)
- [Fees](Fees.md)
