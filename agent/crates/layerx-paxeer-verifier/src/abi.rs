use crate::encoding::invalid;
use crate::EndpointFault;
use sha3::{Digest as _, Keccak256};

pub(crate) fn word(value: u64) -> [u8; 32] {
    let mut result = [0; 32];
    result[24..].copy_from_slice(&value.to_be_bytes());
    result
}

pub(crate) fn call(signature: &str, words: &[[u8; 32]]) -> Vec<u8> {
    let mut result = Keccak256::digest(signature.as_bytes())[..4].to_vec();
    for word in words {
        result.extend_from_slice(word);
    }
    result
}

/// Argument `index` of `count` dynamic `bytes` arguments.
pub(crate) fn dynamic(args: &[u8], count: usize, index: usize) -> Result<&[u8], EndpointFault> {
    let number = |at: usize| -> Result<usize, EndpointFault> {
        let field = args
            .get(at..at.checked_add(32).ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        if field[..24] != [0; 24] {
            return Err(invalid());
        }
        usize::try_from(u64::from_be_bytes(
            field[24..].try_into().map_err(|_| invalid())?,
        ))
        .map_err(|_| invalid())
    };
    if index >= count {
        return Err(invalid());
    }
    let offset = number(index * 32)?;
    if offset < count * 32 || offset % 32 != 0 {
        return Err(invalid());
    }
    let length = number(offset)?;
    let start = offset.checked_add(32).ok_or_else(invalid)?;
    let end = start.checked_add(length).ok_or_else(invalid)?;
    let padded = start
        .checked_add(length.div_ceil(32) * 32)
        .ok_or_else(invalid)?;
    if args
        .get(end..padded)
        .ok_or_else(invalid)?
        .iter()
        .any(|byte| *byte != 0)
    {
        return Err(invalid());
    }
    args.get(start..end).ok_or_else(invalid)
}
