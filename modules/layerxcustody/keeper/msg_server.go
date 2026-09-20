package keeper

import (
	"context"

	"github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

type msgServer struct{ k *Keeper }

// NewMsgServerImpl returns the custody Msg service.
func NewMsgServerImpl(k *Keeper) types.MsgServer { return msgServer{k} }

var _ types.MsgServer = msgServer{}

func (s msgServer) UpdateParams(goCtx context.Context, msg *types.MsgUpdateParams) (*types.MsgUpdateParamsResponse, error) {
	ctx := sdk.UnwrapSDKContext(goCtx)
	if err := s.k.requireAuthority(ctx, msg.Authority); err != nil {
		return nil, err
	}
	return &types.MsgUpdateParamsResponse{}, s.k.SetParams(ctx, msg.Params)
}

func (s msgServer) SetAsset(goCtx context.Context, msg *types.MsgSetAsset) (*types.MsgSetAssetResponse, error) {
	ctx := sdk.UnwrapSDKContext(goCtx)
	if err := s.k.requireAuthority(ctx, msg.Authority); err != nil {
		return nil, err
	}
	return &types.MsgSetAssetResponse{}, s.k.SetAsset(ctx, msg.Asset)
}

func (s msgServer) RegisterCheckpoint(goCtx context.Context, msg *types.MsgRegisterCheckpoint) (*types.MsgRegisterCheckpointResponse, error) {
	ctx := sdk.UnwrapSDKContext(goCtx)
	if err := s.k.requireAuthority(ctx, msg.Authority); err != nil {
		return nil, err
	}
	stateRoot, err := types.ParseNonZeroHash32(msg.StateRoot)
	if err != nil {
		return nil, err
	}
	receiptRoot, err := types.ParseNonZeroHash32(msg.ReceiptRoot)
	if err != nil {
		return nil, err
	}
	return &types.MsgRegisterCheckpointResponse{}, s.k.RegisterCheckpoint(ctx, msg.BatchNumber, stateRoot, receiptRoot)
}

func (s msgServer) SetEmergency(goCtx context.Context, msg *types.MsgSetEmergency) (*types.MsgSetEmergencyResponse, error) {
	ctx := sdk.UnwrapSDKContext(goCtx)
	if err := s.k.requireAuthority(ctx, msg.Authority); err != nil {
		return nil, err
	}
	return &types.MsgSetEmergencyResponse{}, s.k.SetEmergency(ctx, msg.Enabled)
}

func (s msgServer) CancelClaim(goCtx context.Context, msg *types.MsgCancelClaim) (*types.MsgCancelClaimResponse, error) {
	ctx := sdk.UnwrapSDKContext(goCtx)
	if err := s.k.requireAuthority(ctx, msg.Authority); err != nil {
		return nil, err
	}
	claimID, err := types.ParseNonZeroHash32(msg.ClaimId)
	if err != nil {
		return nil, err
	}
	_, err = s.k.CancelClaim(ctx, claimID)
	return &types.MsgCancelClaimResponse{}, err
}

func (s msgServer) RequestWithdrawal(goCtx context.Context, msg *types.MsgRequestWithdrawal) (*types.MsgRequestWithdrawalResponse, error) {
	claim, err := s.k.RequestWithdrawal(sdk.UnwrapSDKContext(goCtx), WithdrawalEvidence{Receipt: msg.Receipt,
		Proof: msg.Proof, Header: msg.Header, HeaderSignature: msg.HeaderSignature})
	if err != nil {
		return nil, err
	}
	return &types.MsgRequestWithdrawalResponse{ClaimId: claim.ClaimId, AvailableAt: claim.AvailableAt}, nil
}

func (s msgServer) FinaliseWithdrawal(goCtx context.Context, msg *types.MsgFinaliseWithdrawal) (*types.MsgFinaliseWithdrawalResponse, error) {
	result, err := s.k.FinaliseWithdrawal(sdk.UnwrapSDKContext(goCtx), WithdrawalEvidence{Receipt: msg.Receipt,
		Proof: msg.Proof, Header: msg.Header, HeaderSignature: msg.HeaderSignature})
	if err != nil {
		return nil, err
	}
	return &types.MsgFinaliseWithdrawalResponse{ClaimId: result.Claim.ClaimId}, nil
}

func exitEvidence(witness []byte, batchNumber uint64, account, assetID, recipient string, signature []byte) (ExitEvidence, error) {
	accountID, err := types.ParseNonZeroHash32(account)
	if err != nil {
		return ExitEvidence{}, err
	}
	asset, err := types.ParseNonZeroHash32(assetID)
	if err != nil {
		return ExitEvidence{}, err
	}
	to, err := types.ParseAddress(recipient)
	if err != nil {
		return ExitEvidence{}, err
	}
	return ExitEvidence{Witness: witness, BatchNumber: batchNumber, Account: accountID, AssetID: asset,
		Recipient: to, RecipientSignature: signature}, nil
}

func (s msgServer) RequestForcedExit(goCtx context.Context, msg *types.MsgRequestForcedExit) (*types.MsgRequestForcedExitResponse, error) {
	evidence, err := exitEvidence(msg.Witness, msg.BatchNumber, msg.Account, msg.AssetId, msg.Recipient, msg.RecipientSignature)
	if err != nil {
		return nil, err
	}
	claim, err := s.k.RequestForcedExit(sdk.UnwrapSDKContext(goCtx), evidence)
	if err != nil {
		return nil, err
	}
	return &types.MsgRequestForcedExitResponse{ClaimId: claim.ClaimId, AvailableAt: claim.AvailableAt}, nil
}

func (s msgServer) ExecuteForcedExit(goCtx context.Context, msg *types.MsgExecuteForcedExit) (*types.MsgExecuteForcedExitResponse, error) {
	evidence, err := exitEvidence(msg.Witness, msg.BatchNumber, msg.Account, msg.AssetId, msg.Recipient, msg.RecipientSignature)
	if err != nil {
		return nil, err
	}
	result, err := s.k.ExecuteForcedExit(sdk.UnwrapSDKContext(goCtx), evidence)
	if err != nil {
		return nil, err
	}
	return &types.MsgExecuteForcedExitResponse{ClaimId: result.Claim.ClaimId}, nil
}
