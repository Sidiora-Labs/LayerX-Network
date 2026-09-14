//! Canonical native Budget state records shared by protocol consumers.

/// A malformed native Budget state record.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BudgetRecordError;

/// Native Budget state with explicitly timestamp-based period fields.
#[derive(Debug, Eq, PartialEq)]
pub struct BudgetRecord {
    pub id: [u8; 32],
    pub owner: [u8; 32],
    pub account: [u8; 32],
    pub asset: [u8; 32],
    pub source: Option<[u8; 32]>,
    pub limit: u128,
    pub spent: u128,
    pub period_start: u64,
    pub period_length: u64,
    pub expiry: u64,
    pub revocation: u64,
    pub closed: bool,
    pub revoked: bool,
}

fn field<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], BudgetRecordError> {
    bytes
        .get(offset..offset + N)
        .ok_or(BudgetRecordError)?
        .try_into()
        .map_err(|_| BudgetRecordError)
}

impl BudgetRecord {
    /// Decodes the exact version-one or version-two native Budget record.
    ///
    /// # Errors
    /// Refuses malformed lengths, versions, flags, identities, limits and delegates.
    pub fn decode(bytes: &[u8]) -> Result<Self, BudgetRecordError> {
        if bytes.len() < 278
            || bytes[0] != 0
            || !matches!(bytes[1], 1 | 2)
            || bytes[275] > 1
            || bytes[276] > 1
            || bytes[277] > 16
        {
            return Err(BudgetRecordError);
        }
        let delegates = usize::from(bytes[277]);
        let source_offset = 278 + delegates * 32;
        let has_source = bytes[1] == 2;
        if bytes.len() != source_offset + if has_source { 32 } else { 0 }
            || bytes[278..source_offset]
                .chunks_exact(32)
                .collect::<Vec<_>>()
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(BudgetRecordError);
        }
        let record = Self {
            id: field(bytes, 2)?,
            owner: field(bytes, 34)?,
            account: field(bytes, 66)?,
            asset: field(bytes, 98)?,
            source: if has_source {
                Some(field(bytes, source_offset)?)
            } else {
                None
            },
            limit: u128::from_be_bytes(field(bytes, 162)?),
            spent: u128::from_be_bytes(field(bytes, 210)?),
            period_start: u64::from_be_bytes(field(bytes, 250)?),
            period_length: u64::from_be_bytes(field(bytes, 242)?),
            expiry: u64::from_be_bytes(field(bytes, 258)?),
            revocation: u64::from_be_bytes(field(bytes, 266)?),
            closed: bytes[275] == 1,
            revoked: bytes[276] == 1,
        };
        let period = u64::from_be_bytes(field(bytes, 242)?);
        let carry_cap = u128::from_be_bytes(field(bytes, 194)?);
        if record.id == [0; 32]
            || record.source == Some([0; 32])
            || record.limit == 0
            || period == 0
            || record.expiry <= record.period_start
            || !matches!(bytes[274], 1 | 2)
            || (bytes[274] == 1 && carry_cap != 0)
        {
            return Err(BudgetRecordError);
        }
        Ok(record)
    }

    /// Returns the allowance available at the authenticated batch timestamp.
    #[must_use]
    pub fn remaining(&self, balance: u128, timestamp_ms: u64, frozen: bool) -> u128 {
        if self.closed
            || self.revoked
            || frozen
            || timestamp_ms >= self.expiry
            || timestamp_ms < self.period_start
        {
            return 0;
        }
        self.limit.saturating_sub(self.spent).min(balance)
    }
}

