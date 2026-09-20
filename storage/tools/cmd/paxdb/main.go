package main

import (
	"fmt"
	"os"

	"github.com/sidiora-labs/paxeer-network/storage/tools/cmd/paxdb/benchmark"
	"github.com/sidiora-labs/paxeer-network/storage/tools/cmd/paxdb/operations"
	"github.com/spf13/cobra"
)

func main() {
	rootCmd := &cobra.Command{
		Use:   "paxdb",
		Short: "A tool to generate raw key value data from a node as well as benchmark different backends",
	}

	rootCmd.AddCommand(
		benchmark.GenerateCmd(),
		benchmark.DBWriteCmd(),
		benchmark.DBRandomReadCmd(),
		benchmark.DBIterationCmd(),
		benchmark.DBReverseIterationCmd(),
		operations.DumpDbCmd(),
		operations.PruneCmd(),
		operations.DumpIAVLCmd(),
		operations.DumpFlatKVCmd(),
		operations.StateSizeCmd(),
		operations.MemiavlLatestVersionCmd(),
		operations.ImportFlatKVFromMemiavlCmd(),
		operations.ReplayChangelogCmd(),
		operations.TraceProfileReportCmd(),
		operations.MigrateEvmStatusCmd())
	if err := rootCmd.Execute(); err != nil {
		fmt.Println(err)
		os.Exit(1)
	}
}
