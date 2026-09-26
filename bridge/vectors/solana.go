// Package vectors is the one place the Paxeer X Network bridge writes down how
// a Solana identity enters the fixed attestation digests, and it pins that
// mapping with digest vectors. bridge/ATTESTATION-SOLANA.md states the same
// mapping in prose and carries the same values in its table.
//
// The preimages, the digest and the signature rules are fixed elsewhere:
// modules/layerxbridge/ATTESTATION.md is their byte-exact specification and
// modules/layerxbridge/types builds them. This package adds no second domain,
// hash or signature format; it only says which 20 and 32 bytes a Solana key,
// mint, transaction and nonce contribute. solana_test.go rebuilds every digest
// below through modules/layerxbridge/types.InboundPreimage and
// OutboundPreimage, so a value here cannot claim a digest the chain would not
// produce.
//
// Nothing in this package depends on a Solana toolchain: base58, the program
// address derivation and the handle are all computed here, so any Go tool in
// the repository can read the mapping from one place.
package vectors

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"math/big"
	"strings"

	"filippo.io/edwards25519"
	"github.com/ethereum/go-ethereum/crypto"
)

// Key32 is a 32-byte Solana public key: an account, a mint, a program id or a
// program-derived address.
type Key32 [32]byte

// Address20 is the 20 bytes an identity contributes to a digest, the same width
// as an EVM address.
type Address20 [20]byte

// Digest is a 32-byte keccak256 output or any other 32-byte word of a preimage.
type Digest [32]byte

// Signature64 is a Solana transaction signature as the RPC reports it.
type Signature64 [64]byte

const (
	// SolanaChainIDLabel is the ASCII name whose bytes, left-padded to eight
	// and read big-endian, are SolanaChainID.
	SolanaChainIDLabel = "SOLANA"

	// SolanaChainID is the chain id the Paxeer side registers for Solana:
	// 0x0000534f4c414e41, the big-endian value of SolanaChainIDLabel padded to
	// eight bytes. It sits far above every EIP-155 chain id in use, so it can
	// never collide with one, and governance registers it like any other chain.
	SolanaChainID uint64 = 91600046870081

	// VaultAuthoritySeed is the single seed of the custody program's
	// vault-authority PDA. The handle of that PDA is what governance registers
	// as Solana's vault on Paxeer.
	VaultAuthoritySeed = "vault-authority"

	// SidioraMintBase58 is Sidiora's SPL mint. Solana is Sidiora's foreign
	// home; it exists on no other chain but Paxeer X.
	SidioraMintBase58 = "5w3wVdJaESaJKyLmStM6Hv9UyUkmZ1b9DLQquAqqpump"

	// SidioraDecimals is the decimal count of both the SPL mint and the usid
	// denom the bridge module administers.
	SidioraDecimals uint8 = 6

	// WrappedSolMintBase58 is the wrapped SOL mint, the first and always
	// registered asset of the Solana configuration: native SOL bridges as this
	// mint so one code path serves every asset.
	WrappedSolMintBase58 = "So11111111111111111111111111111111111111112"

	// WrappedSolDecimals is the decimal count of wrapped SOL.
	WrappedSolDecimals uint8 = 9

	// SidioraAssetIDHex is the 20 bytes Sidiora's mint is registered with. It
	// is not the mint's derived handle: the chain already fixed this pair, so
	// the registry records it explicitly. See the note on SidioraAssetID.
	SidioraAssetIDHex = "21f7b20a555199fa73A238B1a91FD0f549068fEe"
)

// The labels below derive every input of the pinned vectors from a documented
// ASCII string, so the Go vectors, the custody program's tests and the
// relayer's tests can each recompute the same bytes instead of copying an
// unexplained value. They stand in for values a deployment produces: a program
// id, a transaction signature, an account.
const (
	// VectorProgramIDLabel derives VectorProgramID, the program id every vector
	// here is computed against. A deployment's real program id is recorded by
	// bridge/deploy/deploy-solana-program.sh, and the vault handle governance
	// registers is derived from that one.
	VectorProgramIDLabel = "PAXEERX_BRIDGE_SOLANA_VECTOR_PROGRAM"

	// VectorDepositSignatureLabel derives VectorDepositSignature, the 64-byte
	// transaction signature the inbound vector's txHash hashes.
	VectorDepositSignatureLabel = "PAXEERX_BRIDGE_SOLANA_VECTOR_DEPOSIT_SIGNATURE"

	// VectorPaxeerRecipientLabel derives the Paxeer EVM address the inbound
	// vector credits.
	VectorPaxeerRecipientLabel = "PAXEERX_BRIDGE_SOLANA_VECTOR_PAXEER_RECIPIENT"

	// VectorSolanaRecipientLabel derives the 32-byte Solana pubkey the outbound
	// vector releases to.
	VectorSolanaRecipientLabel = "PAXEERX_BRIDGE_SOLANA_VECTOR_RECIPIENT"

	// VectorPaxeerBurnLabel derives the Paxeer transaction hash of the burn the
	// outbound vector releases against.
	VectorPaxeerBurnLabel = "PAXEERX_BRIDGE_SOLANA_VECTOR_BURN"
)

// base58Alphabet is the Bitcoin alphabet Solana encodes keys and signatures in.
const base58Alphabet = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"

// programDerivedAddressMarker is the suffix the Solana runtime appends before
// hashing a program-derived address, so a PDA can never be a key on the curve.
const programDerivedAddressMarker = "ProgramDerivedAddress"

// maxSeeds and maxSeedLength are the runtime's bounds on program address seeds.
const (
	maxSeeds      = 16
	maxSeedLength = 32
)

var (
	// SidioraMint is Sidiora's SPL mint as its 32 raw bytes.
	SidioraMint = mustKey(SidioraMintBase58)

	// WrappedSolMint is the wrapped SOL mint as its 32 raw bytes.
	WrappedSolMint = mustKey(WrappedSolMintBase58)

	// SidioraAssetID is the asset id Sidiora's mint is registered with:
	// 0x21f7b20a555199fa73A238B1a91FD0f549068fEe, the bank-backed pointer on
	// Paxeer X, and not Handle(SidioraMint). The chain fixed the pair first -
	// EnsureSidioraDenom in modules/layerxbridge/keeper/sidiora.go registers the
	// bridged asset (chain id, this address, the usid denom) - so registering
	// the mint with this id is what makes an inbound SID deposit resolve to the
	// denom the bridge module already administers.
	SidioraAssetID = mustAddress(SidioraAssetIDHex)

	// WrappedSolAssetID is the asset id of the wrapped SOL mint: its derived
	// handle, the registry's default.
	WrappedSolAssetID = mustAddress("cf996523b5d068a26f0aa8a116602fe5033ee3a1")

	// VectorProgramID is keccak256(VectorProgramIDLabel) read as a Solana
	// address: the program id the vectors are computed against.
	VectorProgramID = mustKeyHex("875f91f2b7ddd837d8584366fa8d7a8c73905b2212713a4826d4a89faa8d40fc")

	// VectorVaultAuthority is the vault-authority PDA of VectorProgramID.
	VectorVaultAuthority = mustKeyHex("ed349d8496a5f534b3bb8f5d240d8633acf502427eca37504ec7cf68766aa71d")

	// VectorVaultHandle is Handle(VectorVaultAuthority): the 20 bytes that enter
	// every digest below as the vault.
	VectorVaultHandle = mustAddress("334121a65b47bd45c3f6381537d9180e98e445bc")

	// VectorDepositSignature is the transaction signature the inbound vector
	// observed: keccak256(label || 0x01) followed by keccak256(label || 0x02).
	VectorDepositSignature = mustSignature(
		"be5dcb8ac1c1153020e5002e376da27e2c817c11725d6b37ea46deadb92b55ef" +
			"edb2c44ad7ec9132c7685ed84b08fcefb65b76a9912b1f0cc8f82eeb379b96e2",
	)
)

// VectorVaultAuthorityBump is the bump FindProgramAddress returns for
// VectorVaultAuthority.
const VectorVaultAuthorityBump uint8 = 255

// Inbound is one attested Solana deposit: what the relayer submits to the
// layerxBridge precompile as a bridgeIn after observing the custody program.
type Inbound struct {
	// ChainID is SolanaChainID.
	ChainID uint64
	// Vault is the handle of the custody program's vault-authority PDA.
	Vault Address20
	// Mint is the SPL mint deposited.
	Mint Key32
	// Asset is the 20-byte asset id the program's registry holds for Mint.
	Asset Address20
	// Decimals is the mint's decimal count.
	Decimals uint8
	// Signature is the Solana transaction signature carrying the deposit.
	Signature Signature64
	// TxHash is keccak256(Signature).
	TxHash Digest
	// LogIndex is the deposit nonce the receipt PDA records.
	LogIndex uint64
	// Recipient is the 32-byte Paxeer recipient, an EVM address in its low 20
	// bytes with the high 12 zero.
	Recipient Digest
	// Amount is the deposited amount in the mint's base units, decimal.
	Amount string
	// PreimageLength is the length of the inbound preimage: 196 bytes.
	PreimageLength int
	// Digest is keccak256 of that preimage: what the attestors sign.
	Digest Digest
}

// Outbound is one attested release on Solana: what the relayer submits to the
// custody program after a Paxeer burn.
type Outbound struct {
	// ChainID is SolanaChainID.
	ChainID uint64
	// Vault is the handle of the custody program's vault-authority PDA.
	Vault Address20
	// Mint is the SPL mint released.
	Mint Key32
	// Asset is the 20-byte asset id the program's registry holds for Mint.
	Asset Address20
	// Decimals is the mint's decimal count.
	Decimals uint8
	// PaxeerTxHash is the Paxeer transaction hash of the burn.
	PaxeerTxHash Digest
	// PaxeerNonce is the Paxeer bridge nonce of the burn.
	PaxeerNonce uint64
	// RecipientKey is the full 32-byte Solana pubkey the release instruction
	// names. The program derives Recipient from it, so a release can pay only
	// the key the attestors signed for.
	RecipientKey Key32
	// Recipient is Handle(RecipientKey): the 20 bytes in the signed digest.
	Recipient Address20
	// Amount is the released amount in the mint's base units, decimal.
	Amount string
	// PreimageLength is the length of the outbound preimage: 185 bytes.
	PreimageLength int
	// Digest is keccak256 of that preimage: what the attestors sign and what
	// the custody program has the native secp256k1 program verify.
	Digest Digest
}

// InboundVector is the pinned inbound attestation: a Sidiora deposit on Solana
// credited to a Paxeer account.
var InboundVector = Inbound{
	ChainID:        SolanaChainID,
	Vault:          VectorVaultHandle,
	Mint:           SidioraMint,
	Asset:          SidioraAssetID,
	Decimals:       SidioraDecimals,
	Signature:      VectorDepositSignature,
	TxHash:         mustDigest("4219bc1e7d357618e0c662c494981e70d02920d8f7c3f850dd25712ef87e48d3"),
	LogIndex:       7,
	Recipient:      mustDigest("000000000000000000000000b65aa00b0baa8fe2abc2d188312fb0a6d00e3ed5"),
	Amount:         "12345678",
	PreimageLength: 196,
	Digest:         mustDigest("3333b122e2a4e61ad324d5c875a8a9c72cf98a2724da26c211235e30a8d294ef"),
}

// OutboundVector is the pinned outbound attestation: a Paxeer burn of the usid
// denom released as Sidiora to a Solana account.
var OutboundVector = Outbound{
	ChainID:        SolanaChainID,
	Vault:          VectorVaultHandle,
	Mint:           SidioraMint,
	Asset:          SidioraAssetID,
	Decimals:       SidioraDecimals,
	PaxeerTxHash:   mustDigest("6f79d9a61a77030bbeba5f435907f53b09321779310326b9faaebb391b3b5d5f"),
	PaxeerNonce:    11,
	RecipientKey:   mustKeyHex("3d992e1dc9d1cc1c5311d79c69a3801d40099046094d125fc687c6c57c395e88"),
	Recipient:      mustAddress("fb02125a3275d53a9f6538626b49894d2aae80cc"),
	Amount:         "4200000",
	PreimageLength: 185,
	Digest:         mustDigest("c583652dd9b59e0fcef102cfc8866a52beadb77b82de25445d86900baf6d1c4e"),
}

// Handle is the 20 bytes a 32-byte Solana key contributes to a digest: the last
// 20 bytes of keccak256 of the key. It is the only place the mapping lives, and
// it is how the vault, an asset's default id and a release recipient become 20
// bytes wide.
func Handle(key Key32) Address20 {
	var handle Address20
	copy(handle[:], crypto.Keccak256(key[:])[12:])
	return handle
}

// InboundTxHash is the txHash of an inbound attestation: keccak256 of the
// 64-byte Solana transaction signature that carried the deposit. Solana has no
// 32-byte transaction hash of its own, and the custody program never learns its
// own signature, so the relayer hashes the signature it observed.
func InboundTxHash(signature Signature64) Digest {
	var digest Digest
	copy(digest[:], crypto.Keccak256(signature[:]))
	return digest
}

// PaxeerRecipient widens a Paxeer EVM address into the 32-byte recipient word
// of an inbound preimage: the address in the low 20 bytes, the high 12 zero.
func PaxeerRecipient(address Address20) Digest {
	var word Digest
	copy(word[12:], address[:])
	return word
}

// ChainIDFromLabel reads an ASCII label as a chain id the way SolanaChainID is
// read: the bytes left-padded to eight and taken big-endian. It refuses a label
// that does not fit in eight bytes and an empty label.
func ChainIDFromLabel(label string) (uint64, error) {
	if label == "" {
		return 0, errors.New("bridge/vectors: chain id label is empty")
	}
	if len(label) > 8 {
		return 0, fmt.Errorf("bridge/vectors: chain id label %q is %d bytes, at most 8 fit", label, len(label))
	}
	padded := make([]byte, 8)
	copy(padded[8-len(label):], label)
	return new(big.Int).SetBytes(padded).Uint64(), nil
}

// AmountBig parses the vector's amount into the uint256 the preimage carries.
func (v Inbound) AmountBig() (*big.Int, error) { return parseAmount(v.Amount) }

// AmountBig parses the vector's amount into the uint256 the preimage carries.
func (v Outbound) AmountBig() (*big.Int, error) { return parseAmount(v.Amount) }

func parseAmount(amount string) (*big.Int, error) {
	value, ok := new(big.Int).SetString(amount, 10)
	if !ok {
		return nil, fmt.Errorf("bridge/vectors: amount %q is not a decimal integer", amount)
	}
	if value.Sign() <= 0 {
		return nil, fmt.Errorf("bridge/vectors: amount %q is not positive", amount)
	}
	return value, nil
}

// Key decodes a base58 Solana address into its 32 raw bytes.
func Key(address string) (Key32, error) {
	raw, err := DecodeBase58(address)
	if err != nil {
		return Key32{}, err
	}
	if len(raw) != len(Key32{}) {
		return Key32{}, fmt.Errorf("bridge/vectors: address %q decodes to %d bytes, want 32", address, len(raw))
	}
	var key Key32
	copy(key[:], raw)
	return key, nil
}

// Base58 renders a Solana key the way the RPC and the explorers render it.
func (k Key32) Base58() string { return EncodeBase58(k[:]) }

// Base58 renders a transaction signature the way the RPC renders it.
func (s Signature64) Base58() string { return EncodeBase58(s[:]) }

// Hex renders the 20 bytes with the 0x prefix a Paxeer-side address carries.
func (a Address20) Hex() string { return "0x" + hex.EncodeToString(a[:]) }

// Hex renders the 32 bytes with the 0x prefix a digest carries.
func (d Digest) Hex() string { return "0x" + hex.EncodeToString(d[:]) }

// DecodeBase58 decodes the base58 Solana writes keys and signatures in. It
// refuses any character outside the alphabet rather than skipping it.
func DecodeBase58(encoded string) ([]byte, error) {
	if encoded == "" {
		return nil, errors.New("bridge/vectors: base58 string is empty")
	}
	value := new(big.Int)
	radix := big.NewInt(58)
	digit := new(big.Int)
	for index, symbol := range encoded {
		position := strings.IndexRune(base58Alphabet, symbol)
		if position < 0 {
			return nil, fmt.Errorf("bridge/vectors: base58 string has invalid character %q at index %d", symbol, index)
		}
		value.Mul(value, radix)
		value.Add(value, digit.SetInt64(int64(position)))
	}
	leadingZeros := 0
	for leadingZeros < len(encoded) && encoded[leadingZeros] == base58Alphabet[0] {
		leadingZeros++
	}
	body := value.Bytes()
	decoded := make([]byte, leadingZeros+len(body))
	copy(decoded[leadingZeros:], body)
	return decoded, nil
}

// EncodeBase58 is the inverse of DecodeBase58.
func EncodeBase58(raw []byte) string {
	value := new(big.Int).SetBytes(raw)
	radix := big.NewInt(58)
	remainder := new(big.Int)
	encoded := make([]byte, 0, len(raw)*137/100+1)
	for value.Sign() > 0 {
		value.DivMod(value, radix, remainder)
		encoded = append(encoded, base58Alphabet[remainder.Int64()])
	}
	for _, symbol := range raw {
		if symbol != 0 {
			break
		}
		encoded = append(encoded, base58Alphabet[0])
	}
	for left, right := 0, len(encoded)-1; left < right; left, right = left+1, right-1 {
		encoded[left], encoded[right] = encoded[right], encoded[left]
	}
	return string(encoded)
}

// CreateProgramAddress is the Solana runtime's program address derivation:
// sha256 of the seeds, the bump, the program id and the marker, refused when
// the result is a point on the ed25519 curve, because such an address could
// have a private key.
func CreateProgramAddress(programID Key32, seeds [][]byte, bump uint8) (Key32, error) {
	if len(seeds)+1 > maxSeeds {
		return Key32{}, fmt.Errorf("bridge/vectors: %d seeds and a bump exceed the limit of %d", len(seeds), maxSeeds)
	}
	hash := sha256.New()
	for index, seed := range seeds {
		if len(seed) > maxSeedLength {
			return Key32{}, fmt.Errorf("bridge/vectors: seed %d is %d bytes, at most %d are allowed", index, len(seed), maxSeedLength)
		}
		hash.Write(seed)
	}
	hash.Write([]byte{bump})
	hash.Write(programID[:])
	hash.Write([]byte(programDerivedAddressMarker))
	var candidate Key32
	copy(candidate[:], hash.Sum(nil))
	if onCurve(candidate) {
		return Key32{}, fmt.Errorf("bridge/vectors: bump %d yields an address on the ed25519 curve", bump)
	}
	return candidate, nil
}

// FindProgramAddress returns the program address of the seeds and the highest
// bump that puts it off the curve, exactly as the runtime searches.
func FindProgramAddress(programID Key32, seeds [][]byte) (Key32, uint8, error) {
	for bump := maxBump; ; bump-- {
		address, err := CreateProgramAddress(programID, seeds, uint8(bump))
		if err == nil {
			return address, uint8(bump), nil
		}
		if bump == 0 {
			return Key32{}, 0, errors.New("bridge/vectors: no bump yields an address off the ed25519 curve")
		}
	}
}

const maxBump = 255

// VaultAuthority is the custody program's vault-authority PDA and its bump: the
// account that holds every bridged token on Solana and signs their transfers.
func VaultAuthority(programID Key32) (Key32, uint8, error) {
	return FindProgramAddress(programID, [][]byte{[]byte(VaultAuthoritySeed)})
}

// VaultHandle is the 20 bytes governance registers as Solana's vault on Paxeer:
// the handle of the program's vault-authority PDA.
func VaultHandle(programID Key32) (Address20, error) {
	authority, _, err := VaultAuthority(programID)
	if err != nil {
		return Address20{}, err
	}
	return Handle(authority), nil
}

// onCurve reports whether the 32 bytes decode as a point of the ed25519 curve,
// which is what disqualifies a candidate program address.
func onCurve(key Key32) bool {
	_, err := new(edwards25519.Point).SetBytes(key[:])
	return err == nil
}

func mustKey(address string) Key32 {
	key, err := Key(address)
	if err != nil {
		panic(err)
	}
	return key
}

func mustKeyHex(encoded string) Key32 {
	var key Key32
	mustHex(encoded, key[:])
	return key
}

func mustAddress(encoded string) Address20 {
	var address Address20
	mustHex(encoded, address[:])
	return address
}

func mustDigest(encoded string) Digest {
	var digest Digest
	mustHex(encoded, digest[:])
	return digest
}

func mustSignature(encoded string) Signature64 {
	var signature Signature64
	mustHex(encoded, signature[:])
	return signature
}

func mustHex(encoded string, into []byte) {
	raw, err := hex.DecodeString(encoded)
	if err != nil {
		panic(fmt.Sprintf("bridge/vectors: %q is not hex: %v", encoded, err))
	}
	if len(raw) != len(into) {
		panic(fmt.Sprintf("bridge/vectors: %q is %d bytes, want %d", encoded, len(raw), len(into)))
	}
	copy(into, raw)
}
