package cli

import (
	"fmt"
	// "strings"

	"github.com/spf13/cobra"

	"github.com/sidiora-labs/paxeer-network/sdk/client"
	// "github.com/sidiora-labs/paxeer-network/sdk/client/flags"
	// sdk "github.com/sidiora-labs/paxeer-network/sdk/types"

	"github.com/sidiora-labs/paxeer-network/modules/epoch/types"
)

// GetQueryCmd returns the cli query commands for this module
func GetQueryCmd(_ string) *cobra.Command {
	// Group epoch queries under a subcommand
	cmd := &cobra.Command{
		Use:                        types.ModuleName,
		Short:                      fmt.Sprintf("Querying commands for the %s module", types.ModuleName),
		DisableFlagParsing:         true,
		SuggestionsMinimumDistance: 2,
		RunE:                       client.ValidateCmd,
	}

	cmd.AddCommand(CmdQueryParams())
	cmd.AddCommand(CmdQueryEpoch())

	// this line is used by starport scaffolding # 1

	return cmd
}
