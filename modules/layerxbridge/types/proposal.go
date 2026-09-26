package types

import (
	"fmt"
	"strings"

	cdctypes "github.com/sidiora-labs/paxeer-network/sdk/codec/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
)

// ProposalTypeBridge is the governance proposal type of BridgeProposal.
const ProposalTypeBridge = "LayerXBridge"

var (
	_ govtypes.Content                 = &BridgeProposal{}
	_ cdctypes.UnpackInterfacesMessage = &BridgeProposal{}
)

func init() {
	govtypes.RegisterProposalType(ProposalTypeBridge)
	// The governance module signs MsgSubmitProposal with its own amino codec,
	// so the content and every message it can carry are registered there too.
	govtypes.RegisterProposalTypeCodec(&BridgeProposal{}, "layerxbridge/BridgeProposal")
	govtypes.RegisterProposalTypeCodec(&MsgRegisterChain{}, "layerxbridge/MsgRegisterChain")
	govtypes.RegisterProposalTypeCodec(&MsgSetAttestors{}, "layerxbridge/MsgSetAttestors")
	govtypes.RegisterProposalTypeCodec(&MsgSetCap{}, "layerxbridge/MsgSetCap")
	govtypes.RegisterProposalTypeCodec(&MsgPause{}, "layerxbridge/MsgPause")
	govtypes.RegisterProposalTypeCodec(&MsgUnpause{}, "layerxbridge/MsgUnpause")
}

// NewBridgeProposal packs the governance messages under their type URLs into
// one proposal, in the order they are to be executed.
func NewBridgeProposal(title, description string, msgs ...sdk.Msg) (*BridgeProposal, error) {
	packed := make([]*cdctypes.Any, 0, len(msgs))
	for i, msg := range msgs {
		if _, err := governanceMessage(msg); err != nil {
			return nil, sdkerrors.Wrapf(err, "message %d", i)
		}
		value, err := cdctypes.NewAnyWithValue(msg)
		if err != nil {
			return nil, sdkerrors.Wrapf(govtypes.ErrInvalidProposalContent, "message %d: %v", i, err)
		}
		packed = append(packed, value)
	}
	return &BridgeProposal{Title: title, Description: description, Messages: packed}, nil
}

func (p *BridgeProposal) GetTitle() string { return p.Title }

func (p *BridgeProposal) GetDescription() string { return p.Description }

func (p *BridgeProposal) ProposalRoute() string { return RouterKey }

func (p *BridgeProposal) ProposalType() string { return ProposalTypeBridge }

// ValidateBasic refuses a proposal without a title or a description, one that
// carries no message, and one carrying anything but the module's governance
// messages, a message that fails its own ValidateBasic, or a message whose
// authority is not the governance module account that executes it.
func (p *BridgeProposal) ValidateBasic() error {
	if err := govtypes.ValidateAbstract(p); err != nil {
		return err
	}
	msgs, err := p.GetMessages()
	if err != nil {
		return err
	}
	if len(msgs) == 0 {
		return sdkerrors.Wrap(govtypes.ErrInvalidProposalContent, "the proposal carries no message")
	}
	governance := DefaultAuthority()
	for i, msg := range msgs {
		if err := msg.ValidateBasic(); err != nil {
			return sdkerrors.Wrapf(err, "message %d (%s)", i, sdk.MsgTypeURL(msg))
		}
		authority, err := governanceMessage(msg)
		if err != nil {
			return sdkerrors.Wrapf(err, "message %d", i)
		}
		if authority != governance {
			return ErrUnauthorized.Wrapf("message %d (%s): authority %s is not the governance module account %s",
				i, sdk.MsgTypeURL(msg), authority, governance)
		}
	}
	return nil
}

// GetMessages returns the carried messages in order. It refuses a message
// that was not unpacked through an interface registry or that is not one of
// the module's governance messages.
func (p *BridgeProposal) GetMessages() ([]sdk.Msg, error) {
	msgs := make([]sdk.Msg, 0, len(p.Messages))
	for i, packed := range p.Messages {
		if packed == nil {
			return nil, sdkerrors.Wrapf(govtypes.ErrInvalidProposalContent, "message %d is empty", i)
		}
		msg, ok := packed.GetCachedValue().(sdk.Msg)
		if !ok {
			return nil, sdkerrors.Wrapf(govtypes.ErrInvalidProposalContent,
				"message %d (%s) is not an unpacked message", i, packed.TypeUrl)
		}
		if _, err := governanceMessage(msg); err != nil {
			return nil, sdkerrors.Wrapf(err, "message %d", i)
		}
		msgs = append(msgs, msg)
	}
	return msgs, nil
}

// UnpackInterfaces resolves every carried message against the registry.
func (p BridgeProposal) UnpackInterfaces(unpacker cdctypes.AnyUnpacker) error {
	for _, packed := range p.Messages {
		var msg sdk.Msg
		if err := unpacker.UnpackAny(packed, &msg); err != nil {
			return err
		}
	}
	return nil
}

func (p BridgeProposal) String() string {
	var b strings.Builder
	fmt.Fprintf(&b, "Bridge Proposal:\n  Title:       %s\n  Description: %s\n  Messages:\n", p.Title, p.Description)
	for i, packed := range p.Messages {
		if packed == nil {
			fmt.Fprintf(&b, "    %d: empty\n", i)
			continue
		}
		if msg, ok := packed.GetCachedValue().(sdk.Msg); ok {
			fmt.Fprintf(&b, "    %d: %s %s\n", i, packed.TypeUrl, msg.String())
			continue
		}
		fmt.Fprintf(&b, "    %d: %s\n", i, packed.TypeUrl)
	}
	return b.String()
}

// governanceMessage returns the authority of one of the module's governance
// messages and refuses every other message.
func governanceMessage(msg sdk.Msg) (string, error) {
	switch m := msg.(type) {
	case *MsgRegisterChain:
		return m.Authority, nil
	case *MsgSetAttestors:
		return m.Authority, nil
	case *MsgSetCap:
		return m.Authority, nil
	case *MsgPause:
		return m.Authority, nil
	case *MsgUnpause:
		return m.Authority, nil
	default:
		return "", sdkerrors.Wrapf(govtypes.ErrInvalidProposalContent,
			"%T is not a %s governance message", msg, ModuleName)
	}
}
