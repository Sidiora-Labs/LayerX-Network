package types_test

import (
	"math"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	cdctypes "github.com/sidiora-labs/paxeer-network/sdk/codec/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
	govtypes "github.com/sidiora-labs/paxeer-network/sdk/x/gov/types"
	"github.com/stretchr/testify/require"

	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
)

func TestAddERCNativePointerProposalV2(t *testing.T) {
	p := types.AddERCNativePointerProposalV2{
		Title:       "title",
		Description: "desc",
		Token:       "test",
		Name:        "TEST",
		Symbol:      "Test",
		Decimals:    6,
	}
	require.Equal(t, "title", p.GetTitle())
	require.Equal(t, "desc", p.GetDescription())
	require.Equal(t, "evm", p.ProposalRoute())
	require.Equal(t, "AddERCNativePointerV2", p.ProposalType())
	p.Decimals = math.MaxUint32
	require.NotNil(t, p.ValidateBasic())
	p.Decimals = 6
	require.Nil(t, p.ValidateBasic())
	require.NotEmpty(t, p.String())
}

func TestPointerBindingProposalCarriesAGovernanceBinding(t *testing.T) {
	p, err := types.NewPointerBindingProposal("title", "desc", validBindMsg())
	require.NoError(t, err)
	require.Equal(t, "title", p.GetTitle())
	require.Equal(t, "desc", p.GetDescription())
	require.Equal(t, "evm", p.ProposalRoute())
	require.Equal(t, types.ProposalTypePointerBinding, p.ProposalType())
	require.NoError(t, p.ValidateBasic())
	msgs, err := p.GetMessages()
	require.NoError(t, err)
	require.Equal(t, []sdk.Msg{validBindMsg()}, msgs)
	require.Contains(t, p.String(), "/paxprotocol.paxchain.evm.MsgBindERCNativePointer")
}

func TestPointerBindingProposalRefusesAProposalWithoutMessages(t *testing.T) {
	p, err := types.NewPointerBindingProposal("title", "desc")
	require.NoError(t, err)
	require.ErrorContains(t, p.ValidateBasic(), "the proposal carries no message")
}

func TestPointerBindingProposalRefusesAnotherAuthority(t *testing.T) {
	msg := validBindMsg()
	msg.Authority = sdk.AccAddress(common.HexToAddress(bindTestPointer).Bytes()).String()
	p, err := types.NewPointerBindingProposal("title", "desc", msg)
	require.NoError(t, err)
	err = p.ValidateBasic()
	require.ErrorIs(t, err, sdkerrors.ErrUnauthorized)
	require.ErrorContains(t, err, "is not the governance module account")
}

func TestPointerBindingProposalRefusesAMalformedMessage(t *testing.T) {
	msg := validBindMsg()
	msg.Version = 0
	p, err := types.NewPointerBindingProposal("title", "desc", msg)
	require.NoError(t, err)
	require.ErrorContains(t, p.ValidateBasic(), "the pointer version is zero")
}

func TestPointerBindingProposalRefusesAMessageThatIsNotAGovernanceMessage(t *testing.T) {
	send := &types.MsgSend{FromAddress: types.GovernanceAuthority(), ToAddress: bindTestPointer}
	_, err := types.NewPointerBindingProposal("title", "desc", send)
	require.ErrorIs(t, err, govtypes.ErrInvalidProposalContent)

	value, err := cdctypes.NewAnyWithValue(send)
	require.NoError(t, err)
	p := &types.PointerBindingProposal{Title: "title", Description: "desc", Messages: []*cdctypes.Any{value}}
	require.ErrorIs(t, p.ValidateBasic(), govtypes.ErrInvalidProposalContent)
}

func TestPointerBindingProposalRefusesAMissingTitle(t *testing.T) {
	p, err := types.NewPointerBindingProposal("", "desc", validBindMsg())
	require.NoError(t, err)
	require.Error(t, p.ValidateBasic())
}
