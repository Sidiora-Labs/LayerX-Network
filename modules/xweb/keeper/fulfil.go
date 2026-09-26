package keeper

import (
	"bytes"
	"fmt"
	"strings"

	bridgetypes "github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	"github.com/sidiora-labs/paxeer-network/modules/xweb/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

// verifySignatures returns the attestors that signed digest. As on the
// bridge, signatures must be ordered by strictly ascending signer address,
// which also refuses a repeated signer; a malformed or high-s signature, a
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
		recovered, err := bridgetypes.RecoverSigner(bridgetypes.Hash32(digest), signature)
		if err != nil {
			return nil, types.ErrBadSignature.Wrapf("signature %d: %v", index, err)
		}
		signer := types.Address20(recovered)
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

// verifySingle returns the one signer of a single-level fulfilment: exactly
// one signature, recovering to the attestor the request names, who must still
// be registered.
func verifySingle(set types.AttestorSet, named types.Address20, digest types.Hash32,
	signatures [][]byte) ([]types.Address20, error) {
	if len(signatures) != 1 {
		return nil, types.ErrBadSignature.Wrapf("the single level takes exactly one signature, got %d", len(signatures))
	}
	recovered, err := bridgetypes.RecoverSigner(bridgetypes.Hash32(digest), signatures[0])
	if err != nil {
		return nil, types.ErrBadSignature.Wrapf("signature 0: %v", err)
	}
	if signer := types.Address20(recovered); signer != named {
		return nil, types.ErrBadSignature.Wrapf("signature from %s, the single level names %s", signer.Hex(), named.Hex())
	}
	if !set.Has(named) {
		return nil, types.ErrUnknownAttestor.Wrapf("the single level names %s, no longer registered", named.Hex())
	}
	return []types.Address20{named}, nil
}

// Attestation rebuilds the origin-1 attestation of a stored request for a
// submitted response, content digest and full length.
func (k Keeper) Attestation(ctx sdk.Context, request types.Request, response []byte,
	contentDigest types.Hash32, fullLength uint32) types.Attestation {
	return types.Attestation{
		Origin:        types.OriginEVM,
		NetworkID:     k.evmKeeper.ChainID(ctx),
		Requester:     types.EVMRequester(request.Requester),
		RequestID:     request.ID,
		Kind:          request.Kind,
		PayloadHash:   request.PayloadHash,
		ContentDigest: contentDigest,
		ResponseHash:  types.Keccak(response),
		FullLength:    fullLength,
	}
}

// Fulfil accepts the attested answer to a pending request. It refuses when
// paused, for an unknown, fulfilled or refunded request, a response over
// MaxResponseBytes, a full length shorter than the response, and signatures
// that do not verify over the 188-byte origin-1 preimage: at the threshold
// under the majority level, or one signature from the named attestor under
// the single level.
// On success it stores the result, marks the request fulfilled and splits the
// fee equally among the signers' payout accounts with the integer remainder
// to the lowest signer. The caller delivers the callback and records its
// outcome with RecordCallback; the fulfilment stands whatever that outcome.
func (k Keeper) Fulfil(ctx sdk.Context, id uint64, response []byte, contentDigest types.Hash32,
	fullLength uint32, signatures [][]byte) (types.Request, types.Result, error) {
	if k.IsPaused(ctx) {
		return types.Request{}, types.Result{}, types.ErrPaused
	}
	request, found := k.GetRequest(ctx, id)
	if !found {
		return types.Request{}, types.Result{}, types.ErrUnknownRequest.Wrapf("request %d", id)
	}
	switch request.Status {
	case types.StatusFulfilled:
		return types.Request{}, types.Result{}, types.ErrAlreadyFulfilled.Wrapf("request %d", id)
	case types.StatusRefunded:
		return types.Request{}, types.Result{}, types.ErrRefunded.Wrapf("request %d", id)
	}
	if len(response) > types.MaxResponseBytes {
		return types.Request{}, types.Result{}, types.ErrResponseTooLarge.Wrapf("%d bytes, bound %d",
			len(response), types.MaxResponseBytes)
	}
	if uint64(fullLength) < uint64(len(response)) {
		return types.Request{}, types.Result{}, types.ErrInvalidLength.Wrapf("full length %d, response %d bytes",
			fullLength, len(response))
	}
	set := k.GetAttestorSet(ctx)
	attested := types.Digest(k.Attestation(ctx, request, response, contentDigest, fullLength))
	var signers []types.Address20
	var err error
	switch request.Level {
	case types.LevelSingle:
		signers, err = verifySingle(set, request.Attestor, attested, signatures)
	case types.LevelMajority:
		signers, err = verifySignatures(set, attested, signatures)
	default:
		err = types.ErrInvalidLevel.Wrapf("request %d level %d", id, request.Level)
	}
	if err != nil {
		return types.Request{}, types.Result{}, err
	}

	cached, write := ctx.CacheContext()
	share := request.Fee.QuoRaw(int64(len(signers)))
	remainder := request.Fee.Sub(share.MulRaw(int64(len(signers))))
	for index, signer := range signers {
		attestor, _ := set.Find(signer)
		payout, err := sdk.AccAddressFromBech32(attestor.Payout)
		if err != nil {
			return types.Request{}, types.Result{}, types.ErrInvalidAttestors.Wrapf("payout of %s: %v", signer.Hex(), err)
		}
		amount := share
		if index == 0 {
			amount = amount.Add(remainder)
		}
		if !amount.IsPositive() {
			continue
		}
		if err := k.bankKeeper.SendCoins(cached, k.ModuleAddress(), payout, k.feeCoins(amount)); err != nil {
			return types.Request{}, types.Result{}, err
		}
	}
	result := types.Result{
		RequestID:     id,
		Response:      append([]byte(nil), response...),
		ContentDigest: contentDigest,
		FullLength:    fullLength,
		Signers:       signers,
		Height:        ctx.BlockHeight(),
		Callback:      types.CallbackPending,
		Level:         request.Level,
	}
	request.Status = types.StatusFulfilled
	k.setRequest(cached, request)
	k.setResult(cached, result)
	write()
	ctx.EventManager().EmitEvents(cached.EventManager().Events())

	names := make([]string, len(signers))
	for i, signer := range signers {
		names[i] = signer.Hex()
	}
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventFulfilled,
		sdk.NewAttribute(types.AttributeRequestID, fmt.Sprint(id)),
		sdk.NewAttribute(types.AttributeRequester, request.Requester.Hex()),
		sdk.NewAttribute(types.AttributeContentDigest, contentDigest.Hex()),
		sdk.NewAttribute(types.AttributeResponseHash, types.Keccak(response).Hex()),
		sdk.NewAttribute(types.AttributeFullLength, fmt.Sprint(fullLength)),
		sdk.NewAttribute(types.AttributeFee, request.Fee.String()),
		sdk.NewAttribute(types.AttributeSigners, strings.Join(names, ",")),
		sdk.NewAttribute(types.AttributeLevel, fmt.Sprint(request.Level))))
	return request, result, nil
}

// RecordCallback records what the requester's onXWebResponse callback did
// after a fulfilment. It is written once per result.
func (k Keeper) RecordCallback(ctx sdk.Context, id uint64, outcome types.CallbackOutcome, gasUsed uint64) error {
	if outcome == types.CallbackPending || outcome > types.CallbackOutOfGas {
		return types.ErrInvalidRequest.Wrapf("callback outcome %d", outcome)
	}
	result, found := k.GetResult(ctx, id)
	if !found {
		return types.ErrUnknownRequest.Wrapf("no result for request %d", id)
	}
	if result.Callback != types.CallbackPending {
		return types.ErrCallbackRecorded.Wrapf("request %d", id)
	}
	result.Callback = outcome
	result.CallbackGasUsed = gasUsed
	k.setResult(ctx, result)
	ctx.EventManager().EmitEvent(sdk.NewEvent(types.EventCallback,
		sdk.NewAttribute(types.AttributeRequestID, fmt.Sprint(id)),
		sdk.NewAttribute(types.AttributeOutcome, fmt.Sprint(outcome)),
		sdk.NewAttribute(types.AttributeGasUsed, fmt.Sprint(gasUsed))))
	return nil
}
