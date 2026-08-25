package types

import (
	"crypto/sha256"
	"encoding/binary"
	"errors"
	"fmt"
	"iter"
	"maps"
	"slices"
	"time"

	"github.com/ethereum/go-ethereum/common"
	"github.com/holiman/uint256"
	"github.com/sidiora-labs/paxeer-network/consensus/libs/utils"
)

// Immutable slice.
type ImSlice[T any] struct{ s []T }

func (s ImSlice[T]) Len() int         { return len(s.s) }
func (s ImSlice[T]) At(i int) T       { return s.s[i] }
func (s ImSlice[T]) All() iter.Seq[T] { return slices.Values(s.s) }

// Committee represents the consensus committee.
type Committee struct {
	chainID     string
	domain      [sha256.Size]byte
	replicas    ImSlice[PublicKey]
	weights     map[PublicKey]uint64
	totalWeight uint64
	// Number of the first block of the chain.
	// TODO: firstBlock is not really a part of the committee,
	// but it does belong to a chain spec (or epoch spec/genesis/etc.),
	// which should be passed around to verify autobahn messages.
	// Once we introduce the chain spec it should wrap Committee and firstBlock.
	firstBlock GlobalBlockNumber
	// timestamp at genesis. All blocks need to have a timestamp later than genesis.
	genesisTimestamp time.Time
}

func (c *Committee) HasReplica(k PublicKey) bool {
	_, ok := c.weights[k]
	return ok
}

func (c *Committee) HasLane(l LaneID) bool {
	_, ok := c.weights[l]
	return ok
}

// Lanes is the list of nodes which are eligible to produce blocks.
func (c *Committee) Lanes() ImSlice[LaneID] { return c.replicas }

// Replicas is the list of nodes which are eligible to participate in the consensus.
func (c *Committee) Replicas() ImSlice[PublicKey] { return c.replicas }

// FirstBlock is the index of the first global block finalized by this committee.
func (c *Committee) FirstBlock() GlobalBlockNumber { return c.firstBlock }

// GenesisTimestamp is the timestamp at genesis.
func (c *Committee) GenesisTimestamp() time.Time { return c.genesisTimestamp }

// ChainID is the network whose messages this committee is authorized to verify.
func (c *Committee) ChainID() string { return c.chainID }

func committeeDomain(chainID string, replicas []PublicKey, weights map[PublicKey]uint64, firstBlock GlobalBlockNumber, genesisTimestamp time.Time) [sha256.Size]byte {
	material := append([]byte("pax/autobahn/committee/v1\x00"), binary.BigEndian.AppendUint64(nil, uint64(len(chainID)))...)
	material = append(material, chainID...)
	material = binary.BigEndian.AppendUint64(material, uint64(firstBlock))
	material = binary.BigEndian.AppendUint64(material, uint64(genesisTimestamp.Unix()))
	material = binary.BigEndian.AppendUint32(material, uint32(genesisTimestamp.Nanosecond()))
	material = binary.BigEndian.AppendUint64(material, uint64(len(replicas)))
	for _, replica := range replicas {
		key := replica.Bytes()
		material = binary.BigEndian.AppendUint64(material, uint64(len(key)))
		material = append(material, key...)
		material = binary.BigEndian.AppendUint64(material, weights[replica])
	}
	return sha256.Sum256(material)
}

// Deterministic random oracle selecting a replica with probability proportional to the weight.
func (c *Committee) randomReplica(seed []byte) PublicKey {
	h := sha256.Sum256(seed[:])
	var x, total uint256.Int
	x.SetBytes32(h[:])
	total.SetUint64(c.totalWeight)
	y := x.Mod(&x, &total).Uint64()
	// TODO(gprusak): this can be optimized to O(1) lookup
	for k := range c.replicas.All() {
		w := c.weights[k]
		if y < w {
			return k
		}
		y -= w
	}
	panic("unreachable")
}

// Weight of validator k.
func (c *Committee) Weight(k PublicKey) uint64 { return c.weights[k] }

// Replica which is responsible for sequencing transactions from this addr.
func (c *Committee) EvmShard(addr common.Address) PublicKey {
	// TODO(gprusak): given that we currently do not have censorship-resistance,
	// from correctness perspective if doesn't matter if shards are proportional to weights.
	// For private testnet we need the load on each validator to be the same.
	// For mainnet we need to resolve this issue somehow differently.
	seed := append([]byte("pax/autobahn/evm-shard/v1\x00"), c.domain[:]...)
	return c.randomReplica(append(seed, addr[:]...))
}

// Leader for the consensus round with the given index.
func (c *Committee) Leader(view View) PublicKey {
	d := append([]byte("pax/autobahn/leader/v1\x00"), c.domain[:]...)
	d = binary.BigEndian.AppendUint64(d, uint64(view.Index))
	d = binary.BigEndian.AppendUint64(d, uint64(view.Number))
	return c.randomReplica(d)
}

// Faulty is the maximal total weight of faulty replicas that consensus can tolerate.
func (c *Committee) Faulty() uint64 {
	// 3f < N
	return (c.totalWeight - 1) / 3
}

// CommitQuorum is the weight of the quorum required for CommitQC.
func (c *Committee) CommitQuorum() uint64 {
	return c.totalWeight - c.Faulty()
}

// AppQuorum is the weight of the quorum required for AppQC.
func (c *Committee) AppQuorum() uint64 {
	// This needs to be in range (c.Faulty(), c.CommitQuorum()]
	return c.CommitQuorum()
}

// PrepareQuorum is the weight of the quorum required for PrepareQC.
func (c *Committee) PrepareQuorum() uint64 {
	return c.CommitQuorum()
}

// TimeoutQuorum is the size of the quorum required for TimeoutQC.
func (c *Committee) TimeoutQuorum() uint64 {
	return c.CommitQuorum()
}

// LaneQuorum is the weight of the quorum required for LaneQC.
func (c *Committee) LaneQuorum() uint64 {
	return c.Faulty() + 1
}

func NewCommittee(chainID string, weights map[PublicKey]uint64, firstBlock GlobalBlockNumber, genesisTimestamp time.Time) (*Committee, error) {
	if chainID == "" {
		return nil, errors.New("chain ID is empty")
	}
	weights = maps.Clone(weights)
	totalWeight := uint64(0)
	for k, w := range weights {
		if w == 0 {
			delete(weights, k)
		}
		if utils.Max[uint64]()-totalWeight < w {
			return nil, fmt.Errorf("total weight overflow")
		}
		totalWeight += w
	}
	if totalWeight == 0 {
		return nil, errors.New("total weight is 0")
	}
	replicas := slices.SortedFunc(maps.Keys(weights), func(a, b PublicKey) int { return a.Compare(b) })
	return &Committee{
		chainID:          chainID,
		domain:           committeeDomain(chainID, replicas, weights, firstBlock, genesisTimestamp),
		replicas:         ImSlice[PublicKey]{replicas},
		weights:          weights,
		totalWeight:      totalWeight,
		firstBlock:       firstBlock,
		genesisTimestamp: genesisTimestamp,
	}, nil
}

// NewRoundRobinElection creates a Committee with round robin election starting at firstBlock.
func NewRoundRobinElection(chainID string, replicas []PublicKey, firstBlock GlobalBlockNumber, genesisTimestamp time.Time) (*Committee, error) {
	weights := map[PublicKey]uint64{}
	for _, k := range replicas {
		weights[k] = 1
	}
	return NewCommittee(chainID, weights, firstBlock, genesisTimestamp)
}
