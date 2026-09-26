package vectors_test

import (
	"crypto/sha256"
	"encoding/hex"
	"math/big"
	"os"
	"strings"
	"testing"

	"github.com/ethereum/go-ethereum/common"
	"github.com/ethereum/go-ethereum/crypto"
	"github.com/sidiora-labs/paxeer-network/bridge/vectors"
	"github.com/sidiora-labs/paxeer-network/modules/layerxbridge/types"
	"github.com/stretchr/testify/require"
)

// eip155Chains are the EIP-155 chain ids the bridge's own chain configurations
// use. The reserved Solana id must stay above all of them.
var eip155Chains = []struct {
	name string
	id   uint64
}{
	{"ethereum", 1},
	{"optimism", 10},
	{"bnb", 56},
	{"polygon", 137},
	{"hyperevm", 999},
	{"base", 8453},
	{"avalanche", 43114},
	{"arbitrum", 42161},
}

// document reads bridge/ATTESTATION-SOLANA.md, lowercased, so a value pinned in
// Go must also be written in the document that states the mapping.
func document(t *testing.T) string {
	t.Helper()
	raw, err := os.ReadFile("../ATTESTATION-SOLANA.md")
	require.NoError(t, err)
	return strings.ToLower(string(raw))
}

func TestSolanaChainIDIsTheReservedLabel(t *testing.T) {
	fromLabel, err := vectors.ChainIDFromLabel(vectors.SolanaChainIDLabel)
	require.NoError(t, err)
	require.Equal(t, vectors.SolanaChainID, fromLabel)

	padded := make([]byte, 8)
	copy(padded[8-len(vectors.SolanaChainIDLabel):], vectors.SolanaChainIDLabel)
	require.Equal(t, "0000534f4c414e41", hex.EncodeToString(padded))
	require.Equal(t, vectors.SolanaChainID, new(big.Int).SetBytes(padded).Uint64())
	require.Equal(t, uint64(91600046870081), vectors.SolanaChainID)

	for _, chain := range eip155Chains {
		require.Greater(t, vectors.SolanaChainID, chain.id, "the Solana id must not collide with %s", chain.name)
	}

	_, err = vectors.ChainIDFromLabel("")
	require.Error(t, err)
	_, err = vectors.ChainIDFromLabel("SOLANA_MAINNET")
	require.Error(t, err)
}

func TestHandleIsTheLastTwentyBytesOfKeccak(t *testing.T) {
	for _, key := range []vectors.Key32{vectors.SidioraMint, vectors.WrappedSolMint, vectors.VectorVaultAuthority, vectors.OutboundVector.RecipientKey} {
		expected := crypto.Keccak256(key[:])[12:]
		handle := vectors.Handle(key)
		require.Equal(t, expected, handle[:], "handle of %s", key.Base58())
	}
}

func TestAssetIDsOfTheRegisteredMints(t *testing.T) {
	require.Equal(t, vectors.WrappedSolAssetID, vectors.Handle(vectors.WrappedSolMint),
		"the wrapped SOL asset id is the registry default, the mint's derived handle")

	// Sidiora is the one mint whose asset id is not its handle: the chain fixed
	// the pair to the bank-backed pointer over the usid denom.
	require.Equal(t, types.Address20(common.HexToAddress(types.SidioraRemoteAddress)), types.Address20(vectors.SidioraAssetID))
	require.NotEqual(t, vectors.SidioraAssetID, vectors.Handle(vectors.SidioraMint))
	require.Equal(t, types.SidioraDecimals, uint32(vectors.SidioraDecimals))

	require.Equal(t, vectors.SidioraMintBase58, vectors.SidioraMint.Base58())
	require.Equal(t, vectors.WrappedSolMintBase58, vectors.WrappedSolMint.Base58())
}

func TestVaultHandleComesFromTheVaultAuthorityPDA(t *testing.T) {
	require.Equal(t, vectors.VectorProgramID[:], crypto.Keccak256([]byte(vectors.VectorProgramIDLabel)),
		"the vector program id is keccak256 of its documented label")

	authority, bump, err := vectors.VaultAuthority(vectors.VectorProgramID)
	require.NoError(t, err)
	require.Equal(t, vectors.VectorVaultAuthority, authority)
	require.Equal(t, vectors.VectorVaultAuthorityBump, bump)

	// The PDA is exactly the runtime's hash of the seed, the bump, the program
	// id and the marker.
	expected := sha256.Sum256(append(append(append(
		[]byte(vectors.VaultAuthoritySeed), bump),
		vectors.VectorProgramID[:]...),
		[]byte("ProgramDerivedAddress")...))
	require.Equal(t, expected[:], authority[:])

	handle, err := vectors.VaultHandle(vectors.VectorProgramID)
	require.NoError(t, err)
	require.Equal(t, vectors.VectorVaultHandle, handle)
	require.Equal(t, vectors.Handle(authority), handle)

	direct, err := vectors.CreateProgramAddress(vectors.VectorProgramID, [][]byte{[]byte(vectors.VaultAuthoritySeed)}, bump)
	require.NoError(t, err)
	require.Equal(t, authority, direct)

	// A bump whose hash lands on the ed25519 curve is refused rather than
	// returned, because such an address could have a private key.
	refused := 0
	for candidate := 0; candidate <= 255; candidate++ {
		if _, err := vectors.CreateProgramAddress(vectors.VectorProgramID, [][]byte{[]byte(vectors.VaultAuthoritySeed)}, uint8(candidate)); err != nil {
			refused++
		}
	}
	require.Greater(t, refused, 0, "a program address on the curve is refused")

	_, err = vectors.CreateProgramAddress(vectors.VectorProgramID, [][]byte{make([]byte, 33)}, 255)
	require.Error(t, err, "a seed longer than 32 bytes is refused")
}

func TestInboundVectorMatchesTheChainsPreimage(t *testing.T) {
	vector := vectors.InboundVector
	require.Equal(t, vectors.SolanaChainID, vector.ChainID)
	require.Equal(t, vectors.VectorVaultHandle, vector.Vault)
	require.Equal(t, vectors.SidioraAssetID, vector.Asset)

	// The txHash is keccak256 of the 64-byte transaction signature, and the
	// signature itself is the documented derivation.
	label := []byte(vectors.VectorDepositSignatureLabel)
	derived := append(crypto.Keccak256(label, []byte{1}), crypto.Keccak256(label, []byte{2})...)
	require.Equal(t, derived, vector.Signature[:])
	require.Equal(t, vector.TxHash, vectors.InboundTxHash(vector.Signature))
	require.Equal(t, crypto.Keccak256(vector.Signature[:]), vector.TxHash[:])

	// The recipient is a Paxeer EVM address widened to the 32-byte word, and
	// the chain reads that address back out of it.
	paxeer := crypto.Keccak256([]byte(vectors.VectorPaxeerRecipientLabel))[12:]
	var address vectors.Address20
	copy(address[:], paxeer)
	require.Equal(t, vector.Recipient, vectors.PaxeerRecipient(address))
	recovered, ok := types.RecipientAddress(types.Hash32(vector.Recipient))
	require.True(t, ok)
	require.Equal(t, paxeer, recovered[:])

	amount, err := vector.AmountBig()
	require.NoError(t, err)

	in := types.BridgeIn{
		ChainID:   vector.ChainID,
		Vault:     types.Address20(vector.Vault),
		TxHash:    types.Hash32(vector.TxHash),
		LogIndex:  vector.LogIndex,
		Recipient: types.Hash32(vector.Recipient),
		Asset:     types.Address20(vector.Asset),
		Amount:    amount,
	}
	preimage := types.InboundPreimage(in)
	require.Len(t, preimage, vector.PreimageLength)
	require.Equal(t, types.InboundPreimageLength, vector.PreimageLength)
	require.True(t, strings.HasPrefix(string(preimage), types.DomainIn))

	digest := types.InboundDigest(in)
	require.Equal(t, vector.Digest[:], digest[:], "the pinned inbound digest is the one the chain builds")
	require.Contains(t, document(t), strings.ToLower(vector.Digest.Hex()), "the inbound digest is in the document")
}

func TestOutboundVectorMatchesTheChainsPreimage(t *testing.T) {
	vector := vectors.OutboundVector
	require.Equal(t, vectors.SolanaChainID, vector.ChainID)
	require.Equal(t, vectors.VectorVaultHandle, vector.Vault)
	require.Equal(t, vectors.SidioraAssetID, vector.Asset)

	require.Equal(t, vector.RecipientKey[:], crypto.Keccak256([]byte(vectors.VectorSolanaRecipientLabel)))
	require.Equal(t, vector.Recipient, vectors.Handle(vector.RecipientKey),
		"the release pays the key the attestors signed for, through its handle")
	require.Equal(t, vector.PaxeerTxHash[:], crypto.Keccak256([]byte(vectors.VectorPaxeerBurnLabel)))

	amount, err := vector.AmountBig()
	require.NoError(t, err)

	out := types.BridgeOut{
		ChainID:      vector.ChainID,
		Vault:        types.Address20(vector.Vault),
		PaxeerTxHash: types.Hash32(vector.PaxeerTxHash),
		PaxeerNonce:  vector.PaxeerNonce,
		Recipient:    types.Address20(vector.Recipient),
		Asset:        types.Address20(vector.Asset),
		Amount:       amount,
	}
	preimage := types.OutboundPreimage(out)
	require.Len(t, preimage, vector.PreimageLength)
	require.Equal(t, types.OutboundPreimageLength, vector.PreimageLength)
	require.True(t, strings.HasPrefix(string(preimage), types.DomainOut))

	digest := types.OutboundDigest(out)
	require.Equal(t, vector.Digest[:], digest[:], "the pinned outbound digest is the one the chain builds")
	require.Contains(t, document(t), strings.ToLower(vector.Digest.Hex()), "the outbound digest is in the document")
}

// TestAttestorSignsTheVectorDigests signs both pinned digests with a real
// secp256k1 key and recovers them under the vault's own rules, so the digests
// are provably the bytes an attestor signs and nothing here needs a second
// signature format for Solana.
func TestAttestorSignsTheVectorDigests(t *testing.T) {
	key, err := crypto.GenerateKey()
	require.NoError(t, err)
	attestor := types.Address20(crypto.PubkeyToAddress(key.PublicKey))

	for name, digest := range map[string]vectors.Digest{
		"inbound":  vectors.InboundVector.Digest,
		"outbound": vectors.OutboundVector.Digest,
	} {
		signature, err := crypto.Sign(digest[:], key)
		require.NoError(t, err, name)
		require.Len(t, signature, types.SignatureLength)
		signature[64] += 27

		signer, err := types.RecoverSigner(types.Hash32(digest), signature)
		require.NoError(t, err, name)
		require.Equal(t, attestor, signer, name)

		// The same signature carrying the high s the curve also admits is
		// refused, with the v it was produced with left alone so the refusal is
		// the s bound and nothing else.
		order := crypto.S256().Params().N
		high := new(big.Int).Sub(order, new(big.Int).SetBytes(signature[32:64]))
		highS := append([]byte(nil), signature[:32]...)
		highS = append(highS, common.LeftPadBytes(high.Bytes(), 32)...)
		highS = append(highS, signature[64])
		_, err = types.RecoverSigner(types.Hash32(digest), highS)
		require.Error(t, err, "a high s signature over the %s digest is refused", name)

		short := signature[:64]
		_, err = types.RecoverSigner(types.Hash32(digest), short)
		require.Error(t, err, "a signature that is not 65 bytes is refused")

		wrongV := append([]byte(nil), signature...)
		wrongV[64] = 29
		_, err = types.RecoverSigner(types.Hash32(digest), wrongV)
		require.Error(t, err, "a v outside 27 and 28 is refused")
	}
}

func TestBase58RoundTripsAndRefusesBadInput(t *testing.T) {
	for _, address := range []string{vectors.SidioraMintBase58, vectors.WrappedSolMintBase58} {
		raw, err := vectors.DecodeBase58(address)
		require.NoError(t, err)
		require.Len(t, raw, 32)
		require.Equal(t, address, vectors.EncodeBase58(raw))
	}

	require.Equal(t, vectors.VectorDepositSignature.Base58(),
		vectors.EncodeBase58(vectors.VectorDepositSignature[:]))

	_, err := vectors.DecodeBase58("")
	require.Error(t, err)
	_, err = vectors.DecodeBase58("0OIl")
	require.Error(t, err, "the characters base58 excludes are refused, not skipped")
	_, err = vectors.Key("So1111111111111111111111111111111111111111")
	require.Error(t, err, "an address that is not 32 bytes is refused")
}

// TestDocumentCarriesEveryPinnedValue keeps bridge/ATTESTATION-SOLANA.md and
// this package from drifting: every identity the vectors pin is written in the
// document that states the mapping.
func TestDocumentCarriesEveryPinnedValue(t *testing.T) {
	text := document(t)

	for _, value := range []string{
		vectors.SidioraMintBase58,
		vectors.WrappedSolMintBase58,
		vectors.VectorVaultAuthority.Base58(),
		vectors.VaultAuthoritySeed,
		vectors.VectorProgramIDLabel,
		vectors.VectorDepositSignatureLabel,
		vectors.VectorPaxeerRecipientLabel,
		vectors.VectorSolanaRecipientLabel,
		vectors.VectorPaxeerBurnLabel,
		vectors.SidioraAssetID.Hex(),
		vectors.WrappedSolAssetID.Hex(),
		vectors.VectorVaultHandle.Hex(),
		vectors.InboundVector.TxHash.Hex(),
		vectors.InboundVector.Recipient.Hex(),
		vectors.InboundVector.Digest.Hex(),
		vectors.OutboundVector.PaxeerTxHash.Hex(),
		vectors.OutboundVector.RecipientKey.Base58(),
		vectors.OutboundVector.Recipient.Hex(),
		vectors.OutboundVector.Digest.Hex(),
		vectors.InboundVector.Amount,
		vectors.OutboundVector.Amount,
	} {
		require.Contains(t, text, strings.ToLower(value))
	}

	// The document inherits the preimages and the signature rules rather than
	// restating them, and says the Paxeer-side copy is not edited here.
	require.Contains(t, text, strings.ToLower("modules/layerxbridge/ATTESTATION.md"))
	require.Contains(t, text, strings.ToLower(types.DomainIn))
	require.Contains(t, text, strings.ToLower(types.DomainOut))
}
