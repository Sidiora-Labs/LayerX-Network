#[derive(Debug, Eq, PartialEq)]
pub(super) struct Record {
    pub id: [u8; 32],
    pub owner: [u8; 32],
    pub account: [u8; 32],
    pub asset: [u8; 32],
    pub source: Option<[u8; 32]>,
    pub limit: u128,
    pub spent: u128,
    pub period_start: u64,
    pub expiry: u64,
    pub revocation: u64,
    pub closed: bool,
    pub revoked: bool,
}

fn field<const N: usize>(bytes: &[u8], offset: usize) -> Result<[u8; N], ()> {
    bytes
        .get(offset..offset + N)
        .ok_or(())?
        .try_into()
        .map_err(|_| ())
}

impl Record {
    pub fn decode(bytes: &[u8]) -> Result<Self, ()> {
        if bytes.len() < 278
            || bytes[0] != 0
            || !matches!(bytes[1], 1 | 2)
            || bytes[275] > 1
            || bytes[276] > 1
            || bytes[277] > 16
        {
            return Err(());
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
            return Err(());
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
            return Err(());
        }
        Ok(record)
    }

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

#[cfg(test)]
mod tests {
    use super::Record;

    fn vectors() -> [Vec<u8>; 2] {
        let value: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../tests/vectors/native-budget-records.json"
        )))
        .unwrap_or_else(|error| panic!("native vectors: {error}"));
        ["v1", "v2"].map(|key| {
            crate::hex::decode(
                value[key]
                    .as_str()
                    .unwrap_or_else(|| panic!("{key}"))
                    .strip_prefix("0x")
                    .unwrap_or_else(|| panic!("hex prefix")),
            )
            .unwrap_or_else(|_| panic!("canonical native hex"))
        })
    }

    #[test]
    fn actual_native_versions_bind_source_revocation_and_remaining() {
        for (index, bytes) in vectors().iter().enumerate() {
            let record = Record::decode(bytes).unwrap_or_else(|()| panic!("native record"));
            assert_eq!(record.id, [1; 32]);
            assert_eq!(record.owner, [2; 32]);
            assert_eq!(record.account, [3; 32]);
            assert_eq!(record.asset, [4; 32]);
            assert_eq!(record.source, (index == 1).then_some([8; 32]));
            assert_eq!(record.revocation, 31);
            assert_eq!(record.remaining(500, 1000, false), 77);
            assert_eq!(record.remaining(50, 8999, false), 50);
            assert_eq!(record.remaining(500, 9000, false), 0);
            assert_eq!(record.remaining(500, 999, false), 0);
            assert_eq!(record.remaining(500, 1000, true), 0);
        }
    }

    #[test]
    fn native_record_decoder_refuses_noncanonical_encodings() {
        for bytes in vectors() {
            for end in 0..bytes.len() {
                assert!(Record::decode(&bytes[..end]).is_err(), "truncation {end}");
            }
            let mut trailing = bytes.clone();
            trailing.push(0);
            assert!(Record::decode(&trailing).is_err());
            for (offset, value) in [
                (0, 1),
                (1, 3),
                (274, 0),
                (274, 3),
                (275, 2),
                (276, 2),
                (277, 17),
            ] {
                let mut changed = bytes.clone();
                changed[offset] = value;
                assert!(Record::decode(&changed).is_err(), "field {offset}");
            }
            for (offset, length) in [(2, 32), (162, 16), (242, 8), (258, 8)] {
                let mut changed = bytes.clone();
                changed[offset..offset + length].fill(0);
                assert!(Record::decode(&changed).is_err(), "zero field {offset}");
            }
            let mut duplicate = bytes.clone();
            duplicate.copy_within(278..310, 310);
            assert!(Record::decode(&duplicate).is_err());
            let mut unsorted = bytes.clone();
            unsorted[278..310].fill(9);
            assert!(Record::decode(&unsorted).is_err());
            let mut carry = bytes.clone();
            carry[209] = 1;
            assert!(Record::decode(&carry).is_err());
            if bytes[1] == 2 {
                let mut source = bytes.clone();
                source[342..374].fill(0);
                assert!(Record::decode(&source).is_err());
            }
        }
    }

    #[test]
    fn exhausted_closed_and_revoked_budgets_never_report_spendable_value() {
        let [_, bytes] = vectors();
        let mut record = Record::decode(&bytes).unwrap_or_else(|()| panic!("native record"));
        record.spent = record.limit;
        assert_eq!(record.remaining(u128::MAX, 2000, false), 0);
        record.spent = u128::MAX;
        assert_eq!(record.remaining(u128::MAX, 2000, false), 0);
        record.spent = 0;
        record.closed = true;
        assert_eq!(record.remaining(u128::MAX, 2000, false), 0);
        record.closed = false;
        record.revoked = true;
        assert_eq!(record.remaining(u128::MAX, 2000, false), 0);
    }
}
