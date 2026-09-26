package types

import (
	"errors"
	"fmt"
	"math"
	"strings"

	"github.com/ethereum/go-ethereum/common"
	cdctypes "github.com/sidiora-labs/paxeer-network/sdk/codec/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
)

const (
	ProposalTypeAddERCNativePointer   = "AddERCNativePointer"
	ProposalTypeAddERCCW20Pointer     = "AddERCCW20Pointer"
	ProposalTypeAddERCCW721Pointer    = "AddERCCW721Pointer"
	ProposalTypeAddERCCW1155Pointer   = "AddERCCW1155Pointer"
	ProposalTypeAddCWERC20Pointer     = "AddCWERC20Pointer"
	ProposalTypeAddCWERC721Pointer    = "AddCWERC721Pointer"
	ProposalTypeAddCWERC1155Pointer   = "AddCWERC1155Pointer"
	ProposalTypeAddERCNativePointerV2 = "AddERCNativePointerV2"
	ProposalTypePointerBinding        = "PointerBinding"
)

var (
	_ govtypes.Content                 = &PointerBindingProposal{}
	_ cdctypes.UnpackInterfacesMessage = &PointerBindingProposal{}
)

func init() {
	// for routing
	govtypes.RegisterProposalType(ProposalTypeAddERCNativePointer)
	govtypes.RegisterProposalType(ProposalTypeAddERCCW20Pointer)
	govtypes.RegisterProposalType(ProposalTypeAddERCCW721Pointer)
	govtypes.RegisterProposalType(ProposalTypeAddERCCW1155Pointer)
	govtypes.RegisterProposalType(ProposalTypeAddCWERC20Pointer)
	govtypes.RegisterProposalType(ProposalTypeAddCWERC721Pointer)
	govtypes.RegisterProposalType(ProposalTypeAddCWERC1155Pointer)
	govtypes.RegisterProposalType(ProposalTypeAddERCNativePointerV2)
	govtypes.RegisterProposalType(ProposalTypePointerBinding)

	// for marshal and unmarshal
	govtypes.RegisterProposalTypeCodec(&AddERCNativePointerProposal{}, "evm/AddERCNativePointerProposal")
	govtypes.RegisterProposalTypeCodec(&AddERCCW20PointerProposal{}, "evm/AddERCCW20PointerProposal")
	govtypes.RegisterProposalTypeCodec(&AddERCCW721PointerProposal{}, "evm/AddERCCW721PointerProposal")
	govtypes.RegisterProposalTypeCodec(&AddERCCW1155PointerProposal{}, "evm/AddERCCW1155PointerProposal")
	govtypes.RegisterProposalTypeCodec(&AddCWERC20PointerProposal{}, "evm/AddCWERC20PointerProposal")
	govtypes.RegisterProposalTypeCodec(&AddCWERC721PointerProposal{}, "evm/AddCWERC721PointerProposal")
	govtypes.RegisterProposalTypeCodec(&AddCWERC1155PointerProposal{}, "evm/AddCWERC1155PointerProposal")
	govtypes.RegisterProposalTypeCodec(&AddERCNativePointerProposalV2{}, "evm/AddERCNativePointerProposalV2")
	// The governance module signs MsgSubmitProposal with its own amino codec,
	// so the binding proposal and every message it can carry are registered
	// there too.
	govtypes.RegisterProposalTypeCodec(&PointerBindingProposal{}, "evm/PointerBindingProposal")
	govtypes.RegisterProposalTypeCodec(&MsgBindERCNativePointer{}, "evm/MsgBindERCNativePointer")
}

func (p *AddERCNativePointerProposal) GetTitle() string { return p.Title }

func (p *AddERCNativePointerProposal) GetDescription() string { return p.Description }

func (p *AddERCNativePointerProposal) ProposalRoute() string { return RouterKey }

func (p *AddERCNativePointerProposal) ProposalType() string {
	return ProposalTypeAddERCNativePointer
}

func (p *AddERCNativePointerProposal) ValidateBasic() error {
	if p.Pointer != "" && !common.IsHexAddress(p.Pointer) {
		return errors.New("pointer address must be either empty or a valid hex-encoded string")
	}

	if p.Version > math.MaxUint16 {
		return errors.New("pointer version must be <= 65535")
	}

	return govtypes.ValidateAbstract(p)
}

func (p AddERCNativePointerProposal) String() string {
	var b strings.Builder
	b.WriteString(fmt.Sprintf(`Add ERC native pointer Proposal:
  Title:       %s
  Description: %s
  Token:       %s
  Pointer:     %s
  Version:     %d
`, p.Title, p.Description, p.Token, p.Pointer, p.Version))
	return b.String()
}

func (p *AddERCCW20PointerProposal) GetTitle() string { return p.Title }

func (p *AddERCCW20PointerProposal) GetDescription() string { return p.Description }

func (p *AddERCCW20PointerProposal) ProposalRoute() string { return RouterKey }

func (p *AddERCCW20PointerProposal) ProposalType() string {
	return ProposalTypeAddERCCW20Pointer
}

func (p *AddERCCW20PointerProposal) ValidateBasic() error {
	if _, err := sdk.AccAddressFromBech32(p.Pointee); err != nil {
		return err
	}

	if p.Pointer != "" && !common.IsHexAddress(p.Pointer) {
		return errors.New("pointer address must be either empty or a valid hex-encoded string")
	}

	if p.Version > math.MaxUint16 {
		return errors.New("pointer version must be <= 65535")
	}

	return govtypes.ValidateAbstract(p)
}

func (p AddERCCW20PointerProposal) String() string {
	var b strings.Builder
	b.WriteString(fmt.Sprintf(`Add ERC CW20 pointer Proposal:
  Title:       %s
  Description: %s
  Pointee:     %s
  Pointer:     %s
  Version:     %d
`, p.Title, p.Description, p.Pointee, p.Pointer, p.Version))
	return b.String()
}

func (p *AddERCCW721PointerProposal) GetTitle() string { return p.Title }

func (p *AddERCCW721PointerProposal) GetDescription() string { return p.Description }

func (p *AddERCCW721PointerProposal) ProposalRoute() string { return RouterKey }

func (p *AddERCCW721PointerProposal) ProposalType() string {
	return ProposalTypeAddERCCW721Pointer
}

func (p *AddERCCW721PointerProposal) ValidateBasic() error {
	if _, err := sdk.AccAddressFromBech32(p.Pointee); err != nil {
		return err
	}

	if p.Pointer != "" && !common.IsHexAddress(p.Pointer) {
		return errors.New("pointer address must be either empty or a valid hex-encoded string")
	}

	if p.Version > math.MaxUint16 {
		return errors.New("pointer version must be <= 65535")
	}

	return govtypes.ValidateAbstract(p)
}

func (p AddERCCW721PointerProposal) String() string {
	var b strings.Builder
	b.WriteString(fmt.Sprintf(`Add ERC CW721 pointer Proposal:
  Title:       %s
  Description: %s
  Pointee:     %s
  Pointer:     %s
  Version:     %d
`, p.Title, p.Description, p.Pointee, p.Pointer, p.Version))
	return b.String()
}

func (p *AddERCCW1155PointerProposal) GetTitle() string { return p.Title }

func (p *AddERCCW1155PointerProposal) GetDescription() string { return p.Description }

func (p *AddERCCW1155PointerProposal) ProposalRoute() string { return RouterKey }

func (p *AddERCCW1155PointerProposal) ProposalType() string {
	return ProposalTypeAddERCCW1155Pointer
}

func (p *AddERCCW1155PointerProposal) ValidateBasic() error {
	if _, err := sdk.AccAddressFromBech32(p.Pointee); err != nil {
		return err
	}

	if p.Pointer != "" && !common.IsHexAddress(p.Pointer) {
		return errors.New("pointer address must be either empty or a valid hex-encoded string")
	}

	if p.Version > math.MaxUint16 {
		return errors.New("pointer version must be <= 65535")
	}

	return govtypes.ValidateAbstract(p)
}

func (p AddERCCW1155PointerProposal) String() string {
	var b strings.Builder
	b.WriteString(fmt.Sprintf(`Add ERC CW1155 pointer Proposal:
  Title:       %s
  Description: %s
  Pointee:     %s
  Pointer:     %s
  Version:     %d
`, p.Title, p.Description, p.Pointee, p.Pointer, p.Version))
	return b.String()
}

func (p *AddCWERC20PointerProposal) GetTitle() string { return p.Title }

func (p *AddCWERC20PointerProposal) GetDescription() string { return p.Description }

func (p *AddCWERC20PointerProposal) ProposalRoute() string { return RouterKey }

func (p *AddCWERC20PointerProposal) ProposalType() string {
	return ProposalTypeAddCWERC20Pointer
}

func (p *AddCWERC20PointerProposal) ValidateBasic() error {
	if p.Pointer != "" {
		if _, err := sdk.AccAddressFromBech32(p.Pointer); err != nil {
			return err
		}
	}
	if !common.IsHexAddress(p.Pointee) {
		return errors.New("pointee address must be either empty or a valid hex-encoded string")
	}

	if p.Version > math.MaxUint16 {
		return errors.New("pointer version must be <= 65535")
	}

	return govtypes.ValidateAbstract(p)
}

func (p AddCWERC20PointerProposal) String() string {
	var b strings.Builder
	b.WriteString(fmt.Sprintf(`Add CW ERC20 pointer Proposal:
  Title:       %s
  Description: %s
  Pointee:     %s
  Pointer:     %s
  Version:     %d
`, p.Title, p.Description, p.Pointee, p.Pointer, p.Version))
	return b.String()
}

func (p *AddCWERC721PointerProposal) GetTitle() string { return p.Title }

func (p *AddCWERC721PointerProposal) GetDescription() string { return p.Description }

func (p *AddCWERC721PointerProposal) ProposalRoute() string { return RouterKey }

func (p *AddCWERC721PointerProposal) ProposalType() string {
	return ProposalTypeAddCWERC721Pointer
}

func (p *AddCWERC721PointerProposal) ValidateBasic() error {
	if p.Pointer != "" {
		if _, err := sdk.AccAddressFromBech32(p.Pointer); err != nil {
			return err
		}
	}
	if !common.IsHexAddress(p.Pointee) {
		return errors.New("pointee address must be either empty or a valid hex-encoded string")
	}

	if p.Version > math.MaxUint16 {
		return errors.New("pointer version must be <= 65535")
	}

	return govtypes.ValidateAbstract(p)
}

func (p AddCWERC721PointerProposal) String() string {
	var b strings.Builder
	b.WriteString(fmt.Sprintf(`Add CW ERC721 pointer Proposal:
  Title:       %s
  Description: %s
  Pointee:     %s
  Pointer:     %s
  Version:     %d
`, p.Title, p.Description, p.Pointee, p.Pointer, p.Version))
	return b.String()
}

func (p *AddCWERC1155PointerProposal) GetTitle() string { return p.Title }

func (p *AddCWERC1155PointerProposal) GetDescription() string { return p.Description }

func (p *AddCWERC1155PointerProposal) ProposalRoute() string { return RouterKey }

func (p *AddCWERC1155PointerProposal) ProposalType() string {
	return ProposalTypeAddCWERC1155Pointer
}

func (p *AddCWERC1155PointerProposal) ValidateBasic() error {
	if p.Pointer != "" {
		if _, err := sdk.AccAddressFromBech32(p.Pointer); err != nil {
			return err
		}
	}
	if !common.IsHexAddress(p.Pointee) {
		return errors.New("pointee address must be either empty or a valid hex-encoded string")
	}

	if p.Version > math.MaxUint16 {
		return errors.New("pointer version must be <= 65535")
	}

	return govtypes.ValidateAbstract(p)
}

func (p AddCWERC1155PointerProposal) String() string {
	var b strings.Builder
	b.WriteString(fmt.Sprintf(`Add CW ERC1155 pointer Proposal:
  Title:       %s
  Description: %s
  Pointee:     %s
  Pointer:     %s
  Version:     %d
`, p.Title, p.Description, p.Pointee, p.Pointer, p.Version))
	return b.String()
}

func (p *AddERCNativePointerProposalV2) GetTitle() string { return p.Title }

func (p *AddERCNativePointerProposalV2) GetDescription() string { return p.Description }

func (p *AddERCNativePointerProposalV2) ProposalRoute() string { return RouterKey }

func (p *AddERCNativePointerProposalV2) ProposalType() string {
	return ProposalTypeAddERCNativePointerV2
}

func (p *AddERCNativePointerProposalV2) ValidateBasic() error {
	if p.Decimals > math.MaxUint8 {
		return errors.New("pointer version must be <= 255")
	}

	return govtypes.ValidateAbstract(p)
}

func (p AddERCNativePointerProposalV2) String() string {
	var b strings.Builder
	b.WriteString(fmt.Sprintf(`Add ERC native pointer Proposal V2:
  Title:       %s
  Description: %s
  Token:       %s
  Name:        %s
  Symbol:      %s
  Decimals:    %d
`, p.Title, p.Description, p.Token, p.Name, p.Symbol, p.Decimals))
	return b.String()
}

// NewPointerBindingProposal packs the evm governance messages under their type
// URLs into one proposal, in the order they are to be executed.
func NewPointerBindingProposal(title, description string, msgs ...sdk.Msg) (*PointerBindingProposal, error) {
	packed := make([]*cdctypes.Any, 0, len(msgs))
	for i, msg := range msgs {
		if _, err := pointerGovernanceMessage(msg); err != nil {
			return nil, sdkerrors.Wrapf(err, "message %d", i)
		}
		value, err := cdctypes.NewAnyWithValue(msg)
		if err != nil {
			return nil, sdkerrors.Wrapf(govtypes.ErrInvalidProposalContent, "message %d: %v", i, err)
		}
		packed = append(packed, value)
	}
	return &PointerBindingProposal{Title: title, Description: description, Messages: packed}, nil
}

func (p *PointerBindingProposal) GetTitle() string { return p.Title }

func (p *PointerBindingProposal) GetDescription() string { return p.Description }

func (p *PointerBindingProposal) ProposalRoute() string { return RouterKey }

func (p *PointerBindingProposal) ProposalType() string { return ProposalTypePointerBinding }

// ValidateBasic refuses a proposal without a title or a description, one that
// carries no message, and one carrying anything but the module's governance
// messages, a message that fails its own ValidateBasic, or a message whose
// authority is not the governance module account that executes it.
func (p *PointerBindingProposal) ValidateBasic() error {
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
	governance := GovernanceAuthority()
	for i, msg := range msgs {
		if err := msg.ValidateBasic(); err != nil {
			return sdkerrors.Wrapf(err, "message %d (%s)", i, sdk.MsgTypeURL(msg))
		}
		authority, err := pointerGovernanceMessage(msg)
		if err != nil {
			return sdkerrors.Wrapf(err, "message %d", i)
		}
		if authority != governance {
			return sdkerrors.Wrapf(sdkerrors.ErrUnauthorized, "message %d (%s): authority %s is not the governance module account %s",
				i, sdk.MsgTypeURL(msg), authority, governance)
		}
	}
	return nil
}

// GetMessages returns the carried messages in order. It refuses a message
// that was not unpacked through an interface registry or that is not one of
// the module's governance messages.
func (p *PointerBindingProposal) GetMessages() ([]sdk.Msg, error) {
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
		if _, err := pointerGovernanceMessage(msg); err != nil {
			return nil, sdkerrors.Wrapf(err, "message %d", i)
		}
		msgs = append(msgs, msg)
	}
	return msgs, nil
}

// UnpackInterfaces resolves every carried message against the registry.
func (p PointerBindingProposal) UnpackInterfaces(unpacker cdctypes.AnyUnpacker) error {
	for _, packed := range p.Messages {
		var msg sdk.Msg
		if err := unpacker.UnpackAny(packed, &msg); err != nil {
			return err
		}
	}
	return nil
}

func (p PointerBindingProposal) String() string {
	var b strings.Builder
	fmt.Fprintf(&b, "Pointer Binding Proposal:\n  Title:       %s\n  Description: %s\n  Messages:\n", p.Title, p.Description)
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

// pointerGovernanceMessage returns the authority of one of the module's
// governance messages and refuses every other message.
func pointerGovernanceMessage(msg sdk.Msg) (string, error) {
	switch m := msg.(type) {
	case *MsgBindERCNativePointer:
		return m.Authority, nil
	default:
		return "", sdkerrors.Wrapf(govtypes.ErrInvalidProposalContent,
			"%T is not a %s governance message", msg, ModuleName)
	}
}
