// Code generated from platform/sdk/schema/mirror-v2.kvx; DO NOT EDIT.
package layerx

const (
	MirrorSchemaVersion   uint16 = 2
	MirrorMaxArchiveBytes        = 67_108_864
	MirrorMaxSources             = 8
	MirrorMaxJSONDepth           = 32
)

type MirrorPolicyKind string

const (
	MirrorExact             MirrorPolicyKind = "exact"
	MirrorOrderedPreference MirrorPolicyKind = "ordered-preference"
	MirrorAgreement         MirrorPolicyKind = "agreement"
)

type MirrorErrorCode string

const (
	MirrorConfiguration         MirrorErrorCode = "configuration"
	MirrorUnavailable           MirrorErrorCode = "unavailable"
	MirrorRateLimited           MirrorErrorCode = "rate-limited"
	MirrorMissing               MirrorErrorCode = "missing"
	MirrorTargetMismatch        MirrorErrorCode = "target-mismatch"
	MirrorSourceMismatch        MirrorErrorCode = "source-mismatch"
	MirrorMalformed             MirrorErrorCode = "malformed"
	MirrorBounds                MirrorErrorCode = "bounds"
	MirrorCommitment            MirrorErrorCode = "commitment"
	MirrorAuthorization         MirrorErrorCode = "authorization"
	MirrorProof                 MirrorErrorCode = "proof"
	MirrorCheckpointUnavailable MirrorErrorCode = "checkpoint-unavailable"
	MirrorDivergent             MirrorErrorCode = "divergent"
	MirrorInsufficientAgreement MirrorErrorCode = "insufficient-agreement"
	MirrorReorged               MirrorErrorCode = "reorged"
)
