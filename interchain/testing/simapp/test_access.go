package simapp

import (
	"testing"

	"github.com/sidiora-labs/paxeer-network/sdk/baseapp"
	"github.com/sidiora-labs/paxeer-network/sdk/client"

	"github.com/sidiora-labs/paxeer-network/wasm/app/params"

	ibctransferkeeper "github.com/sidiora-labs/paxeer-network/interchain/modules/apps/transfer/keeper"
	ibckeeper "github.com/sidiora-labs/paxeer-network/interchain/modules/core/keeper"
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	bankkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/bank/keeper"
	capabilitykeeper "github.com/sidiora-labs/paxeer-network/sdk/x/capability/keeper"
	distrkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/distribution/keeper"
	evidencekeeper "github.com/sidiora-labs/paxeer-network/sdk/x/evidence/keeper"
	slashingkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/slashing/keeper"
	stakingkeeper "github.com/sidiora-labs/paxeer-network/sdk/x/staking/keeper"
)

type TestSupport struct {
	t   testing.TB
	app *SimApp
}

func NewTestSupport(t testing.TB, app *SimApp) *TestSupport {
	return &TestSupport{t: t, app: app}
}

func (s TestSupport) IBCKeeper() *ibckeeper.Keeper {
	return s.app.IBCKeeper
}

func (s TestSupport) AppCodec() codec.Codec {
	return s.app.appCodec
}

func (s TestSupport) StakingKeeper() stakingkeeper.Keeper {
	return s.app.StakingKeeper
}

func (s TestSupport) BankKeeper() bankkeeper.Keeper {
	return s.app.BankKeeper
}

func (s TestSupport) TransferKeeper() ibctransferkeeper.Keeper {
	return s.app.TransferKeeper
}

func (s TestSupport) CapabilityKeeper() *capabilitykeeper.Keeper {
	return s.app.CapabilityKeeper
}

func (s TestSupport) DistrKeeper() *distrkeeper.Keeper {
	return &s.app.DistrKeeper
}

func (s TestSupport) SlashingKeeper() *slashingkeeper.Keeper {
	return &s.app.SlashingKeeper
}

func (s TestSupport) EvidenceKeeper() *evidencekeeper.Keeper {
	return &s.app.EvidenceKeeper
}

func (s TestSupport) GetBaseApp() *baseapp.BaseApp {
	return s.app.BaseApp
}

func (s TestSupport) GetTxConfig() client.TxConfig {
	return params.MakeEncodingConfig().TxConfig
}
