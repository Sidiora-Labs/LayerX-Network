package types

import (
	"crypto/sha256"
	"encoding/binary"
	"math/big"

	"github.com/ethereum/go-ethereum/accounts/abi"
	"github.com/ethereum/go-ethereum/common"
	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
	"github.com/sidiora-labs/paxeer-network/layerxproof/verify"
)

const (
	// CustodyAddress is the layerxCustody precompile: the address every custody
	// EVM log is emitted from and the `address(this)` of every derived id.
	CustodyAddress = "0x0000000000000000000000000000000000001013"

	DepositDomain         = "LXP/Paxeer/custody-deposit/v1"
	WithdrawalClaimDomain = "LXP/Paxeer/withdrawal-claim/v1"
	ExitClaimDomain       = "LXP/Paxeer/emergency-exit/v1"
	ExitWithdrawalDomain  = "LXP/v1/emergency-withdrawal-id\x00"
)

func mustType(name string) abi.Type {
	t, err := abi.NewType(name, "", nil)
	if err != nil {
		panic(err)
	}
	return t
}

var (
	abiString  = mustType("string")
	abiUint256 = mustType("uint256")
	abiUint64  = mustType("uint64")
	abiAddress = mustType("address")
	abiBytes32 = mustType("bytes32")

	depositArguments = abi.Arguments{{Type: abiString}, {Type: abiUint256}, {Type: abiAddress}, {Type: abiAddress},
		{Type: abiBytes32}, {Type: abiBytes32}, {Type: abiUint256}, {Type: abiUint64}}
	claimArguments = abi.Arguments{{Type: abiString}, {Type: abiUint256}, {Type: abiAddress}, {Type: abiBytes32}, {Type: abiAddress}}
	exitArguments  = abi.Arguments{{Type: abiString}, {Type: abiUint256}, {Type: abiAddress}, {Type: abiBytes32}}
)

func packedDigest(arguments abi.Arguments, values ...interface{}) [32]byte {
	packed, err := arguments.Pack(values...)
	if err != nil {
		panic(err)
	}
	return sha256.Sum256(packed)
}

// DepositID is LayerXVault's depositId formula with the custody precompile as
// address(this): sha256(abi.encode(domain, chainid, this, payer, assetId,
// beneficiary, amount, nonce)).
func DepositID(chainID *big.Int, payer common.Address, assetID, beneficiary [32]byte, amount *big.Int, nonce uint64) [32]byte {
	return packedDigest(depositArguments, DepositDomain, chainID, common.HexToAddress(CustodyAddress), payer,
		assetID, beneficiary, amount, nonce)
}

// WithdrawalClaimID is WithdrawalClaims' claimId formula.
func WithdrawalClaimID(chainID *big.Int, nullifier [32]byte, recipient common.Address) [32]byte {
	return packedDigest(claimArguments, WithdrawalClaimDomain, chainID, common.HexToAddress(CustodyAddress), nullifier, recipient)
}

// ExitClaimID is EmergencyExit's claimId formula.
func ExitClaimID(chainID *big.Int, nullifier [32]byte) [32]byte {
	return packedDigest(exitArguments, ExitClaimDomain, chainID, common.HexToAddress(CustodyAddress), nullifier)
}

// ExitWithdrawalID is EmergencyExit.requiredWithdrawalId.
func ExitWithdrawalID(networkID uint32, account, assetID, anchor [32]byte) [32]byte {
	h := sha256.New()
	h.Write([]byte(ExitWithdrawalDomain))
	h.Write(binary.BigEndian.AppendUint32(nil, networkID))
	h.Write(account[:])
	h.Write(assetID[:])
	h.Write(anchor[:])
	var out [32]byte
	h.Sum(out[:0])
	return out
}

// WithdrawalNullifier is PaxeerWithdrawalCodec.nullifier.
func WithdrawalNullifier(networkID uint32, withdrawalID, account, assetID [32]byte, amount codec.U128, anchor [32]byte) [32]byte {
	value := amount.Bytes()
	h := sha256.New()
	h.Write([]byte(verify.WithdrawalNullifierDomain))
	h.Write(binary.BigEndian.AppendUint32(nil, networkID))
	h.Write(withdrawalID[:])
	h.Write(account[:])
	h.Write(assetID[:])
	h.Write(value[:])
	h.Write(anchor[:])
	var out [32]byte
	h.Sum(out[:0])
	return out
}

// AmountInt converts a LayerX u128 to a big integer.
func AmountInt(amount codec.U128) *big.Int {
	value := amount.Bytes()
	return new(big.Int).SetBytes(value[:])
}
