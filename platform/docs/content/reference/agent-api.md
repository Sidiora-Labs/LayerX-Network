<!-- Generated from agent/schema/agent-api by platform/docs/build/build_site.py. Do not hand-edit. -->

# Agent API reference

Schema `LayerX Agent API`, contract major `1`, minor `2`, generated from `agent/schema/agent-api`.

The agent contract is spoken by `layerx-agentd` and by direct-node SDK deployments. Requests and responses are canonical maps carrying the contract major and minor version; consensus integers are fixed-width in Rust and decimal strings in the dynamic-language SDKs.

Within one major version a release may only add sections or keys. Removing or changing an existing declaration requires a new major version.

## Operations

| Operation | Module | Request | Response | Required request fields |
|---|---|---|---|---|
| `approval.approve` | `approval` | `ApprovalApproveRequest` | `ApprovalDecision` | `tenant`, `approval_id`, `idempotency_key` |
| `approval.get` | `approval` | `ApprovalGetRequest` | `ApprovalRecord` | `tenant`, `approval_id` |
| `approval.list` | `approval` | `ApprovalListRequest` | `ApprovalPage` | `tenant`, `cursor`, `page_limit` |
| `approval.reject` | `approval` | `ApprovalRejectRequest` | `ApprovalDecision` | `tenant`, `approval_id`, `idempotency_key`, `reason` |
| `agent.register` | `identity` | `AgentRegistration` | `AuthorityResponse<AgentRecord>` | `tenant`, `agent_did`, `authority_ref`, `client`, `policy_version` |
| `budget.create` | `identity` | `object` | `AuthorityResponse<BudgetRecord>` | `tenant`, `agent_did`, `asset`, `limit`, `enforcement`, `expiry` |
| `budget.fund` | `identity` | `object` | `AuthorityResponse<BudgetRecord>` | `tenant`, `agent_did`, `budget_id`, `amount` |
| `budget.list` | `identity` | `object` | `AuthorityResponse<BudgetRecords>` | `tenant`, `agent_did` |
| `budget.reconciliation` | `identity` | `object` | `AuthorityResponse<BudgetReconciliation>` | `tenant`, `agent_did`, `budget_id` |
| `budget.revoke` | `identity` | `object` | `AuthorityResponse<BudgetRecord>` | `tenant`, `agent_did`, `budget_id` |
| `capability.attenuate` | `identity` | `object` | `AuthorityResponse<CapabilityRecord>` | `tenant`, `agent_did`, `parent_id`, `dimensions` |
| `capability.create` | `identity` | `object` | `AuthorityResponse<CapabilityRecord>` | `tenant`, `agent_did`, `dimensions` |
| `capability.list` | `identity` | `object` | `AuthorityResponse<CapabilityRecords>` | `tenant`, `agent_did` |
| `capability.revoke` | `identity` | `object` | `AuthorityResponse<CapabilityRecord>` | `tenant`, `agent_did`, `capability_id` |
| `faucet.claim` | `identity` | `object` | `FaucetGrant` | `did`, `signer_public_key` |
| `session.close` | `identity` | `SessionClose` | `AuthorityResponse<SessionRecord>` | `session_id`, `context` |
| `session.list` | `identity` | `SessionList` | `AuthorityResponse<SessionRecords>` | `context` |
| `session.open` | `identity` | `SessionOpen` | `AuthorityResponse<SessionRecord>` | `context` |
| `session.refresh` | `identity` | `SessionRefresh` | `AuthorityResponse<SessionRecord>` | `session_id`, `context` |
| `program.activity` | `programs` | `ProgramActivitySelector` | `ProgramSubmission` | - |
| `program.call` | `programs` | `ProgramCallRequest` | `ProgramSubmission` | `idempotency_key` |
| `program.deploy` | `programs` | `NativeProgramDeployRequest` | `ProgramLifecycleResponse` | `idempotency_key` |
| `program.discover` | `programs` | `ProgramSelector` | `VerifiedProgramDiscovery` | - |
| `program.interface` | `programs` | `ProgramSelector` | `VerifiedProgramInterface` | - |
| `program.receipt` | `programs` | `ProgramReceiptSelector` | `ProgramReceiptResponse` | - |
| `program.simulate` | `programs` | `ProgramCallRequest` | `ProgramSimulation` | - |
| `program.upgrade` | `programs` | `NativeProgramUpgradeRequest` | `ProgramLifecycleResponse` | `idempotency_key` |
| `program.wind-down` | `programs` | `NativeProgramWindDownRequest` | `ProgramLifecycleResponse` | `idempotency_key` |
| `availability.fetch` | `read` | `object` | `VerifiedRead<AvailabilityReport>` | - |
| `export.offline` | `read` | `object` | `VerifiedRead<OfflineExport>` | - |
| `project` | `read` | `object` | `ProjectionResult` | - |
| `read.account` | `read` | `object` | `VerifiedRead<AccountValue>` | - |
| `read.balance` | `read` | `object` | `VerifiedRead<BalanceValue>` | - |
| `read.batch` | `read` | `object` | `VerifiedRead<BatchValue>` | - |
| `read.checkpoint` | `read` | `object` | `VerifiedRead<CheckpointValue>` | - |
| `read.history` | `read` | `object` | `VerifiedRead<HistoryValue>` | - |
| `read.module_state` | `read` | `object` | `VerifiedRead<ModuleStateValue>` | - |
| `read.proof_bundle` | `read` | `object` | `VerifiedRead<ProofBundle>` | - |
| `subscription.acknowledge` | `stream` | `object` | `object` | `scope`, `subscription_id`, `cursor` |
| `subscription.create` | `stream` | `object` | `object` | `scope`, `filter`, `start`, `delivery_target` |
| `subscription.delete` | `stream` | `object` | `object` | `scope`, `subscription_id` |
| `subscription.health` | `stream` | `object` | `object` | `scope`, `subscription_id` |
| `subscription.list` | `stream` | `object` | `object` | `scope` |
| `subscription.pause` | `stream` | `object` | `object` | `scope`, `subscription_id` |
| `subscription.resume` | `stream` | `object` | `object` | `scope`, `subscription_id` |
| `prepare` | `write` | `PrepareRequest` | `Prepared` | `actor`, `authority`, `account_sequence`, `timestamp_bound`, `idempotency_key`, `fee_limit`, `payload`, `payload_hash` |
| `sign` | `write` | `SignRequest` | `Signed` | `preparation_ref`, `signature` |
| `submit` | `write` | `SubmitRequest` | `TrackedSubmission` | `preparation_ref`, `signature` |
| `track` | `write` | `TrackRequest` | `TrackedSubmission` | `submission_ref` |
| `wait` | `write` | `WaitRequest` | `WaitResult` | `submission_ref`, `requested_verification_level`, `deadline` |

## Declared types

### Module `approval`

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `ApprovalDecisionOutcome` | type | variants: `Granted`, `Rejected`, `Expired`, `Defective`, `AlreadyDecided`, `Conflict` |
| `ApprovalLifecycleEvent` | type | variants: `Created`, `Granted`, `Rejected`, `Expired`, `Defective`<br>required: `event_id`, `tenant`, `approval_id`, `kind`, `at`, `record_digest` |
| `ApprovalRecord` | type | required: `approval_id`, `tenant`, `held_activity`, `canonical_bytes_digest`, `hold_reason`, `created_at`, `expires_at`, `state` |
| `ApprovalState` | type | variants: `Held`, `Granted`, `Rejected`, `Expired`, `Defective` |
| `HoldReason` | type | required: `code`, `message` |
| `StructuredActivityDisclosure` | type | required: `canonical_digest`, `activity_type`, `actor`, `authority`, `counterparties`, `amounts`, `asset`, `fee_limit`, `expiry`, `idempotency_key` |

### Module `errors`

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `ApiError` | type | required: `class`, `protocol_result_code`, `retriability`, `request_id`, `reason` |
| `ErrorClass` | type | variants: `TransportFailure`, `Deadline`, `ProtocolIncompatibility`, `UnavailableCapability`, `CoreRejection`, `VerificationFailure`, `PolicyRefusal`, `CapabilityRefusal`, `BudgetRefusal`, `RateLimit`, `IdempotencyConflict`, `InternalFault` |
| `IdempotentMutation` | type | required: `request_id`, `key`, `body_digest`, `operation` |
| `Level` | type | variants: `Unverified`, `SequencerSigned`, `BatchIncluded`, `StateProven`, `CheckpointFinalised`, `SettlementAnchored` |
| `Retriability` | type | variants: `Terminal`, `Retriable` |
| `VerificationStatus` | type | variants: `Achieved`, `Unverified` |

### Module `identity`

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `AuthorityResponse` | type | required: `authority`, `value` |
| `BudgetEnforcement` | type | variants: `ProtocolBudget`, `DaemonLimit` |
| `CapabilityDimensions` | type | required: `activity_types`, `counterparties`, `assets`, `amount_ceilings`, `rate_ceilings`, `purpose_constraints`, `expiry` |
| `FaucetGrant` | type | required: `funded`, `funding_id`, `transaction_id`, `amount`, `network` |
| `SessionContext` | type | required: `tenant`, `agent_did`, `authority_ref`, `permitted_activity_types`, `expiry`, `client`, `policy_version` |

### Module `programs`

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `NativeProgramDeploy` | type | required: `program_id`, `guest_abi`, `policy`, `authority`, `new_hash`, `wasm`<br>optional: `interface` |
| `NativeProgramDeployRequest` | type | required: `signed_activity` |
| `NativeProgramUpgrade` | type | required: `program_id`, `guest_abi`, `old_hash`, `new_hash`, `migration_hook`, `clear_interface`, `wasm`<br>optional: `interface` |
| `NativeProgramUpgradeRequest` | type | required: `signed_activity` |
| `NativeProgramWindDown` | type | variants: `route`, `deprecate`, `tombstone`, `exit`<br>required: `program_id`, `operation` |
| `NativeProgramWindDownRequest` | type | required: `signed_activity` |
| `ProgramActivitySelector` | type | required: `activity_id`, `requested_verification_level` |
| `ProgramCallBudget` | type | required: `fuel`, `fee_limit` |
| `ProgramCallRequest` | type | required: `program_id`, `calldata`, `budget`, `capabilities`, `signed_activity` |
| `ProgramCapability` | type | variants: `storage_read`, `storage_write`, `transfer`, `emit_event`, `compose` |
| `ProgramExecutionEvidence` | type | required: `activity_id`, `receipt`, `terminal_payload`, `call_graph`, `authority` |
| `ProgramFailure` | type | variants: `unknown_program`, `reentrancy`, `depth_exceeded`, `fanout_exceeded`, `guest_refused`, `authority`, `resource`, `response`, `fault` |
| `ProgramLifecycleError` | type | required: `code`, `retry`<br>optional: `retry_after_seconds` |
| `ProgramLifecycleErrorEnvelope` | type | required: `error` |
| `ProgramLifecycleReceipt` | type | required: `activity_id`, `receipt` |
| `ProgramLifecycleReceiptEnvelope` | type | required: `result` |
| `ProgramLifecycleResponse` | type | variants: `ProgramLifecycleResultEnvelope`, `ProgramLifecycleUnknown`, `ProgramLifecycleErrorEnvelope` |
| `ProgramLifecycleResult` | type | required: `state`, `activity_id`, `receipt`, `terminal_payload`, `call_graph` |
| `ProgramLifecycleResultEnvelope` | type | required: `result` |
| `ProgramLifecycleUnknown` | type | required: `state`, `activity_id`, `retry`, `retry_after_seconds` |
| `ProgramOutcome` | type | variants: `completed`, `legacy_completed`, `refused` |
| `ProgramReceiptResponse` | type | variants: `ProgramSubmission`, `ProgramLifecycleReceiptEnvelope` |
| `ProgramReceiptSelector` | type | required: `idempotency_key`, `expected_activity_id`, `requested_verification_level` |
| `ProgramSelector` | type | required: `program_id`, `requested_verification_level` |
| `ProgramSimulation` | type | required: `committed`, `execution`, `simulation_evidence` |
| `ProgramSource` | type | required: `status:string`<br>optional: `source_digest:string`, `environment_digest:string`, `pipeline:string`, `expected_code_hash:string`, `reproduced_artifact_digest:string` |
| `ProgramSubmission` | type | required: `state`, `activity_id`, `idempotency_key` |
| `VerifiedProgramDiscovery` | type | required: `program_id`, `lifecycle`, `version`, `code_hash`, `abi_version`, `receipt_digest`, `state_root`, `observed_sequence`, `observed_at`, `valid_through`, `verification`<br>optional: `deployment_receipt_digest`, `discovery_public_key`, `discovery_signature` |
| `VerifiedProgramInterface` | type | required: `program_id`, `version`, `code_hash`, `abi_version`, `interface`, `interface_digest`, `receipt_digest`, `state_root`, `observed_sequence`, `observed_at`, `valid_through`, `source`, `verification` |

### Module `read`

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `Freshness` | type | required: `chain_head`, `latest_sealed_batch`, `latest_finalised_checkpoint`, `value_sequence`, `relative_to` |
| `VerifiedRead` | type | required: `value`, `achieved_verification_level`, `freshness` |

### Module `stream`

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `Delivery` | type | variants: `Event`, `Gap`, `Truncated` |
| `EventDelivery` | type | required: `event_identity`, `event_bytes`, `deduplication_id`, `cursor`, `receipt_reference` |
| `GapNotice` | type | required: `missing_first`, `missing_last`, `backfill_cursor`, `backfill_attempted` |
| `ReceiptReference` | type | variants: `None`, `Verified` |
| `SubscriptionFilter` | type | required: `agents`, `accounts`, `activity_types`, `modules`, `assets`, `counterparties`, `result_classes` |
| `SubscriptionScope` | type | required: `tenant`, `agent`, `capability` |
| `TruncationNotice` | type | required: `requested_first`, `oldest_available`, `resume_cursor` |

### Module `v1`

| Declaration | Kind | Shape |
|---|---|---|
| `ContractVersion` | record | fields: `major:u16`, `minor:u16` |
| `VersionRequest` | record | fields: `request_id:Sequence`, `supported:ContractVersion` |
| `VersionResponse` | record | fields: `request_id:Sequence`, `contract:ContractVersion`, `node_interface_major:u16` |
| `Amount` | scalar | wire: `decimal_string`<br>rust: `u128`<br>typescript: `bigint`<br>python: `int` |
| `BudgetLimit` | scalar | wire: `decimal_string`<br>rust: `u128`<br>typescript: `bigint`<br>python: `int` |
| `Sequence` | scalar | wire: `decimal_string`<br>rust: `u64`<br>typescript: `bigint`<br>python: `int` |
| `TimestampSeconds` | scalar | wire: `decimal_string`<br>rust: `u64`<br>typescript: `bigint`<br>python: `int` |
| `SettlementDomain` | type | variants: `Paxeer` |

### Module `write`

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `Disclosure` | type | required: `canonical_digest`, `activity_type`, `actor`, `authority`, `counterparties`, `amounts`, `asset`, `fee_limit`, `expiry`, `idempotency_key` |
| `SubmissionState` | type | variants: `Prepared`, `Signed`, `Queued`, `Submitted`, `Acknowledged`, `Unknown`, `Executed`, `Failed`, `Expired` |
| `Transition` | type | required: `from`, `to`, `cause`, `at` |
