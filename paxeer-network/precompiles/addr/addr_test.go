package addr_test

import (
	"encoding/hex"
	"fmt"
	"math/big"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/core/vm"
	"github.com/ethereum/go-ethereum/crypto"
	tmtypes "github.com/sidiora-labs/paxeer-network/consensus/proto/tendermint/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/state"
	"github.com/sidiora-labs/paxeer-network/precompiles/addr"
	testkeeper "github.com/sidiora-labs/paxeer-network/testutil/keeper"
	"github.com/stretchr/testify/require"
)

func TestAssociatePubKey(t *testing.T) {
	testApp := testkeeper.EVMTestApp
	ctx := testApp.NewContext(false, tmtypes.Header{}).WithBlockHeight(2)
	k := &testApp.EvmKeeper

	pre, _ := addr.NewPrecompile(testApp.GetPrecompileKeepers())
	associatePubKey, err := pre.ABI.MethodById(pre.GetExecutor().(*addr.PrecompileExecutor).AssociatePubKeyID)

	// Target refers to the address that the caller is trying to associate.
	targetPrivKey := testkeeper.MockPrivateKey()
	targetPubKey := targetPrivKey.PubKey()
	targetPubKeyHex := hex.EncodeToString(targetPubKey.Bytes())
	targetPaxAddress, targetEvmAddress := testkeeper.PrivateKeyToAddresses(targetPrivKey)

	// Caller refers to the party calling the precompile.
	callerPrivKey := testkeeper.MockPrivateKey()
	callerPaxAddress, callerEvmAddress := testkeeper.PrivateKeyToAddresses(callerPrivKey)
	callerPubKey := callerPrivKey.PubKey()
	callerPubKeyHex := hex.EncodeToString(callerPubKey.Bytes())

	// Associate these addresses, so we can use them to test the case where addresses are already associated association.
	k.SetAddressMapping(ctx, callerPaxAddress, callerEvmAddress)

	require.Nil(t, err)

	happyPathOutput, _ := associatePubKey.Outputs.Pack(targetPaxAddress.String(), targetEvmAddress)

	type args struct {
		evm         *vm.EVM
		caller      common.Address
		pubKey      string
		value       *big.Int
		readOnly    bool
		suppliedGas uint64
	}
	tests := []struct {
		name       string
		args       args
		wantRet    []byte
		wantErr    bool
		wantErrMsg string
		wrongRet   bool
	}{
		{
			name: "fails if payable",
			args: args{
				evm: &vm.EVM{
					StateDB:   state.NewDBImpl(ctx, k, true),
					TxContext: vm.TxContext{Origin: callerEvmAddress},
				},
				caller: callerEvmAddress,
				pubKey: targetPubKeyHex,
				value:  big.NewInt(10),
			},
			wantErr:    true,
			wantErrMsg: "sending funds to a non-payable function",
		},
		{
			name: "fails on static call",
			args: args{
				evm: &vm.EVM{
					StateDB:   state.NewDBImpl(ctx, k, true),
					TxContext: vm.TxContext{Origin: callerEvmAddress},
				},
				caller:   callerEvmAddress,
				pubKey:   targetPubKeyHex,
				value:    big.NewInt(10),
				readOnly: true,
			},
			wantErr:    true,
			wantErrMsg: "cannot call associate pub key precompile from staticcall",
		},
		{
			name: "fails if input is appended with 0x",
			args: args{
				evm: &vm.EVM{
					StateDB:   state.NewDBImpl(ctx, k, true),
					TxContext: vm.TxContext{Origin: callerEvmAddress},
				},
				caller: callerEvmAddress,
				pubKey: fmt.Sprintf("0x%v", targetPubKeyHex),
				value:  big.NewInt(0),
			},
			wantErr:    true,
			wantErrMsg: "encoding/hex: invalid byte: U+0078 'x'",
		},
		{
			name: "fails if addresses are already associated",
			args: args{
				evm: &vm.EVM{
					StateDB:   state.NewDBImpl(ctx, k, true),
					TxContext: vm.TxContext{Origin: callerEvmAddress},
				},
				caller: callerEvmAddress,
				pubKey: callerPubKeyHex,
				value:  big.NewInt(0),
			},
			wantErr:    true,
			wantErrMsg: fmt.Sprintf("address %s is already associated with evm address %s", callerPaxAddress, callerEvmAddress),
		},
		{
			name: "fails if insufficient gas provided",
			args: args{
				evm: &vm.EVM{
					StateDB:   state.NewDBImpl(ctx, k, true),
					TxContext: vm.TxContext{Origin: callerEvmAddress},
				},
				caller:      callerEvmAddress,
				pubKey:      targetPubKeyHex,
				value:       big.NewInt(0),
				suppliedGas: 1,
			},
			wantErr:    true,
			wantErrMsg: "execution reverted: {ReadFlat}",
		},
		{
			name: "happy path - associates addresses if signature is correct",
			args: args{
				evm: &vm.EVM{
					StateDB:   state.NewDBImpl(ctx, k, true),
					TxContext: vm.TxContext{Origin: callerEvmAddress},
				},
				caller: callerEvmAddress,
				pubKey: targetPubKeyHex,
				value:  big.NewInt(0),
			},
			wantRet: happyPathOutput,
			wantErr: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Create the precompile and inputs
			p, _ := addr.NewPrecompile(testApp.GetPrecompileKeepers())
			require.Nil(t, err)
			inputs, err := associatePubKey.Inputs.Pack(tt.args.pubKey)
			require.Nil(t, err)

			// Make the call to associate.
			suppliedGas := uint64(40000)
			if tt.args.suppliedGas != 0 {
				suppliedGas = tt.args.suppliedGas
			}
			ret, _, err := p.RunAndCalculateGas(tt.args.evm, tt.args.caller, tt.args.caller, append(p.GetExecutor().(*addr.PrecompileExecutor).AssociatePubKeyID, inputs...), suppliedGas, tt.args.value, nil, tt.args.readOnly, false)
			if (err != nil) != tt.wantErr {
				t.Errorf("Run() error = %v, wantErr %v %v", err, tt.wantErr, string(ret))
				return
			}
			if err != nil {
				require.Equal(t, vm.ErrExecutionReverted, err)
				require.Nil(t, ret)
			} else if tt.wrongRet {
				// tt.wrongRet is set if we expect a return value that's different from the happy path. This means that the wrong addresses were associated.
				require.NotEqual(t, tt.wantRet, ret)
			} else {
				require.Equal(t, tt.wantRet, ret)
			}
		})
	}
}

func TestAssociate(t *testing.T) {
	testApp := testkeeper.EVMTestApp
	ctx := testApp.NewContext(false, tmtypes.Header{}).WithBlockHeight(2)
	k := &testApp.EvmKeeper

	pre, _ := addr.NewPrecompile(testApp.GetPrecompileKeepers())
	associate, err := pre.ABI.MethodById(pre.GetExecutor().(*addr.PrecompileExecutor).AssociateID)
	require.Nil(t, err)

	// Target refers to the address that the caller is trying to associate.
	targetPrivKey := testkeeper.MockPrivateKey()
	targetPrivHex := hex.EncodeToString(targetPrivKey.Bytes())
	targetPaxAddress, targetEvmAddress := testkeeper.PrivateKeyToAddresses(targetPrivKey)
	targetKey, _ := crypto.HexToECDSA(targetPrivHex)

	// Create the inputs
	emptyData := make([]byte, 32)
	prefixedMessage := fmt.Sprintf("\x19Ethereum Signed Message:\n%d", len(emptyData)) + string(emptyData)
	hash := crypto.Keccak256Hash([]byte(prefixedMessage))
	sig, err := crypto.Sign(hash.Bytes(), targetKey)
	require.Nil(t, err)

	r := fmt.Sprintf("0x%v", new(big.Int).SetBytes(sig[:32]).Text(16))
	s := fmt.Sprintf("0x%v", new(big.Int).SetBytes(sig[32:64]).Text(16))
	v := fmt.Sprintf("0x%v", new(big.Int).SetBytes([]byte{sig[64]}).Text(16))

	// Caller refers to the party calling the precompile.
	callerPrivKey := testkeeper.MockPrivateKey()
	callerPaxAddress, callerEvmAddress := testkeeper.PrivateKeyToAddresses(callerPrivKey)
	callerPrivHex := hex.EncodeToString(callerPrivKey.Bytes())
	callerKey, _ := crypto.HexToECDSA(callerPrivHex)

	// Associate these addresses, so we can use them to test the case where addresses are already associated association.
	k.SetAddressMapping(ctx, callerPaxAddress, callerEvmAddress)
	callerSig, err := crypto.Sign(hash.Bytes(), callerKey)
	callerR := fmt.Sprintf("0x%v", new(big.Int).SetBytes(callerSig[:32]).Text(16))
	callerS := fmt.Sprintf("0x%v", new(big.Int).SetBytes(callerSig[32:64]).Text(16))
	callerV := fmt.Sprintf("0x%v", new(big.Int).SetBytes([]byte{callerSig[64]}).Text(16))

	happyPathOutput, _ := associate.Outputs.Pack(targetPaxAddress.String(), targetEvmAddress)

	type args struct {
		evm      *vm.EVM
		caller   common.Address
		v        string
		r        string
		s        string
		msg      string
		value    *big.Int
		readOnly bool
	}
	tests := []struct {
		name       string
		args       args
		wantRet    []byte
		wantErr    bool
		wantErrMsg string
		wrongRet   bool
	}{
		{
			name: "fails if payable",
			args: args{
				evm: &vm.EVM{
					StateDB:   state.NewDBImpl(ctx, k, true),
					TxContext: vm.TxContext{Origin: callerEvmAddress},
				},
				caller: callerEvmAddress,
				v:      v,
				r:      r,
				s:      s,
				msg:    prefixedMessage,
				value:  big.NewInt(10),
			},
			wantErr:    true,
			wantErrMsg: "sending funds to a non-payable function",
		},
		{
			name: "fails on static calls",
			args: args{
				evm: &vm.EVM{
					StateDB:   state.NewDBImpl(ctx, k, true),
					TxContext: vm.TxContext{Origin: callerEvmAddress},
				},
				caller:   callerEvmAddress,
				v:        v,
				r:        r,
				s:        s,
				msg:      prefixedMessage,
				value:    big.NewInt(10),
				readOnly: true,
			},
			wantErr:    true,
			wantErrMsg: "cannot call associate precompile from staticcall",
		},
		{
			name: "fails if input is not hex",
			args: args{
				evm: &vm.EVM{
					StateDB:   state.NewDBImpl(ctx, k, true),
					TxContext: vm.TxContext{Origin: callerEvmAddress},
				},
				caller: callerEvmAddress,
				v:      "nothex",
				r:      r,
				s:      s,
				msg:    prefixedMessage,
				value:  big.NewInt(0),
			},
			wantErr:    true,
			wantErrMsg: "encoding/hex: invalid byte: U+006E 'n'",
		},
		{
			name: "fails if addresses are already associated",
			args: args{
				evm: &vm.EVM{
					StateDB:   state.NewDBImpl(ctx, k, true),
					TxContext: vm.TxContext{Origin: callerEvmAddress},
				},
				caller: callerEvmAddress,
				v:      callerV,
				r:      callerR,
				s:      callerS,
				msg:    prefixedMessage,
				value:  big.NewInt(0),
			},
			wantErr:    true,
			wantErrMsg: fmt.Sprintf("address %s is already associated with evm address %s", callerPaxAddress, callerEvmAddress),
		},
		{
			name: "associates wrong address if invalid signature (different message)",
			args: args{
				evm: &vm.EVM{
					StateDB:   state.NewDBImpl(ctx, k, true),
					TxContext: vm.TxContext{Origin: callerEvmAddress},
				},
				caller: callerEvmAddress,
				v:      v,
				r:      r,
				s:      s, // Pass in r instead of s here for invalid value
				msg:    "Not the signed message",
				value:  big.NewInt(0),
			},
			wantRet:  happyPathOutput,
			wrongRet: true,
		},
		{
			name: "happy path - associates addresses if signature is correct",
			args: args{
				evm: &vm.EVM{
					StateDB:   state.NewDBImpl(ctx, k, true),
					TxContext: vm.TxContext{Origin: callerEvmAddress},
				},
				caller: callerEvmAddress,
				v:      v,
				r:      r,
				s:      s,
				msg:    prefixedMessage,
				value:  big.NewInt(0),
			},
			wantRet: happyPathOutput,
			wantErr: false,
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			// Create the precompile and inputs
			p, _ := addr.NewPrecompile(testApp.GetPrecompileKeepers())
			require.Nil(t, err)
			inputs, err := associate.Inputs.Pack(tt.args.v, tt.args.r, tt.args.s, tt.args.msg)
			require.Nil(t, err)

			// Make the call to associate.
			ret, _, err := p.RunAndCalculateGas(tt.args.evm, tt.args.caller, tt.args.caller, append(p.GetExecutor().(*addr.PrecompileExecutor).AssociateID, inputs...), 40000, tt.args.value, nil, tt.args.readOnly, false)
			if (err != nil) != tt.wantErr {
				t.Errorf("Run() error = %v, wantErr %v %v", err, tt.wantErr, string(ret))
				return
			}
			if err != nil {
				require.Equal(t, vm.ErrExecutionReverted, err)
				require.Nil(t, ret)
			} else if tt.wrongRet {
				// tt.wrongRet is set if we expect a return value that's different from the happy path. This means that the wrong addresses were associated.
				require.NotEqual(t, tt.wantRet, ret)
			} else {
				require.Equal(t, tt.wantRet, ret)
			}
		})
	}
}

func TestGetAddr(t *testing.T) {
	testApp := testkeeper.EVMTestApp
	ctx := testApp.NewContext(false, tmtypes.Header{}).WithBlockHeight(2)
	k := &testApp.EvmKeeper

	pre, _ := addr.NewPrecompile(testApp.GetPrecompileKeepers())
	getPaxAddr, err := pre.ABI.MethodById(pre.GetExecutor().(*addr.PrecompileExecutor).GetPaxAddressID)
	require.Nil(t, err)
	getEvmAddr, err := pre.ABI.MethodById(pre.GetExecutor().(*addr.PrecompileExecutor).GetEvmAddressID)
	require.Nil(t, err)

	paxAddr, evmAddr := testkeeper.MockAddressPair()
	k.SetAddressMapping(ctx, paxAddr, evmAddr)

	stateDB := &state.DBImpl{}
	stateDB.WithCtx(ctx)

	getPaxAddrBz, err := getPaxAddr.Inputs.Pack(evmAddr)
	require.Nil(t, err)
	res, _, err := pre.RunAndCalculateGas(&vm.EVM{StateDB: stateDB}, evmAddr, evmAddr, append(getPaxAddr.ID, getPaxAddrBz...), 20000, common.Big0, nil, true, false)
	require.Nil(t, err)
	unpacked, err := getPaxAddr.Outputs.Unpack(res)
	require.Nil(t, err)
	require.Equal(t, 1, len(unpacked))
	require.Equal(t, paxAddr.String(), unpacked[0].(string))

	getEvmAddrBz, err := getEvmAddr.Inputs.Pack(paxAddr.String())
	require.Nil(t, err)
	res, _, err = pre.RunAndCalculateGas(&vm.EVM{StateDB: stateDB}, evmAddr, evmAddr, append(getEvmAddr.ID, getEvmAddrBz...), 20000, common.Big0, nil, true, false)
	require.Nil(t, err)
	unpacked, err = getEvmAddr.Outputs.Unpack(res)
	require.Nil(t, err)
	require.Equal(t, 1, len(unpacked))
	require.Equal(t, evmAddr, unpacked[0].(common.Address))
}
