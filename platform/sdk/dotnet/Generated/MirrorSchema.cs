namespace LayerX.Sdk;

// Generated from platform/sdk/schema/mirror-v2.kvx.
public static class MirrorSchemaV2 {
    public const ushort Version = 2;
    public const int MaximumArchiveBytes = 67_108_864;
    public const int MaximumSources = 8;
    public const int MaximumJsonDepth = 32;
    public static ReadOnlySpan<byte> ArchiveMagic => "LXMIRROR"u8;
}
public enum MirrorPolicyKind { Exact, OrderedPreference, Agreement }
public enum MirrorErrorCode { Configuration, Unavailable, RateLimited, Missing,
    TargetMismatch, SourceMismatch, Malformed, Bounds, Commitment, Authorization,
    Proof, CheckpointUnavailable, Divergent, InsufficientAgreement, Reorged }
