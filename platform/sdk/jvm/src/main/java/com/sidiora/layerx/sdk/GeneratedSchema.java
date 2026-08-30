// Code generated from the LayerX Agent API and Human API schemas. DO NOT EDIT.

package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.annotation.JsonCreator;
import com.fasterxml.jackson.annotation.JsonProperty;
import com.fasterxml.jackson.annotation.JsonValue;
import com.fasterxml.jackson.databind.JsonNode;
import java.math.BigInteger;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;

public final class GeneratedSchema {
    private GeneratedSchema() {}
    public static final class AgentModels {
        private AgentModels() {}
        public record ApiError(@JsonProperty("class") JsonNode class_, JsonNode protocol_result_code, JsonNode retriability, JsonNode request_id, JsonNode reason) implements SchemaTypes.GeneratedResponse {
            public ApiError {
                Objects.requireNonNull(class_, "class");
                Objects.requireNonNull(protocol_result_code, "protocol_result_code");
                Objects.requireNonNull(retriability, "retriability");
                Objects.requireNonNull(request_id, "request_id");
                Objects.requireNonNull(reason, "reason");
            }
        }
        public enum ApprovalDecisionOutcome {
            GRANTED("Granted"),
            REJECTED("Rejected"),
            EXPIRED("Expired"),
            DEFECTIVE("Defective"),
            ALREADYDECIDED("AlreadyDecided"),
            CONFLICT("Conflict");
            private final String wire;
            ApprovalDecisionOutcome(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static ApprovalDecisionOutcome fromWire(String wire) {
                for (ApprovalDecisionOutcome value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record ApprovalLifecycleEvent(String kind, JsonNode value) implements SchemaTypes.GeneratedEvent {
            private static final Set<String> KINDS = Set.of("Created", "Granted", "Rejected", "Expired", "Defective");
            public ApprovalLifecycleEvent {
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(value, "value");
                if (!KINDS.contains(kind)) throw PlatformSdkException.invalidArgument();
            }
        }
        public record ApprovalRecord(JsonNode approval_id, JsonNode tenant, JsonNode held_activity, JsonNode canonical_bytes_digest, JsonNode hold_reason, BigInteger created_at, BigInteger expires_at, JsonNode state) implements SchemaTypes.GeneratedResponse {
            public ApprovalRecord {
                Objects.requireNonNull(approval_id, "approval_id");
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(held_activity, "held_activity");
                Objects.requireNonNull(canonical_bytes_digest, "canonical_bytes_digest");
                Objects.requireNonNull(hold_reason, "hold_reason");
                Objects.requireNonNull(created_at, "created_at");
                SchemaTypes.protocolU64(created_at);
                Objects.requireNonNull(expires_at, "expires_at");
                SchemaTypes.protocolU64(expires_at);
                Objects.requireNonNull(state, "state");
            }
        }
        public enum ApprovalState {
            HELD("Held"),
            GRANTED("Granted"),
            REJECTED("Rejected"),
            EXPIRED("Expired"),
            DEFECTIVE("Defective");
            private final String wire;
            ApprovalState(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static ApprovalState fromWire(String wire) {
                for (ApprovalState value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record AuthorityResponse(JsonNode authority, JsonNode value) implements SchemaTypes.GeneratedResponse {
            public AuthorityResponse {
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(value, "value");
            }
        }
        public enum BudgetEnforcement {
            PROTOCOLBUDGET("ProtocolBudget"),
            DAEMONLIMIT("DaemonLimit");
            private final String wire;
            BudgetEnforcement(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static BudgetEnforcement fromWire(String wire) {
                for (BudgetEnforcement value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record CapabilityDimensions(JsonNode activity_types, JsonNode counterparties, JsonNode assets, JsonNode amount_ceilings, JsonNode rate_ceilings, JsonNode purpose_constraints, JsonNode expiry) implements SchemaTypes.GeneratedResponse {
            public CapabilityDimensions {
                Objects.requireNonNull(activity_types, "activity_types");
                Objects.requireNonNull(counterparties, "counterparties");
                Objects.requireNonNull(assets, "assets");
                Objects.requireNonNull(amount_ceilings, "amount_ceilings");
                Objects.requireNonNull(rate_ceilings, "rate_ceilings");
                Objects.requireNonNull(purpose_constraints, "purpose_constraints");
                Objects.requireNonNull(expiry, "expiry");
            }
        }
        public record ContractVersion(long major, long minor) implements SchemaTypes.GeneratedResponse {
        }
        public enum Delivery {
            EVENT("Event"),
            GAP("Gap"),
            TRUNCATED("Truncated");
            private final String wire;
            Delivery(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static Delivery fromWire(String wire) {
                for (Delivery value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record Disclosure(JsonNode canonical_digest, JsonNode activity_type, JsonNode actor, JsonNode authority, JsonNode counterparties, JsonNode amounts, JsonNode asset, JsonNode fee_limit, JsonNode expiry, JsonNode idempotency_key) implements SchemaTypes.GeneratedResponse {
            public Disclosure {
                Objects.requireNonNull(canonical_digest, "canonical_digest");
                Objects.requireNonNull(activity_type, "activity_type");
                Objects.requireNonNull(actor, "actor");
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(counterparties, "counterparties");
                Objects.requireNonNull(amounts, "amounts");
                Objects.requireNonNull(asset, "asset");
                Objects.requireNonNull(fee_limit, "fee_limit");
                Objects.requireNonNull(expiry, "expiry");
                Objects.requireNonNull(idempotency_key, "idempotency_key");
            }
        }
        public enum ErrorClass {
            TRANSPORTFAILURE("TransportFailure"),
            DEADLINE("Deadline"),
            PROTOCOLINCOMPATIBILITY("ProtocolIncompatibility"),
            UNAVAILABLECAPABILITY("UnavailableCapability"),
            COREREJECTION("CoreRejection"),
            VERIFICATIONFAILURE("VerificationFailure"),
            POLICYREFUSAL("PolicyRefusal"),
            CAPABILITYREFUSAL("CapabilityRefusal"),
            BUDGETREFUSAL("BudgetRefusal"),
            RATELIMIT("RateLimit"),
            IDEMPOTENCYCONFLICT("IdempotencyConflict"),
            INTERNALFAULT("InternalFault");
            private final String wire;
            ErrorClass(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static ErrorClass fromWire(String wire) {
                for (ErrorClass value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record EventDelivery(JsonNode event_identity, JsonNode event_bytes, JsonNode deduplication_id, JsonNode cursor, JsonNode receipt_reference) implements SchemaTypes.GeneratedEvent {
            public EventDelivery {
                Objects.requireNonNull(event_identity, "event_identity");
                Objects.requireNonNull(event_bytes, "event_bytes");
                Objects.requireNonNull(deduplication_id, "deduplication_id");
                Objects.requireNonNull(cursor, "cursor");
                Objects.requireNonNull(receipt_reference, "receipt_reference");
            }
        }
        public record Freshness(JsonNode chain_head, JsonNode latest_sealed_batch, JsonNode latest_finalised_checkpoint, JsonNode value_sequence, JsonNode relative_to) implements SchemaTypes.GeneratedResponse {
            public Freshness {
                Objects.requireNonNull(chain_head, "chain_head");
                Objects.requireNonNull(latest_sealed_batch, "latest_sealed_batch");
                Objects.requireNonNull(latest_finalised_checkpoint, "latest_finalised_checkpoint");
                Objects.requireNonNull(value_sequence, "value_sequence");
                Objects.requireNonNull(relative_to, "relative_to");
            }
        }
        public record GapNotice(JsonNode missing_first, JsonNode missing_last, JsonNode backfill_cursor, JsonNode backfill_attempted) implements SchemaTypes.GeneratedResponse {
            public GapNotice {
                Objects.requireNonNull(missing_first, "missing_first");
                Objects.requireNonNull(missing_last, "missing_last");
                Objects.requireNonNull(backfill_cursor, "backfill_cursor");
                Objects.requireNonNull(backfill_attempted, "backfill_attempted");
            }
        }
        public record HoldReason(JsonNode code, JsonNode message) implements SchemaTypes.GeneratedResponse {
            public HoldReason {
                Objects.requireNonNull(code, "code");
                Objects.requireNonNull(message, "message");
            }
        }
        public record IdempotentMutation(JsonNode request_id, JsonNode key, JsonNode body_digest, JsonNode operation) implements SchemaTypes.GeneratedResponse {
            public IdempotentMutation {
                Objects.requireNonNull(request_id, "request_id");
                Objects.requireNonNull(key, "key");
                Objects.requireNonNull(body_digest, "body_digest");
                Objects.requireNonNull(operation, "operation");
            }
        }
        public enum Level {
            UNVERIFIED("Unverified"),
            SEQUENCERSIGNED("SequencerSigned"),
            BATCHINCLUDED("BatchIncluded"),
            STATEPROVEN("StateProven"),
            CHECKPOINTFINALISED("CheckpointFinalised"),
            SETTLEMENTANCHORED("SettlementAnchored");
            private final String wire;
            Level(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static Level fromWire(String wire) {
                for (Level value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record ProgramActivitySelector(JsonNode activity_id, JsonNode requested_verification_level) implements SchemaTypes.GeneratedResponse {
            public ProgramActivitySelector {
                Objects.requireNonNull(activity_id, "activity_id");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
            }
        }
        public record ProgramCallBudget(JsonNode fuel, JsonNode fee_limit) implements SchemaTypes.GeneratedResponse {
            public ProgramCallBudget {
                Objects.requireNonNull(fuel, "fuel");
                Objects.requireNonNull(fee_limit, "fee_limit");
            }
        }
        public record ProgramCallRequest(JsonNode program_id, JsonNode calldata, AgentModels.ProgramCallBudget budget, JsonNode capabilities, JsonNode signed_activity) implements SchemaTypes.GeneratedResponse {
            public ProgramCallRequest {
                Objects.requireNonNull(program_id, "program_id");
                Objects.requireNonNull(calldata, "calldata");
                Objects.requireNonNull(budget, "budget");
                Objects.requireNonNull(capabilities, "capabilities");
                Objects.requireNonNull(signed_activity, "signed_activity");
            }
        }
        public enum ProgramCapability {
            STORAGE_READ("storage_read"),
            STORAGE_WRITE("storage_write"),
            TRANSFER("transfer"),
            EMIT_EVENT("emit_event"),
            COMPOSE("compose");
            private final String wire;
            ProgramCapability(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static ProgramCapability fromWire(String wire) {
                for (ProgramCapability value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record ProgramExecutionEvidence(JsonNode activity_id, JsonNode receipt, JsonNode terminal_payload, JsonNode call_graph, JsonNode authority) implements SchemaTypes.GeneratedResponse {
            public ProgramExecutionEvidence {
                Objects.requireNonNull(activity_id, "activity_id");
                Objects.requireNonNull(receipt, "receipt");
                Objects.requireNonNull(terminal_payload, "terminal_payload");
                Objects.requireNonNull(call_graph, "call_graph");
                Objects.requireNonNull(authority, "authority");
            }
        }
        public enum ProgramFailure {
            UNKNOWN_PROGRAM("unknown_program"),
            REENTRANCY("reentrancy"),
            DEPTH_EXCEEDED("depth_exceeded"),
            FANOUT_EXCEEDED("fanout_exceeded"),
            GUEST_REFUSED("guest_refused"),
            AUTHORITY("authority"),
            RESOURCE("resource"),
            RESPONSE("response"),
            FAULT("fault");
            private final String wire;
            ProgramFailure(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static ProgramFailure fromWire(String wire) {
                for (ProgramFailure value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public enum ProgramOutcome {
            COMPLETED("completed"),
            LEGACY_COMPLETED("legacy_completed"),
            REFUSED("refused");
            private final String wire;
            ProgramOutcome(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static ProgramOutcome fromWire(String wire) {
                for (ProgramOutcome value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record ProgramReceiptSelector(JsonNode idempotency_key, JsonNode expected_activity_id, JsonNode requested_verification_level) implements SchemaTypes.GeneratedResponse {
            public ProgramReceiptSelector {
                Objects.requireNonNull(idempotency_key, "idempotency_key");
                Objects.requireNonNull(expected_activity_id, "expected_activity_id");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
            }
        }
        public record ProgramSelector(JsonNode program_id, JsonNode requested_verification_level) implements SchemaTypes.GeneratedResponse {
            public ProgramSelector {
                Objects.requireNonNull(program_id, "program_id");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
            }
        }
        public record ProgramSimulation(JsonNode committed, JsonNode execution, JsonNode simulation_evidence) implements SchemaTypes.GeneratedResponse {
            public ProgramSimulation {
                Objects.requireNonNull(committed, "committed");
                Objects.requireNonNull(execution, "execution");
                Objects.requireNonNull(simulation_evidence, "simulation_evidence");
            }
        }
        public record ProgramSource(String status, String source_digest, String environment_digest, String pipeline, String expected_code_hash, String reproduced_artifact_digest) implements SchemaTypes.GeneratedResponse {
            public ProgramSource {
                Objects.requireNonNull(status, "status");
            }
        }
        public record ProgramSubmission(JsonNode state, JsonNode activity_id, JsonNode idempotency_key) implements SchemaTypes.GeneratedResponse {
            public ProgramSubmission {
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(activity_id, "activity_id");
                Objects.requireNonNull(idempotency_key, "idempotency_key");
            }
        }
        public record ReceiptReference(String kind, JsonNode value) implements SchemaTypes.GeneratedResponse {
            private static final Set<String> KINDS = Set.of("None", "Verified");
            public ReceiptReference {
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(value, "value");
                if (!KINDS.contains(kind)) throw PlatformSdkException.invalidArgument();
            }
        }
        public enum Retriability {
            TERMINAL("Terminal"),
            RETRIABLE("Retriable");
            private final String wire;
            Retriability(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static Retriability fromWire(String wire) {
                for (Retriability value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record SessionContext(JsonNode tenant, JsonNode agent_did, JsonNode authority_ref, JsonNode permitted_activity_types, JsonNode expiry, JsonNode client, JsonNode policy_version) implements SchemaTypes.GeneratedResponse {
            public SessionContext {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(agent_did, "agent_did");
                Objects.requireNonNull(authority_ref, "authority_ref");
                Objects.requireNonNull(permitted_activity_types, "permitted_activity_types");
                Objects.requireNonNull(expiry, "expiry");
                Objects.requireNonNull(client, "client");
                Objects.requireNonNull(policy_version, "policy_version");
            }
        }
        public enum SettlementDomain {
            PAXEER("Paxeer");
            private final String wire;
            SettlementDomain(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static SettlementDomain fromWire(String wire) {
                for (SettlementDomain value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record StructuredActivityDisclosure(JsonNode canonical_digest, JsonNode activity_type, JsonNode actor, JsonNode authority, JsonNode counterparties, JsonNode amounts, JsonNode asset, JsonNode fee_limit, JsonNode expiry, JsonNode idempotency_key) implements SchemaTypes.GeneratedResponse {
            public StructuredActivityDisclosure {
                Objects.requireNonNull(canonical_digest, "canonical_digest");
                Objects.requireNonNull(activity_type, "activity_type");
                Objects.requireNonNull(actor, "actor");
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(counterparties, "counterparties");
                Objects.requireNonNull(amounts, "amounts");
                Objects.requireNonNull(asset, "asset");
                Objects.requireNonNull(fee_limit, "fee_limit");
                Objects.requireNonNull(expiry, "expiry");
                Objects.requireNonNull(idempotency_key, "idempotency_key");
            }
        }
        public record SubmissionState(String kind, JsonNode value) implements SchemaTypes.GeneratedResponse {
            private static final Set<String> KINDS = Set.of("Prepared", "Signed", "Queued", "Submitted", "Acknowledged", "Unknown", "Executed", "Failed", "Expired");
            public SubmissionState {
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(value, "value");
                if (!KINDS.contains(kind)) throw PlatformSdkException.invalidArgument();
            }
        }
        public record SubscriptionFilter(JsonNode agents, JsonNode accounts, JsonNode activity_types, JsonNode modules, JsonNode assets, JsonNode counterparties, JsonNode result_classes) implements SchemaTypes.GeneratedResponse {
            public SubscriptionFilter {
                Objects.requireNonNull(agents, "agents");
                Objects.requireNonNull(accounts, "accounts");
                Objects.requireNonNull(activity_types, "activity_types");
                Objects.requireNonNull(modules, "modules");
                Objects.requireNonNull(assets, "assets");
                Objects.requireNonNull(counterparties, "counterparties");
                Objects.requireNonNull(result_classes, "result_classes");
            }
        }
        public record SubscriptionScope(JsonNode tenant, JsonNode agent, JsonNode capability) implements SchemaTypes.GeneratedResponse {
            public SubscriptionScope {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(agent, "agent");
                Objects.requireNonNull(capability, "capability");
            }
        }
        public record Transition(JsonNode from, JsonNode to, JsonNode cause, JsonNode at) implements SchemaTypes.GeneratedResponse {
            public Transition {
                Objects.requireNonNull(from, "from");
                Objects.requireNonNull(to, "to");
                Objects.requireNonNull(cause, "cause");
                Objects.requireNonNull(at, "at");
            }
        }
        public record TruncationNotice(JsonNode requested_first, JsonNode oldest_available, JsonNode resume_cursor) implements SchemaTypes.GeneratedResponse {
            public TruncationNotice {
                Objects.requireNonNull(requested_first, "requested_first");
                Objects.requireNonNull(oldest_available, "oldest_available");
                Objects.requireNonNull(resume_cursor, "resume_cursor");
            }
        }
        public record VerificationStatus(String kind, JsonNode value) implements SchemaTypes.GeneratedResponse {
            private static final Set<String> KINDS = Set.of("Achieved", "Unverified");
            public VerificationStatus {
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(value, "value");
                if (!KINDS.contains(kind)) throw PlatformSdkException.invalidArgument();
            }
        }
        public record VerifiedProgramDiscovery(JsonNode program_id, JsonNode lifecycle, JsonNode version, JsonNode code_hash, JsonNode abi_version, JsonNode receipt_digest, JsonNode state_root, JsonNode observed_sequence, JsonNode observed_at, JsonNode valid_through, JsonNode verification) implements SchemaTypes.GeneratedResponse {
            public VerifiedProgramDiscovery {
                Objects.requireNonNull(program_id, "program_id");
                Objects.requireNonNull(lifecycle, "lifecycle");
                Objects.requireNonNull(version, "version");
                Objects.requireNonNull(code_hash, "code_hash");
                Objects.requireNonNull(abi_version, "abi_version");
                Objects.requireNonNull(receipt_digest, "receipt_digest");
                Objects.requireNonNull(state_root, "state_root");
                Objects.requireNonNull(observed_sequence, "observed_sequence");
                Objects.requireNonNull(observed_at, "observed_at");
                Objects.requireNonNull(valid_through, "valid_through");
                Objects.requireNonNull(verification, "verification");
            }
        }
        public record VerifiedProgramInterface(JsonNode program_id, JsonNode version, JsonNode code_hash, JsonNode abi_version, @JsonProperty("interface") JsonNode interface_, JsonNode interface_digest, JsonNode receipt_digest, JsonNode state_root, JsonNode observed_sequence, JsonNode observed_at, JsonNode valid_through, AgentModels.ProgramSource source, JsonNode verification) implements SchemaTypes.GeneratedResponse {
            public VerifiedProgramInterface {
                Objects.requireNonNull(program_id, "program_id");
                Objects.requireNonNull(version, "version");
                Objects.requireNonNull(code_hash, "code_hash");
                Objects.requireNonNull(abi_version, "abi_version");
                Objects.requireNonNull(interface_, "interface");
                Objects.requireNonNull(interface_digest, "interface_digest");
                Objects.requireNonNull(receipt_digest, "receipt_digest");
                Objects.requireNonNull(state_root, "state_root");
                Objects.requireNonNull(observed_sequence, "observed_sequence");
                Objects.requireNonNull(observed_at, "observed_at");
                Objects.requireNonNull(valid_through, "valid_through");
                Objects.requireNonNull(source, "source");
                Objects.requireNonNull(verification, "verification");
            }
        }
        public record VerifiedRead(JsonNode value, JsonNode achieved_verification_level, JsonNode freshness) implements SchemaTypes.GeneratedResponse {
            public VerifiedRead {
                Objects.requireNonNull(value, "value");
                Objects.requireNonNull(achieved_verification_level, "achieved_verification_level");
                Objects.requireNonNull(freshness, "freshness");
            }
        }
        public record VersionRequest(BigInteger request_id, AgentModels.ContractVersion supported) implements SchemaTypes.GeneratedResponse {
            public VersionRequest {
                Objects.requireNonNull(request_id, "request_id");
                SchemaTypes.protocolU64(request_id);
                Objects.requireNonNull(supported, "supported");
            }
        }
        public record VersionResponse(BigInteger request_id, AgentModels.ContractVersion contract, long node_interface_major) implements SchemaTypes.GeneratedResponse {
            public VersionResponse {
                Objects.requireNonNull(request_id, "request_id");
                SchemaTypes.protocolU64(request_id);
                Objects.requireNonNull(contract, "contract");
            }
        }
    }
    public static final class HumanModels {
        private HumanModels() {}
        public record AccountBalance(String account_id, HumanModels.Money money, HumanModels.VerificationLevel verification, HumanModels.ProtocolFreshness freshness, List<HumanModels.EvidenceRef> evidence) implements SchemaTypes.GeneratedResponse {
            public AccountBalance {
                Objects.requireNonNull(account_id, "account_id");
                Objects.requireNonNull(money, "money");
                Objects.requireNonNull(verification, "verification");
                Objects.requireNonNull(freshness, "freshness");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public record AccountCreateRequest(String email, String display_name) implements SchemaTypes.GeneratedResponse {
            public AccountCreateRequest {
                Objects.requireNonNull(email, "email");
                Objects.requireNonNull(display_name, "display_name");
            }
        }
        public record AccountCreation(String account_id, HumanModels.Journey onboarding) implements SchemaTypes.GeneratedResponse {
            public AccountCreation {
                Objects.requireNonNull(account_id, "account_id");
                Objects.requireNonNull(onboarding, "onboarding");
            }
        }
        public record ActivityEntry(String entry_id, HumanModels.ActivityEntryKind kind, HumanModels.JourneyState state, String state_copy_key, String summary_copy_key, String occurred_at, HumanModels.Money money, HumanModels.MoneyDirection direction, String agent_id, String journey_id, String approval_id) implements SchemaTypes.GeneratedResponse {
            public ActivityEntry {
                Objects.requireNonNull(entry_id, "entry_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                Objects.requireNonNull(summary_copy_key, "summary_copy_key");
                Objects.requireNonNull(occurred_at, "occurred_at");
            }
        }
        public record ActivityEntryDetail(String entry_id, HumanModels.ActivityEntryKind kind, HumanModels.JourneyState state, String state_copy_key, String summary_copy_key, String occurred_at, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, HumanModels.Money money, HumanModels.Money fees, HumanModels.MoneyDirection direction, String agent_id, String journey_id, String approval_id) implements SchemaTypes.GeneratedResponse {
            public ActivityEntryDetail {
                Objects.requireNonNull(entry_id, "entry_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                Objects.requireNonNull(summary_copy_key, "summary_copy_key");
                Objects.requireNonNull(occurred_at, "occurred_at");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public enum ActivityEntryKind {
            DEPOSIT("deposit"),
            WITHDRAWAL("withdrawal"),
            MOVEMENT("movement"),
            AGENT_ACTION("agent-action"),
            APPROVAL("approval"),
            SECURITY_EVENT("security-event");
            private final String wire;
            ActivityEntryKind(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static ActivityEntryKind fromWire(String wire) {
                for (ActivityEntryKind value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record ActivityFilter(List<HumanModels.ActivityEntryKind> kinds, String agent_id, String from, String to) implements SchemaTypes.GeneratedResponse {
            public ActivityFilter {
                if (kinds != null) kinds = List.copyOf(kinds);
            }
        }
        public record ActivityGroup(String month, HumanModels.Money subtotal_in, HumanModels.Money subtotal_out, List<HumanModels.ActivityEntry> entries) implements SchemaTypes.GeneratedResponse {
            public ActivityGroup {
                Objects.requireNonNull(month, "month");
                Objects.requireNonNull(subtotal_in, "subtotal_in");
                Objects.requireNonNull(subtotal_out, "subtotal_out");
                entries = List.copyOf(Objects.requireNonNull(entries, "entries"));
            }
        }
        public record ActivityPage(List<HumanModels.ActivityGroup> groups, String next_cursor, HumanModels.ActivityFilter filter) implements SchemaTypes.GeneratedResponse {
            public ActivityPage {
                groups = List.copyOf(Objects.requireNonNull(groups, "groups"));
                Objects.requireNonNull(next_cursor, "next_cursor");
                Objects.requireNonNull(filter, "filter");
            }
        }
        public record ActivityQueryRequest(String cursor, HumanModels.ActivityFilter filter, Long page_limit) implements SchemaTypes.GeneratedResponse {
        }
        public record Agent(String agent_id, String name, String purpose, HumanModels.AgentState state, String state_copy_key, HumanModels.SpendLimit limit, HumanModels.AgentSpend spend, List<HumanModels.EvidenceRef> evidence, String created_at, String updated_at, String creation_journey_id) implements SchemaTypes.GeneratedResponse {
            public Agent {
                Objects.requireNonNull(agent_id, "agent_id");
                Objects.requireNonNull(name, "name");
                Objects.requireNonNull(purpose, "purpose");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                Objects.requireNonNull(limit, "limit");
                Objects.requireNonNull(spend, "spend");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(created_at, "created_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public record AgentArchiveRequest(String confirm_name) implements SchemaTypes.GeneratedResponse {
            public AgentArchiveRequest {
                Objects.requireNonNull(confirm_name, "confirm_name");
            }
        }
        public record AgentCreateRequest(String name, String purpose, HumanModels.Money monthly_limit) implements SchemaTypes.GeneratedResponse {
            public AgentCreateRequest {
                Objects.requireNonNull(name, "name");
                Objects.requireNonNull(purpose, "purpose");
                Objects.requireNonNull(monthly_limit, "monthly_limit");
            }
        }
        public record AgentLimitRequest(HumanModels.Money monthly_limit) implements SchemaTypes.GeneratedResponse {
            public AgentLimitRequest {
                Objects.requireNonNull(monthly_limit, "monthly_limit");
            }
        }
        public record AgentPage(List<HumanModels.Agent> agents, String next_cursor) implements SchemaTypes.GeneratedResponse {
            public AgentPage {
                agents = List.copyOf(Objects.requireNonNull(agents, "agents"));
                Objects.requireNonNull(next_cursor, "next_cursor");
            }
        }
        public record AgentReclaimRequest(HumanModels.Money money) implements SchemaTypes.GeneratedResponse {
            public AgentReclaimRequest {
                Objects.requireNonNull(money, "money");
            }
        }
        public record AgentSpend(String period_start, String period_end, HumanModels.Money spent, HumanModels.Money remaining, HumanModels.VerificationLevel verification, String reconciliation_copy_key) implements SchemaTypes.GeneratedResponse {
            public AgentSpend {
                Objects.requireNonNull(period_start, "period_start");
                Objects.requireNonNull(period_end, "period_end");
                Objects.requireNonNull(spent, "spent");
                Objects.requireNonNull(remaining, "remaining");
                Objects.requireNonNull(verification, "verification");
            }
        }
        public enum AgentState {
            CREATING("creating"),
            ACTIVE("active"),
            PAUSED("paused"),
            ARCHIVING("archiving"),
            ARCHIVED("archived");
            private final String wire;
            AgentState(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static AgentState fromWire(String wire) {
                for (AgentState value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record ApiError(HumanModels.ErrorCode code, String copy_key, HumanModels.Retriability retry, Long retry_after_ms, String field) implements SchemaTypes.GeneratedResponse {
            public ApiError {
                Objects.requireNonNull(code, "code");
                Objects.requireNonNull(copy_key, "copy_key");
                Objects.requireNonNull(retry, "retry");
            }
        }
        public record ApprovalApproveRequest(String step_up_evidence) implements SchemaTypes.GeneratedResponse {
            public ApprovalApproveRequest {
                Objects.requireNonNull(step_up_evidence, "step_up_evidence");
            }
        }
        public record ApprovalDecision(String approval_id, HumanModels.ApprovalState state, String state_copy_key, boolean money_moved, String moved_copy_key, List<HumanModels.EvidenceRef> evidence) implements SchemaTypes.GeneratedResponse {
            public ApprovalDecision {
                Objects.requireNonNull(approval_id, "approval_id");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                Objects.requireNonNull(moved_copy_key, "moved_copy_key");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public record ApprovalDetail(String approval_id, String agent_id, String agent_name, HumanModels.ApprovalState state, String state_copy_key, String reason_copy_key, HumanModels.ApprovalFacts facts, HumanModels.VerifiedMoney budget_remaining_after, String created_at, List<HumanModels.EvidenceRef> evidence) implements SchemaTypes.GeneratedResponse {
            public ApprovalDetail {
                Objects.requireNonNull(approval_id, "approval_id");
                Objects.requireNonNull(agent_id, "agent_id");
                Objects.requireNonNull(agent_name, "agent_name");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                Objects.requireNonNull(reason_copy_key, "reason_copy_key");
                Objects.requireNonNull(facts, "facts");
                Objects.requireNonNull(budget_remaining_after, "budget_remaining_after");
                Objects.requireNonNull(created_at, "created_at");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public record ApprovalFacts(HumanModels.Money amount, String counterparty, String asset, HumanModels.Money fees, String expires_at) implements SchemaTypes.GeneratedResponse {
            public ApprovalFacts {
                Objects.requireNonNull(amount, "amount");
                Objects.requireNonNull(counterparty, "counterparty");
                Objects.requireNonNull(asset, "asset");
                Objects.requireNonNull(fees, "fees");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public record ApprovalPage(List<HumanModels.ApprovalSummary> approvals, String next_cursor) implements SchemaTypes.GeneratedResponse {
            public ApprovalPage {
                approvals = List.copyOf(Objects.requireNonNull(approvals, "approvals"));
                Objects.requireNonNull(next_cursor, "next_cursor");
            }
        }
        public enum ApprovalState {
            PENDING("pending"),
            APPROVED("approved"),
            REJECTED("rejected"),
            EXPIRED("expired"),
            DEFECTIVE("defective");
            private final String wire;
            ApprovalState(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static ApprovalState fromWire(String wire) {
                for (ApprovalState value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record ApprovalSummary(String approval_id, String agent_id, String agent_name, String counterparty, HumanModels.Money amount, String reason_copy_key, String expires_at, HumanModels.ApprovalState state, HumanModels.VerifiedMoney budget_remaining_after) implements SchemaTypes.GeneratedResponse {
            public ApprovalSummary {
                Objects.requireNonNull(approval_id, "approval_id");
                Objects.requireNonNull(agent_id, "agent_id");
                Objects.requireNonNull(agent_name, "agent_name");
                Objects.requireNonNull(counterparty, "counterparty");
                Objects.requireNonNull(amount, "amount");
                Objects.requireNonNull(reason_copy_key, "reason_copy_key");
                Objects.requireNonNull(expires_at, "expires_at");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(budget_remaining_after, "budget_remaining_after");
            }
        }
        public record AuthenticatorDisable(HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedResponse {
            public AuthenticatorDisable {
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record AuthenticatorMethod(String authenticator_id, String label, String enabled_at, String last_used_at) implements SchemaTypes.GeneratedResponse {
            public AuthenticatorMethod {
                Objects.requireNonNull(authenticator_id, "authenticator_id");
                Objects.requireNonNull(label, "label");
                Objects.requireNonNull(enabled_at, "enabled_at");
            }
        }
        public record AuthenticatorSetupBegin(String label, HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedResponse {
            public AuthenticatorSetupBegin {
                Objects.requireNonNull(label, "label");
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record AuthenticatorSetupChallenge(String setup_id, HumanModels.TimedSecret secret, HumanModels.TimedSecret otpauth_uri, String expires_at) implements SchemaTypes.GeneratedResponse {
            public AuthenticatorSetupChallenge {
                Objects.requireNonNull(setup_id, "setup_id");
                Objects.requireNonNull(secret, "secret");
                Objects.requireNonNull(otpauth_uri, "otpauth_uri");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public record AuthenticatorSetupFinish(String code, HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedResponse {
            public AuthenticatorSetupFinish {
                Objects.requireNonNull(code, "code");
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record AuthenticatorSetupResult(HumanModels.AuthenticatorMethod method, HumanModels.BackupCodeSet backup_codes) implements SchemaTypes.GeneratedResponse {
            public AuthenticatorSetupResult {
                Objects.requireNonNull(method, "method");
                Objects.requireNonNull(backup_codes, "backup_codes");
            }
        }
        public record AuthenticatorStatus(List<HumanModels.AuthenticatorMethod> methods, long backup_codes_remaining) implements SchemaTypes.GeneratedResponse {
            public AuthenticatorStatus {
                methods = List.copyOf(Objects.requireNonNull(methods, "methods"));
            }
        }
        public record BackupCodeRotation(HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedResponse {
            public BackupCodeRotation {
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record BackupCodeSet(List<String> codes, String remask_at, boolean copyable) implements SchemaTypes.GeneratedResponse {
            public BackupCodeSet {
                codes = List.copyOf(Objects.requireNonNull(codes, "codes"));
                Objects.requireNonNull(remask_at, "remask_at");
            }
        }
        public record BindingRebindAction(HumanModels.BindingStatement binding, String confirms) implements SchemaTypes.GeneratedResponse {
            public BindingRebindAction {
                Objects.requireNonNull(binding, "binding");
                Objects.requireNonNull(confirms, "confirms");
            }
        }
        public enum BindingState {
            NONE("none"),
            BINDING("binding"),
            BOUND("bound"),
            REBINDING("rebinding");
            private final String wire;
            BindingState(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static BindingState fromWire(String wire) {
                for (BindingState value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record BindingStatement(String statement, String address, String expires_at) implements SchemaTypes.GeneratedResponse {
            public BindingStatement {
                Objects.requireNonNull(statement, "statement");
                Objects.requireNonNull(address, "address");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public record BindingStatementRequest(String address) implements SchemaTypes.GeneratedResponse {
            public BindingStatementRequest {
                Objects.requireNonNull(address, "address");
            }
        }
        public record BindingSubmission(String address, String statement, String signature) implements SchemaTypes.GeneratedResponse {
            public BindingSubmission {
                Objects.requireNonNull(address, "address");
                Objects.requireNonNull(statement, "statement");
                Objects.requireNonNull(signature, "signature");
            }
        }
        public record ChannelPreference(boolean enabled, List<HumanModels.ClassToggle> classes) implements SchemaTypes.GeneratedResponse {
            public ChannelPreference {
                classes = List.copyOf(Objects.requireNonNull(classes, "classes"));
            }
        }
        public record ClassToggle(@JsonProperty("class") HumanModels.NotificationClass class_, boolean enabled) implements SchemaTypes.GeneratedResponse {
            public ClassToggle {
                Objects.requireNonNull(class_, "class");
            }
        }
        public record DepositConfirmRequest(String wallet_transaction, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedResponse {
            public DepositConfirmRequest {
                Objects.requireNonNull(wallet_transaction, "wallet_transaction");
            }
        }
        public record DepositStartRequest(HumanModels.Money money, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedResponse {
            public DepositStartRequest {
                Objects.requireNonNull(money, "money");
            }
        }
        public record Device(String device_id, String label, String platform) implements SchemaTypes.GeneratedResponse {
            public Device {
                Objects.requireNonNull(device_id, "device_id");
                Objects.requireNonNull(label, "label");
                Objects.requireNonNull(platform, "platform");
            }
        }
        public enum ErrorCode {
            UNAUTHENTICATED("unauthenticated"),
            SESSION_EXPIRED("session-expired"),
            STEP_UP_REQUIRED("step-up-required"),
            FORBIDDEN("forbidden"),
            NOT_FOUND("not-found"),
            INVALID_REQUEST("invalid-request"),
            CONFLICT("conflict"),
            RATE_LIMITED("rate-limited"),
            CURSOR_EXPIRED("cursor-expired"),
            UNAVAILABLE("unavailable"),
            UPSTREAM_DEGRADED("upstream-degraded"),
            CHALLENGE_EXPIRED("challenge-expired"),
            REFUSED_BY_POLICY("refused-by-policy"),
            REFUSED_BY_BUDGET("refused-by-budget"),
            REFUSED_BY_CAPABILITY("refused-by-capability"),
            REFUSED_BY_PROTOCOL("refused-by-protocol"),
            REFUSED_BY_LIMIT("refused-by-limit"),
            QUOTE_EXPIRED("quote-expired"),
            WALLET_NOT_BOUND("wallet-not-bound"),
            EXIT_UNAVAILABLE("exit-unavailable"),
            ALREADY_DECIDED("already-decided"),
            HOLD_EXPIRED("hold-expired"),
            HOLD_DEFECTIVE("hold-defective"),
            ARCHIVE_NEEDS_DISPOSITION("archive-needs-disposition"),
            CONFIRMATION_MISMATCH("confirmation-mismatch"),
            NOT_SUPPRESSIBLE("not-suppressible"),
            SUPPORT_UNAVAILABLE("support-unavailable"),
            SUPPORT_CONVERSATION_UNKNOWN("support-conversation-unknown"),
            SUPPORT_MESSAGE_UNKNOWN("support-message-unknown");
            private final String wire;
            ErrorCode(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static ErrorCode fromWire(String wire) {
                for (ErrorCode value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public enum EvidenceClass {
            LOCAL_JOURNEY_STATE("local-journey-state"),
            SUBMISSION_RECORD("submission-record"),
            LAYERX_RECEIPT("layerx-receipt"),
            CHECKPOINT_PROOF("checkpoint-proof"),
            PAXEER_FINALITY("paxeer-finality"),
            TYPED_REFUSAL("typed-refusal"),
            APPROVAL_HOLD("approval-hold"),
            WALLET_ACK("wallet-ack");
            private final String wire;
            EvidenceClass(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static EvidenceClass fromWire(String wire) {
                for (EvidenceClass value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record EvidenceMaterial(String evidence_id, @JsonProperty("class") HumanModels.EvidenceClass class_, HumanModels.VerificationLevel verification, String content_type, String bytes_base64, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedResponse {
            public EvidenceMaterial {
                Objects.requireNonNull(evidence_id, "evidence_id");
                Objects.requireNonNull(class_, "class");
                Objects.requireNonNull(verification, "verification");
                Objects.requireNonNull(content_type, "content_type");
                Objects.requireNonNull(bytes_base64, "bytes_base64");
            }
        }
        public record EvidenceRef(String evidence_id, @JsonProperty("class") HumanModels.EvidenceClass class_, HumanModels.VerificationLevel verification, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedResponse {
            public EvidenceRef {
                Objects.requireNonNull(evidence_id, "evidence_id");
                Objects.requireNonNull(class_, "class");
                Objects.requireNonNull(verification, "verification");
            }
        }
        public record ExitEligibility(boolean eligible, String copy_key, String withdraw_instead_path, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedResponse {
            public ExitEligibility {
                Objects.requireNonNull(copy_key, "copy_key");
            }
        }
        public record ExitStartRequest(String confirmation, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedResponse {
            public ExitStartRequest {
                Objects.requireNonNull(confirmation, "confirmation");
            }
        }
        public record ExportArtefact(String export_id, HumanModels.ExportKind kind, String download_path, String content_type, String created_at, List<HumanModels.EvidenceRef> evidence) implements SchemaTypes.GeneratedResponse {
            public ExportArtefact {
                Objects.requireNonNull(export_id, "export_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(download_path, "download_path");
                Objects.requireNonNull(content_type, "content_type");
                Objects.requireNonNull(created_at, "created_at");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public record ExportEvidenceRequest(HumanModels.ActivityFilter filter, List<String> entry_ids) implements SchemaTypes.GeneratedResponse {
            public ExportEvidenceRequest {
                if (entry_ids != null) entry_ids = List.copyOf(entry_ids);
            }
        }
        public enum ExportKind {
            STATEMENT("statement"),
            EVIDENCE_BUNDLE("evidence-bundle");
            private final String wire;
            ExportKind(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static ExportKind fromWire(String wire) {
                for (ExportKind value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record ExportStatementRequest(HumanModels.ActivityFilter filter) implements SchemaTypes.GeneratedResponse {
        }
        public record HomeSummary(HumanModels.AccountBalance balance, List<HumanModels.Agent> agents, List<HumanModels.ApprovalSummary> approvals, List<HumanModels.ActivityEntryDetail> recent_activity) implements SchemaTypes.GeneratedResponse {
            public HomeSummary {
                Objects.requireNonNull(balance, "balance");
                agents = List.copyOf(Objects.requireNonNull(agents, "agents"));
                approvals = List.copyOf(Objects.requireNonNull(approvals, "approvals"));
                recent_activity = List.copyOf(Objects.requireNonNull(recent_activity, "recent_activity"));
            }
        }
        public record Journey(String journey_id, HumanModels.JourneyKind kind, HumanModels.JourneyState state, String state_copy_key, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, String started_at, String updated_at, HumanModels.Refusal refusal, HumanModels.WalletSignRequest wallet_request) implements SchemaTypes.GeneratedResponse {
            public Journey {
                Objects.requireNonNull(journey_id, "journey_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(started_at, "started_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public enum JourneyKind {
            ONBOARDING("onboarding"),
            WALLET_BINDING("wallet-binding"),
            DEPOSIT("deposit"),
            WITHDRAW("withdraw"),
            EXIT("exit"),
            MOVE("move"),
            AGENT_CREATE("agent-create"),
            AGENT_FUND("agent-fund"),
            AGENT_PAUSE("agent-pause"),
            AGENT_RETIRE("agent-retire");
            private final String wire;
            JourneyKind(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static JourneyKind fromWire(String wire) {
                for (JourneyKind value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record JourneyPage(List<HumanModels.Journey> journeys, String next_cursor) implements SchemaTypes.GeneratedResponse {
            public JourneyPage {
                journeys = List.copyOf(Objects.requireNonNull(journeys, "journeys"));
                Objects.requireNonNull(next_cursor, "next_cursor");
            }
        }
        public record JourneyStage(String stage_id, String copy_key, HumanModels.JourneyState state, List<HumanModels.EvidenceRef> evidence) implements SchemaTypes.GeneratedResponse {
            public JourneyStage {
                Objects.requireNonNull(stage_id, "stage_id");
                Objects.requireNonNull(copy_key, "copy_key");
                Objects.requireNonNull(state, "state");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public enum JourneyState {
            GETTING_READY("getting-ready"),
            SENDING("sending"),
            PROCESSING("processing"),
            DONE("done"),
            DONE_FINALISED("done-finalised"),
            STILL_CHECKING("still-checking"),
            REFUSED("refused"),
            WAITING_FOR_YOU("waiting-for-you");
            private final String wire;
            JourneyState(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static JourneyState fromWire(String wire) {
                for (JourneyState value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record KeyChallenge(String agent_id, HumanModels.KeyChallengeKind kind, String delay_copy_key, long delay_seconds, String ready_at, List<HumanModels.EvidenceRef> evidence) implements SchemaTypes.GeneratedResponse {
            public KeyChallenge {
                Objects.requireNonNull(agent_id, "agent_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(delay_copy_key, "delay_copy_key");
                Objects.requireNonNull(ready_at, "ready_at");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public enum KeyChallengeKind {
            ROTATE("rotate"),
            RECOVER("recover");
            private final String wire;
            KeyChallengeKind(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static KeyChallengeKind fromWire(String wire) {
                for (KeyChallengeKind value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public enum LimitEnforcement {
            PROTOCOL("protocol"),
            APP("app");
            private final String wire;
            LimitEnforcement(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static LimitEnforcement fromWire(String wire) {
                for (LimitEnforcement value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record Money(ProtocolAmount amount, String currency) implements SchemaTypes.GeneratedResponse {
            public Money {
                Objects.requireNonNull(amount, "amount");
                Objects.requireNonNull(currency, "currency");
            }
        }
        public enum MoneyDirection {
            IN("in"),
            OUT("out");
            private final String wire;
            MoneyDirection(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static MoneyDirection fromWire(String wire) {
                for (MoneyDirection value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record MoveCommitRequest(String quote_id) implements SchemaTypes.GeneratedResponse {
            public MoveCommitRequest {
                Objects.requireNonNull(quote_id, "quote_id");
            }
        }
        public enum MoveMechanism {
            FUND("fund"),
            ALLOCATE("allocate"),
            RETURN("return"),
            TRANSFER("transfer");
            private final String wire;
            MoveMechanism(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static MoveMechanism fromWire(String wire) {
                for (MoveMechanism value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record MoveQuote(String quote_id, String description_copy_key, HumanModels.MoveMechanism mechanism, HumanModels.Money money, HumanModels.Money fee_estimate, HumanModels.Money fee_ceiling, String arrival_estimate, String expires_at, String irreversibility_copy_key) implements SchemaTypes.GeneratedResponse {
            public MoveQuote {
                Objects.requireNonNull(quote_id, "quote_id");
                Objects.requireNonNull(description_copy_key, "description_copy_key");
                Objects.requireNonNull(mechanism, "mechanism");
                Objects.requireNonNull(money, "money");
                Objects.requireNonNull(fee_estimate, "fee_estimate");
                Objects.requireNonNull(fee_ceiling, "fee_ceiling");
                Objects.requireNonNull(arrival_estimate, "arrival_estimate");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public record MoveQuoteRequest(String source, String destination, HumanModels.Money money) implements SchemaTypes.GeneratedResponse {
            public MoveQuoteRequest {
                Objects.requireNonNull(source, "source");
                Objects.requireNonNull(destination, "destination");
                Objects.requireNonNull(money, "money");
            }
        }
        public enum NotificationClass {
            APPROVAL_WAITING("approval-waiting"),
            MONEY_ARRIVED("money-arrived"),
            JOURNEY_FINISHED("journey-finished"),
            CLAIM_READY("claim-ready"),
            SECURITY_NEW_DEVICE("security-new-device"),
            SECURITY_RECOVERY("security-recovery"),
            SECURITY_WALLET_REBINDING("security-wallet-rebinding"),
            SECURITY_KEY_ROTATION("security-key-rotation"),
            SERVICE_STATUS("service-status");
            private final String wire;
            NotificationClass(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static NotificationClass fromWire(String wire) {
                for (NotificationClass value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public enum NotificationDetailLevel {
            FULL("full"),
            SUMMARY("summary"),
            MINIMAL("minimal");
            private final String wire;
            NotificationDetailLevel(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static NotificationDetailLevel fromWire(String wire) {
                for (NotificationDetailLevel value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record NotificationGroup(HumanModels.NotificationRecency recency, List<HumanModels.NotificationSummary> notifications) implements SchemaTypes.GeneratedResponse {
            public NotificationGroup {
                Objects.requireNonNull(recency, "recency");
                notifications = List.copyOf(Objects.requireNonNull(notifications, "notifications"));
            }
        }
        public record NotificationPage(List<HumanModels.NotificationGroup> groups, String next_cursor, long unread_count) implements SchemaTypes.GeneratedResponse {
            public NotificationPage {
                groups = List.copyOf(Objects.requireNonNull(groups, "groups"));
                Objects.requireNonNull(next_cursor, "next_cursor");
            }
        }
        public record NotificationPreferences(HumanModels.ChannelPreference push, HumanModels.ChannelPreference email, HumanModels.ChannelPreference in_app, HumanModels.NotificationDetailLevel detail) implements SchemaTypes.GeneratedResponse {
            public NotificationPreferences {
                Objects.requireNonNull(push, "push");
                Objects.requireNonNull(email, "email");
                Objects.requireNonNull(in_app, "in_app");
                Objects.requireNonNull(detail, "detail");
            }
        }
        public enum NotificationRecency {
            TODAY("today"),
            YESTERDAY("yesterday"),
            THIS_WEEK("this-week"),
            EARLIER("earlier");
            private final String wire;
            NotificationRecency(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static NotificationRecency fromWire(String wire) {
                for (NotificationRecency value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record NotificationSummary(String notification_id, @JsonProperty("class") HumanModels.NotificationClass class_, String title_copy_key, String body_copy_key, String deep_link, boolean read, String created_at, HumanModels.Money money, String agent_id, String approval_id, String journey_id, String action_copy_key) implements SchemaTypes.GeneratedResponse {
            public NotificationSummary {
                Objects.requireNonNull(notification_id, "notification_id");
                Objects.requireNonNull(class_, "class");
                Objects.requireNonNull(title_copy_key, "title_copy_key");
                Objects.requireNonNull(body_copy_key, "body_copy_key");
                Objects.requireNonNull(deep_link, "deep_link");
                Objects.requireNonNull(created_at, "created_at");
            }
        }
        public record Passkey(String passkey_id, String label, String created_at, String last_used_at) implements SchemaTypes.GeneratedResponse {
            public Passkey {
                Objects.requireNonNull(passkey_id, "passkey_id");
                Objects.requireNonNull(label, "label");
                Objects.requireNonNull(created_at, "created_at");
            }
        }
        public record PasskeyAssertion(String assertion_id, String passkey_id, String completed_at, String expires_at) implements SchemaTypes.GeneratedResponse {
            public PasskeyAssertion {
                Objects.requireNonNull(assertion_id, "assertion_id");
                Objects.requireNonNull(passkey_id, "passkey_id");
                Objects.requireNonNull(completed_at, "completed_at");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public record PasskeyAssertionBegin(String email) implements SchemaTypes.GeneratedResponse {
        }
        public record PasskeyAssertionChallenge(String assertion_id, String ceremony, String expires_at) implements SchemaTypes.GeneratedResponse {
            public PasskeyAssertionChallenge {
                Objects.requireNonNull(assertion_id, "assertion_id");
                Objects.requireNonNull(ceremony, "ceremony");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public record PasskeyAssertionFinish(String credential) implements SchemaTypes.GeneratedResponse {
            public PasskeyAssertionFinish {
                Objects.requireNonNull(credential, "credential");
            }
        }
        public record PasskeyList(List<HumanModels.Passkey> passkeys) implements SchemaTypes.GeneratedResponse {
            public PasskeyList {
                passkeys = List.copyOf(Objects.requireNonNull(passkeys, "passkeys"));
            }
        }
        public record PasskeyRegistrationBegin(String account_id) implements SchemaTypes.GeneratedResponse {
            public PasskeyRegistrationBegin {
                Objects.requireNonNull(account_id, "account_id");
            }
        }
        public record PasskeyRegistrationChallenge(String registration_id, String ceremony, String expires_at) implements SchemaTypes.GeneratedResponse {
            public PasskeyRegistrationChallenge {
                Objects.requireNonNull(registration_id, "registration_id");
                Objects.requireNonNull(ceremony, "ceremony");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public record PasskeyRegistrationFinish(String credential) implements SchemaTypes.GeneratedResponse {
            public PasskeyRegistrationFinish {
                Objects.requireNonNull(credential, "credential");
            }
        }
        public record Profile(String display_name, String avatar_url) implements SchemaTypes.GeneratedResponse {
            public Profile {
                Objects.requireNonNull(display_name, "display_name");
            }
        }
        public record ProfileUpdate(String display_name, String avatar_url) implements SchemaTypes.GeneratedResponse {
        }
        public record ProtocolFreshness(String observed_at, long age_seconds, String source_head, boolean within_bound, String checkpoint) implements SchemaTypes.GeneratedResponse {
            public ProtocolFreshness {
                Objects.requireNonNull(observed_at, "observed_at");
                Objects.requireNonNull(source_head, "source_head");
            }
        }
        public record RebindingSubmission(String address, String statement, String signature, HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedResponse {
            public RebindingSubmission {
                Objects.requireNonNull(address, "address");
                Objects.requireNonNull(statement, "statement");
                Objects.requireNonNull(signature, "signature");
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record Refusal(HumanModels.RefusedBy refused_by, String copy_key, boolean money_left, String change_path) implements SchemaTypes.GeneratedResponse {
            public Refusal {
                Objects.requireNonNull(refused_by, "refused_by");
                Objects.requireNonNull(copy_key, "copy_key");
            }
        }
        public enum RefusedBy {
            POLICY("policy"),
            BUDGET("budget"),
            CAPABILITY("capability"),
            PROTOCOL("protocol"),
            LIMIT("limit");
            private final String wire;
            RefusedBy(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static RefusedBy fromWire(String wire) {
                for (RefusedBy value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record ResponseEnvelope(boolean ok, JsonNode result, String trace) implements SchemaTypes.GeneratedResponse {
            public ResponseEnvelope {
                Objects.requireNonNull(result, "result");
                Objects.requireNonNull(trace, "trace");
            }
        }
        public enum Retriability {
            RETRIABLE("retriable"),
            RETRIABLE_AFTER("retriable-after"),
            STRUCTURAL("structural"),
            FINAL("final");
            private final String wire;
            Retriability(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static Retriability fromWire(String wire) {
                for (Retriability value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record SchemaVersion(long major, long minor) implements SchemaTypes.GeneratedResponse {
        }
        public record SecurityAction(String confirms) implements SchemaTypes.GeneratedResponse {
            public SecurityAction {
                Objects.requireNonNull(confirms, "confirms");
            }
        }
        public enum SecurityActionKind {
            ADD_PASSKEY("add-passkey"),
            REVOKE_PASSKEY("revoke-passkey"),
            REVOKE_SESSION("revoke-session"),
            REVOKE_ALL_SESSIONS("revoke-all-sessions"),
            ADD_AUTHENTICATOR("add-authenticator"),
            DISABLE_AUTHENTICATOR("disable-authenticator"),
            ROTATE_BACKUP_CODES("rotate-backup-codes"),
            REVEAL_RECOVERY_EVIDENCE("reveal-recovery-evidence");
            private final String wire;
            SecurityActionKind(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static SecurityActionKind fromWire(String wire) {
                for (SecurityActionKind value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record SecurityActionRequest(HumanModels.SecurityActionKind action, String target_id) implements SchemaTypes.GeneratedResponse {
            public SecurityActionRequest {
                Objects.requireNonNull(action, "action");
            }
        }
        public record SecurityPasskeyRegistrationBegin(String label, HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedResponse {
            public SecurityPasskeyRegistrationBegin {
                Objects.requireNonNull(label, "label");
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record SecurityPasskeyRegistrationFinish(String credential, HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedResponse {
            public SecurityPasskeyRegistrationFinish {
                Objects.requireNonNull(credential, "credential");
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record SecurityPasskeyRevocation(HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedResponse {
            public SecurityPasskeyRevocation {
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record SecurityRecoveryReveal(String evidence_id, HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedResponse {
            public SecurityRecoveryReveal {
                Objects.requireNonNull(evidence_id, "evidence_id");
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record SecuritySessionRevocation(HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedResponse {
            public SecuritySessionRevocation {
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record Session(String session_id, HumanModels.Device device, String opened_at, String last_active_at, boolean current) implements SchemaTypes.GeneratedResponse {
            public Session {
                Objects.requireNonNull(session_id, "session_id");
                Objects.requireNonNull(device, "device");
                Objects.requireNonNull(opened_at, "opened_at");
                Objects.requireNonNull(last_active_at, "last_active_at");
            }
        }
        public record SessionList(List<HumanModels.Session> sessions) implements SchemaTypes.GeneratedResponse {
            public SessionList {
                sessions = List.copyOf(Objects.requireNonNull(sessions, "sessions"));
            }
        }
        public record SessionOpenRequest(String assertion_id) implements SchemaTypes.GeneratedResponse {
            public SessionOpenRequest {
                Objects.requireNonNull(assertion_id, "assertion_id");
            }
        }
        public record SessionRevocation(List<String> revoked_session_ids, String revoked_at) implements SchemaTypes.GeneratedResponse {
            public SessionRevocation {
                revoked_session_ids = List.copyOf(Objects.requireNonNull(revoked_session_ids, "revoked_session_ids"));
                Objects.requireNonNull(revoked_at, "revoked_at");
            }
        }
        public enum SettlementDomain {
            PAXEER("paxeer");
            private final String wire;
            SettlementDomain(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static SettlementDomain fromWire(String wire) {
                for (SettlementDomain value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record SpendLimit(HumanModels.Money monthly, HumanModels.LimitEnforcement enforcement, String enforcement_copy_key) implements SchemaTypes.GeneratedResponse {
            public SpendLimit {
                Objects.requireNonNull(monthly, "monthly");
                Objects.requireNonNull(enforcement, "enforcement");
                Objects.requireNonNull(enforcement_copy_key, "enforcement_copy_key");
            }
        }
        public record StepUpChallenge(String challenge_id, String confirms, String ceremony, String expires_at) implements SchemaTypes.GeneratedResponse {
            public StepUpChallenge {
                Objects.requireNonNull(challenge_id, "challenge_id");
                Objects.requireNonNull(confirms, "confirms");
                Objects.requireNonNull(ceremony, "ceremony");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public record StepUpEvidence(String challenge_id, String confirms, String passkey_id, String completed_at, String expires_at) implements SchemaTypes.GeneratedResponse {
            public StepUpEvidence {
                Objects.requireNonNull(challenge_id, "challenge_id");
                Objects.requireNonNull(confirms, "confirms");
                Objects.requireNonNull(passkey_id, "passkey_id");
                Objects.requireNonNull(completed_at, "completed_at");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public record StepUpFinish(String credential) implements SchemaTypes.GeneratedResponse {
            public StepUpFinish {
                Objects.requireNonNull(credential, "credential");
            }
        }
        public record StepUpRequest(String confirms) implements SchemaTypes.GeneratedResponse {
            public StepUpRequest {
                Objects.requireNonNull(confirms, "confirms");
            }
        }
        public record StreamEvent(String cursor, HumanModels.StreamEventKind kind, String observed_at, HumanModels.Journey journey, HumanModels.ApprovalSummary approval, HumanModels.NotificationSummary notification) implements SchemaTypes.GeneratedEvent {
            public StreamEvent {
                Objects.requireNonNull(cursor, "cursor");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(observed_at, "observed_at");
            }
        }
        public enum StreamEventKind {
            JOURNEY_PROGRESS("journey-progress"),
            APPROVAL_CREATED("approval-created"),
            APPROVAL_APPROVED("approval-approved"),
            APPROVAL_REJECTED("approval-rejected"),
            APPROVAL_EXPIRED("approval-expired"),
            NOTIFICATION("notification");
            private final String wire;
            StreamEventKind(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static StreamEventKind fromWire(String wire) {
                for (StreamEventKind value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record StreamPage(List<HumanModels.StreamEvent> events, String next_cursor) implements SchemaTypes.GeneratedResponse {
            public StreamPage {
                events = List.copyOf(Objects.requireNonNull(events, "events"));
                Objects.requireNonNull(next_cursor, "next_cursor");
            }
        }
        public record StreamPosition(String cursor) implements SchemaTypes.GeneratedResponse {
            public StreamPosition {
                Objects.requireNonNull(cursor, "cursor");
            }
        }
        public enum SupportAuthor {
            YOU("you"),
            SUPPORT("support");
            private final String wire;
            SupportAuthor(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static SupportAuthor fromWire(String wire) {
                for (SupportAuthor value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record SupportConversation(String conversation_id, HumanModels.SupportShell shell, HumanModels.SupportConversationState state, String created_at, String updated_at, List<HumanModels.SupportMessage> messages, List<HumanModels.SupportFeedback> feedback, String trace_id) implements SchemaTypes.GeneratedResponse {
            public SupportConversation {
                Objects.requireNonNull(conversation_id, "conversation_id");
                Objects.requireNonNull(shell, "shell");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(created_at, "created_at");
                Objects.requireNonNull(updated_at, "updated_at");
                messages = List.copyOf(Objects.requireNonNull(messages, "messages"));
                feedback = List.copyOf(Objects.requireNonNull(feedback, "feedback"));
            }
        }
        public record SupportConversationPage(List<HumanModels.SupportConversation> conversations) implements SchemaTypes.GeneratedResponse {
            public SupportConversationPage {
                conversations = List.copyOf(Objects.requireNonNull(conversations, "conversations"));
            }
        }
        public enum SupportConversationState {
            WAITING_FOR_SUPPORT("waiting-for-support"),
            WAITING_FOR_YOU("waiting-for-you"),
            RESOLVED("resolved");
            private final String wire;
            SupportConversationState(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static SupportConversationState fromWire(String wire) {
                for (SupportConversationState value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record SupportConversationStatus(String conversation_id, HumanModels.SupportConversationState state, long unread_count, String updated_at) implements SchemaTypes.GeneratedResponse {
            public SupportConversationStatus {
                Objects.requireNonNull(conversation_id, "conversation_id");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public record SupportCreateRequest(String body, HumanModels.SupportShell shell, HumanModels.SupportTopic topic, String trace_id) implements SchemaTypes.GeneratedResponse {
            public SupportCreateRequest {
                Objects.requireNonNull(body, "body");
                Objects.requireNonNull(shell, "shell");
            }
        }
        public record SupportFeedback(String message_id, boolean helpful, String received_at) implements SchemaTypes.GeneratedResponse {
            public SupportFeedback {
                Objects.requireNonNull(message_id, "message_id");
                Objects.requireNonNull(received_at, "received_at");
            }
        }
        public record SupportFeedbackRequest(String message_id, boolean helpful) implements SchemaTypes.GeneratedResponse {
            public SupportFeedbackRequest {
                Objects.requireNonNull(message_id, "message_id");
            }
        }
        public record SupportMessage(String message_id, HumanModels.SupportAuthor author, String body, String sent_at, boolean read, HumanModels.SupportTopic topic) implements SchemaTypes.GeneratedResponse {
            public SupportMessage {
                Objects.requireNonNull(message_id, "message_id");
                Objects.requireNonNull(author, "author");
                Objects.requireNonNull(body, "body");
                Objects.requireNonNull(sent_at, "sent_at");
            }
        }
        public record SupportReadRequest(String through_message_id) implements SchemaTypes.GeneratedResponse {
            public SupportReadRequest {
                Objects.requireNonNull(through_message_id, "through_message_id");
            }
        }
        public record SupportReplyRequest(String body) implements SchemaTypes.GeneratedResponse {
            public SupportReplyRequest {
                Objects.requireNonNull(body, "body");
            }
        }
        public enum SupportShell {
            MOBILE("mobile"),
            DESKTOP("desktop");
            private final String wire;
            SupportShell(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static SupportShell fromWire(String wire) {
                for (SupportShell value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public enum SupportTopic {
            DEPOSIT("deposit"),
            WITHDRAWAL("withdrawal"),
            AGENTS("agents"),
            ACCOUNT("account"),
            REPORT("report");
            private final String wire;
            SupportTopic(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static SupportTopic fromWire(String wire) {
                for (SupportTopic value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record TimedSecret(String value, String remask_at, boolean copyable) implements SchemaTypes.GeneratedResponse {
            public TimedSecret {
                Objects.requireNonNull(value, "value");
                Objects.requireNonNull(remask_at, "remask_at");
            }
        }
        public enum VerificationLevel {
            UNVERIFIED("unverified"),
            RECEIPT_VERIFIED("receipt-verified"),
            CHECKPOINT_FINALISED("checkpoint-finalised"),
            PAXEER_FINALISED("paxeer-finalised");
            private final String wire;
            VerificationLevel(String wire) { this.wire = wire; }
            @JsonValue public String wire() { return wire; }
            @JsonCreator public static VerificationLevel fromWire(String wire) {
                for (VerificationLevel value : values()) if (value.wire.equals(wire)) return value;
                throw new IllegalArgumentException("unknown schema variant");
            }
        }
        public record VerifiedMoney(HumanModels.Money money, HumanModels.VerificationLevel verification) implements SchemaTypes.GeneratedResponse {
            public VerifiedMoney {
                Objects.requireNonNull(money, "money");
                Objects.requireNonNull(verification, "verification");
            }
        }
        public record VersionInfo(HumanModels.SchemaVersion schema, String service) implements SchemaTypes.GeneratedResponse {
            public VersionInfo {
                Objects.requireNonNull(schema, "schema");
                Objects.requireNonNull(service, "service");
            }
        }
        public record WalletBinding(HumanModels.BindingState state, String address, String bound_at, HumanModels.EvidenceRef evidence) implements SchemaTypes.GeneratedResponse {
            public WalletBinding {
                Objects.requireNonNull(state, "state");
            }
        }
        public record WalletSignRequest(String stage_id, String copy_key, String from_address, String to_sign_base64, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedResponse {
            public WalletSignRequest {
                Objects.requireNonNull(stage_id, "stage_id");
                Objects.requireNonNull(copy_key, "copy_key");
                Objects.requireNonNull(from_address, "from_address");
                Objects.requireNonNull(to_sign_base64, "to_sign_base64");
            }
        }
        public record WithdrawClaimRequest(String claim_signature, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedResponse {
            public WithdrawClaimRequest {
                Objects.requireNonNull(claim_signature, "claim_signature");
            }
        }
        public record WithdrawStartRequest(HumanModels.Money money, String destination, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedResponse {
            public WithdrawStartRequest {
                Objects.requireNonNull(money, "money");
                Objects.requireNonNull(destination, "destination");
            }
        }
    }
    public static final class AgentOperations {
        private AgentOperations() {}
        public record AgentRegisterRequest(JsonNode tenant, JsonNode agent_did, JsonNode authority_ref, JsonNode client, JsonNode policy_version) implements SchemaTypes.GeneratedRequest {
            public AgentRegisterRequest {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(agent_did, "agent_did");
                Objects.requireNonNull(authority_ref, "authority_ref");
                Objects.requireNonNull(client, "client");
                Objects.requireNonNull(policy_version, "policy_version");
            }
        }
        public record AgentRegisterResponse(JsonNode authority, JsonNode value) implements SchemaTypes.GeneratedResponse {
            public AgentRegisterResponse {
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(value, "value");
            }
        }
        public static final SchemaTypes.TypedOperation<AgentRegisterRequest, AgentRegisterResponse> AGENT_REGISTER = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "agent.register", true, AgentRegisterRequest.class, AgentRegisterResponse.class);
        public record ApprovalApproveRequest(JsonNode tenant, JsonNode approval_id, JsonNode idempotency_key) implements SchemaTypes.GeneratedRequest {
            public ApprovalApproveRequest {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(approval_id, "approval_id");
                Objects.requireNonNull(idempotency_key, "idempotency_key");
            }
        }
        public record ApprovalApproveResponse(JsonNode outcome, JsonNode submission_ref) implements SchemaTypes.GeneratedResponse {
            public ApprovalApproveResponse {
                Objects.requireNonNull(outcome, "outcome");
                Objects.requireNonNull(submission_ref, "submission_ref");
            }
        }
        public static final SchemaTypes.TypedOperation<ApprovalApproveRequest, ApprovalApproveResponse> APPROVAL_APPROVE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "approval.approve", true, ApprovalApproveRequest.class, ApprovalApproveResponse.class);
        public record ApprovalGetRequest(JsonNode tenant, JsonNode approval_id) implements SchemaTypes.GeneratedRequest {
            public ApprovalGetRequest {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(approval_id, "approval_id");
            }
        }
        public record ApprovalGetResponse(JsonNode approval_id, JsonNode tenant, JsonNode held_activity, JsonNode canonical_bytes_digest, JsonNode hold_reason, BigInteger created_at, BigInteger expires_at, JsonNode state) implements SchemaTypes.GeneratedResponse {
            public ApprovalGetResponse {
                Objects.requireNonNull(approval_id, "approval_id");
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(held_activity, "held_activity");
                Objects.requireNonNull(canonical_bytes_digest, "canonical_bytes_digest");
                Objects.requireNonNull(hold_reason, "hold_reason");
                Objects.requireNonNull(created_at, "created_at");
                SchemaTypes.protocolU64(created_at);
                Objects.requireNonNull(expires_at, "expires_at");
                SchemaTypes.protocolU64(expires_at);
                Objects.requireNonNull(state, "state");
            }
        }
        public static final SchemaTypes.TypedOperation<ApprovalGetRequest, ApprovalGetResponse> APPROVAL_GET = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "approval.get", false, ApprovalGetRequest.class, ApprovalGetResponse.class);
        public record ApprovalListRequest(JsonNode tenant, JsonNode cursor, JsonNode page_limit) implements SchemaTypes.GeneratedRequest {
            public ApprovalListRequest {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(cursor, "cursor");
                Objects.requireNonNull(page_limit, "page_limit");
            }
        }
        public record ApprovalListResponse(JsonNode approvals, JsonNode next_cursor) implements SchemaTypes.GeneratedResponse {
            public ApprovalListResponse {
                Objects.requireNonNull(approvals, "approvals");
                Objects.requireNonNull(next_cursor, "next_cursor");
            }
        }
        public static final SchemaTypes.TypedOperation<ApprovalListRequest, ApprovalListResponse> APPROVAL_LIST = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "approval.list", false, ApprovalListRequest.class, ApprovalListResponse.class);
        public record ApprovalRejectRequest(JsonNode tenant, JsonNode approval_id, JsonNode idempotency_key, JsonNode reason) implements SchemaTypes.GeneratedRequest {
            public ApprovalRejectRequest {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(approval_id, "approval_id");
                Objects.requireNonNull(idempotency_key, "idempotency_key");
                Objects.requireNonNull(reason, "reason");
            }
        }
        public record ApprovalRejectResponse(JsonNode outcome) implements SchemaTypes.GeneratedResponse {
            public ApprovalRejectResponse {
                Objects.requireNonNull(outcome, "outcome");
            }
        }
        public static final SchemaTypes.TypedOperation<ApprovalRejectRequest, ApprovalRejectResponse> APPROVAL_REJECT = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "approval.reject", true, ApprovalRejectRequest.class, ApprovalRejectResponse.class);
        public record AvailabilityFetchRequest(JsonNode selector, JsonNode requested_verification_level, JsonNode maximum_bytes, JsonNode maximum_chunks, JsonNode deadline) implements SchemaTypes.GeneratedRequest {
            public AvailabilityFetchRequest {
                Objects.requireNonNull(selector, "selector");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
                Objects.requireNonNull(maximum_bytes, "maximum_bytes");
                Objects.requireNonNull(maximum_chunks, "maximum_chunks");
                Objects.requireNonNull(deadline, "deadline");
            }
        }
        public record AvailabilityFetchResponse(JsonNode value, JsonNode achieved_verification_level, JsonNode freshness) implements SchemaTypes.GeneratedResponse {
            public AvailabilityFetchResponse {
                Objects.requireNonNull(value, "value");
                Objects.requireNonNull(achieved_verification_level, "achieved_verification_level");
                Objects.requireNonNull(freshness, "freshness");
            }
        }
        public static final SchemaTypes.TypedOperation<AvailabilityFetchRequest, AvailabilityFetchResponse> AVAILABILITY_FETCH = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "availability.fetch", false, AvailabilityFetchRequest.class, AvailabilityFetchResponse.class);
        public record BudgetCreateRequest(JsonNode tenant, JsonNode agent_did, JsonNode asset, JsonNode limit, JsonNode enforcement, JsonNode expiry) implements SchemaTypes.GeneratedRequest {
            public BudgetCreateRequest {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(agent_did, "agent_did");
                Objects.requireNonNull(asset, "asset");
                Objects.requireNonNull(limit, "limit");
                Objects.requireNonNull(enforcement, "enforcement");
                Objects.requireNonNull(expiry, "expiry");
            }
        }
        public record BudgetCreateResponse(JsonNode authority, JsonNode value) implements SchemaTypes.GeneratedResponse {
            public BudgetCreateResponse {
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(value, "value");
            }
        }
        public static final SchemaTypes.TypedOperation<BudgetCreateRequest, BudgetCreateResponse> BUDGET_CREATE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "budget.create", true, BudgetCreateRequest.class, BudgetCreateResponse.class);
        public record BudgetFundRequest(JsonNode tenant, JsonNode agent_did, JsonNode budget_id, JsonNode amount) implements SchemaTypes.GeneratedRequest {
            public BudgetFundRequest {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(agent_did, "agent_did");
                Objects.requireNonNull(budget_id, "budget_id");
                Objects.requireNonNull(amount, "amount");
            }
        }
        public record BudgetFundResponse(JsonNode authority, JsonNode value) implements SchemaTypes.GeneratedResponse {
            public BudgetFundResponse {
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(value, "value");
            }
        }
        public static final SchemaTypes.TypedOperation<BudgetFundRequest, BudgetFundResponse> BUDGET_FUND = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "budget.fund", true, BudgetFundRequest.class, BudgetFundResponse.class);
        public record BudgetListRequest(JsonNode tenant, JsonNode agent_did) implements SchemaTypes.GeneratedRequest {
            public BudgetListRequest {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(agent_did, "agent_did");
            }
        }
        public record BudgetListResponse(JsonNode authority, JsonNode value) implements SchemaTypes.GeneratedResponse {
            public BudgetListResponse {
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(value, "value");
            }
        }
        public static final SchemaTypes.TypedOperation<BudgetListRequest, BudgetListResponse> BUDGET_LIST = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "budget.list", false, BudgetListRequest.class, BudgetListResponse.class);
        public record BudgetReconciliationRequest(JsonNode tenant, JsonNode agent_did, JsonNode budget_id) implements SchemaTypes.GeneratedRequest {
            public BudgetReconciliationRequest {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(agent_did, "agent_did");
                Objects.requireNonNull(budget_id, "budget_id");
            }
        }
        public record BudgetReconciliationResponse(JsonNode authority, JsonNode value) implements SchemaTypes.GeneratedResponse {
            public BudgetReconciliationResponse {
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(value, "value");
            }
        }
        public static final SchemaTypes.TypedOperation<BudgetReconciliationRequest, BudgetReconciliationResponse> BUDGET_RECONCILIATION = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "budget.reconciliation", false, BudgetReconciliationRequest.class, BudgetReconciliationResponse.class);
        public record BudgetRevokeRequest(JsonNode tenant, JsonNode agent_did, JsonNode budget_id) implements SchemaTypes.GeneratedRequest {
            public BudgetRevokeRequest {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(agent_did, "agent_did");
                Objects.requireNonNull(budget_id, "budget_id");
            }
        }
        public record BudgetRevokeResponse(JsonNode authority, JsonNode value) implements SchemaTypes.GeneratedResponse {
            public BudgetRevokeResponse {
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(value, "value");
            }
        }
        public static final SchemaTypes.TypedOperation<BudgetRevokeRequest, BudgetRevokeResponse> BUDGET_REVOKE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "budget.revoke", true, BudgetRevokeRequest.class, BudgetRevokeResponse.class);
        public record CapabilityAttenuateRequest(JsonNode tenant, JsonNode agent_did, JsonNode parent_id, JsonNode dimensions) implements SchemaTypes.GeneratedRequest {
            public CapabilityAttenuateRequest {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(agent_did, "agent_did");
                Objects.requireNonNull(parent_id, "parent_id");
                Objects.requireNonNull(dimensions, "dimensions");
            }
        }
        public record CapabilityAttenuateResponse(JsonNode authority, JsonNode value) implements SchemaTypes.GeneratedResponse {
            public CapabilityAttenuateResponse {
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(value, "value");
            }
        }
        public static final SchemaTypes.TypedOperation<CapabilityAttenuateRequest, CapabilityAttenuateResponse> CAPABILITY_ATTENUATE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "capability.attenuate", true, CapabilityAttenuateRequest.class, CapabilityAttenuateResponse.class);
        public record CapabilityCreateRequest(JsonNode tenant, JsonNode agent_did, JsonNode dimensions) implements SchemaTypes.GeneratedRequest {
            public CapabilityCreateRequest {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(agent_did, "agent_did");
                Objects.requireNonNull(dimensions, "dimensions");
            }
        }
        public record CapabilityCreateResponse(JsonNode authority, JsonNode value) implements SchemaTypes.GeneratedResponse {
            public CapabilityCreateResponse {
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(value, "value");
            }
        }
        public static final SchemaTypes.TypedOperation<CapabilityCreateRequest, CapabilityCreateResponse> CAPABILITY_CREATE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "capability.create", true, CapabilityCreateRequest.class, CapabilityCreateResponse.class);
        public record CapabilityListRequest(JsonNode tenant, JsonNode agent_did) implements SchemaTypes.GeneratedRequest {
            public CapabilityListRequest {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(agent_did, "agent_did");
            }
        }
        public record CapabilityListResponse(JsonNode authority, JsonNode value) implements SchemaTypes.GeneratedResponse {
            public CapabilityListResponse {
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(value, "value");
            }
        }
        public static final SchemaTypes.TypedOperation<CapabilityListRequest, CapabilityListResponse> CAPABILITY_LIST = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "capability.list", false, CapabilityListRequest.class, CapabilityListResponse.class);
        public record CapabilityRevokeRequest(JsonNode tenant, JsonNode agent_did, JsonNode capability_id) implements SchemaTypes.GeneratedRequest {
            public CapabilityRevokeRequest {
                Objects.requireNonNull(tenant, "tenant");
                Objects.requireNonNull(agent_did, "agent_did");
                Objects.requireNonNull(capability_id, "capability_id");
            }
        }
        public record CapabilityRevokeResponse(JsonNode authority, JsonNode value) implements SchemaTypes.GeneratedResponse {
            public CapabilityRevokeResponse {
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(value, "value");
            }
        }
        public static final SchemaTypes.TypedOperation<CapabilityRevokeRequest, CapabilityRevokeResponse> CAPABILITY_REVOKE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "capability.revoke", true, CapabilityRevokeRequest.class, CapabilityRevokeResponse.class);
        public record ExportOfflineRequest(JsonNode fact_set, JsonNode requested_verification_level) implements SchemaTypes.GeneratedRequest {
            public ExportOfflineRequest {
                Objects.requireNonNull(fact_set, "fact_set");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
            }
        }
        public record ExportOfflineResponse(JsonNode value, JsonNode achieved_verification_level, JsonNode freshness) implements SchemaTypes.GeneratedResponse {
            public ExportOfflineResponse {
                Objects.requireNonNull(value, "value");
                Objects.requireNonNull(achieved_verification_level, "achieved_verification_level");
                Objects.requireNonNull(freshness, "freshness");
            }
        }
        public static final SchemaTypes.TypedOperation<ExportOfflineRequest, ExportOfflineResponse> EXPORT_OFFLINE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "export.offline", false, ExportOfflineRequest.class, ExportOfflineResponse.class);
        public record PrepareRequest(JsonNode actor, JsonNode authority, JsonNode account_sequence, JsonNode timestamp_bound, JsonNode idempotency_key, JsonNode fee_limit, JsonNode payload, JsonNode payload_hash) implements SchemaTypes.GeneratedRequest {
            public PrepareRequest {
                Objects.requireNonNull(actor, "actor");
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(account_sequence, "account_sequence");
                Objects.requireNonNull(timestamp_bound, "timestamp_bound");
                Objects.requireNonNull(idempotency_key, "idempotency_key");
                Objects.requireNonNull(fee_limit, "fee_limit");
                Objects.requireNonNull(payload, "payload");
                Objects.requireNonNull(payload_hash, "payload_hash");
            }
        }
        public record PrepareResponse(JsonNode preparation_ref, JsonNode unsigned_canonical_bytes, JsonNode signing_preimage, JsonNode disclosure, JsonNode expiry) implements SchemaTypes.GeneratedResponse {
            public PrepareResponse {
                Objects.requireNonNull(preparation_ref, "preparation_ref");
                Objects.requireNonNull(unsigned_canonical_bytes, "unsigned_canonical_bytes");
                Objects.requireNonNull(signing_preimage, "signing_preimage");
                Objects.requireNonNull(disclosure, "disclosure");
                Objects.requireNonNull(expiry, "expiry");
            }
        }
        public static final SchemaTypes.TypedOperation<PrepareRequest, PrepareResponse> PREPARE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "prepare", true, PrepareRequest.class, PrepareResponse.class);
        public record ProgramActivityRequest(JsonNode activity_id, JsonNode requested_verification_level) implements SchemaTypes.GeneratedRequest {
            public ProgramActivityRequest {
                Objects.requireNonNull(activity_id, "activity_id");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
            }
        }
        public record ProgramActivityResponse(JsonNode state, JsonNode activity_id, JsonNode idempotency_key) implements SchemaTypes.GeneratedResponse {
            public ProgramActivityResponse {
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(activity_id, "activity_id");
                Objects.requireNonNull(idempotency_key, "idempotency_key");
            }
        }
        public static final SchemaTypes.TypedOperation<ProgramActivityRequest, ProgramActivityResponse> PROGRAM_ACTIVITY = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "program.activity", false, ProgramActivityRequest.class, ProgramActivityResponse.class);
        public record ProgramCallRequest(JsonNode program_id, JsonNode calldata, AgentModels.ProgramCallBudget budget, JsonNode capabilities, JsonNode signed_activity) implements SchemaTypes.GeneratedRequest {
            public ProgramCallRequest {
                Objects.requireNonNull(program_id, "program_id");
                Objects.requireNonNull(calldata, "calldata");
                Objects.requireNonNull(budget, "budget");
                Objects.requireNonNull(capabilities, "capabilities");
                Objects.requireNonNull(signed_activity, "signed_activity");
            }
        }
        public record ProgramCallResponse(JsonNode state, JsonNode activity_id, JsonNode idempotency_key) implements SchemaTypes.GeneratedResponse {
            public ProgramCallResponse {
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(activity_id, "activity_id");
                Objects.requireNonNull(idempotency_key, "idempotency_key");
            }
        }
        public static final SchemaTypes.TypedOperation<ProgramCallRequest, ProgramCallResponse> PROGRAM_CALL = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "program.call", true, ProgramCallRequest.class, ProgramCallResponse.class);
        public record ProgramDiscoverRequest(JsonNode program_id, JsonNode requested_verification_level) implements SchemaTypes.GeneratedRequest {
            public ProgramDiscoverRequest {
                Objects.requireNonNull(program_id, "program_id");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
            }
        }
        public record ProgramDiscoverResponse(JsonNode program_id, JsonNode lifecycle, JsonNode version, JsonNode code_hash, JsonNode abi_version, JsonNode receipt_digest, JsonNode state_root, JsonNode observed_sequence, JsonNode observed_at, JsonNode valid_through, JsonNode verification) implements SchemaTypes.GeneratedResponse {
            public ProgramDiscoverResponse {
                Objects.requireNonNull(program_id, "program_id");
                Objects.requireNonNull(lifecycle, "lifecycle");
                Objects.requireNonNull(version, "version");
                Objects.requireNonNull(code_hash, "code_hash");
                Objects.requireNonNull(abi_version, "abi_version");
                Objects.requireNonNull(receipt_digest, "receipt_digest");
                Objects.requireNonNull(state_root, "state_root");
                Objects.requireNonNull(observed_sequence, "observed_sequence");
                Objects.requireNonNull(observed_at, "observed_at");
                Objects.requireNonNull(valid_through, "valid_through");
                Objects.requireNonNull(verification, "verification");
            }
        }
        public static final SchemaTypes.TypedOperation<ProgramDiscoverRequest, ProgramDiscoverResponse> PROGRAM_DISCOVER = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "program.discover", false, ProgramDiscoverRequest.class, ProgramDiscoverResponse.class);
        public record ProgramInterfaceRequest(JsonNode program_id, JsonNode requested_verification_level) implements SchemaTypes.GeneratedRequest {
            public ProgramInterfaceRequest {
                Objects.requireNonNull(program_id, "program_id");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
            }
        }
        public record ProgramInterfaceResponse(JsonNode program_id, JsonNode version, JsonNode code_hash, JsonNode abi_version, @JsonProperty("interface") JsonNode interface_, JsonNode interface_digest, JsonNode receipt_digest, JsonNode state_root, JsonNode observed_sequence, JsonNode observed_at, JsonNode valid_through, AgentModels.ProgramSource source, JsonNode verification) implements SchemaTypes.GeneratedResponse {
            public ProgramInterfaceResponse {
                Objects.requireNonNull(program_id, "program_id");
                Objects.requireNonNull(version, "version");
                Objects.requireNonNull(code_hash, "code_hash");
                Objects.requireNonNull(abi_version, "abi_version");
                Objects.requireNonNull(interface_, "interface");
                Objects.requireNonNull(interface_digest, "interface_digest");
                Objects.requireNonNull(receipt_digest, "receipt_digest");
                Objects.requireNonNull(state_root, "state_root");
                Objects.requireNonNull(observed_sequence, "observed_sequence");
                Objects.requireNonNull(observed_at, "observed_at");
                Objects.requireNonNull(valid_through, "valid_through");
                Objects.requireNonNull(source, "source");
                Objects.requireNonNull(verification, "verification");
            }
        }
        public static final SchemaTypes.TypedOperation<ProgramInterfaceRequest, ProgramInterfaceResponse> PROGRAM_INTERFACE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "program.interface", false, ProgramInterfaceRequest.class, ProgramInterfaceResponse.class);
        public record ProgramReceiptRequest(JsonNode idempotency_key, JsonNode expected_activity_id, JsonNode requested_verification_level) implements SchemaTypes.GeneratedRequest {
            public ProgramReceiptRequest {
                Objects.requireNonNull(idempotency_key, "idempotency_key");
                Objects.requireNonNull(expected_activity_id, "expected_activity_id");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
            }
        }
        public record ProgramReceiptResponse(JsonNode state, JsonNode activity_id, JsonNode idempotency_key) implements SchemaTypes.GeneratedResponse {
            public ProgramReceiptResponse {
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(activity_id, "activity_id");
                Objects.requireNonNull(idempotency_key, "idempotency_key");
            }
        }
        public static final SchemaTypes.TypedOperation<ProgramReceiptRequest, ProgramReceiptResponse> PROGRAM_RECEIPT = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "program.receipt", true, ProgramReceiptRequest.class, ProgramReceiptResponse.class);
        public record ProgramSimulateRequest(JsonNode program_id, JsonNode calldata, AgentModels.ProgramCallBudget budget, JsonNode capabilities, JsonNode signed_activity) implements SchemaTypes.GeneratedRequest {
            public ProgramSimulateRequest {
                Objects.requireNonNull(program_id, "program_id");
                Objects.requireNonNull(calldata, "calldata");
                Objects.requireNonNull(budget, "budget");
                Objects.requireNonNull(capabilities, "capabilities");
                Objects.requireNonNull(signed_activity, "signed_activity");
            }
        }
        public record ProgramSimulateResponse(JsonNode committed, JsonNode execution, JsonNode simulation_evidence) implements SchemaTypes.GeneratedResponse {
            public ProgramSimulateResponse {
                Objects.requireNonNull(committed, "committed");
                Objects.requireNonNull(execution, "execution");
                Objects.requireNonNull(simulation_evidence, "simulation_evidence");
            }
        }
        public static final SchemaTypes.TypedOperation<ProgramSimulateRequest, ProgramSimulateResponse> PROGRAM_SIMULATE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "program.simulate", false, ProgramSimulateRequest.class, ProgramSimulateResponse.class);
        public record ProjectRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record ProjectResponse(JsonNode value) implements SchemaTypes.GeneratedResponse {
            @JsonCreator(mode = JsonCreator.Mode.DELEGATING)
            public ProjectResponse {
                Objects.requireNonNull(value, "value");
            }
            @JsonValue public JsonNode wireValue() { return value; }
        }
        public static final SchemaTypes.TypedOperation<ProjectRequest, ProjectResponse> PROJECT = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "project", false, ProjectRequest.class, ProjectResponse.class);
        public record ReadAccountRequest(JsonNode account, JsonNode requested_verification_level) implements SchemaTypes.GeneratedRequest {
            public ReadAccountRequest {
                Objects.requireNonNull(account, "account");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
            }
        }
        public record ReadAccountResponse(JsonNode value, JsonNode achieved_verification_level, JsonNode freshness) implements SchemaTypes.GeneratedResponse {
            public ReadAccountResponse {
                Objects.requireNonNull(value, "value");
                Objects.requireNonNull(achieved_verification_level, "achieved_verification_level");
                Objects.requireNonNull(freshness, "freshness");
            }
        }
        public static final SchemaTypes.TypedOperation<ReadAccountRequest, ReadAccountResponse> READ_ACCOUNT = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "read.account", false, ReadAccountRequest.class, ReadAccountResponse.class);
        public record ReadBalanceRequest(JsonNode account, JsonNode asset, JsonNode requested_verification_level) implements SchemaTypes.GeneratedRequest {
            public ReadBalanceRequest {
                Objects.requireNonNull(account, "account");
                Objects.requireNonNull(asset, "asset");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
            }
        }
        public record ReadBalanceResponse(JsonNode value, JsonNode achieved_verification_level, JsonNode freshness) implements SchemaTypes.GeneratedResponse {
            public ReadBalanceResponse {
                Objects.requireNonNull(value, "value");
                Objects.requireNonNull(achieved_verification_level, "achieved_verification_level");
                Objects.requireNonNull(freshness, "freshness");
            }
        }
        public static final SchemaTypes.TypedOperation<ReadBalanceRequest, ReadBalanceResponse> READ_BALANCE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "read.balance", false, ReadBalanceRequest.class, ReadBalanceResponse.class);
        public record ReadBatchRequest(JsonNode batch, JsonNode requested_verification_level) implements SchemaTypes.GeneratedRequest {
            public ReadBatchRequest {
                Objects.requireNonNull(batch, "batch");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
            }
        }
        public record ReadBatchResponse(JsonNode value, JsonNode achieved_verification_level, JsonNode freshness) implements SchemaTypes.GeneratedResponse {
            public ReadBatchResponse {
                Objects.requireNonNull(value, "value");
                Objects.requireNonNull(achieved_verification_level, "achieved_verification_level");
                Objects.requireNonNull(freshness, "freshness");
            }
        }
        public static final SchemaTypes.TypedOperation<ReadBatchRequest, ReadBatchResponse> READ_BATCH = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "read.batch", false, ReadBatchRequest.class, ReadBatchResponse.class);
        public record ReadCheckpointRequest(JsonNode checkpoint, JsonNode requested_verification_level) implements SchemaTypes.GeneratedRequest {
            public ReadCheckpointRequest {
                Objects.requireNonNull(checkpoint, "checkpoint");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
            }
        }
        public record ReadCheckpointResponse(JsonNode value, JsonNode achieved_verification_level, JsonNode freshness) implements SchemaTypes.GeneratedResponse {
            public ReadCheckpointResponse {
                Objects.requireNonNull(value, "value");
                Objects.requireNonNull(achieved_verification_level, "achieved_verification_level");
                Objects.requireNonNull(freshness, "freshness");
            }
        }
        public static final SchemaTypes.TypedOperation<ReadCheckpointRequest, ReadCheckpointResponse> READ_CHECKPOINT = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "read.checkpoint", false, ReadCheckpointRequest.class, ReadCheckpointResponse.class);
        public record ReadHistoryRequest(JsonNode range, JsonNode cursor, JsonNode page_limit, JsonNode requested_verification_level) implements SchemaTypes.GeneratedRequest {
            public ReadHistoryRequest {
                Objects.requireNonNull(range, "range");
                Objects.requireNonNull(cursor, "cursor");
                Objects.requireNonNull(page_limit, "page_limit");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
            }
        }
        public record ReadHistoryResponse(JsonNode value, JsonNode achieved_verification_level, JsonNode freshness) implements SchemaTypes.GeneratedResponse {
            public ReadHistoryResponse {
                Objects.requireNonNull(value, "value");
                Objects.requireNonNull(achieved_verification_level, "achieved_verification_level");
                Objects.requireNonNull(freshness, "freshness");
            }
        }
        public static final SchemaTypes.TypedOperation<ReadHistoryRequest, ReadHistoryResponse> READ_HISTORY = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "read.history", false, ReadHistoryRequest.class, ReadHistoryResponse.class);
        public record ReadModuleStateRequest(JsonNode module, JsonNode key, JsonNode requested_verification_level) implements SchemaTypes.GeneratedRequest {
            public ReadModuleStateRequest {
                Objects.requireNonNull(module, "module");
                Objects.requireNonNull(key, "key");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
            }
        }
        public record ReadModuleStateResponse(JsonNode value, JsonNode achieved_verification_level, JsonNode freshness) implements SchemaTypes.GeneratedResponse {
            public ReadModuleStateResponse {
                Objects.requireNonNull(value, "value");
                Objects.requireNonNull(achieved_verification_level, "achieved_verification_level");
                Objects.requireNonNull(freshness, "freshness");
            }
        }
        public static final SchemaTypes.TypedOperation<ReadModuleStateRequest, ReadModuleStateResponse> READ_MODULE_STATE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "read.module_state", false, ReadModuleStateRequest.class, ReadModuleStateResponse.class);
        public record ReadProofBundleRequest(JsonNode target, JsonNode requested_verification_level) implements SchemaTypes.GeneratedRequest {
            public ReadProofBundleRequest {
                Objects.requireNonNull(target, "target");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
            }
        }
        public record ReadProofBundleResponse(JsonNode value, JsonNode achieved_verification_level, JsonNode freshness) implements SchemaTypes.GeneratedResponse {
            public ReadProofBundleResponse {
                Objects.requireNonNull(value, "value");
                Objects.requireNonNull(achieved_verification_level, "achieved_verification_level");
                Objects.requireNonNull(freshness, "freshness");
            }
        }
        public static final SchemaTypes.TypedOperation<ReadProofBundleRequest, ReadProofBundleResponse> READ_PROOF_BUNDLE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "read.proof_bundle", false, ReadProofBundleRequest.class, ReadProofBundleResponse.class);
        public record SessionCloseRequest(JsonNode session_id, JsonNode context) implements SchemaTypes.GeneratedRequest {
            public SessionCloseRequest {
                Objects.requireNonNull(session_id, "session_id");
                Objects.requireNonNull(context, "context");
            }
        }
        public record SessionCloseResponse(JsonNode authority, JsonNode value) implements SchemaTypes.GeneratedResponse {
            public SessionCloseResponse {
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(value, "value");
            }
        }
        public static final SchemaTypes.TypedOperation<SessionCloseRequest, SessionCloseResponse> SESSION_CLOSE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "session.close", true, SessionCloseRequest.class, SessionCloseResponse.class);
        public record SessionListRequest(JsonNode context) implements SchemaTypes.GeneratedRequest {
            public SessionListRequest {
                Objects.requireNonNull(context, "context");
            }
        }
        public record SessionListResponse(JsonNode authority, JsonNode value) implements SchemaTypes.GeneratedResponse {
            public SessionListResponse {
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(value, "value");
            }
        }
        public static final SchemaTypes.TypedOperation<SessionListRequest, SessionListResponse> SESSION_LIST = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "session.list", false, SessionListRequest.class, SessionListResponse.class);
        public record SessionOpenRequest(JsonNode context) implements SchemaTypes.GeneratedRequest {
            public SessionOpenRequest {
                Objects.requireNonNull(context, "context");
            }
        }
        public record SessionOpenResponse(JsonNode authority, JsonNode value) implements SchemaTypes.GeneratedResponse {
            public SessionOpenResponse {
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(value, "value");
            }
        }
        public static final SchemaTypes.TypedOperation<SessionOpenRequest, SessionOpenResponse> SESSION_OPEN = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "session.open", true, SessionOpenRequest.class, SessionOpenResponse.class);
        public record SessionRefreshRequest(JsonNode session_id, JsonNode context) implements SchemaTypes.GeneratedRequest {
            public SessionRefreshRequest {
                Objects.requireNonNull(session_id, "session_id");
                Objects.requireNonNull(context, "context");
            }
        }
        public record SessionRefreshResponse(JsonNode authority, JsonNode value) implements SchemaTypes.GeneratedResponse {
            public SessionRefreshResponse {
                Objects.requireNonNull(authority, "authority");
                Objects.requireNonNull(value, "value");
            }
        }
        public static final SchemaTypes.TypedOperation<SessionRefreshRequest, SessionRefreshResponse> SESSION_REFRESH = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "session.refresh", true, SessionRefreshRequest.class, SessionRefreshResponse.class);
        public record SignRequest(JsonNode preparation_ref, JsonNode signature) implements SchemaTypes.GeneratedRequest {
            public SignRequest {
                Objects.requireNonNull(preparation_ref, "preparation_ref");
                Objects.requireNonNull(signature, "signature");
            }
        }
        public record SignResponse(JsonNode value) implements SchemaTypes.GeneratedResponse {
            @JsonCreator(mode = JsonCreator.Mode.DELEGATING)
            public SignResponse {
                Objects.requireNonNull(value, "value");
            }
            @JsonValue public JsonNode wireValue() { return value; }
        }
        public static final SchemaTypes.TypedOperation<SignRequest, SignResponse> SIGN = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "sign", true, SignRequest.class, SignResponse.class);
        public record SubmitRequest(JsonNode preparation_ref, JsonNode signature) implements SchemaTypes.GeneratedRequest {
            public SubmitRequest {
                Objects.requireNonNull(preparation_ref, "preparation_ref");
                Objects.requireNonNull(signature, "signature");
            }
        }
        public record SubmitResponse(JsonNode value) implements SchemaTypes.GeneratedResponse {
            @JsonCreator(mode = JsonCreator.Mode.DELEGATING)
            public SubmitResponse {
                Objects.requireNonNull(value, "value");
            }
            @JsonValue public JsonNode wireValue() { return value; }
        }
        public static final SchemaTypes.TypedOperation<SubmitRequest, SubmitResponse> SUBMIT = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "submit", true, SubmitRequest.class, SubmitResponse.class);
        public record SubscriptionAcknowledgeRequest(JsonNode scope, JsonNode subscription_id, JsonNode cursor) implements SchemaTypes.GeneratedRequest {
            public SubscriptionAcknowledgeRequest {
                Objects.requireNonNull(scope, "scope");
                Objects.requireNonNull(subscription_id, "subscription_id");
                Objects.requireNonNull(cursor, "cursor");
            }
        }
        public record SubscriptionAcknowledgeResponse(JsonNode value) implements SchemaTypes.GeneratedResponse {
            @JsonCreator(mode = JsonCreator.Mode.DELEGATING)
            public SubscriptionAcknowledgeResponse {
                Objects.requireNonNull(value, "value");
            }
            @JsonValue public JsonNode wireValue() { return value; }
        }
        public static final SchemaTypes.TypedOperation<SubscriptionAcknowledgeRequest, SubscriptionAcknowledgeResponse> SUBSCRIPTION_ACKNOWLEDGE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "subscription.acknowledge", true, SubscriptionAcknowledgeRequest.class, SubscriptionAcknowledgeResponse.class);
        public record SubscriptionCreateRequest(JsonNode scope, JsonNode filter, JsonNode start, JsonNode delivery_target) implements SchemaTypes.GeneratedRequest {
            public SubscriptionCreateRequest {
                Objects.requireNonNull(scope, "scope");
                Objects.requireNonNull(filter, "filter");
                Objects.requireNonNull(start, "start");
                Objects.requireNonNull(delivery_target, "delivery_target");
            }
        }
        public record SubscriptionCreateResponse(JsonNode value) implements SchemaTypes.GeneratedResponse {
            @JsonCreator(mode = JsonCreator.Mode.DELEGATING)
            public SubscriptionCreateResponse {
                Objects.requireNonNull(value, "value");
            }
            @JsonValue public JsonNode wireValue() { return value; }
        }
        public static final SchemaTypes.TypedOperation<SubscriptionCreateRequest, SubscriptionCreateResponse> SUBSCRIPTION_CREATE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "subscription.create", true, SubscriptionCreateRequest.class, SubscriptionCreateResponse.class);
        public record SubscriptionDeleteRequest(JsonNode scope, JsonNode subscription_id) implements SchemaTypes.GeneratedRequest {
            public SubscriptionDeleteRequest {
                Objects.requireNonNull(scope, "scope");
                Objects.requireNonNull(subscription_id, "subscription_id");
            }
        }
        public record SubscriptionDeleteResponse(JsonNode value) implements SchemaTypes.GeneratedResponse {
            @JsonCreator(mode = JsonCreator.Mode.DELEGATING)
            public SubscriptionDeleteResponse {
                Objects.requireNonNull(value, "value");
            }
            @JsonValue public JsonNode wireValue() { return value; }
        }
        public static final SchemaTypes.TypedOperation<SubscriptionDeleteRequest, SubscriptionDeleteResponse> SUBSCRIPTION_DELETE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "subscription.delete", true, SubscriptionDeleteRequest.class, SubscriptionDeleteResponse.class);
        public record SubscriptionHealthRequest(JsonNode scope, JsonNode subscription_id) implements SchemaTypes.GeneratedRequest {
            public SubscriptionHealthRequest {
                Objects.requireNonNull(scope, "scope");
                Objects.requireNonNull(subscription_id, "subscription_id");
            }
        }
        public record SubscriptionHealthResponse(JsonNode value) implements SchemaTypes.GeneratedResponse {
            @JsonCreator(mode = JsonCreator.Mode.DELEGATING)
            public SubscriptionHealthResponse {
                Objects.requireNonNull(value, "value");
            }
            @JsonValue public JsonNode wireValue() { return value; }
        }
        public static final SchemaTypes.TypedOperation<SubscriptionHealthRequest, SubscriptionHealthResponse> SUBSCRIPTION_HEALTH = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "subscription.health", false, SubscriptionHealthRequest.class, SubscriptionHealthResponse.class);
        public record SubscriptionListRequest(JsonNode scope) implements SchemaTypes.GeneratedRequest {
            public SubscriptionListRequest {
                Objects.requireNonNull(scope, "scope");
            }
        }
        public record SubscriptionListResponse(JsonNode value) implements SchemaTypes.GeneratedResponse {
            @JsonCreator(mode = JsonCreator.Mode.DELEGATING)
            public SubscriptionListResponse {
                Objects.requireNonNull(value, "value");
            }
            @JsonValue public JsonNode wireValue() { return value; }
        }
        public static final SchemaTypes.TypedOperation<SubscriptionListRequest, SubscriptionListResponse> SUBSCRIPTION_LIST = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "subscription.list", false, SubscriptionListRequest.class, SubscriptionListResponse.class);
        public record SubscriptionPauseRequest(JsonNode scope, JsonNode subscription_id) implements SchemaTypes.GeneratedRequest {
            public SubscriptionPauseRequest {
                Objects.requireNonNull(scope, "scope");
                Objects.requireNonNull(subscription_id, "subscription_id");
            }
        }
        public record SubscriptionPauseResponse(JsonNode value) implements SchemaTypes.GeneratedResponse {
            @JsonCreator(mode = JsonCreator.Mode.DELEGATING)
            public SubscriptionPauseResponse {
                Objects.requireNonNull(value, "value");
            }
            @JsonValue public JsonNode wireValue() { return value; }
        }
        public static final SchemaTypes.TypedOperation<SubscriptionPauseRequest, SubscriptionPauseResponse> SUBSCRIPTION_PAUSE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "subscription.pause", true, SubscriptionPauseRequest.class, SubscriptionPauseResponse.class);
        public record SubscriptionResumeRequest(JsonNode scope, JsonNode subscription_id) implements SchemaTypes.GeneratedRequest {
            public SubscriptionResumeRequest {
                Objects.requireNonNull(scope, "scope");
                Objects.requireNonNull(subscription_id, "subscription_id");
            }
        }
        public record SubscriptionResumeResponse(JsonNode value) implements SchemaTypes.GeneratedResponse {
            @JsonCreator(mode = JsonCreator.Mode.DELEGATING)
            public SubscriptionResumeResponse {
                Objects.requireNonNull(value, "value");
            }
            @JsonValue public JsonNode wireValue() { return value; }
        }
        public static final SchemaTypes.TypedOperation<SubscriptionResumeRequest, SubscriptionResumeResponse> SUBSCRIPTION_RESUME = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "subscription.resume", true, SubscriptionResumeRequest.class, SubscriptionResumeResponse.class);
        public record TrackRequest(JsonNode submission_ref) implements SchemaTypes.GeneratedRequest {
            public TrackRequest {
                Objects.requireNonNull(submission_ref, "submission_ref");
            }
        }
        public record TrackResponse(JsonNode state, JsonNode evidence, JsonNode verification_level, JsonNode transitions) implements SchemaTypes.GeneratedResponse {
            public TrackResponse {
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(evidence, "evidence");
                Objects.requireNonNull(verification_level, "verification_level");
                Objects.requireNonNull(transitions, "transitions");
            }
        }
        public static final SchemaTypes.TypedOperation<TrackRequest, TrackResponse> TRACK = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "track", false, TrackRequest.class, TrackResponse.class);
        public record WaitRequest(JsonNode submission_ref, JsonNode requested_verification_level, JsonNode deadline) implements SchemaTypes.GeneratedRequest {
            public WaitRequest {
                Objects.requireNonNull(submission_ref, "submission_ref");
                Objects.requireNonNull(requested_verification_level, "requested_verification_level");
                Objects.requireNonNull(deadline, "deadline");
            }
        }
        public record WaitResponse(JsonNode submission, JsonNode actual_verification_level, JsonNode deadline_elapsed) implements SchemaTypes.GeneratedResponse {
            public WaitResponse {
                Objects.requireNonNull(submission, "submission");
                Objects.requireNonNull(actual_verification_level, "actual_verification_level");
                Objects.requireNonNull(deadline_elapsed, "deadline_elapsed");
            }
        }
        public static final SchemaTypes.TypedOperation<WaitRequest, WaitResponse> WAIT = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.AGENT, "wait", false, WaitRequest.class, WaitResponse.class);
    }
    public static final class HumanOperations {
        private HumanOperations() {}
        public record AccountBalanceRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record AccountBalanceResponse(String account_id, HumanModels.Money money, HumanModels.VerificationLevel verification, HumanModels.ProtocolFreshness freshness, List<HumanModels.EvidenceRef> evidence) implements SchemaTypes.GeneratedResponse {
            public AccountBalanceResponse {
                Objects.requireNonNull(account_id, "account_id");
                Objects.requireNonNull(money, "money");
                Objects.requireNonNull(verification, "verification");
                Objects.requireNonNull(freshness, "freshness");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public static final SchemaTypes.TypedOperation<AccountBalanceRequest, AccountBalanceResponse> ACCOUNT_BALANCE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "account.balance", false, AccountBalanceRequest.class, AccountBalanceResponse.class);
        public record AccountCreateRequest(String email, String display_name) implements SchemaTypes.GeneratedRequest {
            public AccountCreateRequest {
                Objects.requireNonNull(email, "email");
                Objects.requireNonNull(display_name, "display_name");
            }
        }
        public record AccountCreateResponse(String account_id, HumanModels.Journey onboarding) implements SchemaTypes.GeneratedResponse {
            public AccountCreateResponse {
                Objects.requireNonNull(account_id, "account_id");
                Objects.requireNonNull(onboarding, "onboarding");
            }
        }
        public static final SchemaTypes.TypedOperation<AccountCreateRequest, AccountCreateResponse> ACCOUNT_CREATE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "account.create", true, AccountCreateRequest.class, AccountCreateResponse.class);
        public record ActivityEntryRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record ActivityEntryResponse(String entry_id, HumanModels.ActivityEntryKind kind, HumanModels.JourneyState state, String state_copy_key, String summary_copy_key, String occurred_at, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, HumanModels.Money money, HumanModels.Money fees, HumanModels.MoneyDirection direction, String agent_id, String journey_id, String approval_id) implements SchemaTypes.GeneratedResponse {
            public ActivityEntryResponse {
                Objects.requireNonNull(entry_id, "entry_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                Objects.requireNonNull(summary_copy_key, "summary_copy_key");
                Objects.requireNonNull(occurred_at, "occurred_at");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public static final SchemaTypes.TypedOperation<ActivityEntryRequest, ActivityEntryResponse> ACTIVITY_ENTRY = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "activity.entry", false, ActivityEntryRequest.class, ActivityEntryResponse.class);
        public record ActivityExportEvidenceRequest(HumanModels.ActivityFilter filter, List<String> entry_ids) implements SchemaTypes.GeneratedRequest {
            public ActivityExportEvidenceRequest {
                if (entry_ids != null) entry_ids = List.copyOf(entry_ids);
            }
        }
        public record ActivityExportEvidenceResponse(String export_id, HumanModels.ExportKind kind, String download_path, String content_type, String created_at, List<HumanModels.EvidenceRef> evidence) implements SchemaTypes.GeneratedResponse {
            public ActivityExportEvidenceResponse {
                Objects.requireNonNull(export_id, "export_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(download_path, "download_path");
                Objects.requireNonNull(content_type, "content_type");
                Objects.requireNonNull(created_at, "created_at");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public static final SchemaTypes.TypedOperation<ActivityExportEvidenceRequest, ActivityExportEvidenceResponse> ACTIVITY_EXPORT_EVIDENCE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "activity.export.evidence", true, ActivityExportEvidenceRequest.class, ActivityExportEvidenceResponse.class);
        public record ActivityExportStatementRequest(HumanModels.ActivityFilter filter) implements SchemaTypes.GeneratedRequest {
        }
        public record ActivityExportStatementResponse(String export_id, HumanModels.ExportKind kind, String download_path, String content_type, String created_at, List<HumanModels.EvidenceRef> evidence) implements SchemaTypes.GeneratedResponse {
            public ActivityExportStatementResponse {
                Objects.requireNonNull(export_id, "export_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(download_path, "download_path");
                Objects.requireNonNull(content_type, "content_type");
                Objects.requireNonNull(created_at, "created_at");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public static final SchemaTypes.TypedOperation<ActivityExportStatementRequest, ActivityExportStatementResponse> ACTIVITY_EXPORT_STATEMENT = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "activity.export.statement", true, ActivityExportStatementRequest.class, ActivityExportStatementResponse.class);
        public record ActivityQueryRequest(String cursor, HumanModels.ActivityFilter filter, Long page_limit) implements SchemaTypes.GeneratedRequest {
        }
        public record ActivityQueryResponse(List<HumanModels.ActivityGroup> groups, String next_cursor, HumanModels.ActivityFilter filter) implements SchemaTypes.GeneratedResponse {
            public ActivityQueryResponse {
                groups = List.copyOf(Objects.requireNonNull(groups, "groups"));
                Objects.requireNonNull(next_cursor, "next_cursor");
                Objects.requireNonNull(filter, "filter");
            }
        }
        public static final SchemaTypes.TypedOperation<ActivityQueryRequest, ActivityQueryResponse> ACTIVITY_QUERY = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "activity.query", false, ActivityQueryRequest.class, ActivityQueryResponse.class);
        public record AgentArchiveRequest(String confirm_name) implements SchemaTypes.GeneratedRequest {
            public AgentArchiveRequest {
                Objects.requireNonNull(confirm_name, "confirm_name");
            }
        }
        public record AgentArchiveResponse(String journey_id, HumanModels.JourneyKind kind, HumanModels.JourneyState state, String state_copy_key, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, String started_at, String updated_at, HumanModels.Refusal refusal, HumanModels.WalletSignRequest wallet_request) implements SchemaTypes.GeneratedResponse {
            public AgentArchiveResponse {
                Objects.requireNonNull(journey_id, "journey_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(started_at, "started_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<AgentArchiveRequest, AgentArchiveResponse> AGENT_ARCHIVE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "agent.archive", true, AgentArchiveRequest.class, AgentArchiveResponse.class);
        public record AgentCreateRequest(String name, String purpose, HumanModels.Money monthly_limit) implements SchemaTypes.GeneratedRequest {
            public AgentCreateRequest {
                Objects.requireNonNull(name, "name");
                Objects.requireNonNull(purpose, "purpose");
                Objects.requireNonNull(monthly_limit, "monthly_limit");
            }
        }
        public record AgentCreateResponse(String journey_id, HumanModels.JourneyKind kind, HumanModels.JourneyState state, String state_copy_key, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, String started_at, String updated_at, HumanModels.Refusal refusal, HumanModels.WalletSignRequest wallet_request) implements SchemaTypes.GeneratedResponse {
            public AgentCreateResponse {
                Objects.requireNonNull(journey_id, "journey_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(started_at, "started_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<AgentCreateRequest, AgentCreateResponse> AGENT_CREATE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "agent.create", true, AgentCreateRequest.class, AgentCreateResponse.class);
        public record AgentGetRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record AgentGetResponse(String agent_id, String name, String purpose, HumanModels.AgentState state, String state_copy_key, HumanModels.SpendLimit limit, HumanModels.AgentSpend spend, List<HumanModels.EvidenceRef> evidence, String created_at, String updated_at, String creation_journey_id) implements SchemaTypes.GeneratedResponse {
            public AgentGetResponse {
                Objects.requireNonNull(agent_id, "agent_id");
                Objects.requireNonNull(name, "name");
                Objects.requireNonNull(purpose, "purpose");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                Objects.requireNonNull(limit, "limit");
                Objects.requireNonNull(spend, "spend");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(created_at, "created_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<AgentGetRequest, AgentGetResponse> AGENT_GET = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "agent.get", false, AgentGetRequest.class, AgentGetResponse.class);
        public record AgentLimitRequest(HumanModels.Money monthly_limit) implements SchemaTypes.GeneratedRequest {
            public AgentLimitRequest {
                Objects.requireNonNull(monthly_limit, "monthly_limit");
            }
        }
        public record AgentLimitResponse(String agent_id, String name, String purpose, HumanModels.AgentState state, String state_copy_key, HumanModels.SpendLimit limit, HumanModels.AgentSpend spend, List<HumanModels.EvidenceRef> evidence, String created_at, String updated_at, String creation_journey_id) implements SchemaTypes.GeneratedResponse {
            public AgentLimitResponse {
                Objects.requireNonNull(agent_id, "agent_id");
                Objects.requireNonNull(name, "name");
                Objects.requireNonNull(purpose, "purpose");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                Objects.requireNonNull(limit, "limit");
                Objects.requireNonNull(spend, "spend");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(created_at, "created_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<AgentLimitRequest, AgentLimitResponse> AGENT_LIMIT = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "agent.limit", true, AgentLimitRequest.class, AgentLimitResponse.class);
        public record AgentListRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record AgentListResponse(List<HumanModels.Agent> agents, String next_cursor) implements SchemaTypes.GeneratedResponse {
            public AgentListResponse {
                agents = List.copyOf(Objects.requireNonNull(agents, "agents"));
                Objects.requireNonNull(next_cursor, "next_cursor");
            }
        }
        public static final SchemaTypes.TypedOperation<AgentListRequest, AgentListResponse> AGENT_LIST = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "agent.list", false, AgentListRequest.class, AgentListResponse.class);
        public record AgentPauseRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record AgentPauseResponse(String agent_id, String name, String purpose, HumanModels.AgentState state, String state_copy_key, HumanModels.SpendLimit limit, HumanModels.AgentSpend spend, List<HumanModels.EvidenceRef> evidence, String created_at, String updated_at, String creation_journey_id) implements SchemaTypes.GeneratedResponse {
            public AgentPauseResponse {
                Objects.requireNonNull(agent_id, "agent_id");
                Objects.requireNonNull(name, "name");
                Objects.requireNonNull(purpose, "purpose");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                Objects.requireNonNull(limit, "limit");
                Objects.requireNonNull(spend, "spend");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(created_at, "created_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<AgentPauseRequest, AgentPauseResponse> AGENT_PAUSE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "agent.pause", true, AgentPauseRequest.class, AgentPauseResponse.class);
        public record AgentReclaimRequest(HumanModels.Money money) implements SchemaTypes.GeneratedRequest {
            public AgentReclaimRequest {
                Objects.requireNonNull(money, "money");
            }
        }
        public record AgentReclaimResponse(String journey_id, HumanModels.JourneyKind kind, HumanModels.JourneyState state, String state_copy_key, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, String started_at, String updated_at, HumanModels.Refusal refusal, HumanModels.WalletSignRequest wallet_request) implements SchemaTypes.GeneratedResponse {
            public AgentReclaimResponse {
                Objects.requireNonNull(journey_id, "journey_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(started_at, "started_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<AgentReclaimRequest, AgentReclaimResponse> AGENT_RECLAIM = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "agent.reclaim", true, AgentReclaimRequest.class, AgentReclaimResponse.class);
        public record AgentRecoverRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record AgentRecoverResponse(String agent_id, HumanModels.KeyChallengeKind kind, String delay_copy_key, long delay_seconds, String ready_at, List<HumanModels.EvidenceRef> evidence) implements SchemaTypes.GeneratedResponse {
            public AgentRecoverResponse {
                Objects.requireNonNull(agent_id, "agent_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(delay_copy_key, "delay_copy_key");
                Objects.requireNonNull(ready_at, "ready_at");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public static final SchemaTypes.TypedOperation<AgentRecoverRequest, AgentRecoverResponse> AGENT_RECOVER = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "agent.recover", true, AgentRecoverRequest.class, AgentRecoverResponse.class);
        public record AgentResumeRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record AgentResumeResponse(String agent_id, String name, String purpose, HumanModels.AgentState state, String state_copy_key, HumanModels.SpendLimit limit, HumanModels.AgentSpend spend, List<HumanModels.EvidenceRef> evidence, String created_at, String updated_at, String creation_journey_id) implements SchemaTypes.GeneratedResponse {
            public AgentResumeResponse {
                Objects.requireNonNull(agent_id, "agent_id");
                Objects.requireNonNull(name, "name");
                Objects.requireNonNull(purpose, "purpose");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                Objects.requireNonNull(limit, "limit");
                Objects.requireNonNull(spend, "spend");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(created_at, "created_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<AgentResumeRequest, AgentResumeResponse> AGENT_RESUME = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "agent.resume", true, AgentResumeRequest.class, AgentResumeResponse.class);
        public record AgentRotateRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record AgentRotateResponse(String agent_id, HumanModels.KeyChallengeKind kind, String delay_copy_key, long delay_seconds, String ready_at, List<HumanModels.EvidenceRef> evidence) implements SchemaTypes.GeneratedResponse {
            public AgentRotateResponse {
                Objects.requireNonNull(agent_id, "agent_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(delay_copy_key, "delay_copy_key");
                Objects.requireNonNull(ready_at, "ready_at");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public static final SchemaTypes.TypedOperation<AgentRotateRequest, AgentRotateResponse> AGENT_ROTATE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "agent.rotate", true, AgentRotateRequest.class, AgentRotateResponse.class);
        public record ApprovalApproveRequest(String step_up_evidence) implements SchemaTypes.GeneratedRequest {
            public ApprovalApproveRequest {
                Objects.requireNonNull(step_up_evidence, "step_up_evidence");
            }
        }
        public record ApprovalApproveResponse(String approval_id, HumanModels.ApprovalState state, String state_copy_key, boolean money_moved, String moved_copy_key, List<HumanModels.EvidenceRef> evidence) implements SchemaTypes.GeneratedResponse {
            public ApprovalApproveResponse {
                Objects.requireNonNull(approval_id, "approval_id");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                Objects.requireNonNull(moved_copy_key, "moved_copy_key");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public static final SchemaTypes.TypedOperation<ApprovalApproveRequest, ApprovalApproveResponse> APPROVAL_APPROVE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "approval.approve", true, ApprovalApproveRequest.class, ApprovalApproveResponse.class);
        public record ApprovalGetRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record ApprovalGetResponse(String approval_id, String agent_id, String agent_name, HumanModels.ApprovalState state, String state_copy_key, String reason_copy_key, HumanModels.ApprovalFacts facts, HumanModels.VerifiedMoney budget_remaining_after, String created_at, List<HumanModels.EvidenceRef> evidence) implements SchemaTypes.GeneratedResponse {
            public ApprovalGetResponse {
                Objects.requireNonNull(approval_id, "approval_id");
                Objects.requireNonNull(agent_id, "agent_id");
                Objects.requireNonNull(agent_name, "agent_name");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                Objects.requireNonNull(reason_copy_key, "reason_copy_key");
                Objects.requireNonNull(facts, "facts");
                Objects.requireNonNull(budget_remaining_after, "budget_remaining_after");
                Objects.requireNonNull(created_at, "created_at");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public static final SchemaTypes.TypedOperation<ApprovalGetRequest, ApprovalGetResponse> APPROVAL_GET = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "approval.get", false, ApprovalGetRequest.class, ApprovalGetResponse.class);
        public record ApprovalListRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record ApprovalListResponse(List<HumanModels.ApprovalSummary> approvals, String next_cursor) implements SchemaTypes.GeneratedResponse {
            public ApprovalListResponse {
                approvals = List.copyOf(Objects.requireNonNull(approvals, "approvals"));
                Objects.requireNonNull(next_cursor, "next_cursor");
            }
        }
        public static final SchemaTypes.TypedOperation<ApprovalListRequest, ApprovalListResponse> APPROVAL_LIST = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "approval.list", false, ApprovalListRequest.class, ApprovalListResponse.class);
        public record ApprovalRejectRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record ApprovalRejectResponse(String approval_id, HumanModels.ApprovalState state, String state_copy_key, boolean money_moved, String moved_copy_key, List<HumanModels.EvidenceRef> evidence) implements SchemaTypes.GeneratedResponse {
            public ApprovalRejectResponse {
                Objects.requireNonNull(approval_id, "approval_id");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                Objects.requireNonNull(moved_copy_key, "moved_copy_key");
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
            }
        }
        public static final SchemaTypes.TypedOperation<ApprovalRejectRequest, ApprovalRejectResponse> APPROVAL_REJECT = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "approval.reject", true, ApprovalRejectRequest.class, ApprovalRejectResponse.class);
        public record AuthenticatorBackupRotateRequest(HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedRequest {
            public AuthenticatorBackupRotateRequest {
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record AuthenticatorBackupRotateResponse(List<String> codes, String remask_at, boolean copyable) implements SchemaTypes.GeneratedResponse {
            public AuthenticatorBackupRotateResponse {
                codes = List.copyOf(Objects.requireNonNull(codes, "codes"));
                Objects.requireNonNull(remask_at, "remask_at");
            }
        }
        public static final SchemaTypes.TypedOperation<AuthenticatorBackupRotateRequest, AuthenticatorBackupRotateResponse> AUTHENTICATOR_BACKUP_ROTATE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "authenticator.backup.rotate", false, AuthenticatorBackupRotateRequest.class, AuthenticatorBackupRotateResponse.class);
        public record AuthenticatorDisableRequest(HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedRequest {
            public AuthenticatorDisableRequest {
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record AuthenticatorDisableResponse(List<HumanModels.AuthenticatorMethod> methods, long backup_codes_remaining) implements SchemaTypes.GeneratedResponse {
            public AuthenticatorDisableResponse {
                methods = List.copyOf(Objects.requireNonNull(methods, "methods"));
            }
        }
        public static final SchemaTypes.TypedOperation<AuthenticatorDisableRequest, AuthenticatorDisableResponse> AUTHENTICATOR_DISABLE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "authenticator.disable", false, AuthenticatorDisableRequest.class, AuthenticatorDisableResponse.class);
        public record AuthenticatorSetupBeginRequest(String label, HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedRequest {
            public AuthenticatorSetupBeginRequest {
                Objects.requireNonNull(label, "label");
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record AuthenticatorSetupBeginResponse(String setup_id, HumanModels.TimedSecret secret, HumanModels.TimedSecret otpauth_uri, String expires_at) implements SchemaTypes.GeneratedResponse {
            public AuthenticatorSetupBeginResponse {
                Objects.requireNonNull(setup_id, "setup_id");
                Objects.requireNonNull(secret, "secret");
                Objects.requireNonNull(otpauth_uri, "otpauth_uri");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public static final SchemaTypes.TypedOperation<AuthenticatorSetupBeginRequest, AuthenticatorSetupBeginResponse> AUTHENTICATOR_SETUP_BEGIN = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "authenticator.setup.begin", false, AuthenticatorSetupBeginRequest.class, AuthenticatorSetupBeginResponse.class);
        public record AuthenticatorSetupFinishRequest(String code, HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedRequest {
            public AuthenticatorSetupFinishRequest {
                Objects.requireNonNull(code, "code");
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record AuthenticatorSetupFinishResponse(HumanModels.AuthenticatorMethod method, HumanModels.BackupCodeSet backup_codes) implements SchemaTypes.GeneratedResponse {
            public AuthenticatorSetupFinishResponse {
                Objects.requireNonNull(method, "method");
                Objects.requireNonNull(backup_codes, "backup_codes");
            }
        }
        public static final SchemaTypes.TypedOperation<AuthenticatorSetupFinishRequest, AuthenticatorSetupFinishResponse> AUTHENTICATOR_SETUP_FINISH = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "authenticator.setup.finish", false, AuthenticatorSetupFinishRequest.class, AuthenticatorSetupFinishResponse.class);
        public record AuthenticatorStatusRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record AuthenticatorStatusResponse(List<HumanModels.AuthenticatorMethod> methods, long backup_codes_remaining) implements SchemaTypes.GeneratedResponse {
            public AuthenticatorStatusResponse {
                methods = List.copyOf(Objects.requireNonNull(methods, "methods"));
            }
        }
        public static final SchemaTypes.TypedOperation<AuthenticatorStatusRequest, AuthenticatorStatusResponse> AUTHENTICATOR_STATUS = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "authenticator.status", false, AuthenticatorStatusRequest.class, AuthenticatorStatusResponse.class);
        public record BindingRebindRequest(String address, String statement, String signature, HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedRequest {
            public BindingRebindRequest {
                Objects.requireNonNull(address, "address");
                Objects.requireNonNull(statement, "statement");
                Objects.requireNonNull(signature, "signature");
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record BindingRebindResponse(String journey_id, HumanModels.JourneyKind kind, HumanModels.JourneyState state, String state_copy_key, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, String started_at, String updated_at, HumanModels.Refusal refusal, HumanModels.WalletSignRequest wallet_request) implements SchemaTypes.GeneratedResponse {
            public BindingRebindResponse {
                Objects.requireNonNull(journey_id, "journey_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(started_at, "started_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<BindingRebindRequest, BindingRebindResponse> BINDING_REBIND = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "binding.rebind", true, BindingRebindRequest.class, BindingRebindResponse.class);
        public record BindingRebindActionRequest(String address) implements SchemaTypes.GeneratedRequest {
            public BindingRebindActionRequest {
                Objects.requireNonNull(address, "address");
            }
        }
        public record BindingRebindActionResponse(HumanModels.BindingStatement binding, String confirms) implements SchemaTypes.GeneratedResponse {
            public BindingRebindActionResponse {
                Objects.requireNonNull(binding, "binding");
                Objects.requireNonNull(confirms, "confirms");
            }
        }
        public static final SchemaTypes.TypedOperation<BindingRebindActionRequest, BindingRebindActionResponse> BINDING_REBIND_ACTION = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "binding.rebind.action", false, BindingRebindActionRequest.class, BindingRebindActionResponse.class);
        public record BindingStatementRequest(String address) implements SchemaTypes.GeneratedRequest {
            public BindingStatementRequest {
                Objects.requireNonNull(address, "address");
            }
        }
        public record BindingStatementResponse(String statement, String address, String expires_at) implements SchemaTypes.GeneratedResponse {
            public BindingStatementResponse {
                Objects.requireNonNull(statement, "statement");
                Objects.requireNonNull(address, "address");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public static final SchemaTypes.TypedOperation<BindingStatementRequest, BindingStatementResponse> BINDING_STATEMENT = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "binding.statement", false, BindingStatementRequest.class, BindingStatementResponse.class);
        public record BindingStatusRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record BindingStatusResponse(HumanModels.BindingState state, String address, String bound_at, HumanModels.EvidenceRef evidence) implements SchemaTypes.GeneratedResponse {
            public BindingStatusResponse {
                Objects.requireNonNull(state, "state");
            }
        }
        public static final SchemaTypes.TypedOperation<BindingStatusRequest, BindingStatusResponse> BINDING_STATUS = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "binding.status", false, BindingStatusRequest.class, BindingStatusResponse.class);
        public record BindingSubmitRequest(String address, String statement, String signature) implements SchemaTypes.GeneratedRequest {
            public BindingSubmitRequest {
                Objects.requireNonNull(address, "address");
                Objects.requireNonNull(statement, "statement");
                Objects.requireNonNull(signature, "signature");
            }
        }
        public record BindingSubmitResponse(String journey_id, HumanModels.JourneyKind kind, HumanModels.JourneyState state, String state_copy_key, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, String started_at, String updated_at, HumanModels.Refusal refusal, HumanModels.WalletSignRequest wallet_request) implements SchemaTypes.GeneratedResponse {
            public BindingSubmitResponse {
                Objects.requireNonNull(journey_id, "journey_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(started_at, "started_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<BindingSubmitRequest, BindingSubmitResponse> BINDING_SUBMIT = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "binding.submit", true, BindingSubmitRequest.class, BindingSubmitResponse.class);
        public record DepositConfirmRequest(String wallet_transaction, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedRequest {
            public DepositConfirmRequest {
                Objects.requireNonNull(wallet_transaction, "wallet_transaction");
            }
        }
        public record DepositConfirmResponse(String journey_id, HumanModels.JourneyKind kind, HumanModels.JourneyState state, String state_copy_key, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, String started_at, String updated_at, HumanModels.Refusal refusal, HumanModels.WalletSignRequest wallet_request) implements SchemaTypes.GeneratedResponse {
            public DepositConfirmResponse {
                Objects.requireNonNull(journey_id, "journey_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(started_at, "started_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<DepositConfirmRequest, DepositConfirmResponse> DEPOSIT_CONFIRM = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "deposit.confirm", false, DepositConfirmRequest.class, DepositConfirmResponse.class);
        public record DepositStartRequest(HumanModels.Money money, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedRequest {
            public DepositStartRequest {
                Objects.requireNonNull(money, "money");
            }
        }
        public record DepositStartResponse(String journey_id, HumanModels.JourneyKind kind, HumanModels.JourneyState state, String state_copy_key, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, String started_at, String updated_at, HumanModels.Refusal refusal, HumanModels.WalletSignRequest wallet_request) implements SchemaTypes.GeneratedResponse {
            public DepositStartResponse {
                Objects.requireNonNull(journey_id, "journey_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(started_at, "started_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<DepositStartRequest, DepositStartResponse> DEPOSIT_START = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "deposit.start", true, DepositStartRequest.class, DepositStartResponse.class);
        public record EvidenceGetRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record EvidenceGetResponse(String evidence_id, @JsonProperty("class") HumanModels.EvidenceClass class_, HumanModels.VerificationLevel verification, String content_type, String bytes_base64, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedResponse {
            public EvidenceGetResponse {
                Objects.requireNonNull(evidence_id, "evidence_id");
                Objects.requireNonNull(class_, "class");
                Objects.requireNonNull(verification, "verification");
                Objects.requireNonNull(content_type, "content_type");
                Objects.requireNonNull(bytes_base64, "bytes_base64");
            }
        }
        public static final SchemaTypes.TypedOperation<EvidenceGetRequest, EvidenceGetResponse> EVIDENCE_GET = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "evidence.get", false, EvidenceGetRequest.class, EvidenceGetResponse.class);
        public record ExitEligibilityRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record ExitEligibilityResponse(boolean eligible, String copy_key, String withdraw_instead_path, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedResponse {
            public ExitEligibilityResponse {
                Objects.requireNonNull(copy_key, "copy_key");
            }
        }
        public static final SchemaTypes.TypedOperation<ExitEligibilityRequest, ExitEligibilityResponse> EXIT_ELIGIBILITY = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "exit.eligibility", false, ExitEligibilityRequest.class, ExitEligibilityResponse.class);
        public record ExitStartRequest(String confirmation, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedRequest {
            public ExitStartRequest {
                Objects.requireNonNull(confirmation, "confirmation");
            }
        }
        public record ExitStartResponse(String journey_id, HumanModels.JourneyKind kind, HumanModels.JourneyState state, String state_copy_key, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, String started_at, String updated_at, HumanModels.Refusal refusal, HumanModels.WalletSignRequest wallet_request) implements SchemaTypes.GeneratedResponse {
            public ExitStartResponse {
                Objects.requireNonNull(journey_id, "journey_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(started_at, "started_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<ExitStartRequest, ExitStartResponse> EXIT_START = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "exit.start", true, ExitStartRequest.class, ExitStartResponse.class);
        public record HomeSummaryRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record HomeSummaryResponse(HumanModels.AccountBalance balance, List<HumanModels.Agent> agents, List<HumanModels.ApprovalSummary> approvals, List<HumanModels.ActivityEntryDetail> recent_activity) implements SchemaTypes.GeneratedResponse {
            public HomeSummaryResponse {
                Objects.requireNonNull(balance, "balance");
                agents = List.copyOf(Objects.requireNonNull(agents, "agents"));
                approvals = List.copyOf(Objects.requireNonNull(approvals, "approvals"));
                recent_activity = List.copyOf(Objects.requireNonNull(recent_activity, "recent_activity"));
            }
        }
        public static final SchemaTypes.TypedOperation<HomeSummaryRequest, HomeSummaryResponse> HOME_SUMMARY = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "home.summary", false, HomeSummaryRequest.class, HomeSummaryResponse.class);
        public record JourneyGetRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record JourneyGetResponse(String journey_id, HumanModels.JourneyKind kind, HumanModels.JourneyState state, String state_copy_key, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, String started_at, String updated_at, HumanModels.Refusal refusal, HumanModels.WalletSignRequest wallet_request) implements SchemaTypes.GeneratedResponse {
            public JourneyGetResponse {
                Objects.requireNonNull(journey_id, "journey_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(started_at, "started_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<JourneyGetRequest, JourneyGetResponse> JOURNEY_GET = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "journey.get", false, JourneyGetRequest.class, JourneyGetResponse.class);
        public record JourneyListRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record JourneyListResponse(List<HumanModels.Journey> journeys, String next_cursor) implements SchemaTypes.GeneratedResponse {
            public JourneyListResponse {
                journeys = List.copyOf(Objects.requireNonNull(journeys, "journeys"));
                Objects.requireNonNull(next_cursor, "next_cursor");
            }
        }
        public static final SchemaTypes.TypedOperation<JourneyListRequest, JourneyListResponse> JOURNEY_LIST = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "journey.list", false, JourneyListRequest.class, JourneyListResponse.class);
        public record MoveCommitRequest(String quote_id) implements SchemaTypes.GeneratedRequest {
            public MoveCommitRequest {
                Objects.requireNonNull(quote_id, "quote_id");
            }
        }
        public record MoveCommitResponse(String journey_id, HumanModels.JourneyKind kind, HumanModels.JourneyState state, String state_copy_key, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, String started_at, String updated_at, HumanModels.Refusal refusal, HumanModels.WalletSignRequest wallet_request) implements SchemaTypes.GeneratedResponse {
            public MoveCommitResponse {
                Objects.requireNonNull(journey_id, "journey_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(started_at, "started_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<MoveCommitRequest, MoveCommitResponse> MOVE_COMMIT = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "move.commit", true, MoveCommitRequest.class, MoveCommitResponse.class);
        public record MoveQuoteRequest(String source, String destination, HumanModels.Money money) implements SchemaTypes.GeneratedRequest {
            public MoveQuoteRequest {
                Objects.requireNonNull(source, "source");
                Objects.requireNonNull(destination, "destination");
                Objects.requireNonNull(money, "money");
            }
        }
        public record MoveQuoteResponse(String quote_id, String description_copy_key, HumanModels.MoveMechanism mechanism, HumanModels.Money money, HumanModels.Money fee_estimate, HumanModels.Money fee_ceiling, String arrival_estimate, String expires_at, String irreversibility_copy_key) implements SchemaTypes.GeneratedResponse {
            public MoveQuoteResponse {
                Objects.requireNonNull(quote_id, "quote_id");
                Objects.requireNonNull(description_copy_key, "description_copy_key");
                Objects.requireNonNull(mechanism, "mechanism");
                Objects.requireNonNull(money, "money");
                Objects.requireNonNull(fee_estimate, "fee_estimate");
                Objects.requireNonNull(fee_ceiling, "fee_ceiling");
                Objects.requireNonNull(arrival_estimate, "arrival_estimate");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public static final SchemaTypes.TypedOperation<MoveQuoteRequest, MoveQuoteResponse> MOVE_QUOTE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "move.quote", false, MoveQuoteRequest.class, MoveQuoteResponse.class);
        public record NotificationListRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record NotificationListResponse(List<HumanModels.NotificationGroup> groups, String next_cursor, long unread_count) implements SchemaTypes.GeneratedResponse {
            public NotificationListResponse {
                groups = List.copyOf(Objects.requireNonNull(groups, "groups"));
                Objects.requireNonNull(next_cursor, "next_cursor");
            }
        }
        public static final SchemaTypes.TypedOperation<NotificationListRequest, NotificationListResponse> NOTIFICATION_LIST = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "notification.list", false, NotificationListRequest.class, NotificationListResponse.class);
        public record NotificationPreferencesGetRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record NotificationPreferencesGetResponse(HumanModels.ChannelPreference push, HumanModels.ChannelPreference email, HumanModels.ChannelPreference in_app, HumanModels.NotificationDetailLevel detail) implements SchemaTypes.GeneratedResponse {
            public NotificationPreferencesGetResponse {
                Objects.requireNonNull(push, "push");
                Objects.requireNonNull(email, "email");
                Objects.requireNonNull(in_app, "in_app");
                Objects.requireNonNull(detail, "detail");
            }
        }
        public static final SchemaTypes.TypedOperation<NotificationPreferencesGetRequest, NotificationPreferencesGetResponse> NOTIFICATION_PREFERENCES_GET = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "notification.preferences.get", false, NotificationPreferencesGetRequest.class, NotificationPreferencesGetResponse.class);
        public record NotificationPreferencesSetRequest(HumanModels.ChannelPreference push, HumanModels.ChannelPreference email, HumanModels.ChannelPreference in_app, HumanModels.NotificationDetailLevel detail) implements SchemaTypes.GeneratedRequest {
            public NotificationPreferencesSetRequest {
                Objects.requireNonNull(push, "push");
                Objects.requireNonNull(email, "email");
                Objects.requireNonNull(in_app, "in_app");
                Objects.requireNonNull(detail, "detail");
            }
        }
        public record NotificationPreferencesSetResponse(HumanModels.ChannelPreference push, HumanModels.ChannelPreference email, HumanModels.ChannelPreference in_app, HumanModels.NotificationDetailLevel detail) implements SchemaTypes.GeneratedResponse {
            public NotificationPreferencesSetResponse {
                Objects.requireNonNull(push, "push");
                Objects.requireNonNull(email, "email");
                Objects.requireNonNull(in_app, "in_app");
                Objects.requireNonNull(detail, "detail");
            }
        }
        public static final SchemaTypes.TypedOperation<NotificationPreferencesSetRequest, NotificationPreferencesSetResponse> NOTIFICATION_PREFERENCES_SET = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "notification.preferences.set", false, NotificationPreferencesSetRequest.class, NotificationPreferencesSetResponse.class);
        public record NotificationReadRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record NotificationReadResponse(String notification_id, @JsonProperty("class") HumanModels.NotificationClass class_, String title_copy_key, String body_copy_key, String deep_link, boolean read, String created_at, HumanModels.Money money, String agent_id, String approval_id, String journey_id, String action_copy_key) implements SchemaTypes.GeneratedResponse {
            public NotificationReadResponse {
                Objects.requireNonNull(notification_id, "notification_id");
                Objects.requireNonNull(class_, "class");
                Objects.requireNonNull(title_copy_key, "title_copy_key");
                Objects.requireNonNull(body_copy_key, "body_copy_key");
                Objects.requireNonNull(deep_link, "deep_link");
                Objects.requireNonNull(created_at, "created_at");
            }
        }
        public static final SchemaTypes.TypedOperation<NotificationReadRequest, NotificationReadResponse> NOTIFICATION_READ = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "notification.read", false, NotificationReadRequest.class, NotificationReadResponse.class);
        public record OnboardingResumeRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record OnboardingResumeResponse(String journey_id, HumanModels.JourneyKind kind, HumanModels.JourneyState state, String state_copy_key, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, String started_at, String updated_at, HumanModels.Refusal refusal, HumanModels.WalletSignRequest wallet_request) implements SchemaTypes.GeneratedResponse {
            public OnboardingResumeResponse {
                Objects.requireNonNull(journey_id, "journey_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(started_at, "started_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<OnboardingResumeRequest, OnboardingResumeResponse> ONBOARDING_RESUME = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "onboarding.resume", false, OnboardingResumeRequest.class, OnboardingResumeResponse.class);
        public record OnboardingStatusRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record OnboardingStatusResponse(String journey_id, HumanModels.JourneyKind kind, HumanModels.JourneyState state, String state_copy_key, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, String started_at, String updated_at, HumanModels.Refusal refusal, HumanModels.WalletSignRequest wallet_request) implements SchemaTypes.GeneratedResponse {
            public OnboardingStatusResponse {
                Objects.requireNonNull(journey_id, "journey_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(started_at, "started_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<OnboardingStatusRequest, OnboardingStatusResponse> ONBOARDING_STATUS = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "onboarding.status", false, OnboardingStatusRequest.class, OnboardingStatusResponse.class);
        public record PasskeyAssertBeginRequest(String email) implements SchemaTypes.GeneratedRequest {
        }
        public record PasskeyAssertBeginResponse(String assertion_id, String ceremony, String expires_at) implements SchemaTypes.GeneratedResponse {
            public PasskeyAssertBeginResponse {
                Objects.requireNonNull(assertion_id, "assertion_id");
                Objects.requireNonNull(ceremony, "ceremony");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public static final SchemaTypes.TypedOperation<PasskeyAssertBeginRequest, PasskeyAssertBeginResponse> PASSKEY_ASSERT_BEGIN = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "passkey.assert.begin", false, PasskeyAssertBeginRequest.class, PasskeyAssertBeginResponse.class);
        public record PasskeyAssertFinishRequest(String credential) implements SchemaTypes.GeneratedRequest {
            public PasskeyAssertFinishRequest {
                Objects.requireNonNull(credential, "credential");
            }
        }
        public record PasskeyAssertFinishResponse(String assertion_id, String passkey_id, String completed_at, String expires_at) implements SchemaTypes.GeneratedResponse {
            public PasskeyAssertFinishResponse {
                Objects.requireNonNull(assertion_id, "assertion_id");
                Objects.requireNonNull(passkey_id, "passkey_id");
                Objects.requireNonNull(completed_at, "completed_at");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public static final SchemaTypes.TypedOperation<PasskeyAssertFinishRequest, PasskeyAssertFinishResponse> PASSKEY_ASSERT_FINISH = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "passkey.assert.finish", false, PasskeyAssertFinishRequest.class, PasskeyAssertFinishResponse.class);
        public record PasskeyRegisterBeginRequest(String account_id) implements SchemaTypes.GeneratedRequest {
            public PasskeyRegisterBeginRequest {
                Objects.requireNonNull(account_id, "account_id");
            }
        }
        public record PasskeyRegisterBeginResponse(String registration_id, String ceremony, String expires_at) implements SchemaTypes.GeneratedResponse {
            public PasskeyRegisterBeginResponse {
                Objects.requireNonNull(registration_id, "registration_id");
                Objects.requireNonNull(ceremony, "ceremony");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public static final SchemaTypes.TypedOperation<PasskeyRegisterBeginRequest, PasskeyRegisterBeginResponse> PASSKEY_REGISTER_BEGIN = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "passkey.register.begin", false, PasskeyRegisterBeginRequest.class, PasskeyRegisterBeginResponse.class);
        public record PasskeyRegisterFinishRequest(String credential) implements SchemaTypes.GeneratedRequest {
            public PasskeyRegisterFinishRequest {
                Objects.requireNonNull(credential, "credential");
            }
        }
        public record PasskeyRegisterFinishResponse(String passkey_id, String label, String created_at, String last_used_at) implements SchemaTypes.GeneratedResponse {
            public PasskeyRegisterFinishResponse {
                Objects.requireNonNull(passkey_id, "passkey_id");
                Objects.requireNonNull(label, "label");
                Objects.requireNonNull(created_at, "created_at");
            }
        }
        public static final SchemaTypes.TypedOperation<PasskeyRegisterFinishRequest, PasskeyRegisterFinishResponse> PASSKEY_REGISTER_FINISH = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "passkey.register.finish", false, PasskeyRegisterFinishRequest.class, PasskeyRegisterFinishResponse.class);
        public record ProfileGetRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record ProfileGetResponse(String display_name, String avatar_url) implements SchemaTypes.GeneratedResponse {
            public ProfileGetResponse {
                Objects.requireNonNull(display_name, "display_name");
            }
        }
        public static final SchemaTypes.TypedOperation<ProfileGetRequest, ProfileGetResponse> PROFILE_GET = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "profile.get", false, ProfileGetRequest.class, ProfileGetResponse.class);
        public record ProfileUpdateRequest(String display_name, String avatar_url) implements SchemaTypes.GeneratedRequest {
        }
        public record ProfileUpdateResponse(String display_name, String avatar_url) implements SchemaTypes.GeneratedResponse {
            public ProfileUpdateResponse {
                Objects.requireNonNull(display_name, "display_name");
            }
        }
        public static final SchemaTypes.TypedOperation<ProfileUpdateRequest, ProfileUpdateResponse> PROFILE_UPDATE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "profile.update", false, ProfileUpdateRequest.class, ProfileUpdateResponse.class);
        public record SecurityActionRequest(HumanModels.SecurityActionKind action, String target_id) implements SchemaTypes.GeneratedRequest {
            public SecurityActionRequest {
                Objects.requireNonNull(action, "action");
            }
        }
        public record SecurityActionResponse(String confirms) implements SchemaTypes.GeneratedResponse {
            public SecurityActionResponse {
                Objects.requireNonNull(confirms, "confirms");
            }
        }
        public static final SchemaTypes.TypedOperation<SecurityActionRequest, SecurityActionResponse> SECURITY_ACTION = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "security.action", false, SecurityActionRequest.class, SecurityActionResponse.class);
        public record SecurityPasskeyListRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record SecurityPasskeyListResponse(List<HumanModels.Passkey> passkeys) implements SchemaTypes.GeneratedResponse {
            public SecurityPasskeyListResponse {
                passkeys = List.copyOf(Objects.requireNonNull(passkeys, "passkeys"));
            }
        }
        public static final SchemaTypes.TypedOperation<SecurityPasskeyListRequest, SecurityPasskeyListResponse> SECURITY_PASSKEY_LIST = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "security.passkey.list", false, SecurityPasskeyListRequest.class, SecurityPasskeyListResponse.class);
        public record SecurityPasskeyRegisterBeginRequest(String label, HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedRequest {
            public SecurityPasskeyRegisterBeginRequest {
                Objects.requireNonNull(label, "label");
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record SecurityPasskeyRegisterBeginResponse(String registration_id, String ceremony, String expires_at) implements SchemaTypes.GeneratedResponse {
            public SecurityPasskeyRegisterBeginResponse {
                Objects.requireNonNull(registration_id, "registration_id");
                Objects.requireNonNull(ceremony, "ceremony");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public static final SchemaTypes.TypedOperation<SecurityPasskeyRegisterBeginRequest, SecurityPasskeyRegisterBeginResponse> SECURITY_PASSKEY_REGISTER_BEGIN = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "security.passkey.register.begin", false, SecurityPasskeyRegisterBeginRequest.class, SecurityPasskeyRegisterBeginResponse.class);
        public record SecurityPasskeyRegisterFinishRequest(String credential, HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedRequest {
            public SecurityPasskeyRegisterFinishRequest {
                Objects.requireNonNull(credential, "credential");
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record SecurityPasskeyRegisterFinishResponse(String passkey_id, String label, String created_at, String last_used_at) implements SchemaTypes.GeneratedResponse {
            public SecurityPasskeyRegisterFinishResponse {
                Objects.requireNonNull(passkey_id, "passkey_id");
                Objects.requireNonNull(label, "label");
                Objects.requireNonNull(created_at, "created_at");
            }
        }
        public static final SchemaTypes.TypedOperation<SecurityPasskeyRegisterFinishRequest, SecurityPasskeyRegisterFinishResponse> SECURITY_PASSKEY_REGISTER_FINISH = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "security.passkey.register.finish", false, SecurityPasskeyRegisterFinishRequest.class, SecurityPasskeyRegisterFinishResponse.class);
        public record SecurityPasskeyRevokeRequest(HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedRequest {
            public SecurityPasskeyRevokeRequest {
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record SecurityPasskeyRevokeResponse(List<HumanModels.Passkey> passkeys) implements SchemaTypes.GeneratedResponse {
            public SecurityPasskeyRevokeResponse {
                passkeys = List.copyOf(Objects.requireNonNull(passkeys, "passkeys"));
            }
        }
        public static final SchemaTypes.TypedOperation<SecurityPasskeyRevokeRequest, SecurityPasskeyRevokeResponse> SECURITY_PASSKEY_REVOKE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "security.passkey.revoke", false, SecurityPasskeyRevokeRequest.class, SecurityPasskeyRevokeResponse.class);
        public record SecurityRecoveryRevealRequest(String evidence_id, HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedRequest {
            public SecurityRecoveryRevealRequest {
                Objects.requireNonNull(evidence_id, "evidence_id");
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record SecurityRecoveryRevealResponse(String value, String remask_at, boolean copyable) implements SchemaTypes.GeneratedResponse {
            public SecurityRecoveryRevealResponse {
                Objects.requireNonNull(value, "value");
                Objects.requireNonNull(remask_at, "remask_at");
            }
        }
        public static final SchemaTypes.TypedOperation<SecurityRecoveryRevealRequest, SecurityRecoveryRevealResponse> SECURITY_RECOVERY_REVEAL = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "security.recovery.reveal", false, SecurityRecoveryRevealRequest.class, SecurityRecoveryRevealResponse.class);
        public record SecuritySessionRevokeRequest(HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedRequest {
            public SecuritySessionRevokeRequest {
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record SecuritySessionRevokeResponse(List<String> revoked_session_ids, String revoked_at) implements SchemaTypes.GeneratedResponse {
            public SecuritySessionRevokeResponse {
                revoked_session_ids = List.copyOf(Objects.requireNonNull(revoked_session_ids, "revoked_session_ids"));
                Objects.requireNonNull(revoked_at, "revoked_at");
            }
        }
        public static final SchemaTypes.TypedOperation<SecuritySessionRevokeRequest, SecuritySessionRevokeResponse> SECURITY_SESSION_REVOKE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "security.session.revoke", false, SecuritySessionRevokeRequest.class, SecuritySessionRevokeResponse.class);
        public record SecuritySessionRevokeAllRequest(HumanModels.StepUpEvidence step_up) implements SchemaTypes.GeneratedRequest {
            public SecuritySessionRevokeAllRequest {
                Objects.requireNonNull(step_up, "step_up");
            }
        }
        public record SecuritySessionRevokeAllResponse(List<String> revoked_session_ids, String revoked_at) implements SchemaTypes.GeneratedResponse {
            public SecuritySessionRevokeAllResponse {
                revoked_session_ids = List.copyOf(Objects.requireNonNull(revoked_session_ids, "revoked_session_ids"));
                Objects.requireNonNull(revoked_at, "revoked_at");
            }
        }
        public static final SchemaTypes.TypedOperation<SecuritySessionRevokeAllRequest, SecuritySessionRevokeAllResponse> SECURITY_SESSION_REVOKE_ALL = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "security.session.revoke-all", false, SecuritySessionRevokeAllRequest.class, SecuritySessionRevokeAllResponse.class);
        public record SessionListRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record SessionListResponse(List<HumanModels.Session> sessions) implements SchemaTypes.GeneratedResponse {
            public SessionListResponse {
                sessions = List.copyOf(Objects.requireNonNull(sessions, "sessions"));
            }
        }
        public static final SchemaTypes.TypedOperation<SessionListRequest, SessionListResponse> SESSION_LIST = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "session.list", false, SessionListRequest.class, SessionListResponse.class);
        public record SessionOpenRequest(String assertion_id) implements SchemaTypes.GeneratedRequest {
            public SessionOpenRequest {
                Objects.requireNonNull(assertion_id, "assertion_id");
            }
        }
        public record SessionOpenResponse(String session_id, HumanModels.Device device, String opened_at, String last_active_at, boolean current) implements SchemaTypes.GeneratedResponse {
            public SessionOpenResponse {
                Objects.requireNonNull(session_id, "session_id");
                Objects.requireNonNull(device, "device");
                Objects.requireNonNull(opened_at, "opened_at");
                Objects.requireNonNull(last_active_at, "last_active_at");
            }
        }
        public static final SchemaTypes.TypedOperation<SessionOpenRequest, SessionOpenResponse> SESSION_OPEN = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "session.open", false, SessionOpenRequest.class, SessionOpenResponse.class);
        public record SessionRefreshRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record SessionRefreshResponse(String session_id, HumanModels.Device device, String opened_at, String last_active_at, boolean current) implements SchemaTypes.GeneratedResponse {
            public SessionRefreshResponse {
                Objects.requireNonNull(session_id, "session_id");
                Objects.requireNonNull(device, "device");
                Objects.requireNonNull(opened_at, "opened_at");
                Objects.requireNonNull(last_active_at, "last_active_at");
            }
        }
        public static final SchemaTypes.TypedOperation<SessionRefreshRequest, SessionRefreshResponse> SESSION_REFRESH = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "session.refresh", false, SessionRefreshRequest.class, SessionRefreshResponse.class);
        public record SessionRevokeRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record SessionRevokeResponse(List<String> revoked_session_ids, String revoked_at) implements SchemaTypes.GeneratedResponse {
            public SessionRevokeResponse {
                revoked_session_ids = List.copyOf(Objects.requireNonNull(revoked_session_ids, "revoked_session_ids"));
                Objects.requireNonNull(revoked_at, "revoked_at");
            }
        }
        public static final SchemaTypes.TypedOperation<SessionRevokeRequest, SessionRevokeResponse> SESSION_REVOKE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "session.revoke", false, SessionRevokeRequest.class, SessionRevokeResponse.class);
        public record SessionRevokeAllRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record SessionRevokeAllResponse(List<String> revoked_session_ids, String revoked_at) implements SchemaTypes.GeneratedResponse {
            public SessionRevokeAllResponse {
                revoked_session_ids = List.copyOf(Objects.requireNonNull(revoked_session_ids, "revoked_session_ids"));
                Objects.requireNonNull(revoked_at, "revoked_at");
            }
        }
        public static final SchemaTypes.TypedOperation<SessionRevokeAllRequest, SessionRevokeAllResponse> SESSION_REVOKE_ALL = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "session.revoke-all", false, SessionRevokeAllRequest.class, SessionRevokeAllResponse.class);
        public record StepupBeginRequest(String confirms) implements SchemaTypes.GeneratedRequest {
            public StepupBeginRequest {
                Objects.requireNonNull(confirms, "confirms");
            }
        }
        public record StepupBeginResponse(String challenge_id, String confirms, String ceremony, String expires_at) implements SchemaTypes.GeneratedResponse {
            public StepupBeginResponse {
                Objects.requireNonNull(challenge_id, "challenge_id");
                Objects.requireNonNull(confirms, "confirms");
                Objects.requireNonNull(ceremony, "ceremony");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public static final SchemaTypes.TypedOperation<StepupBeginRequest, StepupBeginResponse> STEPUP_BEGIN = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "stepup.begin", false, StepupBeginRequest.class, StepupBeginResponse.class);
        public record StepupFinishRequest(String credential) implements SchemaTypes.GeneratedRequest {
            public StepupFinishRequest {
                Objects.requireNonNull(credential, "credential");
            }
        }
        public record StepupFinishResponse(String challenge_id, String confirms, String passkey_id, String completed_at, String expires_at) implements SchemaTypes.GeneratedResponse {
            public StepupFinishResponse {
                Objects.requireNonNull(challenge_id, "challenge_id");
                Objects.requireNonNull(confirms, "confirms");
                Objects.requireNonNull(passkey_id, "passkey_id");
                Objects.requireNonNull(completed_at, "completed_at");
                Objects.requireNonNull(expires_at, "expires_at");
            }
        }
        public static final SchemaTypes.TypedOperation<StepupFinishRequest, StepupFinishResponse> STEPUP_FINISH = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "stepup.finish", false, StepupFinishRequest.class, StepupFinishResponse.class);
        public record StreamNextRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record StreamNextResponse(List<HumanModels.StreamEvent> events, String next_cursor) implements SchemaTypes.GeneratedResponse {
            public StreamNextResponse {
                events = List.copyOf(Objects.requireNonNull(events, "events"));
                Objects.requireNonNull(next_cursor, "next_cursor");
            }
        }
        public static final SchemaTypes.TypedOperation<StreamNextRequest, StreamNextResponse> STREAM_NEXT = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "stream.next", false, StreamNextRequest.class, StreamNextResponse.class);
        public record StreamOpenRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record StreamOpenResponse(String cursor) implements SchemaTypes.GeneratedResponse {
            public StreamOpenResponse {
                Objects.requireNonNull(cursor, "cursor");
            }
        }
        public static final SchemaTypes.TypedOperation<StreamOpenRequest, StreamOpenResponse> STREAM_OPEN = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "stream.open", false, StreamOpenRequest.class, StreamOpenResponse.class);
        public record SupportCreateRequest(String body, HumanModels.SupportShell shell, HumanModels.SupportTopic topic, String trace_id) implements SchemaTypes.GeneratedRequest {
            public SupportCreateRequest {
                Objects.requireNonNull(body, "body");
                Objects.requireNonNull(shell, "shell");
            }
        }
        public record SupportCreateResponse(String conversation_id, HumanModels.SupportShell shell, HumanModels.SupportConversationState state, String created_at, String updated_at, List<HumanModels.SupportMessage> messages, List<HumanModels.SupportFeedback> feedback, String trace_id) implements SchemaTypes.GeneratedResponse {
            public SupportCreateResponse {
                Objects.requireNonNull(conversation_id, "conversation_id");
                Objects.requireNonNull(shell, "shell");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(created_at, "created_at");
                Objects.requireNonNull(updated_at, "updated_at");
                messages = List.copyOf(Objects.requireNonNull(messages, "messages"));
                feedback = List.copyOf(Objects.requireNonNull(feedback, "feedback"));
            }
        }
        public static final SchemaTypes.TypedOperation<SupportCreateRequest, SupportCreateResponse> SUPPORT_CREATE = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "support.create", true, SupportCreateRequest.class, SupportCreateResponse.class);
        public record SupportFeedbackRequest(String message_id, boolean helpful) implements SchemaTypes.GeneratedRequest {
            public SupportFeedbackRequest {
                Objects.requireNonNull(message_id, "message_id");
            }
        }
        public record SupportFeedbackResponse(String conversation_id, HumanModels.SupportShell shell, HumanModels.SupportConversationState state, String created_at, String updated_at, List<HumanModels.SupportMessage> messages, List<HumanModels.SupportFeedback> feedback, String trace_id) implements SchemaTypes.GeneratedResponse {
            public SupportFeedbackResponse {
                Objects.requireNonNull(conversation_id, "conversation_id");
                Objects.requireNonNull(shell, "shell");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(created_at, "created_at");
                Objects.requireNonNull(updated_at, "updated_at");
                messages = List.copyOf(Objects.requireNonNull(messages, "messages"));
                feedback = List.copyOf(Objects.requireNonNull(feedback, "feedback"));
            }
        }
        public static final SchemaTypes.TypedOperation<SupportFeedbackRequest, SupportFeedbackResponse> SUPPORT_FEEDBACK = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "support.feedback", false, SupportFeedbackRequest.class, SupportFeedbackResponse.class);
        public record SupportListRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record SupportListResponse(List<HumanModels.SupportConversation> conversations) implements SchemaTypes.GeneratedResponse {
            public SupportListResponse {
                conversations = List.copyOf(Objects.requireNonNull(conversations, "conversations"));
            }
        }
        public static final SchemaTypes.TypedOperation<SupportListRequest, SupportListResponse> SUPPORT_LIST = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "support.list", false, SupportListRequest.class, SupportListResponse.class);
        public record SupportReadRequest(String through_message_id) implements SchemaTypes.GeneratedRequest {
            public SupportReadRequest {
                Objects.requireNonNull(through_message_id, "through_message_id");
            }
        }
        public record SupportReadResponse(String conversation_id, HumanModels.SupportConversationState state, long unread_count, String updated_at) implements SchemaTypes.GeneratedResponse {
            public SupportReadResponse {
                Objects.requireNonNull(conversation_id, "conversation_id");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<SupportReadRequest, SupportReadResponse> SUPPORT_READ = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "support.read", false, SupportReadRequest.class, SupportReadResponse.class);
        public record SupportReplyRequest(String body) implements SchemaTypes.GeneratedRequest {
            public SupportReplyRequest {
                Objects.requireNonNull(body, "body");
            }
        }
        public record SupportReplyResponse(String conversation_id, HumanModels.SupportShell shell, HumanModels.SupportConversationState state, String created_at, String updated_at, List<HumanModels.SupportMessage> messages, List<HumanModels.SupportFeedback> feedback, String trace_id) implements SchemaTypes.GeneratedResponse {
            public SupportReplyResponse {
                Objects.requireNonNull(conversation_id, "conversation_id");
                Objects.requireNonNull(shell, "shell");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(created_at, "created_at");
                Objects.requireNonNull(updated_at, "updated_at");
                messages = List.copyOf(Objects.requireNonNull(messages, "messages"));
                feedback = List.copyOf(Objects.requireNonNull(feedback, "feedback"));
            }
        }
        public static final SchemaTypes.TypedOperation<SupportReplyRequest, SupportReplyResponse> SUPPORT_REPLY = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "support.reply", true, SupportReplyRequest.class, SupportReplyResponse.class);
        public record SupportStatusRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record SupportStatusResponse(String conversation_id, HumanModels.SupportConversationState state, long unread_count, String updated_at) implements SchemaTypes.GeneratedResponse {
            public SupportStatusResponse {
                Objects.requireNonNull(conversation_id, "conversation_id");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<SupportStatusRequest, SupportStatusResponse> SUPPORT_STATUS = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "support.status", false, SupportStatusRequest.class, SupportStatusResponse.class);
        public record VersionRequest() implements SchemaTypes.GeneratedRequest {
        }
        public record VersionResponse(HumanModels.SchemaVersion schema, String service) implements SchemaTypes.GeneratedResponse {
            public VersionResponse {
                Objects.requireNonNull(schema, "schema");
                Objects.requireNonNull(service, "service");
            }
        }
        public static final SchemaTypes.TypedOperation<VersionRequest, VersionResponse> VERSION = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "version", false, VersionRequest.class, VersionResponse.class);
        public record WithdrawClaimRequest(String claim_signature, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedRequest {
            public WithdrawClaimRequest {
                Objects.requireNonNull(claim_signature, "claim_signature");
            }
        }
        public record WithdrawClaimResponse(String journey_id, HumanModels.JourneyKind kind, HumanModels.JourneyState state, String state_copy_key, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, String started_at, String updated_at, HumanModels.Refusal refusal, HumanModels.WalletSignRequest wallet_request) implements SchemaTypes.GeneratedResponse {
            public WithdrawClaimResponse {
                Objects.requireNonNull(journey_id, "journey_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(started_at, "started_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<WithdrawClaimRequest, WithdrawClaimResponse> WITHDRAW_CLAIM = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "withdraw.claim", false, WithdrawClaimRequest.class, WithdrawClaimResponse.class);
        public record WithdrawStartRequest(HumanModels.Money money, String destination, HumanModels.SettlementDomain settlement_domain) implements SchemaTypes.GeneratedRequest {
            public WithdrawStartRequest {
                Objects.requireNonNull(money, "money");
                Objects.requireNonNull(destination, "destination");
            }
        }
        public record WithdrawStartResponse(String journey_id, HumanModels.JourneyKind kind, HumanModels.JourneyState state, String state_copy_key, List<HumanModels.JourneyStage> stages, List<HumanModels.EvidenceRef> evidence, String started_at, String updated_at, HumanModels.Refusal refusal, HumanModels.WalletSignRequest wallet_request) implements SchemaTypes.GeneratedResponse {
            public WithdrawStartResponse {
                Objects.requireNonNull(journey_id, "journey_id");
                Objects.requireNonNull(kind, "kind");
                Objects.requireNonNull(state, "state");
                Objects.requireNonNull(state_copy_key, "state_copy_key");
                stages = List.copyOf(Objects.requireNonNull(stages, "stages"));
                evidence = List.copyOf(Objects.requireNonNull(evidence, "evidence"));
                Objects.requireNonNull(started_at, "started_at");
                Objects.requireNonNull(updated_at, "updated_at");
            }
        }
        public static final SchemaTypes.TypedOperation<WithdrawStartRequest, WithdrawStartResponse> WITHDRAW_START = new SchemaTypes.TypedOperation<>(OperationCatalog.Plane.HUMAN, "withdraw.start", true, WithdrawStartRequest.class, WithdrawStartResponse.class);
    }
    public static final Map<String, SchemaTypes.TypedOperation<?, ?>> AGENT = Map.ofEntries(
        Map.entry("agent.register", AgentOperations.AGENT_REGISTER ),
        Map.entry("approval.approve", AgentOperations.APPROVAL_APPROVE ),
        Map.entry("approval.get", AgentOperations.APPROVAL_GET ),
        Map.entry("approval.list", AgentOperations.APPROVAL_LIST ),
        Map.entry("approval.reject", AgentOperations.APPROVAL_REJECT ),
        Map.entry("availability.fetch", AgentOperations.AVAILABILITY_FETCH ),
        Map.entry("budget.create", AgentOperations.BUDGET_CREATE ),
        Map.entry("budget.fund", AgentOperations.BUDGET_FUND ),
        Map.entry("budget.list", AgentOperations.BUDGET_LIST ),
        Map.entry("budget.reconciliation", AgentOperations.BUDGET_RECONCILIATION ),
        Map.entry("budget.revoke", AgentOperations.BUDGET_REVOKE ),
        Map.entry("capability.attenuate", AgentOperations.CAPABILITY_ATTENUATE ),
        Map.entry("capability.create", AgentOperations.CAPABILITY_CREATE ),
        Map.entry("capability.list", AgentOperations.CAPABILITY_LIST ),
        Map.entry("capability.revoke", AgentOperations.CAPABILITY_REVOKE ),
        Map.entry("export.offline", AgentOperations.EXPORT_OFFLINE ),
        Map.entry("prepare", AgentOperations.PREPARE ),
        Map.entry("program.activity", AgentOperations.PROGRAM_ACTIVITY ),
        Map.entry("program.call", AgentOperations.PROGRAM_CALL ),
        Map.entry("program.discover", AgentOperations.PROGRAM_DISCOVER ),
        Map.entry("program.interface", AgentOperations.PROGRAM_INTERFACE ),
        Map.entry("program.receipt", AgentOperations.PROGRAM_RECEIPT ),
        Map.entry("program.simulate", AgentOperations.PROGRAM_SIMULATE ),
        Map.entry("project", AgentOperations.PROJECT ),
        Map.entry("read.account", AgentOperations.READ_ACCOUNT ),
        Map.entry("read.balance", AgentOperations.READ_BALANCE ),
        Map.entry("read.batch", AgentOperations.READ_BATCH ),
        Map.entry("read.checkpoint", AgentOperations.READ_CHECKPOINT ),
        Map.entry("read.history", AgentOperations.READ_HISTORY ),
        Map.entry("read.module_state", AgentOperations.READ_MODULE_STATE ),
        Map.entry("read.proof_bundle", AgentOperations.READ_PROOF_BUNDLE ),
        Map.entry("session.close", AgentOperations.SESSION_CLOSE ),
        Map.entry("session.list", AgentOperations.SESSION_LIST ),
        Map.entry("session.open", AgentOperations.SESSION_OPEN ),
        Map.entry("session.refresh", AgentOperations.SESSION_REFRESH ),
        Map.entry("sign", AgentOperations.SIGN ),
        Map.entry("submit", AgentOperations.SUBMIT ),
        Map.entry("subscription.acknowledge", AgentOperations.SUBSCRIPTION_ACKNOWLEDGE ),
        Map.entry("subscription.create", AgentOperations.SUBSCRIPTION_CREATE ),
        Map.entry("subscription.delete", AgentOperations.SUBSCRIPTION_DELETE ),
        Map.entry("subscription.health", AgentOperations.SUBSCRIPTION_HEALTH ),
        Map.entry("subscription.list", AgentOperations.SUBSCRIPTION_LIST ),
        Map.entry("subscription.pause", AgentOperations.SUBSCRIPTION_PAUSE ),
        Map.entry("subscription.resume", AgentOperations.SUBSCRIPTION_RESUME ),
        Map.entry("track", AgentOperations.TRACK ),
        Map.entry("wait", AgentOperations.WAIT ));
    public static final Map<String, SchemaTypes.TypedOperation<?, ?>> HUMAN = Map.ofEntries(
        Map.entry("account.balance", HumanOperations.ACCOUNT_BALANCE ),
        Map.entry("account.create", HumanOperations.ACCOUNT_CREATE ),
        Map.entry("activity.entry", HumanOperations.ACTIVITY_ENTRY ),
        Map.entry("activity.export.evidence", HumanOperations.ACTIVITY_EXPORT_EVIDENCE ),
        Map.entry("activity.export.statement", HumanOperations.ACTIVITY_EXPORT_STATEMENT ),
        Map.entry("activity.query", HumanOperations.ACTIVITY_QUERY ),
        Map.entry("agent.archive", HumanOperations.AGENT_ARCHIVE ),
        Map.entry("agent.create", HumanOperations.AGENT_CREATE ),
        Map.entry("agent.get", HumanOperations.AGENT_GET ),
        Map.entry("agent.limit", HumanOperations.AGENT_LIMIT ),
        Map.entry("agent.list", HumanOperations.AGENT_LIST ),
        Map.entry("agent.pause", HumanOperations.AGENT_PAUSE ),
        Map.entry("agent.reclaim", HumanOperations.AGENT_RECLAIM ),
        Map.entry("agent.recover", HumanOperations.AGENT_RECOVER ),
        Map.entry("agent.resume", HumanOperations.AGENT_RESUME ),
        Map.entry("agent.rotate", HumanOperations.AGENT_ROTATE ),
        Map.entry("approval.approve", HumanOperations.APPROVAL_APPROVE ),
        Map.entry("approval.get", HumanOperations.APPROVAL_GET ),
        Map.entry("approval.list", HumanOperations.APPROVAL_LIST ),
        Map.entry("approval.reject", HumanOperations.APPROVAL_REJECT ),
        Map.entry("authenticator.backup.rotate", HumanOperations.AUTHENTICATOR_BACKUP_ROTATE ),
        Map.entry("authenticator.disable", HumanOperations.AUTHENTICATOR_DISABLE ),
        Map.entry("authenticator.setup.begin", HumanOperations.AUTHENTICATOR_SETUP_BEGIN ),
        Map.entry("authenticator.setup.finish", HumanOperations.AUTHENTICATOR_SETUP_FINISH ),
        Map.entry("authenticator.status", HumanOperations.AUTHENTICATOR_STATUS ),
        Map.entry("binding.rebind", HumanOperations.BINDING_REBIND ),
        Map.entry("binding.rebind.action", HumanOperations.BINDING_REBIND_ACTION ),
        Map.entry("binding.statement", HumanOperations.BINDING_STATEMENT ),
        Map.entry("binding.status", HumanOperations.BINDING_STATUS ),
        Map.entry("binding.submit", HumanOperations.BINDING_SUBMIT ),
        Map.entry("deposit.confirm", HumanOperations.DEPOSIT_CONFIRM ),
        Map.entry("deposit.start", HumanOperations.DEPOSIT_START ),
        Map.entry("evidence.get", HumanOperations.EVIDENCE_GET ),
        Map.entry("exit.eligibility", HumanOperations.EXIT_ELIGIBILITY ),
        Map.entry("exit.start", HumanOperations.EXIT_START ),
        Map.entry("home.summary", HumanOperations.HOME_SUMMARY ),
        Map.entry("journey.get", HumanOperations.JOURNEY_GET ),
        Map.entry("journey.list", HumanOperations.JOURNEY_LIST ),
        Map.entry("move.commit", HumanOperations.MOVE_COMMIT ),
        Map.entry("move.quote", HumanOperations.MOVE_QUOTE ),
        Map.entry("notification.list", HumanOperations.NOTIFICATION_LIST ),
        Map.entry("notification.preferences.get", HumanOperations.NOTIFICATION_PREFERENCES_GET ),
        Map.entry("notification.preferences.set", HumanOperations.NOTIFICATION_PREFERENCES_SET ),
        Map.entry("notification.read", HumanOperations.NOTIFICATION_READ ),
        Map.entry("onboarding.resume", HumanOperations.ONBOARDING_RESUME ),
        Map.entry("onboarding.status", HumanOperations.ONBOARDING_STATUS ),
        Map.entry("passkey.assert.begin", HumanOperations.PASSKEY_ASSERT_BEGIN ),
        Map.entry("passkey.assert.finish", HumanOperations.PASSKEY_ASSERT_FINISH ),
        Map.entry("passkey.register.begin", HumanOperations.PASSKEY_REGISTER_BEGIN ),
        Map.entry("passkey.register.finish", HumanOperations.PASSKEY_REGISTER_FINISH ),
        Map.entry("profile.get", HumanOperations.PROFILE_GET ),
        Map.entry("profile.update", HumanOperations.PROFILE_UPDATE ),
        Map.entry("security.action", HumanOperations.SECURITY_ACTION ),
        Map.entry("security.passkey.list", HumanOperations.SECURITY_PASSKEY_LIST ),
        Map.entry("security.passkey.register.begin", HumanOperations.SECURITY_PASSKEY_REGISTER_BEGIN ),
        Map.entry("security.passkey.register.finish", HumanOperations.SECURITY_PASSKEY_REGISTER_FINISH ),
        Map.entry("security.passkey.revoke", HumanOperations.SECURITY_PASSKEY_REVOKE ),
        Map.entry("security.recovery.reveal", HumanOperations.SECURITY_RECOVERY_REVEAL ),
        Map.entry("security.session.revoke", HumanOperations.SECURITY_SESSION_REVOKE ),
        Map.entry("security.session.revoke-all", HumanOperations.SECURITY_SESSION_REVOKE_ALL ),
        Map.entry("session.list", HumanOperations.SESSION_LIST ),
        Map.entry("session.open", HumanOperations.SESSION_OPEN ),
        Map.entry("session.refresh", HumanOperations.SESSION_REFRESH ),
        Map.entry("session.revoke", HumanOperations.SESSION_REVOKE ),
        Map.entry("session.revoke-all", HumanOperations.SESSION_REVOKE_ALL ),
        Map.entry("stepup.begin", HumanOperations.STEPUP_BEGIN ),
        Map.entry("stepup.finish", HumanOperations.STEPUP_FINISH ),
        Map.entry("stream.next", HumanOperations.STREAM_NEXT ),
        Map.entry("stream.open", HumanOperations.STREAM_OPEN ),
        Map.entry("support.create", HumanOperations.SUPPORT_CREATE ),
        Map.entry("support.feedback", HumanOperations.SUPPORT_FEEDBACK ),
        Map.entry("support.list", HumanOperations.SUPPORT_LIST ),
        Map.entry("support.read", HumanOperations.SUPPORT_READ ),
        Map.entry("support.reply", HumanOperations.SUPPORT_REPLY ),
        Map.entry("support.status", HumanOperations.SUPPORT_STATUS ),
        Map.entry("version", HumanOperations.VERSION ),
        Map.entry("withdraw.claim", HumanOperations.WITHDRAW_CLAIM ),
        Map.entry("withdraw.start", HumanOperations.WITHDRAW_START ));
}
