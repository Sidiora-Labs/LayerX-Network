package types

import (
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	cdctypes "github.com/sidiora-labs/paxeer-network/sdk/codec/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/types/msgservice"
)

func RegisterCodec(cdc *codec.LegacyAmino) {
	cdc.RegisterConcrete(&MsgUpdateParams{}, "layerxcustody/MsgUpdateParams", nil)
	cdc.RegisterConcrete(&MsgSetAsset{}, "layerxcustody/MsgSetAsset", nil)
	cdc.RegisterConcrete(&MsgRegisterCheckpoint{}, "layerxcustody/MsgRegisterCheckpoint", nil)
	cdc.RegisterConcrete(&MsgSetEmergency{}, "layerxcustody/MsgSetEmergency", nil)
	cdc.RegisterConcrete(&MsgCancelClaim{}, "layerxcustody/MsgCancelClaim", nil)
	cdc.RegisterConcrete(&MsgRequestWithdrawal{}, "layerxcustody/MsgRequestWithdrawal", nil)
	cdc.RegisterConcrete(&MsgFinaliseWithdrawal{}, "layerxcustody/MsgFinaliseWithdrawal", nil)
	cdc.RegisterConcrete(&MsgRequestForcedExit{}, "layerxcustody/MsgRequestForcedExit", nil)
	cdc.RegisterConcrete(&MsgExecuteForcedExit{}, "layerxcustody/MsgExecuteForcedExit", nil)
}

func RegisterInterfaces(registry cdctypes.InterfaceRegistry) {
	registry.RegisterImplementations((*sdk.Msg)(nil),
		&MsgUpdateParams{},
		&MsgSetAsset{},
		&MsgRegisterCheckpoint{},
		&MsgSetEmergency{},
		&MsgCancelClaim{},
		&MsgRequestWithdrawal{},
		&MsgFinaliseWithdrawal{},
		&MsgRequestForcedExit{},
		&MsgExecuteForcedExit{},
	)
	msgservice.RegisterMsgServiceDesc(registry, &_Msg_serviceDesc)
}

var (
	amino     = codec.NewLegacyAmino()
	ModuleCdc = codec.NewAminoCodec(amino)
)

func init() {
	RegisterCodec(amino)
	sdk.RegisterLegacyAminoCodec(amino)
	amino.Seal()
}
