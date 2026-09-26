package msgs

import (
	"crypto/ecdsa"
	"math/big"

	"github.com/ethereum/go-ethereum/common"
	ethtypes "github.com/ethereum/go-ethereum/core/types"
	"github.com/ethereum/go-ethereum/crypto"
	evmtypes "github.com/sidiora-labs/paxeer-network/modules/evm/types"
	"github.com/sidiora-labs/paxeer-network/modules/evm/types/ethtx"
	"github.com/sidiora-labs/paxeer-network/precompiles"
	"github.com/sidiora-labs/paxeer-network/precompiles/feetoken"
	"github.com/sidiora-labs/paxeer-network/precompiles/layerxbridge"
	"github.com/sidiora-labs/paxeer-network/sdk/crypto/keys/secp256k1"
	sdk "github.com/sidiora-labs/paxeer-network/sdk/types"
	"github.com/sidiora-labs/paxeer-network/sdk/x/auth/signing"
	"github.com/sidiora-labs/paxeer-network/testutil/processblock"
)

const (
	// BridgeInGas covers the bridge precompile's own gas plus the calldata an
	// attested deposit carries.
	BridgeInGas uint64 = 500000
	// FeeDenomGas covers the fee-token precompile's write of one preference.
	FeeDenomGas uint64 = 200000
	// TransferGas is the intrinsic gas of a value-free transfer between
	// accounts, so the whole limit is spent and no gas is returned.
	TransferGas uint64 = 21000
)

// FeeTokenSigner is one secp256k1 key with the Paxeer address and the EVM
// address it signs as, derived the way the chain derives both from one key.
type FeeTokenSigner struct {
	Key     *ecdsa.PrivateKey
	PaxAddr sdk.AccAddress
	EVMAddr common.Address
}

// FeeTokenSignerFromSeed derives a signer from a deterministic key so a proof
// reruns over the same addresses.
func FeeTokenSignerFromSeed(seed byte) FeeTokenSigner {
	material := make([]byte, 32)
	material[0] = 0xf1
	material[31] = seed
	key, err := crypto.ToECDSA(material)
	if err != nil {
		panic(err)
	}
	paxKey := &secp256k1.PrivKey{Key: material}
	return FeeTokenSigner{
		Key:     key,
		PaxAddr: sdk.AccAddress(paxKey.PubKey().Address()),
		EVMAddr: crypto.PubkeyToAddress(key.PublicKey),
	}
}

// RecipientWord is the left-padded bridge recipient of an EVM address.
func RecipientWord(recipient common.Address) [32]byte {
	return common.BytesToHash(recipient.Bytes())
}

// SidioraBridgeIn is relayer's attested deposit of amount of the remote asset
// of chain to recipient, carrying the attestor signatures of its digest.
func SidioraBridgeIn(app *processblock.App, relayer FeeTokenSigner, nonce uint64, gasPrice *big.Int, chain uint64,
	vault common.Address, txHash common.Hash, logIndex uint64, recipient common.Address, asset common.Address,
	amount *big.Int, signatures [][]byte) signing.Tx {
	info := precompiles.GetPrecompileInfo(layerxbridge.PrecompileName)
	data, err := info.ABI.Pack(layerxbridge.BridgeInMethod, chain, vault, [32]byte(txHash), logIndex,
		RecipientWord(recipient), asset, amount, signatures)
	if err != nil {
		panic(err)
	}
	return EVMTransaction(app, relayer, nonce, gasPrice, BridgeInGas, info.Address, data)
}

// FeeDenomPreference is payer's own choice of denom to pay gas in, made
// through the fee-token precompile.
func FeeDenomPreference(app *processblock.App, payer FeeTokenSigner, nonce uint64, gasPrice *big.Int,
	denom string) signing.Tx {
	info := precompiles.GetPrecompileInfo(feetoken.PrecompileName)
	data, err := info.ABI.Pack(feetoken.SetFeeDenomMethod, denom)
	if err != nil {
		panic(err)
	}
	return EVMTransaction(app, payer, nonce, gasPrice, FeeDenomGas, info.Address, data)
}

// FeeTokenTransfer is a value-free transfer from sender to recipient whose gas
// the denom sender prefers pays for.
func FeeTokenTransfer(app *processblock.App, sender FeeTokenSigner, nonce uint64, gasPrice *big.Int,
	recipient common.Address) signing.Tx {
	return EVMTransaction(app, sender, nonce, gasPrice, TransferGas, recipient, nil)
}

// EVMTransaction signs an Ethereum transaction with the signer's key and wraps
// it in the cosmos transaction a block carries.
func EVMTransaction(app *processblock.App, signer FeeTokenSigner, nonce uint64, gasPrice *big.Int, gas uint64,
	to common.Address, data []byte) signing.Tx {
	ctx := app.Ctx()
	ethCfg := evmtypes.DefaultChainConfig().EthereumConfig(app.EvmKeeper.ChainID(ctx))
	ethSigner := ethtypes.MakeSigner(ethCfg, big.NewInt(ctx.BlockHeight()), uint64(ctx.BlockTime().Unix())) // nolint:gosec
	recipient := to
	signed, err := ethtypes.SignTx(ethtypes.NewTx(&ethtypes.LegacyTx{
		Nonce:    nonce,
		GasPrice: gasPrice,
		Gas:      gas,
		To:       &recipient,
		Data:     data,
	}), ethSigner, signer.Key)
	if err != nil {
		panic(err)
	}
	typedTx, err := ethtx.NewLegacyTx(signed)
	if err != nil {
		panic(err)
	}
	msg, err := evmtypes.NewMsgEVMTransaction(typedTx)
	if err != nil {
		panic(err)
	}
	txBuilder := processblock.TxConfig.NewTxBuilder()
	if err := txBuilder.SetMsgs(msg); err != nil {
		panic(err)
	}
	return txBuilder.GetTx()
}
