// Code generated from platform/sdk/generators/receipt.kvx. DO NOT EDIT.
#nullable enable

namespace LayerX.Sdk;

public enum ReceiptCheck
{
    Decode,
    CanonicalEncoding,
    ReceiptShape,
    MissingSignature,
    ProtocolVersion,
    ResultCode,
    Operation,
    ActivityId,
    GlobalSequence,
    ModuleId,
    ModuleVersion,
    Timestamp,
    BatchId,
    Asset,
    PreviousStateRoot,
    ResultingStateRoot,
    DebitBalance,
    CreditBalance,
    ProgramOutcome,
    SequencerSignature,
}

public static class GeneratedReceiptContract
{
    public const ushort ProgramsModuleId = 9;
    public const uint ProgramOutcomeV1 = 0x50524731;
    public const uint ProgramOutcomeV2 = 0x50524732;
    public const uint ProgramOutcomeV3 = 0x50524733;
    public static readonly ReceiptCheck[] RequiredNonzeroChecks = [
        ReceiptCheck.GlobalSequence,
        ReceiptCheck.ModuleId,
        ReceiptCheck.ModuleVersion,
        ReceiptCheck.Timestamp,
        ReceiptCheck.ActivityId,
        ReceiptCheck.ResultingStateRoot,
    ];

    public static string MachineCode(this ReceiptCheck check) => check switch
    {
        ReceiptCheck.Decode => "decode",
        ReceiptCheck.CanonicalEncoding => "canonical-encoding",
        ReceiptCheck.ReceiptShape => "receipt-shape",
        ReceiptCheck.MissingSignature => "missing-signature",
        ReceiptCheck.ProtocolVersion => "protocol-version",
        ReceiptCheck.ResultCode => "result-code",
        ReceiptCheck.Operation => "operation",
        ReceiptCheck.ActivityId => "activity-id",
        ReceiptCheck.GlobalSequence => "global-sequence",
        ReceiptCheck.ModuleId => "module-id",
        ReceiptCheck.ModuleVersion => "module-version",
        ReceiptCheck.Timestamp => "timestamp",
        ReceiptCheck.BatchId => "batch-id",
        ReceiptCheck.Asset => "asset",
        ReceiptCheck.PreviousStateRoot => "previous-state-root",
        ReceiptCheck.ResultingStateRoot => "resulting-state-root",
        ReceiptCheck.DebitBalance => "debit-balance",
        ReceiptCheck.CreditBalance => "credit-balance",
        ReceiptCheck.ProgramOutcome => "program-outcome",
        ReceiptCheck.SequencerSignature => "sequencer-signature",
        _ => throw new ArgumentOutOfRangeException(nameof(check)),
    };
}
