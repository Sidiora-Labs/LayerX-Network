// Package verify checks LayerX signed objects and proofs against caller
// supplied trust anchors with the refusal semantics of the Rust verifier
// (agent/crates/layerx-proof, layerx-crypto, layerx-client). It is pure: no
// clock, network, database or chain state. Every failure is an error.
package verify

import (
	"crypto/ed25519"
	"errors"

	"filippo.io/edwards25519"
	"github.com/sidiora-labs/paxeer-network/layerxproof/codec"
)

// ErrBadSignature covers a non-canonical, weak, non-reduced or mathematically
// invalid key or signature, exactly as layerx-crypto VerifyError::BadSignature.
var ErrBadSignature = errors.New("layerx verify: bad signature")

var fieldPrime = [32]byte{
	0xed, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
	0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f,
}

// groupOrder is the little-endian Ed25519 group order L.
var groupOrder = [32]byte{
	0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
	0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
}

func littleEndianLess(left, right *[32]byte) bool {
	for index := 31; index >= 0; index-- {
		if left[index] != right[index] {
			return left[index] < right[index]
		}
	}
	return false
}

// PublicKeyIsCanonical mirrors lxp_ed25519_pubkey_is_canonical: the y
// coordinate is reduced and is neither zero nor one.
func PublicKeyIsCanonical(publicKey [32]byte) bool {
	y := publicKey
	y[31] &= 0x7f
	rest := byte(0)
	for _, b := range y[1:] {
		rest |= b
	}
	if rest == 0 && (y[0] == 0 || y[0] == 1) {
		return false
	}
	return littleEndianLess(&y, &fieldPrime)
}

func smallOrder(encoded []byte) (bool, error) {
	point, err := new(edwards25519.Point).SetBytes(encoded)
	if err != nil {
		return false, err
	}
	return new(edwards25519.Point).MultByCofactor(point).Equal(edwards25519.NewIdentityPoint()) == 1, nil
}

// Ed25519 verifies one strict Ed25519 signature over message. It refuses a
// non-canonical or small-order public key, a small-order or undecodable R, a
// non-reduced S and any signature failing the cofactorless equation.
func Ed25519(publicKey [32]byte, signature [64]byte, message []byte) error {
	if !PublicKeyIsCanonical(publicKey) {
		return ErrBadSignature
	}
	var s [32]byte
	copy(s[:], signature[32:])
	if !littleEndianLess(&s, &groupOrder) {
		return ErrBadSignature
	}
	if weak, err := smallOrder(publicKey[:]); err != nil || weak {
		return ErrBadSignature
	}
	if weak, err := smallOrder(signature[:32]); err != nil || weak {
		return ErrBadSignature
	}
	if !ed25519.Verify(ed25519.PublicKey(publicKey[:]), message, signature[:]) {
		return ErrBadSignature
	}
	return nil
}

// Ed25519Digest verifies a signature over an already domain-separated digest.
func Ed25519Digest(publicKey [32]byte, signature [64]byte, digest [32]byte) error {
	return Ed25519(publicKey, signature, digest[:])
}

// Ed25519Domain verifies a signature over SHA256(domain tag || canonical),
// the form every LayerX signed object uses.
func Ed25519Domain(publicKey [32]byte, signature [64]byte, domain codec.Domain, canonical []byte) error {
	digest, err := codec.DomainHash(domain, canonical)
	if err != nil {
		return err
	}
	return Ed25519Digest(publicKey, signature, digest)
}
