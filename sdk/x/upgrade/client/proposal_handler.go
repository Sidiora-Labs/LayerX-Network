package client

import (
	govclient "github.com/sidiora-labs/paxeer-network/sdk/x/gov/client"
	"github.com/sidiora-labs/paxeer-network/sdk/x/upgrade/client/cli"
	"github.com/sidiora-labs/paxeer-network/sdk/x/upgrade/client/rest"
)

var ProposalHandler = govclient.NewProposalHandler(cli.NewCmdSubmitUpgradeProposal, rest.ProposalRESTHandler)
var CancelProposalHandler = govclient.NewProposalHandler(cli.NewCmdSubmitCancelUpgradeProposal, rest.ProposalCancelRESTHandler)
