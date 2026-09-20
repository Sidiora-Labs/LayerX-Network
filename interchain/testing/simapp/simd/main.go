package main

import (
	"os"

	"github.com/sidiora-labs/paxeer-network/sdk/server"
	svrcmd "github.com/sidiora-labs/paxeer-network/sdk/server/cmd"

	"github.com/sidiora-labs/paxeer-network/interchain/testing/simapp"
	"github.com/sidiora-labs/paxeer-network/interchain/testing/simapp/simd/cmd"
)

func main() {
	rootCmd, _ := cmd.NewRootCmd()

	if err := svrcmd.Execute(rootCmd, simapp.DefaultNodeHome); err != nil {
		switch e := err.(type) {
		case server.ErrorCode:
			os.Exit(e.Code)

		default:
			os.Exit(1)
		}
	}
}
