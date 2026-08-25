package types

import (
	"math"
	"testing"
	"time"

	"github.com/sidiora-labs/paxeer-network/consensus/libs/utils"
	"github.com/sidiora-labs/paxeer-network/consensus/libs/utils/require"
)

func TestNewCommittee_FiltersOutZeroWeightValidators(t *testing.T) {
	rng := utils.TestRng()
	firstBlock := GenGlobalBlockNumber(rng)
	genesisTimestamp := time.Now()
	zeroWeightKey := GenPublicKey(rng)
	nonZeroWeightKey := GenPublicKey(rng)

	committee, err := NewCommittee("test-chain", map[PublicKey]uint64{
		zeroWeightKey:    0,
		nonZeroWeightKey: 7,
	}, firstBlock, genesisTimestamp)
	if err != nil {
		t.Fatalf("NewCommittee(): %v", err)
	}

	if committee.HasReplica(zeroWeightKey) {
		t.Fatal("HasReplica() = true for zero-weight validator, want false")
	}
	if got := committee.Replicas().Len(); got != 1 {
		t.Fatalf("Replicas().Len() = %v, want 1", got)
	}
	if got := committee.Replicas().At(0); got != nonZeroWeightKey {
		t.Fatalf("Replicas().At(0) = %v, want %v", got, nonZeroWeightKey)
	}
	if got := committee.Weight(nonZeroWeightKey); got != 7 {
		t.Fatalf("Weight() = %v, want 7", got)
	}
}

func TestNewCommittee_RejectsZeroTotalWeight(t *testing.T) {
	rng := utils.TestRng()
	firstBlock := GenGlobalBlockNumber(rng)
	genesisTimestamp := time.Now()

	_, err := NewCommittee("test-chain", map[PublicKey]uint64{
		GenPublicKey(rng): 0,
		GenPublicKey(rng): 0,
	}, firstBlock, genesisTimestamp)
	if err == nil {
		t.Fatal("NewCommittee() succeeded, want error")
	}
}

func TestNewCommittee_RejectsWeightOverflow(t *testing.T) {
	rng := utils.TestRng()
	firstBlock := GenGlobalBlockNumber(rng)
	genesisTimestamp := time.Now()

	_, err := NewCommittee("test-chain", map[PublicKey]uint64{
		GenPublicKey(rng): math.MaxUint64,
		GenPublicKey(rng): 1,
	}, firstBlock, genesisTimestamp)
	if err == nil {
		t.Fatal("NewCommittee() succeeded, want error")
	}
}

func TestSignatureVerificationBindsChainAndCommittee(t *testing.T) {
	rng := utils.TestRng()
	key := GenSecretKey(rng)
	weights := map[PublicKey]uint64{key.Public(): 1}
	genesis := time.Unix(1_700_000_000, 0).UTC()
	committeeA := utils.OrPanic1(NewCommittee("chain-a", weights, 1, genesis))
	committeeB := utils.OrPanic1(NewCommittee("chain-b", weights, 1, genesis))
	nextEpoch := utils.OrPanic1(NewCommittee("chain-a", weights, 2, genesis))
	vote := NewLaneVote(NewBlock(key.Public(), 0, GenBlockHeaderHash(rng), GenPayload(rng)).Header())
	signed := Sign(key.ForCommittee(committeeA), vote)

	require.NoError(t, signed.VerifySig(committeeA))
	require.Error(t, signed.VerifySig(committeeB))
	require.Error(t, signed.VerifySig(nextEpoch))
}

func TestSignRejectsUnboundValidatorKey(t *testing.T) {
	rng := utils.TestRng()
	key := GenSecretKey(rng)
	vote := NewLaneVote(NewBlock(key.Public(), 0, GenBlockHeaderHash(rng), GenPayload(rng)).Header())
	require.Panics(t, func() { Sign(key, vote) })
}

func TestLeaderElectionUsesCommitteeDomain(t *testing.T) {
	rng := utils.TestRng()
	keys := []SecretKey{GenSecretKey(rng), GenSecretKey(rng), GenSecretKey(rng), GenSecretKey(rng)}
	weights := map[PublicKey]uint64{}
	for _, key := range keys {
		weights[key.Public()] = 1
	}
	a := utils.OrPanic1(NewCommittee("chain-a", weights, 1, time.Unix(1_700_000_000, 0).UTC()))
	b := utils.OrPanic1(NewCommittee("chain-b", weights, 1, time.Unix(1_700_000_000, 0).UTC()))
	different := false
	for number := ViewNumber(0); number < 64; number++ {
		if a.Leader(View{Index: 7, Number: number}) != b.Leader(View{Index: 7, Number: number}) {
			different = true
			break
		}
	}
	require.True(t, different)
}

func makeCommittee() (*Committee, []SecretKey) {
	keys := []SecretKey{
		TestSecretKey("heavy"),
		TestSecretKey("light1"),
		TestSecretKey("light2"),
	}
	committee := utils.OrPanic1(NewCommittee("test-chain", map[PublicKey]uint64{
		keys[0].Public(): 5,
		keys[1].Public(): 1,
		keys[2].Public(): 1,
	}, 0, time.Now()))
	for i := range keys {
		keys[i] = keys[i].ForCommittee(committee)
	}
	return committee, keys
}

func TestLaneQCVerifyChecksWeight(t *testing.T) {
	rng := utils.TestRng()
	committee, keys := makeCommittee()
	vote := NewLaneVote(NewBlock(keys[0].Public(), 0, GenBlockHeaderHash(rng), GenPayload(rng)).Header())

	heavyOnly := NewLaneQC([]*Signed[*LaneVote]{
		Sign(keys[0], vote),
	})
	require.NoError(t, heavyOnly.Verify(committee))
	lightMajority := NewLaneQC([]*Signed[*LaneVote]{
		Sign(keys[1], vote),
		Sign(keys[2], vote),
	})
	require.Error(t, lightMajority.Verify(committee))
}

func TestPrepareQCVerifyChecksWeight(t *testing.T) {
	rng := utils.TestRng()
	committee, keys := makeCommittee()
	vote := NewPrepareVote(GenProposalAt(rng, View{}))

	heavyOnly := NewPrepareQC([]*Signed[*PrepareVote]{
		Sign(keys[0], vote),
	})
	require.NoError(t, heavyOnly.Verify(committee))
	lightMajority := NewPrepareQC([]*Signed[*PrepareVote]{
		Sign(keys[1], vote),
		Sign(keys[2], vote),
	})
	require.Error(t, lightMajority.Verify(committee))
}

func TestCommitQCVerifyChecksWeight(t *testing.T) {
	rng := utils.TestRng()
	committee, keys := makeCommittee()
	vote := NewCommitVote(GenProposalAt(rng, View{}))

	heavyOnly := NewCommitQC([]*Signed[*CommitVote]{
		Sign(keys[0], vote),
	})
	require.NoError(t, heavyOnly.Verify(committee))
	lightMajority := NewCommitQC([]*Signed[*CommitVote]{
		Sign(keys[1], vote),
		Sign(keys[2], vote),
	})
	require.Error(t, lightMajority.Verify(committee))
}

func TestAppQCVerifyChecksWeight(t *testing.T) {
	rng := utils.TestRng()
	committee, keys := makeCommittee()
	vote := NewAppVote(NewAppProposal(0, 0, GenAppHash(rng)))

	heavyOnly := NewAppQC([]*Signed[*AppVote]{
		Sign(keys[0], vote),
	})
	require.NoError(t, heavyOnly.Verify(committee))

	lightMajority := NewAppQC([]*Signed[*AppVote]{
		Sign(keys[1], vote),
		Sign(keys[2], vote),
	})
	require.Error(t, lightMajority.Verify(committee))
}

func TestTimeoutQCVerifyChecksWeight(t *testing.T) {
	committee, keys := makeCommittee()
	view := View{}

	heavyOnly := NewTimeoutQC([]*FullTimeoutVote{
		NewFullTimeoutVote(keys[0], view, utils.None[*PrepareQC]()),
	})
	if err := heavyOnly.Verify(committee, utils.None[*CommitQC]()); err != nil {
		t.Fatalf("heavyOnly.Verify(): %v", err)
	}

	lightMajority := NewTimeoutQC([]*FullTimeoutVote{
		NewFullTimeoutVote(keys[1], view, utils.None[*PrepareQC]()),
		NewFullTimeoutVote(keys[2], view, utils.None[*PrepareQC]()),
	})
	if err := lightMajority.Verify(committee, utils.None[*CommitQC]()); err == nil {
		t.Fatal("lightMajority.Verify() succeeded, want error")
	}
}
