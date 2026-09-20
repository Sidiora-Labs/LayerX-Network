package addr

import (
	"bytes"
	"embed"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"

	"github.com/btcsuite/btcd/btcec/v2"

	"math/big"

	"github.com/ethereum/go-ethereum/crypto"
	putils "github.com/sidiora-labs/paxeer-network/precompiles/utils"
	"github.com/sidiora-labs/paxeer-network/utils"
	"github.com/sidiora-labs/paxeer-network/utils/helpers"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/ethereum/go-ethereum/core/vm"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"

	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common"
	cryptotypes "github.com/sidiora-labs/paxeer-network/sdk/crypto/types"
	"github.com/sidiora-labs/paxeer-network/utils/metrics"
)

const (
	GetPaxAddressMethod = "getPaxAddr"
	GetEvmAddressMethod = "getEvmAddr"
	Associate           = "associate"
	AssociatePubKey     = "associatePubKey"

	BindLayerXMethod         = "bindLayerX"
	UnbindLayerXMethod       = "unbindLayerX"
	GetLayerXDidMethod       = "getLayerXDid"
	GetEvmAddrByLayerXMethod = "getEvmAddrByLayerX"
	LayerXBindNonceMethod    = "layerXBindNonce"
	GetUnifiedAccountMethod  = "getUnifiedAccount"
)

const (
	LayerXBoundEvent   = "LayerXBound"
	LayerXUnboundEvent = "LayerXUnbound"

	// LayerXBindSignatureGas is the EVM gas charged for the strict Ed25519
	// verification of a bind, on top of the metered store reads and writes. It
	// equals the per-signature charge of the layerxVerify precompile.
	LayerXBindSignatureGas uint64 = 4000
)

const (
	AddrAddress = "0x0000000000000000000000000000000000001004"
)

// Embed abi json file to the executable binary. Needed when importing as dependency.
//
//go:embed abi.json
var f embed.FS

type PrecompileExecutor struct {
	evmKeeper     putils.EVMKeeper
	bankKeeper    putils.BankKeeper
	accountKeeper putils.AccountKeeper

	GetPaxAddressID   []byte
	GetEvmAddressID   []byte
	AssociateID       []byte
	AssociatePubKeyID []byte

	BindLayerXID         []byte
	UnbindLayerXID       []byte
	GetLayerXDidID       []byte
	GetEvmAddrByLayerXID []byte
	LayerXBindNonceID    []byte
	GetUnifiedAccountID  []byte

	layerXBound   abi.Event
	layerXUnbound abi.Event
}

func NewPrecompile(keepers putils.Keepers) (*pcommon.DynamicGasPrecompile, error) {

	newAbi := pcommon.MustGetABI(f, "abi.json")

	p := &PrecompileExecutor{
		evmKeeper:     keepers.EVMK(),
		bankKeeper:    keepers.BankK(),
		accountKeeper: keepers.AccountK(),
		layerXBound:   newAbi.Events[LayerXBoundEvent],
		layerXUnbound: newAbi.Events[LayerXUnboundEvent],
	}

	for name, m := range newAbi.Methods {
		switch name {
		case GetPaxAddressMethod:
			p.GetPaxAddressID = m.ID
		case GetEvmAddressMethod:
			p.GetEvmAddressID = m.ID
		case Associate:
			p.AssociateID = m.ID
		case AssociatePubKey:
			p.AssociatePubKeyID = m.ID
		case BindLayerXMethod:
			p.BindLayerXID = m.ID
		case UnbindLayerXMethod:
			p.UnbindLayerXID = m.ID
		case GetLayerXDidMethod:
			p.GetLayerXDidID = m.ID
		case GetEvmAddrByLayerXMethod:
			p.GetEvmAddrByLayerXID = m.ID
		case LayerXBindNonceMethod:
			p.LayerXBindNonceID = m.ID
		case GetUnifiedAccountMethod:
			p.GetUnifiedAccountID = m.ID
		}
	}

	return pcommon.NewDynamicGasPrecompile(newAbi, p, common.HexToAddress(AddrAddress), "addr"), nil
}

// RequiredGas returns the required bare minimum gas to execute the precompile.
func (p PrecompileExecutor) RequiredGas(input []byte, method *abi.Method) uint64 {
	if bytes.Equal(method.ID, p.AssociateID) || bytes.Equal(method.ID, p.AssociatePubKeyID) {
		return 50000
	}
	return pcommon.DefaultGasCost(input, p.IsTransaction(method.Name))
}

func (p PrecompileExecutor) Execute(ctx sdk.Context, method *abi.Method, caller common.Address, _ common.Address, args []interface{}, value *big.Int, readOnly bool, evm *vm.EVM, suppliedGas uint64, hooks *tracing.Hooks) (ret []byte, remainingGas uint64, err error) {
	// Needed to catch gas meter panics
	defer func() {
		if r := recover(); r != nil {
			err = fmt.Errorf("execution reverted: %v", r)
		}
	}()
	switch method.Name {
	case GetPaxAddressMethod:
		return p.getPaxAddr(ctx, method, args, value)
	case GetEvmAddressMethod:
		return p.getEvmAddr(ctx, method, args, value)
	case Associate:
		if readOnly {
			return nil, 0, errors.New("cannot call associate precompile from staticcall")
		}
		return p.associate(ctx, method, args, value)
	case AssociatePubKey:
		if readOnly {
			return nil, 0, errors.New("cannot call associate pub key precompile from staticcall")
		}
		return p.associatePublicKey(ctx, method, args, value)
	case BindLayerXMethod:
		if readOnly {
			return nil, 0, errors.New("cannot call bindLayerX from staticcall")
		}
		if ctx.EVMPrecompileCalledFromDelegateCall() {
			return nil, 0, errors.New("cannot delegatecall bindLayerX")
		}
		return p.bindLayerX(ctx, method, caller, args, value, evm)
	case UnbindLayerXMethod:
		if readOnly {
			return nil, 0, errors.New("cannot call unbindLayerX from staticcall")
		}
		if ctx.EVMPrecompileCalledFromDelegateCall() {
			return nil, 0, errors.New("cannot delegatecall unbindLayerX")
		}
		return p.unbindLayerX(ctx, method, caller, args, value, evm)
	case GetLayerXDidMethod:
		return p.getLayerXDid(ctx, method, args, value)
	case GetEvmAddrByLayerXMethod:
		return p.getEvmAddrByLayerX(ctx, method, args, value)
	case LayerXBindNonceMethod:
		return p.layerXBindNonce(ctx, method, args, value)
	case GetUnifiedAccountMethod:
		return p.getUnifiedAccount(ctx, method, args, value)
	}
	return
}

func (p PrecompileExecutor) getPaxAddr(ctx sdk.Context, method *abi.Method, args []interface{}, value *big.Int) (ret []byte, remainingGas uint64, err error) {
	if err := pcommon.ValidateNonPayable(value); err != nil {
		return nil, 0, err
	}

	if err := pcommon.ValidateArgsLength(args, 1); err != nil {
		return nil, 0, err
	}

	paxAddr, found := p.evmKeeper.GetPaxAddress(ctx, args[0].(common.Address))
	if !found {
		metrics.IncrementAssociationError("getPaxAddr", types.NewAssociationMissingErr(args[0].(common.Address).Hex()))
		return nil, 0, fmt.Errorf("EVM address %s is not associated", args[0].(common.Address).Hex())
	}
	ret, err = method.Outputs.Pack(paxAddr.String())
	return ret, pcommon.GetRemainingGas(ctx, p.evmKeeper), err
}

func (p PrecompileExecutor) getEvmAddr(ctx sdk.Context, method *abi.Method, args []interface{}, value *big.Int) (ret []byte, remainingGas uint64, err error) {
	if err := pcommon.ValidateNonPayable(value); err != nil {
		return nil, 0, err
	}

	if err := pcommon.ValidateArgsLength(args, 1); err != nil {
		return nil, 0, err
	}

	paxAddr, err := sdk.AccAddressFromBech32(args[0].(string))
	if err != nil {
		return nil, 0, err
	}

	evmAddr, found := p.evmKeeper.GetEVMAddress(ctx, paxAddr)
	if !found {
		metrics.IncrementAssociationError("getEvmAddr", types.NewAssociationMissingErr(args[0].(string)))
		return nil, 0, fmt.Errorf("pax address %s is not associated", args[0].(string))
	}
	ret, err = method.Outputs.Pack(evmAddr)
	return ret, pcommon.GetRemainingGas(ctx, p.evmKeeper), err
}

func (p PrecompileExecutor) associate(ctx sdk.Context, method *abi.Method, args []interface{}, value *big.Int) (ret []byte, remainingGas uint64, err error) {
	if err := pcommon.ValidateNonPayable(value); err != nil {
		return nil, 0, err
	}

	if err := pcommon.ValidateArgsLength(args, 4); err != nil {
		return nil, 0, err
	}

	// v, r and s are components of a signature over the customMessage sent.
	// We use the signature to construct the user's pubkey to obtain their addresses.
	v := args[0].(string)
	r := args[1].(string)
	s := args[2].(string)
	customMessage := args[3].(string)

	rBytes, err := decodeHexString(r)
	if err != nil {
		return nil, 0, err
	}
	sBytes, err := decodeHexString(s)
	if err != nil {
		return nil, 0, err
	}
	vBytes, err := decodeHexString(v)
	if err != nil {
		return nil, 0, err
	}

	vBig := new(big.Int).SetBytes(vBytes)
	rBig := new(big.Int).SetBytes(rBytes)
	sBig := new(big.Int).SetBytes(sBytes)

	// Derive addresses
	vBig = new(big.Int).Add(vBig, utils.Big27)

	customMessageHash := crypto.Keccak256Hash([]byte(customMessage))
	evmAddr, paxAddr, pubkey, err := helpers.GetAddresses(vBig, rBig, sBig, customMessageHash)
	if err != nil {
		return nil, 0, err
	}

	return p.associateAddresses(ctx, method, evmAddr, paxAddr, pubkey)
}

func (p PrecompileExecutor) associatePublicKey(ctx sdk.Context, method *abi.Method, args []interface{}, value *big.Int) (ret []byte, remainingGas uint64, err error) {
	if err := pcommon.ValidateNonPayable(value); err != nil {
		return nil, 0, err
	}

	if err := pcommon.ValidateArgsLength(args, 1); err != nil {
		return nil, 0, err
	}

	// Takes a single argument, a compressed pubkey in hex format, excluding the '0x'
	pubKeyHex := args[0].(string)

	pubKeyBytes, err := hex.DecodeString(pubKeyHex)
	if err != nil {
		return nil, 0, err
	}

	// Parse the compressed public key
	pubKey, err := btcec.ParsePubKey(pubKeyBytes)
	if err != nil {
		return nil, 0, err
	}

	// Convert to uncompressed public key
	uncompressedPubKey := pubKey.SerializeUncompressed()

	evmAddr, paxAddr, pubkey, err := helpers.GetAddressesFromPubkeyBytes(uncompressedPubKey)
	if err != nil {
		return nil, 0, err
	}

	return p.associateAddresses(ctx, method, evmAddr, paxAddr, pubkey)
}

func (p PrecompileExecutor) associateAddresses(ctx sdk.Context, method *abi.Method, evmAddr common.Address, paxAddr sdk.AccAddress, pubkey cryptotypes.PubKey) (ret []byte, remainingGas uint64, err error) {
	// Check that address is not already associated
	_, found := p.evmKeeper.GetEVMAddress(ctx, paxAddr)
	if found {
		return nil, 0, fmt.Errorf("address %s is already associated with evm address %s", paxAddr, evmAddr)
	}

	// Associate Addresses:
	associationHelper := helpers.NewAssociationHelper(p.evmKeeper, p.bankKeeper, p.accountKeeper)
	err = associationHelper.AssociateAddresses(ctx, paxAddr, evmAddr, pubkey, false)
	if err != nil {
		return nil, 0, err
	}

	ret, err = method.Outputs.Pack(paxAddr.String(), evmAddr)
	return ret, pcommon.GetRemainingGas(ctx, p.evmKeeper), err
}

func (PrecompileExecutor) IsTransaction(method string) bool {
	switch method {
	case Associate, BindLayerXMethod, UnbindLayerXMethod:
		return true
	default:
		return false
	}
}

// bindLayerX binds msg.sender to the did:layerx identity of didPublicKey. The
// caller consents by sending the transaction; the DID key consents with a
// strict Ed25519 signature over types.LayerXBindMessage for the chain id, the
// caller and the caller's current layerXBindNonce.
func (p PrecompileExecutor) bindLayerX(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{}, value *big.Int, evm *vm.EVM) (ret []byte, remainingGas uint64, err error) {
	if err := pcommon.ValidateNonPayable(value); err != nil {
		return nil, 0, err
	}
	if err := pcommon.ValidateArgsLength(args, 2); err != nil {
		return nil, 0, err
	}
	didPublicKey := args[0].([32]byte)
	rawSignature := args[1].([]byte)
	var signature [64]byte
	if len(rawSignature) != len(signature) {
		return nil, 0, types.ErrLayerXSignatureLength
	}
	copy(signature[:], rawSignature)

	ctx.GasMeter().ConsumeGas(p.evmKeeper.GetCosmosGasLimitFromEVMGas(ctx, LayerXBindSignatureGas), "layerx bind signature")
	nonce, err := p.evmKeeper.BindLayerX(ctx, caller, didPublicKey, signature)
	if err != nil {
		return nil, 0, err
	}
	if err := p.emitLayerXEvent(evm, p.layerXBound, caller, didPublicKey, nonce); err != nil {
		return nil, 0, err
	}
	ret, err = method.Outputs.Pack()
	return ret, pcommon.GetRemainingGas(ctx, p.evmKeeper), err
}

// unbindLayerX removes msg.sender's binding on msg.sender's authority alone.
func (p PrecompileExecutor) unbindLayerX(ctx sdk.Context, method *abi.Method, caller common.Address, args []interface{}, value *big.Int, evm *vm.EVM) (ret []byte, remainingGas uint64, err error) {
	if err := pcommon.ValidateNonPayable(value); err != nil {
		return nil, 0, err
	}
	if err := pcommon.ValidateArgsLength(args, 0); err != nil {
		return nil, 0, err
	}
	didPublicKey, nonce, err := p.evmKeeper.UnbindLayerX(ctx, caller)
	if err != nil {
		return nil, 0, err
	}
	if err := p.emitLayerXEvent(evm, p.layerXUnbound, caller, didPublicKey, nonce); err != nil {
		return nil, 0, err
	}
	ret, err = method.Outputs.Pack()
	return ret, pcommon.GetRemainingGas(ctx, p.evmKeeper), err
}

// emitLayerXEvent logs LayerXBound or LayerXUnbound with the consumed nonce.
func (p PrecompileExecutor) emitLayerXEvent(evm *vm.EVM, event abi.Event, evmAddress common.Address, didPublicKey [32]byte, nonce uint64) error {
	data, err := event.Inputs.NonIndexed().Pack(nonce)
	if err != nil {
		return err
	}
	topics := []common.Hash{event.ID, common.BytesToHash(evmAddress.Bytes()), common.Hash(didPublicKey)}
	return pcommon.EmitEVMLog(evm, common.HexToAddress(AddrAddress), topics, data)
}

func (p PrecompileExecutor) getLayerXDid(ctx sdk.Context, method *abi.Method, args []interface{}, value *big.Int) (ret []byte, remainingGas uint64, err error) {
	if err := pcommon.ValidateNonPayable(value); err != nil {
		return nil, 0, err
	}
	if err := pcommon.ValidateArgsLength(args, 1); err != nil {
		return nil, 0, err
	}
	evmAddr := args[0].(common.Address)
	didPublicKey, found := p.evmKeeper.GetLayerXDid(ctx, evmAddr)
	if !found {
		return nil, 0, fmt.Errorf("EVM address %s is not bound to a LayerX DID", evmAddr.Hex())
	}
	ret, err = method.Outputs.Pack(didPublicKey, types.LayerXDid(didPublicKey))
	return ret, pcommon.GetRemainingGas(ctx, p.evmKeeper), err
}

func (p PrecompileExecutor) getEvmAddrByLayerX(ctx sdk.Context, method *abi.Method, args []interface{}, value *big.Int) (ret []byte, remainingGas uint64, err error) {
	if err := pcommon.ValidateNonPayable(value); err != nil {
		return nil, 0, err
	}
	if err := pcommon.ValidateArgsLength(args, 1); err != nil {
		return nil, 0, err
	}
	didPublicKey := args[0].([32]byte)
	evmAddr, found := p.evmKeeper.GetEVMAddressByLayerXDid(ctx, didPublicKey)
	if !found {
		return nil, 0, fmt.Errorf("LayerX DID %s is not bound to an EVM address", types.LayerXDid(didPublicKey))
	}
	ret, err = method.Outputs.Pack(evmAddr)
	return ret, pcommon.GetRemainingGas(ctx, p.evmKeeper), err
}

func (p PrecompileExecutor) layerXBindNonce(ctx sdk.Context, method *abi.Method, args []interface{}, value *big.Int) (ret []byte, remainingGas uint64, err error) {
	if err := pcommon.ValidateNonPayable(value); err != nil {
		return nil, 0, err
	}
	if err := pcommon.ValidateArgsLength(args, 1); err != nil {
		return nil, 0, err
	}
	ret, err = method.Outputs.Pack(p.evmKeeper.GetLayerXBindNonce(ctx, args[0].(common.Address)))
	return ret, pcommon.GetRemainingGas(ctx, p.evmKeeper), err
}

// getUnifiedAccount joins the three identities of one account and returns what
// exists: an empty paxAddr without an association, zero didPublicKey and
// layerxMainAccountId without a binding.
func (p PrecompileExecutor) getUnifiedAccount(ctx sdk.Context, method *abi.Method, args []interface{}, value *big.Int) (ret []byte, remainingGas uint64, err error) {
	if err := pcommon.ValidateNonPayable(value); err != nil {
		return nil, 0, err
	}
	if err := pcommon.ValidateArgsLength(args, 1); err != nil {
		return nil, 0, err
	}
	evmAddr := args[0].(common.Address)
	paxAddr := ""
	if associated, found := p.evmKeeper.GetPaxAddress(ctx, evmAddr); found {
		paxAddr = associated.String()
	}
	var mainAccountID [32]byte
	didPublicKey, bound := p.evmKeeper.GetLayerXDid(ctx, evmAddr)
	if bound {
		if mainAccountID, err = types.LayerXMainAccountID(didPublicKey); err != nil {
			return nil, 0, err
		}
	}
	ret, err = method.Outputs.Pack(evmAddr, paxAddr, didPublicKey, mainAccountID)
	return ret, pcommon.GetRemainingGas(ctx, p.evmKeeper), err
}

func (p PrecompileExecutor) EVMKeeper() putils.EVMKeeper {
	return p.evmKeeper
}

func decodeHexString(hexString string) ([]byte, error) {
	trimmed := strings.TrimPrefix(hexString, "0x")
	if len(trimmed)%2 != 0 {
		trimmed = "0" + trimmed
	}
	return hex.DecodeString(trimmed)
}
