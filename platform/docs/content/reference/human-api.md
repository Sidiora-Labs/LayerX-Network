<!-- Generated from human/schema/human-api by platform/docs/build/build_site.py. Do not hand-edit. -->

# Human API reference

Schema `LayerX Human API`, contract major `1`, minor `2`, generated from `human/schema/human-api`.

Transport is HTTPS with JSON bodies under the `/v1` base path. Every amount is a decimal string of base units and always travels with its currency code. Every mutation that can move money requires the `Idempotency-Key` header, and repeating the request returns the original journey rather than a second effect.

Additive only within a major version: a release may only add sections, keys, list entries or optional fields. Removing or changing an existing declaration requires a new major version.

## Operations

| Operation | Method | Path | Request | Response | Idempotency-Key |
|---|---|---|---|---|---|
| `activity.entry` | `GET` | `/v1/activity/{entry_id}` | `Empty` | `ActivityEntryDetail` | not used |
| `activity.export.evidence` | `POST` | `/v1/activity/exports/evidence` | `ExportEvidenceRequest` | `ExportArtefact` | required |
| `activity.export.statement` | `POST` | `/v1/activity/exports/statement` | `ExportStatementRequest` | `ExportArtefact` | required |
| `activity.query` | `POST` | `/v1/activity/query` | `ActivityQueryRequest` | `ActivityPage` | not used |
| `notification.list` | `GET` | `/v1/notifications` | `Empty` | `NotificationPage` | not used |
| `notification.preferences.get` | `GET` | `/v1/notifications/preferences` | `Empty` | `NotificationPreferences` | not used |
| `notification.preferences.set` | `POST` | `/v1/notifications/preferences` | `NotificationPreferences` | `NotificationPreferences` | not used |
| `notification.read` | `POST` | `/v1/notifications/{notification_id}/read` | `Empty` | `NotificationSummary` | not used |
| `agent.archive` | `POST` | `/v1/agents/{agent_id}/archive` | `AgentArchiveRequest` | `Journey` | required |
| `agent.create` | `POST` | `/v1/agents` | `AgentCreateRequest` | `Journey` | required |
| `agent.get` | `GET` | `/v1/agents/{agent_id}` | `Empty` | `Agent` | not used |
| `agent.limit` | `POST` | `/v1/agents/{agent_id}/limit` | `AgentLimitRequest` | `Agent` | required |
| `agent.list` | `GET` | `/v1/agents` | `Empty` | `AgentPage` | not used |
| `agent.pause` | `POST` | `/v1/agents/{agent_id}/pause` | `Empty` | `Agent` | required |
| `agent.reclaim` | `POST` | `/v1/agents/{agent_id}/reclaim` | `AgentReclaimRequest` | `Journey` | required |
| `agent.recover` | `POST` | `/v1/agents/{agent_id}/recover` | `Empty` | `KeyChallenge` | required |
| `agent.resume` | `POST` | `/v1/agents/{agent_id}/resume` | `Empty` | `Agent` | required |
| `agent.rotate` | `POST` | `/v1/agents/{agent_id}/rotate` | `Empty` | `KeyChallenge` | required |
| `approval.approve` | `POST` | `/v1/approvals/{approval_id}/approve` | `ApprovalApproveRequest` | `ApprovalDecision` | required |
| `approval.get` | `GET` | `/v1/approvals/{approval_id}` | `Empty` | `ApprovalDetail` | not used |
| `approval.list` | `GET` | `/v1/approvals` | `Empty` | `ApprovalPage` | not used |
| `approval.reject` | `POST` | `/v1/approvals/{approval_id}/reject` | `Empty` | `ApprovalDecision` | required |
| `account.balance` | `GET` | `/v1/account/balance` | `Empty` | `AccountBalance` | not used |
| `home.summary` | `GET` | `/v1/home` | `Empty` | `HomeSummary` | not used |
| `account.create` | `POST` | `/v1/accounts` | `AccountCreateRequest` | `AccountCreation` | required |
| `authenticator.backup.rotate` | `POST` | `/v1/security/authenticators/backup-codes` | `BackupCodeRotation` | `BackupCodeSet` | not used |
| `authenticator.disable` | `POST` | `/v1/security/authenticators/{authenticator_id}/disable` | `AuthenticatorDisable` | `AuthenticatorStatus` | not used |
| `authenticator.setup.begin` | `POST` | `/v1/security/authenticators/setups` | `AuthenticatorSetupBegin` | `AuthenticatorSetupChallenge` | not used |
| `authenticator.setup.finish` | `POST` | `/v1/security/authenticators/setups/{setup_id}` | `AuthenticatorSetupFinish` | `AuthenticatorSetupResult` | not used |
| `authenticator.status` | `GET` | `/v1/security/authenticators` | `Empty` | `AuthenticatorStatus` | not used |
| `binding.rebind` | `POST` | `/v1/wallet-binding/rebind` | `RebindingSubmission` | `Journey` | required |
| `binding.rebind.action` | `POST` | `/v1/wallet-binding/rebind/action` | `BindingStatementRequest` | `BindingRebindAction` | not used |
| `binding.statement` | `POST` | `/v1/wallet-binding/statement` | `BindingStatementRequest` | `BindingStatement` | not used |
| `binding.status` | `GET` | `/v1/wallet-binding` | `Empty` | `WalletBinding` | not used |
| `binding.submit` | `POST` | `/v1/wallet-binding` | `BindingSubmission` | `Journey` | required |
| `onboarding.resume` | `POST` | `/v1/onboarding/resume` | `Empty` | `Journey` | not used |
| `onboarding.status` | `GET` | `/v1/onboarding` | `Empty` | `Journey` | not used |
| `passkey.assert.begin` | `POST` | `/v1/passkeys/assertions` | `PasskeyAssertionBegin` | `PasskeyAssertionChallenge` | not used |
| `passkey.assert.finish` | `POST` | `/v1/passkeys/assertions/{assertion_id}` | `PasskeyAssertionFinish` | `PasskeyAssertion` | not used |
| `passkey.register.begin` | `POST` | `/v1/passkeys/registrations` | `PasskeyRegistrationBegin` | `PasskeyRegistrationChallenge` | not used |
| `passkey.register.finish` | `POST` | `/v1/passkeys/registrations/{registration_id}` | `PasskeyRegistrationFinish` | `Passkey` | not used |
| `profile.get` | `GET` | `/v1/profile` | `Empty` | `Profile` | not used |
| `profile.update` | `PATCH` | `/v1/profile` | `ProfileUpdate` | `Profile` | not used |
| `security.action` | `POST` | `/v1/security/actions` | `SecurityActionRequest` | `SecurityAction` | not used |
| `security.passkey.list` | `GET` | `/v1/security/passkeys` | `Empty` | `PasskeyList` | not used |
| `security.passkey.register.begin` | `POST` | `/v1/security/passkeys/registrations` | `SecurityPasskeyRegistrationBegin` | `PasskeyRegistrationChallenge` | not used |
| `security.passkey.register.finish` | `POST` | `/v1/security/passkeys/registrations/{registration_id}` | `SecurityPasskeyRegistrationFinish` | `Passkey` | not used |
| `security.passkey.revoke` | `POST` | `/v1/security/passkeys/{passkey_id}/revoke` | `SecurityPasskeyRevocation` | `PasskeyList` | not used |
| `security.recovery.reveal` | `POST` | `/v1/security/recovery/evidence` | `SecurityRecoveryReveal` | `TimedSecret` | not used |
| `security.session.revoke` | `POST` | `/v1/security/sessions/{session_id}/revoke` | `SecuritySessionRevocation` | `SessionRevocation` | required |
| `security.session.revoke-all` | `POST` | `/v1/security/sessions/revoke-all` | `SecuritySessionRevocation` | `SessionRevocation` | required |
| `session.list` | `GET` | `/v1/sessions` | `Empty` | `SessionList` | not used |
| `session.open` | `POST` | `/v1/sessions` | `SessionOpenRequest` | `Session` | required |
| `session.refresh` | `POST` | `/v1/sessions/refresh` | `Empty` | `Session` | not used |
| `session.revoke` | `DELETE` | `/v1/sessions/{session_id}` | `Empty` | `SessionRevocation` | required |
| `session.revoke-all` | `POST` | `/v1/sessions/revoke-all` | `Empty` | `SessionRevocation` | required |
| `stepup.begin` | `POST` | `/v1/step-up` | `StepUpRequest` | `StepUpChallenge` | not used |
| `stepup.finish` | `POST` | `/v1/step-up/{challenge_id}` | `StepUpFinish` | `StepUpEvidence` | not used |
| `evidence.get` | `GET` | `/v1/evidence/{evidence_id}` | `Empty` | `EvidenceMaterial` | not used |
| `journey.get` | `GET` | `/v1/journeys/{journey_id}` | `Empty` | `Journey` | not used |
| `journey.list` | `GET` | `/v1/journeys` | `Empty` | `JourneyPage` | not used |
| `deposit.confirm` | `POST` | `/v1/deposits/{journey_id}/confirm` | `DepositConfirmRequest` | `Journey` | not used |
| `deposit.start` | `POST` | `/v1/deposits` | `DepositStartRequest` | `Journey` | required |
| `exit.eligibility` | `GET` | `/v1/exit/eligibility` | `Empty` | `ExitEligibility` | not used |
| `exit.start` | `POST` | `/v1/exit` | `ExitStartRequest` | `Journey` | required |
| `move.commit` | `POST` | `/v1/moves` | `MoveCommitRequest` | `Journey` | required |
| `move.quote` | `POST` | `/v1/moves/quote` | `MoveQuoteRequest` | `MoveQuote` | not used |
| `withdraw.claim` | `POST` | `/v1/withdrawals/{journey_id}/claim` | `WithdrawClaimRequest` | `Journey` | not used |
| `withdraw.start` | `POST` | `/v1/withdrawals` | `WithdrawStartRequest` | `Journey` | required |
| `stream.next` | `GET` | `/v1/stream/{cursor}` | `Empty` | `StreamPage` | not used |
| `stream.open` | `POST` | `/v1/stream` | `Empty` | `StreamPosition` | not used |
| `support.create` | `POST` | `/v1/support/conversations` | `SupportCreateRequest` | `SupportConversation` | required |
| `support.feedback` | `POST` | `/v1/support/conversations/{conversation_id}/feedback` | `SupportFeedbackRequest` | `SupportConversation` | not used |
| `support.list` | `GET` | `/v1/support/conversations` | `Empty` | `SupportConversationPage` | not used |
| `support.read` | `POST` | `/v1/support/conversations/{conversation_id}/read` | `SupportReadRequest` | `SupportConversationStatus` | not used |
| `support.reply` | `POST` | `/v1/support/conversations/{conversation_id}/replies` | `SupportReplyRequest` | `SupportConversation` | required |
| `support.status` | `GET` | `/v1/support/conversations/{conversation_id}/status` | `Empty` | `SupportConversationStatus` | not used |
| `version` | `GET` | `/v1/version` | `Empty` | `VersionInfo` | not used |

## Declared types

### Module `activity`

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `ActivityEntryId` | scalar | json: `string`<br>prefix: `act_`<br>rust: `String`<br>typescript: `string` |
| `ExportId` | scalar | json: `string`<br>prefix: `exp_`<br>rust: `String`<br>typescript: `string` |
| `NotificationId` | scalar | json: `string`<br>prefix: `ntf_`<br>rust: `String`<br>typescript: `string` |
| `ActivityEntry` | type | required: `entry_id:ActivityEntryId`, `kind:ActivityEntryKind`, `state:JourneyState`, `state_copy_key:CopyKey`, `summary_copy_key:CopyKey`, `occurred_at:Timestamp`<br>optional: `money:Money`, `direction:MoneyDirection`, `agent_id:AgentId`, `journey_id:JourneyId`, `approval_id:ApprovalId` |
| `ActivityEntryDetail` | type | required: `entry_id:ActivityEntryId`, `kind:ActivityEntryKind`, `state:JourneyState`, `state_copy_key:CopyKey`, `summary_copy_key:CopyKey`, `occurred_at:Timestamp`, `stages:JourneyStage[]`, `evidence:EvidenceRef[]`<br>optional: `money:Money`, `fees:Money`, `direction:MoneyDirection`, `agent_id:AgentId`, `journey_id:JourneyId`, `approval_id:ApprovalId` |
| `ActivityEntryKind` | type | variants: `deposit`, `withdrawal`, `movement`, `agent-action`, `approval`, `security-event` |
| `ActivityFilter` | type | optional: `kinds:ActivityEntryKind[]`, `agent_id:AgentId`, `from:Timestamp`, `to:Timestamp` |
| `ActivityGroup` | type | required: `month:string`, `subtotal_in:Money`, `subtotal_out:Money`, `entries:ActivityEntry[]` |
| `ActivityPage` | type | required: `groups:ActivityGroup[]`, `next_cursor:Cursor`, `filter:ActivityFilter` |
| `ActivityQueryRequest` | type | optional: `cursor:Cursor`, `filter:ActivityFilter`, `page_limit:integer` |
| `ChannelPreference` | type | required: `enabled:boolean`, `classes:ClassToggle[]` |
| `ClassToggle` | type | required: `class:NotificationClass`, `enabled:boolean` |
| `ExportArtefact` | type | required: `export_id:ExportId`, `kind:ExportKind`, `download_path:string`, `content_type:string`, `created_at:Timestamp`, `evidence:EvidenceRef[]` |
| `ExportEvidenceRequest` | type | optional: `filter:ActivityFilter`, `entry_ids:ActivityEntryId[]` |
| `ExportKind` | type | variants: `statement`, `evidence-bundle` |
| `ExportStatementRequest` | type | optional: `filter:ActivityFilter` |
| `MoneyDirection` | type | variants: `in`, `out` |
| `NotificationClass` | type | variants: `approval-waiting`, `money-arrived`, `journey-finished`, `claim-ready`, `security-new-device`, `security-recovery`, `security-wallet-rebinding`, `security-key-rotation`, `service-status` |
| `NotificationDetailLevel` | type | variants: `full`, `summary`, `minimal` |
| `NotificationGroup` | type | required: `recency:NotificationRecency`, `notifications:NotificationSummary[]` |
| `NotificationPage` | type | required: `groups:NotificationGroup[]`, `next_cursor:Cursor`, `unread_count:integer` |
| `NotificationPreferences` | type | required: `push:ChannelPreference`, `email:ChannelPreference`, `in_app:ChannelPreference`, `detail:NotificationDetailLevel` |
| `NotificationRecency` | type | variants: `today`, `yesterday`, `this-week`, `earlier` |
| `NotificationSummary` | type | required: `notification_id:NotificationId`, `class:NotificationClass`, `title_copy_key:CopyKey`, `body_copy_key:CopyKey`, `deep_link:string`, `read:boolean`, `created_at:Timestamp`<br>optional: `money:Money`, `agent_id:AgentId`, `approval_id:ApprovalId`, `journey_id:JourneyId`, `action_copy_key:CopyKey` |

### Module `agents`

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `AgentId` | scalar | json: `string`<br>prefix: `agt_`<br>rust: `String`<br>typescript: `string` |
| `ApprovalId` | scalar | json: `string`<br>prefix: `apr_`<br>rust: `String`<br>typescript: `string` |
| `Agent` | type | required: `agent_id:AgentId`, `name:string`, `purpose:string`, `state:AgentState`, `state_copy_key:CopyKey`, `limit:SpendLimit`, `spend:AgentSpend`, `evidence:EvidenceRef[]`, `created_at:Timestamp`, `updated_at:Timestamp`<br>optional: `creation_journey_id:JourneyId` |
| `AgentArchiveRequest` | type | required: `confirm_name:string` |
| `AgentCreateRequest` | type | required: `name:string`, `purpose:string`, `monthly_limit:Money` |
| `AgentLimitRequest` | type | required: `monthly_limit:Money` |
| `AgentPage` | type | required: `agents:Agent[]`, `next_cursor:Cursor` |
| `AgentReclaimRequest` | type | required: `money:Money` |
| `AgentSpend` | type | required: `period_start:Timestamp`, `period_end:Timestamp`, `spent:Money`, `remaining:Money`, `verification:VerificationLevel`<br>optional: `reconciliation_copy_key:CopyKey` |
| `AgentState` | type | variants: `creating`, `active`, `paused`, `archiving`, `archived` |
| `ApprovalApproveRequest` | type | required: `step_up_evidence:EvidenceId` |
| `ApprovalDecision` | type | required: `approval_id:ApprovalId`, `state:ApprovalState`, `state_copy_key:CopyKey`, `money_moved:boolean`, `moved_copy_key:CopyKey`, `evidence:EvidenceRef[]` |
| `ApprovalDetail` | type | required: `approval_id:ApprovalId`, `agent_id:AgentId`, `agent_name:string`, `state:ApprovalState`, `state_copy_key:CopyKey`, `reason_copy_key:CopyKey`, `facts:ApprovalFacts`, `budget_remaining_after:VerifiedMoney`, `created_at:Timestamp`, `evidence:EvidenceRef[]` |
| `ApprovalFacts` | type | required: `amount:Money`, `counterparty:string`, `asset:CurrencyCode`, `fees:Money`, `expires_at:Timestamp` |
| `ApprovalPage` | type | required: `approvals:ApprovalSummary[]`, `next_cursor:Cursor` |
| `ApprovalState` | type | variants: `pending`, `approved`, `rejected`, `expired`, `defective` |
| `ApprovalSummary` | type | required: `approval_id:ApprovalId`, `agent_id:AgentId`, `agent_name:string`, `counterparty:string`, `amount:Money`, `reason_copy_key:CopyKey`, `expires_at:Timestamp`, `state:ApprovalState`, `budget_remaining_after:VerifiedMoney` |
| `KeyChallenge` | type | required: `agent_id:AgentId`, `kind:KeyChallengeKind`, `delay_copy_key:CopyKey`, `delay_seconds:integer`, `ready_at:Timestamp`, `evidence:EvidenceRef[]` |
| `KeyChallengeKind` | type | variants: `rotate`, `recover` |
| `LimitEnforcement` | type | variants: `protocol`, `app` |
| `SpendLimit` | type | required: `monthly:Money`, `enforcement:LimitEnforcement`, `enforcement_copy_key:CopyKey` |
| `VerifiedMoney` | type | required: `money:Money`, `verification:VerificationLevel` |

### Module `errors`

Every failure response carries one typed shape: a stable machine code, the copy-catalog key naming the human message, the trace identifier on the envelope, and a retriability classification. No operation returns an unstructured error.

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `ApiError` | type | required: `code:ErrorCode`, `copy_key:CopyKey`, `retry:Retriability`<br>optional: `retry_after_ms:integer`, `field:string` |
| `ErrorCode` | type | variants: `unauthenticated`, `session-expired`, `step-up-required`, `forbidden`, `not-found`, `invalid-request`, `conflict`, `rate-limited`, `cursor-expired`, `unavailable`, `upstream-degraded`, `challenge-expired`, `refused-by-policy`, `refused-by-budget`, `refused-by-capability`, `refused-by-protocol`, `refused-by-limit`, `quote-expired`, `wallet-not-bound`, `exit-unavailable`, `already-decided`, `hold-expired`, `hold-defective`, `archive-needs-disposition`, `confirmation-mismatch`, `not-suppressible`, `support-unavailable`, `support-conversation-unknown`, `support-message-unknown` |
| `Retriability` | type | variants: `retriable`, `retriable-after`, `structural`, `final` |

### Module `home`

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `AccountBalance` | type | required: `account_id:AccountId`, `money:Money`, `verification:VerificationLevel`, `freshness:ProtocolFreshness`, `evidence:EvidenceRef[]` |
| `HomeSummary` | type | required: `balance:AccountBalance`, `agents:Agent[]`, `approvals:ApprovalSummary[]`, `recent_activity:ActivityEntryDetail[]` |
| `ProtocolFreshness` | type | required: `observed_at:Timestamp`, `age_seconds:integer`, `source_head:string`, `within_bound:boolean`<br>optional: `checkpoint:string` |

### Module `identity`

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `AccountId` | scalar | json: `string`<br>prefix: `act_`<br>rust: `String`<br>typescript: `string` |
| `AssertionId` | scalar | json: `string`<br>prefix: `asr_`<br>rust: `String`<br>typescript: `string` |
| `AuthenticatorId` | scalar | json: `string`<br>prefix: `mfa_`<br>rust: `String`<br>typescript: `string` |
| `AuthenticatorSetupId` | scalar | json: `string`<br>prefix: `mfs_`<br>rust: `String`<br>typescript: `string` |
| `DeviceId` | scalar | json: `string`<br>prefix: `dev_`<br>rust: `String`<br>typescript: `string` |
| `EmailAddress` | scalar | json: `string`<br>rust: `String`<br>typescript: `string` |
| `EvmAddress` | scalar | json: `string`<br>prefix: `0x`<br>rust: `String`<br>typescript: `string` |
| `EvmSignature` | scalar | json: `string`<br>prefix: `0x`<br>rust: `String`<br>typescript: `string` |
| `OpaqueCredential` | scalar | json: `string`<br>rust: `String`<br>typescript: `string` |
| `OperationDigest` | scalar | json: `string`<br>prefix: `opd_`<br>rust: `String`<br>typescript: `string` |
| `PasskeyId` | scalar | json: `string`<br>prefix: `pky_`<br>rust: `String`<br>typescript: `string` |
| `RegistrationId` | scalar | json: `string`<br>prefix: `reg_`<br>rust: `String`<br>typescript: `string` |
| `SessionId` | scalar | json: `string`<br>prefix: `ses_`<br>rust: `String`<br>typescript: `string` |
| `StepUpChallengeId` | scalar | json: `string`<br>prefix: `chg_`<br>rust: `String`<br>typescript: `string` |
| `AccountCreateRequest` | type | required: `email:EmailAddress`, `display_name:string` |
| `AccountCreation` | type | required: `account_id:AccountId`, `onboarding:Journey` |
| `AuthenticatorDisable` | type | required: `step_up:StepUpEvidence` |
| `AuthenticatorMethod` | type | required: `authenticator_id:AuthenticatorId`, `label:string`, `enabled_at:Timestamp`<br>optional: `last_used_at:Timestamp` |
| `AuthenticatorSetupBegin` | type | required: `label:string`, `step_up:StepUpEvidence` |
| `AuthenticatorSetupChallenge` | type | required: `setup_id:AuthenticatorSetupId`, `secret:TimedSecret`, `otpauth_uri:TimedSecret`, `expires_at:Timestamp` |
| `AuthenticatorSetupFinish` | type | required: `code:string`, `step_up:StepUpEvidence` |
| `AuthenticatorSetupResult` | type | required: `method:AuthenticatorMethod`, `backup_codes:BackupCodeSet` |
| `AuthenticatorStatus` | type | required: `methods:AuthenticatorMethod[]`, `backup_codes_remaining:integer` |
| `BackupCodeRotation` | type | required: `step_up:StepUpEvidence` |
| `BackupCodeSet` | type | required: `codes:string[]`, `remask_at:Timestamp`, `copyable:boolean` |
| `BindingRebindAction` | type | required: `binding:BindingStatement`, `confirms:OperationDigest` |
| `BindingState` | type | variants: `none`, `binding`, `bound`, `rebinding` |
| `BindingStatement` | type | required: `statement:string`, `address:EvmAddress`, `expires_at:Timestamp` |
| `BindingStatementRequest` | type | required: `address:EvmAddress` |
| `BindingSubmission` | type | required: `address:EvmAddress`, `statement:string`, `signature:EvmSignature` |
| `Device` | type | required: `device_id:DeviceId`, `label:string`, `platform:string` |
| `Passkey` | type | required: `passkey_id:PasskeyId`, `label:string`, `created_at:Timestamp`<br>optional: `last_used_at:Timestamp` |
| `PasskeyAssertion` | type | required: `assertion_id:AssertionId`, `passkey_id:PasskeyId`, `completed_at:Timestamp`, `expires_at:Timestamp` |
| `PasskeyAssertionBegin` | type | optional: `email:EmailAddress` |
| `PasskeyAssertionChallenge` | type | required: `assertion_id:AssertionId`, `ceremony:OpaqueCredential`, `expires_at:Timestamp` |
| `PasskeyAssertionFinish` | type | required: `credential:OpaqueCredential` |
| `PasskeyList` | type | required: `passkeys:Passkey[]` |
| `PasskeyRegistrationBegin` | type | required: `account_id:AccountId` |
| `PasskeyRegistrationChallenge` | type | required: `registration_id:RegistrationId`, `ceremony:OpaqueCredential`, `expires_at:Timestamp` |
| `PasskeyRegistrationFinish` | type | required: `credential:OpaqueCredential` |
| `Profile` | type | required: `display_name:string`<br>optional: `avatar_url:string` |
| `ProfileUpdate` | type | optional: `display_name:string`, `avatar_url:string` |
| `RebindingSubmission` | type | required: `address:EvmAddress`, `statement:string`, `signature:EvmSignature`, `step_up:StepUpEvidence` |
| `SecurityAction` | type | required: `confirms:OperationDigest` |
| `SecurityActionKind` | type | variants: `add-passkey`, `revoke-passkey`, `revoke-session`, `revoke-all-sessions`, `add-authenticator`, `disable-authenticator`, `rotate-backup-codes`, `reveal-recovery-evidence` |
| `SecurityActionRequest` | type | required: `action:SecurityActionKind`<br>optional: `target_id:string` |
| `SecurityPasskeyRegistrationBegin` | type | required: `label:string`, `step_up:StepUpEvidence` |
| `SecurityPasskeyRegistrationFinish` | type | required: `credential:OpaqueCredential`, `step_up:StepUpEvidence` |
| `SecurityPasskeyRevocation` | type | required: `step_up:StepUpEvidence` |
| `SecurityRecoveryReveal` | type | required: `evidence_id:EvidenceId`, `step_up:StepUpEvidence` |
| `SecuritySessionRevocation` | type | required: `step_up:StepUpEvidence` |
| `Session` | type | required: `session_id:SessionId`, `device:Device`, `opened_at:Timestamp`, `last_active_at:Timestamp`, `current:boolean` |
| `SessionDevice` | type | required: `label:string`, `platform:string` |
| `SessionList` | type | required: `sessions:Session[]` |
| `SessionOpenRequest` | type | required: `assertion_id:AssertionId`<br>optional: `device:SessionDevice` |
| `SessionRevocation` | type | required: `revoked_session_ids:SessionId[]`, `revoked_at:Timestamp` |
| `StepUpChallenge` | type | required: `challenge_id:StepUpChallengeId`, `confirms:OperationDigest`, `ceremony:OpaqueCredential`, `expires_at:Timestamp` |
| `StepUpEvidence` | type | required: `challenge_id:StepUpChallengeId`, `confirms:OperationDigest`, `passkey_id:PasskeyId`, `completed_at:Timestamp`, `expires_at:Timestamp` |
| `StepUpFinish` | type | required: `credential:OpaqueCredential` |
| `StepUpRequest` | type | required: `confirms:OperationDigest` |
| `TimedSecret` | type | required: `value:string`, `remask_at:Timestamp`, `copyable:boolean` |
| `WalletBinding` | type | required: `state:BindingState`<br>optional: `address:EvmAddress`, `bound_at:Timestamp`, `evidence:EvidenceRef` |

### Module `journeys`

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `EvidenceClass` | type | variants: `local-journey-state`, `submission-record`, `layerx-receipt`, `checkpoint-proof`, `paxeer-finality`, `typed-refusal`, `approval-hold`, `wallet-ack` |
| `EvidenceMaterial` | type | required: `evidence_id:EvidenceId`, `class:EvidenceClass`, `verification:VerificationLevel`, `content_type:string`, `bytes_base64:string`<br>optional: `settlement_domain:SettlementDomain` |
| `EvidenceRef` | type | required: `evidence_id:EvidenceId`, `class:EvidenceClass`, `verification:VerificationLevel`<br>optional: `settlement_domain:SettlementDomain` |
| `Journey` | type | required: `journey_id:JourneyId`, `kind:JourneyKind`, `state:JourneyState`, `state_copy_key:CopyKey`, `stages:JourneyStage[]`, `evidence:EvidenceRef[]`, `started_at:Timestamp`, `updated_at:Timestamp`<br>optional: `refusal:Refusal`, `wallet_request:WalletSignRequest` |
| `JourneyKind` | type | variants: `onboarding`, `wallet-binding`, `deposit`, `withdraw`, `exit`, `move`, `agent-create`, `agent-fund`, `agent-pause`, `agent-retire` |
| `JourneyPage` | type | required: `journeys:Journey[]`, `next_cursor:Cursor` |
| `JourneyStage` | type | required: `stage_id:StageId`, `copy_key:CopyKey`, `state:JourneyState`, `evidence:EvidenceRef[]` |
| `JourneyState` | type | variants: `getting-ready`, `sending`, `processing`, `done`, `done-finalised`, `still-checking`, `refused`, `waiting-for-you` |
| `VerificationLevel` | type | variants: `unverified`, `receipt-verified`, `checkpoint-finalised`, `paxeer-finalised` |

### Module `movement`

Movements inside LayerX are fund, allocate, return or transfer, in every API surface, log and copy string. Deposit, withdraw and exit name journeys across the Paxeer custody boundary exclusively and never name an internal movement.

The user sees one verb: Move money. The user says who gets how much, the route resolver picks the mechanism, and the user never selects a transfer type, bridge or route.

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `QuoteId` | scalar | json: `string`<br>prefix: `qte_`<br>rust: `String`<br>typescript: `string` |
| `WalletAddress` | scalar | json: `string`<br>prefix: `0x`<br>rust: `String`<br>typescript: `string` |
| `WalletSignature` | scalar | json: `string`<br>prefix: `0x`<br>rust: `String`<br>typescript: `string` |
| `WalletTxId` | scalar | json: `string`<br>prefix: `0x`<br>rust: `String`<br>typescript: `string` |
| `DepositConfirmRequest` | type | required: `wallet_transaction:WalletTxId`<br>optional: `settlement_domain:SettlementDomain` |
| `DepositStartRequest` | type | required: `money:Money`<br>optional: `settlement_domain:SettlementDomain` |
| `ExitEligibility` | type | required: `eligible:boolean`, `copy_key:CopyKey`<br>optional: `withdraw_instead_path:string`, `settlement_domain:SettlementDomain` |
| `ExitStartRequest` | type | required: `confirmation:string`<br>optional: `settlement_domain:SettlementDomain` |
| `MoveCommitRequest` | type | required: `quote_id:QuoteId` |
| `MoveMechanism` | type | variants: `fund`, `allocate`, `return`, `transfer` |
| `MoveQuote` | type | required: `quote_id:QuoteId`, `description_copy_key:CopyKey`, `mechanism:MoveMechanism`, `money:Money`, `fee_estimate:Money`, `fee_ceiling:Money`, `arrival_estimate:Timestamp`, `expires_at:Timestamp`<br>optional: `irreversibility_copy_key:CopyKey` |
| `MoveQuoteRequest` | type | required: `source:string`, `destination:string`, `money:Money` |
| `Refusal` | type | required: `refused_by:RefusedBy`, `copy_key:CopyKey`, `money_left:boolean`<br>optional: `change_path:string` |
| `RefusedBy` | type | variants: `policy`, `budget`, `capability`, `protocol`, `limit` |
| `SettlementDomain` | type | variants: `paxeer` |
| `WalletSignRequest` | type | required: `stage_id:StageId`, `copy_key:CopyKey`, `from_address:WalletAddress`, `to_sign_base64:string`<br>optional: `settlement_domain:SettlementDomain` |
| `WithdrawClaimRequest` | type | required: `claim_signature:WalletSignature`<br>optional: `settlement_domain:SettlementDomain` |
| `WithdrawStartRequest` | type | required: `money:Money`, `destination:WalletAddress`<br>optional: `settlement_domain:SettlementDomain` |

### Module `stream`

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `StreamEvent` | type | required: `cursor:Cursor`, `kind:StreamEventKind`, `observed_at:Timestamp`<br>optional: `journey:Journey`, `approval:ApprovalSummary`, `notification:NotificationSummary` |
| `StreamEventKind` | type | variants: `journey-progress`, `approval-created`, `approval-approved`, `approval-rejected`, `approval-expired`, `notification` |
| `StreamPage` | type | required: `events:StreamEvent[]`, `next_cursor:Cursor` |
| `StreamPosition` | type | required: `cursor:Cursor` |

### Module `support`

additive_only

| Declaration | Kind | Shape |
|---|---|---|
| `SupportConversationId` | scalar | json: `string`<br>prefix: `sup_`<br>rust: `String`<br>typescript: `string` |
| `SupportMessageId` | scalar | json: `string`<br>prefix: `msg_`<br>rust: `String`<br>typescript: `string` |
| `SupportAuthor` | type | variants: `you`, `support` |
| `SupportConversation` | type | required: `conversation_id:SupportConversationId`, `shell:SupportShell`, `state:SupportConversationState`, `created_at:Timestamp`, `updated_at:Timestamp`, `messages:SupportMessage[]`, `feedback:SupportFeedback[]`<br>optional: `trace_id:TraceId` |
| `SupportConversationPage` | type | required: `conversations:SupportConversation[]` |
| `SupportConversationState` | type | variants: `waiting-for-support`, `waiting-for-you`, `resolved` |
| `SupportConversationStatus` | type | required: `conversation_id:SupportConversationId`, `state:SupportConversationState`, `unread_count:integer`, `updated_at:Timestamp` |
| `SupportCreateRequest` | type | required: `body:string`, `shell:SupportShell`<br>optional: `topic:SupportTopic`, `trace_id:TraceId` |
| `SupportFeedback` | type | required: `message_id:SupportMessageId`, `helpful:boolean`, `received_at:Timestamp` |
| `SupportFeedbackRequest` | type | required: `message_id:SupportMessageId`, `helpful:boolean` |
| `SupportMessage` | type | required: `message_id:SupportMessageId`, `author:SupportAuthor`, `body:string`, `sent_at:Timestamp`, `read:boolean`<br>optional: `topic:SupportTopic` |
| `SupportReadRequest` | type | required: `through_message_id:SupportMessageId` |
| `SupportReplyRequest` | type | required: `body:string` |
| `SupportShell` | type | variants: `mobile`, `desktop` |
| `SupportTopic` | type | variants: `deposit`, `withdrawal`, `agents`, `account`, `report` |

### Module `v1`

| Declaration | Kind | Shape |
|---|---|---|
| `Money` | record | fields: `amount:Amount`, `currency:CurrencyCode` |
| `ResponseEnvelope` | record | optional: `error:ApiError`<br>fields: `ok:boolean`, `result:object`, `trace:TraceId` |
| `SchemaVersion` | record | fields: `major:integer`, `minor:integer` |
| `VersionInfo` | record | fields: `schema:SchemaVersion`, `service:string` |
| `Amount` | scalar | json: `string`<br>format: `decimal`<br>rust: `u128`<br>typescript: `bigint` |
| `CopyKey` | scalar | json: `string`<br>format: `copy-key`<br>rust: `String`<br>typescript: `string` |
| `CurrencyCode` | scalar | json: `string`<br>format: `currency`<br>rust: `String`<br>typescript: `string` |
| `Cursor` | scalar | json: `string`<br>rust: `String`<br>typescript: `string` |
| `EvidenceId` | scalar | json: `string`<br>prefix: `evd_`<br>rust: `String`<br>typescript: `string` |
| `JourneyId` | scalar | json: `string`<br>prefix: `jrn_`<br>rust: `String`<br>typescript: `string` |
| `StageId` | scalar | json: `string`<br>prefix: `stg_`<br>rust: `String`<br>typescript: `string` |
| `Timestamp` | scalar | json: `string`<br>format: `rfc3339-utc`<br>rust: `String`<br>typescript: `string` |
| `TraceId` | scalar | json: `string`<br>prefix: `trc_`<br>rust: `String`<br>typescript: `string` |
| `Empty` | type | - |
