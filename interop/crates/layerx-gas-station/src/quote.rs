use sha3::{Digest, Keccak256};

pub type Address = [u8; 20];
pub type Word = [u8; 32];
pub const SIDIORA: Address = [
    0x21, 0xf7, 0xb2, 0x0a, 0x55, 0x51, 0x99, 0xfa, 0x73, 0xa2, 0x38, 0xb1, 0xa9, 0x1f, 0xd0, 0xf5,
    0x49, 0x06, 0x8f, 0xee,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Quote {
    pub sponsor: Address,
    pub token: Address,
    pub max_token_amount: Word,
    pub token_amount: Word,
    pub deadline: Word,
    pub quote_nonce: Word,
    pub gas_cost: Word,
}

#[must_use]
pub fn word(value: u128) -> Word {
    let mut result = [0; 32];
    result[16..].copy_from_slice(&value.to_be_bytes());
    result
}

#[must_use]
pub fn address_word(address: Address) -> Word {
    let mut result = [0; 32];
    result[12..].copy_from_slice(&address);
    result
}

#[must_use]
pub fn keccak(bytes: &[u8]) -> Word {
    Keccak256::digest(bytes).into()
}

#[must_use]
pub fn quote_digest(chain_id: Word, account: Address, quote: &Quote) -> Word {
    let typehash = keccak(b"Quote(uint256 chainId,address account,address sponsor,address token,uint256 maxTokenAmount,uint256 tokenAmount,uint256 deadline,uint256 quoteNonce,uint256 gasCost)");
    let encoded = [
        typehash,
        chain_id,
        address_word(account),
        address_word(quote.sponsor),
        address_word(quote.token),
        quote.max_token_amount,
        quote.token_amount,
        quote.deadline,
        quote.quote_nonce,
        quote.gas_cost,
    ]
    .concat();
    let mut prefixed = b"\x19Ethereum Signed Message:\n32".to_vec();
    prefixed.extend_from_slice(&keccak(&encoded));
    keccak(&prefixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vector() -> Quote {
        Quote {
            sponsor: [0x22; 20],
            token: SIDIORA,
            max_token_amount: word(2_100_000),
            token_amount: word(2_000_000),
            deadline: word(1000),
            quote_nonce: word(7),
            gas_cost: word(1_000_000_000_000_000_000),
        }
    }

    #[test]
    fn shared_foundry_quote_vector() {
        assert_eq!(
            quote_digest(word(1325), [0x11; 20], &vector()),
            [
                0x6c, 0x11, 0xf3, 0x4e, 0x78, 0x48, 0xd9, 0x8b, 0x1a, 0xe3, 0x28, 0xfe, 0x84, 0xbf,
                0x47, 0x22, 0x3e, 0xb1, 0x4e, 0x52, 0x74, 0xae, 0x04, 0xda, 0xf3, 0xdc, 0x64, 0xc3,
                0x04, 0xc8, 0x13, 0xba,
            ]
        );
    }

    #[test]
    fn binds_all_fields_and_full_width_values() {
        let original = vector();
        let digest = quote_digest(word(1325), [0x11; 20], &original);
        for field in 0..7 {
            let mut changed = original.clone();
            match field {
                0 => changed.sponsor[0] ^= 1,
                1 => changed.token[0] ^= 1,
                2 => changed.max_token_amount[0] ^= 1,
                3 => changed.token_amount[0] ^= 1,
                4 => changed.deadline[0] ^= 1,
                5 => changed.quote_nonce[0] ^= 1,
                _ => changed.gas_cost[0] ^= 1,
            }
            assert_ne!(quote_digest(word(1325), [0x11; 20], &changed), digest);
        }
        assert_ne!(quote_digest(word(1326), [0x11; 20], &original), digest);
        assert_ne!(quote_digest(word(1325), [0x12; 20], &original), digest);
    }
}
