#nullable enable

namespace LayerX.Sdk;

public sealed record ProgramBudget(ulong Fuel, ProtocolAmount FeeLimit);

public enum ProgramCapability { StorageRead, StorageWrite, Transfer, EmitEvent, Compose }

public sealed record ProgramCall
{
    public byte[] ProgramId { get; } public byte[] Calldata { get; }
    public ProgramBudget Budget { get; } public IReadOnlyList<ProgramCapability> Capabilities { get; } public byte[] SignedActivity { get; }

    public ProgramCall(byte[] programId, byte[] calldata, ProgramBudget budget,
        IEnumerable<ProgramCapability> capabilities, byte[] signedActivity)
    {
        var bounded = capabilities?.ToArray();
        if (programId?.Length != 32 || calldata is null || calldata.Length > 1_048_576 || budget is null || budget.Fuel == 0 || bounded is null || bounded.Length > 5 ||
            bounded.Zip(bounded.Skip(1)).Any(pair => pair.First >= pair.Second) ||
            signedActivity is null || signedActivity.Length == 0 || signedActivity.Length > 1_048_576) throw Invalid();
        ProgramId = programId.ToArray(); Calldata = calldata.ToArray(); Budget = budget; Capabilities = Array.AsReadOnly(bounded);
        SignedActivity = signedActivity.ToArray();
    }
    private static PlatformSdkException Invalid() => new(SdkErrorCode.InvalidArgument, RetryClass.Never);
}

public sealed record ProgramDiscovery(JsonValue Value);
public sealed record ProgramInterface(JsonValue Value);
public sealed record ProgramSimulation(JsonValue Value);
public sealed record ProgramSubmission
{
    public JsonValue Value { get; } public string State { get; } public bool IsUnknown => State == "unknown";
    public ProgramSubmission(JsonValue value)
    {
        if (value is not JsonValue.ObjectValue map || !map.Value.TryGetValue("state", out var raw) ||
            raw is not JsonValue.StringValue state || state.Value is not ("refused" or "unknown" or "executed"))
            throw new PlatformSdkException(SdkErrorCode.VerificationFailure, RetryClass.Never);
        Value = value; State = state.Value;
    }
}

public sealed class ProgramsClient
{
    public const ushort ReceiptModuleId = 9; public const byte CallOperation = 3;
    private readonly PlatformClient _client;
    public ProgramsClient(PlatformClient client) => _client = client ?? throw Invalid();

    public async Task<ProgramDiscovery> DiscoverAsync(byte[] programId, string verificationLevel, CancellationToken cancellationToken = default)
    {
        var id = Identifier(programId); var value = await _client.AgentProgramDiscoverAsync(JsonValue.Object(new Dictionary<string, JsonValue>
            { ["program_id"] = JsonValue.String(id), ["requested_verification_level"] = JsonValue.String(Level(verificationLevel)) }),
            new Dictionary<string, string> { ["program_id"] = id }, cancellationToken).ConfigureAwait(false);
        VerifiedDiscovery(value, id, false); return new(value);
    }
    public async Task<ProgramInterface> InterfaceAsync(byte[] programId, string verificationLevel, CancellationToken cancellationToken = default)
    {
        var id = Identifier(programId);
        var value = await _client.AgentProgramInterfaceAsync(JsonValue.Object(new Dictionary<string, JsonValue>
            { ["program_id"] = JsonValue.String(id), ["requested_verification_level"] = JsonValue.String(Level(verificationLevel)) }),
            new Dictionary<string, string> { ["program_id"] = id }, cancellationToken).ConfigureAwait(false);
        VerifiedDiscovery(value, id, true); return new(value);
    }
    public async Task<ProgramSimulation> SimulateAsync(ProgramCall call, CancellationToken cancellationToken = default)
    {
        var value = await _client.AgentProgramSimulateAsync(Encode(call), cancellationToken: cancellationToken).ConfigureAwait(false);
        await VerifySimulationAsync(value, Identifier(call.ProgramId), cancellationToken).ConfigureAwait(false);
        return new(value);
    }
    public async Task<ProgramSubmission> SubmitAsync(ProgramCall call, IdempotencyKey idempotencyKey, CancellationToken cancellationToken = default)
    {
        if (!Hex32(idempotencyKey.Value)) throw Invalid();
        var value = await _client.AgentProgramCallAsync(Encode(call), idempotencyKey, cancellationToken: cancellationToken).ConfigureAwait(false);
        return await VerifiedSubmissionAsync(value, Identifier(call.ProgramId), null, idempotencyKey.Value,
            Convert.ToHexString(call.SignedActivity).ToLowerInvariant(), cancellationToken).ConfigureAwait(false);
    }
    public async Task<ProgramSubmission> ReceiptAsync(IdempotencyKey idempotencyKey, byte[] expectedActivityId, string verificationLevel, CancellationToken cancellationToken = default)
    {
        var activity = Identifier(expectedActivityId);
        var value = await _client.AgentProgramReceiptAsync(JsonValue.Object(new Dictionary<string, JsonValue>
            { ["idempotency_key"] = JsonValue.String(idempotencyKey.Value), ["expected_activity_id"] = JsonValue.String(activity),
              ["requested_verification_level"] = JsonValue.String(Level(verificationLevel)) }),
            new Dictionary<string, string> { ["idempotency_key"] = idempotencyKey.Value }, cancellationToken).ConfigureAwait(false);
        return await VerifiedSubmissionAsync(value, null, activity, idempotencyKey.Value, null, cancellationToken).ConfigureAwait(false);
    }
    public async Task<ProgramSubmission> ActivityAsync(byte[] activityId, string verificationLevel, CancellationToken cancellationToken = default)
    {
        var id = Identifier(activityId);
        var value = await _client.AgentProgramActivityAsync(JsonValue.Object(new Dictionary<string, JsonValue>
            { ["activity_id"] = JsonValue.String(id), ["requested_verification_level"] = JsonValue.String(Level(verificationLevel)) }),
            new Dictionary<string, string> { ["activity_id"] = id }, cancellationToken).ConfigureAwait(false);
        return await VerifiedSubmissionAsync(value, null, id, null, null, cancellationToken).ConfigureAwait(false);
    }

    public static async ValueTask<ReceiptVerification> VerifyReceiptAsync(byte[] canonicalReceipt, AuthorizedReceiptBatch authorized,
        byte[] expectedActivityId, ushort expectedGuestAbiVersion, byte[] terminalPayload, byte[] callGraph,
        CancellationToken cancellationToken = default)
    {
        if (expectedActivityId?.Length != 32 || expectedGuestAbiVersion is not (1 or 2)) throw Invalid();
        var verified = await LocalVerifier.VerifyReceiptOutcomeAsync(canonicalReceipt, authorized, cancellationToken).ConfigureAwait(false);
        var receipt = verified.Receipt;
        var outcome = receipt.ProgramOutcome;
        if (receipt.ProtocolVersion == 0 || receipt.ModuleId != ReceiptModuleId || receipt.Operation != CallOperation ||
            receipt.ModuleVersion is < 1 or > 3 || !receipt.ActivityId.SequenceEqual(expectedActivityId) ||
            outcome is null || outcome.AbiVersion != expectedGuestAbiVersion || terminalPayload is null ||
            callGraph is null || callGraph.Length == 0 ||
            !System.Security.Cryptography.CryptographicOperations.FixedTimeEquals(System.Security.Cryptography.SHA256.HashData(terminalPayload), outcome.TerminalPayloadRoot) ||
            !System.Security.Cryptography.CryptographicOperations.FixedTimeEquals(System.Security.Cryptography.SHA256.HashData(callGraph), outcome.CallGraphRoot))
            throw new PlatformSdkException(SdkErrorCode.VerificationFailure, RetryClass.Never);
        return verified;
    }

    private static async Task<ProgramSubmission> VerifiedSubmissionAsync(JsonValue value, string? expectedProgramId,
        string? expectedActivityId, string? expectedIdempotencyKey, string? retainedSignedActivity,
        CancellationToken cancellationToken)
    {
        var map = Map(value); var state = Text(map, "state");
        if (state == "unknown")
        {
            var activity = Hex(map, "activity_id", 32, true);
            var idempotency = Hex(map, "idempotency_key", 32, true);
            var retained = Hex(map, "retained_signed_activity", 1_048_576, false);
            if ((expectedActivityId is not null && activity != expectedActivityId) ||
                (expectedIdempotencyKey is not null && idempotency != expectedIdempotencyKey) ||
                (retainedSignedActivity is not null && retained != retainedSignedActivity)) throw Verify();
            return new(value);
        }
        if (state is not ("executed" or "refused")) throw Verify();
        await VerifyExecutionAsync(map, state, expectedProgramId, expectedActivityId, expectedIdempotencyKey, cancellationToken).ConfigureAwait(false);
        return new(value);
    }

    private static async Task VerifyExecutionAsync(IReadOnlyDictionary<string, JsonValue> map, string state,
        string? expectedProgramId, string? expectedActivityId, string? expectedIdempotencyKey,
        CancellationToken cancellationToken)
    {
        var activity = Hex(map, "activity_id", 32, true); var program = Hex(map, "program_id", 32, true);
        if ((expectedProgramId is not null && program != expectedProgramId) ||
            (expectedActivityId is not null && activity != expectedActivityId) ||
            (expectedIdempotencyKey is not null && Text(map, "idempotency_key") != expectedIdempotencyKey)) throw Verify();
        var guestAbi = Integer(map, "guest_abi_version");
        if (guestAbi is not (1 or 2)) throw Verify();
        var outcome = Map(Field(map, "outcome"));
        if ((state == "refused") != (Text(outcome, "kind") == "refused")) throw Verify();
        var authority = Authority(Field(map, "authority"));
        _ = await VerifyReceiptAsync(Bytes(map, "receipt", 1_048_576), authority, Convert.FromHexString(activity),
            checked((ushort)guestAbi), Bytes(map, "terminal_payload", 1_048_576), Bytes(map, "call_graph", 1_048_576), cancellationToken).ConfigureAwait(false);
    }

    private static async Task VerifySimulationAsync(JsonValue value, string expectedProgramId, CancellationToken cancellationToken)
    {
        var map = Map(value);
        if (Field(map, "committed") is not JsonValue.BooleanValue { Value: false }) throw Verify();
        var execution = Map(Field(map, "execution"));
        if (Text(execution, "state") != "simulated") throw Verify();
        await VerifyExecutionAsync(execution, "simulated", expectedProgramId, null, null, cancellationToken).ConfigureAwait(false);
        var evidence = Map(Field(map, "simulation_evidence"));
        if (Field(evidence, "committed") is not JsonValue.BooleanValue { Value: false }) throw Verify();
        var boundary = Bytes(evidence, "boundary_id", 32, true); var publicKey = Bytes(evidence, "public_key", 32, true);
        var boundaryMaterial = System.Text.Encoding.UTF8.GetBytes("LayerX/emulator/simulation-boundary/v1\0").Concat(publicKey).ToArray();
        if (!System.Security.Cryptography.CryptographicOperations.FixedTimeEquals(System.Security.Cryptography.SHA256.HashData(boundaryMaterial), boundary)) throw Verify();
        var activity = Hex(evidence, "activity_id", 32, true); var previous = Bytes(evidence, "previous_state_root", 32, true);
        var hypothetical = Hex(evidence, "hypothetical_state_root", 32, true);
        if (activity != Text(execution, "activity_id") || hypothetical != Text(execution, "state_root")) throw Verify();
        var sequence = DecimalUInt64(evidence, "observed_sequence"); var observedAt = DecimalUInt64(evidence, "observed_at");
        var signed = new List<byte>(256); signed.AddRange(System.Text.Encoding.UTF8.GetBytes("LayerX/agent/program-simulation-evidence/v1\0"));
        signed.AddRange(boundary); signed.AddRange(Convert.FromHexString(activity)); signed.AddRange(previous); signed.AddRange(Convert.FromHexString(hypothetical));
        var word = new byte[8]; System.Buffers.Binary.BinaryPrimitives.WriteUInt64BigEndian(word, sequence); signed.AddRange(word);
        System.Buffers.Binary.BinaryPrimitives.WriteUInt64BigEndian(word, observedAt); signed.AddRange(word); signed.Add(0);
        var digest = System.Security.Cryptography.SHA256.HashData(signed.ToArray()); var signature = Bytes(evidence, "signature", 64, true);
        if (!LocalVerifier.VerifyEd25519Digest(publicKey, signature, digest)) throw Verify();
    }

    private static void VerifiedDiscovery(JsonValue value, string programId, bool @interface)
    {
        var map = Map(value);
        if (Text(map, "program_id") != programId || Text(map, "verification") != (@interface ? "deployment-interface-and-current-head-verified" : "registry-receipt-and-current-head-verified")) throw Verify();
        _ = DecimalUInt64(map, "observed_sequence"); _ = DecimalUInt64(map, "observed_at"); _ = DecimalUInt64(map, "valid_through");
    }

    private static AuthorizedReceiptBatch Authority(JsonValue value)
    {
        var map = Map(value); return new(Bytes(map, "batch_id", 32, true), Bytes(map, "asset", 32, true),
            Bytes(map, "previous_state_root", 32, true), Bytes(map, "resulting_state_root", 32, true),
            Bytes(map, "sequencer_public_key", 32, true));
    }

    private static IReadOnlyDictionary<string, JsonValue> Map(JsonValue value) => value is JsonValue.ObjectValue map ? map.Value : throw Verify();
    private static JsonValue Field(IReadOnlyDictionary<string, JsonValue> map, string name) => map.TryGetValue(name, out var value) ? value : throw Verify();
    private static string Text(IReadOnlyDictionary<string, JsonValue> map, string name) => Field(map, name) is JsonValue.StringValue text ? text.Value : throw Verify();
    private static long Integer(IReadOnlyDictionary<string, JsonValue> map, string name) => Field(map, name) is JsonValue.IntegerValue integer ? integer.Value : throw Verify();
    private static string Hex(IReadOnlyDictionary<string, JsonValue> map, string name, int bytes, bool exact)
    {
        var value = Text(map, name);
        if (value.Length % 2 != 0 || (exact ? value.Length != bytes * 2 : value.Length > bytes * 2) ||
            value.Any(character => !(character is >= '0' and <= '9' or >= 'a' and <= 'f'))) throw Verify();
        return value;
    }
    private static byte[] Bytes(IReadOnlyDictionary<string, JsonValue> map, string name, int bytes, bool exact = false) => Convert.FromHexString(Hex(map, name, bytes, exact));
    private static ulong DecimalUInt64(IReadOnlyDictionary<string, JsonValue> map, string name)
    {
        var value = Text(map, name);
        if (string.IsNullOrEmpty(value) || value.Length > 1 && value[0] == '0' || !ulong.TryParse(value, System.Globalization.NumberStyles.None, System.Globalization.CultureInfo.InvariantCulture, out var parsed)) throw Verify();
        return parsed;
    }

    private static JsonValue Encode(ProgramCall call)
    {
        ArgumentNullException.ThrowIfNull(call);
        return JsonValue.Object(new Dictionary<string, JsonValue> {
            ["program_id"] = JsonValue.String(Convert.ToHexString(call.ProgramId).ToLowerInvariant()), ["calldata"] = JsonValue.String(Convert.ToHexString(call.Calldata).ToLowerInvariant()),
            ["budget"] = JsonValue.Object(new Dictionary<string, JsonValue> { ["fuel"] = JsonValue.String(call.Budget.Fuel.ToString(System.Globalization.CultureInfo.InvariantCulture)), ["fee_limit"] = JsonValue.String(call.Budget.FeeLimit.ToString()) }),
            ["capabilities"] = JsonValue.Array(call.Capabilities.Select(value => JsonValue.String(Capability(value)))),
            ["signed_activity"] = JsonValue.String(Convert.ToHexString(call.SignedActivity).ToLowerInvariant()) });
    }
    private static string Capability(ProgramCapability value) => value switch { ProgramCapability.StorageRead => "storage_read", ProgramCapability.StorageWrite => "storage_write", ProgramCapability.Transfer => "transfer", ProgramCapability.EmitEvent => "emit_event", ProgramCapability.Compose => "compose", _ => throw Invalid() };
    private static string Identifier(byte[] value) => value?.Length == 32 ? Convert.ToHexString(value).ToLowerInvariant() : throw Invalid();
    private static string Level(string value) => value == "sequencer-signed" ? value : throw Invalid();
    private static bool Hex32(string value) => value.Length == 64 && value.All(character => character is >= '0' and <= '9' or >= 'a' and <= 'f');
    private static PlatformSdkException Invalid() => new(SdkErrorCode.InvalidArgument, RetryClass.Never);
    private static PlatformSdkException Verify() => new(SdkErrorCode.VerificationFailure, RetryClass.Never);
}
