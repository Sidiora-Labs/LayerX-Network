package keeper_test

import (
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"testing"

	"github.com/ethereum/go-ethereum/accounts/abi"
	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	"github.com/sidiora-labs/paxeer-network/layerxproof/testvectors"
	anchorkeeper "github.com/sidiora-labs/paxeer-network/modules/layerxanchor/keeper"
	anchortypes "github.com/sidiora-labs/paxeer-network/modules/layerxanchor/types"
	"github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func depositRootRegistration(checkpointID, stateRoot, depositRoot, custodyReference [32]byte, network uint32, protocol uint16) []byte {
	out := []byte("LX:PAXEER:DEPOSIT:ROOT:v1")
	out = append(out, checkpointID[:]...)
	out = append(out, stateRoot[:]...)
	out = append(out, depositRoot[:]...)
	out = append(out, custodyReference[:]...)
	out = binary.BigEndian.AppendUint32(out, network)
	return binary.BigEndian.AppendUint16(out, protocol)
}

// TestDepositRootRegistration finalizes a checkpoint on the anchor keeper and
// registers its deposit root the way the settlement service does.
func TestDepositRootRegistration(t *testing.T) {
	app := testkeeper.EVMTestApp
	ctx, _ := app.NewContext(false, tmtypes.Header{}).WithBlockHeight(7).WithBlockTime(genesisTime).CacheContext()
	fixture, err := testvectors.Load()
	require.NoError(t, err)
	vector := fixture["withdrawal"][0]
	k := app.LayerXCustodyKeeper
	anchor := app.LayerXAnchorKeeper
	k.SetAnchorReader(anchorkeeper.NewCustodyAnchor(anchor))

	headerBytes := vectorBytes(t, vector, "header")
	header, err := codec.DecodeBatchHeader(headerBytes)
	require.NoError(t, err)
	public, private, err := ed25519.GenerateKey(nil)
	require.NoError(t, err)

	k.InitGenesis(ctx, *types.DefaultGenesis())
	params := types.DefaultParams()
	params.NetworkId = header.NetworkID
	params.DepositRootAuthority = "00" + hex.EncodeToString(public)
	require.ErrorIs(t, params.Validate(), types.ErrInvalidParams)
	params.DepositRootAuthority = ""
	require.NoError(t, k.SetParams(ctx, params))

	authority, _ := testkeeper.MockAddressPair()
	genesis := anchortypes.DefaultGenesis()
	genesis.Params = anchortypes.DefaultParams(authority.String())
	genesis.Params.NetworkID = header.NetworkID
	genesis.Params.PaxeerChainID = anchorPaxeerChainID
	genesis.Params.Threshold = 1
	genesis.Anchor = anchortypes.Anchor{Set: true, BatchNumber: header.BatchNumber - 1, LastSequence: header.FirstSequence - 1,
		StateRoot: anchortypes.Hash32(header.PreviousStateRoot)}
	genesis.Sequencers = []anchortypes.SequencerAuthorization{{SequencerID: anchortypes.Hash32(header.SequencerID),
		PublicKey:        anchortypes.Hash32(vectorArray(t, vector, "public_key")),
		FirstBatchNumber: header.BatchNumber, LastBatchNumber: header.BatchNumber}}
	anchor.InitGenesis(ctx, *genesis)

	guarantorID := [32]byte{0x62}
	certificate, signer := guarantorCertificate(t, headerBytes, guarantorID, [20]byte(genesis.Params.SettlementContract))
	operator, _ := testkeeper.MockAddressPair()
	bond := sdk.NewCoins(sdk.NewCoin(genesis.Params.BondDenom, genesis.Params.MinBond))
	require.NoError(t, app.BankKeeper.MintCoins(ctx, "evm", bond))
	require.NoError(t, app.BankKeeper.SendCoinsFromModuleToAccount(ctx, "evm", operator, bond))
	_, err = anchor.RegisterGuarantor(ctx, operator, guarantorID, signer, genesis.Params.MinBond)
	require.NoError(t, err)
	require.NoError(t, anchor.ActivateGuarantor(ctx, authority, guarantorID))

	checkpointID := codec.CheckpointHash(headerBytes, nil)
	depositRoot, custodyReference := [32]byte{0xd1}, [32]byte{0xc1}
	registration := depositRootRegistration(checkpointID, header.ResultingStateRoot, depositRoot, custodyReference,
		header.NetworkID, header.ProtocolVersion)
	signature := ed25519.Sign(private, registration)
	ordering := [][32]byte{{0x01}, {0x02}}

	// Nothing is final on the anchor yet.
	_, err = k.RegisterDepositRoot(ctx, operator, registration, signature, ordering)
	require.ErrorIs(t, err, types.ErrInvalidDepositRoot)

	var headerSignature [64]byte
	copy(headerSignature[:], vectorBytes(t, vector, "header_signature"))
	checkpoint, err := anchor.SubmitCheckpoint(ctx, operator, headerBytes, headerSignature, certificate)
	require.NoError(t, err)
	require.Equal(t, anchortypes.CheckpointFinal, checkpoint.Status)

	// No authority is configured.
	_, err = k.RegisterDepositRoot(ctx, operator, registration, signature, ordering)
	require.ErrorIs(t, err, types.ErrInvalidDepositRoot)
	params.DepositRootAuthority = hex.EncodeToString(public)
	require.NoError(t, k.SetParams(ctx, params))

	_, err = k.RegisterDepositRoot(ctx, authority, registration, signature, ordering)
	require.ErrorIs(t, err, types.ErrDepositRootProposer)

	refused := func(mutate func(registration, signature []byte) ([]byte, []byte), leaves [][32]byte) {
		t.Helper()
		changed, signed := mutate(append([]byte(nil), registration...), append([]byte(nil), signature...))
		_, err := k.RegisterDepositRoot(ctx, operator, changed, signed, leaves)
		require.ErrorIs(t, err, types.ErrInvalidDepositRoot)
	}
	resign := func(changed []byte) ([]byte, []byte) { return changed, ed25519.Sign(private, changed) }
	refused(func(r, s []byte) ([]byte, []byte) { s[0] ^= 1; return r, s }, ordering)
	refused(func(r, s []byte) ([]byte, []byte) { return r, s }, nil)
	refused(func(r, s []byte) ([]byte, []byte) { return r, s }, make([][32]byte, 4097))
	refused(func(r, s []byte) ([]byte, []byte) { r[0] ^= 1; return resign(r) }, ordering)
	refused(func(r, s []byte) ([]byte, []byte) { r[25+32] ^= 1; return resign(r) }, ordering)
	refused(func(r, s []byte) ([]byte, []byte) { copy(r[25+64:], make([]byte, 32)); return resign(r) }, ordering)
	refused(func(r, s []byte) ([]byte, []byte) { copy(r[25+96:], make([]byte, 32)); return resign(r) }, ordering)
	refused(func(r, s []byte) ([]byte, []byte) { r[25+131] ^= 1; return resign(r) }, ordering)
	refused(func(r, s []byte) ([]byte, []byte) { r[25+133] ^= 1; return resign(r) }, ordering)
	refused(func(r, s []byte) ([]byte, []byte) { return resign(append(r, 0)) }, ordering)
	_, found := k.GetDepositRoot(ctx, checkpointID)
	require.False(t, found)

	recorded, err := k.RegisterDepositRoot(ctx, operator, registration, signature, ordering)
	require.NoError(t, err)
	kind := func(name string) abi.Type {
		out, err := abi.NewType(name, "", nil)
		require.NoError(t, err)
		return out
	}
	encoded, err := abi.Arguments{{Type: kind("uint16")}, {Type: kind("bytes")}, {Type: kind("bytes")},
		{Type: kind("bytes32[]")}}.Pack(uint16(2), registration, signature, ordering)
	require.NoError(t, err)
	require.Equal(t, types.DepositRootRegistration{CheckpointId: types.Hash32(checkpointID),
		DepositRoot: types.Hash32(depositRoot), Commitment: types.Hash32(sha256.Sum256(encoded))}, recorded)
	stored, found := k.GetDepositRoot(ctx, checkpointID)
	require.True(t, found)
	require.Equal(t, recorded, stored)

	_, err = k.RegisterDepositRoot(ctx, operator, registration, signature, ordering)
	require.ErrorIs(t, err, types.ErrDepositRootExists)

	exported := k.ExportGenesis(ctx)
	require.Equal(t, []types.DepositRootRegistration{recorded}, exported.DepositRoots)
	require.NoError(t, exported.Validate())
}
