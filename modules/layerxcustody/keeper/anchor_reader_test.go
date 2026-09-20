package keeper_test

import (
	"encoding/binary"
	"testing"

	"github.com/ethereum/go-ethereum/crypto"
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

const anchorPaxeerChainID = 713715

// guarantorCertificate is a checkpoint certificate over header carrying one
// attestation signed by a secp256k1 key generated for the test, in the wire
// form the anchor keeper decodes and verifies.
func guarantorCertificate(t *testing.T, headerBytes []byte, guarantorID [32]byte, contract [20]byte) ([]byte, [20]byte) {
	t.Helper()
	header, err := codec.DecodeBatchHeader(headerBytes)
	require.NoError(t, err)
	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	signer := crypto.PubkeyToAddress(key.PublicKey)
	checkpointID := codec.CheckpointHash(headerBytes, nil)

	message := make([]byte, 0, codec.GuarantorAttestationBytes)
	message = binary.BigEndian.AppendUint16(message, header.ProtocolVersion)
	message = binary.BigEndian.AppendUint32(message, header.NetworkID)
	message = binary.BigEndian.AppendUint64(message, anchorPaxeerChainID)
	message = append(message, contract[:]...)
	message = binary.BigEndian.AppendUint64(message, header.Epoch)
	message = append(message, checkpointID[:]...)
	message = append(message, checkpointID[:]...)
	message = append(message, guarantorID[:]...)
	message = binary.BigEndian.AppendUint64(message, header.BatchNumber)
	message = append(message, header.DataAvailabilityRoot[:]...)
	message = append(message, 1, 1, codec.AvailabilityAll)
	message = binary.BigEndian.AppendUint64(message, header.TimestampMs+1)
	require.Len(t, message, codec.GuarantorAttestationMessageBytes)

	digest, err := codec.DomainHash(codec.DomainGuarantorAttestation, message)
	require.NoError(t, err)
	signature, err := crypto.Sign(digest[:], key)
	require.NoError(t, err)
	attestation := append(message, signer[:]...)
	attestation = append(attestation, signature[:64]...)
	attestation = append(attestation, signature[64]+27)
	require.Len(t, attestation, codec.GuarantorAttestationBytes)

	certificate := binary.BigEndian.AppendUint16(nil, codec.CheckpointWireVersion)
	certificate = binary.BigEndian.AppendUint32(certificate, uint32(len(headerBytes))) //nolint:gosec
	certificate = append(certificate, headerBytes...)
	certificate = binary.BigEndian.AppendUint32(certificate, 0)
	certificate = append(certificate, 1)
	certificate = append(certificate, attestation...)
	certificate = append(certificate, 1)
	certificate = binary.BigEndian.AppendUint16(certificate, 0)
	return certificate, signer
}

// TestWithdrawalReadsAnchorFinalizedCheckpoint wires custody to the anchor
// keeper exactly as the app does and proves the withdrawal roots come from a
// checkpoint the anchor finalized on a sequencer-signed header and a bonded
// guarantor's certificate, not from custody's authority-registered checkpoints.
func TestWithdrawalReadsAnchorFinalizedCheckpoint(t *testing.T) {
	app := testkeeper.EVMTestApp
	ctx, _ := app.NewContext(false, tmtypes.Header{}).WithBlockHeight(7).WithBlockTime(genesisTime).CacheContext()
	fixture, err := testvectors.Load()
	require.NoError(t, err)
	e := &env{t: t, ctx: ctx, k: app.LayerXCustodyKeeper, withdrawal: fixture["withdrawal"][0], exit: fixture["exit"][0]}
	anchor := app.LayerXAnchorKeeper
	e.k.SetAnchorReader(anchorkeeper.NewCustodyAnchor(anchor))

	e.payerAcc, e.payer = testkeeper.MockAddressPair()
	app.EvmKeeper.SetAddressMapping(ctx, e.payerAcc, e.payer)
	coins := sdk.NewCoins(sdk.NewCoin(sdk.MustGetBaseDenom(), sdk.NewInt(50_000_000)))
	require.NoError(t, app.BankKeeper.MintCoins(ctx, "evm", coins))
	require.NoError(t, app.BankKeeper.SendCoinsFromModuleToAccount(ctx, "evm", e.payerAcc, coins))

	e.k.InitGenesis(ctx, *types.DefaultGenesis())
	params := types.DefaultParams()
	params.NetworkId = uint32(vectorNumber(t, e.withdrawal, "network_id")) //nolint:gosec
	params.WithdrawalDelaySeconds = 0
	require.NoError(t, e.k.SetParams(ctx, params))
	require.Empty(t, params.SequencerAuthorizations)
	require.NoError(t, e.k.SetAsset(ctx, types.AssetMapping{AssetId: e.withdrawal.Fields["asset"],
		Denom: sdk.MustGetBaseDenom(), Enabled: true}))
	e.deposit(e.withdrawal.Fields["asset"], 1_000)

	headerBytes := vectorBytes(t, e.withdrawal, "header")
	header, err := codec.DecodeBatchHeader(headerBytes)
	require.NoError(t, err)
	batch := header.BatchNumber

	// The anchor starts at the batch before the withdrawal's and authorises the
	// vector's sequencer for it.
	authority, _ := testkeeper.MockAddressPair()
	genesis := anchortypes.DefaultGenesis()
	genesis.Params = anchortypes.DefaultParams(authority.String())
	genesis.Params.NetworkID = header.NetworkID
	genesis.Params.PaxeerChainID = anchorPaxeerChainID
	genesis.Params.Threshold = 1
	genesis.Anchor = anchortypes.Anchor{Set: true, BatchNumber: batch - 1, LastSequence: header.FirstSequence - 1,
		StateRoot: anchortypes.Hash32(header.PreviousStateRoot)}
	genesis.Sequencers = []anchortypes.SequencerAuthorization{{SequencerID: anchortypes.Hash32(header.SequencerID),
		PublicKey: anchortypes.Hash32(vectorArray(t, e.withdrawal, "public_key")), FirstBatchNumber: batch, LastBatchNumber: batch}}
	anchor.InitGenesis(ctx, *genesis)

	// A custody-registered checkpoint with the right roots is not trusted once
	// the anchor reader is set: nothing is finalized on the anchor yet.
	require.NoError(t, e.k.RegisterCheckpoint(ctx, batch, header.ResultingStateRoot, header.ReceiptMerkleRoot))
	_, _, err = e.k.VerifyWithdrawal(ctx, e.evidence())
	require.ErrorIs(t, err, types.ErrNotFinalized)

	guarantorID := [32]byte{0x61}
	certificate, signer := guarantorCertificate(t, headerBytes, guarantorID, [20]byte(genesis.Params.SettlementContract))
	operator, _ := testkeeper.MockAddressPair()
	bond := sdk.NewCoins(sdk.NewCoin(genesis.Params.BondDenom, genesis.Params.MinBond))
	require.NoError(t, app.BankKeeper.MintCoins(ctx, "evm", bond))
	require.NoError(t, app.BankKeeper.SendCoinsFromModuleToAccount(ctx, "evm", operator, bond))
	_, err = anchor.RegisterGuarantor(ctx, operator, guarantorID, signer, genesis.Params.MinBond)
	require.NoError(t, err)
	require.NoError(t, anchor.ActivateGuarantor(ctx, authority, guarantorID))

	var headerSignature [64]byte
	copy(headerSignature[:], vectorBytes(t, e.withdrawal, "header_signature"))
	checkpoint, err := anchor.SubmitCheckpoint(ctx, operator, headerBytes, headerSignature, certificate)
	require.NoError(t, err)
	require.Equal(t, anchortypes.CheckpointFinal, checkpoint.Status)

	stateRoot, ok := e.k.Anchor().FinalizedStateRoot(ctx, batch)
	require.True(t, ok)
	require.Equal(t, header.ResultingStateRoot, stateRoot)
	latest, finalizedAt, ok := e.k.Anchor().LatestFinalizedBatch(ctx)
	require.True(t, ok)
	require.Equal(t, batch, latest)
	require.Equal(t, genesisTime.Unix(), finalizedAt)

	effect, verified, err := e.k.VerifyWithdrawal(ctx, e.evidence())
	require.NoError(t, err)
	require.Equal(t, batch, verified.BatchNumber)
	require.Equal(t, vectorArray(t, e.withdrawal, "nullifier"), effect.Nullifier)
	result, err := e.k.FinaliseWithdrawal(ctx, e.evidence())
	require.NoError(t, err)
	require.Equal(t, types.ClaimStatus_CLAIM_STATUS_PAID, result.Claim.Status)
	e.solvent()

	// A second sequencer authorised for the same batch makes the batch-only
	// lookup ambiguous; custody refuses instead of picking one.
	ambiguous, _ := ctx.CacheContext()
	require.NoError(t, anchor.SetSequencerAuthorization(ambiguous, authority, anchortypes.SequencerAuthorization{
		SequencerID: anchortypes.Hash32{0x77}, PublicKey: anchortypes.Hash32(vectorArray(t, e.withdrawal, "public_key")),
		FirstBatchNumber: batch, LastBatchNumber: batch}))
	_, _, err = e.k.VerifyWithdrawal(ambiguous, e.evidence())
	require.ErrorIs(t, err, types.ErrNotAuthorized)
}
