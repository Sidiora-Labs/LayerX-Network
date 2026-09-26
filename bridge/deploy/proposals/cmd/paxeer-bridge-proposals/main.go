// Command paxeer-bridge-proposals reads one chain configuration and the
// committed attestor-set manifest and writes the governance bodies that open
// that chain on Paxeer X Network into an output directory.
//
// It refuses a placeholder or zero authority, owner or attestor, a zero
// threshold or one above the attestor count and a zero cap, naming the file and
// the field, and it writes no partial output when it refuses.
//
//	paxeer-bridge-proposals [-manifest <path>] <chain-configuration.json> <output-directory>
package main

import (
	"fmt"
	"os"

	"github.com/sidiora-labs/paxeer-network/bridge/deploy/proposals"
)

func main() {
	if err := proposals.Run(os.Args[1:], os.Stdout); err != nil {
		fmt.Fprintf(os.Stderr, "%s: %v\n", proposals.CommandName, err)
		os.Exit(1)
	}
}
