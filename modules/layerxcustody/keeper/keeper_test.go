package keeper_test

import (
	"testing"
	"time"

	"github.com/ethereum/go-ethereum/common"
	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/layerxproof/testvectors"
	"github.com/sidiora-labs/paxeer-network/modules/layerxcustody/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

const tokenDenom = "ufoo"

var (
	tokenPointer = common.HexToAddress("0x00000000000000000000000000000000000f00f0")
	genesisTime  = time.Unix(1_800_000_000, 0).UTC()
)

// env is the real application: the custody keeper wired to the real bank,
// account and EVM keepers, on a branch of the test app's state.
type env struct {
	t          *testing.T
	ctx        sdk.Context
	k          *keeper.Keeper
	withdrawal testvectors.Vector
	exit       testvectors.Vector
	payer      common.Address
	payerAcc   sdk.AccAddress
}

func vectorArray(t *testing.T, v testvectors.Vector, key string) [32]byte {
	t.Helper()
	out, err := v.Array32(key)
	require.NoError(t, err)
	return out
}

func vectorBytes(t *testing.T, v testvectors.Vector, key string) []byte {
	t.Helper()
	out, err := v.Bytes(key)
	require.NoError(t, err)
	return out
}

func vectorNumber(t *testing.T, v testvectors.Vector, key string) uint64 {
	t.Helper()
	out, err := v.Uint64(key)
	require.NoError(t, err)
	return out
}

func newEnv(t *testing.T, withdrawalDelay uint64) *env {
	t.Helper()
	app := testkeeper.EVMTestApp
	ctx, _ := app.NewContext(false, tmtypes.Header{}).WithBlockHeight(7).WithBlockTime(genesisTime).CacheContext()
	fixture, err := testvectors.Load()
	require.NoError(t, err)
	e := &env{t: t, ctx: ctx, k: app.LayerXCustodyKeeper, withdrawal: fixture["withdrawal"][0], exit: fixture["exit"][0]}
	e.payerAcc, e.payer = testkeeper.MockAddressPair()
	app.EvmKeeper.SetAddressMapping(ctx, e.payerAcc, e.payer)
	for _, denom := range []string{sdk.MustGetBaseDenom(), tokenDenom} {
		coins := sdk.NewCoins(sdk.NewCoin(denom, sdk.NewInt(50_000_000)))
		require.NoError(t, app.BankKeeper.MintCoins(ctx, "evm", coins))
		require.NoError(t, app.BankKeeper.SendCoinsFromModuleToAccount(ctx, "evm", e.payerAcc, coins))
	}
	e.k.InitGenesis(ctx, *types.DefaultGenesis())
	batch := vectorNumber(t, e.withdrawal, "batch_number")
	params := types.DefaultParams()
	params.NetworkId = uint32(vectorNumber(t, e.withdrawal, "network_id")) //nolint:gosec
	params.WithdrawalDelaySeconds = withdrawalDelay
	params.SequencerAuthorizations = []types.SequencerAuthorization{{
		SequencerId: e.withdrawal.Fields["sequencer_id"], PublicKey: e.withdrawal.Fields["public_key"],
		FirstBatchNumber: batch, LastBatchNumber: batch}}
	require.NoError(t, e.k.SetParams(ctx, params))
	require.NoError(t, e.k.SetAsset(ctx, types.AssetMapping{AssetId: e.withdrawal.Fields["asset"],
		Denom: sdk.MustGetBaseDenom(), Enabled: true}))
	require.NoError(t, e.k.SetAsset(ctx, types.AssetMapping{AssetId: e.exit.Fields["asset"], Denom: tokenDenom,
		Pointer: tokenPointer.Hex(), Enabled: true, MinimumDeposit: "10"}))
	require.NoError(t, e.k.RegisterCheckpoint(ctx, batch, vectorArray(t, e.withdrawal, "header_state_root"),
		vectorArray(t, e.withdrawal, "header_receipt_root")))
	return e
}

func (e *env) evidence() keeper.WithdrawalEvidence {
	return keeper.WithdrawalEvidence{Receipt: vectorBytes(e.t, e.withdrawal, "receipt"),
		Proof: vectorBytes(e.t, e.withdrawal, "proof"), Header: vectorBytes(e.t, e.withdrawal, "header"),
		HeaderSignature: vectorBytes(e.t, e.withdrawal, "header_signature")}
}

func (e *env) exitEvidence(batch uint64) keeper.ExitEvidence {
	return keeper.ExitEvidence{Witness: vectorBytes(e.t, e.exit, "witness"), BatchNumber: batch,
		Account: vectorArray(e.t, e.exit, "account"), AssetID: vectorArray(e.t, e.exit, "asset"),
		Recipient:          common.BytesToAddress(vectorBytes(e.t, e.exit, "recipient")),
		RecipientSignature: vectorBytes(e.t, e.exit, "recipient_signature")}
}

func (e *env) deposit(asset string, amount int64) types.Deposit {
	e.t.Helper()
	assetID, err := types.ParseHash32(asset)
	require.NoError(e.t, err)
	deposit, err := e.k.Deposit(e.ctx, e.payer, e.payerAcc, assetID, [32]byte{0xbe, 0xef}, sdk.NewInt(amount))
	require.NoError(e.t, err)
	return deposit
}

func (e *env) balance(account sdk.AccAddress, denom string) sdk.Int {
	return testkeeper.EVMTestApp.BankKeeper.GetBalance(e.ctx, account, denom).Amount
}

func (e *env) recipient() sdk.AccAddress {
	return testkeeper.EVMTestApp.EvmKeeper.GetPaxAddressOrDefault(e.ctx,
		common.BytesToAddress(vectorBytes(e.t, e.withdrawal, "recipient")))
}

func (e *env) solvent() {
	e.t.Helper()
	message, broken := keeper.SolvencyInvariant(e.k)(e.ctx)
	require.False(e.t, broken, message)
}

func TestDepositMovesFundsIntoTheModuleAccountAndRecords(t *testing.T) {
	e := newEnv(t, 0)
	before := e.balance(e.payerAcc, sdk.MustGetBaseDenom())
	first := e.deposit(e.withdrawal.Fields["asset"], 1_000)
	second := e.deposit(e.withdrawal.Fields["asset"], 1_000)
	require.Equal(t, sdk.NewInt(2_000), e.balance(e.k.ModuleAddress(), sdk.MustGetBaseDenom()))
	require.Equal(t, before.SubRaw(2_000), e.balance(e.payerAcc, sdk.MustGetBaseDenom()))
	require.Equal(t, uint64(2), e.k.GetDepositCount(e.ctx))
	require.NotEqual(t, first.DepositId, second.DepositId)
	require.Equal(t, uint64(2), second.Nonce)
	require.Equal(t, e.payer.Hex(), first.Depositor)
	require.Equal(t, types.Hash32([32]byte{0xbe, 0xef}), first.Beneficiary)
	require.Equal(t, int64(7), first.Height)
	byIndex, found := e.k.GetDepositByIndex(e.ctx, 2)
	require.True(t, found)
	require.Equal(t, second, byIndex)
	assetID, _ := types.ParseHash32(e.withdrawal.Fields["asset"])
	expected := types.DepositID(testkeeper.EVMTestApp.EvmKeeper.ChainID(e.ctx), e.payer, assetID, [32]byte{0xbe, 0xef},
		sdk.NewInt(1_000).BigInt(), 1)
	require.Equal(t, types.Hash32(expected), first.DepositId)
	found = false
	for _, event := range e.ctx.EventManager().Events() {
		found = found || event.Type == "paxprotocol.paxchain.layerxcustody.EventCustodyDeposit"
	}
	require.True(t, found, "deposit typed event")
	e.solvent()

	_, err := e.k.Deposit(e.ctx, e.payer, e.payerAcc, assetID, [32]byte{}, sdk.NewInt(5))
	require.ErrorIs(t, err, types.ErrInvalidDeposit)
	_, err = e.k.Deposit(e.ctx, e.payer, e.payerAcc, [32]byte{9}, [32]byte{1}, sdk.NewInt(5))
	require.ErrorIs(t, err, types.ErrUnknownAsset)
	exitAsset, _ := types.ParseHash32(e.exit.Fields["asset"])
	_, err = e.k.Deposit(e.ctx, e.payer, e.payerAcc, exitAsset, [32]byte{1}, sdk.NewInt(9))
	require.ErrorIs(t, err, types.ErrInvalidDeposit)
}

func TestWithdrawalPaysOnceAndReplayIsRefused(t *testing.T) {
	e := newEnv(t, 0)
	e.deposit(e.withdrawal.Fields["asset"], 1_000)
	result, err := e.k.FinaliseWithdrawal(e.ctx, e.evidence())
	require.NoError(t, err)
	require.True(t, result.Queued)
	require.Equal(t, types.ClaimStatus_CLAIM_STATUS_PAID, result.Claim.Status)
	require.Equal(t, e.withdrawal.Fields["nullifier"], result.Claim.Nullifier)
	require.Equal(t, sdk.NewInt(int64(vectorNumber(t, e.withdrawal, "amount"))), e.balance(e.recipient(), sdk.MustGetBaseDenom()))
	require.Equal(t, sdk.NewInt(999), e.balance(e.k.ModuleAddress(), sdk.MustGetBaseDenom()))
	nullifier, found := e.k.GetNullifier(e.ctx, vectorArray(t, e.withdrawal, "nullifier"))
	require.True(t, found)
	require.Equal(t, types.NullifierStatus_NULLIFIER_STATUS_CONSUMED, nullifier.Status)
	e.solvent()

	_, err = e.k.FinaliseWithdrawal(e.ctx, e.evidence())
	require.ErrorIs(t, err, types.ErrClaimNotPending)
	_, err = e.k.RequestWithdrawal(e.ctx, e.evidence())
	require.ErrorIs(t, err, types.ErrNullifierUsed)
	require.Equal(t, sdk.NewInt(1), e.balance(e.recipient(), sdk.MustGetBaseDenom()))
	require.Equal(t, sdk.NewInt(999), e.balance(e.k.ModuleAddress(), sdk.MustGetBaseDenom()))
}

func TestWithdrawalDelayAndCancellation(t *testing.T) {
	e := newEnv(t, 600)
	e.deposit(e.withdrawal.Fields["asset"], 1_000)
	_, err := e.k.FinaliseWithdrawal(e.ctx, e.evidence())
	require.ErrorIs(t, err, types.ErrClaimNotReady)
	_, reserved := e.k.GetNullifier(e.ctx, vectorArray(t, e.withdrawal, "nullifier"))
	require.False(t, reserved, "a refused finalise must leave no state")

	claim, err := e.k.RequestWithdrawal(e.ctx, e.evidence())
	require.NoError(t, err)
	require.Equal(t, genesisTime.Unix()+600, claim.AvailableAt)
	e.solvent()
	e.ctx = e.ctx.WithBlockTime(genesisTime.Add(599 * time.Second))
	_, err = e.k.FinaliseWithdrawal(e.ctx, e.evidence())
	require.ErrorIs(t, err, types.ErrClaimNotReady)
	require.True(t, e.balance(e.recipient(), sdk.MustGetBaseDenom()).IsZero())

	paid, _ := e.ctx.CacheContext()
	result, err := e.k.FinaliseWithdrawal(paid.WithBlockTime(genesisTime.Add(600*time.Second)), e.evidence())
	require.NoError(t, err)
	require.False(t, result.Queued)

	claimID, _ := types.ParseHash32(claim.ClaimId)
	cancelled, err := e.k.CancelClaim(e.ctx, claimID)
	require.NoError(t, err)
	require.Equal(t, types.ClaimStatus_CLAIM_STATUS_CANCELLED, cancelled.Status)
	e.ctx = e.ctx.WithBlockTime(genesisTime.Add(time.Hour))
	_, err = e.k.FinaliseWithdrawal(e.ctx, e.evidence())
	require.ErrorIs(t, err, types.ErrClaimNotPending)
	_, err = e.k.RequestWithdrawal(e.ctx, e.evidence())
	require.ErrorIs(t, err, types.ErrNullifierUsed)
	require.True(t, e.balance(e.recipient(), sdk.MustGetBaseDenom()).IsZero())
	e.solvent()
}

func TestWithdrawalRefusesWrongRootSignatureRecipientAndAuthority(t *testing.T) {
	e := newEnv(t, 0)
	e.deposit(e.withdrawal.Fields["asset"], 1_000)
	refused := func(name string, evidence keeper.WithdrawalEvidence, ctx sdk.Context) {
		_, err := e.k.FinaliseWithdrawal(ctx, evidence)
		require.Error(t, err, name)
		require.True(t, e.balance(e.recipient(), sdk.MustGetBaseDenom()).IsZero(), name)
	}

	signature := e.evidence()
	signature.HeaderSignature = append([]byte(nil), signature.HeaderSignature...)
	signature.HeaderSignature[5] ^= 1
	refused("header signature", signature, e.ctx)

	// A receipt whose recipient was altered no longer carries the sequencer's
	// signature: the recipient is whatever the sequencer signed, never calldata.
	recipient := e.evidence()
	recipient.Receipt = append([]byte(nil), recipient.Receipt...)
	index := -1
	needle := vectorBytes(t, e.withdrawal, "recipient")
	for i := 0; i+len(needle) <= len(recipient.Receipt); i++ {
		if string(recipient.Receipt[i:i+len(needle)]) == string(needle) {
			index = i
		}
	}
	require.GreaterOrEqual(t, index, 0)
	recipient.Receipt[index] ^= 1
	refused("recipient", recipient, e.ctx)

	// The re-signed vector is a valid withdrawal under a key this chain never
	// authorised, and is not under the finalized receipt root.
	fixture, err := testvectors.Load()
	require.NoError(t, err)
	foreign := e.evidence()
	for _, v := range fixture["withdrawal"] {
		if v.Name == "resigned-event-offset-130" {
			foreign.Receipt = vectorBytes(t, v, "receipt")
		}
	}
	refused("re-signed recipient", foreign, e.ctx)

	// The same evidence against a chain whose finalized roots differ.
	otherRoots := newEnvWithRoots(t, [32]byte{1}, vectorArray(t, e.withdrawal, "header_receipt_root"))
	_, err = otherRoots.k.FinaliseWithdrawal(otherRoots.ctx, otherRoots.evidence())
	require.ErrorIs(t, err, types.ErrNotFinalized)
	otherRoots = newEnvWithRoots(t, vectorArray(t, e.withdrawal, "header_state_root"), [32]byte{2})
	_, err = otherRoots.k.FinaliseWithdrawal(otherRoots.ctx, otherRoots.evidence())
	require.ErrorIs(t, err, types.ErrNotFinalized)

	// No sequencer authorised for the batch.
	batch := vectorNumber(t, e.withdrawal, "batch_number")
	unauthorised, _ := e.ctx.CacheContext()
	params := e.k.GetParams(unauthorised)
	params.SequencerAuthorizations[0].FirstBatchNumber = batch + 1
	params.SequencerAuthorizations[0].LastBatchNumber = batch + 1
	require.NoError(t, e.k.SetParams(unauthorised, params))
	_, err = e.k.FinaliseWithdrawal(unauthorised, e.evidence())
	require.ErrorIs(t, err, types.ErrNotAuthorized)

	// Another LayerX network.
	network, _ := e.ctx.CacheContext()
	params = e.k.GetParams(network)
	params.NetworkId++
	require.NoError(t, e.k.SetParams(network, params))
	_, err = e.k.FinaliseWithdrawal(network, e.evidence())
	require.ErrorIs(t, err, types.ErrWrongNetwork)

	_, err = e.k.FinaliseWithdrawal(e.ctx, e.evidence())
	require.NoError(t, err)
}

// newEnvWithRoots is newEnv with a different finalized checkpoint for the
// withdrawal batch.
func newEnvWithRoots(t *testing.T, stateRoot, receiptRoot [32]byte) *env {
	t.Helper()
	app := testkeeper.EVMTestApp
	ctx, _ := app.NewContext(false, tmtypes.Header{}).WithBlockHeight(7).WithBlockTime(genesisTime).CacheContext()
	fixture, err := testvectors.Load()
	require.NoError(t, err)
	e := &env{t: t, ctx: ctx, k: app.LayerXCustodyKeeper, withdrawal: fixture["withdrawal"][0], exit: fixture["exit"][0]}
	batch := vectorNumber(t, e.withdrawal, "batch_number")
	params := types.DefaultParams()
	params.NetworkId = uint32(vectorNumber(t, e.withdrawal, "network_id")) //nolint:gosec
	params.WithdrawalDelaySeconds = 0
	params.SequencerAuthorizations = []types.SequencerAuthorization{{
		SequencerId: e.withdrawal.Fields["sequencer_id"], PublicKey: e.withdrawal.Fields["public_key"],
		FirstBatchNumber: batch, LastBatchNumber: batch}}
	require.NoError(t, e.k.SetParams(ctx, params))
	require.NoError(t, e.k.SetAsset(ctx, types.AssetMapping{AssetId: e.withdrawal.Fields["asset"],
		Denom: sdk.MustGetBaseDenom(), Enabled: true}))
	require.NoError(t, e.k.RegisterCheckpoint(ctx, batch, stateRoot, receiptRoot))
	return e
}

func TestWithdrawalNeverExceedsCustody(t *testing.T) {
	e := newEnv(t, 0)
	_, err := e.k.FinaliseWithdrawal(e.ctx, e.evidence())
	require.ErrorIs(t, err, types.ErrInvalidRelease)
	_, found := e.k.GetNullifier(e.ctx, vectorArray(t, e.withdrawal, "nullifier"))
	require.False(t, found)
	e.solvent()
}

func TestForcedExit(t *testing.T) {
	e := newEnv(t, 0)
	e.deposit(e.exit.Fields["asset"], 6_000_000)
	exitBatch := vectorNumber(t, e.withdrawal, "batch_number") + 4
	stateRoot := vectorArray(t, e.exit, "state_root")
	require.NoError(t, e.k.RegisterCheckpoint(e.ctx, exitBatch, stateRoot, [32]byte{7}))
	recipient := testkeeper.EVMTestApp.EvmKeeper.GetPaxAddressOrDefault(e.ctx, e.exitEvidence(exitBatch).Recipient)

	_, err := e.k.ExecuteForcedExit(e.ctx, e.exitEvidence(exitBatch))
	require.ErrorIs(t, err, types.ErrExitNotEligible)
	e.ctx = e.ctx.WithBlockTime(genesisTime.Add(time.Duration(types.DefaultLivenessBoundSeconds) * time.Second))
	require.True(t, e.k.ExitEligible(e.ctx))

	_, err = e.k.ExecuteForcedExit(e.ctx, e.exitEvidence(vectorNumber(t, e.withdrawal, "batch_number")))
	require.ErrorIs(t, err, types.ErrNotFinalized, "only the latest finalized state root is exitable")

	fixture, err := testvectors.Load()
	require.NoError(t, err)
	forged := e.exitEvidence(exitBatch)
	forged.RecipientSignature = vectorBytes(t, fixture["exit"][1], "recipient_signature")
	_, err = e.k.ExecuteForcedExit(e.ctx, forged)
	require.ErrorIs(t, err, types.ErrInvalidProof)
	stolen := e.exitEvidence(exitBatch)
	stolen.Recipient = e.payer
	_, err = e.k.ExecuteForcedExit(e.ctx, stolen)
	require.ErrorIs(t, err, types.ErrInvalidProof)
	require.True(t, e.balance(recipient, tokenDenom).IsZero())

	result, err := e.k.ExecuteForcedExit(e.ctx, e.exitEvidence(exitBatch))
	require.NoError(t, err)
	require.True(t, result.Queued)
	require.Equal(t, types.ClaimKind_CLAIM_KIND_FORCED_EXIT, result.Claim.Kind)
	require.Equal(t, e.exit.Fields["balance"], result.Claim.Amount)
	require.Equal(t, types.Hash32(stateRoot), result.Claim.Anchor)
	require.Equal(t, sdk.NewInt(5_000_000), e.balance(recipient, tokenDenom))
	require.Equal(t, sdk.NewInt(1_000_000), e.balance(e.k.ModuleAddress(), tokenDenom))
	e.solvent()

	_, err = e.k.ExecuteForcedExit(e.ctx, e.exitEvidence(exitBatch))
	require.ErrorIs(t, err, types.ErrClaimNotPending)
	_, err = e.k.RequestForcedExit(e.ctx, e.exitEvidence(exitBatch))
	require.ErrorIs(t, err, types.ErrExitConsumed)
	require.Equal(t, sdk.NewInt(5_000_000), e.balance(recipient, tokenDenom))
}

func TestForcedExitByEmergencyWithDelay(t *testing.T) {
	e := newEnv(t, 0)
	e.deposit(e.exit.Fields["asset"], 6_000_000)
	params := e.k.GetParams(e.ctx)
	params.ForcedExitDelaySeconds = 120
	require.NoError(t, e.k.SetParams(e.ctx, params))
	exitBatch := vectorNumber(t, e.withdrawal, "batch_number") + 1
	require.NoError(t, e.k.RegisterCheckpoint(e.ctx, exitBatch, vectorArray(t, e.exit, "state_root"), [32]byte{7}))
	require.False(t, e.k.ExitEligible(e.ctx))
	require.NoError(t, e.k.SetEmergency(e.ctx, true))

	claim, err := e.k.RequestForcedExit(e.ctx, e.exitEvidence(exitBatch))
	require.NoError(t, err)
	require.Equal(t, genesisTime.Unix()+120, claim.AvailableAt)
	_, err = e.k.ExecuteForcedExit(e.ctx, e.exitEvidence(exitBatch))
	require.ErrorIs(t, err, types.ErrClaimNotReady)
	e.solvent()

	// A newer checkpoint does not strand the queued exit.
	e.ctx = e.ctx.WithBlockTime(genesisTime.Add(2 * time.Minute))
	require.NoError(t, e.k.RegisterCheckpoint(e.ctx, exitBatch+1, [32]byte{8}, [32]byte{9}))
	result, err := e.k.ExecuteForcedExit(e.ctx, e.exitEvidence(exitBatch))
	require.NoError(t, err)
	require.False(t, result.Queued)
	require.Equal(t, claim.ClaimId, result.Claim.ClaimId)
	e.solvent()
}

func TestGenesisRoundTrip(t *testing.T) {
	e := newEnv(t, 600)
	e.deposit(e.withdrawal.Fields["asset"], 1_000)
	e.deposit(e.exit.Fields["asset"], 6_000_000)
	_, err := e.k.RequestWithdrawal(e.ctx, e.evidence())
	require.NoError(t, err)
	exitBatch := vectorNumber(t, e.withdrawal, "batch_number") + 1
	require.NoError(t, e.k.RegisterCheckpoint(e.ctx, exitBatch, vectorArray(t, e.exit, "state_root"), [32]byte{7}))
	require.NoError(t, e.k.SetEmergency(e.ctx, true))
	_, err = e.k.ExecuteForcedExit(e.ctx, e.exitEvidence(exitBatch))
	require.NoError(t, err)

	exported := e.k.ExportGenesis(e.ctx)
	require.NoError(t, exported.Validate())
	require.Len(t, exported.Deposits, 2)
	require.Len(t, exported.Claims, 2)
	require.Len(t, exported.Nullifiers, 2)
	require.Len(t, exported.ConsumedBalances, 1)
	require.Len(t, exported.Checkpoints, 2)
	require.True(t, exported.Emergency)

	bz, err := testkeeper.EVMTestApp.AppCodec().MarshalAsJSON(exported)
	require.NoError(t, err)
	var decoded types.GenesisState
	require.NoError(t, testkeeper.EVMTestApp.AppCodec().UnmarshalAsJSON(bz, &decoded))

	// Import into a branch whose custody store is empty but whose bank
	// balances are the exported chain's.
	fresh, _ := e.ctx.CacheContext()
	store := fresh.KVStore(testkeeper.EVMTestApp.GetKey(types.StoreKey))
	iterator := store.Iterator(nil, nil)
	var keys [][]byte
	for ; iterator.Valid(); iterator.Next() {
		keys = append(keys, append([]byte(nil), iterator.Key()...))
	}
	require.NoError(t, iterator.Close())
	for _, key := range keys {
		store.Delete(key)
	}
	require.Equal(t, uint64(0), e.k.GetDepositCount(fresh))
	e.k.InitGenesis(fresh, decoded)
	require.Equal(t, exported, e.k.ExportGenesis(fresh))
	message, broken := keeper.SolvencyInvariant(e.k)(fresh)
	require.False(t, broken, message)

	// The paid exit stays paid and the pending withdrawal stays pending.
	_, err = e.k.ExecuteForcedExit(fresh, e.exitEvidence(exitBatch))
	require.ErrorIs(t, err, types.ErrClaimNotPending)
	result, err := e.k.FinaliseWithdrawal(fresh.WithBlockTime(genesisTime.Add(time.Hour)), e.evidence())
	require.NoError(t, err)
	require.False(t, result.Queued)

	reopened := *exported
	reopened.Nullifiers = append([]types.Nullifier(nil), exported.Nullifiers...)
	for i := range reopened.Nullifiers {
		reopened.Nullifiers[i].Status = types.NullifierStatus_NULLIFIER_STATUS_RESERVED
	}
	require.ErrorIs(t, reopened.Validate(), types.ErrInvalidGenesis)
}

func TestSolvencyInvariantDetectsMissingFunds(t *testing.T) {
	e := newEnv(t, 0)
	e.deposit(e.withdrawal.Fields["asset"], 1_000)
	e.solvent()
	drained, _ := e.ctx.CacheContext()
	require.NoError(t, testkeeper.EVMTestApp.BankKeeper.SendCoinsFromModuleToAccount(drained, types.ModuleName, e.payerAcc,
		sdk.NewCoins(sdk.NewCoin(sdk.MustGetBaseDenom(), sdk.NewInt(1)))))
	_, broken := keeper.SolvencyInvariant(e.k)(drained)
	require.True(t, broken)
}

func TestAuthorityGatesAdministration(t *testing.T) {
	e := newEnv(t, 0)
	server := keeper.NewMsgServerImpl(e.k)
	goCtx := sdk.WrapSDKContext(e.ctx)
	_, err := server.SetEmergency(goCtx, &types.MsgSetEmergency{Authority: e.payerAcc.String(), Enabled: true})
	require.Error(t, err)
	require.False(t, e.k.GetEmergency(e.ctx))
	_, err = server.RegisterCheckpoint(goCtx, &types.MsgRegisterCheckpoint{Authority: e.payerAcc.String(), BatchNumber: 99,
		StateRoot: types.Hash32([32]byte{1}), ReceiptRoot: types.Hash32([32]byte{2})})
	require.Error(t, err)
	_, err = server.SetEmergency(goCtx, &types.MsgSetEmergency{Authority: e.k.Authority(e.ctx), Enabled: true})
	require.NoError(t, err)
	require.True(t, e.k.GetEmergency(e.ctx))
	batch := vectorNumber(t, e.withdrawal, "batch_number")
	_, err = server.RegisterCheckpoint(goCtx, &types.MsgRegisterCheckpoint{Authority: e.k.Authority(e.ctx), BatchNumber: batch,
		StateRoot: types.Hash32([32]byte{1}), ReceiptRoot: types.Hash32([32]byte{2})})
	require.ErrorIs(t, err, types.ErrInvalidCheckpoint, "a finalized checkpoint is immutable")
}
