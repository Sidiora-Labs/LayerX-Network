package config

import (
	"errors"
	"net/url"
	"strings"

	atypes "github.com/sidiora-labs/paxeer-network/consensus/autobahn/types"
	"github.com/sidiora-labs/paxeer-network/consensus/internal/p2p"
	"github.com/sidiora-labs/paxeer-network/consensus/libs/utils"
	"github.com/sidiora-labs/paxeer-network/consensus/libs/utils/tcp"
)

// Errors reported for the autobahn consensus persistence settings.
//
// Consensus safety state (the highest view entered and the votes cast in it)
// only survives a restart when it is written to disk. A config that leaves it
// in memory installs the no-op persister, so the node forgets what it already
// voted for and can equivocate after a restart. Such a config is refused
// unless the operator opted out explicitly for a throwaway test network.
var (
	// ErrPersistentStateDirRequired is returned when persistent_state_dir is
	// absent and the test-only opt-out was not set.
	ErrPersistentStateDirRequired = errors.New("persistent_state_dir must be set: without it the consensus safety state is discarded on restart, which risks equivocation and slashing; set unsafe_test_only_disable_persistence to run in-memory in a throwaway test network")
	// ErrPersistentStateDirBlank is returned when persistent_state_dir is
	// present but contains only whitespace.
	ErrPersistentStateDirBlank = errors.New("persistent_state_dir must not be blank")
	// ErrPersistenceOptOutConflict is returned when persistent_state_dir and
	// unsafe_test_only_disable_persistence are both set, which states two
	// contradictory intents about durability.
	ErrPersistenceOptOutConflict = errors.New("persistent_state_dir must not be set together with unsafe_test_only_disable_persistence")
)

type URL struct{ *url.URL }

func (u URL) MarshalText() ([]byte, error) { return []byte(u.String()), nil }
func (u *URL) UnmarshalText(text []byte) error {
	url, err := url.Parse(string(text))
	if err != nil {
		return err
	}
	u.URL = url
	return nil
}

// AutobahnValidator represents a validator entry in the autobahn config file.
type AutobahnValidator struct {
	ValidatorKey atypes.PublicKey  `json:"validator_key"`
	NodeKey      p2p.NodePublicKey `json:"node_key"`
	Address      tcp.HostPort      `json:"address"`
	// Each validator is assigned a shard of EVM address space.
	// Upon receiving an EVM transaction, a node needs to proxy it
	// to validator owning the shard.
	EVMRPC utils.Option[URL] `json:"evmrpc"`
}

func (av *AutobahnValidator) GetEVMRPC() utils.Option[*url.URL] {
	if u, ok := av.EVMRPC.Get(); ok {
		return utils.Some(u.URL)
	}
	return utils.None[*url.URL]()
}

// AutobahnFileConfig is the JSON structure of the autobahn config file.
type AutobahnFileConfig struct {
	Validators         []AutobahnValidator  `json:"validators"`
	MaxTxsPerBlock     uint64               `json:"max_txs_per_block"`
	MaxTxsPerSecond    utils.Option[uint64] `json:"max_txs_per_second"`
	AllowEmptyBlocks   bool                 `json:"allow_empty_blocks"`
	BlockInterval      utils.Duration       `json:"block_interval"`
	ViewTimeout        utils.Duration       `json:"view_timeout"`
	PersistentStateDir utils.Option[string] `json:"persistent_state_dir"`
	DialInterval       utils.Duration       `json:"dial_interval"`
	// UnsafeTestOnlyDisablePersistence runs the consensus and data layers fully
	// in memory: the no-op persister is installed and no safety state survives a
	// restart. It exists for throwaway test networks only, where a restarted node
	// that equivocates costs nothing. It is mutually exclusive with
	// PersistentStateDir, and it defaults to false so a config written before this
	// field existed, and any config that simply forgets persistent_state_dir, is
	// refused rather than silently running unsafe.
	UnsafeTestOnlyDisablePersistence bool `json:"unsafe_test_only_disable_persistence"`
}

// validatePersistence enforces that the consensus safety state is durable,
// unless the config explicitly opted out of persistence for tests.
func (fc *AutobahnFileConfig) validatePersistence() error {
	dir, ok := fc.PersistentStateDir.Get()
	if !ok {
		if fc.UnsafeTestOnlyDisablePersistence {
			return nil
		}
		return ErrPersistentStateDirRequired
	}
	if fc.UnsafeTestOnlyDisablePersistence {
		return ErrPersistenceOptOutConflict
	}
	if strings.TrimSpace(dir) == "" {
		return ErrPersistentStateDirBlank
	}
	return nil
}

// ConsensusPersistentStateDir returns the directory that the consensus and data
// layers must persist their state to. It returns None only when the config
// explicitly opted out of persistence for tests, and an error whenever the
// config would otherwise install the no-op persister. Callers that construct a
// consensus config use this instead of reading PersistentStateDir directly, so
// that no code path can turn durability off by accident.
func (fc *AutobahnFileConfig) ConsensusPersistentStateDir() (utils.Option[string], error) {
	if err := fc.validatePersistence(); err != nil {
		return utils.None[string](), err
	}
	return fc.PersistentStateDir, nil
}

// Validate performs basic validation of the autobahn file config.
func (fc *AutobahnFileConfig) Validate() error {
	if len(fc.Validators) == 0 {
		return errors.New("validators must not be empty")
	}
	if fc.MaxTxsPerBlock == 0 {
		return errors.New("max_txs_per_block must be > 0")
	}
	if maxTxsPerSecond, ok := fc.MaxTxsPerSecond.Get(); ok {
		if maxTxsPerSecond == 0 {
			return errors.New("max_txs_per_second must be > 0 when set")
		}
		maxBurst := uint64(^uint(0) >> 1)
		blockTxs := min(fc.MaxTxsPerBlock, atypes.MaxTxsPerBlock)
		if maxTxsPerSecond > maxBurst || blockTxs > maxBurst-maxTxsPerSecond {
			return errors.New("max_txs_per_second is too large")
		}
	}
	if fc.BlockInterval <= 0 {
		return errors.New("block_interval must be > 0")
	}
	if fc.ViewTimeout <= 0 {
		return errors.New("view_timeout must be > 0")
	}
	if fc.DialInterval <= 0 {
		return errors.New("dial_interval must be > 0")
	}
	return fc.validatePersistence()
}
