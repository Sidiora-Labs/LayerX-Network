package layerx

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
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
	executableDigest [32]byte
	configurationDigest [32]byte
	timeout       time.Duration
}

const (
	maxMirrorRequestBytes  = 40 * 1024 * 1024
	maxMirrorResponseBytes = 1024 * 1024
	maxMirrorEvidenceBytes = (maxMirrorRequestBytes - 64*1024) / 2
	maxMirrorExecutableBytes = 512 * 1024 * 1024
	maxMirrorConfigurationBytes = 16 * 1024 * 1024
)

type boundedMirrorOutput struct {
	bytes.Buffer
	exceeded bool
}

func (output *boundedMirrorOutput) Write(value []byte) (int, error) {
	length := len(value)
	remaining := maxMirrorResponseBytes - output.Len()
	if remaining > 0 {
		if remaining > length {
			remaining = length
		}
		_, _ = output.Buffer.Write(value[:remaining])
	}
	if length > remaining {
		output.exceeded = true
	}
	return length, nil
}

func NewMirrorVerifier(executable, configuration string, timeout time.Duration) (*MirrorVerifier, error) {
	if timeout < 100*time.Millisecond || timeout > 120*time.Second {
		return nil, MirrorVerificationError{MirrorConfiguration}
	}
	executablePath, executableDigest, err := trustedMirrorFile(executable, true, maxMirrorExecutableBytes)
	if err != nil {
		return nil, MirrorVerificationError{MirrorConfiguration}
	}
	configurationPath, configurationDigest, err := trustedMirrorFile(configuration, false, maxMirrorConfigurationBytes)
	if err != nil {
		return nil, MirrorVerificationError{MirrorConfiguration}
	}
	return &MirrorVerifier{executablePath, configurationPath, executableDigest, configurationDigest, timeout}, nil
}

func trustedMirrorFile(path string, executable bool, maximum int64) (string, [32]byte, error) {
	var zero [32]byte
	if !filepath.IsAbs(path) || filepath.Clean(path) != path {
		return "", zero, MirrorVerificationError{MirrorConfiguration}
	}
	real, err := filepath.EvalSymlinks(path)
	if err != nil || real != path {
		return "", zero, MirrorVerificationError{MirrorConfiguration}
	}
	volume := filepath.VolumeName(path)
	current := volume + string(filepath.Separator)
	relative := path[len(current):]
	for _, component := range splitMirrorPath(relative) {
		current = filepath.Join(current, component)
		info, inspectErr := os.Lstat(current)
		if inspectErr != nil || info.Mode()&os.ModeSymlink != 0 || info.Mode().Perm()&0o022 != 0 || !mirrorOwnerProtected(info) {
			return "", zero, MirrorVerificationError{MirrorConfiguration}
		}
	}
	info, err := os.Lstat(path)
	if err != nil || !info.Mode().IsRegular() || !mirrorOwnerProtected(info) || info.Size() < 0 || info.Size() > maximum || (executable && info.Mode().Perm()&0o111 == 0) {
		return "", zero, MirrorVerificationError{MirrorConfiguration}
	}
	file, err := os.Open(path)
	if err != nil {
		return "", zero, MirrorVerificationError{MirrorConfiguration}
	}
	defer file.Close()
	hasher := sha256.New()
	written, err := io.Copy(hasher, io.LimitReader(file, maximum+1))
	if err != nil || written != info.Size() || written > maximum {
		return "", zero, MirrorVerificationError{MirrorConfiguration}
	}
	copy(zero[:], hasher.Sum(nil))
	return path, zero, nil
}

func mirrorOwnerProtected(info os.FileInfo) bool {
	effective := os.Geteuid()
	if effective < 0 {
		return true
	}
	value := reflect.ValueOf(info.Sys())
	if value.Kind() == reflect.Pointer {
		value = value.Elem()
	}
	if !value.IsValid() || value.Kind() != reflect.Struct {
		return false
	}
	owner := value.FieldByName("Uid")
	if !owner.IsValid() || !owner.CanUint() {
		return false
	}
	return owner.Uint() == 0 || owner.Uint() == uint64(effective)
}

func splitMirrorPath(path string) []string {
	components := make([]string, 0, 8)
	for path != "." && path != "" {
		directory, base := filepath.Split(path)
		if base != "" {
			components = append([]string{base}, components...)
		}
		path = filepath.Clean(directory)
		if path == string(filepath.Separator) {
			break
		}
	}
	return components
}

func (v *MirrorVerifier) trustedInputsUnchanged() bool {
	_, executableDigest, executableErr := trustedMirrorFile(v.executable, true, maxMirrorExecutableBytes)
	_, configurationDigest, configurationErr := trustedMirrorFile(v.configuration, false, maxMirrorConfigurationBytes)
	return executableErr == nil && configurationErr == nil && executableDigest == v.executableDigest && configurationDigest == v.configurationDigest
}
func (v *MirrorVerifier) VerifyReceipt(ctx context.Context, batch uint64, policy MirrorPolicy, receipt []byte) (MirrorVerification, error) {
	if len(receipt) > maxMirrorEvidenceBytes {
		return MirrorVerification{}, MirrorVerificationError{MirrorBounds}
	}
	return v.verify(ctx, batch, policy, map[string]any{"kind": "receipt", "canonical_hex": hex.EncodeToString(receipt)})
}
func (v *MirrorVerifier) VerifyState(ctx context.Context, batch uint64, policy MirrorPolicy, state, proof []byte) (MirrorVerification, error) {
	if len(state) > maxMirrorEvidenceBytes || len(proof) > maxMirrorEvidenceBytes-len(state) {
		return MirrorVerification{}, MirrorVerificationError{MirrorBounds}
	}
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
	if err != nil || len(request) > maxMirrorRequestBytes {
		return zero, MirrorVerificationError{MirrorBounds}
	}
	ctx, cancel := context.WithTimeout(parent, v.timeout)
	defer cancel()
	if !v.trustedInputsUnchanged() {
		return zero, MirrorVerificationError{MirrorConfiguration}
	}
	command := exec.CommandContext(ctx, v.executable, v.configuration)
	command.Stdin = bytes.NewReader(request)
	var stdout boundedMirrorOutput
	stdout.Grow(4096)
	command.Stdout = &stdout
	runErr := command.Run()
	if !v.trustedInputsUnchanged() {
		return zero, MirrorVerificationError{MirrorConfiguration}
	}
	if runErr != nil || ctx.Err() != nil {
		return zero, MirrorVerificationError{MirrorUnavailable}
	}
	if stdout.exceeded {
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
		code := MirrorErrorCode(response.Error)
		if !validMirrorError(code) {
			code = MirrorMalformed
		}
		return zero, MirrorVerificationError{code}
	}
	verifiedBatch, err := canonicalUint64(response.Verification.BatchNumber)
	if err != nil {
		return zero, err
	}
	header, err := fixedDigest(response.Verification.HeaderDigest)
	if err != nil {
		return zero, err
	}
	if verifiedBatch != batch
		|| response.Verification.Level == ""
		|| len(response.Verification.Level) > 64
		|| response.Verification.SourceID == ""
		|| len(response.Verification.SourceID) > 64
		|| response.Verification.Target == ""
		|| len(response.Verification.Target) > 2048
		|| response.Verification.CanonicalPosition == ""
		|| len(response.Verification.CanonicalPosition) > 2048
		|| (response.Verification.Provenance != "Canonical" && response.Verification.Provenance != "Reorged")
		|| response.Verification.BatchLag == ""
		|| len(response.Verification.BatchLag) > 64
		|| response.Verification.FailoverCount < 0
		|| response.Verification.FailoverCount >= len(policy.Candidates)
		|| response.Verification.AgreeingSources < 1
		|| response.Verification.AgreeingSources > len(policy.Candidates)
		|| (policy.Kind == MirrorAgreement && response.Verification.AgreeingSources < policy.Minimum)
		|| response.Verification.CheckpointLevel != "unavailable" {
		return zero, MirrorVerificationError{MirrorMalformed}
	}
	evidenceDigest, err := fixedDigest(response.Verification.EvidenceDigest)
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

func validMirrorError(code MirrorErrorCode) bool {
	switch code {
	case MirrorConfiguration, MirrorUnavailable, MirrorRateLimited, MirrorMissing,
		MirrorTargetMismatch, MirrorSourceMismatch, MirrorMalformed, MirrorBounds,
		MirrorCommitment, MirrorAuthorization, MirrorProof, MirrorCheckpointUnavailable,
		MirrorDivergent, MirrorInsufficientAgreement, MirrorReorged:
		return true
	default:
		return false
	}
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
