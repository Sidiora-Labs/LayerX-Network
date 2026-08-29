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
            signedActivity is null || signedActivity.Length == 0) throw Invalid();
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
        var id = Identifier(programId); return new(await _client.AgentProgramDiscoverAsync(JsonValue.Object(new Dictionary<string, JsonValue>
            { ["program_id"] = JsonValue.String(id), ["requested_verification_level"] = JsonValue.String(Level(verificationLevel)) }),
            new Dictionary<string, string> { ["program_id"] = id }, cancellationToken).ConfigureAwait(false));
    }
    public async Task<ProgramInterface> InterfaceAsync(byte[] programId, string verificationLevel, CancellationToken cancellationToken = default)
    {
        var id = Identifier(programId);
        return new(await _client.AgentProgramInterfaceAsync(JsonValue.Object(new Dictionary<string, JsonValue>
            { ["program_id"] = JsonValue.String(id), ["requested_verification_level"] = JsonValue.String(Level(verificationLevel)) }),
            new Dictionary<string, string> { ["program_id"] = id }, cancellationToken).ConfigureAwait(false));
    }
    public async Task<ProgramSimulation> SimulateAsync(ProgramCall call, CancellationToken cancellationToken = default) =>
        new(await _client.AgentProgramSimulateAsync(Encode(call), cancellationToken: cancellationToken).ConfigureAwait(false));
    public async Task<ProgramSubmission> SubmitAsync(ProgramCall call, IdempotencyKey idempotencyKey, CancellationToken cancellationToken = default) =>
        new(await _client.AgentProgramCallAsync(Encode(call), idempotencyKey, cancellationToken: cancellationToken).ConfigureAwait(false));
    public async Task<ProgramSubmission> ReceiptAsync(IdempotencyKey idempotencyKey, byte[] expectedActivityId, string verificationLevel, CancellationToken cancellationToken = default)
    {
        var activity = Identifier(expectedActivityId);
        return new(await _client.AgentProgramReceiptAsync(JsonValue.Object(new Dictionary<string, JsonValue>
            { ["idempotency_key"] = JsonValue.String(idempotencyKey.Value), ["expected_activity_id"] = JsonValue.String(activity),
              ["requested_verification_level"] = JsonValue.String(Level(verificationLevel)) }),
            new Dictionary<string, string> { ["idempotency_key"] = idempotencyKey.Value }, cancellationToken).ConfigureAwait(false));
    }
    public async Task<ProgramSubmission> ActivityAsync(byte[] activityId, string verificationLevel, CancellationToken cancellationToken = default)
    {
        var id = Identifier(activityId);
        return new(await _client.AgentProgramActivityAsync(JsonValue.Object(new Dictionary<string, JsonValue>
            { ["activity_id"] = JsonValue.String(id), ["requested_verification_level"] = JsonValue.String(Level(verificationLevel)) }),
            new Dictionary<string, string> { ["activity_id"] = id }, cancellationToken).ConfigureAwait(false));
    }

    public static async ValueTask<ReceiptVerification> VerifyReceiptAsync(byte[] canonicalReceipt, AuthorizedReceiptBatch authorized,
        byte[] expectedActivityId, CancellationToken cancellationToken = default)
    {
        if (expectedActivityId?.Length != 32) throw Invalid();
        var verified = await LocalVerifier.VerifyReceiptOutcomeAsync(canonicalReceipt, authorized, cancellationToken).ConfigureAwait(false);
        var receipt = verified.Receipt;
        if (receipt.ProtocolVersion == 0 || receipt.ModuleId != ReceiptModuleId || receipt.Operation != CallOperation ||
            receipt.ModuleVersion is < 1 or > 3 || !receipt.ActivityId.SequenceEqual(expectedActivityId))
            throw new PlatformSdkException(SdkErrorCode.VerificationFailure, RetryClass.Never);
        return verified;
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
    private static string Level(string value) => !string.IsNullOrEmpty(value) && System.Text.Encoding.UTF8.GetByteCount(value) <= 64 ? value : throw Invalid();
    private static PlatformSdkException Invalid() => new(SdkErrorCode.InvalidArgument, RetryClass.Never);
}
