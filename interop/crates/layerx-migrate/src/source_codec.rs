use crate::MigrationError;

pub(crate) struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    pub(crate) fn new(domain: &[u8]) -> Self {
        Self {
            bytes: domain.to_vec(),
        }
    }

    pub(crate) fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub(crate) fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn u128(&mut self, value: u128) {
        self.bytes.extend_from_slice(&value.to_be_bytes());
    }

    pub(crate) fn fixed(&mut self, value: &[u8]) {
        self.bytes.extend_from_slice(value);
    }

    pub(crate) fn finish(self) -> Vec<u8> {
        self.bytes
    }
}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8], domain: &[u8]) -> Result<Self, MigrationError> {
        if !bytes.starts_with(domain) {
            return Err(MigrationError::InvalidEvidence);
        }
        Ok(Self {
            bytes,
            offset: domain.len(),
        })
    }

    pub(crate) fn u8(&mut self) -> Result<u8, MigrationError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn u64(&mut self) -> Result<u64, MigrationError> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    pub(crate) fn u128(&mut self) -> Result<u128, MigrationError> {
        Ok(u128::from_be_bytes(self.array()?))
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], MigrationError> {
        self.take(N)?
            .try_into()
            .map_err(|_| MigrationError::InvalidEvidence)
    }

    pub(crate) fn finish(self) -> Result<(), MigrationError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(MigrationError::InvalidEvidence)
        }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], MigrationError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(MigrationError::InvalidEvidence)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(MigrationError::InvalidEvidence)?;
        self.offset = end;
        Ok(value)
    }
}

pub(crate) fn decode_hex(value: &str) -> Result<Vec<u8>, MigrationError> {
    let value = value
        .strip_prefix("0x")
        .ok_or(MigrationError::RpcResponseMismatch)?;
    if value.len() % 2 != 0 {
        return Err(MigrationError::RpcResponseMismatch);
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair =
                std::str::from_utf8(pair).map_err(|_| MigrationError::RpcResponseMismatch)?;
            u8::from_str_radix(pair, 16).map_err(|_| MigrationError::RpcResponseMismatch)
        })
        .collect()
}

pub(crate) fn decode_fixed_hex<const N: usize>(value: &str) -> Result<[u8; N], MigrationError> {
    decode_hex(value)?
        .try_into()
        .map_err(|_| MigrationError::RpcResponseMismatch)
}

pub(crate) fn decode_quantity(value: &str) -> Result<u64, MigrationError> {
    let digits = value
        .strip_prefix("0x")
        .ok_or(MigrationError::RpcResponseMismatch)?;
    if digits.is_empty()
        || (digits.len() > 1 && digits.starts_with('0'))
        || !digits.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(MigrationError::RpcResponseMismatch);
    }
    u64::from_str_radix(digits, 16).map_err(|_| MigrationError::RpcResponseMismatch)
}

pub(crate) fn hex(value: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(value.len().saturating_mul(2));
    for byte in value {
        output.push(char::from(DIGITS[usize::from(byte >> 4)]));
        output.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    output
}

pub(crate) fn ethereum_hex(value: &[u8]) -> String {
    format!("0x{}", hex(value))
}

pub(crate) fn quantity(value: u64) -> String {
    format!("0x{value:x}")
}

pub(crate) fn base58_decode(value: &str, maximum: usize) -> Result<Vec<u8>, MigrationError> {
    if value.is_empty() || value.len() > maximum.saturating_mul(2) {
        return Err(MigrationError::RpcResponseMismatch);
    }
    let mut bytes = vec![0_u8];
    for character in value.bytes() {
        let digit = base58_digit(character).ok_or(MigrationError::RpcResponseMismatch)?;
        let mut carry = u32::from(digit);
        for byte in bytes.iter_mut().rev() {
            let next = u32::from(*byte).saturating_mul(58).saturating_add(carry);
            *byte = (next & 0xff) as u8;
            carry = next >> 8;
        }
        while carry > 0 {
            bytes.insert(0, (carry & 0xff) as u8);
            carry >>= 8;
        }
        if bytes.len() > maximum {
            return Err(MigrationError::RpcResponseMismatch);
        }
    }
    let leading = value.bytes().take_while(|byte| *byte == b'1').count();
    let first_nonzero = bytes
        .iter()
        .position(|byte| *byte != 0)
        .unwrap_or(bytes.len());
    let mut decoded = vec![0_u8; leading];
    decoded.extend_from_slice(&bytes[first_nonzero..]);
    if decoded.len() > maximum {
        return Err(MigrationError::RpcResponseMismatch);
    }
    Ok(decoded)
}

pub(crate) fn base58_encode(value: &[u8]) -> String {
    const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    if value.is_empty() {
        return String::new();
    }
    let zeroes = value.iter().take_while(|byte| **byte == 0).count();
    let mut digits = vec![0_u8];
    for byte in value {
        let mut carry = u32::from(*byte);
        for digit in digits.iter_mut().rev() {
            let next = u32::from(*digit).saturating_mul(256).saturating_add(carry);
            *digit = (next % 58) as u8;
            carry = next / 58;
        }
        while carry > 0 {
            digits.insert(0, (carry % 58) as u8);
            carry /= 58;
        }
    }
    let first_nonzero = digits
        .iter()
        .position(|digit| *digit != 0)
        .unwrap_or(digits.len());
    let mut output = String::with_capacity(zeroes.saturating_add(digits.len()));
    output.extend(std::iter::repeat_n('1', zeroes));
    for digit in &digits[first_nonzero..] {
        output.push(char::from(ALPHABET[usize::from(*digit)]));
    }
    output
}

fn base58_digit(value: u8) -> Option<u8> {
    const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    ALPHABET
        .iter()
        .position(|candidate| *candidate == value)
        .and_then(|position| u8::try_from(position).ok())
}
