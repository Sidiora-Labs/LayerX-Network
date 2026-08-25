package app

import (
	"time"

	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
)

type OptimisticProcessingInfo struct {
	Height     int64
	Hash       []byte
	Aborted    bool
	Completion chan struct{}
	// result fields
	Events       []abci.Event
	TxRes        []*abci.ExecTxResult
	EndBlockResp abci.ResponseEndBlock
}

type BlockProcessRequest struct {
	Hash                []byte
	ByzantineValidators []abci.Misbehavior
	Height              int64
	Time                time.Time
}
