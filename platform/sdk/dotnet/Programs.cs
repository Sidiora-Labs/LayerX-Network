#nullable enable

using System.Buffers.Binary;
using System.Globalization;
using System.Numerics;
using System.Security.Cryptography;
using System.Text;

namespace LayerX.Sdk;

public sealed record ProgramBudget(ulong Fuel, ProtocolAmount FeeLimit);

public enum ProgramCapability { StorageRead, StorageWrite, Transfer, EmitEvent, Compose }

public sealed record ProgramCall
{
    public NativeProgramCall? NativeCall { get; }
    public byte[] ProgramId { get; } public byte[] Calldata { get; }
    public ProgramBudget Budget { get; } public IReadOnlyList<ProgramCapability> Capabilities { get; } public byte[] SignedActivity { get; }

    public ProgramCall(byte[] programId, byte[] calldata, ProgramBudget budget,
        IEnumerable<ProgramCapability> capabilities, byte[] signedActivity)
    {
        var bounded = capabilities?.ToArray();
        if (programId?.Length != 32 || programId.All(value => value == 0) || calldata is null || calldata.Length > 1_048_576 || budget is null || budget.Fuel == 0 || bounded is null || bounded.Length > 5 ||
            bounded.Zip(bounded.Skip(1)).Any(pair => pair.First >= pair.Second) ||
            signedActivity is null || signedActivity.Length == 0 || signedActivity.Length > 1_048_576) throw Invalid();
        ProgramId = programId.ToArray(); Calldata = calldata.ToArray(); Budget = budget; Capabilities = Array.AsReadOnly(bounded);
        SignedActivity = signedActivity.ToArray();
    }
    public ProgramCall(NativeProgramCall nativeCall, ProtocolAmount feeLimit, byte[] signedActivity)
        : this(nativeCall.ProgramId, nativeCall.Calldata, new ProgramBudget(nativeCall.Resources[0], feeLimit), Array.Empty<ProgramCapability>(), signedActivity)
    {
        _ = nativeCall.Encode(); NativeCall = nativeCall;
    }
    private static PlatformSdkException Invalid() => new(SdkErrorCode.InvalidArgument, RetryClass.Never);
}

public enum ProgramLifecycle { Active, Deprecated, Tombstoned }
public abstract record ProgramSource
{
    public sealed record Unpublished : ProgramSource;
    public sealed record Verified(byte[] SourceDigest, byte[] EnvironmentDigest, string Pipeline) : ProgramSource;
    public sealed record Mismatch(byte[] ExpectedCodeHash, byte[] ReproducedArtifactDigest) : ProgramSource;
}
public sealed record ProgramDiscovery(byte[] ProgramId, ProgramLifecycle Lifecycle, uint Version, byte[] CodeHash,
    ushort AbiVersion, byte[] ReceiptDigest, byte[] StateRoot, ulong ObservedSequence, ulong ObservedAt,
    ulong ValidThrough, string Verification);
public sealed record ProgramInterface(byte[] ProgramId, uint Version, byte[] CodeHash, ushort AbiVersion,
    byte[] Interface, byte[] InterfaceDigest, byte[] ReceiptDigest, byte[] StateRoot, ulong ObservedSequence,
    ulong ObservedAt, ulong ValidThrough, ProgramSource Source, string Verification);
public sealed class VerifiedProgramExecution
{
    public JsonValue Value { get; }
    public ReceiptVerification Receipt { get; }
    public byte[] TerminalPayload { get; }
    public byte[] CallGraph { get; }
    public ushort GuestAbiVersion => checked((ushort)((JsonValue.IntegerValue)((JsonValue.ObjectValue)Value).Value["guest_abi_version"]).Value);
    public byte[] TerminalPayloadRoot => Receipt.Receipt.ProgramOutcome!.TerminalPayloadRoot.ToArray();
    public byte[] CallGraphRoot => Receipt.Receipt.ProgramOutcome!.CallGraphRoot.ToArray();

    internal VerifiedProgramExecution(JsonValue value, ReceiptVerification receipt, byte[] terminalPayload, byte[] callGraph)
    {
        Value = value; Receipt = receipt;
        TerminalPayload = terminalPayload.ToArray(); CallGraph = callGraph.ToArray();
    }
}

public sealed record ProgramSimulation(JsonValue Value, VerifiedProgramExecution Execution);
public sealed class ProgramSubmission
{
    public JsonValue Value { get; } public string State { get; } public bool IsUnknown => State == "unknown";
    public byte[] ActivityId { get; } public string IdempotencyKey { get; }
    public byte[]? RetainedSignedActivity { get; } public VerifiedProgramExecution? Execution { get; }
    internal ProgramSubmission(JsonValue value, string state, byte[] activityId, string idempotencyKey,
        byte[]? retainedSignedActivity, VerifiedProgramExecution? execution)
    {
        Value = value; State = state; ActivityId = activityId.ToArray(); IdempotencyKey = idempotencyKey;
        RetainedSignedActivity = retainedSignedActivity?.ToArray(); Execution = execution;
    }
}

public sealed class ProgramsClient
{
    public const ushort ReceiptModuleId = 9; public const byte CallOperation = 3;
    private const int MaximumProgramBytes = 1_048_576;
    private const int MaximumInterfaceBytes = 952;
    private static readonly int MaximumCallGraphBytes = Encoding.UTF8.GetByteCount("LayerX/programs/call-graph/v1\0") + 32 + 16 + 8 + 64 * 68;
    private readonly PlatformClient _client;
    private readonly byte[] _sequencerPublicKey;
    private readonly ushort _protocolVersion;
    private readonly TimeProvider _timeProvider;
    private readonly ulong _maximumSimulationAgeMilliseconds;
    private sealed record ActivityBinding(byte[] ActivityId, byte[] IdempotencyKey, ulong NotBefore, ulong NotAfter);
    private sealed record TerminalUsage(ulong Cpu, ulong Memory, ulong Read, ulong Write,
        uint Values, ulong OutputBytes, BigInteger Fee);
    private sealed record TerminalAttachments(byte[] Inner, byte[]? Occupancy, byte[]? Authorization, byte[]? TransferRoot);
    private sealed record CapabilityKey(int Order, IReadOnlyList<byte[]> Fields);
    private sealed record ProgramAuthorityBinding(byte[] Owner, byte[] Frame, byte[] Source, byte[] Asset,
        byte[] Destination, BigInteger Amount);
    private sealed record ProgramFundingBinding(byte[] Owner, byte[] Destination, byte[] Asset);
    private sealed record OccupancyChargeBinding(byte[] Payer, BigInteger AmountDue, bool Paid, BigInteger ArrearsAfter);
    private sealed record OccupancySettlementBinding(BigInteger ByteBatches, BigInteger FeeUnits,
        IReadOnlyList<OccupancyChargeBinding> Charges);
    private sealed record StorageNamespaceBinding(byte[] Canonical, byte[] Wire, byte[] Program, byte[]? Principal);
    private sealed class OccupancyPayer
    {
        internal required byte[] Payer { get; init; }
        internal BigInteger Due { get; set; }
        internal BigInteger Paid { get; set; }
        internal BigInteger Arrears { get; set; }
    }

    public ProgramsClient(PlatformClient client, byte[] sequencerPublicKey,
        TimeProvider? timeProvider = null, TimeSpan? maximumSimulationAge = null, ushort protocolVersion = 2)
    {
        if (protocolVersion != 2 && protocolVersion != 3) throw Invalid();
        _protocolVersion = protocolVersion;
        _client = client ?? throw Invalid();
        if (sequencerPublicKey?.Length != 32 || sequencerPublicKey.All(value => value == 0)) throw Invalid();
        _sequencerPublicKey = sequencerPublicKey.ToArray(); _timeProvider = timeProvider ?? TimeProvider.System;
        var age = maximumSimulationAge ?? TimeSpan.FromMinutes(5);
        if (age <= TimeSpan.Zero || age.TotalMilliseconds > ulong.MaxValue) throw Invalid();
        _maximumSimulationAgeMilliseconds = checked((ulong)age.TotalMilliseconds);
    }

    public async Task<ProgramDiscovery> DiscoverAsync(byte[] programId, string verificationLevel, CancellationToken cancellationToken = default)
    {
        var id = Identifier(programId); var value = await _client.ProgramAsync("program.discover", JsonValue.Object(new Dictionary<string, JsonValue>
            { ["program_id"] = JsonValue.String(id), ["requested_verification_level"] = JsonValue.String(Level(verificationLevel)) }),
            pathParameters: new Dictionary<string, string> { ["program_id"] = id }, cancellationToken: cancellationToken).ConfigureAwait(false);
        return (ProgramDiscovery)VerifyDiscovery(value, id, false, NowMilliseconds());
    }
    public async Task<ProgramInterface> InterfaceAsync(byte[] programId, string verificationLevel, CancellationToken cancellationToken = default)
    {
        var id = Identifier(programId);
        var value = await _client.ProgramAsync("program.interface", JsonValue.Object(new Dictionary<string, JsonValue>
            { ["program_id"] = JsonValue.String(id), ["requested_verification_level"] = JsonValue.String(Level(verificationLevel)) }),
            pathParameters: new Dictionary<string, string> { ["program_id"] = id }, cancellationToken: cancellationToken).ConfigureAwait(false);
        return (ProgramInterface)VerifyDiscovery(value, id, true, NowMilliseconds());
    }
    public async Task<ProgramSimulation> SimulateAsync(ProgramCall call, CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(call); if (call.NativeCall is not null && _protocolVersion != 3) throw Invalid();
        var binding = DecodeSignedCall(call);
        var value = await _client.ProgramAsync("program.simulate", Encode(call), cancellationToken: cancellationToken).ConfigureAwait(false);
        var execution = await VerifySimulationAsync(value, call.ProgramId, binding, cancellationToken).ConfigureAwait(false);
        return new(value, execution);
    }
    public async Task<ProgramSubmission> SubmitAsync(ProgramCall call, IdempotencyKey idempotencyKey, CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(call); if (!Hex32(idempotencyKey.Value)) throw Invalid();
        if (call.NativeCall is not null && _protocolVersion != 3) throw Invalid();
        var binding = DecodeSignedCall(call); var key = Convert.FromHexString(idempotencyKey.Value);
        if (!CryptographicOperations.FixedTimeEquals(binding.IdempotencyKey, key)) throw Invalid();
        JsonValue value;
        try
        {
            value = await _client.ProgramAsync("program.call", Encode(call), idempotencyKey,
                cancellationToken: cancellationToken).ConfigureAwait(false);
        }
        catch (PlatformSdkException error) when (error.Code == SdkErrorCode.UnknownOutcome)
        {
            return UnknownSubmission(binding.ActivityId, idempotencyKey.Value, call.SignedActivity);
        }
        try
        {
            return await VerifySubmissionAsync(value, call.ProgramId, binding.ActivityId, idempotencyKey.Value,
                call.SignedActivity, cancellationToken).ConfigureAwait(false);
        }
        catch (PlatformSdkException error) when (error.Code is SdkErrorCode.DecodeFailure or SdkErrorCode.VerificationFailure)
        {
            return UnknownSubmission(binding.ActivityId, idempotencyKey.Value, call.SignedActivity);
        }
    }
    public async Task<ProgramSubmission> ReceiptAsync(IdempotencyKey idempotencyKey, byte[] expectedActivityId, string verificationLevel, CancellationToken cancellationToken = default)
    {
        if (!Hex32(idempotencyKey.Value)) throw Invalid(); var activity = Identifier(expectedActivityId);
        var value = await _client.ProgramAsync("program.receipt", JsonValue.Object(new Dictionary<string, JsonValue>
            { ["idempotency_key"] = JsonValue.String(idempotencyKey.Value), ["expected_activity_id"] = JsonValue.String(activity),
              ["requested_verification_level"] = JsonValue.String(Level(verificationLevel)) }),
            pathParameters: new Dictionary<string, string> { ["idempotency_key"] = idempotencyKey.Value }, cancellationToken: cancellationToken).ConfigureAwait(false);
        return await VerifySubmissionAsync(value, null, expectedActivityId, idempotencyKey.Value, null, cancellationToken).ConfigureAwait(false);
    }
    public async Task<ProgramSubmission> ActivityAsync(byte[] activityId, string verificationLevel, CancellationToken cancellationToken = default)
    {
        var id = Identifier(activityId);
        var value = await _client.ProgramAsync("program.activity", JsonValue.Object(new Dictionary<string, JsonValue>
            { ["activity_id"] = JsonValue.String(id), ["requested_verification_level"] = JsonValue.String(Level(verificationLevel)) }),
            pathParameters: new Dictionary<string, string> { ["activity_id"] = id }, cancellationToken: cancellationToken).ConfigureAwait(false);
        return await VerifySubmissionAsync(value, null, activityId, null, null, cancellationToken).ConfigureAwait(false);
    }

    public static async ValueTask<ReceiptVerification> VerifyReceiptAsync(byte[] canonicalReceipt, AuthorizedReceiptBatch authorized,
        byte[] expectedActivityId, ushort expectedGuestAbiVersion, byte[] terminalPayload, byte[] callGraph,
        CancellationToken cancellationToken = default, ushort protocolVersion = 2)
    {
        if (expectedActivityId?.Length != 32 || expectedGuestAbiVersion is not (1 or 2)) throw Invalid();
        var verified = await LocalVerifier.VerifyReceiptOutcomeAsync(canonicalReceipt, authorized, cancellationToken, protocolVersion).ConfigureAwait(false);
        var receipt = verified.Receipt;
        var outcome = receipt.ProgramOutcome;
        if (receipt.ProtocolVersion == 0 || receipt.ModuleId != ReceiptModuleId || receipt.Operation != CallOperation ||
            receipt.ModuleVersion is < 1 or > 4 || !receipt.ActivityId.SequenceEqual(expectedActivityId) ||
            outcome is null || outcome.AbiVersion != expectedGuestAbiVersion || terminalPayload is null ||
            terminalPayload.Length > MaximumProgramBytes || callGraph is null || callGraph.Length == 0 ||
            callGraph.Length > MaximumCallGraphBytes ||
            !System.Security.Cryptography.CryptographicOperations.FixedTimeEquals(System.Security.Cryptography.SHA256.HashData(terminalPayload), outcome.TerminalPayloadRoot) ||
            !System.Security.Cryptography.CryptographicOperations.FixedTimeEquals(System.Security.Cryptography.SHA256.HashData(callGraph), outcome.CallGraphRoot))
            throw new PlatformSdkException(SdkErrorCode.VerificationFailure, RetryClass.Never);
        return verified;
    }

    private async Task<ProgramSubmission> VerifySubmissionAsync(JsonValue value, byte[]? expectedProgramId,
        byte[]? expectedActivityId, string? expectedIdempotencyKey, byte[]? expectedRetained,
        CancellationToken cancellationToken)
    {
        var map = Map(value); var state = Text(map, "state");
        if (state == "unknown")
        {
            var hasRetained = map.ContainsKey("retained_signed_activity");
            RequireFields(map, hasRetained ? ["state", "activity_id", "idempotency_key", "retained_signed_activity"] :
                ["state", "activity_id", "idempotency_key"]);
            var activity = Bytes(map, "activity_id", 32, true); var idempotency = Hex(map, "idempotency_key", 32, true);
            var retained = hasRetained ? Bytes(map, "retained_signed_activity", MaximumProgramBytes) : null;
            if ((expectedActivityId is not null && !Fixed(activity, expectedActivityId)) ||
                (expectedIdempotencyKey is not null && idempotency != expectedIdempotencyKey) ||
                (expectedRetained is not null && (retained is null || !Fixed(retained, expectedRetained)))) throw Verify();
            return new(value, state, activity, idempotency, retained, null);
        }
        if (state is not ("executed" or "refused")) throw Decode();
        var execution = await VerifyExecutionAsync(map, state, true, expectedProgramId, expectedActivityId,
            expectedIdempotencyKey, cancellationToken).ConfigureAwait(false);
        var outcomeKind = Text(Map(Field(map, "outcome")), "kind");
        if (state == "refused" && outcomeKind != "refused" || state == "executed" &&
            outcomeKind is not ("completed" or "legacy_completed")) throw Verify();
        return new(value, state, Bytes(map, "activity_id", 32, true), Text(map, "idempotency_key"), null, execution);
    }

    private async Task<VerifiedProgramExecution> VerifyExecutionAsync(IReadOnlyDictionary<string, JsonValue> map,
        string state, bool idempotent, byte[]? expectedProgramId, byte[]? expectedActivityId,
        string? expectedIdempotencyKey, CancellationToken cancellationToken)
    {
        string[] fields = ["state", "activity_id", "program_id", "guest_abi_version", "module_version",
            "batch_id", "global_sequence", "result_code", "state_root", "receipt", "receipt_digest",
            "terminal_payload", "call_graph", "authority", "usage", "outcome", "verification"];
        if (idempotent) fields = [.. fields, "idempotency_key"];
        RequireFields(map, fields);
        var activity = Bytes(map, "activity_id", 32, true); var program = Bytes(map, "program_id", 32, true);
        if (Text(map, "state") != state || expectedProgramId is not null && !Fixed(program, expectedProgramId) ||
            expectedActivityId is not null && !Fixed(activity, expectedActivityId) ||
            expectedIdempotencyKey is not null && Text(map, "idempotency_key") != expectedIdempotencyKey) throw Verify();
        var guestAbi = Integer(map, "guest_abi_version"); var moduleVersion = Integer(map, "module_version");
        var resultCode = Integer32(map, "result_code"); var globalSequence = DecimalUInt64(map, "global_sequence");
        if (guestAbi is not (1 or 2) || moduleVersion is < 1 or > 4 ||
            Text(map, "verification") != "receipt-terminal-and-call-graph-verified") throw Decode();
        var authorityMap = Map(Field(map, "authority"));
        RequireFields(authorityMap, ["batch_id", "asset", "previous_state_root", "resulting_state_root", "sequencer_public_key"]);
        var authority = Authority(Field(map, "authority"));
        if (!Fixed(authority.BatchId, Bytes(map, "batch_id", 32, true)) ||
            !Fixed(authority.ResultingStateRoot, Bytes(map, "state_root", 32, true)) ||
            !Fixed(authority.SequencerPublicKey, _sequencerPublicKey)) throw Verify();
        var usage = Map(Field(map, "usage"));
        RequireFields(usage, ["cpu_fuel", "memory_bytes", "storage_read_bytes", "storage_write_bytes",
            "output_values", "output_bytes", "fee_units"]);
        var cpu = DecimalUInt64(usage, "cpu_fuel"); var memory = DecimalUInt64(usage, "memory_bytes");
        var read = DecimalUInt64(usage, "storage_read_bytes"); var write = DecimalUInt64(usage, "storage_write_bytes");
        var outputValues = UInt32Integer(usage, "output_values"); var outputBytes = DecimalUInt64(usage, "output_bytes");
        var fee = DecimalUInt128(usage, "fee_units");
        var outcomeDocument = Map(Field(map, "outcome")); var outcomeKind = ValidateOutcome(outcomeDocument);
        var receiptBytes = Bytes(map, "receipt", MaximumProgramBytes); var terminal = Bytes(map, "terminal_payload", MaximumProgramBytes, empty: true);
        var graph = Bytes(map, "call_graph", MaximumProgramBytes); var receiptDigest = Bytes(map, "receipt_digest", 32, true);
        var verified = await VerifyReceiptAsync(receiptBytes, authority, activity, checked((ushort)guestAbi), terminal,
            graph, cancellationToken, _protocolVersion).ConfigureAwait(false);
        var receipt = verified.Receipt; var receiptOutcome = receipt.ProgramOutcome!;
        VerifyTerminal(terminal, graph, program, outcomeDocument, receipt.ProtocolVersion, receiptOutcome);
        var kindMatches = outcomeKind switch
        {
            "completed" or "legacy_completed" => receiptOutcome.TerminalKind == 1 &&
                Integer32(outcomeDocument, "code") == receiptOutcome.ResultCode,
            "refused" => receiptOutcome.TerminalKind is 2 or 3,
            _ => false,
        };
        if (!Fixed(verified.ReceiptDigest, receiptDigest) || receipt.GlobalSequence != globalSequence ||
            receipt.ResultCode != resultCode || receiptOutcome.ResultCode != resultCode ||
            receipt.ModuleVersion != moduleVersion || receiptOutcome.CpuFuel != cpu || receiptOutcome.MemoryBytes != memory ||
            receiptOutcome.StorageReadBytes != read || receiptOutcome.StorageWriteBytes != write ||
            receiptOutcome.OutputValues != outputValues || receiptOutcome.OutputBytes != outputBytes ||
            UInt128Big(receiptOutcome.FeeUnits) != fee || !kindMatches) throw Verify();
        return new(JsonValue.Object(map), verified, terminal, graph);
    }

    private async Task<VerifiedProgramExecution> VerifySimulationAsync(JsonValue value, byte[] expectedProgramId,
        ActivityBinding binding, CancellationToken cancellationToken)
    {
        var map = Map(value); RequireFields(map, ["committed", "execution", "simulation_evidence"]);
        if (Field(map, "committed") is not JsonValue.BooleanValue { Value: false }) throw Verify();
        var executionMap = Map(Field(map, "execution"));
        var execution = await VerifyExecutionAsync(executionMap, "simulated", false, expectedProgramId,
            binding.ActivityId, null, cancellationToken).ConfigureAwait(false);
        var evidence = Map(Field(map, "simulation_evidence"));
        RequireFields(evidence, ["boundary_id", "activity_id", "previous_state_root", "hypothetical_state_root",
            "observed_sequence", "observed_at", "committed", "public_key", "signature"]);
        if (Field(evidence, "committed") is not JsonValue.BooleanValue { Value: false }) throw Verify();
        var authority = Map(Field(executionMap, "authority")); var boundary = Bytes(evidence, "boundary_id", 32, true);
        var publicKey = Bytes(evidence, "public_key", 32, true); var activity = Bytes(evidence, "activity_id", 32, true);
        var previous = Bytes(evidence, "previous_state_root", 32, true);
        var hypothetical = Bytes(evidence, "hypothetical_state_root", 32, true);
        var sequence = DecimalUInt64(evidence, "observed_sequence"); var observedAt = DecimalUInt64(evidence, "observed_at");
        var signature = Bytes(evidence, "signature", 64, true); var now = NowMilliseconds();
        if (sequence == ulong.MaxValue || !Fixed(activity, binding.ActivityId) ||
            !Fixed(activity, Bytes(executionMap, "activity_id", 32, true)) ||
            !Fixed(previous, Bytes(authority, "previous_state_root", 32, true)) ||
            !Fixed(hypothetical, Bytes(authority, "resulting_state_root", 32, true)) ||
            !Fixed(hypothetical, Bytes(executionMap, "state_root", 32, true)) ||
            !Fixed(publicKey, Bytes(authority, "sequencer_public_key", 32, true)) || !Fixed(publicKey, _sequencerPublicKey) ||
            DecimalUInt64(executionMap, "global_sequence") != sequence + 1 || observedAt < binding.NotBefore ||
            observedAt > binding.NotAfter || observedAt > now || now - observedAt > _maximumSimulationAgeMilliseconds ||
            !Fixed(boundary, Digest(Encoding.UTF8.GetBytes("LayerX/emulator/simulation-boundary/v1\0"), publicKey))) throw Verify();
        var signed = new List<byte>(256); signed.AddRange(Encoding.UTF8.GetBytes("LayerX/agent/program-simulation-evidence/v1\0"));
        signed.AddRange(boundary); signed.AddRange(activity); signed.AddRange(previous); signed.AddRange(hypothetical);
        var word = new byte[8]; BinaryPrimitives.WriteUInt64BigEndian(word, sequence); signed.AddRange(word);
        BinaryPrimitives.WriteUInt64BigEndian(word, observedAt); signed.AddRange(word); signed.Add(0);
        if (!LocalVerifier.VerifyEd25519Digest(publicKey, signature, SHA256.HashData(signed.ToArray()))) throw Verify();
        return execution;
    }

    private static object VerifyDiscovery(JsonValue value, string programId, bool @interface, ulong now)
    {
        var map = Map(value);
        string[] fields = @interface
            ? ["program_id", "version", "code_hash", "abi_version", "interface", "interface_digest",
                "receipt_digest", "state_root", "observed_sequence", "observed_at", "valid_through", "source", "verification"]
            : ["program_id", "lifecycle", "version", "code_hash", "abi_version", "receipt_digest", "state_root",
                "observed_sequence", "observed_at", "valid_through", "verification"];
        RequireFields(map, fields);
        var observedAt = DecimalUInt64(map, "observed_at"); var validThrough = DecimalUInt64(map, "valid_through");
        var version = UInt32Integer(map, "version"); var abi = UInt16Integer(map, "abi_version");
        if (Text(map, "program_id") != programId || version == 0 || abi is not (1 or 2) ||
            !Hex32(Text(map, "code_hash")) || !Hex32(Text(map, "receipt_digest")) || !Hex32(Text(map, "state_root")) ||
            validThrough < observedAt || now > validThrough ||
            Text(map, "verification") != (@interface ? "deployment-interface-and-current-head-verified" :
                "registry-receipt-and-current-head-verified")) throw Verify();
        var observedSequence = DecimalUInt64(map, "observed_sequence");
        var program = Bytes(map, "program_id", 32, true); var codeHash = Bytes(map, "code_hash", 32, true);
        var receiptDigest = Bytes(map, "receipt_digest", 32, true); var stateRoot = Bytes(map, "state_root", 32, true);
        if (@interface)
        {
            var bytes = Bytes(map, "interface", MaximumInterfaceBytes); var digest = Bytes(map, "interface_digest", 32, true);
            if (bytes.Length == 0 || !Fixed(SHA256.HashData(bytes), digest)) throw Verify();
            var source = TypedSource(Map(Field(map, "source")));
            return new ProgramInterface(program, version, codeHash, abi, bytes, digest, receiptDigest, stateRoot,
                observedSequence, observedAt, validThrough, source, "server-side-receipt-verification-only");
        }
        var lifecycle = Text(map, "lifecycle") switch
        {
            "active" => ProgramLifecycle.Active, "deprecated" => ProgramLifecycle.Deprecated,
            "tombstoned" => ProgramLifecycle.Tombstoned, _ => throw Decode()
        };
        return new ProgramDiscovery(program, lifecycle, version, codeHash, abi, receiptDigest, stateRoot,
            observedSequence, observedAt, validThrough, "server-side-receipt-verification-only");
    }

    private static ProgramSource TypedSource(IReadOnlyDictionary<string, JsonValue> source)
    {
        ValidateSource(source);
        return Text(source, "status") switch
        {
            "unpublished" => new ProgramSource.Unpublished(),
            "verified" => new ProgramSource.Verified(Bytes(source, "source_digest", 32, true),
                Bytes(source, "environment_digest", 32, true), Text(source, "pipeline")),
            "mismatch" => new ProgramSource.Mismatch(Bytes(source, "expected_code_hash", 32, true),
                Bytes(source, "reproduced_artifact_digest", 32, true)),
            _ => throw Decode()
        };
    }

    private static AuthorizedReceiptBatch Authority(JsonValue value)
    {
        var map = Map(value); return new(Bytes(map, "batch_id", 32, true), Bytes(map, "asset", 32, true),
            Bytes(map, "previous_state_root", 32, true), Bytes(map, "resulting_state_root", 32, true),
            Bytes(map, "sequencer_public_key", 32, true));
    }

    private static void ValidateSource(IReadOnlyDictionary<string, JsonValue> source)
    {
        switch (Text(source, "status"))
        {
            case "unpublished": RequireFields(source, ["status"]); break;
            case "verified":
                RequireFields(source, ["status", "source_digest", "environment_digest", "pipeline"]);
                _ = Bytes(source, "source_digest", 32, true); _ = Bytes(source, "environment_digest", 32, true);
                if (Text(source, "pipeline") != "sha256-source-artifact-reproducible-build-v1") throw Decode();
                break;
            case "mismatch":
                RequireFields(source, ["status", "expected_code_hash", "reproduced_artifact_digest"]);
                _ = Bytes(source, "expected_code_hash", 32, true); _ = Bytes(source, "reproduced_artifact_digest", 32, true);
                break;
            default: throw Decode();
        }
    }

    private static string ValidateOutcome(IReadOnlyDictionary<string, JsonValue> outcome)
    {
        var kind = Text(outcome, "kind");
        switch (kind)
        {
            case "completed": RequireFields(outcome, ["kind", "code", "response"]);
                _ = Integer32(outcome, "code"); _ = Bytes(outcome, "response", MaximumProgramBytes, empty: true); break;
            case "legacy_completed":
                RequireFields(outcome, ["kind", "code", "values"]); _ = Integer32(outcome, "code");
                if (Field(outcome, "values") is not JsonValue.ArrayValue values || values.Value.Count > 512) throw Decode();
                foreach (var value in values.Value) ValidateLegacyValue(value); break;
            case "refused": RequireFields(outcome, ["kind", "failure"]); ValidateFailure(Map(Field(outcome, "failure"))); break;
            default: throw Decode();
        }
        return kind;
    }

    private static void ValidateLegacyValue(JsonValue value)
    {
        var map = Map(value); RequireFields(map, ["type", "value"]);
        if (Text(map, "type") == "i32") _ = Integer32(map, "value");
        else if (Text(map, "type") == "i64") _ = DecimalInt64(map, "value");
        else throw Decode();
    }

    private static void ValidateFailure(IReadOnlyDictionary<string, JsonValue> failure)
    {
        switch (Text(failure, "kind"))
        {
            case "unknown_program": case "reentrancy": case "authority": case "resource": case "response": case "fault":
                RequireFields(failure, ["kind"]); break;
            case "depth_exceeded": case "fanout_exceeded":
                RequireFields(failure, ["kind", "limit", "attempted"]);
                _ = UInt32Integer(failure, "limit"); _ = UInt32Integer(failure, "attempted"); break;
            case "guest_refused": RequireFields(failure, ["kind", "code"]); _ = Integer32(failure, "code"); break;
            default: throw Decode();
        }
    }

    private static void VerifyTerminal(byte[] encoded, byte[] availableGraph, byte[] expectedProgram,
        IReadOnlyDictionary<string, JsonValue> documentOutcome, ushort protocolVersion, ProgramReceiptOutcome receipt)
    {
        try
        {
            var attachments = UnwrapTerminal(encoded); var inner = attachments.Inner;
            var candidate = Starts(inner, "LXP/program-execution/v4\0"); var successful = false;
            if (Starts(inner, "LXP/program-execution/v2\0") || Starts(inner, "LXP/program-execution/v3\0"))
            {
                var traced = Starts(inner, "LXP/program-execution/v3\0"); var domain = Encoding.UTF8.GetBytes(
                    traced ? "LXP/program-execution/v3\0" : "LXP/program-execution/v2\0");
                var cursor = new TerminalCursor(inner, domain.Length); var runtime = cursor.U16(); var abi = cursor.U16();
                var metering = cursor.U32(); var countValue = cursor.U128();
                if (countValue > cursor.Remaining / 5) throw new InvalidDataException(); var count = (int)countValue;
                if (Field(documentOutcome, "values") is not JsonValue.ArrayValue values || values.Value.Count != count ||
                    Text(documentOutcome, "kind") != "legacy_completed" || runtime == 0 || metering == 0 ||
                    runtime != receipt.RuntimeVersion || abi != 1 || abi != receipt.AbiVersion ||
                    metering != receipt.MeteringScheduleVersion) throw new InvalidDataException();
                for (var index = 0; index < count; index++)
                {
                    var value = Map(values.Value[index]); var tag = cursor.U8();
                    if (tag == 1 && (Text(value, "type") != "i32" || cursor.I32() != Integer32(value, "value"))) throw new InvalidDataException();
                    if (tag == 2 && (Text(value, "type") != "i64" || cursor.I64() != DecimalInt64(value, "value"))) throw new InvalidDataException();
                    if (tag is not (1 or 2)) throw new InvalidDataException();
                }
                var usage = new TerminalUsage(cursor.U64(), cursor.U64(), cursor.U64(), cursor.U64(), cursor.U32(), 0, cursor.U128());
                if (traced && (cursor.U8() != 1 || cursor.Sized64().Length > 34 + 65_536 * 52)) throw new InvalidDataException();
                cursor.Finish(); if (receipt.TerminalKind != 1 || Integer32(documentOutcome, "code") < 0) throw new InvalidDataException();
                MatchUsage(usage, receipt); successful = true;
            }
            else if (candidate)
            {
                var cursor = new TerminalCursor(inner, Encoding.UTF8.GetByteCount("LXP/program-execution/v4\0"));
                var runtime = cursor.U16(); var feeSchedule = cursor.U32(); var metering = cursor.U32();
                var countValue = cursor.U64(); if (countValue > (ulong)(cursor.Remaining / 5)) throw new InvalidDataException();
                for (var index = 0; index < (int)countValue; index++)
                {
                    var tag = cursor.U8(); if (tag == 1) _ = cursor.I32(); else if (tag == 2) _ = cursor.I64();
                    else throw new InvalidDataException();
                }
                var usage = new TerminalUsage(cursor.U64(), cursor.U64(), cursor.U64(), cursor.U64(), cursor.U32(),
                    cursor.U64(), cursor.U128());
                var traceTag = cursor.U8(); if (traceTag == 1 && cursor.Sized64().Length > 34 + 65_536 * 52) throw new InvalidDataException();
                if (traceTag is not (0 or 1)) throw new InvalidDataException();
                var program = cursor.Take(32); var abi = cursor.U16(); var outcomeTag = cursor.U8(); string expectedKind;
                if (outcomeTag == 0)
                {
                    var code = cursor.I32(); var response = cursor.Sized64();
                    if (code < 0 || response.Length > MaximumProgramBytes || Text(documentOutcome, "kind") != "completed" ||
                        code != Integer32(documentOutcome, "code") || !Fixed(response, Bytes(documentOutcome, "response", MaximumProgramBytes, empty: true)))
                        throw new InvalidDataException();
                    expectedKind = "completed"; successful = true;
                }
                else if (outcomeTag == 1) { ValidateAuthenticatedProgramFailure(cursor.Sized64()); expectedKind = "guest_refused"; }
                else if (outcomeTag == 2) { ValidateCandidateResource(cursor, usage); expectedKind = "resource"; }
                else throw new InvalidDataException();
                var graph = cursor.Sized64(); cursor.Finish();
                if (graph.Length > MaximumCallGraphBytes || !Fixed(graph, availableGraph) || !Fixed(program, expectedProgram) ||
                    abi != 2 || abi != receipt.AbiVersion || runtime == 0 || feeSchedule == 0 || metering == 0 ||
                    runtime != receipt.RuntimeVersion || feeSchedule != receipt.FeeScheduleVersion ||
                    metering != receipt.MeteringScheduleVersion) throw new InvalidDataException();
                MatchUsage(usage, receipt);
                if (outcomeTag == 0) { if (receipt.TerminalKind != 1) throw new InvalidDataException(); }
                else { RequireRefusal(documentOutcome, expectedKind, receipt.ResultCode); if (receipt.TerminalKind == 1) throw new InvalidDataException(); }
            }
            else if (Starts(inner, "LXP/programs/failure-detail/v1\0"))
            {
                var cursor = new TerminalCursor(inner, Encoding.UTF8.GetByteCount("LXP/programs/failure-detail/v1\0"));
                var family = cursor.U8(); var payload = cursor.Sized32(); cursor.Finish();
                if (family is < 1 or > 4 || payload.Length == 0) throw new InvalidDataException();
                ValidateFailureDetail(family, payload); RequireRefusal(documentOutcome, "guest_refused", receipt.ResultCode);
                if (receipt.TerminalKind != 2) throw new InvalidDataException();
            }
            else if (Starts(inner, "LXP/programs/resource-detail/v1\0"))
            {
                var cursor = new TerminalCursor(inner, Encoding.UTF8.GetByteCount("LXP/programs/resource-detail/v1\0"));
                ValidateLegacyResource(cursor); cursor.Finish(); RequireRefusal(documentOutcome, "resource", receipt.ResultCode);
                if (receipt.TerminalKind != 3) throw new InvalidDataException();
            }
            else if (Starts(inner, "LXP/programs/settlement-failure/v1\0"))
            {
                var cursor = new TerminalCursor(inner, Encoding.UTF8.GetByteCount("LXP/programs/settlement-failure/v1\0"));
                if (cursor.U8() is < 1 or > 12) throw new InvalidDataException(); cursor.Finish();
                RequireRefusal(documentOutcome, "guest_refused", receipt.ResultCode); if (receipt.TerminalKind != 2) throw new InvalidDataException();
            }
            else if (Starts(inner, "LXP/programs/callback-failure/v1\0"))
            {
                var cursor = new TerminalCursor(inner, Encoding.UTF8.GetByteCount("LXP/programs/callback-failure/v1\0"));
                _ = cursor.U8(); _ = cursor.I32(); cursor.Finish(); RequireRefusal(documentOutcome, "guest_refused", receipt.ResultCode);
                if (receipt.TerminalKind != 2) throw new InvalidDataException();
            }
            else throw new InvalidDataException();
            VerifyTerminalAttachments(attachments, candidate, successful, protocolVersion, receipt);
        }
        catch { throw Verify(); }
    }

    private static void MatchUsage(TerminalUsage usage, ProgramReceiptOutcome receipt)
    {
        if (usage.Cpu != receipt.CpuFuel || usage.Memory != receipt.MemoryBytes || usage.Read != receipt.StorageReadBytes ||
            usage.Write != receipt.StorageWriteBytes || usage.Values != receipt.OutputValues || usage.OutputBytes != receipt.OutputBytes ||
            usage.Fee != UInt128Big(receipt.FeeUnits)) throw new InvalidDataException();
    }

    private static void RequireRefusal(IReadOnlyDictionary<string, JsonValue> outcome, string expected, int code)
    {
        if (Text(outcome, "kind") != "refused") throw new InvalidDataException(); var failure = Map(Field(outcome, "failure"));
        if (Text(failure, "kind") != expected || expected == "guest_refused" && Integer32(failure, "code") != code)
            throw new InvalidDataException();
    }

    private static TerminalAttachments UnwrapTerminal(byte[] encoded)
    {
        var current = encoded; byte[]? authorization = null; byte[]? transferRoot = null; byte[]? occupancy = null;
        var authorityDomain = Encoding.UTF8.GetBytes("LXP/program-execution-with-transfer-authority/v2\0");
        var occupancyDomain = Encoding.UTF8.GetBytes("LXP/program-execution-with-occupancy/v1\0");
        if (Starts(current, authorityDomain))
        {
            var cursor = new TerminalCursor(current, authorityDomain.Length); current = cursor.Sized32();
            authorization = cursor.Sized32(); transferRoot = cursor.Take(32); cursor.Finish();
        }
        if (Starts(current, occupancyDomain))
        {
            var cursor = new TerminalCursor(current, occupancyDomain.Length); current = cursor.Sized32();
            occupancy = cursor.Sized32(); cursor.Finish();
        }
        if (Starts(current, authorityDomain) || Starts(current, occupancyDomain)) throw new InvalidDataException();
        return new(current, occupancy, authorization, transferRoot);
    }

    private static void VerifyTerminalAttachments(TerminalAttachments attachments, bool candidate, bool successful,
        ushort protocolVersion, ProgramReceiptOutcome receipt)
    {
        if (protocolVersion is not (1 or 2 or 3)) throw new InvalidDataException();
        var occupancyRequired = (protocolVersion == 2 || protocolVersion == 3) && successful;
        if (occupancyRequired != (attachments.Occupancy is not null)) throw new InvalidDataException();
        if (attachments.Occupancy is { } occupancy)
        {
            if (occupancy.Length == 0)
            {
                if (receipt.OccupancyEvidenceDigest.Any(value => value != 0) || receipt.OccupancyTransferRoot.Any(value => value != 0) ||
                    UInt128Big(receipt.OccupancyByteBatches) != 0 || UInt128Big(receipt.OccupancyFeeUnits) != 0) throw new InvalidDataException();
            }
            else
            {
                if (!Fixed(SHA256.HashData(occupancy), receipt.OccupancyEvidenceDigest)) throw new InvalidDataException();
                var settlement = DecodeOccupancySettlement(occupancy);
                if (settlement.ByteBatches != UInt128Big(receipt.OccupancyByteBatches) ||
                    settlement.FeeUnits != UInt128Big(receipt.OccupancyFeeUnits) ||
                    !Fixed(OccupancyTransferRoot(settlement, receipt.OccupancyAssetId), receipt.OccupancyTransferRoot))
                    throw new InvalidDataException();
            }
        }
        else if (receipt.OccupancyEvidenceDigest.Any(value => value != 0) || receipt.OccupancyTransferRoot.Any(value => value != 0) ||
            UInt128Big(receipt.OccupancyByteBatches) != 0 || UInt128Big(receipt.OccupancyFeeUnits) != 0)
            throw new InvalidDataException();
        var transferPresent = receipt.TransferRoot.Any(value => value != 0);
        if (candidate ? (attachments.Authorization is not null) != transferPresent : attachments.Authorization is not null)
            throw new InvalidDataException();
        if (attachments.Authorization is { } authorization)
        {
            if (authorization.Length == 0 || attachments.TransferRoot is null || !Fixed(attachments.TransferRoot, receipt.TransferRoot))
                throw new InvalidDataException();
            VerifyAuthorizationRoot(authorization, receipt.TransferRoot);
        }
    }

    private static void VerifyAuthorizationRoot(byte[] encoded, byte[] expected)
    {
        if (!Fixed(DecodeAuthorizationRoot(encoded), expected)) throw new InvalidDataException();
    }

    private static byte[] DecodeAuthorizationRoot(byte[] encoded)
    {
        var v1 = Encoding.UTF8.GetBytes("LayerX/programs/402LXP/transfer-set/v1\0");
        var v2 = Encoding.UTF8.GetBytes("LayerX/programs/402LXP/transfer-set/v2\0");
        var candidate = Starts(encoded, v2); var domain = candidate ? v2 : v1; var cursor = new TerminalCursor(encoded, 0);
        if (!Starts(encoded, domain) || !Fixed(cursor.Take(domain.Length), domain)) throw new InvalidDataException();
        RequireNonzero(cursor.Take(32)); var principal = cursor.Take(32); RequireNonzero(principal);
        RequireNonzero(cursor.Take(32)); _ = DecodeFrame(cursor);
        DecodeEventEnvelope(cursor.Take(checked((int)cursor.U32())));
        var callCount = cursor.U64(); if (callCount > 64) throw new InvalidDataException();
        for (var index = 0; index < (int)callCount; index++)
        {
            RequireNonzero(cursor.Take(32)); RequireNonzero(cursor.Take(32)); RequireNonzero(cursor.Take(32));
            _ = DecodeFrame(cursor); _ = DecodeFrame(cursor);
            DecodeCapabilitySet(cursor.Take(checked((int)cursor.U32())), candidate);
        }
        var legCount = cursor.U64(); if (legCount == 0 || legCount > 256) throw new InvalidDataException();
        var kernelLegs = new List<byte[]>(); var total = BigInteger.Zero;
        for (var index = 0; index < (int)legCount; index++)
        {
            var frame = DecodeFrame(cursor); var source = principal; ProgramAuthorityBinding? authority = null;
            ProgramFundingBinding? funding = null;
            if (candidate)
            {
                switch (cursor.U8())
                {
                    case 1:
                        source = cursor.Take(32); RequireNonzero(source);
                        if (!Fixed(source, principal)) throw new InvalidDataException(); break;
                    case 2:
                        authority = DecodeProgramAuthority(cursor.Sized32()); source = authority.Source; break;
                    case 3:
                        source = cursor.Take(32); RequireNonzero(source);
                        if (!Fixed(source, principal)) throw new InvalidDataException();
                        funding = DecodeProgramFunding(cursor.Sized32()); break;
                    default: throw new InvalidDataException();
                }
            }
            var asset = cursor.Take(32); var destination = cursor.Take(32); var amount = cursor.U128(); var program = cursor.Take(32);
            RequireNonzero(asset); RequireNonzero(destination); RequireNonzero(program); if (amount == 0) throw new InvalidDataException();
            if (authority is not null && (!Fixed(authority.Owner, program) || !Fixed(authority.Frame, frame) ||
                !Fixed(authority.Asset, asset) || !Fixed(authority.Destination, destination) || authority.Amount != amount))
                throw new InvalidDataException();
            if (funding is not null && (!Fixed(funding.Owner, program) || !Fixed(funding.Destination, destination) ||
                !Fixed(funding.Asset, asset))) throw new InvalidDataException();
            total = CheckedU128Add(total, amount);
            kernelLegs.Add(Concatenate([0], source, destination, asset, UInt128Bytes(amount), BigEndian(1, 2)));
        }
        cursor.Finish(); _ = total;
        return MerkleRoot(kernelLegs);
    }

    private static ProgramAuthorityBinding DecodeProgramAuthority(byte[] encoded)
    {
        var domain = Encoding.UTF8.GetBytes("LayerX/programs/402LXP/program-authority/v1\0");
        var cursor = new TerminalCursor(encoded, 0); if (!Fixed(cursor.Take(domain.Length), domain)) throw new InvalidDataException();
        var owner = cursor.Take(32); RequireNonzero(owner); var seedLength = cursor.U16();
        if (seedLength > 128) throw new InvalidDataException(); var seed = cursor.Take(seedLength); var source = cursor.Take(32);
        var frame = DecodeFrame(cursor); var asset = cursor.Take(32); var destination = cursor.Take(32); var amount = cursor.U128();
        cursor.Finish(); RequireNonzero(asset); RequireNonzero(destination);
        if (amount == 0 || !Fixed(DeriveProgramAccount(owner, seed), source)) throw new InvalidDataException();
        return new(owner, frame, source, asset, destination, amount);
    }

    private static ProgramFundingBinding DecodeProgramFunding(byte[] encoded)
    {
        var domain = Encoding.UTF8.GetBytes("LayerX/programs/402LXP/program-funding/v1\0");
        var cursor = new TerminalCursor(encoded, 0); if (!Fixed(cursor.Take(domain.Length), domain)) throw new InvalidDataException();
        var owner = cursor.Take(32); RequireNonzero(owner); var seedLength = cursor.U16();
        if (seedLength > 128) throw new InvalidDataException(); var seed = cursor.Take(seedLength);
        var destination = cursor.Take(32); var asset = cursor.Take(32); cursor.Finish();
        RequireNonzero(destination); RequireNonzero(asset);
        if (!Fixed(DeriveProgramAccount(owner, seed), destination)) throw new InvalidDataException();
        return new(owner, destination, asset);
    }

    private static byte[] DeriveProgramAccount(byte[] owner, byte[] seed) => Digest(
        Encoding.UTF8.GetBytes("LayerX/programs/program-account/v1\0"), owner, BigEndian((ulong)seed.Length, 4), seed);

    private static void DecodeEventEnvelope(byte[] encoded)
    {
        var domain = Encoding.UTF8.GetBytes("LayerX/programs/events/v1\0"); var cursor = new TerminalCursor(encoded, 0);
        if (!Fixed(cursor.Take(domain.Length), domain)) throw new InvalidDataException();
        var count = cursor.U32(); if (count > 64) throw new InvalidDataException();
        for (var index = 0; index < count; index++)
        {
            RequireNonzero(cursor.Take(32)); RequireNonzero(cursor.Take(32)); _ = DecodeFrame(cursor);
            if (cursor.Sized32().Length > 64 || cursor.Sized32().Length > 65_536) throw new InvalidDataException();
        }
        cursor.Finish();
    }

    private static byte[] DecodeFrame(TerminalCursor cursor)
    {
        var path = cursor.Take(8); var depth = cursor.U8();
        if (depth > 8 || path.Take(depth).Any(value => value == 0) || path.Skip(depth).Any(value => value != 0))
            throw new InvalidDataException();
        return Concatenate(path, [depth]);
    }

    private static void DecodeCapabilitySet(byte[] encoded, bool candidate)
    {
        if (encoded.Length < 2 || encoded.Length > 65_535) throw new InvalidDataException();
        var cursor = new TerminalCursor(encoded, 0); var count = cursor.U16(); if (count > 269) throw new InvalidDataException();
        CapabilityKey? prior = null; var balanceViews = 0;
        for (var index = 0; index < count; index++)
        {
            CapabilityKey key;
            switch (cursor.U8())
            {
                case 1: key = new(0, []); break;
                case 2: key = new(1, []); break;
                case 3: key = new(2, []); break;
                case 4:
                    var program = cursor.Take(32); RequireNonzero(program); key = new(3, [program]); break;
                case 5:
                    var asset = cursor.Take(32); var destination = cursor.Take(32); var maximum = cursor.U128();
                    RequireNonzero(asset); RequireNonzero(destination); if (maximum == 0) throw new InvalidDataException();
                    key = new(4, [asset, destination]); break;
                case 9 when candidate:
                    var owner = cursor.Take(32); RequireNonzero(owner); var seedLength = cursor.U16();
                    if (seedLength > 128) throw new InvalidDataException(); var seed = cursor.Take(seedLength);
                    var source = cursor.Take(32); var spendAsset = cursor.Take(32); var spendDestination = cursor.Take(32);
                    var spendMaximum = cursor.U128(); RequireNonzero(spendAsset); RequireNonzero(spendDestination);
                    if (spendMaximum == 0 || !Fixed(DeriveProgramAccount(owner, seed), source)) throw new InvalidDataException();
                    key = new(5, [owner, seed, source, spendAsset, spendDestination]); break;
                case 6:
                    var receipt = cursor.Take(32); RequireNonzero(receipt); key = new(6, [receipt]); break;
                case 10 when candidate:
                    var account = cursor.Take(32); var balanceAsset = cursor.Take(32); var balanceReceipt = cursor.Take(32);
                    RequireNonzero(account); RequireNonzero(balanceAsset); RequireNonzero(balanceReceipt);
                    if (++balanceViews > 32) throw new InvalidDataException(); key = new(7, [account, balanceAsset]); break;
                case 7: key = new(8, []); break;
                case 8: key = new(9, []); break;
                default: throw new InvalidDataException();
            }
            if (prior is not null && CompareCapabilityKeys(prior, key) >= 0) throw new InvalidDataException(); prior = key;
        }
        cursor.Finish();
    }

    private static int CompareCapabilityKeys(CapabilityKey left, CapabilityKey right)
    {
        if (left.Order != right.Order) return left.Order.CompareTo(right.Order);
        for (var index = 0; index < Math.Min(left.Fields.Count, right.Fields.Count); index++)
        {
            var order = CompareBytes(left.Fields[index], right.Fields[index]); if (order != 0) return order;
        }
        return left.Fields.Count.CompareTo(right.Fields.Count);
    }

    private static OccupancySettlementBinding DecodeOccupancySettlement(byte[] encoded)
    {
        if (encoded.Length > 65_536) throw new InvalidDataException();
        var v1 = Encoding.UTF8.GetBytes("LXP/storage-occupancy-settlement/v1\0");
        var v2 = Encoding.UTF8.GetBytes("LXP/storage-occupancy-settlement/v2\0");
        var v3 = Encoding.UTF8.GetBytes("LXP/storage-occupancy-settlement/v3\0");
        if (Starts(encoded, v1) || Starts(encoded, v2)) return DecodeLegacyOccupancy(encoded, v1, v2);
        var cursor = new TerminalCursor(encoded, 0); if (!Fixed(cursor.Take(v3.Length), v3)) throw new InvalidDataException();
        var batch = cursor.U64(); var occupancyPrice = DecodeOccupancySchedule(cursor, true);
        var declaredUnits = cursor.U128(); var declaredFee = cursor.U128(); var declaredPaid = cursor.U128();
        var declaredArrears = cursor.U128(); var count = cursor.U32(); if (count > 256) throw new InvalidDataException();
        var byteBatches = BigInteger.Zero; var feeUnits = BigInteger.Zero; var paidUnits = BigInteger.Zero;
        var arrearsUnits = BigInteger.Zero; byte[]? priorNamespace = null; var charges = new List<OccupancyChargeBinding>();
        for (var index = 0; index < count; index++)
        {
            var storageNamespace = DecodeStorageNamespace(cursor);
            if (priorNamespace is not null && CompareBytes(priorNamespace, storageNamespace.Canonical) >= 0) throw new InvalidDataException();
            priorNamespace = storageNamespace.Canonical; var payer = cursor.Take(32); RequireNonzero(payer);
            if (storageNamespace.Principal is not null && !Fixed(storageNamespace.Principal, payer)) throw new InvalidDataException();
            var rootProgram = cursor.Take(32); RequireNonzero(rootProgram); var activity = cursor.Take(32);
            var fromBatch = cursor.U64(); var toBatch = cursor.U64(); var recordedBytes = cursor.U64(); var finalBytes = cursor.U64();
            var units = cursor.U128(); var price = cursor.U64(); var accrued = cursor.U128(); var priorArrears = cursor.U128();
            var amountDue = cursor.U128(); var authorizedAdded = cursor.U128(); var disposition = cursor.U8();
            if (disposition is < 1 or > 5) throw new InvalidDataException(); var arrearsAfter = cursor.U128();
            var maximumBytes = cursor.U64(); var maximumPrice = cursor.U64(); _ = cursor.U128(); var mandate = cursor.Take(32);
            if (toBatch < fromBatch) throw new InvalidDataException();
            var expectedUnits = CheckedU128Multiply(recordedBytes, toBatch - fromBatch);
            var expectedFee = CheckedU128Multiply(expectedUnits, price); var expectedDue = CheckedU128Add(priorArrears, expectedFee);
            var migration = disposition == 5;
            if (toBatch != batch || !migration && price != occupancyPrice || units != expectedUnits || accrued != expectedFee ||
                amountDue != expectedDue || finalBytes > maximumBytes || !migration && (mandate.All(value => value == 0) || activity.All(value => value == 0)) ||
                migration && (price != 0 || accrued != 0 || priorArrears != 0 || amountDue != 0 || arrearsAfter != 0 ||
                    mandate.Any(value => value != 0) || activity.Any(value => value != 0) || !Fixed(rootProgram, storageNamespace.Program)) ||
                (disposition == 4) != (price > maximumPrice) || disposition == 1 && arrearsAfter != 0 ||
                disposition != 1 && arrearsAfter != amountDue) throw new InvalidDataException();
            if (authorizedAdded != 0)
            {
                var expectedMandate = Digest(Encoding.UTF8.GetBytes("LXP/storage-occupancy-mandate/v1\0"), payer,
                    rootProgram, activity, storageNamespace.Wire, BigEndian(maximumBytes, 8), BigEndian(maximumPrice, 8),
                    UInt128Bytes(authorizedAdded));
                if (!Fixed(mandate, expectedMandate)) throw new InvalidDataException();
            }
            byteBatches = CheckedU128Add(byteBatches, units); feeUnits = CheckedU128Add(feeUnits, accrued);
            if (disposition == 1) paidUnits = CheckedU128Add(paidUnits, amountDue);
            else arrearsUnits = CheckedU128Add(arrearsUnits, arrearsAfter);
            charges.Add(new(payer, amountDue, disposition == 1, arrearsAfter));
        }
        cursor.Finish();
        if (byteBatches != declaredUnits || feeUnits != declaredFee || paidUnits != declaredPaid || arrearsUnits != declaredArrears)
            throw new InvalidDataException();
        return new(byteBatches, feeUnits, charges.AsReadOnly());
    }

    private static OccupancySettlementBinding DecodeLegacyOccupancy(byte[] encoded, byte[] v1, byte[] v2)
    {
        var versioned = Starts(encoded, v2); var domain = versioned ? v2 : v1; var cursor = new TerminalCursor(encoded, 0);
        if (!Fixed(cursor.Take(domain.Length), domain)) throw new InvalidDataException();
        var batch = cursor.U64(); var occupancyPrice = DecodeOccupancySchedule(cursor, versioned);
        var declaredUnits = cursor.U128(); var declaredFee = cursor.U128(); var count = cursor.U64();
        if (count > 256) throw new InvalidDataException(); var byteBatches = BigInteger.Zero; var feeUnits = BigInteger.Zero;
        var charges = new List<OccupancyChargeBinding>();
        for (var index = 0; index < (int)count; index++)
        {
            _ = DecodeStorageNamespace(cursor); var payer = cursor.Take(32); RequireNonzero(payer);
            var fromBatch = cursor.U64(); var toBatch = cursor.U64(); var recordedBytes = cursor.U64(); _ = cursor.U64();
            var units = cursor.U128(); var price = cursor.U64(); var accrued = cursor.U128();
            if (toBatch < fromBatch) throw new InvalidDataException(); var expectedUnits = CheckedU128Multiply(recordedBytes, toBatch - fromBatch);
            if (toBatch != batch || units != expectedUnits || price != occupancyPrice ||
                accrued != CheckedU128Multiply(units, price)) throw new InvalidDataException();
            byteBatches = CheckedU128Add(byteBatches, units); feeUnits = CheckedU128Add(feeUnits, accrued);
            charges.Add(new(payer, accrued, true, BigInteger.Zero));
        }
        cursor.Finish(); if (byteBatches != declaredUnits || feeUnits != declaredFee) throw new InvalidDataException();
        return new(byteBatches, feeUnits, charges.AsReadOnly());
    }

    private static ulong DecodeOccupancySchedule(TerminalCursor cursor, bool versioned)
    {
        var version = versioned ? cursor.U32() : 1; if (version == 0) throw new InvalidDataException();
        ulong occupancyPrice = 0; for (var index = 0; index < 7; index++) occupancyPrice = cursor.U64(); return occupancyPrice;
    }

    private static StorageNamespaceBinding DecodeStorageNamespace(TerminalCursor cursor)
    {
        var length = cursor.U8(); if (length is not (33 or 65)) throw new InvalidDataException();
        var canonical = cursor.Take(length); var program = canonical.AsSpan(0, 32).ToArray(); RequireNonzero(program);
        var tag = canonical[32]; byte[]? principal = null;
        if (tag == 0 && length == 65) { principal = canonical.AsSpan(33).ToArray(); RequireNonzero(principal); }
        else if (!(tag == 1 && length == 33) && !(tag == 2 && length == 65)) throw new InvalidDataException();
        return new(canonical, Concatenate([length], canonical), program, principal);
    }

    private static byte[] OccupancyTransferRoot(OccupancySettlementBinding settlement, byte[] asset)
    {
        if (asset.Length != 32) throw new InvalidDataException(); RequireNonzero(asset);
        var payers = new Dictionary<string, OccupancyPayer>(StringComparer.Ordinal);
        foreach (var charge in settlement.Charges)
        {
            var key = Convert.ToHexString(charge.Payer); if (!payers.TryGetValue(key, out var entry))
            {
                entry = new OccupancyPayer { Payer = charge.Payer }; payers.Add(key, entry);
            }
            entry.Due = CheckedU128Add(entry.Due, charge.AmountDue);
            if (charge.Paid) entry.Paid = CheckedU128Add(entry.Paid, charge.AmountDue);
            entry.Arrears = CheckedU128Add(entry.Arrears, charge.ArrearsAfter);
        }
        var treasury = Digest(Encoding.UTF8.GetBytes("LX:ACCOUNT:v1"), BigEndian(11, 4), Encoding.UTF8.GetBytes("system:fees"));
        var legs = new List<byte[]>();
        foreach (var entry in payers.Values.Where(value => value.Due != 0 || value.Arrears != 0)
            .OrderBy(value => value.Payer, Comparer<byte[]>.Create(CompareBytes)))
        {
            if (entry.Paid != 0) legs.Add(Concatenate([0], entry.Payer, treasury, asset, UInt128Bytes(entry.Paid), BigEndian(23, 2)));
        }
        return MerkleRoot(legs);
    }

    private static byte[] MerkleRoot(IReadOnlyList<byte[]> legs)
    {
        if (legs.Count == 0) return new byte[32];
        var level = legs.Select(leg => Digest(Encoding.UTF8.GetBytes("LXP/v1/merkle-leaf\0"), leg)).ToList();
        while (level.Count > 1)
        {
            var next = new List<byte[]>();
            for (var index = 0; index < level.Count; index += 2)
                next.Add(Digest(Encoding.UTF8.GetBytes("LXP/v1/merkle-internal\0"), level[index],
                    index + 1 < level.Count ? level[index + 1] : level[index]));
            level = next;
        }
        return level[0];
    }

    private static BigInteger CheckedU128Add(BigInteger left, BigInteger right)
    {
        var value = left + right; return value <= ((BigInteger.One << 128) - 1) ? value : throw new InvalidDataException();
    }

    private static BigInteger CheckedU128Multiply(BigInteger left, BigInteger right)
    {
        var value = left * right; return value <= ((BigInteger.One << 128) - 1) ? value : throw new InvalidDataException();
    }

    private static byte[] BigEndian(ulong value, int length)
    {
        if (length is < 1 or > 8 || length < 8 && value >= (BigInteger.One << (length * 8))) throw new InvalidDataException();
        var encoded = new byte[8]; BinaryPrimitives.WriteUInt64BigEndian(encoded, value); return encoded.AsSpan(8 - length).ToArray();
    }

    private static byte[] Concatenate(params byte[][] values)
    {
        var length = values.Sum(value => value.Length); var result = new byte[length]; var offset = 0;
        foreach (var value in values) { value.CopyTo(result, offset); offset += value.Length; } return result;
    }

    private static int CompareBytes(byte[] left, byte[] right)
    {
        for (var index = 0; index < Math.Min(left.Length, right.Length); index++)
            if (left[index] != right[index]) return left[index].CompareTo(right[index]);
        return left.Length.CompareTo(right.Length);
    }

    private static void ValidateAuthenticatedProgramFailure(byte[] encoded)
    {
        var cursor = new TerminalCursor(encoded, 0); var program = cursor.Take(32); var failureClass = cursor.U32();
        var reason = cursor.Sized32(); cursor.Finish();
        if (program.All(value => value == 0) || failureClass is not (1 or 2 or 3 or 4 or 5 or 254 or 255) ||
            reason.Length > 4_096 || failureClass is 254 or 255 && reason.Length != 0) throw new InvalidDataException();
    }

    private static void ValidateFailureDetail(int family, byte[] encoded)
    {
        if (family == 1) { ValidateAuthenticatedProgramFailure(encoded); return; }
        var cursor = new TerminalCursor(encoded, 0); var tag = cursor.U8();
        if (family == 2) ValidateCompositionFailure(cursor, tag);
        else if (family == 3) ValidateEntrypointFailure(cursor, tag);
        else if (family == 4) ValidateAbiFailure(cursor, tag);
        else throw new InvalidDataException();
        cursor.Finish();
    }

    private static void ValidateCompositionFailure(TerminalCursor cursor, int tag)
    {
        switch (tag)
        {
            case 1: case 9: case 10: case 11: case 20: case 21: case 22: break;
            case 2:
                if (cursor.U8() is < 1 or > 2 || cursor.U8() is < 1 or > 2) throw new InvalidDataException(); break;
            case 3: case 4: RequireNonzero(cursor.Take(32)); break;
            case 5: case 6: case 7: _ = cursor.U32(); _ = cursor.U32(); break;
            case 8: RequireNonzero(cursor.Take(32)); _ = cursor.U32(); _ = cursor.U32(); break;
            case 12: _ = cursor.I32(); break;
            case 13: _ = cursor.U64(); _ = cursor.U64(); break;
            case 14: RequireNonzero(cursor.Take(32)); _ = cursor.I32(); break;
            case 15: ValidateAuthenticatedProgramFailure(cursor.Rest()); break;
            case 16: ValidateAbiFailure(cursor, cursor.U8()); break;
            case 17: ValidateFault(cursor); break;
            case 18: ValidateMeterFailure(cursor); break;
            case 19: ValidateResponseFailure(cursor); break;
            case 23: _ = cursor.Take(76); _ = cursor.Take(76); break;
            default: throw new InvalidDataException();
        }
    }

    private static void ValidateEntrypointFailure(TerminalCursor cursor, int tag)
    {
        switch (tag)
        {
            case 1: _ = cursor.U64(); _ = cursor.U64(); break;
            case 2: case 3: case 4: break;
            case 5: case 6: _ = cursor.I32(); break;
            case 7: ValidateFault(cursor); break;
            case 8: ValidateMeterFailure(cursor); break;
            default: throw new InvalidDataException();
        }
    }

    private static void ValidateAbiFailure(TerminalCursor cursor, int tag)
    {
        if (tag is >= 1 and <= 10 or >= 13 and <= 15) return;
        if (tag == 11) { if (cursor.U8() is < 1 or > 11) throw new InvalidDataException(); }
        else if (tag == 12) ValidateMeterFailure(cursor); else throw new InvalidDataException();
    }

    private static void ValidateMeterFailure(TerminalCursor cursor)
    {
        var tag = cursor.U8();
        if (tag == 1)
        {
            var resource = cursor.U8(); var limit = cursor.U64(); var attempted = cursor.U64();
            if (resource is < 1 or > 7 || attempted <= limit) throw new InvalidDataException();
        }
        else if (tag == 2) { if (cursor.U8() is < 1 or > 7) throw new InvalidDataException(); }
        else if (tag != 3) throw new InvalidDataException();
    }

    private static void ValidateFault(TerminalCursor cursor)
    {
        var tag = cursor.U8();
        if (tag is 1 or 2 or 16)
        {
            var name = cursor.Sized32(); if (!Encoding.UTF8.GetBytes(Encoding.UTF8.GetString(name)).SequenceEqual(name))
                throw new InvalidDataException();
        }
        else if (tag is >= 3 and <= 13 or 15) return;
        else if (tag == 14) ValidateMeterFailure(cursor); else throw new InvalidDataException();
    }

    private static void ValidateResponseFailure(TerminalCursor cursor)
    {
        switch (cursor.U8())
        {
            case 1: case 2: _ = cursor.U64(); _ = cursor.U64(); break;
            case 3: case 4: break;
            case 5: _ = cursor.I32(); _ = cursor.I32(); break;
            case 6: ValidateMeterFailure(cursor); break;
            default: throw new InvalidDataException();
        }
    }

    private static void ValidateCandidateResource(TerminalCursor cursor, TerminalUsage usage)
    {
        var tag = cursor.U8(); var resource = cursor.U8(); if (resource > 6) throw new InvalidDataException();
        if (tag == 0)
        {
            var limit = cursor.U64(); var attempted = cursor.U64();
            if (attempted <= limit || CandidateUsage(usage, resource) > limit) throw new InvalidDataException();
        }
        else if (tag != 1) throw new InvalidDataException();
    }

    private static ulong CandidateUsage(TerminalUsage usage, int resource) => resource switch
    {
        0 => usage.Cpu, 1 => usage.Memory, 2 => usage.Read, 3 => usage.Write,
        4 => usage.Values, 5 => usage.OutputBytes, 6 => 0, _ => throw new InvalidDataException(),
    };

    private static void ValidateLegacyResource(TerminalCursor cursor)
    {
        var tag = cursor.U8(); if (cursor.U8() is < 1 or > 7) throw new InvalidDataException();
        if (tag == 1) { var limit = cursor.U64(); if (cursor.U64() <= limit) throw new InvalidDataException(); }
        else if (tag != 2) throw new InvalidDataException();
    }

    private static void RequireNonzero(byte[] value)
    {
        if (value.All(item => item == 0)) throw new InvalidDataException();
    }

    private static ActivityBinding DecodeSignedCall(ProgramCall call)
    {
        try
        {
            var signed = call.SignedActivity; var cursor = new BinaryCursor(signed);
            var envelopeVersion = cursor.U16();
            if (envelopeVersion is not (1 or 2 or 3) || cursor.U16() != 0x1001 || cursor.U8() != 12) throw new InvalidDataException();
            cursor.Tag(1); var protocol = cursor.U16(); if (protocol != envelopeVersion) throw new InvalidDataException();
            cursor.Tag(2); _ = cursor.U32(); cursor.Tag(3);
            if (cursor.U32() != ((uint)ReceiptModuleId << 16 | CallOperation)) throw new InvalidDataException();
            cursor.Tag(4); _ = cursor.Bounded(255, true); cursor.Tag(5); _ = cursor.Bounded(524_288, true);
            cursor.Tag(6); _ = cursor.U64(); cursor.Tag(7); var notBefore = cursor.U64(); var notAfter = cursor.U64();
            if (notAfter < notBefore) throw new InvalidDataException(); cursor.Tag(8); var idempotency = cursor.Bounded(32, false);
            if (idempotency.Length != 32) throw new InvalidDataException(); cursor.Tag(9); var envelopeFee = new BigInteger(cursor.Take(16), true, true);
            if (call.NativeCall is not null && (protocol != 3 || envelopeFee != call.Budget.FeeLimit.Value)) throw new InvalidDataException();
            cursor.Tag(10); var payloadHash = cursor.Bounded(32, false); if (payloadHash.Length != 32) throw new InvalidDataException();
            cursor.Tag(11); var payload = cursor.Bounded(524_288, true); cursor.Tag(12); _ = cursor.Bounded(128, true); cursor.Finish();
            var expected = CanonicalCallPayload(call);
            if (!Fixed(payload, expected) || !Fixed(payloadHash, Digest(Encoding.UTF8.GetBytes("LXP/v1/payload-hash\0"), payload)))
                throw new InvalidDataException();
            return new(Digest(Encoding.UTF8.GetBytes("LXP/v1/activity-id\0"), signed), idempotency, notBefore, notAfter);
        }
        catch { throw Invalid(); }
    }

    private static byte[] CanonicalCallPayload(ProgramCall call)
    {
        if (call.NativeCall is { } n)
        {
            if (!n.ProgramId.SequenceEqual(call.ProgramId) || !n.Calldata.SequenceEqual(call.Calldata) || n.Resources[0] != call.Budget.Fuel || call.Capabilities.Count != 0) throw Invalid();
            return n.Encode();
        }
        var domain = Encoding.UTF8.GetBytes("LayerX/programs/call/v1\0"); using var stream = new MemoryStream();
        stream.Write(domain); stream.Write(call.ProgramId); var word = new byte[8]; BinaryPrimitives.WriteUInt64BigEndian(word, call.Budget.Fuel);
        stream.Write(word); stream.Write(UInt128Bytes(call.Budget.FeeLimit.Value));
        var count = new byte[2]; BinaryPrimitives.WriteUInt16BigEndian(count, checked((ushort)call.Capabilities.Count)); stream.Write(count);
        foreach (var capability in call.Capabilities) stream.WriteByte(checked((byte)((int)capability + 1)));
        var length = new byte[4]; BinaryPrimitives.WriteUInt32BigEndian(length, checked((uint)call.Calldata.Length)); stream.Write(length);
        stream.Write(call.Calldata); return stream.ToArray();
    }

    private static ProgramSubmission UnknownSubmission(byte[] activity, string key, byte[] retained)
    {
        var value = JsonValue.Object(new Dictionary<string, JsonValue>
        {
            ["state"] = JsonValue.String("unknown"), ["activity_id"] = JsonValue.String(Convert.ToHexString(activity).ToLowerInvariant()),
            ["idempotency_key"] = JsonValue.String(key), ["retained_signed_activity"] =
                JsonValue.String(Convert.ToHexString(retained).ToLowerInvariant()),
        });
        return new(value, "unknown", activity, key, retained, null);
    }

    private ulong NowMilliseconds()
    {
        var milliseconds = _timeProvider.GetUtcNow().ToUnixTimeMilliseconds();
        return milliseconds >= 0 ? checked((ulong)milliseconds) : throw Verify();
    }

    private static IReadOnlyDictionary<string, JsonValue> Map(JsonValue value) => value is JsonValue.ObjectValue map ? map.Value : throw Verify();
    private static void RequireFields(IReadOnlyDictionary<string, JsonValue> map, IEnumerable<string> fields)
    {
        var exact = fields.ToArray(); if (map.Count != exact.Length || exact.Any(field => !map.ContainsKey(field))) throw Decode();
    }
    private static JsonValue Field(IReadOnlyDictionary<string, JsonValue> map, string name) => map.TryGetValue(name, out var value) ? value : throw Decode();
    private static string Text(IReadOnlyDictionary<string, JsonValue> map, string name) => Field(map, name) is JsonValue.StringValue text && !string.IsNullOrEmpty(text.Value) ? text.Value : throw Decode();
    private static long Integer(IReadOnlyDictionary<string, JsonValue> map, string name) => Field(map, name) is JsonValue.IntegerValue integer ? integer.Value : throw Decode();
    private static int Integer32(IReadOnlyDictionary<string, JsonValue> map, string name)
    {
        var value = Integer(map, name); return value is >= int.MinValue and <= int.MaxValue ? (int)value : throw Decode();
    }
    private static ushort UInt16Integer(IReadOnlyDictionary<string, JsonValue> map, string name)
    {
        var value = Integer(map, name); return value is >= 0 and <= ushort.MaxValue ? (ushort)value : throw Decode();
    }
    private static uint UInt32Integer(IReadOnlyDictionary<string, JsonValue> map, string name)
    {
        var value = Integer(map, name); return value is >= 0 and <= uint.MaxValue ? (uint)value : throw Decode();
    }
    private static long DecimalInt64(IReadOnlyDictionary<string, JsonValue> map, string name)
    {
        var value = Text(map, name);
        if (value == "-0" || value.Length > 1 && value[0] == '0' || value.StartsWith("-0", StringComparison.Ordinal) ||
            !long.TryParse(value, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var parsed) ||
            parsed.ToString(CultureInfo.InvariantCulture) != value) throw Decode();
        return parsed;
    }
    private static string Hex(IReadOnlyDictionary<string, JsonValue> map, string name, int bytes, bool exact, bool empty = false)
    {
        var value = Field(map, name) is JsonValue.StringValue text ? text.Value : throw Decode();
        if ((!empty && value.Length == 0) || value.Length % 2 != 0 || (exact ? value.Length != bytes * 2 : value.Length > bytes * 2) ||
            value.Any(character => !(character is >= '0' and <= '9' or >= 'a' and <= 'f'))) throw Verify();
        return value;
    }
    private static byte[] Bytes(IReadOnlyDictionary<string, JsonValue> map, string name, int bytes, bool exact = false,
        bool empty = false) => Convert.FromHexString(Hex(map, name, bytes, exact, empty));
    private static ulong DecimalUInt64(IReadOnlyDictionary<string, JsonValue> map, string name)
    {
        var value = Text(map, name);
        if (string.IsNullOrEmpty(value) || value.Length > 1 && value[0] == '0' || !ulong.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out var parsed)) throw Decode();
        return parsed;
    }
    private static BigInteger DecimalUInt128(IReadOnlyDictionary<string, JsonValue> map, string name)
    {
        var value = Text(map, name);
        if (value.Length > 1 && value[0] == '0' || !BigInteger.TryParse(value, NumberStyles.None,
                CultureInfo.InvariantCulture, out var parsed) || parsed < 0 || parsed >= (BigInteger.One << 128)) throw Decode();
        return parsed;
    }
    private static BigInteger UInt128Big(UInt128Value value) => (new BigInteger(value.High) << 64) + value.Low;
    private static byte[] UInt128Bytes(BigInteger value)
    {
        var raw = value.ToByteArray(true, true); var encoded = new byte[16];
        if (raw.Length > encoded.Length) throw Invalid(); raw.CopyTo(encoded, encoded.Length - raw.Length); return encoded;
    }
    private static bool Fixed(byte[] left, byte[] right) => left.Length == right.Length &&
        CryptographicOperations.FixedTimeEquals(left, right);
    private static byte[] Digest(params byte[][] values)
    {
        using var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
        foreach (var value in values) hash.AppendData(value); return hash.GetHashAndReset();
    }
    private static bool Starts(byte[] value, string prefix) => Starts(value, Encoding.UTF8.GetBytes(prefix));
    private static bool Starts(byte[] value, byte[] prefix) => value.Length >= prefix.Length &&
        Fixed(value.AsSpan(0, prefix.Length).ToArray(), prefix);

    private static JsonValue Encode(ProgramCall call)
    {
        ArgumentNullException.ThrowIfNull(call);
        if (call.NativeCall is { } n)
        {
            return JsonValue.Object(new Dictionary<string, JsonValue> {
                ["payload_encoding"] = JsonValue.String("native-v1"), ["program_id"] = JsonValue.String(Identifier(call.ProgramId)), ["calldata"] = JsonValue.String(Convert.ToHexString(call.Calldata).ToLowerInvariant()),
                ["budget"] = JsonValue.Object(new Dictionary<string, JsonValue> { ["fuel"] = JsonValue.String(call.Budget.Fuel.ToString(CultureInfo.InvariantCulture)), ["fee_limit"] = JsonValue.String(call.Budget.FeeLimit.ToString()) }),
                ["signed_activity"] = JsonValue.String(Convert.ToHexString(call.SignedActivity).ToLowerInvariant()),
                ["native_call"] = JsonValue.Object(new Dictionary<string, JsonValue> { ["guest_abi"] = JsonValue.Integer(n.GuestAbi), ["entrypoint"] = JsonValue.String(n.Entrypoint), ["capabilities_hex"] = JsonValue.String(Convert.ToHexString(n.Capabilities).ToLowerInvariant()), ["access_declaration_hex"] = JsonValue.String(Convert.ToHexString(n.AccessDeclaration).ToLowerInvariant()), ["response_capacity"] = JsonValue.Integer(n.ResponseCapacity), ["resources"] = JsonValue.Array(n.Resources.Select(value => JsonValue.String(value.ToString(CultureInfo.InvariantCulture)))) }) });
        }
        return JsonValue.Object(new Dictionary<string, JsonValue> {
            ["program_id"] = JsonValue.String(Convert.ToHexString(call.ProgramId).ToLowerInvariant()), ["calldata"] = JsonValue.String(Convert.ToHexString(call.Calldata).ToLowerInvariant()),
            ["budget"] = JsonValue.Object(new Dictionary<string, JsonValue> { ["fuel"] = JsonValue.String(call.Budget.Fuel.ToString(CultureInfo.InvariantCulture)), ["fee_limit"] = JsonValue.String(call.Budget.FeeLimit.ToString()) }),
            ["capabilities"] = JsonValue.Array(call.Capabilities.Select(value => JsonValue.String(Capability(value)))),
            ["signed_activity"] = JsonValue.String(Convert.ToHexString(call.SignedActivity).ToLowerInvariant()) });
    }
    private static string Capability(ProgramCapability value) => value switch { ProgramCapability.StorageRead => "storage_read", ProgramCapability.StorageWrite => "storage_write", ProgramCapability.Transfer => "transfer", ProgramCapability.EmitEvent => "emit_event", ProgramCapability.Compose => "compose", _ => throw Invalid() };
    private static string Identifier(byte[] value) => value?.Length == 32 ? Convert.ToHexString(value).ToLowerInvariant() : throw Invalid();
    private static string Level(string value) => value == "sequencer-signed" ? value : throw Invalid();
    private static bool Hex32(string value) => value.Length == 64 && value.All(character => character is >= '0' and <= '9' or >= 'a' and <= 'f');
    private static PlatformSdkException Invalid() => new(SdkErrorCode.InvalidArgument, RetryClass.Never);
    private static PlatformSdkException Decode() => new(SdkErrorCode.DecodeFailure, RetryClass.Never);
    private static PlatformSdkException Verify() => new(SdkErrorCode.VerificationFailure, RetryClass.Never);

    private sealed class BinaryCursor
    {
        private readonly byte[] _bytes; private int _offset;
        internal BinaryCursor(byte[] bytes) => _bytes = bytes;
        internal byte U8() => Take(1)[0]; internal ushort U16() => BinaryPrimitives.ReadUInt16BigEndian(Take(2));
        internal uint U32() => BinaryPrimitives.ReadUInt32BigEndian(Take(4)); internal ulong U64() => BinaryPrimitives.ReadUInt64BigEndian(Take(8));
        internal void Tag(byte expected) { if (U8() != expected) throw new InvalidDataException(); }
        internal byte[] Bounded(int maximum, bool empty)
        {
            var length = U32(); if (length > maximum || !empty && length == 0) throw new InvalidDataException(); return Take((int)length);
        }
        internal byte[] Take(int length)
        {
            if (length < 0 || _offset > _bytes.Length - length) throw new InvalidDataException();
            var value = _bytes.AsSpan(_offset, length).ToArray(); _offset += length; return value;
        }
        internal void Finish() { if (_offset != _bytes.Length) throw new InvalidDataException(); }
    }

    private sealed class TerminalCursor
    {
        private readonly byte[] _bytes; private int _offset;
        internal TerminalCursor(byte[] bytes, int offset)
        {
            if (offset < 0 || offset > bytes.Length) throw new InvalidDataException(); _bytes = bytes; _offset = offset;
        }
        internal byte U8() => Take(1)[0]; internal ushort U16() => BinaryPrimitives.ReadUInt16BigEndian(Take(2));
        internal uint U32() => BinaryPrimitives.ReadUInt32BigEndian(Take(4)); internal ulong U64() => BinaryPrimitives.ReadUInt64BigEndian(Take(8));
        internal int I32() => BinaryPrimitives.ReadInt32BigEndian(Take(4)); internal long I64() => BinaryPrimitives.ReadInt64BigEndian(Take(8));
        internal BigInteger U128() => new(Take(16), true, true);
        internal int Remaining => _bytes.Length - _offset;
        internal byte[] Sized32() { var length = U32(); return length <= int.MaxValue ? Take((int)length) : throw new InvalidDataException(); }
        internal byte[] Sized64() { var length = U64(); return length <= int.MaxValue ? Take((int)length) : throw new InvalidDataException(); }
        internal byte[] Take(int length)
        {
            if (length < 0 || _offset > _bytes.Length - length) throw new InvalidDataException();
            var value = _bytes.AsSpan(_offset, length).ToArray(); _offset += length; return value;
        }
        internal byte[] Rest() => Take(_bytes.Length - _offset);
        internal void Finish() { if (_offset != _bytes.Length) throw new InvalidDataException(); }
    }
}
