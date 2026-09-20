package v575

import (
	"bytes"
	"embed"
	"encoding/hex"
	"fmt"
	"strings"

	"math/big"

	"github.com/ethereum/go-ethereum/crypto"
	putils "github.com/sidiora-labs/paxeer-network/precompiles/utils"
	"github.com/sidiora-labs/paxeer-network/utils"
	helpers "github.com/sidiora-labs/paxeer-network/utils/helpers/legacy/v575"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/tracing"
	"github.com/ethereum/go-ethereum/core/vm"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"

	"github.com/sidiora-labs/paxeer-network/modules/evm/types"
	pcommon "github.com/sidiora-labs/paxeer-network/precompiles/common/legacy/v575"
	"github.com/sidiora-labs/paxeer-network/utils/metrics"
)

const (
	GetPaxAddressMethod = "getPaxAddr"
	GetEvmAddressMethod = "getEvmAddr"
	Associate           = "associate"
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

	GetPaxAddressID []byte
	GetEvmAddressID []byte
	AssociateID     []byte
}

func NewPrecompile(keepers putils.Keepers) (*pcommon.Precompile, error) {

	newAbi := pcommon.MustGetABI(f, "abi.json")

	p := &PrecompileExecutor{
		evmKeeper:     keepers.EVMK(),
		bankKeeper:    keepers.BankK(),
		accountKeeper: keepers.AccountK(),
	}

	for name, m := range newAbi.Methods {
		switch name {
		case GetPaxAddressMethod:
			p.GetPaxAddressID = m.ID
		case GetEvmAddressMethod:
			p.GetEvmAddressID = m.ID
		case Associate:
			p.AssociateID = m.ID
		}
	}

	return pcommon.NewPrecompile(newAbi, p, common.HexToAddress(AddrAddress), "addr"), nil
}

// RequiredGas returns the required bare minimum gas to execute the precompile.
func (p PrecompileExecutor) RequiredGas(input []byte, method *abi.Method) uint64 {
	if bytes.Equal(method.ID, p.AssociateID) {
		return 50000
	}
	return pcommon.DefaultGasCost(input, p.IsTransaction(method.Name))
}

func (p PrecompileExecutor) Execute(ctx sdk.Context, method *abi.Method, _ common.Address, _ common.Address, args []interface{}, value *big.Int, _ bool, _ *vm.EVM, hooks *tracing.Hooks) (bz []byte, err error) {
	switch method.Name {
	case GetPaxAddressMethod:
		return p.getPaxAddr(ctx, method, args, value)
	case GetEvmAddressMethod:
		return p.getEvmAddr(ctx, method, args, value)
	case Associate:
		return p.associate(ctx, method, args, value)
	}
	return
}

func (p PrecompileExecutor) getPaxAddr(ctx sdk.Context, method *abi.Method, args []interface{}, value *big.Int) ([]byte, error) {
	if err := pcommon.ValidateNonPayable(value); err != nil {
		return nil, err
	}

	if err := pcommon.ValidateArgsLength(args, 1); err != nil {
		return nil, err
	}

	paxAddr, found := p.evmKeeper.GetPaxAddress(ctx, args[0].(common.Address))
	if !found {
		metrics.IncrementAssociationError("getPaxAddr", types.NewAssociationMissingErr(args[0].(common.Address).Hex()))
		return nil, fmt.Errorf("EVM address %s is not associated", args[0].(common.Address).Hex())
	}
	return method.Outputs.Pack(paxAddr.String())
}

func (p PrecompileExecutor) getEvmAddr(ctx sdk.Context, method *abi.Method, args []interface{}, value *big.Int) ([]byte, error) {
	if err := pcommon.ValidateNonPayable(value); err != nil {
		return nil, err
	}

	if err := pcommon.ValidateArgsLength(args, 1); err != nil {
		return nil, err
	}

	paxAddr, err := sdk.AccAddressFromBech32(args[0].(string))
	if err != nil {
		return nil, err
	}

	evmAddr, found := p.evmKeeper.GetEVMAddress(ctx, paxAddr)
	if !found {
		metrics.IncrementAssociationError("getEvmAddr", types.NewAssociationMissingErr(args[0].(string)))
		return nil, fmt.Errorf("pax address %s is not associated", args[0].(string))
	}
	return method.Outputs.Pack(evmAddr)
}

func (p PrecompileExecutor) associate(ctx sdk.Context, method *abi.Method, args []interface{}, value *big.Int) ([]byte, error) {
	if err := pcommon.ValidateNonPayable(value); err != nil {
		return nil, err
	}

	if err := pcommon.ValidateArgsLength(args, 4); err != nil {
		return nil, err
	}

	// v, r and s are components of a signature over the customMessage sent.
	// We use the signature to construct the user's pubkey to obtain their addresses.
	v := args[0].(string)
	r := args[1].(string)
	s := args[2].(string)
	customMessage := args[3].(string)

	rBytes, err := decodeHexString(r)
	if err != nil {
		return nil, err
	}
	sBytes, err := decodeHexString(s)
	if err != nil {
		return nil, err
	}
	vBytes, err := decodeHexString(v)
	if err != nil {
		return nil, err
	}

	vBig := new(big.Int).SetBytes(vBytes)
	rBig := new(big.Int).SetBytes(rBytes)
	sBig := new(big.Int).SetBytes(sBytes)

	// Derive addresses
	vBig = new(big.Int).Add(vBig, utils.Big27)

	customMessageHash := crypto.Keccak256Hash([]byte(customMessage))
	evmAddr, paxAddr, pubkey, err := helpers.GetAddresses(vBig, rBig, sBig, customMessageHash)
	if err != nil {
		return nil, err
	}

	// Check that address is not already associated
	_, found := p.evmKeeper.GetEVMAddress(ctx, paxAddr)
	if found {
		return nil, fmt.Errorf("address %s is already associated with evm address %s", paxAddr, evmAddr)
	}

	// Associate Addresses:
	associationHelper := helpers.NewAssociationHelper(p.evmKeeper, p.bankKeeper, p.accountKeeper)
	err = associationHelper.AssociateAddresses(ctx, paxAddr, evmAddr, pubkey)
	if err != nil {
		return nil, err
	}

	return method.Outputs.Pack(paxAddr.String(), evmAddr)
}

func (PrecompileExecutor) IsTransaction(method string) bool {
	switch method {
	case Associate:
		return true
	default:
		return false
	}
}

func decodeHexString(hexString string) ([]byte, error) {
	trimmed := strings.TrimPrefix(hexString, "0x")
	if len(trimmed)%2 != 0 {
		trimmed = "0" + trimmed
	}
	return hex.DecodeString(trimmed)
}
