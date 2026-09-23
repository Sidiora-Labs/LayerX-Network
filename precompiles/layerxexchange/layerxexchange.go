// Package layerxexchange is the precompile through which EVM accounts and
// contracts submit LayerX exchange intents and read finalized LayerX exchange
// state.
//
// Every write records a pending intent in the layerxexchange module, keyed by
// its intent id, and is emitted both as an EVM log from this address and as a
// Cosmos typed event for the LayerX intent router. No order is matched on
// Paxeer. Margin moves only into the layerxcustody module account: a margin
// deposit is a layerxcustody deposit, and a margin withdrawal is paid by a
// later proof-carrying layerxCustody withdrawal.
//
// Views verify a caller-supplied native state witness against the finalized
// state root the anchor holds for the named batch and never read over the
// network.
//
// Gas is charged by the EVM from RequiredGas before Execute runs:
//
//	gas = BaseGas
//	    + GasPerByte      * len(calldata after the selector)
//	    + GasPerSignature * signatures(method)
//	    + GasPerProofNode * proofNodes(method, args)
//	    + GasPerWrite     * writes(method)
//
// signatures is 0 for every method. proofNodes is len(witness)/32 for
// getMarket, getOrder, getPosition and getMargin, 0 otherwise. writes is the
// fixed number of state slots a method may touch: depositMargin and
// depositMarginToken 11 (the custody deposit's 8 and the intent's 3), the
// other writes 3, views 0.
package layerxexchange

import (
	"embed"
	"errors"
	"fmt"
	"math/big"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/ethereum/go-ethereum/core/vm"
	custodytypes "github.com/sidiora-labs/paxeer-network/modules/layerxcustody/types"
	exchangekeeper "github.com/sidiora-labs/paxeer-network/modules/layerxexchange/keeper"
	"github.com/sidiora-labs/paxeer-network/modules/layerxexchange/types"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	"github.com/sidiora-labs/paxeer-network/precompiles/utils"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
)

const (
	DepositMarginMethod      = "depositMargin"
	DepositMarginTokenMethod = "depositMarginToken"
	WithdrawMarginMethod     = "withdrawMargin"
	PlaceOrderMethod         = "placeOrder"
	CancelOrderMethod        = "cancelOrder"
	RequestSettlementMethod  = "requestSettlement"
	GetIntentMethod          = "getIntent"
	IntentNonceMethod        = "intentNonce"
	GetMarketMethod          = "getMarket"
	GetOrderMethod           = "getOrder"
	GetPositionMethod        = "getPosition"
	GetMarginMethod          = "getMargin"

	MarginDepositedEvent           = "MarginDeposited"
	MarginWithdrawalRequestedEvent = "MarginWithdrawalRequested"
	OrderPlacedEvent               = "OrderPlaced"
	OrderCancelRequestedEvent      = "OrderCancelRequested"
	SettlementRequestedEvent       = "SettlementRequested"
)

const (
	ExchangeAddress = types.ExchangeAddress
	PrecompileName  = "layerxExchange"

	BaseGas         uint64 = 3000
	GasPerByte      uint64 = 16
	GasPerSignature uint64 = 4000
	GasPerProofNode uint64 = 100
	GasPerWrite     uint64 = 5000
)

//go:embed abi.json
var f embed.FS

// ExchangeKeepers is the accessor the application's precompile keepers
// provide for the layerxexchange keeper.
type ExchangeKeepers interface {
	LayerXExchangeK() *exchangekeeper.Keeper
}

// Intent is the ABI tuple of a recorded intent.
type Intent struct {
	IntentId    [32]byte //nolint:revive,stylecheck
	Kind        uint8
	Status      uint8
	Owner       common.Address
	Nonce       uint64
	Height      uint64
	Account     [32]byte
	AssetId     [32]byte //nolint:revive,stylecheck
	Denom       string
	Amount      *big.Int
	DepositId   [32]byte //nolint:revive,stylecheck
	MarketId    [32]byte //nolint:revive,stylecheck
	Side        uint8
	Price       *big.Int
	Quantity    *big.Int
	TimeInForce uint8
	OrderId     [32]byte //nolint:revive,stylecheck
	PositionId  [32]byte //nolint:revive,stylecheck
}

// StateRecord is the ABI tuple of a LayerX state entry proven under a
// finalized state root.
type StateRecord struct {
	BatchNumber uint64
	StateRoot   [32]byte
	Key         []byte
	Value       []byte
}

// Margin is the ABI tuple of a LayerX account balance proven under a
// finalized state root.
type Margin struct {
	BatchNumber uint64
	StateRoot   [32]byte
	Account     [32]byte
	AssetId     [32]byte //nolint:revive,stylecheck
	Balance     *big.Int
	Frozen      bool
}

type PrecompileExecutor struct {
	abi        abi.ABI
	address    common.Address
	keeper     *exchangekeeper.Keeper
	bankKeeper utils.BankKeeper
	evmKeeper  utils.EVMKeeper
}

func NewPrecompile(keepers utils.Keepers) (*pcommon.Precompile, error) {
	newAbi := pcommon.MustGetABI(f, "abi.json")
	p := &PrecompileExecutor{
		abi:        newAbi,
		address:    common.HexToAddress(ExchangeAddress),
		bankKeeper: keepers.BankK(),
		evmKeeper:  keepers.EVMK(),
	}
	if provider, ok := keepers.(ExchangeKeepers); ok {
		p.keeper = provider.LayerXExchangeK()
	}
	return pcommon.NewPrecompile(newAbi, p, p.address, PrecompileName).WithRevertReasons(), nil
}

// Signatures returns the Ed25519 verifications a method performs.
func Signatures(string) uint64 { return 0 }

// Writes returns the fixed state-slot bound of a method.
func Writes(method string) uint64 {
	switch method {
	case DepositMarginMethod, DepositMarginTokenMethod:
		return 11
	case WithdrawMarginMethod, PlaceOrderMethod, CancelOrderMethod, RequestSettlementMethod:
		return 3
	default:
		return 0
	}
}

// proofArgument is the index of the argument carrying the state witness.
func proofArgument(method string) (int, bool) {
	switch method {
	case GetMarketMethod:
		return 2, true
	case GetOrderMethod, GetPositionMethod, GetMarginMethod:
		return 3, true
	default:
		return 0, false
	}
}

// Gas is the documented formula over already-measured quantities.
func Gas(inputBytes, signatures, proofNodes, writes uint64) uint64 {
	return BaseGas + GasPerByte*inputBytes + GasPerSignature*signatures + GasPerProofNode*proofNodes + GasPerWrite*writes
}

func (p PrecompileExecutor) RequiredGas(input []byte, method *abi.Method) uint64 {
	nodes := uint64(0)
	if index, ok := proofArgument(method.Name); ok {
		if args, err := method.Inputs.Unpack(input); err == nil && len(args) > index {
			if witness, ok := args[index].([]byte); ok {
				nodes = uint64(len(witness)) / 32
			}
		}
	}
	return Gas(uint64(len(input)), Signatures(method.Name), nodes, Writes(method.Name))
}

func (p PrecompileExecutor) Execute(ctx sdk.Context, method *abi.Method, caller common.Address, _ common.Address, args []interface{}, value *big.Int, readOnly bool, evm *vm.EVM, hooks *tracing.Hooks) (ret []byte, err error) {
	defer func() {
		if recovered := recover(); recovered != nil {
			ret = nil
			err = fmt.Errorf("layerxexchange: %v", recovered)
		}
	}()
	if ctx.EVMPrecompileCalledFromDelegateCall() {
		return nil, errors.New("cannot delegatecall layerxExchange")
	}
	if method.Name != DepositMarginMethod {
		if err = pcommon.ValidateNonPayable(value); err != nil {
			return nil, err
		}
	}
	if readOnly && Writes(method.Name) != 0 {
		return nil, errors.New("cannot call a layerxExchange state change from staticcall")
	}
	if p.keeper == nil {
		return nil, types.ErrKeeperMissing
	}
	switch method.Name {
	case DepositMarginMethod:
		return p.depositMargin(ctx, method, caller, args, value, evm, hooks)
	case DepositMarginTokenMethod:
		return p.depositMarginToken(ctx, method, caller, args, evm)
	case WithdrawMarginMethod:
		return p.withdrawMargin(ctx, method, caller, args, evm)
	case PlaceOrderMethod:
		return p.placeOrder(ctx, method, caller, args, evm)
	case CancelOrderMethod:
		return p.cancelOrder(ctx, method, caller, args, evm)
	case RequestSettlementMethod:
		return p.requestSettlement(ctx, method, caller, args, evm)
	case GetIntentMethod:
		if err = pcommon.ValidateArgsLength(args, 1); err != nil {
			return nil, err
		}
		intent, _ := p.keeper.GetIntent(ctx, args[0].([32]byte))
		return method.Outputs.Pack(intentRecord(intent))
	case IntentNonceMethod:
		if err = pcommon.ValidateArgsLength(args, 1); err != nil {
			return nil, err
		}
		return method.Outputs.Pack(p.keeper.GetOwnerNonce(ctx, args[0].(common.Address)))
	case GetMarketMethod:
		if err = pcommon.ValidateArgsLength(args, 3); err != nil {
			return nil, err
		}
		proven, err := p.keeper.ProveMarket(ctx, args[0].([32]byte), args[1].(uint64), args[2].([]byte))
		if err != nil {
			return nil, err
		}
		return method.Outputs.Pack(stateRecord(proven))
	case GetOrderMethod, GetPositionMethod:
		if err = pcommon.ValidateArgsLength(args, 4); err != nil {
			return nil, err
		}
		prove := p.keeper.ProveOrder
		if method.Name == GetPositionMethod {
			prove = p.keeper.ProvePosition
		}
		proven, err := prove(ctx, args[0].([32]byte), args[1].([32]byte), args[2].(uint64), args[3].([]byte))
		if err != nil {
			return nil, err
		}
		return method.Outputs.Pack(stateRecord(proven))
	case GetMarginMethod:
		if err = pcommon.ValidateArgsLength(args, 4); err != nil {
			return nil, err
		}
		proven, err := p.keeper.ProveMargin(ctx, args[0].([32]byte), args[1].([32]byte), args[2].(uint64), args[3].([]byte))
		if err != nil {
			return nil, err
		}
		balance := proven.Account.Balance.Bytes()
		return method.Outputs.Pack(Margin{BatchNumber: proven.BatchNumber, StateRoot: proven.StateRoot,
			Account: proven.Account.AccountID, AssetId: proven.Account.AssetID,
			Balance: new(big.Int).SetBytes(balance[:]), Frozen: proven.Account.Frozen})
	}
	return nil, fmt.Errorf("layerxexchange: unknown method %s", method.Name)
}

func hash32(text string) [32]byte {
	value, _ := custodytypes.ParseHash32(text)
	return value
}

func address(text string) common.Address {
	value, _ := custodytypes.ParseAddress(text)
	return value
}

func amount(text string) *big.Int {
	value, ok := new(big.Int).SetString(text, 10)
	if !ok {
		return new(big.Int)
	}
	return value
}

func intentRecord(i types.Intent) Intent {
	return Intent{IntentId: hash32(i.IntentId), Kind: uint8(i.Kind), Status: uint8(i.Status), //nolint:gosec
		Owner: address(i.Owner), Nonce: i.Nonce, Height: uint64(i.Height), Account: hash32(i.Account), //nolint:gosec
		AssetId: hash32(i.AssetId), Denom: i.Denom, Amount: amount(i.Amount), DepositId: hash32(i.DepositId),
		MarketId: hash32(i.MarketId), Side: uint8(i.Side), Price: amount(i.Price), Quantity: amount(i.Quantity), //nolint:gosec
		TimeInForce: uint8(i.TimeInForce), OrderId: hash32(i.OrderId), PositionId: hash32(i.PositionId)} //nolint:gosec
}

func stateRecord(s exchangekeeper.ProvenState) StateRecord {
	return StateRecord{BatchNumber: s.BatchNumber, StateRoot: s.StateRoot, Key: s.Key, Value: s.Value}
}

// log emits one ABI event from the exchange address: indexed values become
// topics in declaration order and the rest is ABI-encoded data.
func (p PrecompileExecutor) log(evm *vm.EVM, name string, topics []common.Hash, data ...interface{}) error {
	event, ok := p.abi.Events[name]
	if !ok {
		return fmt.Errorf("layerxexchange: unknown event %s", name)
	}
	packed, err := event.Inputs.NonIndexed().Pack(data...)
	if err != nil {
		return err
	}
	return pcommon.EmitEVMLog(evm, p.address, append([]common.Hash{event.ID}, topics...), packed)
}

func addressTopic(value common.Address) common.Hash { return common.BytesToHash(value.Bytes()) }

func (p PrecompileExecutor) recordDeposit(ctx sdk.Context, method *abi.Method, caller common.Address,
	assetID, account [32]byte, value sdk.Int, evm *vm.EVM) ([]byte, error) {
	intent, err := p.keeper.DepositMargin(ctx, caller, p.evmKeeper.GetPaxAddressOrDefault(ctx, caller), assetID, account, value)
	if err != nil {
		return nil, err
	}
	intentID, depositID := hash32(intent.IntentId), hash32(intent.DepositId)
	if err := p.log(evm, MarginDepositedEvent, []common.Hash{intentID, account, addressTopic(caller)},
		assetID, value.BigInt(), depositID, intent.Nonce); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(intentID, depositID)
}

// depositMargin custodies the native coin as margin. msg.value must be a
// whole number of base units: LayerX amounts are bank base units.
func (p PrecompileExecutor) depositMargin(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{},
	value *big.Int, evm *vm.EVM, hooks *tracing.Hooks) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 1); err != nil {
		return nil, err
	}
	if value == nil || value.Sign() <= 0 {
		return nil, errors.New("layerxexchange: depositMargin requires a non-zero value")
	}
	asset, found := p.keeper.Custody().GetAssetByDenom(ctx, sdk.MustGetBaseDenom())
	if !found {
		return nil, custodytypes.ErrUnknownAsset
	}
	coin, err := pcommon.HandlePaymentUhpx(ctx, p.evmKeeper.GetPaxAddressOrDefault(ctx, p.address),
		p.evmKeeper.GetPaxAddressOrDefault(ctx, caller), value, p.bankKeeper, p.evmKeeper, hooks, evm.GetDepth())
	if err != nil {
		return nil, err
	}
	return p.recordDeposit(ctx, method, caller, hash32(asset.AssetId), args[0].([32]byte), coin.Amount, evm)
}

// depositMarginToken custodies a bank denom addressed by its ERC20 pointer as
// margin; amount is in the denom's base units.
func (p PrecompileExecutor) depositMarginToken(ctx sdk.Context, method *abi.Method, caller common.Address,
	args []interface{}, evm *vm.EVM) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 3); err != nil {
		return nil, err
	}
	asset, found := p.keeper.Custody().GetAssetByPointer(ctx, args[0].(common.Address))
	if !found {
		return nil, custodytypes.ErrUnknownAsset
	}
	return p.recordDeposit(ctx, method, caller, hash32(asset.AssetId), args[2].([32]byte),
		sdk.NewIntFromBigInt(args[1].(*big.Int)), evm)
}

func (p PrecompileExecutor) withdrawMargin(ctx sdk.Context, method *abi.Method, caller common.Address,
	args []interface{}, evm *vm.EVM) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 3); err != nil {
		return nil, err
	}
	account, assetID, value := args[0].([32]byte), args[1].([32]byte), args[2].(*big.Int)
	intent, err := p.keeper.WithdrawMargin(ctx, caller, account, assetID, sdk.NewIntFromBigInt(value))
	if err != nil {
		return nil, err
	}
	intentID := hash32(intent.IntentId)
	if err := p.log(evm, MarginWithdrawalRequestedEvent, []common.Hash{intentID, account, addressTopic(caller)},
		assetID, value, intent.Nonce); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(intentID)
}

func (p PrecompileExecutor) placeOrder(ctx sdk.Context, method *abi.Method, caller common.Address,
	args []interface{}, evm *vm.EVM) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 5); err != nil {
		return nil, err
	}
	market, side, price, quantity, tif := args[0].([32]byte), args[1].(uint8), args[2].(*big.Int), args[3].(*big.Int), args[4].(uint8)
	intent, err := p.keeper.PlaceOrder(ctx, caller, market, side, sdk.NewIntFromBigInt(price),
		sdk.NewIntFromBigInt(quantity), tif)
	if err != nil {
		return nil, err
	}
	intentID := hash32(intent.IntentId)
	if err := p.log(evm, OrderPlacedEvent, []common.Hash{intentID, market, addressTopic(caller)},
		side, price, quantity, tif, intent.Nonce); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(intentID)
}

func (p PrecompileExecutor) cancelOrder(ctx sdk.Context, method *abi.Method, caller common.Address,
	args []interface{}, evm *vm.EVM) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 1); err != nil {
		return nil, err
	}
	orderID := args[0].([32]byte)
	intent, err := p.keeper.CancelOrder(ctx, caller, orderID)
	if err != nil {
		return nil, err
	}
	intentID := hash32(intent.IntentId)
	if err := p.log(evm, OrderCancelRequestedEvent, []common.Hash{intentID, orderID, addressTopic(caller)},
		intent.Nonce); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(intentID)
}

func (p PrecompileExecutor) requestSettlement(ctx sdk.Context, method *abi.Method, caller common.Address,
	args []interface{}, evm *vm.EVM) ([]byte, error) {
	if err := pcommon.ValidateArgsLength(args, 1); err != nil {
		return nil, err
	}
	positionID := args[0].([32]byte)
	intent, err := p.keeper.RequestSettlement(ctx, caller, positionID)
	if err != nil {
		return nil, err
	}
	intentID := hash32(intent.IntentId)
	if err := p.log(evm, SettlementRequestedEvent, []common.Hash{intentID, positionID, addressTopic(caller)},
		intent.Nonce); err != nil {
		return nil, err
	}
	return method.Outputs.Pack(intentID)
}
