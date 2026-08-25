package bindings

import "github.com/sidiora-labs/paxeer-network/modules/epoch/types"

type PaxEpochQuery struct {
	// queries the current Epoch
	Epoch *types.QueryEpochRequest `json:"epoch,omitempty"`
}
