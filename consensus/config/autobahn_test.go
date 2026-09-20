package config

import (
	"encoding/json"
	"net/url"
	"testing"
	"time"

	"github.com/stretchr/testify/require"

	"github.com/sidiora-labs/paxeer-network/consensus/libs/utils"
)

func TestURLJSONReencode(t *testing.T) {
	want := URL{URL: &url.URL{
		Scheme:   "https",
		Host:     "example.com:8545",
		Path:     "/rpc",
		RawQuery: "foo=bar&baz=qux",
	}}

	encoded, err := json.Marshal(want)
	require.NoError(t, err)

	var got URL
	require.NoError(t, json.Unmarshal(encoded, &got))
	require.NotNil(t, got.URL)
	require.Equal(t, want.String(), got.String())
}

// validPersistentFileConfig returns a config that passes every check unrelated
// to persistence, so persistence tests vary one thing at a time.
func validPersistentFileConfig() AutobahnFileConfig {
	return AutobahnFileConfig{
		Validators:         []AutobahnValidator{{}},
		MaxTxsPerBlock:     5_000,
		MaxTxsPerSecond:    utils.None[uint64](),
		AllowEmptyBlocks:   true,
		BlockInterval:      utils.Duration(400 * time.Millisecond),
		ViewTimeout:        utils.Duration(1500 * time.Millisecond),
		PersistentStateDir: utils.Some("data/autobahn"),
		DialInterval:       utils.Duration(10 * time.Second),
	}
}

func TestAutobahnFileConfigValidatePersistence(t *testing.T) {
	for _, tc := range []struct {
		name    string
		mutate  func(*AutobahnFileConfig)
		wantErr error
	}{{
		name:   "state dir set",
		mutate: func(*AutobahnFileConfig) {},
	}, {
		name: "state dir absent",
		mutate: func(fc *AutobahnFileConfig) {
			fc.PersistentStateDir = utils.None[string]()
		},
		wantErr: ErrPersistentStateDirRequired,
	}, {
		name: "state dir absent with explicit test-only opt-out",
		mutate: func(fc *AutobahnFileConfig) {
			fc.PersistentStateDir = utils.None[string]()
			fc.UnsafeTestOnlyDisablePersistence = true
		},
	}, {
		name: "state dir set together with the opt-out",
		mutate: func(fc *AutobahnFileConfig) {
			fc.UnsafeTestOnlyDisablePersistence = true
		},
		wantErr: ErrPersistenceOptOutConflict,
	}, {
		name: "state dir empty",
		mutate: func(fc *AutobahnFileConfig) {
			fc.PersistentStateDir = utils.Some("")
		},
		wantErr: ErrPersistentStateDirBlank,
	}, {
		name: "state dir whitespace only",
		mutate: func(fc *AutobahnFileConfig) {
			fc.PersistentStateDir = utils.Some("  \t ")
		},
		wantErr: ErrPersistentStateDirBlank,
	}} {
		t.Run(tc.name, func(t *testing.T) {
			fc := validPersistentFileConfig()
			tc.mutate(&fc)
			err := fc.Validate()
			if tc.wantErr == nil {
				require.NoError(t, err)
				return
			}
			require.ErrorIs(t, err, tc.wantErr)
		})
	}
}

func TestAutobahnFileConfigConsensusPersistentStateDir(t *testing.T) {
	t.Run("returns the configured dir", func(t *testing.T) {
		fc := validPersistentFileConfig()
		dir, err := fc.ConsensusPersistentStateDir()
		require.NoError(t, err)
		got, ok := dir.Get()
		require.True(t, ok)
		require.Equal(t, "data/autobahn", got)
	})

	t.Run("returns None under the explicit test-only opt-out", func(t *testing.T) {
		fc := validPersistentFileConfig()
		fc.PersistentStateDir = utils.None[string]()
		fc.UnsafeTestOnlyDisablePersistence = true
		dir, err := fc.ConsensusPersistentStateDir()
		require.NoError(t, err)
		require.False(t, dir.IsPresent())
	})

	t.Run("refuses the no-op persister without the opt-out", func(t *testing.T) {
		fc := validPersistentFileConfig()
		fc.PersistentStateDir = utils.None[string]()
		dir, err := fc.ConsensusPersistentStateDir()
		require.ErrorIs(t, err, ErrPersistentStateDirRequired)
		require.False(t, dir.IsPresent())
	})
}

// TestAutobahnFileConfigPersistenceJSON pins the on-disk contract: a config
// file written before the opt-out field existed decodes with the opt-out off
// and is therefore refused, and the opt-out survives a JSON round trip.
func TestAutobahnFileConfigPersistenceJSON(t *testing.T) {
	t.Run("config without either field is refused", func(t *testing.T) {
		encoded, err := json.Marshal(validPersistentFileConfig())
		require.NoError(t, err)
		var raw map[string]json.RawMessage
		require.NoError(t, json.Unmarshal(encoded, &raw))
		delete(raw, "persistent_state_dir")
		delete(raw, "unsafe_test_only_disable_persistence")
		trimmed, err := json.Marshal(raw)
		require.NoError(t, err)

		var fc AutobahnFileConfig
		require.NoError(t, json.Unmarshal(trimmed, &fc))
		require.False(t, fc.PersistentStateDir.IsPresent())
		require.False(t, fc.UnsafeTestOnlyDisablePersistence)
		require.ErrorIs(t, fc.Validate(), ErrPersistentStateDirRequired)
	})

	t.Run("opt-out round trips", func(t *testing.T) {
		want := validPersistentFileConfig()
		want.PersistentStateDir = utils.None[string]()
		want.UnsafeTestOnlyDisablePersistence = true
		encoded, err := json.Marshal(want)
		require.NoError(t, err)

		var got AutobahnFileConfig
		require.NoError(t, json.Unmarshal(encoded, &got))
		require.True(t, got.UnsafeTestOnlyDisablePersistence)
		require.False(t, got.PersistentStateDir.IsPresent())
		require.NoError(t, got.Validate())
	})
}
