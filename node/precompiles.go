package app

import (
	launchpadkeeper "github.com/sidiora-labs/paxeer-network/modules/launchpad/keeper"
	layerxbridgekeeper "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/keeper"
	layerxcustodykeeper "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/keeper"
	layerxexchangekeeper "github.com/sidiora-labs/paxeer-network/modules/layerxexchange/keeper"
	xwebkeeper "github.com/sidiora-labs/paxeer-network/modules/xweb/keeper"
	putils "github.com/sidiora-labs/paxeer-network/precompiles/utils"
	"github.com/sidiora-labs/paxeer-network/sdk/client"
	bankkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/bank/keeper"
	govkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/gov/keeper"
	stakingkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/staking/keeper"
	wasmkeeper "github.com/sidiora-labs/paxeer-network/wasm/x/wasm/keeper"
)

type PrecompileKeepers struct {
	putils.BankKeeper
	putils.BankMsgServer
	putils.EVMKeeper
	putils.AccountKeeper
	putils.OracleKeeper
	putils.WasmdKeeper
	putils.WasmdViewKeeper
	putils.StakingKeeper
	putils.StakingQuerier
	putils.GovKeeper
	putils.GovMsgServer
	putils.DistributionKeeper
	putils.TransferKeeper
	putils.ClientKeeper
	putils.ConnectionKeeper
	putils.ChannelKeeper
	putils.AnchorKeeper
	txConf         client.TxConfig
	layerxCustody  *layerxcustodykeeper.Keeper
	layerxExchange *layerxexchangekeeper.Keeper
	layerxBridge   *layerxbridgekeeper.Keeper
	launchpad      *launchpadkeeper.Keeper
	xweb           *xwebkeeper.Keeper
}

func NewPrecompileKeepers(a *App) *PrecompileKeepers {
	return &PrecompileKeepers{
		BankKeeper:         a.BankKeeper,
		BankMsgServer:      bankkeeper.NewMsgServerImpl(a.BankKeeper),
		EVMKeeper:          &a.EvmKeeper,
		AccountKeeper:      a.AccountKeeper,
		OracleKeeper:       a.OracleKeeper,
		WasmdKeeper:        wasmkeeper.NewDefaultPermissionKeeper(a.WasmKeeper),
		WasmdViewKeeper:    a.WasmKeeper,
		StakingKeeper:      stakingkeeper.NewMsgServerImpl(a.StakingKeeper),
		StakingQuerier:     stakingkeeper.Querier{Keeper: a.StakingKeeper},
		GovKeeper:          a.GovKeeper,
		GovMsgServer:       govkeeper.NewMsgServerImpl(a.GovKeeper),
		DistributionKeeper: a.DistrKeeper,
		TransferKeeper:     a.TransferKeeper,
		ClientKeeper:       a.IBCKeeper.ClientKeeper,
		ConnectionKeeper:   a.IBCKeeper.ConnectionKeeper,
		ChannelKeeper:      a.IBCKeeper.ChannelKeeper,
		AnchorKeeper:       a.LayerXAnchorKeeper,
		txConf:             a.GetTxConfig(),
		layerxCustody:      a.LayerXCustodyKeeper,
		layerxExchange:     a.LayerXExchangeKeeper,
		layerxBridge:       &a.LayerXBridgeKeeper,
		launchpad:          a.LaunchpadKeeper,
		xweb:               &a.XWebKeeper,
	}
}

func (pk *PrecompileKeepers) BankK() putils.BankKeeper                 { return pk.BankKeeper }
func (pk *PrecompileKeepers) BankMS() putils.BankMsgServer             { return pk.BankMsgServer }
func (pk *PrecompileKeepers) EVMK() putils.EVMKeeper                   { return pk.EVMKeeper }
func (pk *PrecompileKeepers) AccountK() putils.AccountKeeper           { return pk.AccountKeeper }
func (pk *PrecompileKeepers) OracleK() putils.OracleKeeper             { return pk.OracleKeeper }
func (pk *PrecompileKeepers) WasmdK() putils.WasmdKeeper               { return pk.WasmdKeeper }
func (pk *PrecompileKeepers) WasmdVK() putils.WasmdViewKeeper          { return pk.WasmdViewKeeper }
func (pk *PrecompileKeepers) StakingK() putils.StakingKeeper           { return pk.StakingKeeper }
func (pk *PrecompileKeepers) StakingQ() putils.StakingQuerier          { return pk.StakingQuerier }
func (pk *PrecompileKeepers) GovK() putils.GovKeeper                   { return pk.GovKeeper }
func (pk *PrecompileKeepers) GovMS() putils.GovMsgServer               { return pk.GovMsgServer }
func (pk *PrecompileKeepers) DistributionK() putils.DistributionKeeper { return pk.DistributionKeeper }
func (pk *PrecompileKeepers) TransferK() putils.TransferKeeper         { return pk.TransferKeeper }
func (pk *PrecompileKeepers) ClientK() putils.ClientKeeper             { return pk.ClientKeeper }
func (pk *PrecompileKeepers) ConnectionK() putils.ConnectionKeeper     { return pk.ConnectionKeeper }
func (pk *PrecompileKeepers) ChannelK() putils.ChannelKeeper           { return pk.ChannelKeeper }
func (pk *PrecompileKeepers) AnchorK() putils.AnchorKeeper             { return pk.AnchorKeeper }
func (pk *PrecompileKeepers) TxConfig() client.TxConfig                { return pk.txConf }
func (pk *PrecompileKeepers) LayerXCustodyK() *layerxcustodykeeper.Keeper {
	return pk.layerxCustody
}
func (pk *PrecompileKeepers) LayerXExchangeK() *layerxexchangekeeper.Keeper {
	return pk.layerxExchange
}
func (pk *PrecompileKeepers) LayerXBridgeK() *layerxbridgekeeper.Keeper {
	return pk.layerxBridge
}
func (pk *PrecompileKeepers) LaunchpadK() *launchpadkeeper.Keeper {
	return pk.launchpad
}

// XWebK hands the xweb precompile the application's xweb keeper.
func (pk *PrecompileKeepers) XWebK() *xwebkeeper.Keeper {
	return pk.xweb
}

var _ putils.XWebKeepers = (*PrecompileKeepers)(nil)
