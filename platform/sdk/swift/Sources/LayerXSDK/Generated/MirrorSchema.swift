// Generated from platform/sdk/schema/mirror-v2.kvx.
import Foundation
public enum MirrorSchemaV2 {
    public static let version: UInt16 = 2
    public static let archiveMagic = Data([0x4c,0x58,0x4d,0x49,0x52,0x52,0x4f,0x52])
    public static let maximumArchiveBytes = 67_108_864
    public static let maximumSources = 8
    public static let maximumJSONDepth = 32
}

public enum MirrorPolicyKind: String, Sendable { case exact, orderedPreference, agreement }
public enum MirrorErrorCode: String, Error, Sendable {
    case configuration, unavailable, missing, malformed, bounds, commitment, authorization, proof, divergent, reorged
    case rateLimited = "rate-limited"
    case targetMismatch = "target-mismatch"
    case sourceMismatch = "source-mismatch"
    case checkpointUnavailable = "checkpoint-unavailable"
    case insufficientAgreement = "insufficient-agreement"
}
