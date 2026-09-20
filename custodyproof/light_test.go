package custodyproof

import (
	"bytes"
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/hex"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/sidiora-labs/paxeer-network/consensus/crypto"
	tmed25519 "github.com/sidiora-labs/paxeer-network/consensus/crypto/ed25519"
	"github.com/sidiora-labs/paxeer-network/consensus/libs/utils"
	"github.com/sidiora-labs/paxeer-network/consensus/types"
)

const lightVectors = "../tests/fixtures/custody/paxeer-light-v1"

func lightVector(t *testing.T, name string) []byte {
	t.Helper()
	data, err := os.ReadFile(filepath.Join(lightVectors, name))
	if err != nil {
		t.Fatal(err)
	}
	return data
}

// lightNow is a verifier clock one hour after the profile's trusted header,
// inside the vectors' trusting period and after every vector header.
func lightNow(t *testing.T, profileBytes []byte) time.Time {
	t.Helper()
	profile, err := DecodeLightProfile(profileBytes)
	if err != nil {
		t.Fatal(err)
	}
	return time.Unix(int64(profile.TrustedTime)+3600, 0).UTC()
}

func TestLightVectorIdentity(t *testing.T) {
	profileBytes := lightVector(t, "custody.profile")
	profile, err := DecodeLightProfile(profileBytes)
	if err != nil {
		t.Fatal(err)
	}
	if profile.NetworkID != 77 || profile.AssetID != sha256.Sum256([]byte("LayerX/native-custody-qualification/asset")) ||
		profile.CometChainID != "hyperpax_125-1" || profile.TrustedHeight > 2 {
		t.Fatalf("profile parameters: %+v", profile)
	}
	seed := lightVector(t, "actor.seed")
	if expected := sha256.Sum256([]byte("LayerX/light-credit-vector/actor")); !bytes.Equal(seed, expected[:]) {
		t.Fatal("actor seed")
	}
	public := ed25519.NewKeyFromSeed(seed).Public().(ed25519.PublicKey)
	did := "did:layerx:" + hex.EncodeToString(public)
	if strings.TrimSpace(string(lightVector(t, "did.txt"))) != did {
		t.Fatal("did")
	}
	if profile.TrustingPeriod != 1209600 || profile.TrustedTime == 0 || len(profileBytes) != 223 {
		t.Fatalf("profile trust parameters: %+v", profile)
	}
	now := lightNow(t, profileBytes)
	credit, err := VerifyLightCredit(profileBytes, lightVector(t, "custody.credit"), nil, now)
	if err != nil {
		t.Fatal(err)
	}
	if credit.Beneficiary != LightAccountID("agent:"+did+":main") || !bytes.Equal(credit.OwnerKey[:], public) ||
		credit.Amount.String() != "1000000" || credit.Nonce != 1 || credit.HeaderHeight != credit.StateHeight+1 ||
		credit.HeaderHeight <= profile.TrustedHeight+1 || credit.Evidence.Trusted != nil {
		t.Fatalf("credit parameters: %+v", credit)
	}
	nullifier := strings.TrimSpace(string(lightVector(t, "custody.credit.nullifier")))
	if nullifier != hex.EncodeToString(credit.Nullifier[:]) {
		t.Fatal("nullifier")
	}
}

func TestLightVectorRoundTrip(t *testing.T) {
	profileBytes := lightVector(t, "custody.profile")
	profile, err := DecodeLightProfile(profileBytes)
	if err != nil {
		t.Fatal(err)
	}
	now := lightNow(t, profileBytes)
	for _, name := range []string{"custody.credit", "custody-later.credit"} {
		payload := lightVector(t, name)
		bundle := payload[LightCreditHeadBytes:]
		evidence, err := DecodeLightBundle(bundle, profile.CometChainID)
		if err != nil {
			t.Fatal(name, err)
		}
		if !bytes.Equal(evidence.Header.Hash(), payload[223:255]) || !bytes.Equal(evidence.Validators.Hash(), payload[295:327]) {
			t.Fatal(name, "decoded header or validator hash")
		}
		encoded, err := EncodeLightBundle(evidence)
		if err != nil || !bytes.Equal(encoded, bundle) {
			t.Fatal(name, "bundle round trip", err)
		}
		var owner [32]byte
		copy(owner[:], payload[139:171])
		rebuilt, err := BuildLightCredit(profileBytes, owner, 77, evidence, now)
		if err != nil || !bytes.Equal(rebuilt, payload) {
			t.Fatal(name, "credit rebuild", err)
		}
	}
}

func TestLightVectorTrustRules(t *testing.T) {
	profile, adjacentProfile := lightVector(t, "custody.profile"), lightVector(t, "custody-adjacent.profile")
	first, adjacent, later := lightVector(t, "custody.credit"), lightVector(t, "custody-adjacent.credit"),
		lightVector(t, "custody-later.credit")
	now := lightNow(t, profile)
	credit, err := VerifyLightCredit(profile, first, nil, now)
	if err != nil {
		t.Fatal(err)
	}
	decoded, err := DecodeLightProfile(adjacentProfile)
	if err != nil {
		t.Fatal(err)
	}
	if decoded.TrustedHeight+1 != credit.HeaderHeight || !bytes.Equal(adjacent[LightCreditHeadBytes:], first[LightCreditHeadBytes:]) ||
		bytes.Equal(adjacent[5:37], first[5:37]) {
		t.Fatal("adjacent vector shape")
	}
	if _, err := VerifyLightCredit(adjacentProfile, adjacent, nil, now); err != nil {
		t.Fatal(err)
	}
	if _, err := VerifyLightCredit(adjacentProfile, first, nil, now); err == nil {
		t.Fatal("credit accepted under a profile its head does not bind")
	}
	again, err := VerifyLightCredit(profile, first, &credit.Trust, now)
	if err != nil || again.Trust.Height != credit.Trust.Height {
		t.Fatal("header at the trusted height", err)
	}
	advanced, err := VerifyLightCredit(profile, later, &credit.Trust, now)
	if err != nil {
		t.Fatal(err)
	}
	if advanced.HeaderHeight < credit.HeaderHeight+5 || advanced.DepositID != credit.DepositID ||
		advanced.Nullifier != credit.Nullifier {
		t.Fatal("later vector shape")
	}
	if _, err := VerifyLightCredit(profile, first, &advanced.Trust, now); err == nil {
		t.Fatal("header below the trusted height accepted")
	}
	forked := advanced.Trust
	forked.HeaderHash = bytes.Repeat([]byte{1}, 32)
	if _, err := VerifyLightCredit(profile, later, &forked, now); err == nil {
		t.Fatal("different header at the trusted height accepted")
	}
	rotated := credit.Trust
	rotated.NextValidatorsHash = bytes.Repeat([]byte{2}, 32)
	if _, err := VerifyLightCredit(profile, later, &rotated, now); err == nil {
		t.Fatal("non-adjacent update without a trusted validator set accepted")
	}
}

func TestLightVectorTamper(t *testing.T) {
	profile, payload := lightVector(t, "custody.profile"), lightVector(t, "custody.credit")
	now := lightNow(t, profile)
	for offset := 0; offset < len(payload); offset++ {
		mutated := append([]byte(nil), payload...)
		mutated[offset] ^= 0x01
		if _, err := VerifyLightCredit(profile, mutated, nil, now); err == nil && !(offset >= 139 && offset < 171) {
			t.Fatalf("mutation at %d accepted", offset)
		}
	}
	if _, err := VerifyLightCredit(profile, append(append([]byte(nil), payload...), 0), nil, now); err == nil {
		t.Fatal("trailing byte accepted")
	}
	if _, err := VerifyLightCredit(profile, payload[:len(payload)-1], nil, now); err == nil {
		t.Fatal("truncation accepted")
	}
}

func TestLightVectorTimeRules(t *testing.T) {
	profileBytes, payload, later := lightVector(t, "custody.profile"), lightVector(t, "custody.credit"),
		lightVector(t, "custody-later.credit")
	profile, err := DecodeLightProfile(profileBytes)
	if err != nil {
		t.Fatal(err)
	}
	evidence, err := DecodeLightBundle(payload[LightCreditHeadBytes:], profile.CometChainID)
	if err != nil {
		t.Fatal(err)
	}
	trusted, header := int64(profile.TrustedTime), evidence.Header.Time.Unix()
	period := int64(profile.TrustingPeriod)
	at := func(seconds int64) time.Time { return time.Unix(seconds, 0).UTC() }
	if _, err := VerifyLightCredit(profileBytes, payload, nil, at(trusted+period-1)); err != nil {
		t.Fatal("last second of the trusting period", err)
	}
	if _, err := VerifyLightCredit(profileBytes, payload, nil, at(trusted+period)); err == nil {
		t.Fatal("expired trust accepted")
	}
	if _, err := VerifyLightCredit(profileBytes, payload, nil, at(header-LightMaxClockDriftSecs)); err != nil {
		t.Fatal("header at the clock drift bound", err)
	}
	if _, err := VerifyLightCredit(profileBytes, payload, nil, at(header-LightMaxClockDriftSecs-1)); err == nil {
		t.Fatal("header from the future accepted")
	}
	if _, err := VerifyLightCredit(profileBytes, payload, nil, time.Time{}); err == nil {
		t.Fatal("zero verification time accepted")
	}
	now := lightNow(t, profileBytes)
	credit, err := VerifyLightCredit(profileBytes, payload, nil, now)
	if err != nil {
		t.Fatal(err)
	}
	if !credit.Trust.Time.Equal(evidence.Header.Time) {
		t.Fatal("trust time after the update")
	}
	if _, err := VerifyLightCredit(profileBytes, later, &credit.Trust, at(header+period)); err == nil {
		t.Fatal("expired advanced trust accepted")
	}
	stale := credit.Trust
	stale.Height, stale.HeaderHash = stale.Height-1, nil
	if _, err := VerifyLightCredit(profileBytes, payload, &stale, now); err == nil {
		t.Fatal("header not after the trusted header time accepted")
	}
	seeded := append([]byte(nil), profileBytes...)
	copy(seeded[215:223], []byte{0, 0, 0, 0, 0, 0, 0, 0})
	if _, err := DecodeLightProfile(seeded); err == nil {
		t.Fatal("profile without a trusted time accepted")
	}
	if _, err := DecodeLightProfile(seeded[:207]); err == nil {
		t.Fatal("207-byte profile accepted")
	}
}

func TestLightNilVoteSignature(t *testing.T) {
	const chainID = "hyperpax_125-1"
	keys := make([]crypto.PrivKey, 4)
	validators := make([]*types.Validator, 4)
	for index := range keys {
		keys[index] = tmed25519.TestSecretKey([]byte{byte(index), 'l', 'x'})
		validators[index] = types.NewValidator(keys[index].Public(), 10)
	}
	set := types.NewValidatorSet(validators)
	blockID := types.BlockID{Hash: bytes.Repeat([]byte{7}, 32),
		PartSetHeader: types.PartSetHeader{Total: 1, Hash: bytes.Repeat([]byte{8}, 32)}}
	build := func(wrongBytes bool) *types.Commit {
		commit := &types.Commit{Height: 9, Round: 0, BlockID: blockID}
		stamp := time.Unix(1_800_000_000, 5).UTC()
		for index, validator := range set.Validators {
			flag := types.BlockIDFlagCommit
			if index == 3 {
				flag = types.BlockIDFlagNil
			}
			commit.Signatures = append(commit.Signatures, types.CommitSig{BlockIDFlag: flag,
				ValidatorAddress: validator.Address, Timestamp: stamp})
		}
		for index, validator := range set.Validators {
			var key crypto.PrivKey
			for _, candidate := range keys {
				if candidate.Public().Address().String() == validator.Address.String() {
					key = candidate
				}
			}
			signed := *commit
			if wrongBytes && index == 3 {
				signed.Signatures = append([]types.CommitSig(nil), commit.Signatures...)
				signed.Signatures[3].BlockIDFlag = types.BlockIDFlagCommit
			}
			signBytes, ok := signed.VoteSignBytes(chainID, int32(index))
			if !ok {
				t.Fatal("vote sign bytes")
			}
			commit.Signatures[index].Signature = utils.Some(key.Sign(signBytes))
		}
		return commit
	}
	good := build(false)
	if err := set.VerifyCommitLightAllSignatures(chainID, blockID, 9, good); err != nil {
		t.Fatal(err)
	}
	if err := lightNilVotes(chainID, set, good); err != nil {
		t.Fatal(err)
	}
	bad := build(true)
	if err := set.VerifyCommitLightAllSignatures(chainID, blockID, 9, bad); err != nil {
		t.Fatal("the quorum check does not look at nil votes", err)
	}
	if err := lightNilVotes(chainID, set, bad); err == nil {
		t.Fatal("nil vote signed over a block id accepted")
	}
	if err := lightNilVotes("hyperpax_125-2", set, good); err == nil {
		t.Fatal("nil vote for another chain accepted")
	}
}

func TestLightVectorSkipAcrossValidatorChange(t *testing.T) {
	profileBytes, payload := lightVector(t, "custody-skip.profile"), lightVector(t, "custody-skip.credit")
	profile, err := DecodeLightProfile(profileBytes)
	if err != nil {
		t.Fatal(err)
	}
	now := lightNow(t, profileBytes)
	credit, err := VerifyLightCredit(profileBytes, payload, nil, now)
	if err != nil {
		t.Fatal(err)
	}
	trusted := credit.Evidence.Trusted
	if trusted == nil || !bytes.Equal(trusted.Hash(), profile.TrustedHash[:]) ||
		bytes.Equal(credit.ValidatorsHash[:], profile.TrustedHash[:]) || credit.HeaderHeight <= profile.TrustedHeight+1 ||
		trusted.TotalVotingPower() == credit.Evidence.Validators.TotalVotingPower() {
		t.Fatal("skip vector shape")
	}
	evidence, err := DecodeLightBundle(payload[LightCreditHeadBytes:], profile.CometChainID)
	if err != nil {
		t.Fatal(err)
	}
	var owner [32]byte
	copy(owner[:], payload[139:171])
	evidence.Trusted = nil
	if _, err := BuildLightCredit(profileBytes, owner, 77, evidence, now); err == nil {
		t.Fatal("validator change skipped without the trusted validator set")
	}
	evidence.Trusted = types.NewValidatorSet([]*types.Validator{
		types.NewValidator(tmed25519.TestSecretKey([]byte("lx-unrelated")).Public(), 7000000000)})
	if _, err := BuildLightCredit(profileBytes, owner, 77, evidence, now); err == nil {
		t.Fatal("unrelated trusted validator set accepted")
	}
	for offset := LightCreditHeadBytes; offset < len(payload); offset++ {
		mutated := append([]byte(nil), payload...)
		mutated[offset] ^= 0x01
		if _, err := VerifyLightCredit(profileBytes, mutated, nil, now); err == nil {
			t.Fatalf("mutation at %d accepted", offset)
		}
	}
}
