package client

import (
	govclient "github.com/sidiora-labs/paxeer-network/sdk/x/gov/client"
	"github.com/sidiora-labs/paxeer-network/sdk/x/params/client/cli"
	"github.com/sidiora-labs/paxeer-network/sdk/x/params/client/rest"
)

// ProposalHandler is the param change proposal handler.
var ProposalHandler = govclient.NewProposalHandler(cli.NewSubmitParamChangeProposalTxCmd, rest.ProposalRESTHandler)
