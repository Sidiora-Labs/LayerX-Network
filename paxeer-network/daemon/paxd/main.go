package main

import (
	"os"

	"github.com/sidiora-labs/paxeer-network/daemon/paxd/cmd"
	"github.com/sidiora-labs/paxeer-network/node/params"

	app "github.com/sidiora-labs/paxeer-network/node"
	svrcmd "github.com/sidiora-labs/paxeer-network/sdk/server/cmd"
)

func main() {
	params.SetAddressPrefixes()
	rootCmd, _ := cmd.NewRootCmd()
	if err := svrcmd.Execute(rootCmd, app.DefaultNodeHome); err != nil {
		os.Exit(1)
	}
}
