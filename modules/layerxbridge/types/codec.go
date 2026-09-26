package types

import (
	"bytes"
	"encoding/json"
	"fmt"

	"github.com/gogo/protobuf/jsonpb"
	"github.com/sidiora-labs/paxeer-network/sdk/codec"
	cdctypes "github.com/sidiora-labs/paxeer-network/sdk/codec/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/types/msgservice"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
)

// RegisterCodec registers the governance messages and the proposal content
// that carries them on the legacy amino codec that signs them in legacy amino
// JSON mode.
func RegisterCodec(cdc *codec.LegacyAmino) {
	cdc.RegisterConcrete(&MsgRegisterChain{}, "layerxbridge/MsgRegisterChain", nil)
	cdc.RegisterConcrete(&MsgSetAttestors{}, "layerxbridge/MsgSetAttestors", nil)
	cdc.RegisterConcrete(&MsgSetCap{}, "layerxbridge/MsgSetCap", nil)
	cdc.RegisterConcrete(&MsgPause{}, "layerxbridge/MsgPause", nil)
	cdc.RegisterConcrete(&MsgUnpause{}, "layerxbridge/MsgUnpause", nil)
	cdc.RegisterConcrete(&MsgRegisterSidioraPair{}, "layerxbridge/MsgRegisterSidioraPair", nil)
	cdc.RegisterConcrete(&BridgeProposal{}, "layerxbridge/BridgeProposal", nil)
}

// RegisterInterfaces registers every governance message as an sdk.Msg under
// its type URL, the Msg service that executes them and BridgeProposal as the
// governance proposal content that carries them.
func RegisterInterfaces(registry cdctypes.InterfaceRegistry) {
	registry.RegisterImplementations((*sdk.Msg)(nil),
		&MsgRegisterChain{},
		&MsgSetAttestors{},
		&MsgSetCap{},
		&MsgPause{},
		&MsgUnpause{},
		&MsgRegisterSidioraPair{},
	)
	registry.RegisterImplementations((*govtypes.Content)(nil), &BridgeProposal{})
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

// Address20 is a protobuf custom type: on the wire it is exactly its twenty
// bytes, and anything of another length is refused.

func (a Address20) Size() int { return len(a) }

func (a Address20) Marshal() ([]byte, error) { return append([]byte(nil), a[:]...), nil }

func (a Address20) MarshalTo(data []byte) (int, error) {
	if len(data) < len(a) {
		return 0, fmt.Errorf("address20: buffer of %d bytes, need %d", len(data), len(a))
	}
	return copy(data, a[:]), nil
}

func (a *Address20) Unmarshal(data []byte) error {
	if len(data) != len(a) {
		return fmt.Errorf("address20: expected %d bytes, got %d", len(a), len(data))
	}
	copy(a[:], data)
	return nil
}

// Chain, Attestor and AttestorSet are the module's state types and the
// protobuf messages the governance messages carry. Their protobuf JSON is the
// JSON the module's state and genesis already use, decoded with unknown
// fields forbidden unless the caller allows them.

func (c Chain) MarshalJSONPB(*jsonpb.Marshaler) ([]byte, error) { return json.Marshal(c) }

func (c *Chain) UnmarshalJSONPB(u *jsonpb.Unmarshaler, raw []byte) error {
	var decoded Chain
	if err := decodeJSONPB(u, raw, &decoded); err != nil {
		return err
	}
	*c = decoded
	return nil
}

func (c Chain) String() string { return jsonString(c) }

func (a Attestor) MarshalJSONPB(*jsonpb.Marshaler) ([]byte, error) { return json.Marshal(a) }

func (a *Attestor) UnmarshalJSONPB(u *jsonpb.Unmarshaler, raw []byte) error {
	var decoded Attestor
	if err := decodeJSONPB(u, raw, &decoded); err != nil {
		return err
	}
	*a = decoded
	return nil
}

func (a Attestor) String() string { return jsonString(a) }

func (s AttestorSet) MarshalJSONPB(*jsonpb.Marshaler) ([]byte, error) { return json.Marshal(s) }

func (s *AttestorSet) UnmarshalJSONPB(u *jsonpb.Unmarshaler, raw []byte) error {
	var decoded AttestorSet
	if err := decodeJSONPB(u, raw, &decoded); err != nil {
		return err
	}
	*s = decoded
	return nil
}

func (s AttestorSet) String() string { return jsonString(s) }

func decodeJSONPB(u *jsonpb.Unmarshaler, raw []byte, out any) error {
	decoder := json.NewDecoder(bytes.NewReader(raw))
	if u == nil || !u.AllowUnknownFields {
		decoder.DisallowUnknownFields()
	}
	if err := decoder.Decode(out); err != nil {
		return err
	}
	if decoder.More() {
		return fmt.Errorf("%T: more than one JSON value", out)
	}
	return nil
}

func jsonString(value any) string {
	encoded, err := json.Marshal(value)
	if err != nil {
		return fmt.Sprintf("%T: %v", value, err)
	}
	return string(encoded)
}
