package factory

import (
	"testing"

	"github.com/sidiora-labs/paxeer-network/consensus/types"
)

func TestMakeHeader(t *testing.T) {
	MakeHeader(&types.Header{})
}

func TestRandomNodeID(t *testing.T) {
	RandomNodeID(t)
}
