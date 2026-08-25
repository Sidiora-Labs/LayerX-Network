package simulation

import (
	"math/rand"

	paxappparams "github.com/sidiora-labs/paxeer-network/node/params"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	simtypes "github.com/sidiora-labs/paxeer-network/sdk/types/simulation"
	"github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/simulation"
)

// OpWeightSubmitTextProposal app params key for text proposal
const OpWeightSubmitTextProposal = "op_weight_submit_text_proposal"

// ProposalContents defines the module weighted proposals' contents
func ProposalContents() []simtypes.WeightedProposalContent {
	return []simtypes.WeightedProposalContent{
		simulation.NewWeightedProposalContent(
			OpWeightMsgDeposit,
			paxappparams.DefaultWeightTextProposal,
			SimulateTextProposalContent,
		),
	}
}

// SimulateTextProposalContent returns a random text proposal content.
func SimulateTextProposalContent(r *rand.Rand, _ sdk.Context, _ []simtypes.Account) simtypes.Content {
	return types.NewTextProposal(
		simtypes.RandStringOfLength(r, 140),
		simtypes.RandStringOfLength(r, 5000),
		false,
	)
}
