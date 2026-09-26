package types

// The credential envelope: ECIES over secp256k1 with HKDF-SHA256 and
// AES-256-GCM, one envelope per attestor, readable only by the attestor whose
// registered key it is sealed to.
//
//	envelope = attestor (20) || ephemeralKey (33) || nonce (12) || ciphertext || tag (16)
//
//	shared   = x coordinate of ephemeralPrivate * attestorPublic, 32 bytes big-endian
//	key      = HKDF-SHA256(ikm = shared, salt = ephemeralKey, info = EnvelopeInfo, length 32)
//	aad      = attestor (20) || the ASCII origin of the api payload's URL
//	ciphertext || tag = AES-256-GCM(key, nonce, credential plaintext, aad)
//
// The attestor address travels in the clear so each sidecar picks its own
// envelope; the origin in the associated data means an envelope copied into
// a request for another origin does not open. The plaintext is the credential
// encoding of api.go. testdata/envelope-vectors.json pins keys, plaintexts
// and ciphertexts for every other implementation.

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/ecdsa"
	"crypto/hkdf"
	"crypto/rand"
	"crypto/sha256"

	"github.com/ethereum/go-ethereum/crypto"
)

const (
	// EnvelopeInfo is the HKDF info string.
	EnvelopeInfo = "PAXEERX_WEB_API_ENVELOPE_V1"

	EnvelopeKeyLength   = 33
	EnvelopeNonceLength = 12
	EnvelopeTagLength   = 16
	envelopeHeader      = 20 + EnvelopeKeyLength + EnvelopeNonceLength

	// EnvelopeOverhead is an envelope's length minus its plaintext's.
	EnvelopeOverhead = envelopeHeader + EnvelopeTagLength

	// MaxCredentialBytes bounds a credential plaintext; MaxEnvelopeBytes
	// bounds an envelope.
	MaxCredentialBytes = 1024
	MaxEnvelopeBytes   = EnvelopeOverhead + MaxCredentialBytes
)

// Envelope is one credential envelope. Ciphertext carries the GCM tag.
type Envelope struct {
	Attestor     Address20
	EphemeralKey [EnvelopeKeyLength]byte
	Nonce        [EnvelopeNonceLength]byte
	Ciphertext   []byte
}

// Bytes is the envelope's wire form.
func (e Envelope) Bytes() []byte {
	out := make([]byte, 0, envelopeHeader+len(e.Ciphertext))
	out = append(out, e.Attestor[:]...)
	out = append(out, e.EphemeralKey[:]...)
	out = append(out, e.Nonce[:]...)
	return append(out, e.Ciphertext...)
}

func (e Envelope) validate() error {
	if e.Attestor == (Address20{}) {
		return ErrInvalidEnvelope.Wrap("addressed to the zero attestor")
	}
	if length := envelopeHeader + len(e.Ciphertext); length < EnvelopeOverhead+1 || length > MaxEnvelopeBytes {
		return ErrInvalidEnvelope.Wrapf("%d bytes, want %d to %d", length, EnvelopeOverhead+1, MaxEnvelopeBytes)
	}
	if _, err := crypto.DecompressPubkey(e.EphemeralKey[:]); err != nil {
		return ErrInvalidEnvelope.Wrapf("ephemeral key: %v", err)
	}
	return nil
}

// ParseEnvelope splits raw into its parts and checks its length, its
// attestor and that its ephemeral key is a compressed secp256k1 point.
func ParseEnvelope(raw []byte) (Envelope, error) {
	if len(raw) < EnvelopeOverhead+1 || len(raw) > MaxEnvelopeBytes {
		return Envelope{}, ErrInvalidEnvelope.Wrapf("%d bytes, want %d to %d", len(raw), EnvelopeOverhead+1,
			MaxEnvelopeBytes)
	}
	var e Envelope
	copy(e.Attestor[:], raw[:20])
	copy(e.EphemeralKey[:], raw[20:20+EnvelopeKeyLength])
	copy(e.Nonce[:], raw[20+EnvelopeKeyLength:envelopeHeader])
	e.Ciphertext = append([]byte(nil), raw[envelopeHeader:]...)
	if err := e.validate(); err != nil {
		return Envelope{}, err
	}
	return e, nil
}

// EnvelopeAAD is the GCM associated data: the attestor then the origin.
func EnvelopeAAD(attestor Address20, origin string) []byte {
	return append(append([]byte(nil), attestor[:]...), origin...)
}

// EnvelopeSharedX is the 32-byte x coordinate of private * public.
func EnvelopeSharedX(private *ecdsa.PrivateKey, public *ecdsa.PublicKey) ([]byte, error) {
	curve := crypto.S256()
	if public == nil || public.X == nil || public.Y == nil || !curve.IsOnCurve(public.X, public.Y) {
		return nil, ErrInvalidEnvelope.Wrap("public key is not on secp256k1")
	}
	x, _ := curve.ScalarMult(public.X, public.Y, crypto.FromECDSA(private))
	if x == nil || x.Sign() == 0 {
		return nil, ErrInvalidEnvelope.Wrap("shared point is the identity")
	}
	return x.FillBytes(make([]byte, 32)), nil
}

// EnvelopeKey is HKDF-SHA256 of the shared x coordinate, salted with the
// compressed ephemeral key, under EnvelopeInfo: the 32-byte AES-256 key.
func EnvelopeKey(sharedX []byte, ephemeralKey []byte) ([]byte, error) {
	key, err := hkdf.Key(sha256.New, sharedX, ephemeralKey, EnvelopeInfo, 32)
	if err != nil {
		return nil, ErrInvalidEnvelope.Wrapf("hkdf: %v", err)
	}
	return key, nil
}

func envelopeAEAD(key []byte) (cipher.AEAD, error) {
	block, err := aes.NewCipher(key)
	if err != nil {
		return nil, ErrInvalidEnvelope.Wrapf("aes: %v", err)
	}
	aead, err := cipher.NewGCM(block)
	if err != nil {
		return nil, ErrInvalidEnvelope.Wrapf("gcm: %v", err)
	}
	return aead, nil
}

// SealEnvelope seals plaintext to recipient for origin with a fresh
// ephemeral key and nonce.
func SealEnvelope(recipient *ecdsa.PublicKey, origin string, plaintext []byte) ([]byte, error) {
	ephemeral, err := crypto.GenerateKey()
	if err != nil {
		return nil, ErrInvalidEnvelope.Wrapf("ephemeral key: %v", err)
	}
	var nonce [EnvelopeNonceLength]byte
	if _, err := rand.Read(nonce[:]); err != nil {
		return nil, ErrInvalidEnvelope.Wrapf("nonce: %v", err)
	}
	return SealEnvelopeWith(recipient, ephemeral, nonce, origin, plaintext)
}

// SealEnvelopeWith seals plaintext to recipient for origin with the given
// ephemeral key and nonce, which must never be used twice.
func SealEnvelopeWith(recipient *ecdsa.PublicKey, ephemeral *ecdsa.PrivateKey, nonce [EnvelopeNonceLength]byte,
	origin string, plaintext []byte) ([]byte, error) {
	if len(plaintext) == 0 || len(plaintext) > MaxCredentialBytes {
		return nil, ErrInvalidEnvelope.Wrapf("plaintext is %d bytes, want 1 to %d", len(plaintext), MaxCredentialBytes)
	}
	shared, err := EnvelopeSharedX(ephemeral, recipient)
	if err != nil {
		return nil, err
	}
	e := Envelope{Attestor: Address20(crypto.PubkeyToAddress(*recipient)), Nonce: nonce}
	copy(e.EphemeralKey[:], crypto.CompressPubkey(&ephemeral.PublicKey))
	key, err := EnvelopeKey(shared, e.EphemeralKey[:])
	if err != nil {
		return nil, err
	}
	aead, err := envelopeAEAD(key)
	if err != nil {
		return nil, err
	}
	e.Ciphertext = aead.Seal(nil, nonce[:], plaintext, EnvelopeAAD(e.Attestor, origin))
	return e.Bytes(), nil
}

// OpenEnvelope opens raw with the attestor key for origin. It refuses an
// envelope addressed to another attestor and one that does not authenticate.
func OpenEnvelope(raw []byte, key *ecdsa.PrivateKey, origin string) ([]byte, error) {
	e, err := ParseEnvelope(raw)
	if err != nil {
		return nil, err
	}
	own := Address20(crypto.PubkeyToAddress(key.PublicKey))
	if e.Attestor != own {
		return nil, ErrInvalidEnvelope.Wrapf("addressed to %s, this key is %s", e.Attestor.Hex(), own.Hex())
	}
	ephemeral, err := crypto.DecompressPubkey(e.EphemeralKey[:])
	if err != nil {
		return nil, ErrInvalidEnvelope.Wrapf("ephemeral key: %v", err)
	}
	shared, err := EnvelopeSharedX(key, ephemeral)
	if err != nil {
		return nil, err
	}
	aesKey, err := EnvelopeKey(shared, e.EphemeralKey[:])
	if err != nil {
		return nil, err
	}
	aead, err := envelopeAEAD(aesKey)
	if err != nil {
		return nil, err
	}
	plaintext, err := aead.Open(nil, e.Nonce[:], e.Ciphertext, EnvelopeAAD(e.Attestor, origin))
	if err != nil {
		return nil, ErrInvalidEnvelope.Wrapf("does not authenticate for %s", origin)
	}
	return plaintext, nil
}
