package wasmbinding

import (
	epochwasm "github.com/sidiora-labs/paxeer-network/modules/epoch/client/wasm"
	epochkeeper "github.com/sidiora-labs/paxeer-network/modules/epoch/keeper"
	evmwasm "github.com/sidiora-labs/paxeer-network/modules/evm/client/wasm"
	evmkeeper "github.com/sidiora-labs/paxeer-network/modules/evm/keeper"
	oraclewasm "github.com/sidiora-labs/paxeer-network/modules/oracle/client/wasm"
	oraclekeeper "github.com/sidiora-labs/paxeer-network/modules/oracle/keeper"
	tokenfactorywasm "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/client/wasm"
	tokenfactorykeeper "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/keeper"
	codectypes "github.com/sidiora-labs/paxeer-network/sdk/codec/types"
	authkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/auth/keeper"
	stakingkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/staking/keeper"
	"github.com/sidiora-labs/paxeer-network/wasm/x/wasm"
	wasmkeeper "github.com/sidiora-labs/paxeer-network/wasm/x/wasm/keeper"
	wasmtypes "github.com/sidiora-labs/paxeer-network/wasm/x/wasm/types"
)

func RegisterCustomPlugins(
	oracle *oraclekeeper.Keeper,
	epoch *epochkeeper.Keeper,
	tokenfactory *tokenfactorykeeper.Keeper,
	_ *authkeeper.AccountKeeper,
	router wasmkeeper.MessageRouter,
	channelKeeper wasmtypes.ChannelKeeper,
	capabilityKeeper wasmtypes.CapabilityKeeper,
	bankKeeper wasmtypes.Burner,
	unpacker codectypes.AnyUnpacker,
	portSource wasmtypes.ICS20TransferPortSource,
	evmKeeper *evmkeeper.Keeper,
	stakingKeeper stakingkeeper.Keeper,
) []wasmkeeper.Option {
	oracleHandler := oraclewasm.NewOracleWasmQueryHandler(oracle)
	epochHandler := epochwasm.NewEpochWasmQueryHandler(epoch)
	tokenfactoryHandler := tokenfactorywasm.NewTokenFactoryWasmQueryHandler(tokenfactory)
	evmHandler := evmwasm.NewEVMQueryHandler(evmKeeper)
	wasmQueryPlugin := NewQueryPlugin(oracleHandler, epochHandler, tokenfactoryHandler, evmHandler, stakingKeeper)

	queryPluginOpt := wasmkeeper.WithQueryPlugins(&wasmkeeper.QueryPlugins{
		Custom: CustomQuerier(wasmQueryPlugin),
	})
	messengerHandlerOpt := wasmkeeper.WithMessageHandler(
		CustomMessageHandler(router, channelKeeper, capabilityKeeper, bankKeeper, evmKeeper, unpacker, portSource),
	)

	return []wasm.Option{
		queryPluginOpt,
		messengerHandlerOpt,
	}
}
