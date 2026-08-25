package types

import (
	"fmt"

	"github.com/sidiora-labs/paxeer-network/consensus/internal/autobahn/pb"
	"github.com/sidiora-labs/paxeer-network/consensus/internal/protoutils"
	"github.com/sidiora-labs/paxeer-network/consensus/libs/utils"
)

// PrepareVote .
type PrepareVote struct {
	utils.ReadOnly
	proposal *Proposal
}

// NewPrepareVote creates a new PrepareVote.
func NewPrepareVote(proposal *Proposal) *PrepareVote {
	return &PrepareVote{proposal: proposal}
}

// Proposal .
func (m *PrepareVote) Proposal() *Proposal { return m.proposal }

// PrepareVoteConv is the protobuf converter for PrepareVote.
var PrepareVoteConv = protoutils.Conv[*PrepareVote, *pb.Proposal]{
	Encode: func(m *PrepareVote) *pb.Proposal {
		return ProposalConv.Encode(m.proposal)
	},
	Decode: func(m *pb.Proposal) (*PrepareVote, error) {
		proposal, err := ProposalConv.DecodeReq(m)
		if err != nil {
			return nil, fmt.Errorf("proposal: %w", err)
		}
		return &PrepareVote{proposal: proposal}, nil
	},
}
