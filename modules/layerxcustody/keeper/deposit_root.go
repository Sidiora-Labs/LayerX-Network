package keeper

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
	"github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

const (
	// DepositRootEvidenceVersion is the evidence version a registration commits to.
	DepositRootEvidenceVersion uint16 = 2
	// MaxDepositRootLeaves bounds the leaf ordering of one registration.
	MaxDepositRootLeaves = 4096
)

var depositRootDomain = []byte("LX:PAXEER:DEPOSIT:ROOT:v1")

var depositRootCommitment = func() abi.Arguments {
	kind := func(name string) abi.Type {
		out, err := abi.NewType(name, "", nil)
		if err != nil {
			panic(err)
		}
		return out
	}
	return abi.Arguments{{Type: kind("uint16")}, {Type: kind("bytes")}, {Type: kind("bytes")}, {Type: kind("bytes32[]")}}
}()

func (k *Keeper) setDepositRoot(ctx sdk.Context, registration types.DepositRootRegistration) {
	checkpointID, _ := types.ParseHash32(registration.CheckpointId)
	k.store(ctx).Set(types.DepositRootKey(checkpointID), k.cdc.MustMarshal(&registration))
}

func (k *Keeper) GetDepositRoot(ctx sdk.Context, checkpointID [32]byte) (types.DepositRootRegistration, bool) {
	var registration types.DepositRootRegistration
	bz := k.store(ctx).Get(types.DepositRootKey(checkpointID))
	if bz == nil {
		return registration, false
	}
	k.cdc.MustUnmarshal(bz, &registration)
	return registration, true
}

// RegisterDepositRoot records the deposit root of a finalized checkpoint. The
// registration is the domain followed by the checkpoint identifier, its state
// root, the deposit root, the custody reference, the network and the protocol
// version. Only the account that submitted the checkpoint may register it, the
// checkpoint must be final under that state root, and the registration must
// carry the deposit root authority's Ed25519 signature. The leaf ordering is
// bound into the commitment, not checked against the root.
func (k *Keeper) RegisterDepositRoot(ctx sdk.Context, proposer sdk.AccAddress, registration, signature []byte,
	leafOrdering [][32]byte) (types.DepositRootRegistration, error) {
	offset := len(depositRootDomain)
	if len(registration) != offset+134 || len(signature) != 64 || len(leafOrdering) == 0 ||
		len(leafOrdering) > MaxDepositRootLeaves || !bytes.Equal(registration[:offset], depositRootDomain) {
		return types.DepositRootRegistration{}, types.ErrInvalidDepositRoot
	}
	var checkpointID, stateRoot, depositRoot, custodyReference [32]byte
	copy(checkpointID[:], registration[offset:])
	copy(stateRoot[:], registration[offset+32:])
	copy(depositRoot[:], registration[offset+64:])
	copy(custodyReference[:], registration[offset+96:])
	network := binary.BigEndian.Uint32(registration[offset+128:])
	protocol := binary.BigEndian.Uint16(registration[offset+132:])

	checkpoint, found := k.Anchor().FinalizedCheckpoint(ctx, checkpointID)
	if !found {
		return types.DepositRootRegistration{}, sdkerrors.Wrap(types.ErrInvalidDepositRoot, "checkpoint is not final")
	}
	if !checkpoint.Proposer.Equals(proposer) {
		return types.DepositRootRegistration{}, types.ErrDepositRootProposer
	}
	params := k.GetParams(ctx)
	if checkpoint.StateRoot != stateRoot || depositRoot == ([32]byte{}) || custodyReference == ([32]byte{}) ||
		network != checkpoint.NetworkID || network != params.NetworkId || protocol != checkpoint.ProtocolVersion {
		return types.DepositRootRegistration{}, types.ErrInvalidDepositRoot
	}
	if _, exists := k.GetDepositRoot(ctx, checkpointID); exists {
		return types.DepositRootRegistration{}, types.ErrDepositRootExists
	}
	authority, err := types.ParseNonZeroHash32(params.DepositRootAuthority)
	if err != nil {
		return types.DepositRootRegistration{}, sdkerrors.Wrap(types.ErrInvalidDepositRoot, "no deposit root authority")
	}
	var signed [64]byte
	copy(signed[:], signature)
	if err := verify.Ed25519(authority, signed, registration); err != nil {
		return types.DepositRootRegistration{}, sdkerrors.Wrap(types.ErrInvalidDepositRoot, "authority signature")
	}
	encoded, err := depositRootCommitment.Pack(DepositRootEvidenceVersion, registration, signature, leafOrdering)
	if err != nil {
		return types.DepositRootRegistration{}, sdkerrors.Wrap(types.ErrInvalidDepositRoot, err.Error())
	}
	commitment := sha256.Sum256(encoded)
	recorded := types.DepositRootRegistration{CheckpointId: types.Hash32(checkpointID),
		DepositRoot: types.Hash32(depositRoot), Commitment: types.Hash32(commitment)}
	k.setDepositRoot(ctx, recorded)
	return recorded, ctx.EventManager().EmitTypedEvent(&types.EventDepositRootRegistered{
		CheckpointId: recorded.CheckpointId, DepositRoot: recorded.DepositRoot, Commitment: recorded.Commitment})
}
