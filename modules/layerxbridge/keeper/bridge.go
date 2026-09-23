package keeper

import (
	"bytes"
	"fmt"
	"math/big"
	"strings"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	tokenfactorytypes "github.com/sidiora-labs/paxeer-network/modules/tokenfactory/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// BridgeInResult is what one accepted bridgeIn minted.
type BridgeInResult struct {
	Denom   string
	Amount  sdk.Int
	Digest  types.Hash32
	Signers []types.Address20
}

// BridgeOutResult is the outbound record a bridgeOut issued.
type BridgeOutResult struct {
	Denom string
	Nonce uint64
}

// verifySignatures returns the attestors that signed digest. As on the
// PaxeerXVault, signatures must be ordered by strictly ascending signer
// address, which also refuses a repeated signer; a malformed signature, a
// non-member signer or fewer signers than the threshold is refused.
func verifySignatures(set types.AttestorSet, digest types.Hash32, signatures [][]byte) ([]types.Address20, error) {
	if set.Threshold == 0 {
		return nil, types.ErrBelowThreshold.Wrap("no attestor set")
	}
	if len(signatures) > types.MaxAttestors {
		return nil, types.ErrBadSignature.Wrapf("more than %d signatures", types.MaxAttestors)
	}
	signers := make([]types.Address20, 0, len(signatures))
	for index, signature := range signatures {
		signer, err := types.RecoverSigner(digest, signature)
		if err != nil {
			return nil, err
		}
		if !set.Has(signer) {
			return nil, types.ErrBadSignature.Wrapf("signature %d is from non-attestor %s", index, signer.Hex())
		}
		if index > 0 && bytes.Compare(signers[index-1][:], signer[:]) >= 0 {
			return nil, types.ErrBadSignature.Wrapf("signature %d is not in strictly ascending signer order", index)
		}
		signers = append(signers, signer)
	}
	if len(signers) < int(set.Threshold) {
		return nil, types.ErrBelowThreshold.Wrapf("%d of %d", len(signers), set.Threshold)
	}
	return signers, nil
}

// BridgeIn mints an attested remote deposit to its recipient. It refuses when
// paused, for an unregistered or disabled chain, a vault other than the
// registered one, an asset without a denom, an amount over the per-tx or
// in-flight cap, a consumed nullifier, or fewer attestor signatures than the
// threshold. Nothing is written unless the mint succeeds.
func (k Keeper) BridgeIn(ctx sdk.Context, in types.BridgeIn, signatures [][]byte) (BridgeInResult, error) {
	if k.IsPaused(ctx) {
		return BridgeInResult{}, types.ErrPaused
	}
	chain, found := k.GetChain(ctx, in.ChainID)
	if !found {
		return BridgeInResult{}, types.ErrUnknownChain.Wrapf("chain %d", in.ChainID)
	}
	if !chain.Enabled {
		return BridgeInResult{}, types.ErrChainDisabled.Wrapf("chain %d", in.ChainID)
	}
	if chain.Vault != in.Vault {
		return BridgeInResult{}, types.ErrVaultMismatch
	}
	if in.Amount == nil || in.Amount.Sign() <= 0 || in.Amount.BitLen() > 255 {
		return BridgeInResult{}, types.ErrInvalidRequest.Wrap("amount must be positive and below 2^255")
	}
	recipientAddress, ok := types.RecipientAddress(in.Recipient)
	if !ok {
		return BridgeInResult{}, types.ErrInvalidRequest.Wrap("recipient is not a non-zero left-padded EVM address")
	}
	asset, found := k.GetAsset(ctx, in.ChainID, in.Asset)
	if !found {
		return BridgeInResult{}, types.ErrUnknownAsset.Wrapf("asset %s of chain %d", in.Asset.Hex(), in.ChainID)
	}
	if k.IsNullified(ctx, in.Nullifier()) {
		return BridgeInResult{}, types.ErrNullified
	}
	amount := sdk.NewIntFromBigInt(in.Amount)
	limits, _ := k.GetCap(ctx, asset.Denom)
	if limits.MaxPerTx.IsNil() || amount.GT(limits.MaxPerTx) {
		return BridgeInResult{}, types.ErrCapExceeded.Wrap("over the per-transaction cap")
	}
	inFlight := k.InFlight(ctx, asset.Denom).Add(amount)
	if limits.MaxInFlight.IsNil() || inFlight.GT(limits.MaxInFlight) {
		return BridgeInResult{}, types.ErrCapExceeded.Wrap("over the in-flight cap")
	}
	digest := types.InboundDigest(in)
	signers, err := verifySignatures(k.GetAttestorSet(ctx), digest, signatures)
	if err != nil {
		return BridgeInResult{}, err
	}

	cached, write := ctx.CacheContext()
	coin := sdk.NewCoin(asset.Denom, amount)
	if _, err := k.tokenFactoryMsg.Mint(sdk.WrapSDKContext(cached),
		&tokenfactorytypes.MsgMint{Sender: k.ModuleAddress().String(), Amount: coin}); err != nil {
		return BridgeInResult{}, err
	}
	recipient := k.evmKeeper.GetPaxAddressOrDefault(cached, common.Address(recipientAddress))
	if err := k.bankKeeper.SendCoins(cached, k.ModuleAddress(), recipient, sdk.NewCoins(coin)); err != nil {
		return BridgeInResult{}, err
	}
	k.setNullifier(cached, in.Nullifier())
	k.setInFlight(cached, asset.Denom, inFlight)
	write()
	ctx.EventManager().EmitEvents(cached.EventManager().Events())

	names := make([]string, len(signers))
	for i, signer := range signers {
		names[i] = signer.Hex()
	}
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventBridgeIn,
		sdk.NewAttribute(types.AttributeChainID, fmt.Sprint(in.ChainID)),
		sdk.NewAttribute(types.AttributeTxHash, hexBytes(in.TxHash[:])),
		sdk.NewAttribute(types.AttributeLogIndex, fmt.Sprint(in.LogIndex)),
		sdk.NewAttribute(types.AttributeRecipient, recipient.String()),
		sdk.NewAttribute(types.AttributeAsset, in.Asset.Hex()),
		sdk.NewAttribute(types.AttributeDenom, asset.Denom),
		sdk.NewAttribute(types.AttributeAmount, amount.String()),
		sdk.NewAttribute(types.AttributeSigners, strings.Join(names, ","))))
	return BridgeInResult{Denom: asset.Denom, Amount: amount, Digest: digest, Signers: signers}, nil
}

// BridgeOut burns amount of the bridged denom of (chain, asset) from sender
// and issues the chain's next outbound nonce, which the remote vault releases
// against. recipient is the address the vault releases to.
func (k Keeper) BridgeOut(ctx sdk.Context, sender common.Address, chainID uint64, assetAddress types.Address20,
	amountValue *big.Int, recipient types.Address20) (BridgeOutResult, error) {
	if k.IsPaused(ctx) {
		return BridgeOutResult{}, types.ErrPaused
	}
	chain, found := k.GetChain(ctx, chainID)
	if !found {
		return BridgeOutResult{}, types.ErrUnknownChain.Wrapf("chain %d", chainID)
	}
	if !chain.Enabled {
		return BridgeOutResult{}, types.ErrChainDisabled.Wrapf("chain %d", chainID)
	}
	asset, found := k.GetAsset(ctx, chainID, assetAddress)
	if !found {
		return BridgeOutResult{}, types.ErrUnknownAsset.Wrapf("asset %s of chain %d", assetAddress.Hex(), chainID)
	}
	if amountValue == nil || amountValue.Sign() <= 0 || amountValue.BitLen() > 255 {
		return BridgeOutResult{}, types.ErrInvalidRequest.Wrap("amount must be positive and below 2^255")
	}
	if recipient == (types.Address20{}) {
		return BridgeOutResult{}, types.ErrInvalidRequest.Wrap("zero recipient")
	}
	amount := sdk.NewIntFromBigInt(amountValue)
	inFlight := k.InFlight(ctx, asset.Denom)
	if amount.GT(inFlight) {
		return BridgeOutResult{}, types.ErrInvalidRequest.Wrap("amount exceeds the bridged supply")
	}

	cached, write := ctx.CacheContext()
	coin := sdk.NewCoin(asset.Denom, amount)
	from := k.evmKeeper.GetPaxAddressOrDefault(cached, sender)
	if err := k.bankKeeper.SendCoins(cached, from, k.ModuleAddress(), sdk.NewCoins(coin)); err != nil {
		return BridgeOutResult{}, err
	}
	if _, err := k.tokenFactoryMsg.Burn(sdk.WrapSDKContext(cached),
		&tokenfactorytypes.MsgBurn{Sender: k.ModuleAddress().String(), Amount: coin}); err != nil {
		return BridgeOutResult{}, err
	}
	k.setInFlight(cached, asset.Denom, inFlight.Sub(amount))
	nonce := k.OutboundNonce(cached, chainID) + 1
	k.setOutboundNonce(cached, chainID, nonce)
	write()
	ctx.EventManager().EmitEvents(cached.EventManager().Events())

	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventBridgeOut,
		sdk.NewAttribute(types.AttributeChainID, fmt.Sprint(chainID)),
		sdk.NewAttribute(types.AttributeAsset, assetAddress.Hex()),
		sdk.NewAttribute(types.AttributeDenom, asset.Denom),
		sdk.NewAttribute(types.AttributeAmount, amount.String()),
		sdk.NewAttribute(types.AttributeRecipient, recipient.Hex()),
		sdk.NewAttribute(types.AttributeSender, from.String()),
		sdk.NewAttribute(types.AttributeNonce, fmt.Sprint(nonce))))
	return BridgeOutResult{Denom: asset.Denom, Nonce: nonce}, nil
}
