package keeper

import (
	"fmt"
	"strings"

	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// RegisterChain adds or replaces a remote chain. Re-registering updates the
// vault, finality depth or enabled flag; assets and nullifiers are kept.
func (k Keeper) RegisterChain(ctx sdk.Context, msg types.MsgRegisterChain) error {
	if err := msg.ValidateBasic(); err != nil {
		return err
	}
	if err := k.requireAuthority(ctx, msg.Authority); err != nil {
		return err
	}
	k.setChain(ctx, msg.Chain)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventChainRegistered,
		sdk.NewAttribute(types.AttributeChainID, fmt.Sprint(msg.Chain.ChainID)),
		sdk.NewAttribute(types.AttributeVault, msg.Chain.Vault.Hex()),
		sdk.NewAttribute(types.AttributeFinalityDepth, fmt.Sprint(msg.Chain.FinalityDepth)),
		sdk.NewAttribute(types.AttributeEnabled, fmt.Sprint(msg.Chain.Enabled))))
	return nil
}

// SetAttestors replaces the attestor set and threshold.
func (k Keeper) SetAttestors(ctx sdk.Context, msg types.MsgSetAttestors) error {
	if err := msg.ValidateBasic(); err != nil {
		return err
	}
	if err := k.requireAuthority(ctx, msg.Authority); err != nil {
		return err
	}
	k.setAttestorSet(ctx, msg.Set)
	signers := make([]string, 0, len(msg.Set.Attestors))
	for _, attestor := range msg.Set.Attestors {
		signers = append(signers, attestor.Signer.Hex())
	}
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventAttestorsSet,
		sdk.NewAttribute(types.AttributeThreshold, fmt.Sprint(msg.Set.Threshold)),
		sdk.NewAttribute(types.AttributeAttestors, strings.Join(signers, ","))))
	return nil
}

// SetCap sets the caps of a registered chain's asset. The first cap of an
// asset creates its tokenfactory denom with the bridge module account as
// admin.
func (k Keeper) SetCap(ctx sdk.Context, msg types.MsgSetCap) error {
	if err := msg.ValidateBasic(); err != nil {
		return err
	}
	if err := k.requireAuthority(ctx, msg.Authority); err != nil {
		return err
	}
	if _, found := k.GetChain(ctx, msg.ChainID); !found {
		return types.ErrUnknownChain.Wrapf("chain %d", msg.ChainID)
	}
	record, found := k.GetAsset(ctx, msg.ChainID, msg.Asset)
	if !found {
		denom, err := k.tokenFactory.CreateDenom(ctx, k.ModuleAddress().String(), types.Subdenom(msg.ChainID, msg.Asset))
		if err != nil {
			return err
		}
		if denom != types.Denom(msg.ChainID, msg.Asset) {
			return types.ErrInvalidCap.Wrapf("tokenfactory created %s", denom)
		}
		record = types.BridgedAsset{ChainID: msg.ChainID, Asset: msg.Asset, Denom: denom}
		k.setAsset(ctx, record)
	}
	k.setCap(ctx, types.Cap{Denom: record.Denom, MaxInFlight: msg.MaxInFlight, MaxPerTx: msg.MaxPerTx})
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventCapSet,
		sdk.NewAttribute(types.AttributeChainID, fmt.Sprint(msg.ChainID)),
		sdk.NewAttribute(types.AttributeAsset, msg.Asset.Hex()),
		sdk.NewAttribute(types.AttributeDenom, record.Denom),
		sdk.NewAttribute(types.AttributeMaxInFlight, msg.MaxInFlight.String()),
		sdk.NewAttribute(types.AttributeMaxPerTx, msg.MaxPerTx.String())))
	return nil
}

// Pause stops every bridgeIn and bridgeOut.
func (k Keeper) Pause(ctx sdk.Context, msg types.MsgPause) error {
	if err := msg.ValidateBasic(); err != nil {
		return err
	}
	if err := k.requireAuthority(ctx, msg.Authority); err != nil {
		return err
	}
	k.setPaused(ctx, true)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventPaused))
	return nil
}

func (k Keeper) Unpause(ctx sdk.Context, msg types.MsgUnpause) error {
	if err := msg.ValidateBasic(); err != nil {
		return err
	}
	if err := k.requireAuthority(ctx, msg.Authority); err != nil {
		return err
	}
	k.setPaused(ctx, false)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventUnpaused))
	return nil
}
