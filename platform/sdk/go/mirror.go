package layerx

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"os/exec"
	"path/filepath"
	"strconv"
	"time"
)

type MirrorCandidate struct {
	Source     int      `json:"source"`
	Commitment [32]byte `json:"-"`
}
type MirrorPolicy struct {
	Kind       MirrorPolicyKind
	Candidates []MirrorCandidate
	Minimum    int
}
type MirrorVerification struct {
	Level             string
	BatchNumber       uint64
	HeaderDigest      [32]byte
	EvidenceDigest    [32]byte
	SourceID          string
	Target            string
	CanonicalPosition string
	Provenance        string
	LatestBatch       *uint64
	BatchLag          string
	FailoverCount     int
	AgreeingSources   int
	CheckpointLevel   string
}
type MirrorVerificationError struct{ Code MirrorErrorCode }

func (e MirrorVerificationError) Error() string {
	return "mirror verification refused: " + string(e.Code)
}

type MirrorVerifier struct {
	executable    string
	configuration string
	timeout       time.Duration
}

func NewMirrorVerifier(executable, configuration string, timeout time.Duration) (*MirrorVerifier, error) {
	if !filepath.IsAbs(executable) || !filepath.IsAbs(configuration) || timeout < 100*time.Millisecond || timeout > 120*time.Second {
		return nil, MirrorVerificationError{MirrorConfiguration}
	}
	return &MirrorVerifier{executable, configuration, timeout}, nil
}
func (v *MirrorVerifier) VerifyReceipt(ctx context.Context, batch uint64, policy MirrorPolicy, receipt []byte) (MirrorVerification, error) {
	return v.verify(ctx, batch, policy, map[string]any{"kind": "receipt", "canonical_hex": hex.EncodeToString(receipt)})
}
func (v *MirrorVerifier) VerifyState(ctx context.Context, batch uint64, policy MirrorPolicy, state, proof []byte) (MirrorVerification, error) {
	return v.verify(ctx, batch, policy, map[string]any{"kind": "state", "canonical_hex": hex.EncodeToString(state), "proof_hex": hex.EncodeToString(proof)})
}
func (v *MirrorVerifier) verify(parent context.Context, batch uint64, policy MirrorPolicy, evidence map[string]any) (MirrorVerification, error) {
	var zero MirrorVerification
	if batch == 0 || len(policy.Candidates) == 0 || len(policy.Candidates) > MirrorMaxSources {
		return zero, MirrorVerificationError{MirrorConfiguration}
	}
	candidates := make([]map[string]any, 0, len(policy.Candidates))
	seen := map[int]struct{}{}
	for _, candidate := range policy.Candidates {
		if candidate.Source < 0 {
			return zero, MirrorVerificationError{MirrorConfiguration}
		}
		if _, ok := seen[candidate.Source]; ok {
			return zero, MirrorVerificationError{MirrorConfiguration}
		}
		seen[candidate.Source] = struct{}{}
		candidates = append(candidates, map[string]any{"source": candidate.Source, "commitment_hex": hex.EncodeToString(candidate.Commitment[:])})
	}
	var encodedPolicy map[string]any
	switch policy.Kind {
	case MirrorExact:
		if len(candidates) != 1 {
			return zero, MirrorVerificationError{MirrorConfiguration}
		}
		encodedPolicy = map[string]any{"kind": "exact", "candidate": candidates[0]}
	case MirrorOrderedPreference:
		encodedPolicy = map[string]any{"kind": "ordered-preference", "candidates": candidates}
	case MirrorAgreement:
		if policy.Minimum < 1 || policy.Minimum > len(candidates) {
			return zero, MirrorVerificationError{MirrorConfiguration}
		}
		encodedPolicy = map[string]any{"kind": "agreement", "candidates": candidates, "minimum": policy.Minimum}
	default:
		return zero, MirrorVerificationError{MirrorConfiguration}
	}
	request, err := json.Marshal(map[string]any{"batch_number": strconv.FormatUint(batch, 10), "evidence": evidence, "policy": encodedPolicy})
	if err != nil || len(request) > 40*1024*1024 {
		return zero, MirrorVerificationError{MirrorBounds}
	}
	ctx, cancel := context.WithTimeout(parent, v.timeout)
	defer cancel()
	command := exec.CommandContext(ctx, v.executable, v.configuration)
	command.Stdin = bytes.NewReader(request)
	var stdout bytes.Buffer
	stdout.Grow(4096)
	command.Stdout = &stdout
	if err := command.Run(); err != nil || ctx.Err() != nil {
		return zero, MirrorVerificationError{MirrorUnavailable}
	}
	if stdout.Len() > 1_048_576 {
		return zero, MirrorVerificationError{MirrorBounds}
	}
	var response struct {
		Ok           bool   `json:"ok"`
		Error        string `json:"error"`
		Verification struct {
			Level             string  `json:"level"`
			BatchNumber       string  `json:"batchNumber"`
			HeaderDigest      string  `json:"headerDigest"`
			EvidenceDigest    string  `json:"evidenceDigest"`
			SourceID          string  `json:"sourceId"`
			Target            string  `json:"target"`
			CanonicalPosition string  `json:"canonicalPosition"`
			Provenance        string  `json:"provenance"`
			LatestBatch       *string `json:"latestBatch"`
			BatchLag          string  `json:"batchLag"`
			FailoverCount     int     `json:"failoverCount"`
			AgreeingSources   int     `json:"agreeingSources"`
			CheckpointLevel   string  `json:"checkpointLevel"`
		} `json:"verification"`
	}
	if err := json.Unmarshal(stdout.Bytes(), &response); err != nil {
		return zero, MirrorVerificationError{MirrorMalformed}
	}
	if !response.Ok {
		return zero, MirrorVerificationError{MirrorErrorCode(response.Error)}
	}
	header, err := fixedDigest(response.Verification.HeaderDigest)
	if err != nil {
		return zero, err
	}
	evidenceDigest, err := fixedDigest(response.Verification.EvidenceDigest)
	if err != nil {
		return zero, err
	}
	verifiedBatch, err := canonicalUint64(response.Verification.BatchNumber)
	if err != nil {
		return zero, err
	}
	var latest *uint64
	if response.Verification.LatestBatch != nil {
		value, err := canonicalUint64(*response.Verification.LatestBatch)
		if err != nil {
			return zero, err
		}
		latest = &value
	}
	return MirrorVerification{response.Verification.Level, verifiedBatch, header, evidenceDigest, response.Verification.SourceID, response.Verification.Target, response.Verification.CanonicalPosition, response.Verification.Provenance, latest, response.Verification.BatchLag, response.Verification.FailoverCount, response.Verification.AgreeingSources, response.Verification.CheckpointLevel}, nil
}

func canonicalUint64(value string) (uint64, error) {
	if value == "" || value == "0" || value[0] == '0' {
		return 0, MirrorVerificationError{MirrorMalformed}
	}
	result, err := strconv.ParseUint(value, 10, 64)
	if err != nil || strconv.FormatUint(result, 10) != value {
		return 0, MirrorVerificationError{MirrorMalformed}
	}
	return result, nil
}

func fixedDigest(value string) ([32]byte, error) {
	var out [32]byte
	decoded, err := hex.DecodeString(value)
	if err != nil || len(decoded) != 32 {
		return out, MirrorVerificationError{MirrorMalformed}
	}
	copy(out[:], decoded)
	return out, nil
}
