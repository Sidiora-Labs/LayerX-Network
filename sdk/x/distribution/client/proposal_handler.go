package client

import (
	"github.com/sidiora-labs/paxeer-network/sdk/x/distribution/client/cli"
	"github.com/sidiora-labs/paxeer-network/sdk/x/distribution/client/rest"
	govclient "github.com/sidiora-labs/paxeer-network/sdk/x/gov/client"
)

// ProposalHandler is the community spend proposal handler.
var (
	ProposalHandler = govclient.NewProposalHandler(cli.GetCmdSubmitProposal, rest.ProposalRESTHandler)
)
