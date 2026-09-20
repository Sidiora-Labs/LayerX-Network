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
	credit, err := VerifyLightCredit(profileBytes, lightVector(t, "custody.credit"), nil)
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
		rebuilt, err := BuildLightCredit(profileBytes, owner, 77, evidence)
		if err != nil || !bytes.Equal(rebuilt, payload) {
			t.Fatal(name, "credit rebuild", err)
		}
	}
}

func TestLightVectorTrustRules(t *testing.T) {
	profile, adjacentProfile := lightVector(t, "custody.profile"), lightVector(t, "custody-adjacent.profile")
	first, adjacent, later := lightVector(t, "custody.credit"), lightVector(t, "custody-adjacent.credit"),
		lightVector(t, "custody-later.credit")
	credit, err := VerifyLightCredit(profile, first, nil)
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
	if _, err := VerifyLightCredit(adjacentProfile, adjacent, nil); err != nil {
		t.Fatal(err)
	}
	if _, err := VerifyLightCredit(adjacentProfile, first, nil); err == nil {
		t.Fatal("credit accepted under a profile its head does not bind")
	}
	again, err := VerifyLightCredit(profile, first, &credit.Trust)
	if err != nil || again.Trust.Height != credit.Trust.Height {
		t.Fatal("header at the trusted height", err)
	}
	advanced, err := VerifyLightCredit(profile, later, &credit.Trust)
	if err != nil {
		t.Fatal(err)
	}
	if advanced.HeaderHeight < credit.HeaderHeight+5 || advanced.DepositID != credit.DepositID ||
		advanced.Nullifier != credit.Nullifier {
		t.Fatal("later vector shape")
	}
	if _, err := VerifyLightCredit(profile, first, &advanced.Trust); err == nil {
		t.Fatal("header below the trusted height accepted")
	}
	forked := advanced.Trust
	forked.HeaderHash = bytes.Repeat([]byte{1}, 32)
	if _, err := VerifyLightCredit(profile, later, &forked); err == nil {
		t.Fatal("different header at the trusted height accepted")
	}
	rotated := credit.Trust
	rotated.NextValidatorsHash = bytes.Repeat([]byte{2}, 32)
	if _, err := VerifyLightCredit(profile, later, &rotated); err == nil {
		t.Fatal("non-adjacent update without a trusted validator set accepted")
	}
}

func TestLightVectorTamper(t *testing.T) {
	profile, payload := lightVector(t, "custody.profile"), lightVector(t, "custody.credit")
	for offset := 0; offset < len(payload); offset++ {
		mutated := append([]byte(nil), payload...)
		mutated[offset] ^= 0x01
		if _, err := VerifyLightCredit(profile, mutated, nil); err == nil && !(offset >= 139 && offset < 171) {
			t.Fatalf("mutation at %d accepted", offset)
		}
	}
	if _, err := VerifyLightCredit(profile, append(append([]byte(nil), payload...), 0), nil); err == nil {
		t.Fatal("trailing byte accepted")
	}
	if _, err := VerifyLightCredit(profile, payload[:len(payload)-1], nil); err == nil {
		t.Fatal("truncation accepted")
	}
}
