use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use crate::quote::{address_word, keccak, quote_digest, word, Address, Quote, Word};
use crate::signer::{QuoteSigner, SignerError};
use crate::SignedQuote;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TxError {
    Invalid,
    Signature,
    Signer(SignerError),
}
impl std::fmt::Display for TxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "transaction refused: {self:?}")
    }
}
impl std::error::Error for TxError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Call {
    pub to: Address,
    pub value: Word,
    pub data: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Authorization {
    pub chain_id: u64,
    pub delegate: Address,
    pub nonce: u64,
    pub signature: [u8; 65],
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Fees {
    pub gas_limit: u64,
    #[serde(with = "fee_amount")]
    pub max_fee_per_gas: u128,
    #[serde(with = "fee_amount")]
    pub max_priority_fee_per_gas: u128,
}
mod fee_amount {
    use serde::{Deserialize as _, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &u128, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u128, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.is_empty()
            || (text.len() > 1 && text.starts_with('0'))
            || !text.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(serde::de::Error::custom("invalid fee amount"));
        }
        text.parse()
            .map_err(|_| serde::de::Error::custom("invalid fee amount"))
    }
}
impl Fees {
    /// # Errors
    /// Refuses zero gas, invalid fee ordering or overflow.
    pub fn gas_cost(self) -> Result<u128, TxError> {
        if self.gas_limit == 0
            || self.max_fee_per_gas == 0
            || self.max_priority_fee_per_gas > self.max_fee_per_gas
        {
            return Err(TxError::Invalid);
        }
        u128::from(self.gas_limit)
            .checked_mul(self.max_fee_per_gas)
            .ok_or(TxError::Invalid)
    }
    /// The lowest fees a node accepts for a cancellation in place of a pending
    /// transaction paying `self`: both fee caps raised by the minimum bump.
    /// # Errors
    /// Refuses invalid original fees or overflow.
    pub fn replacement(self) -> Result<Self, TxError> {
        self.gas_cost()?;
        let bump = |fee: u128| {
            fee.checked_mul(100 + REPLACEMENT_BUMP_PERCENT)
                .map(|raised| raised.div_ceil(100))
                .ok_or(TxError::Invalid)
        };
        let fees = Self {
            gas_limit: CANCELLATION_GAS,
            max_fee_per_gas: bump(self.max_fee_per_gas)?,
            max_priority_fee_per_gas: bump(self.max_priority_fee_per_gas)?,
        };
        fees.gas_cost()?;
        Ok(fees)
    }
    /// # Errors
    /// Refuses invalid original fees.
    pub fn replaces(self, original: Self) -> Result<bool, TxError> {
        let floor = original.replacement()?;
        Ok(self.gas_limit == CANCELLATION_GAS
            && self.gas_cost().is_ok()
            && self.max_fee_per_gas >= floor.max_fee_per_gas
            && self.max_priority_fee_per_gas >= floor.max_priority_fee_per_gas)
    }
}

pub const CANCELLATION_GAS: u64 = 21_000;
pub const REPLACEMENT_BUMP_PERCENT: u128 = 10;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignedTransaction {
    pub raw: Vec<u8>,
    pub hash: Word,
}

fn rlp_length(length: usize, short: u8, out: &mut Vec<u8>) -> Result<(), TxError> {
    if length < 56 {
        out.push(short + u8::try_from(length).map_err(|_| TxError::Invalid)?);
    } else {
        let bytes = length.to_be_bytes();
        let skip = bytes.iter().take_while(|b| **b == 0).count();
        out.push(short + 55 + u8::try_from(bytes.len() - skip).map_err(|_| TxError::Invalid)?);
        out.extend_from_slice(&bytes[skip..]);
    }
    Ok(())
}
fn rlp_bytes(bytes: &[u8], out: &mut Vec<u8>) -> Result<(), TxError> {
    if bytes.len() == 1 && bytes[0] < 128 {
        out.push(bytes[0]);
    } else {
        rlp_length(bytes.len(), 128, out)?;
        out.extend_from_slice(bytes);
    }
    Ok(())
}
fn integer(bytes: &[u8], out: &mut Vec<u8>) -> Result<(), TxError> {
    let skip = bytes.iter().take_while(|b| **b == 0).count();
    rlp_bytes(&bytes[skip..], out)
}
fn list(payload: &[u8]) -> Result<Vec<u8>, TxError> {
    let mut result = Vec::new();
    rlp_length(payload.len(), 192, &mut result)?;
    result.extend_from_slice(payload);
    Ok(result)
}
fn auth_fields(auth: &Authorization) -> Result<Vec<u8>, TxError> {
    let mut result = Vec::new();
    integer(&auth.chain_id.to_be_bytes(), &mut result)?;
    rlp_bytes(&auth.delegate, &mut result)?;
    integer(&auth.nonce.to_be_bytes(), &mut result)?;
    Ok(result)
}
/// # Errors
/// Refuses an unencodable authorization.
pub fn authorization_digest(auth: &Authorization) -> Result<Word, TxError> {
    let mut payload = vec![5];
    payload.extend(list(&auth_fields(auth)?)?);
    Ok(keccak(&payload))
}
/// # Errors
/// Refuses noncanonical signatures and failed public-key recovery.
pub fn recover(digest: Word, signature: &[u8; 65]) -> Result<Address, TxError> {
    let parity = signature[64]
        .checked_sub(27)
        .filter(|p| *p <= 1)
        .ok_or(TxError::Signature)?;
    let sig = Signature::from_slice(&signature[..64]).map_err(|_| TxError::Signature)?;
    if sig.normalize_s().is_some() {
        return Err(TxError::Signature);
    }
    let recovery = RecoveryId::try_from(parity).map_err(|_| TxError::Signature)?;
    let key = VerifyingKey::recover_from_prehash(&digest, &sig, recovery)
        .map_err(|_| TxError::Signature)?;
    let hash = keccak(&key.to_encoded_point(false).as_bytes()[1..]);
    hash[12..].try_into().map_err(|_| TxError::Signature)
}
fn signature_fields(signature: &[u8; 65], out: &mut Vec<u8>) -> Result<(), TxError> {
    let parity = signature[64]
        .checked_sub(27)
        .filter(|p| *p <= 1)
        .ok_or(TxError::Signature)?;
    integer(&[parity], out)?;
    integer(&signature[..32], out)?;
    integer(&signature[32..64], out)
}
fn dynamic(bytes: &[u8]) -> Vec<u8> {
    let mut result = word(bytes.len() as u128).to_vec();
    result.extend_from_slice(bytes);
    result.resize(result.len().div_ceil(32) * 32, 0);
    result
}
fn calls_array(calls: &[Call]) -> Result<Vec<u8>, TxError> {
    if calls.len() > 256 || calls.iter().any(|c| c.data.len() > 65_536) {
        return Err(TxError::Invalid);
    }
    let mut head = word(calls.len() as u128).to_vec();
    let mut tail = Vec::new();
    for call in calls {
        head.extend(word((calls.len() * 32 + tail.len()) as u128));
        tail.extend(address_word(call.to));
        tail.extend(call.value);
        tail.extend(word(96));
        tail.extend(dynamic(&call.data));
    }
    head.extend(tail);
    Ok(head)
}
/// # Errors
/// Refuses oversized batches.
pub fn batch_digest(
    chain_id: u64,
    account: Address,
    batch_nonce: Word,
    calls: &[Call],
    quote: &Quote,
) -> Result<Word, TxError> {
    let encoded_calls = [word(32).to_vec(), calls_array(calls)?].concat();
    let encoded = [
        keccak(b"SponsoredBatch(uint256 nonce,bytes32 callsHash,bytes32 quoteDigest)"),
        batch_nonce,
        keccak(&encoded_calls),
        quote_digest(word(u128::from(chain_id)), account, quote),
    ]
    .concat();
    Ok(keccak(
        &[
            b"\x19Ethereum Signed Message:\n32".as_slice(),
            &keccak(&encoded),
        ]
        .concat(),
    ))
}
/// # Errors
/// Refuses oversized batches.
pub fn encode_sponsored(
    calls: &[Call],
    quote: &SignedQuote,
    account_signature: &[u8; 65],
) -> Result<Vec<u8>, TxError> {
    let calls = calls_array(calls)?;
    let account = dynamic(account_signature);
    let relayer = dynamic(&quote.signature);
    let q = &quote.quote;
    let mut result = keccak(b"executeSponsored((address,uint256,bytes)[],(address,address,uint256,uint256,uint256,uint256,uint256),bytes,bytes)")[..4].to_vec();
    result.extend(
        [
            word(320),
            address_word(q.sponsor),
            address_word(q.token),
            q.max_token_amount,
            q.token_amount,
            q.deadline,
            q.nonce,
            q.gas_cost,
            word((320 + calls.len()) as u128),
            word((320 + calls.len() + account.len()) as u128),
        ]
        .concat(),
    );
    result.extend(calls);
    result.extend(account);
    result.extend(relayer);
    Ok(result)
}

pub struct TransactionRequest<'a> {
    pub chain_id: u64,
    pub account: Address,
    pub paymaster: Address,
    pub nonce: u64,
    pub batch_nonce: Word,
    pub fees: Fees,
    pub calls: &'a [Call],
    pub authorizations: &'a [Authorization],
    pub quote: &'a SignedQuote,
    pub account_signature: &'a [u8; 65],
}
/// # Errors
/// Refuses wrong chains, delegates, signatures, gas budgets or signer identities.
pub fn sign(
    request: &TransactionRequest<'_>,
    signer: &impl QuoteSigner,
) -> Result<SignedTransaction, TxError> {
    let q = &request.quote.quote;
    if request.chain_id == 0
        || request.account == [0; 20]
        || request.account == signer.address()
        || q.sponsor != signer.address()
        || request.authorizations.len() != 1
        || q.gas_cost != word(request.fees.gas_cost()?)
    {
        return Err(TxError::Invalid);
    }
    let digest = quote_digest(word(u128::from(request.chain_id)), request.account, q);
    if digest != request.quote.digest
        || recover(digest, &request.quote.signature)? != signer.address()
        || recover(
            batch_digest(
                request.chain_id,
                request.account,
                request.batch_nonce,
                request.calls,
                q,
            )?,
            request.account_signature,
        )? != request.account
    {
        return Err(TxError::Signature);
    }
    let mut authorities = Vec::new();
    for auth in request.authorizations {
        if auth.chain_id != request.chain_id
            || auth.delegate != request.paymaster
            || auth.nonce == u64::MAX
            || recover(authorization_digest(auth)?, &auth.signature)? != request.account
        {
            return Err(TxError::Invalid);
        }
        let mut payload = auth_fields(auth)?;
        signature_fields(&auth.signature, &mut payload)?;
        authorities.extend(list(&payload)?);
    }
    let mut payload = Vec::new();
    for bytes in [
        request.chain_id.to_be_bytes().to_vec(),
        request.nonce.to_be_bytes().to_vec(),
        request.fees.max_priority_fee_per_gas.to_be_bytes().to_vec(),
        request.fees.max_fee_per_gas.to_be_bytes().to_vec(),
        request.fees.gas_limit.to_be_bytes().to_vec(),
    ] {
        integer(&bytes, &mut payload)?;
    }
    rlp_bytes(&request.account, &mut payload)?;
    integer(&[], &mut payload)?;
    rlp_bytes(
        &encode_sponsored(request.calls, request.quote, request.account_signature)?,
        &mut payload,
    )?;
    payload.push(192);
    payload.extend(list(&authorities)?);
    let unsigned = [vec![4], list(&payload)?].concat();
    let signature = signer
        .sign_digest(keccak(&unsigned))
        .map_err(TxError::Signer)?;
    if recover(keccak(&unsigned), &signature)? != signer.address() {
        return Err(TxError::Signature);
    }
    signature_fields(&signature, &mut payload)?;
    let raw = [vec![4], list(&payload)?].concat();
    Ok(SignedTransaction {
        hash: keccak(&raw),
        raw,
    })
}

/// Signs the zero-value self-transfer that fills a sponsor nonce in place of a
/// dropped or expired sponsored transaction.
/// # Errors
/// Refuses a zero chain, fees that are not cancellation fees, or signer failures.
pub fn sign_cancellation(
    chain_id: u64,
    nonce: u64,
    fees: Fees,
    signer: &impl QuoteSigner,
) -> Result<SignedTransaction, TxError> {
    if chain_id == 0 || fees.gas_limit != CANCELLATION_GAS {
        return Err(TxError::Invalid);
    }
    fees.gas_cost()?;
    let mut payload = Vec::new();
    for bytes in [
        chain_id.to_be_bytes().to_vec(),
        nonce.to_be_bytes().to_vec(),
        fees.max_priority_fee_per_gas.to_be_bytes().to_vec(),
        fees.max_fee_per_gas.to_be_bytes().to_vec(),
        fees.gas_limit.to_be_bytes().to_vec(),
    ] {
        integer(&bytes, &mut payload)?;
    }
    rlp_bytes(&signer.address(), &mut payload)?;
    integer(&[], &mut payload)?;
    rlp_bytes(&[], &mut payload)?;
    payload.push(192);
    let unsigned = [vec![2], list(&payload)?].concat();
    let signature = signer
        .sign_digest(keccak(&unsigned))
        .map_err(TxError::Signer)?;
    if recover(keccak(&unsigned), &signature)? != signer.address() {
        return Err(TxError::Signature);
    }
    signature_fields(&signature, &mut payload)?;
    let raw = [vec![2], list(&payload)?].concat();
    Ok(SignedTransaction {
        hash: keccak(&raw),
        raw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn encoding_and_refusals() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(list(&[])?, vec![192]);
        let mut encoded = Vec::new();
        integer(&[0, 0], &mut encoded)?;
        assert_eq!(encoded, vec![128]);
        assert_eq!(recover([0; 32], &[0; 65]), Err(TxError::Signature));
        assert!(calls_array(&vec![
            Call {
                to: [0; 20],
                value: word(0),
                data: vec![]
            };
            257
        ])
        .is_err());
        let quote = SignedQuote {
            quote: Quote {
                sponsor: [1; 20],
                token: [2; 20],
                max_token_amount: word(3),
                token_amount: word(4),
                deadline: word(5),
                nonce: word(6),
                gas_cost: word(7),
            },
            digest: word(0),
            signature: [0; 65],
        };
        let encoded = encode_sponsored(&[], &quote, &[0; 65])?;
        for (i, expected) in [
            word(320),
            address_word([1; 20]),
            address_word([2; 20]),
            word(3),
            word(4),
            word(5),
            word(6),
            word(7),
            word(352),
            word(480),
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(&encoded[4 + i * 32..4 + (i + 1) * 32], expected);
        }
        assert_eq!(
            Fees {
                gas_limit: 1,
                max_fee_per_gas: 1,
                max_priority_fee_per_gas: 2
            }
            .gas_cost(),
            Err(TxError::Invalid)
        );
        Ok(())
    }
    #[test]
    fn cancellation_fills_the_nonce_at_the_bumped_fee() -> Result<(), Box<dyn std::error::Error>> {
        let signer = crate::signer::tests::signer()?;
        let original = Fees {
            gas_limit: 200_000,
            max_fee_per_gas: 5_000_000_000_000,
            max_priority_fee_per_gas: 1_000_000_000,
        };
        let fees = original.replacement()?;
        assert_eq!(
            fees,
            Fees {
                gas_limit: CANCELLATION_GAS,
                max_fee_per_gas: 5_500_000_000_000,
                max_priority_fee_per_gas: 1_100_000_000,
            }
        );
        assert_eq!(
            Fees {
                gas_limit: 1,
                max_fee_per_gas: 11,
                max_priority_fee_per_gas: 1
            }
            .replacement()?,
            Fees {
                gas_limit: CANCELLATION_GAS,
                max_fee_per_gas: 13,
                max_priority_fee_per_gas: 2
            }
        );
        assert!(fees.replaces(original)?);
        let mut low = fees;
        low.max_fee_per_gas -= 1;
        assert!(!low.replaces(original)?);
        let mut low = fees;
        low.max_priority_fee_per_gas -= 1;
        assert!(!low.replaces(original)?);
        assert!(!original.replaces(original)?);
        assert_eq!(
            sign_cancellation(1325, 5, original, &signer),
            Err(TxError::Invalid)
        );
        assert_eq!(
            sign_cancellation(0, 5, fees, &signer),
            Err(TxError::Invalid)
        );
        let signed = sign_cancellation(1325, 5, fees, &signer)?;
        assert_eq!(signed.hash, keccak(&signed.raw));
        assert_eq!(&signed.raw[..2], &[2, 0xf8]);
        assert_eq!(usize::from(signed.raw[2]), signed.raw.len() - 3);
        let mut fields = vec![
            0x82, 0x05, 0x2d, 0x05, 0x84, 0x41, 0x90, 0xab, 0x00, 0x86, 0x05, 0x00, 0x91, 0x8b,
            0xd8, 0x00, 0x82, 0x52, 0x08, 0x94,
        ];
        fields.extend_from_slice(&signer.address());
        fields.extend_from_slice(&[0x80, 0x80, 0xc0]);
        assert_eq!(&signed.raw[3..3 + fields.len()], fields.as_slice());
        let tail = &signed.raw[3 + fields.len()..];
        let parity = match tail[0] {
            0x80 => 0,
            1 => 1,
            _ => return Err("invalid parity".into()),
        };
        let r_length = usize::from(tail[1] - 0x80);
        let r = &tail[2..2 + r_length];
        let s_length = usize::from(tail[2 + r_length] - 0x80);
        let s = &tail[3 + r_length..];
        assert_eq!(s.len(), s_length);
        let mut signature = [0_u8; 65];
        signature[32 - r.len()..32].copy_from_slice(r);
        signature[64 - s.len()..64].copy_from_slice(s);
        signature[64] = 27 + parity;
        let unsigned = [vec![2], list(&fields)?].concat();
        assert_eq!(recover(keccak(&unsigned), &signature)?, signer.address());
        Ok(())
    }
}
