package com.sidiora.layerx.sdk;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.node.ArrayNode;
import com.fasterxml.jackson.databind.node.JsonNodeFactory;
import com.fasterxml.jackson.databind.node.ObjectNode;
import com.sidiora.layerx.sdk.verify.LocalVerifier;
import java.math.BigInteger;
import java.nio.ByteBuffer;
import java.nio.charset.StandardCharsets;
import java.time.Clock;
import java.time.Duration;
import java.security.KeyFactory;
import java.security.MessageDigest;
import java.security.Signature;
import java.security.spec.X509EncodedKeySpec;
import java.util.Arrays;
import java.util.HexFormat;
import java.util.List;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.concurrent.CompletionStage;

public final class ProgramsClient {
    public static final int MAX_CALLDATA_BYTES = 1_048_576;
    public static final int MAX_CAPABILITIES = 5;
    public static final int PROGRAMS_RECEIPT_MODULE_ID = 9;
    public static final int CALL_OPERATION = 3;
    private static final int MAX_LEGACY_VALUES = 512;
    private static final int MAX_INTERFACE_BYTES = 952;
    private static final String SEQUENCER_SIGNED = "sequencer-signed";
    private static final String EXECUTION_VERIFICATION = "receipt-terminal-and-call-graph-verified";
    private static final BigInteger MAX_U32 = BigInteger.ONE.shiftLeft(32).subtract(BigInteger.ONE);
    private static final BigInteger MAX_U64 = BigInteger.ONE.shiftLeft(64).subtract(BigInteger.ONE);
    private static final byte[] SIMULATION_BOUNDARY_DOMAIN =
        "LayerX/emulator/simulation-boundary/v1\0".getBytes(StandardCharsets.UTF_8);
    private static final byte[] SIMULATION_EVIDENCE_DOMAIN =
        "LayerX/agent/program-simulation-evidence/v1\0".getBytes(StandardCharsets.UTF_8);
    private static final byte[] ED25519_X509_PREFIX = HexFormat.of().parseHex("302a300506032b6570032100");
    private static final byte[] ACTIVITY_ID_DOMAIN = "LXP/v1/activity-id\0".getBytes(StandardCharsets.UTF_8);
    private static final byte[] PAYLOAD_HASH_DOMAIN = "LXP/v1/payload-hash\0".getBytes(StandardCharsets.UTF_8);
    private static final byte[] PROGRAM_CALL_DOMAIN = "LayerX/programs/call/v1\0".getBytes(StandardCharsets.UTF_8);
    private record ActivityBinding(byte[] activityId, byte[] idempotencyKey,
                                   BigInteger notBefore, BigInteger notAfter) {
        private ActivityBinding {
            activityId = activityId.clone();
            idempotencyKey = idempotencyKey.clone();
        }
        @Override public byte[] activityId() { return activityId.clone(); }
        @Override public byte[] idempotencyKey() { return idempotencyKey.clone(); }
    }

    public enum Capability {
        STORAGE_READ("storage_read"), STORAGE_WRITE("storage_write"), TRANSFER("transfer"),
        EMIT_EVENT("emit_event"), COMPOSE("compose");
        private final String wire;
        Capability(String wire) { this.wire = wire; }
        public String wire() { return wire; }
    }

    public enum SubmissionState {
        REFUSED("refused"), UNKNOWN("unknown"), EXECUTED("executed");
        private final String wire;
        SubmissionState(String wire) { this.wire = wire; }
        public String wire() { return wire; }
        private static SubmissionState parse(String value) {
            for (SubmissionState state : values()) if (state.wire.equals(value)) return state;
            throw decodeFailure();
        }
    }

    public record Budget(BigInteger fuel, BigInteger feeLimit) {
        public Budget {
            if (fuel == null || fuel.signum() <= 0 || fuel.bitLength() > 64
                    || feeLimit == null || feeLimit.signum() < 0 || feeLimit.bitLength() > 128) invalid();
        }
    }

    public record Call(byte[] programId, byte[] calldata, Budget budget,
                       List<Capability> capabilities, byte[] signedActivity) {
        public Call {
            programId = exactArgument(programId, 32);
            if (calldata == null || calldata.length > MAX_CALLDATA_BYTES || budget == null
                    || signedActivity == null || signedActivity.length == 0
                    || signedActivity.length > MAX_CALLDATA_BYTES || capabilities == null
                    || capabilities.size() > MAX_CAPABILITIES || allZero(programId)) invalid();
            capabilities = List.copyOf(capabilities);
            if (capabilities.stream().anyMatch(Objects::isNull)) invalid();
            for (int index = 1; index < capabilities.size(); index++) {
                if (capabilities.get(index - 1).ordinal() >= capabilities.get(index).ordinal()) invalid();
            }
            calldata = calldata.clone();
            signedActivity = signedActivity.clone();
        }
        @Override public byte[] programId() { return programId.clone(); }
        @Override public byte[] calldata() { return calldata.clone(); }
        @Override public byte[] signedActivity() { return signedActivity.clone(); }
    }

    public record Discovery(ObjectNode value) {
        public Discovery { value = copy(value); }
    }

    public record Interface(ObjectNode value) {
        public Interface { value = copy(value); }
    }

    public record VerifiedExecution(ObjectNode document, LocalVerifier.ReceiptVerification receipt,
                                    byte[] terminalPayload, byte[] callGraph) {
        public VerifiedExecution {
            document = copy(document);
            Objects.requireNonNull(receipt, "receipt");
            terminalPayload = terminalPayload.clone();
            callGraph = callGraph.clone();
        }
        @Override public byte[] terminalPayload() { return terminalPayload.clone(); }
        @Override public byte[] callGraph() { return callGraph.clone(); }
        public int guestAbiVersion() { return requiredU16(document.get("guest_abi_version")); }
        public byte[] terminalPayloadRoot() {
            return receipt.receipt().programOutcome().terminalPayloadRoot().clone();
        }
        public byte[] callGraphRoot() {
            return receipt.receipt().programOutcome().callGraphRoot().clone();
        }
    }

    public record Simulation(ObjectNode value, VerifiedExecution execution) {
        public Simulation {
            value = copy(value);
            Objects.requireNonNull(execution, "execution");
        }
        public boolean committed() { return false; }
    }

    public record Submission(SubmissionState state, byte[] activityId, String idempotencyKey,
                             byte[] retainedSignedActivity, VerifiedExecution execution, ObjectNode value) {
        public Submission {
            Objects.requireNonNull(state, "state");
            activityId = exactVerification(activityId, 32);
            if (!canonicalLowerHex(idempotencyKey, 32)) invalidVerification();
            retainedSignedActivity = retainedSignedActivity == null ? null : retainedSignedActivity.clone();
            if (state == SubmissionState.UNKNOWN && execution != null
                    || state != SubmissionState.UNKNOWN && execution == null) invalidVerification();
            value = copy(value);
        }
        @Override public byte[] activityId() { return activityId.clone(); }
        @Override public byte[] retainedSignedActivity() {
            return retainedSignedActivity == null ? null : retainedSignedActivity.clone();
        }
        public boolean unknown() { return state == SubmissionState.UNKNOWN; }
    }

    private final ProductionClient client;
    private final byte[] sequencerPublicKey;
    private final Clock clock;
    private final BigInteger maximumSimulationAgeMillis;

    public ProgramsClient(ProductionClient client, byte[] sequencerPublicKey) {
        this(client, sequencerPublicKey, Clock.systemUTC(), Duration.ofMinutes(5));
    }

    public ProgramsClient(ProductionClient client, byte[] sequencerPublicKey, Clock clock,
                          Duration maximumSimulationAge) {
        this.client = Objects.requireNonNull(client, "client");
        this.sequencerPublicKey = exactArgument(sequencerPublicKey, 32);
        if (allZero(this.sequencerPublicKey)) invalid();
        this.clock = Objects.requireNonNull(clock, "clock");
        Objects.requireNonNull(maximumSimulationAge, "maximumSimulationAge");
        if (maximumSimulationAge.isZero() || maximumSimulationAge.isNegative()) invalid();
        this.maximumSimulationAgeMillis = durationMillis(maximumSimulationAge);
    }

    public CompletionStage<Discovery> discover(byte[] programId) {
        return discover(programId, SEQUENCER_SIGNED);
    }

    public CompletionStage<Discovery> discover(byte[] programId, String verificationLevel) {
        requireSequencerSigned(verificationLevel);
        String id = hex(exactArgument(programId, 32));
        ObjectNode body = object().put("program_id", id)
            .put("requested_verification_level", SEQUENCER_SIGNED);
        return raw("program.discover", body, Map.of("program_id", id), null)
            .thenApply(value -> decodeDiscovery(value, id, BigInteger.valueOf(clock.millis())));
    }

    public CompletionStage<Interface> interfaceAt(byte[] programId) {
        return interfaceAt(programId, SEQUENCER_SIGNED);
    }

    public CompletionStage<Interface> interfaceAt(byte[] programId, String verificationLevel) {
        requireSequencerSigned(verificationLevel);
        String id = hex(exactArgument(programId, 32));
        ObjectNode body = object().put("program_id", id)
            .put("requested_verification_level", SEQUENCER_SIGNED);
        return raw("program.interface", body, Map.of("program_id", id), null)
            .thenApply(value -> decodeInterface(value, id, BigInteger.valueOf(clock.millis())));
    }

    public CompletionStage<Simulation> simulate(Call call) {
        Objects.requireNonNull(call, "call");
        ActivityBinding activity = decodeSignedCall(call);
        return raw("program.simulate", encode(call), Map.of(), null)
            .thenApply(value -> decodeSimulation(value, call.programId(), activity, sequencerPublicKey,
                BigInteger.valueOf(clock.millis()), maximumSimulationAgeMillis));
    }

    public CompletionStage<Submission> submit(Call call, IdempotencyKey key) {
        Objects.requireNonNull(call, "call");
        Objects.requireNonNull(key, "key");
        if (!canonicalLowerHex(key.value(), 32)) throw new PlatformSdkException(
            PlatformSdkException.Code.IDEMPOTENCY_REQUIRED, PlatformSdkException.Retry.NEVER,
            null, null, null);
        ActivityBinding activity = decodeSignedCall(call);
        if (!MessageDigest.isEqual(activity.idempotencyKey(), HexFormat.of().parseHex(key.value()))) invalid();
        return raw("program.call", encode(call), Map.of(), key).handle((value, error) -> {
            if (error != null) throw new java.util.concurrent.CompletionException(error);
            try {
                return decodeSubmission(value, call.programId(), activity.activityId(), key.value(),
                    call.signedActivity(), sequencerPublicKey);
            } catch (PlatformSdkException failure) {
                if (failure.code() == PlatformSdkException.Code.DECODE_FAILURE
                        || failure.code() == PlatformSdkException.Code.VERIFICATION_FAILURE) {
                    throw new java.util.concurrent.CompletionException(unknownOutcome());
                }
                throw failure;
            }
        });
    }

    public CompletionStage<Submission> receipt(IdempotencyKey key, byte[] expectedActivityId) {
        return receipt(key, expectedActivityId, SEQUENCER_SIGNED);
    }

    public CompletionStage<Submission> receipt(IdempotencyKey key, byte[] expectedActivityId,
                                                String verificationLevel) {
        Objects.requireNonNull(key, "key");
        if (!canonicalLowerHex(key.value(), 32)) invalid();
        requireSequencerSigned(verificationLevel);
        byte[] expected = exactArgument(expectedActivityId, 32);
        String activity = hex(expected);
        ObjectNode body = object().put("idempotency_key", key.value())
            .put("expected_activity_id", activity)
            .put("requested_verification_level", SEQUENCER_SIGNED);
        return raw("program.receipt", body, Map.of("idempotency_key", key.value()), null)
            .thenApply(value -> decodeSubmission(value, null, expected, key.value(), null,
                sequencerPublicKey));
    }

    public CompletionStage<Submission> activity(byte[] activityId) {
        return activity(activityId, SEQUENCER_SIGNED);
    }

    public CompletionStage<Submission> activity(byte[] activityId, String verificationLevel) {
        requireSequencerSigned(verificationLevel);
        byte[] expected = exactArgument(activityId, 32);
        String id = hex(expected);
        ObjectNode body = object().put("activity_id", id)
            .put("requested_verification_level", SEQUENCER_SIGNED);
        return raw("program.activity", body, Map.of("activity_id", id), null)
            .thenApply(value -> decodeSubmission(value, null, expected, null, null,
                sequencerPublicKey));
    }

    public static LocalVerifier.ReceiptVerification verifyReceipt(byte[] canonicalReceipt,
            LocalVerifier.AuthorizedReceiptBatch authorized, byte[] expectedActivityId,
            int expectedGuestAbiVersion, byte[] terminalPayload, byte[] callGraph) {
        if (expectedGuestAbiVersion != 1 && expectedGuestAbiVersion != 2) invalid();
        LocalVerifier.ReceiptVerification verified = LocalVerifier.verifyReceiptOutcome(canonicalReceipt, authorized);
        LocalVerifier.ProtocolReceipt receipt = verified.receipt();
        LocalVerifier.ProgramReceiptOutcome outcome = receipt.programOutcome();
        if (receipt.protocolVersion() == 0 || receipt.moduleId() != PROGRAMS_RECEIPT_MODULE_ID
                || receipt.operation() != CALL_OPERATION || receipt.moduleVersion() < 1
                || receipt.moduleVersion() > 3
                || !Arrays.equals(receipt.activityId(), exactArgument(expectedActivityId, 32))
                || outcome == null || outcome.abiVersion() != expectedGuestAbiVersion
                || callGraph == null || callGraph.length == 0 || callGraph.length > MAX_CALLDATA_BYTES
                || terminalPayload == null || terminalPayload.length > MAX_CALLDATA_BYTES
                || !MessageDigest.isEqual(sha256(terminalPayload), outcome.terminalPayloadRoot())
                || !MessageDigest.isEqual(sha256(callGraph), outcome.callGraphRoot())) invalidVerification();
        return verified;
    }

    private CompletionStage<ObjectNode> raw(String operation, ObjectNode body, Map<String, String> path,
                                             IdempotencyKey key) {
        return client.programs(operation, body, ObjectNode.class, new ProductionClient.Options(key, path));
    }

    private static Discovery decodeDiscovery(ObjectNode value, String expectedProgram, BigInteger now) {
        requireFields(value, "program_id", "lifecycle", "version", "code_hash", "abi_version",
            "receipt_digest", "state_root", "observed_sequence", "observed_at", "valid_through",
            "verification");
        String lifecycle = text(value, "lifecycle");
        BigInteger observedAt = unsigned(value.get("observed_at"), 64);
        int abi = requiredU16(value.get("abi_version"));
        if (!expectedProgram.equals(text(value, "program_id"))
                || !Set.of("active", "deprecated", "tombstoned").contains(lifecycle)
                || requiredU32(value.get("version")) == 0 || abi < 1 || abi > 2
                || !canonicalLowerHex(text(value, "code_hash"), 32)
                || !canonicalLowerHex(text(value, "receipt_digest"), 32)
                || !canonicalLowerHex(text(value, "state_root"), 32)
                || unsigned(value.get("valid_through"), 64).compareTo(observedAt) < 0
                || now.compareTo(unsigned(value.get("valid_through"), 64)) > 0
                || !"registry-receipt-and-current-head-verified".equals(text(value, "verification"))) {
            throw decodeFailure();
        }
        unsigned(value.get("observed_sequence"), 64);
        return new Discovery(value);
    }

    private static Interface decodeInterface(ObjectNode value, String expectedProgram, BigInteger now) {
        requireFields(value, "program_id", "version", "code_hash", "abi_version", "interface",
            "interface_digest", "receipt_digest", "state_root", "observed_sequence", "observed_at",
            "valid_through", "source", "verification");
        BigInteger observedAt = unsigned(value.get("observed_at"), 64);
        int abi = requiredU16(value.get("abi_version"));
        if (!expectedProgram.equals(text(value, "program_id")) || requiredU32(value.get("version")) == 0
                || abi < 1 || abi > 2 || !canonicalLowerHex(text(value, "code_hash"), 32)
                || !canonicalLowerHex(text(value, "receipt_digest"), 32)
                || !canonicalLowerHex(text(value, "state_root"), 32)
                || unsigned(value.get("valid_through"), 64).compareTo(observedAt) < 0
                || now.compareTo(unsigned(value.get("valid_through"), 64)) > 0
                || !"deployment-interface-and-current-head-verified".equals(text(value, "verification"))) {
            throw decodeFailure();
        }
        unsigned(value.get("observed_sequence"), 64);
        byte[] interfaceBytes = boundedHex(text(value, "interface"), false);
        if (interfaceBytes.length > MAX_INTERFACE_BYTES) throw decodeFailure();
        byte[] interfaceDigest = hex32(text(value, "interface_digest"));
        if (!MessageDigest.isEqual(sha256(interfaceBytes), interfaceDigest)) throw decodeFailure();
        validateSource(object(value, "source"));
        return new Interface(value);
    }

    private static void validateSource(ObjectNode source) {
        String status = text(source, "status");
        switch (status) {
            case "unpublished" -> requireFields(source, "status");
            case "verified" -> {
                requireFields(source, "status", "source_digest", "environment_digest", "pipeline");
                hex32(text(source, "source_digest"));
                hex32(text(source, "environment_digest"));
                if (!"sha256-source-artifact-reproducible-build-v1".equals(text(source, "pipeline"))) {
                    throw decodeFailure();
                }
            }
            case "mismatch" -> {
                requireFields(source, "status", "expected_code_hash", "reproduced_artifact_digest");
                hex32(text(source, "expected_code_hash"));
                hex32(text(source, "reproduced_artifact_digest"));
            }
            default -> throw decodeFailure();
        }
    }

    private static Simulation decodeSimulation(ObjectNode value, byte[] expectedProgram,
                                                ActivityBinding activity, byte[] pinnedKey,
                                                BigInteger now, BigInteger maximumAge) {
        requireFields(value, "committed", "execution", "simulation_evidence");
        if (!value.path("committed").isBoolean() || value.path("committed").booleanValue()) {
            throw decodeFailure();
        }
        ObjectNode executionDocument = object(value, "execution");
        VerifiedExecution execution = verifyExecution(executionDocument, "simulated", false,
            expectedProgram, activity.activityId(), null, pinnedKey);
        verifySimulationEvidence(executionDocument, object(value, "simulation_evidence"), activity,
            pinnedKey, now, maximumAge);
        return new Simulation(value, execution);
    }

    private static Submission decodeSubmission(ObjectNode value, byte[] expectedProgram,
            byte[] expectedActivity, String expectedKey, byte[] expectedSignedActivity, byte[] pinnedKey) {
        SubmissionState state = SubmissionState.parse(text(value, "state"));
        if (state == SubmissionState.UNKNOWN) {
            boolean retained = value.has("retained_signed_activity");
            if (expectedSignedActivity != null || retained) {
                requireFields(value, "state", "activity_id", "idempotency_key", "retained_signed_activity");
            } else {
                requireFields(value, "state", "activity_id", "idempotency_key");
            }
            byte[] activity = hex32(text(value, "activity_id"));
            String key = text(value, "idempotency_key");
            if (!canonicalLowerHex(key, 32) || expectedKey != null && !expectedKey.equals(key)) {
                throw decodeFailure();
            }
            byte[] retainedActivity = retained
                ? boundedHex(text(value, "retained_signed_activity"), false) : null;
            if (expectedActivity != null && !MessageDigest.isEqual(activity, expectedActivity)
                    || expectedSignedActivity != null && (retainedActivity == null
                        || !MessageDigest.isEqual(retainedActivity, expectedSignedActivity))) {
                invalidVerification();
            }
            return new Submission(state, activity, key, retainedActivity, null, value);
        }
        String key = text(value, "idempotency_key");
        if (!canonicalLowerHex(key, 32) || expectedKey != null && !expectedKey.equals(key)) {
            throw decodeFailure();
        }
        VerifiedExecution execution = verifyExecution(value, state.wire(), true, expectedProgram,
            expectedActivity, key, pinnedKey);
        String outcomeKind = text(object(value, "outcome"), "kind");
        if (state == SubmissionState.REFUSED && !"refused".equals(outcomeKind)
                || state == SubmissionState.EXECUTED
                    && !Set.of("completed", "legacy_completed").contains(outcomeKind)) {
            invalidVerification();
        }
        return new Submission(state, hex32(text(value, "activity_id")), key, null, execution, value);
    }

    private static VerifiedExecution verifyExecution(ObjectNode document, String expectedState,
            boolean idempotent, byte[] expectedProgram, byte[] expectedActivity, String expectedKey,
            byte[] pinnedKey) {
        String[] base = {"state", "activity_id", "program_id", "guest_abi_version", "module_version",
            "batch_id", "global_sequence", "result_code", "state_root", "receipt", "receipt_digest",
            "terminal_payload", "call_graph", "authority", "usage", "outcome", "verification"};
        if (idempotent) {
            String[] fields = Arrays.copyOf(base, base.length + 1);
            fields[base.length] = "idempotency_key";
            requireFields(document, fields);
        } else {
            requireFields(document, base);
        }
        if (!expectedState.equals(text(document, "state"))) throw decodeFailure();
        byte[] activity = hex32(text(document, "activity_id"));
        byte[] program = hex32(text(document, "program_id"));
        if (expectedProgram != null && !MessageDigest.isEqual(program, expectedProgram)
                || expectedActivity != null && !MessageDigest.isEqual(activity, expectedActivity)
                || expectedKey != null && !expectedKey.equals(text(document, "idempotency_key"))) {
            invalidVerification();
        }
        int guestAbi = requiredU16(document.get("guest_abi_version"));
        long moduleVersion = requiredU32(document.get("module_version"));
        int resultCode = requiredI32(document.get("result_code"));
        BigInteger globalSequence = unsigned(document.get("global_sequence"), 64);
        if ((guestAbi != 1 && guestAbi != 2) || moduleVersion < 1 || moduleVersion > 3
                || !EXECUTION_VERIFICATION.equals(text(document, "verification"))) throw decodeFailure();
        ObjectNode authority = object(document, "authority");
        requireFields(authority, "batch_id", "asset", "previous_state_root", "resulting_state_root",
            "sequencer_public_key");
        byte[] batchId = hex32(text(authority, "batch_id"));
        byte[] asset = hex32(text(authority, "asset"));
        byte[] previousRoot = hex32(text(authority, "previous_state_root"));
        byte[] resultingRoot = hex32(text(authority, "resulting_state_root"));
        byte[] sequencerKey = hex32(text(authority, "sequencer_public_key"));
        if (!MessageDigest.isEqual(batchId, hex32(text(document, "batch_id")))
                || !MessageDigest.isEqual(resultingRoot, hex32(text(document, "state_root")))
                || !MessageDigest.isEqual(sequencerKey, pinnedKey)) {
            invalidVerification();
        }
        ObjectNode usage = object(document, "usage");
        requireFields(usage, "cpu_fuel", "memory_bytes", "storage_read_bytes", "storage_write_bytes",
            "output_values", "output_bytes", "fee_units");
        BigInteger cpuFuel = unsigned(usage.get("cpu_fuel"), 64);
        BigInteger memoryBytes = unsigned(usage.get("memory_bytes"), 64);
        BigInteger storageReadBytes = unsigned(usage.get("storage_read_bytes"), 64);
        BigInteger storageWriteBytes = unsigned(usage.get("storage_write_bytes"), 64);
        long outputValues = requiredU32(usage.get("output_values"));
        BigInteger outputBytes = unsigned(usage.get("output_bytes"), 64);
        BigInteger feeUnits = unsigned(usage.get("fee_units"), 128);
        ObjectNode outcomeDocument = object(document, "outcome");
        String outcomeKind = validateOutcome(outcomeDocument);
        byte[] receiptBytes = boundedHex(text(document, "receipt"), false);
        byte[] terminalPayload = boundedHex(text(document, "terminal_payload"), true);
        byte[] callGraph = boundedHex(text(document, "call_graph"), false);
        byte[] declaredReceiptDigest = hex32(text(document, "receipt_digest"));
        LocalVerifier.AuthorizedReceiptBatch authorized = new LocalVerifier.AuthorizedReceiptBatch(
            batchId, asset, previousRoot, resultingRoot, sequencerKey);
        LocalVerifier.ReceiptVerification verified = verifyReceipt(receiptBytes, authorized, activity,
            guestAbi, terminalPayload, callGraph);
        LocalVerifier.ProtocolReceipt receipt = verified.receipt();
        LocalVerifier.ProgramReceiptOutcome receiptOutcome = receipt.programOutcome();
        verifyTerminal(terminalPayload, callGraph, program, outcomeDocument, receipt.protocolVersion(),
            receiptOutcome);
        boolean kindMatches = switch (outcomeKind) {
            case "completed" -> receiptOutcome.terminalKind() == 1
                && requiredI32(outcomeDocument.get("code")) == receiptOutcome.resultCode();
            case "legacy_completed" -> receiptOutcome.terminalKind() == 1
                && requiredI32(outcomeDocument.get("code")) == receiptOutcome.resultCode();
            case "refused" -> receiptOutcome.terminalKind() == 2 || receiptOutcome.terminalKind() == 3;
            default -> false;
        };
        if (!MessageDigest.isEqual(verified.receiptDigest(), declaredReceiptDigest)
                || !receipt.globalSequence().equals(globalSequence) || receipt.resultCode() != resultCode
                || receiptOutcome.resultCode() != resultCode || receipt.moduleVersion() != moduleVersion
                || !receiptOutcome.cpuFuel().equals(cpuFuel) || !receiptOutcome.memoryBytes().equals(memoryBytes)
                || !receiptOutcome.storageReadBytes().equals(storageReadBytes)
                || !receiptOutcome.storageWriteBytes().equals(storageWriteBytes)
                || receiptOutcome.outputValues() != outputValues
                || !receiptOutcome.outputBytes().equals(outputBytes)
                || !receiptOutcome.feeUnits().equals(feeUnits) || !kindMatches) invalidVerification();
        return new VerifiedExecution(document, verified, terminalPayload, callGraph);
    }

    private record TerminalUsage(BigInteger cpu, BigInteger memory, BigInteger read, BigInteger write,
                                 long values, BigInteger outputBytes, BigInteger fee) {}

    private record TerminalAttachments(byte[] occupancy, byte[] authorization, byte[] transferRoot) {}

    private static void verifyTerminal(byte[] encoded, byte[] availableGraph, byte[] expectedProgram,
            ObjectNode documentOutcome, int protocolVersion, LocalVerifier.ProgramReceiptOutcome receipt) {
        try {
            TerminalAttachments attachments = unwrapTerminal(encoded);
            byte[] inner = terminalInner(encoded);
            TerminalCursor cursor;
            boolean candidate = starts(inner, "LXP/program-execution/v4\0");
            boolean successful = false;
            if (starts(inner, "LXP/program-execution/v2\0")
                    || starts(inner, "LXP/program-execution/v3\0")) {
                boolean traced = starts(inner, "LXP/program-execution/v3\0");
                cursor = new TerminalCursor(inner, (traced ? "LXP/program-execution/v3\0"
                    : "LXP/program-execution/v2\0").getBytes(StandardCharsets.UTF_8).length);
                int runtime = cursor.u16();
                int abi = cursor.u16();
                long metering = cursor.u32();
                BigInteger countValue = cursor.integer(16);
                if (countValue.bitLength() > 31) throw new IllegalArgumentException();
                int count = countValue.intValue();
                JsonNode values = documentOutcome.get("values");
                if (!"legacy_completed".equals(text(documentOutcome, "kind")) || !values.isArray()
                        || values.size() != count || runtime != receipt.runtimeVersion()
                        || abi != 1 || abi != receipt.abiVersion()
                        || metering != receipt.meteringScheduleVersion()) throw new IllegalArgumentException();
                for (int index = 0; index < count; index++) {
                    if (!(values.get(index) instanceof ObjectNode value)) throw new IllegalArgumentException();
                    int tag = cursor.u8();
                    if (tag == 1) {
                        if (!"i32".equals(text(value, "type"))
                                || cursor.i32() != requiredI32(value.get("value"))) throw new IllegalArgumentException();
                    } else if (tag == 2) {
                        if (!"i64".equals(text(value, "type"))
                                || cursor.i64() != signedI64(value.get("value"))) throw new IllegalArgumentException();
                    } else throw new IllegalArgumentException();
                }
                TerminalUsage usage = new TerminalUsage(cursor.u64(), cursor.u64(), cursor.u64(),
                    cursor.u64(), cursor.u32(), BigInteger.ZERO, cursor.integer(16));
                if (traced) {
                    if (cursor.u8() != 1 || cursor.sized64().length > 34 + 512 * 52) {
                        throw new IllegalArgumentException();
                    }
                }
                cursor.finish();
                if (receipt.terminalKind() != 1 || requiredI32(documentOutcome.get("code")) < 0) {
                    throw new IllegalArgumentException();
                }
                matchTerminalUsage(usage, receipt);
                successful = true;
            } else if (candidate) {
                cursor = new TerminalCursor(inner, "LXP/program-execution/v4\0".getBytes(StandardCharsets.UTF_8).length);
                int runtime = cursor.u16();
                long feeSchedule = cursor.u32();
                long metering = cursor.u32();
                BigInteger countValue = cursor.u64();
                if (countValue.bitLength() > 31) throw new IllegalArgumentException();
                int count = countValue.intValue();
                for (int index = 0; index < count; index++) {
                    int tag = cursor.u8();
                    if (tag == 1) cursor.i32();
                    else if (tag == 2) cursor.i64();
                    else throw new IllegalArgumentException();
                }
                TerminalUsage usage = new TerminalUsage(cursor.u64(), cursor.u64(), cursor.u64(),
                    cursor.u64(), cursor.u32(), cursor.u64(), cursor.integer(16));
                int traceTag = cursor.u8();
                if (traceTag == 1) {
                    if (cursor.sized64().length > 34 + 512 * 52) throw new IllegalArgumentException();
                } else if (traceTag != 0) throw new IllegalArgumentException();
                byte[] program = cursor.take(32);
                int abi = cursor.u16();
                int outcomeTag = cursor.u8();
                String expectedKind;
                if (outcomeTag == 0) {
                    int code = cursor.i32();
                    byte[] response = cursor.sized64();
                    if (code < 0 || response.length > MAX_CALLDATA_BYTES
                            || !"completed".equals(text(documentOutcome, "kind"))
                            || code != requiredI32(documentOutcome.get("code"))
                            || !MessageDigest.isEqual(response,
                                boundedHex(text(documentOutcome, "response"), true))) {
                        throw new IllegalArgumentException();
                    }
                    expectedKind = "completed";
                    successful = true;
                } else if (outcomeTag == 1) {
                    validateAuthenticatedProgramFailure(cursor.sized64());
                    expectedKind = "guest_refused";
                } else if (outcomeTag == 2) {
                    validateCandidateResource(cursor, usage);
                    expectedKind = "resource";
                } else throw new IllegalArgumentException();
                byte[] graph = cursor.sized64();
                cursor.finish();
                if (graph.length > MAX_CALLDATA_BYTES || !MessageDigest.isEqual(graph, availableGraph)
                        || !MessageDigest.isEqual(program, expectedProgram) || abi != receipt.abiVersion()
                        || runtime != receipt.runtimeVersion() || feeSchedule != receipt.feeScheduleVersion()
                        || metering != receipt.meteringScheduleVersion()) throw new IllegalArgumentException();
                matchTerminalUsage(usage, receipt);
                if (outcomeTag == 0) {
                    if (receipt.terminalKind() != 1) throw new IllegalArgumentException();
                } else {
                    requireRefusal(documentOutcome, expectedKind, receipt.resultCode());
                    if (receipt.terminalKind() == 1) throw new IllegalArgumentException();
                }
            } else if (starts(inner, "LXP/programs/failure-detail/v1\0")) {
                cursor = new TerminalCursor(inner, "LXP/programs/failure-detail/v1\0".getBytes(StandardCharsets.UTF_8).length);
                int tag = cursor.u8();
                byte[] payload = cursor.sized32();
                cursor.finish();
                if (tag < 1 || tag > 4 || payload.length == 0) throw new IllegalArgumentException();
                validateFailureDetail(tag, payload);
                requireRefusal(documentOutcome, "guest_refused", receipt.resultCode());
            } else if (starts(inner, "LXP/programs/resource-detail/v1\0")) {
                cursor = new TerminalCursor(inner, "LXP/programs/resource-detail/v1\0".getBytes(StandardCharsets.UTF_8).length);
                validateLegacyResource(cursor);
                cursor.finish();
                requireRefusal(documentOutcome, "resource", receipt.resultCode());
            } else if (starts(inner, "LXP/programs/settlement-failure/v1\0")) {
                cursor = new TerminalCursor(inner, "LXP/programs/settlement-failure/v1\0".getBytes(StandardCharsets.UTF_8).length);
                if (!Set.of(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12).contains(cursor.u8())) {
                    throw new IllegalArgumentException();
                }
                cursor.finish();
                requireRefusal(documentOutcome, "guest_refused", receipt.resultCode());
            } else if (starts(inner, "LXP/programs/callback-failure/v1\0")) {
                cursor = new TerminalCursor(inner, "LXP/programs/callback-failure/v1\0".getBytes(StandardCharsets.UTF_8).length);
                cursor.u8();
                cursor.i32();
                cursor.finish();
                requireRefusal(documentOutcome, "guest_refused", receipt.resultCode());
            } else throw new IllegalArgumentException();
            verifyTerminalAttachments(attachments, candidate, successful, protocolVersion, receipt);
        } catch (RuntimeException error) {
            invalidVerification();
        }
    }

    private static void requireRefusal(ObjectNode outcome, String expected, int code) {
        if (!"refused".equals(text(outcome, "kind"))) throw new IllegalArgumentException();
        ObjectNode failure = object(outcome, "failure");
        if (!expected.equals(text(failure, "kind"))) throw new IllegalArgumentException();
        if ("guest_refused".equals(expected) && requiredI32(failure.get("code")) != code) {
            throw new IllegalArgumentException();
        }
    }

    private static void matchTerminalUsage(TerminalUsage usage,
                                           LocalVerifier.ProgramReceiptOutcome receipt) {
        if (!usage.cpu().equals(receipt.cpuFuel()) || !usage.memory().equals(receipt.memoryBytes())
                || !usage.read().equals(receipt.storageReadBytes())
                || !usage.write().equals(receipt.storageWriteBytes())
                || usage.values() != receipt.outputValues()
                || !usage.outputBytes().equals(receipt.outputBytes())
                || !usage.fee().equals(receipt.feeUnits())) throw new IllegalArgumentException();
    }

    private static void validateAuthenticatedProgramFailure(byte[] encoded) {
        TerminalCursor cursor = new TerminalCursor(encoded, 0);
        byte[] program = cursor.take(32);
        long failureClass = cursor.u32();
        byte[] reason = cursor.sized32();
        cursor.finish();
        if (allZero(program) || !Set.of(1L, 2L, 3L, 4L, 5L, 254L, 255L).contains(failureClass)
                || reason.length > 4_096 || (failureClass == 254 || failureClass == 255) && reason.length != 0) {
            throw new IllegalArgumentException();
        }
    }

    private static void validateFailureDetail(int family, byte[] encoded) {
        if (family == 1) {
            validateAuthenticatedProgramFailure(encoded);
            return;
        }
        TerminalCursor cursor = new TerminalCursor(encoded, 0);
        int tag = cursor.u8();
        if (family == 2) validateCompositionFailure(cursor, tag);
        else if (family == 3) validateEntrypointFailure(cursor, tag);
        else if (family == 4) validateAbiFailure(cursor, tag);
        else throw new IllegalArgumentException();
        cursor.finish();
    }

    private static void validateCompositionFailure(TerminalCursor cursor, int tag) {
        switch (tag) {
            case 1, 9, 10, 11, 20, 21, 22 -> { }
            case 2 -> {
                int expected = cursor.u8();
                int actual = cursor.u8();
                if (expected < 1 || expected > 2 || actual < 1 || actual > 2) {
                    throw new IllegalArgumentException();
                }
            }
            case 3, 4 -> requireNonzero(cursor.take(32));
            case 5, 6, 7 -> { cursor.u32(); cursor.u32(); }
            case 8 -> { requireNonzero(cursor.take(32)); cursor.u32(); cursor.u32(); }
            case 12 -> cursor.i32();
            case 13 -> { cursor.u64(); cursor.u64(); }
            case 14 -> { requireNonzero(cursor.take(32)); cursor.i32(); }
            case 15 -> validateAuthenticatedProgramFailure(cursor.rest());
            case 16 -> validateNestedAbi(cursor);
            case 17 -> validateFault(cursor);
            case 18 -> validateMeterFailure(cursor);
            case 19 -> validateResponseFailure(cursor);
            case 23 -> { cursor.take(76); cursor.take(76); }
            default -> throw new IllegalArgumentException();
        }
    }

    private static void validateEntrypointFailure(TerminalCursor cursor, int tag) {
        switch (tag) {
            case 1 -> { cursor.u64(); cursor.u64(); }
            case 2, 3, 4 -> { }
            case 5, 6 -> cursor.i32();
            case 7 -> validateFault(cursor);
            case 8 -> validateMeterFailure(cursor);
            default -> throw new IllegalArgumentException();
        }
    }

    private static void validateAbiFailure(TerminalCursor cursor, int tag) {
        if (tag >= 1 && tag <= 10 || tag >= 13 && tag <= 15) return;
        if (tag == 11) {
            int storage = cursor.u8();
            if (storage < 1 || storage > 11) throw new IllegalArgumentException();
        } else if (tag == 12) validateMeterFailure(cursor);
        else throw new IllegalArgumentException();
    }

    private static void validateNestedAbi(TerminalCursor cursor) {
        validateAbiFailure(cursor, cursor.u8());
    }

    private static void validateMeterFailure(TerminalCursor cursor) {
        int tag = cursor.u8();
        if (tag == 1) {
            int resource = cursor.u8();
            BigInteger limit = cursor.u64();
            BigInteger attempted = cursor.u64();
            if (resource < 1 || resource > 7 || attempted.compareTo(limit) <= 0) {
                throw new IllegalArgumentException();
            }
        } else if (tag == 2) {
            int resource = cursor.u8();
            if (resource < 1 || resource > 7) throw new IllegalArgumentException();
        } else if (tag != 3) throw new IllegalArgumentException();
    }

    private static void validateFault(TerminalCursor cursor) {
        int tag = cursor.u8();
        if (tag == 1 || tag == 2 || tag == 16) {
            byte[] name = cursor.sized32();
            if (!Arrays.equals(name, new String(name, StandardCharsets.UTF_8)
                    .getBytes(StandardCharsets.UTF_8))) throw new IllegalArgumentException();
        } else if (tag >= 3 && tag <= 13 || tag == 15) {
            return;
        } else if (tag == 14) validateMeterFailure(cursor);
        else throw new IllegalArgumentException();
    }

    private static void validateResponseFailure(TerminalCursor cursor) {
        int tag = cursor.u8();
        if (tag == 1 || tag == 2) { cursor.u64(); cursor.u64(); }
        else if (tag == 3 || tag == 4) return;
        else if (tag == 5) { cursor.i32(); cursor.i32(); }
        else if (tag == 6) validateMeterFailure(cursor);
        else throw new IllegalArgumentException();
    }

    private static void requireNonzero(byte[] value) {
        if (allZero(value)) throw new IllegalArgumentException();
    }

    private static void validateCandidateResource(TerminalCursor cursor, TerminalUsage usage) {
        int tag = cursor.u8();
        int resource = cursor.u8();
        if (resource < 0 || resource > 6) throw new IllegalArgumentException();
        if (tag == 0) {
            BigInteger limit = cursor.u64();
            BigInteger attempted = cursor.u64();
            if (attempted.compareTo(limit) <= 0 || candidateUsage(usage, resource).compareTo(limit) > 0) {
                throw new IllegalArgumentException();
            }
        } else if (tag != 1) throw new IllegalArgumentException();
    }

    private static BigInteger candidateUsage(TerminalUsage usage, int resource) {
        return switch (resource) {
            case 0 -> usage.cpu();
            case 1 -> usage.memory();
            case 2 -> usage.read();
            case 3 -> usage.write();
            case 4 -> BigInteger.valueOf(usage.values());
            case 5 -> usage.outputBytes();
            case 6 -> BigInteger.ZERO;
            default -> throw new IllegalArgumentException();
        };
    }

    private static void validateLegacyResource(TerminalCursor cursor) {
        int tag = cursor.u8();
        int resource = cursor.u8();
        if (resource < 1 || resource > 7) throw new IllegalArgumentException();
        if (tag == 1) {
            BigInteger limit = cursor.u64();
            if (cursor.u64().compareTo(limit) <= 0) throw new IllegalArgumentException();
        } else if (tag != 2) throw new IllegalArgumentException();
    }

    private static void verifyTerminalAttachments(TerminalAttachments attachments, boolean candidate,
            boolean successful, int protocolVersion, LocalVerifier.ProgramReceiptOutcome receipt) {
        boolean occupancyRequired = protocolVersion == 2 && successful;
        if (occupancyRequired != (attachments.occupancy() != null)) throw new IllegalArgumentException();
        if (attachments.occupancy() != null) {
            byte[] occupancy = attachments.occupancy();
            if (occupancy.length == 0) {
                if (!allZero(receipt.occupancyEvidenceDigest()) || !allZero(receipt.occupancyTransferRoot())
                        || receipt.occupancyByteBatches().signum() != 0
                        || receipt.occupancyFeeUnits().signum() != 0) throw new IllegalArgumentException();
            } else if (!MessageDigest.isEqual(sha256(occupancy), receipt.occupancyEvidenceDigest())) {
                throw new IllegalArgumentException();
            }
        }
        boolean authorityRequired = candidate && !allZero(receipt.transferRoot());
        if (authorityRequired != (attachments.authorization() != null)) throw new IllegalArgumentException();
        if (attachments.authorization() != null
                && !MessageDigest.isEqual(attachments.transferRoot(), receipt.transferRoot())) {
            throw new IllegalArgumentException();
        }
    }

    private static TerminalAttachments unwrapTerminal(byte[] encoded) {
        byte[] current = encoded;
        byte[] authorization = null;
        byte[] transferRoot = null;
        byte[] occupancy = null;
        byte[] authorityDomain = "LXP/program-execution-with-transfer-authority/v2\0"
            .getBytes(StandardCharsets.UTF_8);
        byte[] occupancyDomain = "LXP/program-execution-with-occupancy/v1\0"
            .getBytes(StandardCharsets.UTF_8);
        if (starts(current, authorityDomain)) {
            TerminalCursor cursor = new TerminalCursor(current, authorityDomain.length);
            current = cursor.sized32();
            authorization = cursor.sized32();
            transferRoot = cursor.take(32);
            cursor.finish();
        }
        if (starts(current, occupancyDomain)) {
            TerminalCursor cursor = new TerminalCursor(current, occupancyDomain.length);
            current = cursor.sized32();
            occupancy = cursor.sized32();
            cursor.finish();
        }
        if (starts(current, authorityDomain) || starts(current, occupancyDomain)) {
            throw new IllegalArgumentException();
        }
        return new TerminalAttachments(occupancy, authorization, transferRoot);
    }

    private static byte[] terminalInner(byte[] encoded) {
        byte[] current = encoded;
        byte[] authorityDomain = "LXP/program-execution-with-transfer-authority/v2\0"
            .getBytes(StandardCharsets.UTF_8);
        byte[] occupancyDomain = "LXP/program-execution-with-occupancy/v1\0"
            .getBytes(StandardCharsets.UTF_8);
        if (starts(current, authorityDomain)) {
            TerminalCursor cursor = new TerminalCursor(current, authorityDomain.length);
            current = cursor.sized32();
        }
        if (starts(current, occupancyDomain)) {
            TerminalCursor cursor = new TerminalCursor(current, occupancyDomain.length);
            current = cursor.sized32();
        }
        return current;
    }

    private static boolean starts(byte[] value, String prefix) {
        return starts(value, prefix.getBytes(StandardCharsets.UTF_8));
    }

    private static boolean starts(byte[] value, byte[] prefix) {
        return value.length >= prefix.length
            && MessageDigest.isEqual(Arrays.copyOf(value, prefix.length), prefix);
    }

    private static String validateOutcome(ObjectNode outcome) {
        String kind = text(outcome, "kind");
        switch (kind) {
            case "completed" -> {
                requireFields(outcome, "kind", "code", "response");
                requiredI32(outcome.get("code"));
                boundedHex(text(outcome, "response"), true);
            }
            case "legacy_completed" -> {
                requireFields(outcome, "kind", "code", "values");
                requiredI32(outcome.get("code"));
                JsonNode values = outcome.get("values");
                if (values == null || !values.isArray() || values.size() > MAX_LEGACY_VALUES) throw decodeFailure();
                values.forEach(ProgramsClient::validateLegacyValue);
            }
            case "refused" -> {
                requireFields(outcome, "kind", "failure");
                validateFailure(object(outcome, "failure"));
            }
            default -> throw decodeFailure();
        }
        return kind;
    }

    private static void validateLegacyValue(JsonNode value) {
        if (!(value instanceof ObjectNode object)) throw decodeFailure();
        requireFields(object, "type", "value");
        switch (text(object, "type")) {
            case "i32" -> requiredI32(object.get("value"));
            case "i64" -> signedI64(object.get("value"));
            default -> throw decodeFailure();
        }
    }

    private static void validateFailure(ObjectNode failure) {
        String kind = text(failure, "kind");
        switch (kind) {
            case "unknown_program", "reentrancy", "authority", "resource", "response", "fault" ->
                requireFields(failure, "kind");
            case "depth_exceeded", "fanout_exceeded" -> {
                requireFields(failure, "kind", "limit", "attempted");
                requiredU32(failure.get("limit"));
                requiredU32(failure.get("attempted"));
            }
            case "guest_refused" -> {
                requireFields(failure, "kind", "code");
                requiredI32(failure.get("code"));
            }
            default -> throw decodeFailure();
        }
    }

    private static void verifySimulationEvidence(ObjectNode execution, ObjectNode evidence,
                                                 ActivityBinding binding, byte[] pinnedKey,
                                                 BigInteger now, BigInteger maximumAge) {
        requireFields(evidence, "boundary_id", "activity_id", "previous_state_root",
            "hypothetical_state_root", "observed_sequence", "observed_at", "committed", "public_key",
            "signature");
        if (!evidence.path("committed").isBoolean() || evidence.path("committed").booleanValue()) {
            invalidVerification();
        }
        byte[] boundary = hex32(text(evidence, "boundary_id"));
        byte[] activity = hex32(text(evidence, "activity_id"));
        byte[] previous = hex32(text(evidence, "previous_state_root"));
        byte[] hypothetical = hex32(text(evidence, "hypothetical_state_root"));
        byte[] publicKey = hex32(text(evidence, "public_key"));
        byte[] signature = boundedHex(text(evidence, "signature"), false);
        BigInteger sequence = unsigned(evidence.get("observed_sequence"), 64);
        BigInteger observedAt = unsigned(evidence.get("observed_at"), 64);
        ObjectNode authority = object(execution, "authority");
        if (signature.length != 64 || sequence.equals(MAX_U64)
                || !MessageDigest.isEqual(activity, hex32(text(execution, "activity_id")))
                || !MessageDigest.isEqual(activity, binding.activityId())
                || !MessageDigest.isEqual(hypothetical, hex32(text(execution, "state_root")))
                || !MessageDigest.isEqual(previous, hex32(text(authority, "previous_state_root")))
                || !MessageDigest.isEqual(hypothetical, hex32(text(authority, "resulting_state_root")))
                || !MessageDigest.isEqual(publicKey, hex32(text(authority, "sequencer_public_key")))
                || !MessageDigest.isEqual(publicKey, pinnedKey)
                || !unsigned(execution.get("global_sequence"), 64).equals(sequence.add(BigInteger.ONE))
                || observedAt.compareTo(binding.notBefore()) < 0
                || observedAt.compareTo(binding.notAfter()) > 0
                || observedAt.compareTo(now) > 0 || now.subtract(observedAt).compareTo(maximumAge) > 0
                || !MessageDigest.isEqual(boundary, sha256(SIMULATION_BOUNDARY_DOMAIN, publicKey))) {
            invalidVerification();
        }
        ByteBuffer integers = ByteBuffer.allocate(16);
        integers.putLong(sequence.longValue()).putLong(observedAt.longValue());
        byte[] digest = sha256(SIMULATION_EVIDENCE_DOMAIN, boundary, activity, previous, hypothetical,
            integers.array(), new byte[] {0});
        if (!verifyEd25519(publicKey, signature, digest)) invalidVerification();
    }

    private static ObjectNode encode(Call call) {
        ObjectNode value = object().put("program_id", hex(call.programId()))
            .put("calldata", hex(call.calldata())).put("signed_activity", hex(call.signedActivity()));
        value.set("budget", object().put("fuel", call.budget().fuel().toString())
            .put("fee_limit", call.budget().feeLimit().toString()));
        ArrayNode capabilities = value.putArray("capabilities");
        call.capabilities().forEach(item -> capabilities.add(item.wire()));
        return value;
    }

    private static ActivityBinding decodeSignedCall(Call call) {
        try {
            byte[] signed = call.signedActivity();
            BinaryCursor cursor = new BinaryCursor(signed);
            if (cursor.u16() != 1 || cursor.u16() != 0x1001 || cursor.u8() != 12) {
                throw new IllegalArgumentException();
            }
            cursor.tag(1);
            int protocolVersion = cursor.u16();
            if (protocolVersion != 1 && protocolVersion != 2) throw new IllegalArgumentException();
            cursor.tag(2);
            cursor.u32();
            cursor.tag(3);
            if (cursor.u32() != ((PROGRAMS_RECEIPT_MODULE_ID << 16) | CALL_OPERATION)) {
                throw new IllegalArgumentException();
            }
            cursor.tag(4);
            cursor.bounded(255, true);
            cursor.tag(5);
            cursor.bounded(524_288, true);
            cursor.tag(6);
            cursor.u64();
            cursor.tag(7);
            BigInteger notBefore = cursor.u64();
            BigInteger notAfter = cursor.u64();
            if (notAfter.compareTo(notBefore) < 0) throw new IllegalArgumentException();
            cursor.tag(8);
            byte[] idempotency = cursor.bounded(32, false);
            if (idempotency.length != 32) throw new IllegalArgumentException();
            cursor.tag(9);
            cursor.integer(16);
            cursor.tag(10);
            byte[] payloadHash = cursor.bounded(32, false);
            if (payloadHash.length != 32) throw new IllegalArgumentException();
            cursor.tag(11);
            byte[] payload = cursor.bounded(524_288, true);
            cursor.tag(12);
            cursor.bounded(128, true);
            cursor.finish();
            byte[] expectedPayload = canonicalCallPayload(call);
            if (!MessageDigest.isEqual(payload, expectedPayload)
                    || !MessageDigest.isEqual(payloadHash, sha256(PAYLOAD_HASH_DOMAIN, payload))) {
                throw new IllegalArgumentException();
            }
            return new ActivityBinding(sha256(ACTIVITY_ID_DOMAIN, signed), idempotency,
                notBefore, notAfter);
        } catch (IllegalArgumentException error) {
            invalid();
            throw new AssertionError();
        }
    }

    private static byte[] canonicalCallPayload(Call call) {
        byte[] calldata = call.calldata();
        byte[] program = call.programId();
        byte[] fuel = fixedUnsigned(call.budget().fuel(), 8);
        byte[] fee = fixedUnsigned(call.budget().feeLimit(), 16);
        int length = PROGRAM_CALL_DOMAIN.length + 32 + 8 + 16 + 2
            + call.capabilities().size() + 4 + calldata.length;
        ByteBuffer encoded = ByteBuffer.allocate(length);
        encoded.put(PROGRAM_CALL_DOMAIN).put(program).put(fuel).put(fee)
            .putShort((short) call.capabilities().size());
        call.capabilities().forEach(capability -> encoded.put((byte) (capability.ordinal() + 1)));
        encoded.putInt(calldata.length).put(calldata);
        return encoded.array();
    }

    private static byte[] fixedUnsigned(BigInteger value, int length) {
        byte[] raw = value.toByteArray();
        byte[] fixed = new byte[length];
        int copy = Math.min(raw.length, length);
        System.arraycopy(raw, raw.length - copy, fixed, length - copy, copy);
        return fixed;
    }

    private static BigInteger durationMillis(Duration value) {
        try {
            long millis = value.toMillis();
            if (millis <= 0) invalid();
            return BigInteger.valueOf(millis);
        } catch (ArithmeticException error) {
            invalid();
            throw new AssertionError();
        }
    }

    private static void requireSequencerSigned(String value) {
        if (!SEQUENCER_SIGNED.equals(value)) invalid();
    }

    private static void requireFields(ObjectNode value, String... fields) {
        if (value == null || value.size() != fields.length) throw decodeFailure();
        for (String field : fields) if (!value.has(field)) throw decodeFailure();
    }

    private static ObjectNode object(JsonNode parent, String field) {
        JsonNode value = parent == null ? null : parent.get(field);
        if (!(value instanceof ObjectNode object)) throw decodeFailure();
        return object;
    }

    private static String text(JsonNode parent, String field) {
        JsonNode value = parent == null ? null : parent.get(field);
        if (value == null || !value.isTextual() || value.textValue().isEmpty()) throw decodeFailure();
        return value.textValue();
    }

    private static int requiredU16(JsonNode value) {
        BigInteger integer = jsonInteger(value);
        if (integer.signum() < 0 || integer.bitLength() > 16) throw decodeFailure();
        return integer.intValue();
    }

    private static long requiredU32(JsonNode value) {
        BigInteger integer = jsonInteger(value);
        if (integer.signum() < 0 || integer.compareTo(MAX_U32) > 0) throw decodeFailure();
        return integer.longValue();
    }

    private static int requiredI32(JsonNode value) {
        BigInteger integer = jsonInteger(value);
        if (integer.compareTo(BigInteger.valueOf(Integer.MIN_VALUE)) < 0
                || integer.compareTo(BigInteger.valueOf(Integer.MAX_VALUE)) > 0) throw decodeFailure();
        return integer.intValue();
    }

    private static BigInteger jsonInteger(JsonNode value) {
        if (value == null || !value.isIntegralNumber()) throw decodeFailure();
        return value.bigIntegerValue();
    }

    private static BigInteger unsigned(JsonNode value, int bits) {
        if (value == null || !value.isTextual()) throw decodeFailure();
        String text = value.textValue();
        if (text.isEmpty() || text.length() > 1 && text.charAt(0) == '0'
                || !text.chars().allMatch(character -> character >= '0' && character <= '9')) {
            throw decodeFailure();
        }
        try {
            BigInteger parsed = new BigInteger(text);
            if (parsed.signum() < 0 || parsed.bitLength() > bits) throw decodeFailure();
            return parsed;
        } catch (NumberFormatException error) {
            throw decodeFailure();
        }
    }

    private static long signedI64(JsonNode value) {
        if (value == null || !value.isTextual()) throw decodeFailure();
        String text = value.textValue();
        if (text.isEmpty() || "-0".equals(text) || text.charAt(0) == '0' && text.length() > 1
                || text.charAt(0) == '-' && (text.length() == 1
                    || text.charAt(1) == '0' && text.length() > 2)) throw decodeFailure();
        try {
            return Long.parseLong(text);
        } catch (NumberFormatException error) {
            throw decodeFailure();
        }
    }

    private static byte[] boundedHex(String value, boolean emptyAllowed) {
        if (value == null || (value.isEmpty() && !emptyAllowed) || value.length() > MAX_CALLDATA_BYTES * 2
                || (value.length() & 1) != 0 || !canonicalLowerHex(value, value.length() / 2)) {
            throw decodeFailure();
        }
        try {
            return HexFormat.of().parseHex(value);
        } catch (IllegalArgumentException error) {
            throw decodeFailure();
        }
    }

    private static byte[] hex32(String value) {
        if (!canonicalLowerHex(value, 32)) throw decodeFailure();
        return HexFormat.of().parseHex(value);
    }

    private static boolean canonicalLowerHex(String value, int bytes) {
        if (value == null || value.length() != bytes * 2) return false;
        for (int index = 0; index < value.length(); index++) {
            char current = value.charAt(index);
            if (!(current >= '0' && current <= '9' || current >= 'a' && current <= 'f')) return false;
        }
        return true;
    }

    private static boolean allZero(byte[] value) {
        int aggregate = 0;
        for (byte current : value) aggregate |= current;
        return aggregate == 0;
    }

    private static byte[] exactArgument(byte[] value, int length) {
        if (value == null || value.length != length) invalid();
        return value.clone();
    }

    private static byte[] exactVerification(byte[] value, int length) {
        if (value == null || value.length != length) invalidVerification();
        return value.clone();
    }

    private static ObjectNode object() { return JsonNodeFactory.instance.objectNode(); }

    private static ObjectNode copy(ObjectNode value) {
        if (value == null) throw decodeFailure();
        return value.deepCopy();
    }

    private static String hex(byte[] value) { return HexFormat.of().formatHex(value); }

    private static byte[] sha256(byte[]... values) {
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            for (byte[] value : values) digest.update(value);
            return digest.digest();
        } catch (java.security.GeneralSecurityException impossible) {
            throw new AssertionError(impossible);
        }
    }

    private static boolean verifyEd25519(byte[] publicKey, byte[] signature, byte[] digest) {
        try {
            byte[] encoded = new byte[ED25519_X509_PREFIX.length + publicKey.length];
            System.arraycopy(ED25519_X509_PREFIX, 0, encoded, 0, ED25519_X509_PREFIX.length);
            System.arraycopy(publicKey, 0, encoded, ED25519_X509_PREFIX.length, publicKey.length);
            var key = KeyFactory.getInstance("Ed25519").generatePublic(new X509EncodedKeySpec(encoded));
            var verifier = Signature.getInstance("Ed25519");
            verifier.initVerify(key);
            verifier.update(digest);
            return verifier.verify(signature);
        } catch (java.security.GeneralSecurityException | RuntimeException error) {
            return false;
        }
    }

    private static PlatformSdkException decodeFailure() {
        return new PlatformSdkException(PlatformSdkException.Code.DECODE_FAILURE,
            PlatformSdkException.Retry.NEVER, null, null, null);
    }

    private static void invalid() { throw PlatformSdkException.invalidArgument(); }

    private static void invalidVerification() { throw PlatformSdkException.verificationFailure(); }

    private static PlatformSdkException unknownOutcome() {
        return new PlatformSdkException(PlatformSdkException.Code.UNKNOWN_OUTCOME,
            PlatformSdkException.Retry.UNKNOWN_OUTCOME, null, null, null);
    }

    private static final class BinaryCursor {
        private final byte[] bytes;
        private int offset;
        private BinaryCursor(byte[] bytes) { this.bytes = bytes; }
        private int u8() { return take(1)[0] & 0xff; }
        private int u16() { return integer(2).intValue(); }
        private long u32() { return integer(4).longValue(); }
        private BigInteger u64() { return integer(8); }
        private BigInteger integer(int length) { return new BigInteger(1, take(length)); }
        private void tag(int expected) { if (u8() != expected) throw new IllegalArgumentException(); }
        private byte[] bounded(int maximum, boolean emptyAllowed) {
            long declared = u32();
            if (declared > maximum || declared > Integer.MAX_VALUE || declared == 0 && !emptyAllowed) {
                throw new IllegalArgumentException();
            }
            return take((int) declared);
        }
        private byte[] take(int length) {
            if (length < 0 || offset > bytes.length - length) throw new IllegalArgumentException();
            byte[] value = Arrays.copyOfRange(bytes, offset, offset + length);
            offset += length;
            return value;
        }
        private void finish() { if (offset != bytes.length) throw new IllegalArgumentException(); }
    }

    private static final class TerminalCursor {
        private final byte[] bytes;
        private int offset;
        private TerminalCursor(byte[] bytes, int offset) {
            this.bytes = bytes;
            this.offset = offset;
            if (offset < 0 || offset > bytes.length) throw new IllegalArgumentException();
        }
        private int u8() { return take(1)[0] & 0xff; }
        private int u16() { return integer(2).intValue(); }
        private long u32() { return integer(4).longValue(); }
        private BigInteger u64() { return integer(8); }
        private int i32() { return ByteBuffer.wrap(take(4)).getInt(); }
        private long i64() { return ByteBuffer.wrap(take(8)).getLong(); }
        private BigInteger integer(int length) { return new BigInteger(1, take(length)); }
        private byte[] sized32() {
            long length = u32();
            if (length > Integer.MAX_VALUE) throw new IllegalArgumentException();
            return take((int) length);
        }
        private byte[] sized64() {
            BigInteger length = u64();
            if (length.bitLength() > 31) throw new IllegalArgumentException();
            return take(length.intValue());
        }
        private byte[] take(int length) {
            if (length < 0 || offset > bytes.length - length) throw new IllegalArgumentException();
            byte[] value = Arrays.copyOfRange(bytes, offset, offset + length);
            offset += length;
            return value;
        }
        private byte[] rest() { return take(bytes.length - offset); }
        private void finish() { if (offset != bytes.length) throw new IllegalArgumentException(); }
    }
}
