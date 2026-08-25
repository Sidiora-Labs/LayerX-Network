package cmd

import (
	"net/http"
	//nolint:gosec
	_ "net/http/pprof"

	"github.com/sidiora-labs/paxeer-network/wasm/x/wasm"
	wasmkeeper "github.com/sidiora-labs/paxeer-network/wasm/x/wasm/keeper"
	"github.com/spf13/cast"
	"github.com/spf13/cobra"

	"github.com/sidiora-labs/paxeer-network/node"
	"github.com/sidiora-labs/paxeer-network/sdk/baseapp"
	"github.com/sidiora-labs/paxeer-network/sdk/client/flags"
	"github.com/sidiora-labs/paxeer-network/sdk/server"
	"github.com/sidiora-labs/paxeer-network/sdk/store"
	storetypes "github.com/sidiora-labs/paxeer-network/sdk/store/types"
)

//nolint:gosec
func ReplayCmd(defaultNodeHome string) *cobra.Command {
	cmd := &cobra.Command{
		Use:   "ethreplay",
		Short: "replay EVM transactions",
		Long:  "replay EVM transactions",
		RunE: func(cmd *cobra.Command, _ []string) error {

			serverCtx := server.GetServerContextFromCmd(cmd)
			if err := serverCtx.Viper.BindPFlags(cmd.Flags()); err != nil {
				return err
			}
			go func() {
				logger.Info("Listening for profiling at http://localhost:6060/debug/pprof/")
				err := http.ListenAndServe(":6060", nil)
				if err != nil {
					logger.Error("Error from profiling server", "error", err)
				}
			}()

			home := serverCtx.Viper.GetString(flags.FlagHome)

			cache := store.NewCommitKVStoreCacheManager()
			wasmGasRegisterConfig := wasmkeeper.DefaultGasRegisterConfig()
			wasmGasRegisterConfig.GasMultiplier = 21_000_000
			a := app.New(
				nil,
				nil,
				true,
				map[int64]bool{},
				home,
				0,
				true,
				nil,
				app.MakeEncodingConfig(),
				wasm.EnableAllProposals,
				serverCtx.Viper,
				[]wasm.Option{
					wasmkeeper.WithGasRegister(
						wasmkeeper.NewWasmGasRegister(
							wasmGasRegisterConfig,
						),
					),
				},
				app.EmptyAppOptions,
				baseapp.SetPruning(storetypes.PruneEverything),
				baseapp.SetMinGasPrices(cast.ToString(serverCtx.Viper.Get(server.FlagMinGasPrices))),
				baseapp.SetMinRetainBlocks(cast.ToUint64(serverCtx.Viper.Get(server.FlagMinRetainBlocks))),
				baseapp.SetInterBlockCache(cache),
			)
			app.Replay(a)
			return nil
		},
	}

	cmd.Flags().String(flags.FlagHome, defaultNodeHome, "The database home directory")
	cmd.Flags().String(flags.FlagChainID, "pax-chain", "chain ID")

	return cmd
}
