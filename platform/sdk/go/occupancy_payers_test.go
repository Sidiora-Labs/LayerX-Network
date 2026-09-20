package layerx

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"sort"
	"testing"
)

type occupancyPayerVectorPayer struct {
	DID          string `json:"did"`
	PrincipalHex string `json:"principal_hex"`
	MainHex      string `json:"main_account_hex"`
	AssetHex     string `json:"asset_account_hex"`
}

type occupancyPayerVector struct {
	Name     string `json:"name"`
	Evidence string `json:"evidence_hex"`
	Usage    struct {
		ByteBatches string `json:"byte_batches"`
		FeeUnits    string `json:"fee_units"`
		PaidUnits   string `json:"paid_units"`
		Positions   int    `json:"positions"`
	} `json:"usage"`
	Payers []occupancyPayerVectorPayer `json:"payers"`
	Roots  []struct {
		Accounts []string `json:"accounts"`
		RootHex  string   `json:"root_hex"`
	} `json:"account_transfer_roots"`
	PrincipalRootHex string `json:"principal_transfer_root_hex"`
	StrangerRootHex  string `json:"stranger_transfer_root_hex"`
}

type occupancyPayerFixture struct {
	AssetHex    string                    `json:"asset_hex"`
	TreasuryHex string                    `json:"fee_treasury_account_hex"`
	Stranger    occupancyPayerVectorPayer `json:"stranger"`
	Vectors     []occupancyPayerVector    `json:"vectors"`
}

func loadOccupancyPayerFixture(t *testing.T) occupancyPayerFixture {
	t.Helper()
	raw, err := os.ReadFile("../conformance/fixtures/occupancy-payment-accounts-v3.json")
	if err != nil {
		t.Fatal(err)
	}
	var fixture occupancyPayerFixture
	if err := json.Unmarshal(raw, &fixture); err != nil {
		t.Fatal(err)
	}
	if len(fixture.Vectors) == 0 {
		t.Fatal("occupancy payer fixture carries no vectors")
	}
	return fixture
}

func TestStateCommitmentOccupancyPaysFromTheProvenPayerAccount(t *testing.T) {
	fixture := loadOccupancyPayerFixture(t)
	asset := fixture32(t, fixture.AssetHex)
	if programOccupancyFeeTreasury() != fixture32(t, fixture.TreasuryHex) {
		t.Fatal("fee treasury account derivation diverged from the kernel")
	}
	stranger := fixture32(t, fixture.Stranger.MainHex)
	for _, vector := range fixture.Vectors {
		t.Run(vector.Name, func(t *testing.T) {
			settlement, err := decodeProgramOccupancy(fixtureBytes(t, vector.Evidence), asset)
			if err != nil {
				t.Fatalf("real kernel settlement refused: %v", err)
			}
			if settlement.ByteBatches.String() != vector.Usage.ByteBatches || settlement.FeeUnits.String() != vector.Usage.FeeUnits {
				t.Fatalf("settlement usage diverged: %s %s", settlement.ByteBatches.String(), settlement.FeeUnits.String())
			}
			if len(settlement.PaidByPayer) != vector.Usage.Positions {
				t.Fatalf("settlement charged %d payers, expected %d", len(settlement.PaidByPayer), vector.Usage.Positions)
			}
			payers := append([]occupancyPayerVectorPayer(nil), vector.Payers...)
			sort.Slice(payers, func(i, j int) bool { return payers[i].PrincipalHex < payers[j].PrincipalHex })
			for _, payer := range payers {
				if _, charged := settlement.PaidByPayer[fixture32(t, payer.PrincipalHex)]; !charged {
					t.Fatalf("settlement omits the fixture payer %s", payer.DID)
				}
			}
			offered := make([]OccupancyPayer, 0, len(payers))
			unproven := make([]OccupancyPayer, 0, len(payers))
			for _, payer := range payers {
				offered = append(offered, OccupancyPayer{DID: []byte(payer.DID)})
				unproven = append(unproven, OccupancyPayer{DID: []byte(payer.DID), Account: stranger[:]})
			}
			derivable := len(payers) <= 8
			for _, selection := range vector.Roots {
				root := fixture32(t, selection.RootHex)
				expected := make([][32]byte, 0, len(payers))
				explicit := make([]OccupancyPayer, 0, len(payers))
				for index, payer := range payers {
					account := fixture32(t, payer.MainHex)
					if selection.Accounts[index] == "asset" {
						account = fixture32(t, payer.AssetHex)
					}
					expected = append(expected, account)
					explicit = append(explicit, OccupancyPayer{DID: []byte(payer.DID), Account: account[:]})
				}
				proven, err := verifyProgramOccupancyTransferRoot(settlement.PaidByPayer, 3, asset, root, explicit)
				if err != nil || !sameOccupancyAccounts(proven, expected) {
					t.Fatalf("explicit payment accounts refused: %v %v", proven, err)
				}
				derived, err := verifyProgramOccupancyTransferRoot(settlement.PaidByPayer, 3, asset, root, offered)
				switch {
				case derivable && (err != nil || !sameOccupancyAccounts(derived, expected)):
					t.Fatalf("derived payment accounts refused: %v %v", derived, err)
				case !derivable && err == nil:
					t.Fatal("candidate enumeration exceeded the payer bound without refusing")
				}
				if _, err := verifyProgramOccupancyTransferRoot(settlement.PaidByPayer, 3, asset, root, nil); err == nil {
					t.Fatal("a paid state-commitment charge was accepted with no payer offered")
				}
				if _, err := verifyProgramOccupancyTransferRoot(settlement.PaidByPayer, 3, asset, root, unproven); err == nil {
					t.Fatal("an underivable payment account was accepted")
				}
				if _, err := verifyProgramOccupancyTransferRoot(settlement.PaidByPayer, 3, asset, root,
					[]OccupancyPayer{{DID: []byte(fixture.Stranger.DID)}}); err == nil {
					t.Fatal("an uncharged payer proved the committed accounts")
				}
				if _, err := verifyProgramOccupancyTransferRoot(settlement.PaidByPayer, 3, [32]byte{}, root, explicit); err == nil {
					t.Fatal("a zero occupancy asset was accepted")
				}
			}
			if vector.PrincipalRootHex != "" {
				principal := fixture32(t, vector.PrincipalRootHex)
				if programOccupancyTransferRoot(settlement.PaidByPayer, asset) != principal {
					t.Fatal("legacy principal transfer root diverged from the kernel")
				}
				legacy, err := verifyProgramOccupancyTransferRoot(settlement.PaidByPayer, 2, asset, principal, nil)
				if err != nil || len(legacy) != 0 {
					t.Fatalf("protocol 2 principal payment refused: %v %v", legacy, err)
				}
				if _, err := verifyProgramOccupancyTransferRoot(settlement.PaidByPayer, 3, asset, principal, offered); err == nil {
					t.Fatal("the state commitment accepted the payer principal as the paying account")
				}
			}
			if vector.StrangerRootHex != "" {
				if _, err := verifyProgramOccupancyTransferRoot(settlement.PaidByPayer, 3, asset, fixture32(t, vector.StrangerRootHex), offered); err == nil {
					t.Fatal("the state commitment accepted an unrelated account as the paying account")
				}
			}
		})
	}
}

func TestOccupancyPayerAccountDerivationMatchesTheKernel(t *testing.T) {
	fixture := loadOccupancyPayerFixture(t)
	asset := fixture32(t, fixture.AssetHex)
	for _, vector := range fixture.Vectors {
		for _, payer := range append(append([]occupancyPayerVectorPayer(nil), vector.Payers...), fixture.Stranger) {
			main, err := programOccupancyPaymentAccount([]byte(payer.DID), asset, false)
			if err != nil || main.Payer != fixture32(t, payer.PrincipalHex) || main.Account != fixture32(t, payer.MainHex) {
				t.Fatalf("main account derivation diverged for %s: %v", payer.DID, err)
			}
			scoped, err := programOccupancyPaymentAccount([]byte(payer.DID), asset, true)
			if err != nil || scoped.Account != fixture32(t, payer.AssetHex) {
				t.Fatalf("asset account derivation diverged for %s: %v", payer.DID, err)
			}
			label := "agent:" + payer.DID + ":asset:" + hex.EncodeToString(asset[:])
			material := append([]byte("LX:ACCOUNT:v1"), 0, 0, 0, byte(len(label)))
			if scoped.Account != sha256.Sum256(append(material, label...)) {
				t.Fatalf("asset account label diverged for %s", payer.DID)
			}
		}
	}
	if _, err := programOccupancyPaymentAccount(nil, asset, false); err == nil {
		t.Fatal("an empty payer DID derived a payment account")
	}
	if _, err := programOccupancyPaymentAccount(make([]byte, 256), asset, false); err == nil {
		t.Fatal("an over-long payer DID derived a payment account")
	}
	if _, err := programOccupancyPaymentAccount([]byte("did:layerx:payer0"), [32]byte{}, false); err == nil {
		t.Fatal("a zero asset derived a payment account")
	}
}

func sameOccupancyAccounts(left [][32]byte, right [][32]byte) bool {
	if len(left) != len(right) {
		return false
	}
	for index := range left {
		if left[index] != right[index] {
			return false
		}
	}
	return true
}
