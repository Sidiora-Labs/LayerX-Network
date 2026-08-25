package keeper

import (
	"context"
	"errors"

	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/cw1155"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/cw20"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/cw721"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/erc1155"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/erc20"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/erc721"
	"github.com/sidiora-labs/paxeer-network/modules/evm/artifacts/native"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	sdkerrors "github.com/sidiora-labs/paxeer-network/sdk/types/errors"
)

var _ types.QueryServer = Querier{}

var ErrMustSpecifyPointer = errors.New("must specify a pointer")
var ErrMustSpecifyPointee = errors.New("must specify a pointee")

// Querier defines a wrapper around the modules/mint keeper providing gRPC method
// handlers.
type Querier struct {
	*Keeper
}

func NewQuerier(k *Keeper) Querier {
	return Querier{Keeper: k}
}

func (q Querier) PaxAddressByEVMAddress(c context.Context, req *types.QueryPaxAddressByEVMAddressRequest) (*types.QueryPaxAddressByEVMAddressResponse, error) {
	ctx := sdk.UnwrapSDKContext(c)
	if req.EvmAddress == "" {
		return nil, sdkerrors.ErrInvalidRequest
	}
	evmAddr := common.HexToAddress(req.EvmAddress)
	addr, found := q.GetPaxAddress(ctx, evmAddr)
	if !found {
		return &types.QueryPaxAddressByEVMAddressResponse{Associated: false}, nil
	}

	return &types.QueryPaxAddressByEVMAddressResponse{PaxAddress: addr.String(), Associated: true}, nil
}

func (q Querier) EVMAddressByPaxAddress(c context.Context, req *types.QueryEVMAddressByPaxAddressRequest) (*types.QueryEVMAddressByPaxAddressResponse, error) {
	ctx := sdk.UnwrapSDKContext(c)
	if req.PaxAddress == "" {
		return nil, sdkerrors.ErrInvalidRequest
	}
	paxAddr, err := sdk.AccAddressFromBech32(req.PaxAddress)
	if err != nil {
		return nil, err
	}
	addr, found := q.GetEVMAddress(ctx, paxAddr)
	if !found {
		return &types.QueryEVMAddressByPaxAddressResponse{Associated: false}, nil
	}

	return &types.QueryEVMAddressByPaxAddressResponse{EvmAddress: addr.Hex(), Associated: true}, nil
}

func (q Querier) StaticCall(c context.Context, req *types.QueryStaticCallRequest) (*types.QueryStaticCallResponse, error) {
	ctx := sdk.UnwrapSDKContext(c)
	if req.To == "" {
		return nil, errors.New("cannot use static call to create contracts")
	}
	if ctx.GasMeter().Limit() == 0 {
		ctx = ctx.WithGasMeter(sdk.NewGasMeterWithMultiplier(ctx, q.QueryConfig.GasLimit))
	}
	to := common.HexToAddress(req.To)
	res, err := q.StaticCallEVM(ctx, q.Keeper.AccountKeeper().GetModuleAddress(types.ModuleName), &to, req.Data)
	if err != nil {
		return nil, err
	}
	return &types.QueryStaticCallResponse{Data: res}, nil
}

func (q Querier) Pointer(c context.Context, req *types.QueryPointerRequest) (*types.QueryPointerResponse, error) {
	if req.Pointee == "" {
		return nil, ErrMustSpecifyPointee
	}
	ctx := sdk.UnwrapSDKContext(c)
	switch req.PointerType {
	case types.PointerType_NATIVE:
		p, v, e := q.GetERC20NativePointer(ctx, req.Pointee)
		if !e {
			return &types.QueryPointerResponse{Exists: e}, nil
		}
		return &types.QueryPointerResponse{
			Pointer: p.Hex(),
			Version: uint32(v),
			Exists:  e,
		}, nil
	case types.PointerType_CW20:
		p, v, e := q.GetERC20CW20Pointer(ctx, req.Pointee)
		if !e {
			return &types.QueryPointerResponse{Exists: e}, nil
		}
		return &types.QueryPointerResponse{
			Pointer: p.Hex(),
			Version: uint32(v),
			Exists:  e,
		}, nil
	case types.PointerType_CW721:
		p, v, e := q.GetERC721CW721Pointer(ctx, req.Pointee)
		if !e {
			return &types.QueryPointerResponse{Exists: e}, nil
		}
		return &types.QueryPointerResponse{
			Pointer: p.Hex(),
			Version: uint32(v),
			Exists:  e,
		}, nil
	case types.PointerType_CW1155:
		p, v, e := q.GetERC1155CW1155Pointer(ctx, req.Pointee)
		if !e {
			return &types.QueryPointerResponse{Exists: e}, nil
		}
		return &types.QueryPointerResponse{
			Pointer: p.Hex(),
			Version: uint32(v),
			Exists:  e,
		}, nil
	case types.PointerType_ERC20:
		p, v, e := q.GetCW20ERC20Pointer(ctx, common.HexToAddress(req.Pointee))
		if !e {
			return &types.QueryPointerResponse{Exists: e}, nil
		}
		return &types.QueryPointerResponse{
			Pointer: p.String(),
			Version: uint32(v),
			Exists:  e,
		}, nil
	case types.PointerType_ERC721:
		p, v, e := q.GetCW721ERC721Pointer(ctx, common.HexToAddress(req.Pointee))
		if !e {
			return &types.QueryPointerResponse{Exists: e}, nil
		}
		return &types.QueryPointerResponse{
			Pointer: p.String(),
			Version: uint32(v),
			Exists:  e,
		}, nil
	case types.PointerType_ERC1155:
		p, v, e := q.GetCW1155ERC1155Pointer(ctx, common.HexToAddress(req.Pointee))
		if !e {
			return &types.QueryPointerResponse{Exists: e}, nil
		}
		return &types.QueryPointerResponse{
			Pointer: p.String(),
			Version: uint32(v),
			Exists:  e,
		}, nil
	default:
		return nil, errors.ErrUnsupported
	}
}

func (q Querier) PointerVersion(c context.Context, req *types.QueryPointerVersionRequest) (*types.QueryPointerVersionResponse, error) {
	ctx := sdk.UnwrapSDKContext(c)
	switch req.PointerType {
	case types.PointerType_NATIVE:
		return &types.QueryPointerVersionResponse{
			Version: uint32(native.CurrentVersion),
		}, nil
	case types.PointerType_CW20:
		return &types.QueryPointerVersionResponse{
			Version: uint32(cw20.CurrentVersion(ctx)),
		}, nil
	case types.PointerType_CW721:
		return &types.QueryPointerVersionResponse{
			Version: uint32(cw721.CurrentVersion),
		}, nil
	case types.PointerType_CW1155:
		return &types.QueryPointerVersionResponse{
			Version: uint32(cw1155.CurrentVersion),
		}, nil
	case types.PointerType_ERC20:
		return &types.QueryPointerVersionResponse{
			Version:  uint32(erc20.CurrentVersion),
			CwCodeId: q.GetStoredPointerCodeID(ctx, types.PointerType_ERC20),
		}, nil
	case types.PointerType_ERC721:
		return &types.QueryPointerVersionResponse{
			Version:  uint32(erc721.CurrentVersion),
			CwCodeId: q.GetStoredPointerCodeID(ctx, types.PointerType_ERC721),
		}, nil
	case types.PointerType_ERC1155:
		return &types.QueryPointerVersionResponse{
			Version:  uint32(erc1155.CurrentVersion),
			CwCodeId: q.GetStoredPointerCodeID(ctx, types.PointerType_ERC1155),
		}, nil
	default:
		return nil, errors.ErrUnsupported
	}
}

func (q Querier) Pointee(c context.Context, req *types.QueryPointeeRequest) (*types.QueryPointeeResponse, error) {
	if req.Pointer == "" {
		return nil, ErrMustSpecifyPointer
	}
	ctx := sdk.UnwrapSDKContext(c)
	switch req.PointerType {
	case types.PointerType_NATIVE:
		p, v, e := q.GetNativePointee(ctx, req.Pointer)
		if !e {
			return &types.QueryPointeeResponse{Exists: e}, nil
		}
		return &types.QueryPointeeResponse{
			Pointee: p,
			Version: uint32(v),
			Exists:  e,
		}, nil
	case types.PointerType_CW20:
		p, v, e := q.GetCW20Pointee(ctx, common.HexToAddress(req.Pointer))
		if !e {
			return &types.QueryPointeeResponse{Exists: e}, nil
		}
		return &types.QueryPointeeResponse{
			Pointee: p,
			Version: uint32(v),
			Exists:  e,
		}, nil
	case types.PointerType_CW721:
		p, v, e := q.GetCW721Pointee(ctx, common.HexToAddress(req.Pointer))
		if !e {
			return &types.QueryPointeeResponse{Exists: e}, nil
		}
		return &types.QueryPointeeResponse{
			Pointee: p,
			Version: uint32(v),
			Exists:  e,
		}, nil
	case types.PointerType_CW1155:
		p, v, e := q.GetCW1155Pointee(ctx, common.HexToAddress(req.Pointer))
		if !e {
			return &types.QueryPointeeResponse{Exists: e}, nil
		}
		return &types.QueryPointeeResponse{
			Pointee: p,
			Version: uint32(v),
			Exists:  e,
		}, nil
	case types.PointerType_ERC20:
		p, v, e := q.GetERC20Pointee(ctx, req.Pointer)
		if !e {
			return &types.QueryPointeeResponse{Exists: e}, nil
		}
		return &types.QueryPointeeResponse{
			Pointee: p.Hex(),
			Version: uint32(v),
			Exists:  e,
		}, nil
	case types.PointerType_ERC721:
		p, v, e := q.GetERC721Pointee(ctx, req.Pointer)
		if !e {
			return &types.QueryPointeeResponse{Exists: e}, nil
		}
		return &types.QueryPointeeResponse{
			Pointee: p.Hex(),
			Version: uint32(v),
			Exists:  e,
		}, nil
	case types.PointerType_ERC1155:
		p, v, e := q.GetERC1155Pointee(ctx, req.Pointer)
		if !e {
			return &types.QueryPointeeResponse{Exists: e}, nil
		}
		return &types.QueryPointeeResponse{
			Pointee: p.Hex(),
			Version: uint32(v),
			Exists:  e,
		}, nil
	default:
		return nil, errors.ErrUnsupported
	}
}
