// Package layerxanchor is the native LayerX anchor: checkpoint registry,
// finality authority, availability record and guarantor bonds. State and
// genesis are plain JSON; every state change is reached through the keeper,
// which the layerxAnchor precompile exposes to the EVM.
package layerxanchor

import (
	"encoding/json"
	"fmt"

	"github.com/gorilla/mux"
	"github.com/grpc-ecosystem/grpc-gateway/runtime"
	abci "github.com/sidiora-labs/paxeer-network/consensus/abci/types"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	cdctypes "github.com/sidiora-labs/paxeer-network/sdk/codec/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/types/module"
	"github.com/spf13/cobra"

	"github.com/sidiora-labs/paxeer-network/modules/layerxanchor/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/layerxanchor/types"
)

var (
	_ module.AppModule      = AppModule{}
	_ module.AppModuleBasic = AppModuleBasic{}
)

type AppModuleBasic struct{}

func (AppModuleBasic) Name() string { return types.ModuleName }

func (AppModuleBasic) RegisterLegacyAminoCodec(_ *codec.LegacyAmino) {}

func (AppModuleBasic) RegisterInterfaces(_ cdctypes.InterfaceRegistry) {}

func (AppModuleBasic) DefaultGenesis(_ codec.JSONCodec) json.RawMessage {
	encoded, err := json.Marshal(types.DefaultGenesis())
	if err != nil {
		panic(err)
	}
	return encoded
}

func (AppModuleBasic) ValidateGenesis(_ codec.JSONCodec, _ client.TxEncodingConfig, bz json.RawMessage) error {
	var genesis types.GenesisState
	if err := json.Unmarshal(bz, &genesis); err != nil {
		return fmt.Errorf("failed to unmarshal %s genesis state: %w", types.ModuleName, err)
	}
	return genesis.Validate()
}

func (am AppModuleBasic) ValidateGenesisStream(cdc codec.JSONCodec, config client.TxEncodingConfig, genesisCh <-chan json.RawMessage) error {
	for genesis := range genesisCh {
		if err := am.ValidateGenesis(cdc, config, genesis); err != nil {
			return err
		}
	}
	return nil
}

func (AppModuleBasic) RegisterRESTRoutes(_ client.Context, _ *mux.Router) {}

func (AppModuleBasic) RegisterGRPCGatewayRoutes(_ client.Context, _ *runtime.ServeMux) {}

func (AppModuleBasic) GetTxCmd() *cobra.Command { return nil }

func (AppModuleBasic) GetQueryCmd() *cobra.Command { return nil }

type AppModule struct {
	AppModuleBasic
	keeper keeper.Keeper
}

func NewAppModule(k keeper.Keeper) AppModule { return AppModule{keeper: k} }

func (AppModule) Route() sdk.Route { return sdk.Route{} }

func (AppModule) QuerierRoute() string { return types.QuerierRoute }

func (AppModule) LegacyQuerierHandler(_ *codec.LegacyAmino) sdk.Querier { return nil }

func (AppModule) RegisterServices(_ module.Configurator) {}

func (am AppModule) RegisterInvariants(registry sdk.InvariantRegistry) {
	keeper.RegisterInvariants(registry, am.keeper)
}

func (am AppModule) InitGenesis(ctx sdk.Context, _ codec.JSONCodec, gs json.RawMessage) []abci.ValidatorUpdate {
	var genesis types.GenesisState
	if err := json.Unmarshal(gs, &genesis); err != nil {
		panic(err)
	}
	am.keeper.InitGenesis(ctx, genesis)
	return []abci.ValidatorUpdate{}
}

func (am AppModule) ExportGenesis(ctx sdk.Context, _ codec.JSONCodec) json.RawMessage {
	encoded, err := json.Marshal(am.keeper.ExportGenesis(ctx))
	if err != nil {
		panic(err)
	}
	return encoded
}

func (am AppModule) ExportGenesisStream(ctx sdk.Context, cdc codec.JSONCodec) <-chan json.RawMessage {
	ch := make(chan json.RawMessage)
	go func() {
		ch <- am.ExportGenesis(ctx, cdc)
		close(ch)
	}()
	return ch
}

func (AppModule) ConsensusVersion() uint64 { return 1 }
