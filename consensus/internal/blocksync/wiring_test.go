package blocksync_test

import (
	"testing"

	"github.com/stretchr/testify/require"

	"github.com/sidiora-labs/paxeer-network/consensus/internal/protoutils/wireguard/wgtest"
	bcproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/blocksync"
	tmproto "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
)

// TestWiring_BlocksyncChannel asserts that the blocksync message type
// implements WireguardScan and rejects an over-cap last_commit payload.
func TestWiring_BlocksyncChannel(t *testing.T) {
	msg := &bcproto.Message{Sum: &bcproto.Message_BlockResponse{
		BlockResponse: &bcproto.BlockResponse{
			Block: &tmproto.Block{LastCommit: wgtest.CommitWith(wgtest.MaxCommitSignatures + 1)},
		},
	}}
	require.Error(t, msg.WireguardScan(wgtest.Marshal(t, msg)),
		"blocksync Message.WireguardScan failed to reject an over-cap last_commit")
}
