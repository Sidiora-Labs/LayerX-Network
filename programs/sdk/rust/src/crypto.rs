//! Typed bindings to the deterministic version-two compute primitives.

use crate::{ProgramError, Field, Reason};

pub const MAX_HASH_INPUT_BYTES: usize = 1_048_576;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum HashAlgorithm { Sha256=1, Keccak256=2, Blake3=3 }

pub struct HashInput<'a>(&'a [u8]);
impl<'a> HashInput<'a> {
    /// Constructs a bounded input. # Errors Refuses input beyond the ABI bound.
    pub fn new(bytes: &'a [u8]) -> Result<Self, ProgramError> { if bytes.len()>MAX_HASH_INPUT_BYTES { Err(ProgramError::value(Field::Buffer,Reason::TooLarge)) } else { Ok(Self(bytes)) } }
}

/// An Ed25519 message within the frozen host bound.
pub struct Ed25519Message<'a>(&'a [u8]);
impl<'a> Ed25519Message<'a> {
    /// Constructs a bounded message. # Errors Refuses messages over 64 bytes.
    pub fn new(bytes:&'a [u8])->Result<Self,ProgramError>{if bytes.len()>64{Err(ProgramError::value(Field::Buffer,Reason::TooLarge))}else{Ok(Self(bytes))}}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Secp256k1PublicKey { Compressed([u8;33]), Uncompressed([u8;65]) }
impl Secp256k1PublicKey { fn bytes(&self)->&[u8] { match self { Self::Compressed(v)=>v, Self::Uncompressed(v)=>v } } }

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryId(u8);
impl RecoveryId { pub const fn new(value:u8)->Result<Self,ProgramError>{ if value<=3 { Ok(Self(value)) } else { Err(ProgramError::value(Field::Buffer,Reason::Malformed)) } } }

#[cfg(target_arch = "wasm32")]
pub fn hash(algorithm: HashAlgorithm, input: HashInput<'_>) -> Result<[u8;32],ProgramError> { let mut out=[0u8;32]; crate::host::hash(algorithm as i32,input.0,&mut out)?; Ok(out) }
#[cfg(target_arch = "wasm32")]
pub fn ed25519_verify(message:Ed25519Message<'_>, key:&[u8;32], signature:&[u8;64])->Result<(),ProgramError>{ crate::host::signature_verify(1,message.0,key,signature).map(|_|()) }
#[cfg(target_arch = "wasm32")]
pub fn secp256k1_verify(digest:&[u8;32], key:&Secp256k1PublicKey, signature:&[u8;64])->Result<(),ProgramError>{ crate::host::signature_verify(2,digest,key.bytes(),signature).map(|_|()) }
#[cfg(target_arch = "wasm32")]
pub fn secp256k1_recover(digest:&[u8;32], signature:&[u8;64], recovery_id:RecoveryId)->Result<[u8;65],ProgramError>{ let mut out=[0u8;65]; crate::host::signature_recover(digest,signature,i32::from(recovery_id.0),&mut out)?; Ok(out) }

#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct U256([u8;32]);
#[derive(Clone, Copy, Debug, Eq, PartialEq)] pub struct U512([u8;64]);
impl U512 { #[must_use] pub const fn to_be_bytes(self)->[u8;64]{self.0} }
impl U256 { #[must_use] pub const fn from_be_bytes(bytes:[u8;32])->Self{Self(bytes)} #[must_use] pub const fn to_be_bytes(self)->[u8;32]{self.0} }
#[cfg(target_arch = "wasm32")]
impl U256 {
    fn binary(self,rhs:Self,op:fn(&[u8;32],&[u8;32],&mut[u8;32])->Result<i32,ProgramError>)->Result<Self,ProgramError>{let mut out=[0u8;32];op(&self.0,&rhs.0,&mut out)?;Ok(Self(out))}
    pub fn widening_mul(self,rhs:Self)->Result<U512,ProgramError>{let mut out=[0u8;64];crate::host::bigint_mul(&self.0,&rhs.0,&mut out)?;Ok(U512(out))}
    pub fn checked_div(self,rhs:Self)->Result<Self,ProgramError>{self.binary(rhs,crate::host::bigint_div)}
    pub fn checked_rem(self,rhs:Self)->Result<Self,ProgramError>{self.binary(rhs,crate::host::bigint_rem)}
    pub fn modexp(self,exponent:Self,modulus:Self)->Result<Self,ProgramError>{let mut out=[0u8;32];crate::host::bigint_modexp(&self.0,&exponent.0,&modulus.0,&mut out)?;Ok(Self(out))}
}
